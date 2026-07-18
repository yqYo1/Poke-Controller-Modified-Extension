use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use pokecon_settings::service::{PatchClass, RuntimeSettingsApplier};
use serde_json::Value;
use thiserror::Error;
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::sync::{Mutex, broadcast};
use tokio_util::sync::CancellationToken;

use crate::controller::ControllerState;

use super::{ControllerCodec, ControllerFormat, PortSelector, SerialBackend, SerialIo};

/// Validated connection settings applied as one runtime transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialConfig {
    pub selector: PortSelector,
    pub baud_rate: u32,
    pub data_format: ControllerFormat,
}

impl SerialConfig {
    /// Builds a valid serial configuration.
    ///
    /// # Errors
    ///
    /// Rejects an empty selector or zero baud rate.
    pub fn new(
        selector: impl Into<String>,
        baud_rate: u32,
        data_format: ControllerFormat,
    ) -> Result<Self, SerialError> {
        if baud_rate == 0 {
            return Err(SerialError::InvalidBaudRate);
        }
        Ok(Self {
            selector: PortSelector::new(selector).map_err(|_| SerialError::EmptySelector)?,
            baud_rate,
            data_format,
        })
    }
}

/// Automatic reconnect limits required by the runtime contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconnectPolicy {
    pub maximum_attempts: u8,
    pub interval: Duration,
}

impl ReconnectPolicy {
    pub const PRODUCTION: Self = Self {
        maximum_attempts: 20,
        interval: Duration::from_secs(3),
    };
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self::PRODUCTION
    }
}

struct ConnectionState {
    config: Option<SerialConfig>,
    io: Option<Arc<dyn SerialIo>>,
    codec: ControllerCodec,
    connection_id: u64,
}

impl std::fmt::Debug for ConnectionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectionState")
            .field("config", &self.config)
            .field("connected", &self.io.is_some())
            .field("codec", &self.codec)
            .field("connection_id", &self.connection_id)
            .finish()
    }
}

struct ManagerInner {
    backend: Arc<dyn SerialBackend>,
    state: Mutex<ConnectionState>,
    write_gate: Mutex<()>,
    control_gate: Mutex<()>,
    write_epoch: std::sync::Mutex<CancellationToken>,
    explicit_disconnect: AtomicBool,
    reconnecting: AtomicBool,
    next_connection_id: AtomicU64,
    receive_events: broadcast::Sender<Vec<u8>>,
    reconnect_policy: ReconnectPolicy,
}

impl std::fmt::Debug for ManagerInner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagerInner")
            .field("explicit_disconnect", &self.explicit_disconnect)
            .field("reconnecting", &self.reconnecting)
            .field("reconnect_policy", &self.reconnect_policy)
            .finish_non_exhaustive()
    }
}

/// Concurrent serial service. All frames share one write gate, while a receive
/// monitor can read independently from the split native port.
#[derive(Clone, Debug)]
pub struct SerialManager {
    inner: Arc<ManagerInner>,
}

impl SerialManager {
    #[must_use]
    pub fn new(backend: Arc<dyn SerialBackend>) -> Self {
        Self::with_reconnect_policy(backend, ReconnectPolicy::PRODUCTION)
    }

    #[must_use]
    pub fn with_reconnect_policy(
        backend: Arc<dyn SerialBackend>,
        reconnect_policy: ReconnectPolicy,
    ) -> Self {
        let (receive_events, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(ManagerInner {
                backend,
                state: Mutex::new(ConnectionState {
                    config: None,
                    io: None,
                    codec: ControllerCodec::new(ControllerFormat::Default),
                    connection_id: 0,
                }),
                write_gate: Mutex::new(()),
                control_gate: Mutex::new(()),
                write_epoch: std::sync::Mutex::new(CancellationToken::new()),
                explicit_disconnect: AtomicBool::new(false),
                reconnecting: AtomicBool::new(false),
                next_connection_id: AtomicU64::new(1),
                receive_events,
                reconnect_policy,
            }),
        }
    }

