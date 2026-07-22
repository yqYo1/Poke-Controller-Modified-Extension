//! Rust-main resource adapters for one profile-scoped user-script generation.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};
use std::net::{Shutdown, TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use parking_lot::Mutex as ParkingMutex;
use pokecon_camera::{CameraConfig, CameraManager, ScreenshotRuntimeSettings};
use pokecon_device::controller::{
    ControllerState, ControllerUpdate, Hat, StickInput, StickPosition, TouchUpdate,
};
use pokecon_device::input::{
    ApplyResult, InputArbiter, InputEvent, InputGeneration, InputPriority, InputSequence,
    InputSnapshot, InputSourceId, InputSourceKind, MouseButtons,
};
use pokecon_device::notification::{NotificationOutcome, NotificationService};
use pokecon_device::serial::{SerialError, SerialManager};
use pokecon_server::api::{LogData, LogLevel, LogTarget};
use pokecon_server::websocket::WebSocketBroker;
use pokecon_settings::pipeline::LoadedSettings;
use pokecon_worker::ipc::ResourceSafety;
use pokecon_worker::script::protocol::{
    HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
    HostTkRequest, HostTkResult, MAX_POPUP_IMAGE_BYTES, ScriptButton, ScriptControl,
    ScriptDialogState, ScriptHat, ScriptInputAction, ScriptOutputMode, ScriptOutputTarget,
    ScriptStick,
};
use pokecon_worker::script::{ScriptHost, ScriptHostError};
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::command_service::CommandBackendError;
use crate::dynamic_host::StartupDynamicHost;
use crate::script_runtime::{ScriptGenerationHostFactory, ScriptGenerationResources};

const SOCKET_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_SOCKET_RESPONSE_BYTES: usize = 64 * 1024;

/// Creates isolated ownership domains over process-wide camera, serial, and
/// controller services.
pub(crate) struct ProductionScriptHostFactory {
    runtime: Handle,
    host: Arc<StartupDynamicHost>,
    arbiter: Arc<ParkingMutex<InputArbiter>>,
    serial: SerialManager,
    camera: CameraManager,
    screenshots: ScreenshotRuntimeSettings,
    notifications: Arc<NotificationService>,
    broker: WebSocketBroker,
    next_generation: AtomicU64,
}

impl std::fmt::Debug for ProductionScriptHostFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductionScriptHostFactory")
            .field("serial", &self.serial)
            .field("camera", &self.camera)
            .finish_non_exhaustive()
    }
}

impl ProductionScriptHostFactory {
    #[must_use]
    pub(crate) fn new(
        runtime: Handle,
        host: Arc<StartupDynamicHost>,
        serial: SerialManager,
        camera: CameraManager,
        screenshots: ScreenshotRuntimeSettings,
        notifications: Arc<NotificationService>,
        broker: WebSocketBroker,
    ) -> Self {
        Self {
            runtime,
            arbiter: host.controller_safety().arbiter(),
            host,
            serial,
            camera,
            screenshots,
            notifications,
            broker,
            next_generation: AtomicU64::new(0),
        }
    }
}

impl ScriptGenerationHostFactory for ProductionScriptHostFactory {
    fn create(
        &self,
        _settings: &LoadedSettings,
    ) -> Result<ScriptGenerationResources, CommandBackendError> {
        let id = self
            .next_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                CommandBackendError::new(
                    "InputGenerationExhausted",
                    "script input generation identifiers are exhausted",
                )
            })?
            .saturating_add(1);
        let source = InputSourceId::new(format!("user-script-{id}"))
            .map_err(|error| script_resource_error(&error))?;
        let generation = InputGeneration::new(format!("user-script-{id}"))
            .map_err(|error| script_resource_error(&error))?;
        initialize_source(&self.arbiter, &source, &generation)
            .map_err(|error| script_resource_error(&error))?;
        let shared = Arc::new(GenerationResources {
            runtime: self.runtime.clone(),
            host: Arc::clone(&self.host),
            arbiter: Arc::clone(&self.arbiter),
            serial: self.serial.clone(),
            source,
            generation,
            sequence: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
            dialogs: Mutex::new(DialogStore::default()),
            tk: Mutex::new(TkStore::default()),
            overlays: Mutex::new(Vec::new()),
        });
        let host = Arc::new(ProductionScriptHost {
            shared: Arc::clone(&shared),
            camera: self.camera.clone(),
            screenshots: self.screenshots.clone(),
            notifications: Arc::clone(&self.notifications),
            broker: self.broker.clone(),
            alternate_panel: AtomicBool::new(false),
            network: Mutex::new(NetworkState::default()),
        });
        let safety: Arc<dyn ResourceSafety> = shared;
        Ok(ScriptGenerationResources { host, safety })
    }
}