    /// Applies port, baud, and format atomically. The old port is neutralized
    /// and closed first; failed replacement reopens the exact previous config.
    ///
    /// # Errors
    ///
    /// Distinguishes a successful rollback from a failed rollback.
    pub async fn apply_config(&self, config: SerialConfig) -> Result<(), SerialError> {
        self.apply_optional_config(Some(config)).await
    }

    /// Updates effective settings without opening a disconnected port. If a
    /// port is connected, the full neutralize/replace/rollback transaction is
    /// used instead.
    ///
    /// # Errors
    ///
    /// Returns a transactional connection error only for a connected port.
    pub async fn update_config(&self, config: SerialConfig) -> Result<(), SerialError> {
        let _control = self.inner.control_gate.lock().await;
        let connected = self.inner.state.lock().await.io.is_some();
        if connected {
            return self.apply_optional_config_locked(Some(config)).await;
        }
        self.cancel_write_epoch();
        let _write = self.inner.write_gate.lock().await;
        let mut state = self.inner.state.lock().await;
        let data_format = config.data_format;
        state.config = Some(config);
        state.codec = ControllerCodec::new(data_format);
        self.replace_write_epoch();
        Ok(())
    }

    /// Clears the configured selection transactionally.
    ///
    /// # Errors
    ///
    /// The current implementation completes best-effort disconnect and returns
    /// success; the result shape is retained for settings-applier symmetry.
    pub async fn clear_config(&self) -> Result<(), SerialError> {
        self.apply_optional_config(None).await
    }

    async fn apply_optional_config(
        &self,
        replacement: Option<SerialConfig>,
    ) -> Result<(), SerialError> {
        let _control = self.inner.control_gate.lock().await;
        self.apply_optional_config_locked(replacement).await
    }

    async fn apply_optional_config_locked(
        &self,
        replacement: Option<SerialConfig>,
    ) -> Result<(), SerialError> {
        self.cancel_write_epoch();
        let _write = self.inner.write_gate.lock().await;

        let (previous_config, previous_io, previous_codec) = {
            let mut state = self.inner.state.lock().await;
            (state.config.clone(), state.io.take(), state.codec.clone())
        };
        if replacement == previous_config && previous_io.is_some() {
            let mut state = self.inner.state.lock().await;
            state.io = previous_io;
            state.codec = previous_codec;
            self.replace_write_epoch();
            return Ok(());
        }

        let mut disconnect_failure = None;
        if let Some(io) = &previous_io {
            let neutral = previous_codec.encode(ControllerState::NEUTRAL);
            if let Err(error) = write_all(io, &neutral).await {
                disconnect_failure = Some(error);
            }
            if let Err(error) = io.close().await {
                disconnect_failure.get_or_insert(error);
            }
        }

        let Some(new_config) = replacement else {
            let mut state = self.inner.state.lock().await;
            state.config = None;
            state.io = None;
            state.codec = ControllerCodec::new(ControllerFormat::Default);
            self.inner
                .explicit_disconnect
                .store(true, Ordering::Release);
            self.replace_write_epoch();
            if disconnect_failure.is_some() {
                tracing::warn!(
                    diagnostic_id = "SERIAL_CLEAR_CLOSE_FAILED",
                    "serial configuration was cleared after best-effort disconnect"
                );
            }
            return Ok(());
        };

        match self.open_initialized(&new_config).await {
            Ok((io, codec)) => {
                let connection_id = self.install_connection(new_config, io.clone(), codec).await;
                self.inner
                    .explicit_disconnect
                    .store(false, Ordering::Release);
                self.replace_write_epoch();
                self.spawn_receive_monitor(io, connection_id);
                Ok(())
            }
            Err(_) => {
                self.rollback_after_failed_replacement(previous_config, previous_codec)
                    .await
            }
        }
    }

    async fn rollback_after_failed_replacement(
        &self,
        previous_config: Option<SerialConfig>,
        previous_codec: ControllerCodec,
    ) -> Result<(), SerialError> {
        let Some(config) = previous_config else {
            let mut state = self.inner.state.lock().await;
            state.config = None;
            state.io = None;
            state.codec = previous_codec;
            self.replace_write_epoch();
            return Err(SerialError::TransactionRolledBack);
        };
        if let Ok((io, codec)) = self.open_initialized(&config).await {
            let connection_id = self.install_connection(config, io.clone(), codec).await;
            self.inner
                .explicit_disconnect
                .store(false, Ordering::Release);
            self.replace_write_epoch();
            self.spawn_receive_monitor(io, connection_id);
            Err(SerialError::TransactionRolledBack)
        } else {
            let mut state = self.inner.state.lock().await;
            state.config = Some(config);
            state.io = None;
            state.codec = previous_codec;
            self.replace_write_epoch();
            Err(SerialError::RollbackFailed)
        }
    }

    async fn open_initialized(
        &self,
        config: &SerialConfig,
    ) -> Result<(Arc<dyn SerialIo>, ControllerCodec), SerialError> {
        let io = self
            .inner
            .backend
            .open(&config.selector, config.baud_rate)
            .await
            .map_err(|_| SerialError::OpenFailed)?;
        let mut codec = ControllerCodec::new(config.data_format);
        let neutral = codec.encode(ControllerState::NEUTRAL);
        if write_all(&io, &neutral).await.is_err() {
            let _ = io.close().await;
            return Err(SerialError::WriteFailed);
        }
        codec.commit(ControllerState::NEUTRAL);
        Ok((io, codec))
    }

    async fn install_connection(
        &self,
        config: SerialConfig,
        io: Arc<dyn SerialIo>,
        codec: ControllerCodec,
    ) -> u64 {
        let connection_id = self.inner.next_connection_id.fetch_add(1, Ordering::AcqRel);
        let mut state = self.inner.state.lock().await;
        state.config = Some(config);
        state.io = Some(io);
        state.codec = codec;
        state.connection_id = connection_id;
        connection_id
    }

    /// Writes one controller frame fully. Partial platform writes are retried,
    /// and codec delta state advances only after the final byte succeeds.
    ///
    /// # Errors
    ///
    /// Queued sends fail immediately after an explicit disconnect or config
    /// transaction cancels their connection epoch.
    pub async fn send_controller_state(
        &self,
        controller: ControllerState,
    ) -> Result<(), SerialError> {
        let epoch = self.write_epoch();
        let write_guard = tokio::select! {
            guard = self.inner.write_gate.lock() => guard,
            () = epoch.cancelled() => return Err(SerialError::SendCancelled),
        };
        if epoch.is_cancelled() {
            return Err(SerialError::SendCancelled);
        }
        let (io, frame, connection_id) = {
            let state = self.inner.state.lock().await;
            let io = state.io.clone().ok_or(SerialError::Disconnected)?;
            (io, state.codec.encode(controller), state.connection_id)
        };
        if write_all(&io, &frame).await.is_err() {
            drop(write_guard);
            self.handle_io_failure(io, connection_id).await;
            return Err(SerialError::WriteFailed);
        }
        self.inner.state.lock().await.codec.commit(controller);
        Ok(())
    }

    /// Writes an opaque serial frame through the same non-interleaving gate.
    ///
    /// # Errors
    ///
    /// Returns a connection, cancellation, or write failure.
    pub async fn send_raw(&self, frame: &[u8]) -> Result<(), SerialError> {
        let epoch = self.write_epoch();
        let write_guard = tokio::select! {
            guard = self.inner.write_gate.lock() => guard,
            () = epoch.cancelled() => return Err(SerialError::SendCancelled),
        };
        if epoch.is_cancelled() {
            return Err(SerialError::SendCancelled);
        }
        let (io, connection_id) = {
            let state = self.inner.state.lock().await;
            (
                state.io.clone().ok_or(SerialError::Disconnected)?,
                state.connection_id,
            )
        };
        if write_all(&io, frame).await.is_err() {
            drop(write_guard);
            self.handle_io_failure(io, connection_id).await;
            return Err(SerialError::WriteFailed);
        }
        Ok(())
    }