struct GenerationResources {
    runtime: Handle,
    host: Arc<StartupDynamicHost>,
    arbiter: Arc<ParkingMutex<InputArbiter>>,
    serial: SerialManager,
    source: InputSourceId,
    generation: InputGeneration,
    sequence: AtomicU64,
    accepting: AtomicBool,
    dialogs: Mutex<DialogStore>,
    tk: Mutex<TkStore>,
    overlays: Mutex<Vec<HostOverlayRequest>>,
}

impl GenerationResources {
    fn ensure_accepting(&self) -> Result<(), ScriptHostError> {
        if self.accepting.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(host_error(
                "WorkerStopping",
                "script generation is stopping",
            ))
        }
    }

    fn next_sequence(&self) -> Result<InputSequence, ScriptHostError> {
        let sequence = self
            .sequence
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| host_error("InputSequenceExhausted", "input sequence is exhausted"))?
            .saturating_add(1);
        InputSequence::new(sequence.to_string())
            .map_err(|_error| host_error("InvalidInput", "controller input is invalid"))
    }

    fn publish_output(&self) -> Result<(), ScriptHostError> {
        let output = self.arbiter.lock().output();
        self.host.notify_controller_change();
        run_sync(&self.runtime, self.serial.send_controller_state(output))
            .or_else(|error| {
                if matches!(
                    error,
                    SerialError::Disconnected | SerialError::SendCancelled
                ) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(serial_host_error)
    }

    fn neutralize(&self) -> Result<(), ScriptHostError> {
        self.ensure_accepting()?;
        initialize_source(&self.arbiter, &self.source, &self.generation)
            .map_err(|_error| host_error("InvalidInput", "controller input is invalid"))?;
        self.sequence.store(0, Ordering::Release);
        self.publish_output()
    }

    fn close_ui(&self) {
        let mut dialogs = self
            .dialogs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for state in dialogs.states.values_mut() {
            if matches!(state, ScriptDialogState::Open) {
                *state = ScriptDialogState::Aborted;
            }
        }
        self.tk
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.overlays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

impl ResourceSafety for GenerationResources {
    fn force_release(&self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        let output = {
            let mut arbiter = self.arbiter.lock();
            arbiter.disconnect_source(&self.source);
            arbiter.output()
        };
        self.close_ui();
        self.host.notify_controller_change();
        let serial = self.serial.clone();
        self.runtime.spawn(async move {
            if let Err(error) = serial.send_controller_state(output).await
                && !matches!(
                    error,
                    SerialError::Disconnected | SerialError::SendCancelled
                )
            {
                tracing::warn!(error = %error, "script resource release could not reach serial");
            }
        });
    }
}

struct ProductionScriptHost {
    shared: Arc<GenerationResources>,
    camera: CameraManager,
    screenshots: ScreenshotRuntimeSettings,
    notifications: Arc<NotificationService>,
    broker: WebSocketBroker,
    alternate_panel: AtomicBool,
    network: Mutex<NetworkState>,
}

impl ScriptHost for ProductionScriptHost {
    fn controller_input(&self, request: HostControllerInputRequest) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        let update = controller_update(request);
        let sequence = self.shared.next_sequence()?;
        let result = self
            .shared
            .arbiter
            .lock()
            .apply_event(
                &self.shared.source,
                &self.shared.generation,
                sequence,
                InputEvent::ControllerUpdate(update),
            )
            .map_err(|_error| host_error("InvalidInput", "controller input is invalid"))?;
        if result != ApplyResult::Applied {
            return Err(host_error(
                "InputGenerationUnavailable",
                "script input generation is unavailable",
            ));
        }
        self.shared.publish_output()
    }

    fn controller_neutral(&self) -> Result<(), ScriptHostError> {
        self.shared.neutralize()
    }

    fn serial_write(&self, data: Vec<u8>) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        run_sync(&self.shared.runtime, self.shared.serial.send_raw(&data))
            .map_err(serial_host_error)
    }

    fn serial_write_row(&self, row: String) -> Result<(), ScriptHostError> {
        let mut row = row.replace(['\r', '\n'], "");
        row.push_str("\r\n");
        self.serial_write(row.into_bytes())
    }

    fn serial_reload(&self) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        if run_sync(&self.shared.runtime, self.shared.serial.is_connected()) {
            run_sync(&self.shared.runtime, self.shared.serial.disconnect())
                .map_err(serial_host_error)?;
        }
        run_sync(&self.shared.runtime, self.shared.serial.reconnect()).map_err(serial_host_error)
    }

    fn output(&self, request: HostOutputRequest) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        let target = match request.target {
            ScriptOutputTarget::Stdout => LogTarget::Stdout,
            ScriptOutputTarget::Panel1 => LogTarget::Panel1,
            ScriptOutputTarget::Panel2 => LogTarget::Panel2,
            ScriptOutputTarget::Alternate => {
                if self.alternate_panel.fetch_xor(true, Ordering::AcqRel) {
                    LogTarget::Panel2
                } else {
                    LogTarget::Panel1
                }
            }
        };
        let message = match request.mode.unwrap_or(ScriptOutputMode::Append) {
            ScriptOutputMode::Write | ScriptOutputMode::Append => request.message,
            ScriptOutputMode::Delete => String::new(),
        };
        self.broker.publish_log(LogData {
            level: LogLevel::Info,
            message,
            target,
        });
        Ok(())
    }

    fn dialog_open(
        &self,
        request: HostDialogOpenRequest,
    ) -> Result<HostDialogOpenResult, ScriptHostError> {
        self.shared.ensure_accepting()?;
        if request.widgets.is_empty() {
            return Err(host_error(
                "InvalidDialog",
                "dialog requires at least one widget",
            ));
        }
        let mut dialogs = self
            .shared
            .dialogs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dialog_id = dialogs
            .next_id
            .checked_add(1)
            .ok_or_else(|| host_error("DialogIdExhausted", "dialog identifiers are exhausted"))?;
        dialogs.next_id = dialog_id;
        dialogs.requests.insert(dialog_id, request);
        dialogs.states.insert(dialog_id, ScriptDialogState::Open);
        Ok(HostDialogOpenResult { dialog_id })
    }

    fn dialog_status(
        &self,
        request: HostDialogStatusRequest,
    ) -> Result<HostDialogStatusResult, ScriptHostError> {
        let dialogs = self
            .shared
            .dialogs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = dialogs
            .states
            .get(&request.dialog_id)
            .cloned()
            .ok_or_else(|| {
                host_error(
                    "DialogNotFound",
                    "dialog does not belong to this generation",
                )
            })?;
        Ok(HostDialogStatusResult { state })
    }

    fn dialog_close_all(&self) -> Result<(), ScriptHostError> {
        self.shared.close_ui();
        Ok(())
    }

    fn network(&self, request: HostNetworkRequest) -> Result<HostNetworkResult, ScriptHostError> {
        self.shared.ensure_accepting()?;
        self.network
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .apply(request)
    }

    fn notification(&self, request: HostNotificationRequest) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        let content = match request {
            HostNotificationRequest::DiscordText { content, .. }
            | HostNotificationRequest::DiscordImage { content, .. } => content,
        };
        let outcome = run_sync(
            &self.shared.runtime,
            self.notifications.send_script_discord(&content),
        );
        match outcome {
            NotificationOutcome::Delivered => Ok(()),
            NotificationOutcome::Disabled
            | NotificationOutcome::SkippedMissingConfiguration
            | NotificationOutcome::SkippedUnsupportedPlatform
            | NotificationOutcome::Failed => Err(host_error(
                "NotificationUnavailable",
                "script notification could not be delivered",
            )),
        }
    }

    fn camera_initialize(&self) -> Result<HostCameraInitializeResult, ScriptHostError> {
        self.shared.ensure_accepting()?;
        Ok(HostCameraInitializeResult {
            mapping: Some(self.camera.mapping_descriptor()),
            state: camera_state(&self.camera, &self.screenshots)?,
        })
    }

    fn camera_control(
        &self,
        request: HostCameraControlRequest,
    ) -> Result<HostCameraState, ScriptHostError> {
        self.shared.ensure_accepting()?;
        let status = self.camera.status();
        match request {
            HostCameraControlRequest::State => {}
            HostCameraControlRequest::SetFps { fps } => {
                let resolution = status
                    .camera_resolution
                    .parse()
                    .map_err(|_error| host_error("CameraUnavailable", "camera state is invalid"))?;
                self.camera
                    .apply_config(
                        CameraConfig::new(status.camera_device, fps, resolution)
                            .map_err(|_error| camera_host_error())?,
                    )
                    .map_err(|_error| camera_host_error())?;
            }
            HostCameraControlRequest::SetFlip { mode } => self.camera.set_flip(mode),
            HostCameraControlRequest::Open { selector } => {
                let resolution = status
                    .camera_resolution
                    .parse()
                    .map_err(|_error| host_error("CameraUnavailable", "camera state is invalid"))?;
                self.camera
                    .apply_config(
                        CameraConfig::new(selector, status.camera_fps, resolution)
                            .map_err(|_error| camera_host_error())?,
                    )
                    .map_err(|_error| camera_host_error())?;
                self.camera.retry().map_err(|_error| camera_host_error())?;
            }
            HostCameraControlRequest::Destroy | HostCameraControlRequest::ThreadStop => {
                self.camera.close().map_err(|_error| camera_host_error())?;
            }
            HostCameraControlRequest::ThreadStart | HostCameraControlRequest::Update => {
                self.camera.retry().map_err(|_error| camera_host_error())?;
            }
        }
        self.shared.host.notify_controller_change();
        camera_state(&self.camera, &self.screenshots)
    }

    fn overlay(&self, request: HostOverlayRequest) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        validate_overlay(&request)?;
        let mut overlays = self
            .shared
            .overlays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(request, HostOverlayRequest::Cleanup) {
            overlays.clear();
        } else {
            overlays.push(request);
        }
        Ok(())
    }

    fn popup_image(&self, request: HostPopupImageRequest) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        if request.encoded.is_empty()
            || request.encoded.len() > MAX_POPUP_IMAGE_BYTES
            || !matches!(request.content_type.as_str(), "image/png" | "image/jpeg")
        {
            return Err(host_error("InvalidPopupImage", "popup image is invalid"));
        }
        self.broker.publish_log(LogData {
            level: LogLevel::Info,
            message: format!(
                "popup image ready: {} ({} bytes)",
                request.title,
                request.encoded.len()
            ),
            target: LogTarget::Log,
        });
        Ok(())
    }

    fn tk(&self, request: HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
        self.shared.ensure_accepting()?;
        self.shared
            .tk
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .apply(&request)
    }
}