    /// Cancels retry and queued sends immediately, then neutralizes and closes
    /// the active connection. The saved selector remains available for a later
    /// explicit reconnect.
    ///
    /// # Errors
    ///
    /// Returns a fixed disconnect failure without leaking selector details.
    pub async fn disconnect(&self) -> Result<(), SerialError> {
        self.inner
            .explicit_disconnect
            .store(true, Ordering::Release);
        self.cancel_write_epoch();
        let _control = self.inner.control_gate.lock().await;
        let _write = self.inner.write_gate.lock().await;
        let (io, neutral) = {
            let mut state = self.inner.state.lock().await;
            let neutral = state.codec.encode(ControllerState::NEUTRAL);
            (state.io.take(), neutral)
        };
        let Some(io) = io else {
            return Ok(());
        };
        let write_result = write_all(&io, &neutral).await;
        let close_result = io.close().await;
        if write_result.is_err() || close_result.is_err() {
            return Err(SerialError::DisconnectFailed);
        }
        Ok(())
    }

    /// Reopens the saved configuration under the bounded retry policy.
    ///
    /// # Errors
    ///
    /// Returns after 20 production attempts or explicit cancellation.
    pub async fn reconnect(&self) -> Result<(), SerialError> {
        if self
            .inner
            .reconnecting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(SerialError::ReconnectInProgress);
        }
        self.inner
            .explicit_disconnect
            .store(false, Ordering::Release);
        let result = self.reconnect_loop().await;
        self.inner.reconnecting.store(false, Ordering::Release);
        result
    }

    async fn reconnect_loop(&self) -> Result<(), SerialError> {
        let _control = self.inner.control_gate.lock().await;
        let _write = self.inner.write_gate.lock().await;
        let config = self
            .inner
            .state
            .lock()
            .await
            .config
            .clone()
            .ok_or(SerialError::Disconnected)?;
        let retry_cancellation = CancellationToken::new();
        {
            let mut epoch = self
                .inner
                .write_epoch
                .lock()
                .expect("serial write epoch mutex poisoned");
            epoch.cancel();
            *epoch = retry_cancellation.clone();
        }
        for attempt in 0..self.inner.reconnect_policy.maximum_attempts {
            if self.inner.explicit_disconnect.load(Ordering::Acquire)
                || retry_cancellation.is_cancelled()
            {
                return Err(SerialError::ReconnectCancelled);
            }
            if attempt > 0 {
                tokio::select! {
                    () = tokio::time::sleep(self.inner.reconnect_policy.interval) => {}
                    () = retry_cancellation.cancelled() => {
                        return Err(SerialError::ReconnectCancelled);
                    }
                }
            }
            if let Ok((io, codec)) = self.open_initialized(&config).await {
                let connection_id = self.install_connection(config, io.clone(), codec).await;
                self.inner
                    .explicit_disconnect
                    .store(false, Ordering::Release);
                self.replace_write_epoch();
                self.spawn_receive_monitor(io, connection_id);
                return Ok(());
            }
        }
        Err(SerialError::ReconnectExhausted)
    }

    /// Subscribes to raw receive chunks. Encoding for WebSocket delivery is a
    /// higher-layer concern and no byte decoding occurs here.
    #[must_use]
    pub fn subscribe_received(&self) -> broadcast::Receiver<Vec<u8>> {
        self.inner.receive_events.subscribe()
    }

    #[must_use]
    pub async fn current_config(&self) -> Option<SerialConfig> {
        self.inner.state.lock().await.config.clone()
    }

    #[must_use]
    pub async fn is_connected(&self) -> bool {
        self.inner.state.lock().await.io.is_some()
    }

    fn spawn_receive_monitor(&self, io: Arc<dyn SerialIo>, connection_id: u64) {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0; 4096];
            loop {
                match io.read(&mut buffer).await {
                    Ok(0) | Err(_) => {
                        manager.handle_io_failure(io, connection_id).await;
                        break;
                    }
                    Ok(count) => {
                        let _ = manager.inner.receive_events.send(buffer[..count].to_vec());
                    }
                }
            }
        });
    }

    async fn handle_io_failure(&self, io: Arc<dyn SerialIo>, connection_id: u64) {
        let active = {
            let mut state = self.inner.state.lock().await;
            if state.connection_id != connection_id
                || state
                    .io
                    .as_ref()
                    .is_none_or(|active| !Arc::ptr_eq(active, &io))
            {
                false
            } else {
                state.io = None;
                true
            }
        };
        if !active {
            return;
        }
        self.cancel_write_epoch();
        let _ = io.close().await;
        if self.inner.explicit_disconnect.load(Ordering::Acquire) {
            return;
        }
        if self
            .inner
            .reconnecting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let manager = self.clone();
            tokio::spawn(async move {
                let _ = manager.reconnect_loop().await;
                manager.inner.reconnecting.store(false, Ordering::Release);
            });
        }
    }

    fn write_epoch(&self) -> CancellationToken {
        self.inner
            .write_epoch
            .lock()
            .expect("serial write epoch mutex poisoned")
            .clone()
    }

    fn cancel_write_epoch(&self) {
        self.inner
            .write_epoch
            .lock()
            .expect("serial write epoch mutex poisoned")
            .cancel();
    }

    fn replace_write_epoch(&self) {
        *self
            .inner
            .write_epoch
            .lock()
            .expect("serial write epoch mutex poisoned") = CancellationToken::new();
    }
}

async fn write_all(io: &Arc<dyn SerialIo>, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let written = io.write(bytes).await?;
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "serial write returned zero",
            ));
        }
        bytes = &bytes[written..];
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct SerialSettingsValues {
    port: String,
    baud_rate: u32,
    data_format: ControllerFormat,
}

impl SerialSettingsValues {
    fn config(&self) -> Result<Option<SerialConfig>, SerialError> {
        if self.port.is_empty() {
            return Ok(None);
        }
        SerialConfig::new(self.port.clone(), self.baud_rate, self.data_format).map(Some)
    }

    fn overlay(&self, changes: &BTreeMap<String, Value>) -> Result<Self, SerialError> {
        let mut next = self.clone();
        if let Some(value) = changes.get("serial.port") {
            value
                .as_str()
                .ok_or(SerialError::InvalidSettingsValue)?
                .clone_into(&mut next.port);
        }
        if let Some(value) = changes.get("serial.baud_rate") {
            next.baud_rate =
                u32::try_from(value.as_u64().ok_or(SerialError::InvalidSettingsValue)?)
                    .map_err(|_| SerialError::InvalidSettingsValue)?;
        }
        if let Some(value) = changes.get("serial.data_format") {
            next.data_format = value
                .as_str()
                .and_then(ControllerFormat::parse)
                .ok_or(SerialError::InvalidSettingsValue)?;
        }
        next.config()?;
        Ok(next)
    }
}

/// Synchronous settings-service bridge backed by the application's existing
/// multithread Tokio runtime. It never creates a second runtime.
#[derive(Debug)]
pub struct SerialSettingsApplier {
    manager: SerialManager,
    runtime: Handle,
    current: SerialSettingsValues,
}

impl SerialSettingsApplier {
    /// Creates a bridge for the already resolved startup serial values.
    ///
    /// # Errors
    ///
    /// Rejects invalid startup settings.
    pub fn new(
        manager: SerialManager,
        runtime: Handle,
        port: String,
        baud_rate: u32,
        data_format: ControllerFormat,
    ) -> Result<Self, SerialError> {
        let current = SerialSettingsValues {
            port,
            baud_rate,
            data_format,
        };
        current.config()?;
        Ok(Self {
            manager,
            runtime,
            current,
        })
    }