#[derive(Default)]
struct DialogStore {
    next_id: u64,
    requests: BTreeMap<u64, HostDialogOpenRequest>,
    states: BTreeMap<u64, ScriptDialogState>,
}

#[derive(Default)]
struct TkStore {
    windows: BTreeSet<u64>,
    widgets: BTreeMap<u64, u64>,
    scales: BTreeMap<u64, f64>,
}

impl TkStore {
    fn apply(&mut self, request: &HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
        match request {
            HostTkRequest::CreateToplevel { window_id } => {
                if !self.windows.insert(*window_id) {
                    return Err(host_error("TkObjectExists", "Tk window already exists"));
                }
            }
            HostTkRequest::SetTitle { window_id, .. }
            | HostTkRequest::SetGeometry { window_id, .. } => self.require_window(*window_id)?,
            HostTkRequest::CreateScale {
                window_id,
                widget_id,
                from_value,
                to_value,
                ..
            } => {
                self.require_window(*window_id)?;
                if !from_value.is_finite() || !to_value.is_finite() || from_value > to_value {
                    return Err(host_error("InvalidTkValue", "Tk scale bounds are invalid"));
                }
                self.insert_widget(*window_id, *widget_id)?;
                self.scales.insert(*widget_id, *from_value);
            }
            HostTkRequest::CreateButton {
                window_id,
                widget_id,
                ..
            }
            | HostTkRequest::CreateLabel {
                window_id,
                widget_id,
                ..
            } => {
                self.require_window(*window_id)?;
                self.insert_widget(*window_id, *widget_id)?;
            }
            HostTkRequest::Pack { widget_id, .. }
            | HostTkRequest::ConfigureLabel { widget_id, .. } => self.require_widget(*widget_id)?,
            HostTkRequest::SetScale { widget_id, value } => {
                if !value.is_finite() || !self.scales.contains_key(widget_id) {
                    return Err(host_error("TkObjectNotFound", "Tk scale does not exist"));
                }
                self.scales.insert(*widget_id, *value);
            }
            HostTkRequest::GetScale { widget_id } => {
                let value = self
                    .scales
                    .get(widget_id)
                    .copied()
                    .ok_or_else(|| host_error("TkObjectNotFound", "Tk scale does not exist"))?;
                return Ok(HostTkResult { value: Some(value) });
            }
            HostTkRequest::DestroyWindow { window_id } => {
                self.require_window(*window_id)?;
                self.windows.remove(window_id);
                self.widgets.retain(|_, owner| owner != window_id);
                self.scales
                    .retain(|widget_id, _| self.widgets.contains_key(widget_id));
            }
            HostTkRequest::Cleanup => self.clear(),
        }
        Ok(HostTkResult { value: None })
    }