    fn run<F>(&self, future: F) -> Result<(), SerialError>
    where
        F: Future<Output = Result<(), SerialError>>,
    {
        if Handle::try_current().is_ok() {
            if self.runtime.runtime_flavor() != RuntimeFlavor::MultiThread {
                return Err(SerialError::SynchronousBridgeUnavailable);
            }
            tokio::task::block_in_place(|| self.runtime.block_on(future))
        } else {
            self.runtime.block_on(future)
        }
    }

    fn apply_values(&self, values: &SerialSettingsValues) -> Result<(), SerialError> {
        match values.config()? {
            Some(config) => self.run(self.manager.update_config(config)),
            None => self.run(self.manager.clear_config()),
        }
    }
}

impl RuntimeSettingsApplier for SerialSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if class != PatchClass::Serial {
            return Ok(());
        }
        let next = self
            .current
            .overlay(changes)
            .map_err(|_| "serial runtime transaction failed".to_owned())?;
        self.apply_values(&next)
            .map_err(|_| "serial runtime transaction failed".to_owned())?;
        self.current = next;
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        if class != PatchClass::Serial {
            return;
        }
        let Ok(restored) = self.current.overlay(previous) else {
            tracing::error!(
                diagnostic_id = "SERIAL_SETTINGS_ROLLBACK_INVALID",
                "serial settings rollback values were invalid"
            );
            return;
        };
        if self.apply_values(&restored).is_ok() {
            self.current = restored;
        } else {
            tracing::error!(
                diagnostic_id = "SERIAL_SETTINGS_ROLLBACK_FAILED",
                "serial settings rollback could not restore the previous connection"
            );
        }
    }
}