    fn insert_widget(&mut self, window_id: u64, widget_id: u64) -> Result<(), ScriptHostError> {
        if self.widgets.insert(widget_id, window_id).is_some() {
            Err(host_error("TkObjectExists", "Tk widget already exists"))
        } else {
            Ok(())
        }
    }

    fn require_window(&self, window_id: u64) -> Result<(), ScriptHostError> {
        if self.windows.contains(&window_id) {
            Ok(())
        } else {
            Err(host_error("TkObjectNotFound", "Tk window does not exist"))
        }
    }

    fn require_widget(&self, widget_id: u64) -> Result<(), ScriptHostError> {
        if self.widgets.contains_key(&widget_id) {
            Ok(())
        } else {
            Err(host_error("TkObjectNotFound", "Tk widget does not exist"))
        }
    }

    fn clear(&mut self) {
        self.windows.clear();
        self.widgets.clear();
        self.scales.clear();
    }
}

struct NetworkState {
    socket_address: String,
    socket_port: u16,
    socket_alive: bool,
    socket: Option<TcpStream>,
    mqtt_broker_address: String,
    mqtt_id: String,
    mqtt_client_id: String,
    mqtt_publish_token: String,
    mqtt_subscribe_token: String,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            socket_address: "127.0.0.1".to_owned(),
            socket_port: 0,
            socket_alive: false,
            socket: None,
            mqtt_broker_address: String::new(),
            mqtt_id: String::new(),
            mqtt_client_id: String::new(),
            mqtt_publish_token: String::new(),
            mqtt_subscribe_token: String::new(),
        }
    }
}

impl NetworkState {
    fn apply(&mut self, request: HostNetworkRequest) -> Result<HostNetworkResult, ScriptHostError> {
        match request {
            HostNetworkRequest::Cleanup | HostNetworkRequest::SocketDisconnect => {
                self.disconnect();
            }
            HostNetworkRequest::SocketConnect => self.connect()?,
            HostNetworkRequest::SocketTransmit { message } => {
                let socket = self.socket.as_mut().ok_or_else(network_unavailable)?;
                socket
                    .write_all(message.as_bytes())
                    .map_err(|_error| network_unavailable())?;
            }
            HostNetworkRequest::SocketReceive {
                headers,
                show_message,
            } => {
                let socket = self.socket.as_mut().ok_or_else(network_unavailable)?;
                let mut buffer = vec![0; MAX_SOCKET_RESPONSE_BYTES];
                let count = socket
                    .read(&mut buffer)
                    .map_err(|_error| network_unavailable())?;
                let message = String::from_utf8_lossy(&buffer[..count]).into_owned();
                let matches_header =
                    headers.is_empty() || headers.iter().any(|header| message.starts_with(header));
                return Ok(HostNetworkResult {
                    message: (show_message && matches_header).then_some(message),
                });
            }
            HostNetworkRequest::SocketChangeAddress { address } => {
                if address.trim().is_empty() {
                    return Err(host_error(
                        "InvalidNetworkAddress",
                        "socket address is empty",
                    ));
                }
                self.socket_address = address;
            }
            HostNetworkRequest::SocketChangePort { port } => self.socket_port = port,
            HostNetworkRequest::SocketChangeAlive { alive } => self.socket_alive = alive,
            HostNetworkRequest::MqttChangeBrokerAddress { broker_address } => {
                self.mqtt_broker_address = broker_address;
            }
            HostNetworkRequest::MqttChangeId { mqtt_id } => self.mqtt_id = mqtt_id,
            HostNetworkRequest::MqttChangeClientId { client_id } => self.mqtt_client_id = client_id,
            HostNetworkRequest::MqttChangePublishToken { token } => {
                self.mqtt_publish_token = token;
            }
            HostNetworkRequest::MqttChangeSubscribeToken { token } => {
                self.mqtt_subscribe_token = token;
            }
            HostNetworkRequest::MqttTransmit { .. } | HostNetworkRequest::MqttReceive { .. } => {
                return Err(host_error(
                    "MqttUnavailable",
                    "MQTT compatibility transport is unavailable",
                ));
            }
        }
        Ok(HostNetworkResult { message: None })
    }