/// Fixed serial failures safe to expose without raw selectors or OS text.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SerialError {
    #[error("serial selector must not be empty")]
    EmptySelector,
    #[error("serial baud rate must be positive")]
    InvalidBaudRate,
    #[error("serial setting has an invalid normalized value")]
    InvalidSettingsValue,
    #[error("serial port is disconnected")]
    Disconnected,
    #[error("queued serial send was cancelled")]
    SendCancelled,
    #[error("serial port open failed")]
    OpenFailed,
    #[error("serial frame write failed")]
    WriteFailed,
    #[error("serial disconnect could not complete cleanly")]
    DisconnectFailed,
    #[error("serial replacement failed and the old connection was restored")]
    TransactionRolledBack,
    #[error("serial replacement and rollback reconnection both failed")]
    RollbackFailed,
    #[error("serial reconnect was cancelled by an explicit disconnect")]
    ReconnectCancelled,
    #[error("serial reconnect exhausted its bounded attempts")]
    ReconnectExhausted,
    #[error("serial reconnect is already in progress")]
    ReconnectInProgress,
    #[error("synchronous settings bridge requires the multithread Tokio runtime")]
    SynchronousBridgeUnavailable,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::sync::Arc;
    use std::time::Duration;

    use super::{ReconnectPolicy, SerialConfig, SerialError, SerialManager};
    use crate::controller::{Button, ControllerState};
    use crate::serial::{
        ControllerFormat, SerialSettingsApplier, VirtualOpenPlan, VirtualSerialBackend,
        VirtualSerialEndpoint,
    };
    use pokecon_settings::service::{PatchClass, RuntimeSettingsApplier};
    use serde_json::json;

    fn config(port: &str) -> SerialConfig {
        SerialConfig::new(port, 9600, ControllerFormat::Default).unwrap()
    }

    #[tokio::test]
    async fn partial_writes_complete_and_frames_never_interleave() {
        let backend = VirtualSerialBackend::default();
        let endpoint = VirtualSerialEndpoint::new();
        endpoint.set_maximum_write(2);
        endpoint.set_write_delay(Duration::from_millis(1)).await;
        backend
            .push_plan(VirtualOpenPlan::Success(endpoint.clone()))
            .await;
        let manager = SerialManager::new(Arc::new(backend));
        manager.apply_config(config("raw-port")).await.unwrap();
        let baseline = endpoint.written().await.len();

        let first = manager.clone();
        let second = manager.clone();
        let mut a = ControllerState::NEUTRAL;
        a.buttons.set(Button::A, true);
        let mut b = ControllerState::NEUTRAL;
        b.buttons.set(Button::B, true);
        let (left, right) = tokio::join!(
            first.send_controller_state(a),
            second.send_controller_state(b)
        );
        left.unwrap();
        right.unwrap();
        let bytes = endpoint.written().await;
        let text = std::str::from_utf8(&bytes[baseline..]).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert_eq!(endpoint.maximum_concurrent_writes(), 1);
        assert!(endpoint.write_call_count() > 2);
    }

    #[tokio::test]
    async fn failed_replacement_reopens_exact_old_config() {
        let backend = VirtualSerialBackend::default();
        let old = VirtualSerialEndpoint::new();
        let restored = VirtualSerialEndpoint::new();
        backend
            .push_plan(VirtualOpenPlan::Success(old.clone()))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Fail(io::ErrorKind::PermissionDenied))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Success(restored.clone()))
            .await;
        let manager = SerialManager::new(Arc::new(backend.clone()));
        manager
            .apply_config(config("/dev/by-id/old"))
            .await
            .unwrap();
        assert_eq!(
            manager.apply_config(config("/dev/ttyNEW")).await,
            Err(SerialError::TransactionRolledBack)
        );
        assert_eq!(
            manager.current_config().await.unwrap().selector.as_str(),
            "/dev/by-id/old"
        );
        assert!(manager.is_connected().await);
        assert!(old.is_closed());
        assert!(!restored.written().await.is_empty());
        assert_eq!(
            backend.opened_selectors().await,
            vec![
                ("/dev/by-id/old".to_owned(), 9600),
                ("/dev/ttyNEW".to_owned(), 9600),
                ("/dev/by-id/old".to_owned(), 9600),
            ]
        );
    }

    #[tokio::test]
    async fn rollback_open_failure_preserves_old_config_but_disconnects() {
        let backend = VirtualSerialBackend::default();
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Fail(io::ErrorKind::NotFound))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Fail(io::ErrorKind::NotFound))
            .await;
        let manager = SerialManager::new(Arc::new(backend));
        manager.apply_config(config("old")).await.unwrap();
        assert_eq!(
            manager.apply_config(config("new")).await,
            Err(SerialError::RollbackFailed)
        );
        assert_eq!(
            manager.current_config().await.unwrap().selector.as_str(),
            "old"
        );
        assert!(!manager.is_connected().await);
    }

    #[tokio::test]
    async fn reconnect_is_bounded_to_twenty_attempts() {
        let backend = VirtualSerialBackend::default();
        let endpoint = VirtualSerialEndpoint::new();
        backend.push_plan(VirtualOpenPlan::Success(endpoint)).await;
        for _ in 0..20 {
            backend
                .push_plan(VirtualOpenPlan::Fail(io::ErrorKind::NotFound))
                .await;
        }
        let manager = SerialManager::with_reconnect_policy(
            Arc::new(backend.clone()),
            ReconnectPolicy {
                maximum_attempts: 20,
                interval: Duration::ZERO,
            },
        );
        manager.apply_config(config("port")).await.unwrap();
        manager.inner.state.lock().await.io = None;
        assert_eq!(
            manager.reconnect().await,
            Err(SerialError::ReconnectExhausted)
        );
        assert_eq!(backend.attempt_count(), 21);
    }

    #[tokio::test]
    async fn receive_monitor_forwards_raw_partial_chunks() {
        let backend = VirtualSerialBackend::default();
        let endpoint = VirtualSerialEndpoint::new();
        backend
            .push_plan(VirtualOpenPlan::Success(endpoint.clone()))
            .await;
        let manager = SerialManager::new(Arc::new(backend));
        let mut received = manager.subscribe_received();
        manager.apply_config(config("port")).await.unwrap();
        endpoint.push_read_data([1, 2]).await;
        endpoint.push_read_data([3]).await;
        assert_eq!(received.recv().await.unwrap(), vec![1, 2]);
        assert_eq!(received.recv().await.unwrap(), vec![3]);
        manager.disconnect().await.unwrap();
    }

    #[tokio::test]
    async fn explicit_disconnect_cancels_a_queued_send() {
        let backend = VirtualSerialBackend::default();
        let endpoint = VirtualSerialEndpoint::new();
        endpoint.set_write_delay(Duration::from_millis(30)).await;
        backend.push_plan(VirtualOpenPlan::Success(endpoint)).await;
        let manager = SerialManager::new(Arc::new(backend));
        manager.apply_config(config("port")).await.unwrap();
        let active = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.send_raw(b"active").await })
        };
        tokio::time::sleep(Duration::from_millis(5)).await;
        let queued = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.send_raw(b"queued").await })
        };
        tokio::time::sleep(Duration::from_millis(5)).await;
        let disconnect = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.disconnect().await })
        };
        active.await.unwrap().unwrap();
        assert_eq!(queued.await.unwrap(), Err(SerialError::SendCancelled));
        disconnect.await.unwrap().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn settings_bridge_applies_before_commit_and_keeps_old_values_on_failure() {
        let backend = VirtualSerialBackend::default();
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Fail(io::ErrorKind::NotFound))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        let manager = SerialManager::new(Arc::new(backend));
        manager.apply_config(config("old")).await.unwrap();
        let mut applier = SerialSettingsApplier::new(
            manager.clone(),
            tokio::runtime::Handle::current(),
            "old".to_owned(),
            9600,
            ControllerFormat::Default,
        )
        .unwrap();
        assert!(
            applier
                .apply(
                    PatchClass::Serial,
                    &BTreeMap::from([("serial.port".to_owned(), json!("new"))]),
                )
                .is_err()
        );
        assert_eq!(
            manager.current_config().await.unwrap().selector.as_str(),
            "old"
        );
    }

    #[tokio::test]
    async fn explicit_disconnect_cancels_reconnect_during_interval() {
        let backend = VirtualSerialBackend::default();
        let endpoint = VirtualSerialEndpoint::new();
        backend
            .push_plan(VirtualOpenPlan::Success(endpoint.clone()))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Fail(io::ErrorKind::NotFound))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        let manager = SerialManager::with_reconnect_policy(
            Arc::new(backend.clone()),
            ReconnectPolicy {
                maximum_attempts: 20,
                interval: Duration::from_mins(1),
            },
        );
        manager.apply_config(config("port")).await.unwrap();
        endpoint.push_eof().await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while backend.attempt_count() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        manager.disconnect().await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(backend.attempt_count(), 2);
        assert!(!manager.is_connected().await);
    }

    #[tokio::test]
    async fn disconnected_settings_update_does_not_open_a_port() {
        let backend = VirtualSerialBackend::default();
        let manager = SerialManager::new(Arc::new(backend.clone()));
        manager.update_config(config("saved-only")).await.unwrap();
        assert_eq!(backend.attempt_count(), 0);
        assert!(!manager.is_connected().await);
        assert_eq!(
            manager.current_config().await.unwrap().selector.as_str(),
            "saved-only"
        );
    }

    #[tokio::test]
    async fn same_config_keeps_connection_and_explicit_reconnect_is_allowed() {
        let backend = VirtualSerialBackend::default();
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        let manager = SerialManager::new(Arc::new(backend.clone()));
        manager.apply_config(config("port")).await.unwrap();
        manager.apply_config(config("port")).await.unwrap();
        assert!(manager.is_connected().await);
        assert_eq!(backend.attempt_count(), 1);
        manager.disconnect().await.unwrap();
        manager.reconnect().await.unwrap();
        assert!(manager.is_connected().await);
        assert_eq!(backend.attempt_count(), 2);
    }
}