    fn connect(&mut self) -> Result<(), ScriptHostError> {
        if self.socket.is_some() {
            return Ok(());
        }
        if self.socket_port == 0 {
            return Err(host_error(
                "InvalidNetworkPort",
                "socket port is not configured",
            ));
        }
        let address = (self.socket_address.as_str(), self.socket_port)
            .to_socket_addrs()
            .map_err(|_error| network_unavailable())?
            .next()
            .ok_or_else(network_unavailable)?;
        let stream = TcpStream::connect_timeout(&address, SOCKET_TIMEOUT)
            .map_err(|_error| network_unavailable())?;
        stream
            .set_read_timeout(Some(SOCKET_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(SOCKET_TIMEOUT)))
            .map_err(|_error| network_unavailable())?;
        stream
            .set_nodelay(true)
            .map_err(|_error| network_unavailable())?;
        self.socket = Some(stream);
        Ok(())
    }

    fn disconnect(&mut self) {
        if let Some(socket) = self.socket.take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

fn initialize_source(
    arbiter: &ParkingMutex<InputArbiter>,
    source: &InputSourceId,
    generation: &InputGeneration,
) -> Result<(), pokecon_device::input::InputError> {
    let mut arbiter = arbiter.lock();
    arbiter.begin_generation(
        source.clone(),
        InputSourceKind::UserScript,
        InputPriority::USER_SCRIPT,
        generation.clone(),
    );
    let neutral = ControllerState::NEUTRAL;
    let (result, acknowledgement) = arbiter.apply_snapshot(
        source,
        InputSnapshot {
            generation: generation.clone(),
            sequence: InputSequence::zero(),
            keyboard_keys: Vec::new(),
            mouse_buttons: MouseButtons::default(),
            buttons: neutral.buttons,
            hat: neutral.hat,
            left_stick: neutral.left_stick,
            right_stick: neutral.right_stick,
            touch: neutral.touch,
        },
    )?;
    if result == ApplyResult::Applied && acknowledgement.is_some() {
        Ok(())
    } else {
        Err(pokecon_device::input::InputError::UnknownSource)
    }
}

fn controller_update(request: HostControllerInputRequest) -> ControllerUpdate {
    let pressed = request.action == ScriptInputAction::Press;
    let mut update = ControllerUpdate::default();
    if request.unset_hat {
        update.hat = Some(Hat::Neutral);
    }
    if request.unset_touchscreen {
        update.touch = Some(TouchUpdate {
            x: 0,
            y: 0,
            pressed: false,
        });
    }
    for control in request.controls {
        match control {
            ScriptControl::Button { button } => set_button(&mut update, button, pressed),
            ScriptControl::Hat { direction } => {
                update.hat = Some(if pressed {
                    script_hat(direction)
                } else {
                    Hat::Neutral
                });
            }
            ScriptControl::Stick { stick, x, y } => {
                let position = if pressed {
                    StickPosition { x, y }
                } else {
                    StickPosition::CENTER
                };
                match stick {
                    ScriptStick::Left => update.left_stick = Some(StickInput::Xy(position)),
                    ScriptStick::Right => update.right_stick = Some(StickInput::Xy(position)),
                }
            }
            ScriptControl::Touchscreen { x, y } => {
                update.touch = Some(TouchUpdate { x, y, pressed });
            }
        }
    }
    update
}

fn set_button(update: &mut ControllerUpdate, button: ScriptButton, pressed: bool) {
    let field = match button {
        ScriptButton::A => &mut update.a,
        ScriptButton::B => &mut update.b,
        ScriptButton::X => &mut update.x,
        ScriptButton::Y => &mut update.y,
        ScriptButton::L => &mut update.l,
        ScriptButton::R => &mut update.r,
        ScriptButton::Zl => &mut update.zl,
        ScriptButton::Zr => &mut update.zr,
        ScriptButton::Minus => &mut update.minus,
        ScriptButton::Plus => &mut update.plus,
        ScriptButton::Lclick => &mut update.lclick,
        ScriptButton::Rclick => &mut update.rclick,
        ScriptButton::Home => &mut update.home,
        ScriptButton::Capture => &mut update.capture,
    };
    *field = Some(pressed);
}

const fn script_hat(hat: ScriptHat) -> Hat {
    match hat {
        ScriptHat::Top => Hat::Up,
        ScriptHat::TopRight => Hat::UpRight,
        ScriptHat::Right => Hat::Right,
        ScriptHat::BottomRight => Hat::DownRight,
        ScriptHat::Bottom => Hat::Down,
        ScriptHat::BottomLeft => Hat::DownLeft,
        ScriptHat::Left => Hat::Left,
        ScriptHat::TopLeft => Hat::UpLeft,
        ScriptHat::Center => Hat::Neutral,
    }
}

fn camera_state(
    camera: &CameraManager,
    screenshots: &ScreenshotRuntimeSettings,
) -> Result<HostCameraState, ScriptHostError> {
    let status = camera.status();
    Ok(HostCameraState {
        opened: status.camera_opened,
        fps: status.camera_fps,
        capture_resolution: status
            .camera_resolution
            .parse()
            .map_err(|_error| host_error("CameraUnavailable", "camera state is invalid"))?,
        flip_mode: camera.flip(),
        screenshot_format: screenshots.format(),
    })
}

fn validate_overlay(request: &HostOverlayRequest) -> Result<(), ScriptHostError> {
    match request {
        HostOverlayRequest::Rectangle { outline, tag, .. }
        | HostOverlayRequest::Text {
            color: outline,
            tag,
            ..
        } if outline.is_empty() || tag.is_empty() => Err(host_error(
            "InvalidOverlay",
            "overlay style or tag is empty",
        )),
        HostOverlayRequest::SetFps { fps } if *fps == 0 => {
            Err(host_error("InvalidOverlay", "overlay FPS must be positive"))
        }
        HostOverlayRequest::SetShowSize { height, width } if *height == 0 || *width == 0 => Err(
            host_error("InvalidOverlay", "overlay size must be positive"),
        ),
        HostOverlayRequest::SetTouchscreenArea {
            left,
            top,
            right,
            bottom,
        } if ![left, top, right, bottom]
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            || left >= right
            || top >= bottom =>
        {
            Err(host_error("InvalidOverlay", "touchscreen area is invalid"))
        }
        _ => Ok(()),
    }
}

fn run_sync<F>(runtime: &Handle, future: F) -> F::Output
where
    F: std::future::Future,
{
    if Handle::try_current().is_ok() && runtime.runtime_flavor() == RuntimeFlavor::MultiThread {
        tokio::task::block_in_place(|| runtime.block_on(future))
    } else {
        runtime.block_on(future)
    }
}

fn host_error(code: &str, message: &str) -> ScriptHostError {
    ScriptHostError::new(code, message)
}

fn serial_host_error(_error: SerialError) -> ScriptHostError {
    host_error("SerialUnavailable", "serial operation failed")
}

fn camera_host_error() -> ScriptHostError {
    host_error("CameraUnavailable", "camera operation failed")
}

fn network_unavailable() -> ScriptHostError {
    host_error("NetworkUnavailable", "network operation failed")
}

fn script_resource_error(error: &impl std::fmt::Display) -> CommandBackendError {
    CommandBackendError::new("ScriptResourceUnavailable", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_controller_release_centers_continuous_controls() {
        let update = controller_update(HostControllerInputRequest {
            action: ScriptInputAction::Release,
            controls: vec![
                ScriptControl::Button {
                    button: ScriptButton::A,
                },
                ScriptControl::Hat {
                    direction: ScriptHat::Top,
                },
                ScriptControl::Stick {
                    stick: ScriptStick::Left,
                    x: 255,
                    y: 0,
                },
                ScriptControl::Touchscreen { x: 12, y: 34 },
            ],
            unset_hat: false,
            unset_touchscreen: false,
        });
        assert_eq!(update.a, Some(false));
        assert_eq!(update.hat, Some(Hat::Neutral));
        assert_eq!(
            update.left_stick,
            Some(StickInput::Xy(StickPosition::CENTER))
        );
        assert_eq!(
            update.touch,
            Some(TouchUpdate {
                x: 12,
                y: 34,
                pressed: false,
            })
        );
    }
}
