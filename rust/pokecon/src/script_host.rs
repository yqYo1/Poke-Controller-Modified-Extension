//! Rust-main resource adapters for one profile-scoped user-script generation.

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read as _, Write as _};
use std::net::{Shutdown, TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use crate::camera::{CameraConfig, CameraManager, ScreenshotRuntimeSettings};
use crate::device::{
    ApplyResult, InputArbiter, InputEvent, InputGeneration, InputPriority, InputSequence,
    InputSnapshot, InputSourceId, InputSourceKind, MouseButtons,
};
use crate::device::{
    ControllerState, ControllerUpdate, Hat, StickInput, StickPosition, TouchUpdate,
};
use crate::device::{NotificationOutcome, NotificationService};
use crate::device::{SerialError, SerialManager};
use crate::settings::pipeline::LoadedSettings;
use base64::Engine as _;
use chrono::{Local, Timelike as _};
use parking_lot::Mutex as ParkingMutex;
use pokecon_server::api::{self as wire, LogData, LogLevel, LogOperation, LogTarget};
use pokecon_server::websocket::WebSocketBroker;
use pokecon_worker::ipc::ResourceSafety;
use pokecon_worker::script::protocol::{
    HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
    HostTkRequest, HostTkResult, MAX_NOTIFICATION_IMAGE_BYTES, MAX_POPUP_IMAGE_BYTES, ScriptButton,
    ScriptControl, ScriptDialogState, ScriptDialogValue, ScriptDialogWidget,
    ScriptDialogWidgetKind, ScriptHat, ScriptInputAction, ScriptOutputMode, ScriptOutputTarget,
    ScriptPointerButton, ScriptPointerEvent, ScriptPointerPhase, ScriptStick, ScriptTkEvent,
};
use pokecon_worker::script::{ScriptHost, ScriptHostError};
use rumqttc::{Client as MqttClient, Event as MqttEvent, MqttOptions, Outgoing, Packet, QoS};
use tokio::runtime::{Handle, RuntimeFlavor};
use url::Url;

use crate::command_service::CommandBackendError;
use crate::dynamic_host::StartupDynamicHost;
use crate::script_runtime::{ScriptGenerationHostFactory, ScriptGenerationResources};

const SOCKET_TIMEOUT: Duration = Duration::from_secs(2);
const NETWORK_POLL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_SOCKET_RESPONSE_BYTES: usize = 64 * 1024;
const MQTT_PORT: u16 = 1883;
const MQTT_PACKET_BYTES: usize = 64 * 1024;

/// Process-wide bridge between the active profile-scoped script generation
/// and the typed browser transport.
#[derive(Clone, Default)]
pub(crate) struct ScriptUiCoordinator {
    inner: Arc<ScriptUiCoordinatorInner>,
}

#[derive(Default)]
struct ScriptUiCoordinatorInner {
    active: Mutex<Option<ActiveScriptUi>>,
    broker: OnceLock<WebSocketBroker>,
}

struct ActiveScriptUi {
    generation: String,
    id: u64,
    resources: Weak<GenerationResources>,
}

pub(crate) struct ScriptUiActionOutcome {
    pub(crate) abort_command: bool,
    pub(crate) pointer_event: Option<ScriptPointerEvent>,
    pub(crate) tk_event: Option<ScriptTkEvent>,
}

impl ScriptUiCoordinator {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn install_broker(&self, broker: WebSocketBroker) -> Result<(), String> {
        self.inner
            .broker
            .set(broker)
            .map_err(|_broker| "script UI broker was already installed".to_owned())?;
        if let Some(resources) = self.active_resources() {
            self.publish(&resources);
        }
        Ok(())
    }

    fn broker(&self) -> Option<WebSocketBroker> {
        self.inner.broker.get().cloned()
    }

    fn activate(&self, resources: &Arc<GenerationResources>) {
        let generation = resources.generation.as_str().to_owned();
        *self
            .inner
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ActiveScriptUi {
            generation,
            id: resources.id,
            resources: Arc::downgrade(resources),
        });
        self.publish(resources);
    }

    fn deactivate(&self, generation: u64) {
        let removed = {
            let mut active = self
                .inner
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if active
                .as_ref()
                .is_some_and(|active| active.id == generation)
            {
                active.take();
                true
            } else {
                false
            }
        };
        if removed && let Some(broker) = self.broker() {
            broker.publish_script_ui(wire::ScriptUiSnapshot::default());
        }
    }

    fn active_resources(&self) -> Option<Arc<GenerationResources>> {
        self.inner
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|active| active.resources.upgrade())
    }

    fn publish(&self, resources: &GenerationResources) {
        let generation = {
            let active = self
                .inner
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(active) = active.as_ref().filter(|active| active.id == resources.id) else {
                return;
            };
            active.generation.clone()
        };
        let snapshot = resources.ui_snapshot(generation);
        if let Some(broker) = self.broker() {
            broker.publish_script_ui(snapshot);
        }
    }

    pub(crate) fn apply_action(
        &self,
        request: wire::ScriptUiAction,
    ) -> Result<ScriptUiActionOutcome, ScriptHostError> {
        let requested_generation = action_generation(&request);
        let resources = {
            let active = self
                .inner
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let active = active
                .as_ref()
                .filter(|active| active.generation == requested_generation)
                .ok_or_else(stale_script_ui)?;
            active.resources.upgrade().ok_or_else(stale_script_ui)?
        };
        let outcome = resources.apply_ui_action(request)?;
        self.publish(&resources);
        Ok(outcome)
    }
}

fn action_generation(action: &wire::ScriptUiAction) -> &str {
    match action {
        wire::ScriptUiAction::DialogConfirm { generation, .. }
        | wire::ScriptUiAction::DialogAbort { generation, .. }
        | wire::ScriptUiAction::TkScaleChanged { generation, .. }
        | wire::ScriptUiAction::TkButtonInvoked { generation, .. }
        | wire::ScriptUiAction::TkWindowClosed { generation, .. }
        | wire::ScriptUiAction::PopupClosed { generation, .. }
        | wire::ScriptUiAction::Pointer { generation, .. } => generation,
    }
}

fn stale_script_ui() -> ScriptHostError {
    host_error(
        "StaleScriptUiGeneration",
        "script UI generation is no longer active",
    )
}

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
    script_ui: ScriptUiCoordinator,
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
        script_ui: ScriptUiCoordinator,
    ) -> Self {
        Self {
            runtime,
            arbiter: host.controller_safety().arbiter(),
            host,
            serial,
            camera,
            screenshots,
            notifications,
            script_ui,
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
            id,
            runtime: self.runtime.clone(),
            host: Arc::clone(&self.host),
            arbiter: Arc::clone(&self.arbiter),
            serial: self.serial.clone(),
            source,
            generation,
            sequence: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
            ui: Mutex::new(GenerationUiState::default()),
            script_ui: self.script_ui.clone(),
        });
        self.script_ui.activate(&shared);
        let host = Arc::new(ProductionScriptHost {
            shared: Arc::clone(&shared),
            camera: self.camera.clone(),
            screenshots: self.screenshots.clone(),
            notifications: Arc::clone(&self.notifications),
            broker: self.script_ui.broker().ok_or_else(|| {
                CommandBackendError::new(
                    "ScriptUiUnavailable",
                    "script UI transport is unavailable",
                )
            })?,
            alternate_panel: AtomicBool::new(false),
            network: Mutex::new(NetworkState::default()),
        });
        let safety: Arc<dyn ResourceSafety> = shared;
        Ok(ScriptGenerationResources { host, safety })
    }
}

struct GenerationResources {
    id: u64,
    runtime: Handle,
    host: Arc<StartupDynamicHost>,
    arbiter: Arc<ParkingMutex<InputArbiter>>,
    serial: SerialManager,
    source: InputSourceId,
    generation: InputGeneration,
    sequence: AtomicU64,
    accepting: AtomicBool,
    ui: Mutex<GenerationUiState>,
    script_ui: ScriptUiCoordinator,
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

    fn clear_ui(&self) {
        let mut ui = self
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for state in ui.dialogs.states.values_mut() {
            if matches!(state, ScriptDialogState::Open) {
                *state = ScriptDialogState::Aborted;
            }
        }
        ui.tk.clear();
        ui.overlay.clear();
        ui.popups.clear();
        drop(ui);
        self.script_ui.publish(self);
    }

    fn deactivate_ui(&self) {
        let mut ui = self
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for state in ui.dialogs.states.values_mut() {
            if matches!(state, ScriptDialogState::Open) {
                *state = ScriptDialogState::Aborted;
            }
        }
        ui.tk.clear();
        ui.overlay.clear();
        ui.popups.clear();
        drop(ui);
        self.script_ui.deactivate(self.id);
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
        self.deactivate_ui();
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
        let (message, operation) = match request.mode.unwrap_or(ScriptOutputMode::Append) {
            ScriptOutputMode::Write => (request.message, LogOperation::Replace),
            ScriptOutputMode::Append => (request.message, LogOperation::Append),
            ScriptOutputMode::Delete => (String::new(), LogOperation::Clear),
        };
        self.broker.publish_log(LogData {
            level: LogLevel::Info,
            message,
            target,
            operation,
        });
        Ok(())
    }

    fn dialog_open(
        &self,
        request: HostDialogOpenRequest,
    ) -> Result<HostDialogOpenResult, ScriptHostError> {
        self.shared.ensure_accepting()?;
        validate_dialog_open_request(&request)?;
        let mut ui = self
            .shared
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dialog_id =
            ui.dialogs.next_id.checked_add(1).ok_or_else(|| {
                host_error("DialogIdExhausted", "dialog identifiers are exhausted")
            })?;
        ui.dialogs.next_id = dialog_id;
        ui.dialogs.requests.insert(dialog_id, request);
        ui.dialogs.states.insert(dialog_id, ScriptDialogState::Open);
        drop(ui);
        self.shared.script_ui.publish(&self.shared);
        Ok(HostDialogOpenResult { dialog_id })
    }

    fn dialog_status(
        &self,
        request: HostDialogStatusRequest,
    ) -> Result<HostDialogStatusResult, ScriptHostError> {
        let ui = self
            .shared
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = ui
            .dialogs
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
        self.shared.clear_ui();
        Ok(())
    }

    fn network(&self, request: HostNetworkRequest) -> Result<HostNetworkResult, ScriptHostError> {
        self.shared.ensure_accepting()?;
        self.network
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .apply(request, &self.shared.accepting)
    }

    fn notification(&self, request: HostNotificationRequest) -> Result<(), ScriptHostError> {
        self.shared.ensure_accepting()?;
        let outcome = match request {
            HostNotificationRequest::DiscordText { content, .. } => run_sync(
                &self.shared.runtime,
                self.notifications.send_script_discord(&content),
            ),
            HostNotificationRequest::DiscordImage {
                content,
                content_type,
                encoded,
                ..
            } => {
                if content_type != "image/jpeg"
                    || encoded.is_empty()
                    || encoded.len() > MAX_NOTIFICATION_IMAGE_BYTES
                {
                    return Err(host_error(
                        "InvalidNotificationImage",
                        "notification image is invalid",
                    ));
                }
                run_sync(
                    &self.shared.runtime,
                    self.notifications
                        .send_script_discord_image(&content, &content_type, &encoded),
                )
            }
        };
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
        let expiry = self
            .shared
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .overlay
            .apply(request)?;
        self.shared.script_ui.publish(&self.shared);
        if let Some(expiry) = expiry {
            let shared = Arc::clone(&self.shared);
            self.shared.runtime.spawn(async move {
                tokio::time::sleep(expiry.duration).await;
                shared.expire_overlay(expiry.id);
            });
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
        self.shared
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .popups
            .insert(request)?;
        self.shared.script_ui.publish(&self.shared);
        Ok(())
    }

    fn tk(&self, request: HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
        self.shared.ensure_accepting()?;
        let result = self
            .shared
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tk
            .apply(&request);
        self.shared.script_ui.publish(&self.shared);
        result
    }
}

#[derive(Default)]
struct DialogStore {
    next_id: u64,
    requests: BTreeMap<u64, HostDialogOpenRequest>,
    states: BTreeMap<u64, ScriptDialogState>,
}

#[derive(Default)]
struct GenerationUiState {
    dialogs: DialogStore,
    tk: TkStore,
    overlay: OverlayStore,
    popups: PopupStore,
}

#[derive(Default)]
struct TkStore {
    windows: BTreeMap<u64, TkWindowState>,
    widgets: BTreeMap<u64, TkWidgetState>,
}

#[derive(Default)]
struct TkWindowState {
    title: String,
    geometry: Option<String>,
    widget_order: Vec<u64>,
}

enum TkWidgetState {
    Scale {
        owner: u64,
        from_value: f64,
        to_value: f64,
        orient: String,
        label: Option<String>,
        value: f64,
        pady: Option<i64>,
    },
    Button {
        owner: u64,
        text: String,
        pady: Option<i64>,
    },
    Label {
        owner: u64,
        text: String,
        width: Option<i64>,
        height: Option<i64>,
        relief: Option<String>,
        background: Option<String>,
        pady: Option<i64>,
    },
}

impl TkStore {
    fn apply(&mut self, request: &HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
        match request {
            HostTkRequest::CreateToplevel { window_id } => {
                if self
                    .windows
                    .insert(*window_id, TkWindowState::default())
                    .is_some()
                {
                    return Err(host_error("TkObjectExists", "Tk window already exists"));
                }
            }
            HostTkRequest::SetTitle { window_id, title } => {
                self.window_mut(*window_id)?.title.clone_from(title);
            }
            HostTkRequest::SetGeometry {
                window_id,
                geometry,
            } => {
                self.window_mut(*window_id)?.geometry = Some(geometry.clone());
            }
            HostTkRequest::CreateScale {
                window_id,
                widget_id,
                from_value,
                to_value,
                orient,
                label,
            } => self.create_scale(
                *window_id,
                *widget_id,
                *from_value,
                *to_value,
                orient,
                label.as_deref(),
            )?,
            HostTkRequest::CreateButton {
                window_id,
                widget_id,
                text,
            } => self.insert_widget(
                *window_id,
                *widget_id,
                TkWidgetState::Button {
                    owner: *window_id,
                    text: text.clone(),
                    pady: None,
                },
            )?,
            HostTkRequest::CreateLabel {
                window_id,
                widget_id,
                text,
                width,
                height,
                relief,
                background,
            } => self.insert_widget(
                *window_id,
                *widget_id,
                TkWidgetState::Label {
                    owner: *window_id,
                    text: text.clone(),
                    width: *width,
                    height: *height,
                    relief: relief.clone(),
                    background: background.clone(),
                    pady: None,
                },
            )?,
            HostTkRequest::Pack { widget_id, pady } => {
                self.require_widget_mut(*widget_id)?.set_pady(*pady);
            }
            HostTkRequest::ConfigureLabel {
                widget_id,
                text,
                background,
            } => self.configure_label(*widget_id, text.as_deref(), background.as_deref())?,
            HostTkRequest::SetScale { widget_id, value } => {
                self.set_scale(*widget_id, *value)?;
            }
            HostTkRequest::GetScale { widget_id } => {
                let TkWidgetState::Scale { value, .. } = self.require_widget(*widget_id)? else {
                    return Err(host_error("TkObjectNotFound", "Tk scale does not exist"));
                };
                return Ok(HostTkResult {
                    value: Some(*value),
                });
            }
            HostTkRequest::DestroyWindow { window_id } => {
                self.require_window(*window_id)?;
                self.remove_window(*window_id);
            }
            HostTkRequest::Cleanup => self.clear(),
        }
        Ok(HostTkResult { value: None })
    }

    fn create_scale(
        &mut self,
        window_id: u64,
        widget_id: u64,
        from_value: f64,
        to_value: f64,
        orient: &str,
        label: Option<&str>,
    ) -> Result<(), ScriptHostError> {
        self.require_window(window_id)?;
        if !from_value.is_finite() || !to_value.is_finite() || from_value > to_value {
            return Err(host_error("InvalidTkValue", "Tk scale bounds are invalid"));
        }
        self.insert_widget(
            window_id,
            widget_id,
            TkWidgetState::Scale {
                owner: window_id,
                from_value,
                to_value,
                orient: orient.to_owned(),
                label: label.map(str::to_owned),
                value: from_value,
                pady: None,
            },
        )
    }

    fn configure_label(
        &mut self,
        widget_id: u64,
        text: Option<&str>,
        background: Option<&str>,
    ) -> Result<(), ScriptHostError> {
        let TkWidgetState::Label {
            text: current_text,
            background: current_background,
            ..
        } = self.require_widget_mut(widget_id)?
        else {
            return Err(host_error("TkObjectNotFound", "Tk label does not exist"));
        };
        if let Some(text) = text {
            text.clone_into(current_text);
        }
        if let Some(background) = background {
            *current_background = Some(background.to_owned());
        }
        Ok(())
    }

    fn set_scale(&mut self, widget_id: u64, value: f64) -> Result<(), ScriptHostError> {
        let TkWidgetState::Scale {
            from_value,
            to_value,
            value: current,
            ..
        } = self.require_widget_mut(widget_id)?
        else {
            return Err(host_error("TkObjectNotFound", "Tk scale does not exist"));
        };
        if !value.is_finite() || value < *from_value || value > *to_value {
            return Err(host_error("InvalidTkValue", "Tk scale value is invalid"));
        }
        *current = value;
        Ok(())
    }

    fn insert_widget(
        &mut self,
        window_id: u64,
        widget_id: u64,
        widget: TkWidgetState,
    ) -> Result<(), ScriptHostError> {
        self.require_window(window_id)?;
        if self.widgets.contains_key(&widget_id) {
            return Err(host_error("TkObjectExists", "Tk widget already exists"));
        }
        self.widgets.insert(widget_id, widget);
        self.window_mut(window_id)?.widget_order.push(widget_id);
        Ok(())
    }

    fn require_window(&self, window_id: u64) -> Result<(), ScriptHostError> {
        if self.windows.contains_key(&window_id) {
            Ok(())
        } else {
            Err(host_error("TkObjectNotFound", "Tk window does not exist"))
        }
    }

    fn window_mut(&mut self, window_id: u64) -> Result<&mut TkWindowState, ScriptHostError> {
        self.windows
            .get_mut(&window_id)
            .ok_or_else(|| host_error("TkObjectNotFound", "Tk window does not exist"))
    }

    fn require_widget(&self, widget_id: u64) -> Result<&TkWidgetState, ScriptHostError> {
        self.widgets
            .get(&widget_id)
            .ok_or_else(|| host_error("TkObjectNotFound", "Tk widget does not exist"))
    }

    fn require_widget_mut(
        &mut self,
        widget_id: u64,
    ) -> Result<&mut TkWidgetState, ScriptHostError> {
        self.widgets
            .get_mut(&widget_id)
            .ok_or_else(|| host_error("TkObjectNotFound", "Tk widget does not exist"))
    }

    fn scale_changed(&mut self, widget_id: u64, value: f64) -> Result<(), ScriptHostError> {
        self.apply(&HostTkRequest::SetScale { widget_id, value })
            .map(|_result| ())
    }

    fn button_exists(&self, widget_id: u64) -> Result<(), ScriptHostError> {
        if matches!(
            self.require_widget(widget_id)?,
            TkWidgetState::Button { .. }
        ) {
            Ok(())
        } else {
            Err(host_error("TkObjectNotFound", "Tk button does not exist"))
        }
    }

    fn remove_window(&mut self, window_id: u64) {
        self.windows.remove(&window_id);
        self.widgets.retain(|_, widget| widget.owner() != window_id);
    }

    fn clear(&mut self) {
        self.windows.clear();
        self.widgets.clear();
    }

    fn snapshot(&self) -> Vec<wire::ScriptTkWindow> {
        self.windows
            .iter()
            .map(|(window_id, window)| wire::ScriptTkWindow {
                id: wire::DecimalString::from_u64(*window_id),
                title: window.title.clone(),
                geometry: window.geometry.clone(),
                widgets: window
                    .widget_order
                    .iter()
                    .filter_map(|widget_id| {
                        self.widgets
                            .get(widget_id)
                            .map(|widget| widget.snapshot(*widget_id))
                    })
                    .collect(),
            })
            .collect()
    }
}

impl TkWidgetState {
    const fn owner(&self) -> u64 {
        match self {
            Self::Scale { owner, .. } | Self::Button { owner, .. } | Self::Label { owner, .. } => {
                *owner
            }
        }
    }

    fn set_pady(&mut self, value: Option<i64>) {
        match self {
            Self::Scale { pady, .. } | Self::Button { pady, .. } | Self::Label { pady, .. } => {
                *pady = value;
            }
        }
    }

    fn snapshot(&self, id: u64) -> wire::ScriptTkWidget {
        let id = wire::DecimalString::from_u64(id);
        match self {
            Self::Scale {
                from_value,
                to_value,
                orient,
                label,
                value,
                pady,
                ..
            } => wire::ScriptTkWidget::Scale(wire::ScriptTkScale {
                id,
                from_value: *from_value,
                to_value: *to_value,
                orient: orient.clone(),
                label: label.clone(),
                value: *value,
                pady: *pady,
            }),
            Self::Button { text, pady, .. } => wire::ScriptTkWidget::Button(wire::ScriptTkButton {
                id,
                text: text.clone(),
                pady: *pady,
            }),
            Self::Label {
                text,
                width,
                height,
                relief,
                background,
                pady,
                ..
            } => wire::ScriptTkWidget::Label(wire::ScriptTkLabel {
                id,
                text: text.clone(),
                width: *width,
                height: *height,
                relief: relief.clone(),
                background: background.clone(),
                pady: *pady,
            }),
        }
    }
}

#[derive(Clone)]
enum OverlayShapeState {
    Rectangle(wire::ScriptOverlayRectangle),
    Text(wire::ScriptOverlayText),
}

struct OverlayEntry {
    id: u64,
    shape: OverlayShapeState,
}

struct OverlayExpiry {
    duration: Duration,
    id: u64,
}

struct OverlayStore {
    next_id: u64,
    fps: u32,
    show_width: u32,
    show_height: u32,
    right_mouse_mode: String,
    touchscreen_area: wire::NormalizedRegion,
    bindings: wire::ScriptPointerBindings,
    shapes: Vec<OverlayEntry>,
}

impl Default for OverlayStore {
    fn default() -> Self {
        Self {
            next_id: 0,
            fps: 30,
            show_width: 1280,
            show_height: 720,
            right_mouse_mode: "Default".to_owned(),
            touchscreen_area: wire::NormalizedRegion {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            bindings: wire::ScriptPointerBindings::default(),
            shapes: Vec::new(),
        }
    }
}

impl OverlayStore {
    fn apply(
        &mut self,
        request: HostOverlayRequest,
    ) -> Result<Option<OverlayExpiry>, ScriptHostError> {
        let (shape, expires_ms) = match request {
            HostOverlayRequest::Rectangle {
                x1,
                y1,
                x2,
                y2,
                outline,
                tag,
                expires_ms,
            } => (
                Some(OverlayShapeState::Rectangle(wire::ScriptOverlayRectangle {
                    tag,
                    x1,
                    y1,
                    x2,
                    y2,
                    outline,
                })),
                expires_ms,
            ),
            HostOverlayRequest::Text {
                x,
                y,
                text,
                tag,
                expires_ms,
                font,
                font_size,
                color,
            } => (
                Some(OverlayShapeState::Text(wire::ScriptOverlayText {
                    tag,
                    x,
                    y,
                    text,
                    font,
                    font_size,
                    color,
                })),
                expires_ms,
            ),
            HostOverlayRequest::DeleteRectangle { tag } => {
                self.shapes.retain(|entry| {
                    !matches!(&entry.shape, OverlayShapeState::Rectangle(shape) if shape.tag == tag)
                });
                (None, None)
            }
            HostOverlayRequest::DeleteText { tag } => {
                self.shapes.retain(|entry| {
                    !matches!(&entry.shape, OverlayShapeState::Text(shape) if shape.tag == tag)
                });
                (None, None)
            }
            HostOverlayRequest::SetFps { fps } => {
                self.fps = fps;
                (None, None)
            }
            HostOverlayRequest::SetShowSize { height, width } => {
                self.show_height = height;
                self.show_width = width;
                (None, None)
            }
            HostOverlayRequest::SetRightMouseMode { mode } => {
                self.right_mouse_mode = mode;
                (None, None)
            }
            HostOverlayRequest::SetTouchscreenArea {
                left,
                top,
                right,
                bottom,
            } => {
                self.touchscreen_area = wire::NormalizedRegion {
                    x: left,
                    y: top,
                    width: right - left,
                    height: bottom - top,
                };
                (None, None)
            }
            HostOverlayRequest::SetBinding { button, enabled } => {
                match button {
                    ScriptPointerButton::Left => self.bindings.left = enabled,
                    ScriptPointerButton::Right => self.bindings.right = enabled,
                }
                (None, None)
            }
            HostOverlayRequest::Update => (None, None),
            HostOverlayRequest::Cleanup => {
                self.clear();
                (None, None)
            }
        };
        let Some(shape) = shape else {
            return Ok(None);
        };
        self.insert(shape, expires_ms)
    }

    fn insert(
        &mut self,
        shape: OverlayShapeState,
        expires_ms: Option<u64>,
    ) -> Result<Option<OverlayExpiry>, ScriptHostError> {
        let id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| host_error("OverlayIdExhausted", "overlay identifiers are exhausted"))?;
        self.next_id = id;
        self.shapes.push(OverlayEntry { id, shape });
        Ok(expires_ms.map(|expires_ms| OverlayExpiry {
            duration: Duration::from_millis(expires_ms),
            id,
        }))
    }

    fn expire(&mut self, id: u64) -> bool {
        let before = self.shapes.len();
        self.shapes.retain(|entry| entry.id != id);
        self.shapes.len() != before
    }

    fn clear(&mut self) {
        let next_id = self.next_id;
        *self = Self::default();
        self.next_id = next_id;
    }

    fn snapshot(&self) -> wire::ScriptOverlaySnapshot {
        wire::ScriptOverlaySnapshot {
            fps: self.fps,
            show_width: self.show_width,
            show_height: self.show_height,
            right_mouse_mode: self.right_mouse_mode.clone(),
            touchscreen_area: self.touchscreen_area,
            bindings: self.bindings,
            shapes: self
                .shapes
                .iter()
                .map(|entry| match &entry.shape {
                    OverlayShapeState::Rectangle(shape) => {
                        wire::ScriptOverlayShape::Rectangle(shape.clone())
                    }
                    OverlayShapeState::Text(shape) => wire::ScriptOverlayShape::Text(shape.clone()),
                })
                .collect(),
        }
    }

    fn pointer_event(
        &self,
        button: wire::ScriptPointerButton,
        phase: wire::ScriptPointerPhase,
        x: u32,
        y: u32,
    ) -> Result<ScriptPointerEvent, ScriptHostError> {
        let enabled = match button {
            wire::ScriptPointerButton::Left => self.bindings.left,
            wire::ScriptPointerButton::Right => self.bindings.right,
        };
        if !enabled || x >= self.show_width || y >= self.show_height {
            return Err(host_error(
                "InvalidScriptUiAction",
                "pointer event is outside the active overlay binding",
            ));
        }
        Ok(ScriptPointerEvent {
            button: match button {
                wire::ScriptPointerButton::Left => ScriptPointerButton::Left,
                wire::ScriptPointerButton::Right => ScriptPointerButton::Right,
            },
            phase: match phase {
                wire::ScriptPointerPhase::Pressed => ScriptPointerPhase::Pressed,
                wire::ScriptPointerPhase::Moved => ScriptPointerPhase::Moved,
                wire::ScriptPointerPhase::Released => ScriptPointerPhase::Released,
            },
            x,
            y,
        })
    }
}

#[derive(Default)]
struct PopupStore {
    next_id: u64,
    values: BTreeMap<u64, wire::ScriptPopupImage>,
}

impl PopupStore {
    fn insert(&mut self, request: HostPopupImageRequest) -> Result<(), ScriptHostError> {
        let id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| host_error("PopupIdExhausted", "popup identifiers are exhausted"))?;
        self.next_id = id;
        self.values.insert(
            id,
            wire::ScriptPopupImage {
                id: wire::DecimalString::from_u64(id),
                title: request.title,
                content_type: request.content_type,
                encoded_base64: base64::engine::general_purpose::STANDARD.encode(request.encoded),
            },
        );
        Ok(())
    }

    fn remove(&mut self, id: u64) -> Result<(), ScriptHostError> {
        self.values
            .remove(&id)
            .map(|_value| ())
            .ok_or_else(|| host_error("ScriptUiObjectNotFound", "popup image does not exist"))
    }

    fn clear(&mut self) {
        self.values.clear();
    }
}

impl GenerationResources {
    fn ui_snapshot(&self, generation: String) -> wire::ScriptUiSnapshot {
        let ui = self
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dialogs = ui
            .dialogs
            .states
            .iter()
            .filter_map(|(dialog_id, state)| {
                matches!(state, ScriptDialogState::Open)
                    .then(|| ui.dialogs.requests.get(dialog_id))
                    .flatten()
                    .map(|request| wire::ScriptDialog {
                        id: wire::DecimalString::from_u64(*dialog_id),
                        title: request.title.clone(),
                        description: request.description.clone(),
                        widgets: request.widgets.iter().map(wire_dialog_widget).collect(),
                    })
            })
            .collect();
        wire::ScriptUiSnapshot {
            generation: Some(generation),
            dialogs,
            tk_windows: ui.tk.snapshot(),
            overlay: ui.overlay.snapshot(),
            popup_images: ui.popups.values.values().cloned().collect(),
        }
    }

    fn apply_ui_action(
        &self,
        action: wire::ScriptUiAction,
    ) -> Result<ScriptUiActionOutcome, ScriptHostError> {
        self.ensure_accepting()?;
        let mut ui = self
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut abort_command = false;
        let mut pointer_event = None;
        let mut tk_event = None;
        match action {
            wire::ScriptUiAction::DialogConfirm {
                dialog_id, values, ..
            } => {
                let dialog_id = decimal_id(&dialog_id)?;
                let request = ui
                    .dialogs
                    .requests
                    .get(&dialog_id)
                    .ok_or_else(script_ui_object_not_found)?;
                if !matches!(
                    ui.dialogs.states.get(&dialog_id),
                    Some(ScriptDialogState::Open)
                ) {
                    return Err(script_ui_object_not_found());
                }
                let values = validated_dialog_values(&request.widgets, values)?;
                ui.dialogs
                    .states
                    .insert(dialog_id, ScriptDialogState::Confirmed { values });
            }
            wire::ScriptUiAction::DialogAbort { dialog_id, .. } => {
                let dialog_id = decimal_id(&dialog_id)?;
                let state = ui
                    .dialogs
                    .states
                    .get_mut(&dialog_id)
                    .filter(|state| matches!(state, ScriptDialogState::Open))
                    .ok_or_else(script_ui_object_not_found)?;
                *state = ScriptDialogState::Aborted;
                abort_command = true;
            }
            wire::ScriptUiAction::TkScaleChanged {
                widget_id, value, ..
            } => {
                let widget_id = decimal_id(&widget_id)?;
                ui.tk.scale_changed(widget_id, value)?;
                tk_event = Some(ScriptTkEvent::ScaleChanged { widget_id, value });
            }
            wire::ScriptUiAction::TkButtonInvoked { widget_id, .. } => {
                let widget_id = decimal_id(&widget_id)?;
                ui.tk.button_exists(widget_id)?;
                tk_event = Some(ScriptTkEvent::ButtonInvoked { widget_id });
            }
            wire::ScriptUiAction::TkWindowClosed { window_id, .. } => {
                let window_id = decimal_id(&window_id)?;
                ui.tk.require_window(window_id)?;
                ui.tk.remove_window(window_id);
                tk_event = Some(ScriptTkEvent::WindowClosed { window_id });
            }
            wire::ScriptUiAction::PopupClosed { popup_id, .. } => {
                ui.popups.remove(decimal_id(&popup_id)?)?;
            }
            wire::ScriptUiAction::Pointer {
                button,
                phase,
                x,
                y,
                ..
            } => {
                pointer_event = Some(ui.overlay.pointer_event(button, phase, x, y)?);
            }
        }
        Ok(ScriptUiActionOutcome {
            abort_command,
            pointer_event,
            tk_event,
        })
    }

    fn expire_overlay(&self, id: u64) {
        let changed = self
            .ui
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .overlay
            .expire(id);
        if changed {
            self.script_ui.publish(self);
        }
    }
}

fn decimal_id(id: &wire::DecimalString) -> Result<u64, ScriptHostError> {
    id.as_str()
        .parse()
        .map_err(|_error| host_error("InvalidScriptUiAction", "script UI identifier is invalid"))
}

fn script_ui_object_not_found() -> ScriptHostError {
    host_error(
        "ScriptUiObjectNotFound",
        "script UI object is no longer available",
    )
}

fn wire_dialog_widget(widget: &ScriptDialogWidget) -> wire::ScriptDialogWidget {
    wire::ScriptDialogWidget {
        kind: match widget.kind {
            ScriptDialogWidgetKind::Entry => wire::ScriptDialogWidgetKind::Entry,
            ScriptDialogWidgetKind::Check => wire::ScriptDialogWidgetKind::Check,
            ScriptDialogWidgetKind::Combo => wire::ScriptDialogWidgetKind::Combo,
            ScriptDialogWidgetKind::Radio => wire::ScriptDialogWidgetKind::Radio,
            ScriptDialogWidgetKind::Spin => wire::ScriptDialogWidgetKind::Spin,
            ScriptDialogWidgetKind::Scale => wire::ScriptDialogWidgetKind::Scale,
            ScriptDialogWidgetKind::Next => wire::ScriptDialogWidgetKind::Next,
        },
        label: widget.label.clone(),
        value: wire_dialog_value(&widget.value),
        options: widget.options.iter().map(wire_dialog_value).collect(),
        minimum: widget.minimum,
        maximum: widget.maximum,
        precision: widget.precision,
    }
}

fn wire_dialog_value(value: &ScriptDialogValue) -> wire::ScriptDialogValue {
    match value {
        ScriptDialogValue::None => wire::ScriptDialogValue::None,
        ScriptDialogValue::String(value) => wire::ScriptDialogValue::String(value.clone()),
        ScriptDialogValue::Bool(value) => wire::ScriptDialogValue::Bool(*value),
        ScriptDialogValue::Integer(value) => wire::ScriptDialogValue::Integer(*value),
        ScriptDialogValue::Float(value) => wire::ScriptDialogValue::Float(*value),
    }
}

fn host_dialog_value(value: wire::ScriptDialogValue) -> ScriptDialogValue {
    match value {
        wire::ScriptDialogValue::None => ScriptDialogValue::None,
        wire::ScriptDialogValue::String(value) => ScriptDialogValue::String(value),
        wire::ScriptDialogValue::Bool(value) => ScriptDialogValue::Bool(value),
        wire::ScriptDialogValue::Integer(value) => ScriptDialogValue::Integer(value),
        wire::ScriptDialogValue::Float(value) => ScriptDialogValue::Float(value),
    }
}

fn validated_dialog_values(
    widgets: &[ScriptDialogWidget],
    values: Vec<wire::ScriptDialogValue>,
) -> Result<Vec<ScriptDialogValue>, ScriptHostError> {
    if values.len() != widgets.len() {
        return Err(host_error(
            "InvalidScriptUiAction",
            "dialog result count does not match its widgets",
        ));
    }
    widgets
        .iter()
        .zip(values)
        .map(|(widget, value)| {
            let value = host_dialog_value(value);
            validate_dialog_value(widget, &value)?;
            Ok(value)
        })
        .collect()
}

// Dialog numeric bounds and browser number inputs are represented as f64;
// conversion here intentionally applies that wire-level comparison semantics.
#[allow(clippy::cast_precision_loss)]
fn validate_dialog_value(
    widget: &ScriptDialogWidget,
    value: &ScriptDialogValue,
) -> Result<(), ScriptHostError> {
    let type_matches = matches!(
        (&widget.kind, value),
        (ScriptDialogWidgetKind::Entry, ScriptDialogValue::String(_))
            | (ScriptDialogWidgetKind::Check, ScriptDialogValue::Bool(_))
            | (
                ScriptDialogWidgetKind::Combo | ScriptDialogWidgetKind::Radio,
                _
            )
            | (ScriptDialogWidgetKind::Spin, ScriptDialogValue::Integer(_))
            | (ScriptDialogWidgetKind::Scale, ScriptDialogValue::Float(_))
            | (ScriptDialogWidgetKind::Next, ScriptDialogValue::None)
    );
    let finite = !matches!(value, ScriptDialogValue::Float(value) if !value.is_finite());
    let option_matches = !matches!(
        widget.kind,
        ScriptDialogWidgetKind::Combo | ScriptDialogWidgetKind::Radio
    ) || widget.options.contains(value);
    let numeric = match value {
        ScriptDialogValue::Integer(value) => Some(*value as f64),
        ScriptDialogValue::Float(value) => Some(*value),
        _ => None,
    };
    let in_range = numeric.is_none_or(|value| {
        widget.minimum.is_none_or(|minimum| value >= minimum)
            && widget.maximum.is_none_or(|maximum| value <= maximum)
    });
    if type_matches && finite && option_matches && in_range {
        Ok(())
    } else {
        Err(host_error(
            "InvalidScriptUiAction",
            "dialog result does not satisfy its widget contract",
        ))
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
            socket_alive: true,
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
    fn apply(
        &mut self,
        request: HostNetworkRequest,
        accepting: &AtomicBool,
    ) -> Result<HostNetworkResult, ScriptHostError> {
        match request {
            HostNetworkRequest::Cleanup => {
                self.socket_alive = false;
                self.disconnect();
            }
            HostNetworkRequest::SocketDisconnect => {
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
                return Ok(HostNetworkResult {
                    message: self.socket_receive(&headers, show_message, accepting)?,
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
            HostNetworkRequest::MqttTransmit { room_id, message } => {
                self.mqtt_transmit(&room_id, &message)?;
            }
            HostNetworkRequest::MqttReceive {
                room_id,
                headers,
                show_message,
            } => {
                return Ok(HostNetworkResult {
                    message: self.mqtt_receive(&room_id, &headers, show_message, accepting)?,
                });
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
            .set_read_timeout(Some(NETWORK_POLL_INTERVAL))
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

    fn socket_receive(
        &mut self,
        headers: &[String],
        show_message: bool,
        accepting: &AtomicBool,
    ) -> Result<Option<String>, ScriptHostError> {
        while self.socket_alive && accepting.load(Ordering::Acquire) {
            let mut buffer = vec![0; MAX_SOCKET_RESPONSE_BYTES];
            let read = self
                .socket
                .as_mut()
                .ok_or_else(network_unavailable)?
                .read(&mut buffer);
            let count = match read {
                Ok(0) => {
                    self.disconnect();
                    return Ok(None);
                }
                Ok(count) => count,
                Err(error)
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    continue;
                }
                Err(_error) => return Err(network_unavailable()),
            };
            let message = String::from_utf8_lossy(&buffer[..count]).into_owned();
            let matches_header =
                headers.is_empty() || headers.iter().any(|header| message.starts_with(header));
            if matches_header {
                tracing::info!(target: "pokecon.script.network", "socket message matched");
                return Ok(Some(message));
            }
            if show_message {
                tracing::info!(
                    target: "pokecon.script.network",
                    message,
                    "socket message did not match the requested header"
                );
            }
        }
        if !self.socket_alive {
            self.disconnect();
        }
        Ok(None)
    }

    fn mqtt_transmit(&self, room_id: &str, message: &str) -> Result<(), ScriptHostError> {
        validate_mqtt_topic(room_id)?;
        let options = self.mqtt_options(&self.mqtt_publish_token)?;
        let (client, mut connection) = MqttClient::new(options, 4);
        connection
            .eventloop
            .network_options
            .set_connection_timeout(SOCKET_TIMEOUT.as_secs());
        client
            .publish(
                room_id,
                QoS::AtMostOnce,
                false,
                mqtt_timestamped_message(message),
            )
            .map_err(|_error| mqtt_unavailable())?;
        let deadline = Instant::now() + SOCKET_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(mqtt_unavailable());
            }
            match connection.recv_timeout(remaining) {
                Ok(Ok(MqttEvent::Outgoing(Outgoing::Publish(_)))) => return Ok(()),
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => return Err(mqtt_unavailable()),
            }
        }
    }

    fn mqtt_receive(
        &self,
        room_id: &str,
        headers: &[String],
        show_message: bool,
        accepting: &AtomicBool,
    ) -> Result<Option<String>, ScriptHostError> {
        validate_mqtt_topic(room_id)?;
        let options = self.mqtt_options(&self.mqtt_subscribe_token)?;
        let (client, mut connection) = MqttClient::new(options, 4);
        connection
            .eventloop
            .network_options
            .set_connection_timeout(SOCKET_TIMEOUT.as_secs());
        client
            .subscribe(room_id, QoS::AtMostOnce)
            .map_err(|_error| mqtt_unavailable())?;
        let mut newest_timestamp = mqtt_timestamp();
        while accepting.load(Ordering::Acquire) {
            match connection.recv_timeout(NETWORK_POLL_INTERVAL) {
                Ok(Ok(MqttEvent::Incoming(Packet::Publish(publish)))) => {
                    let Ok(payload) = std::str::from_utf8(&publish.payload) else {
                        continue;
                    };
                    let Some((timestamp, message)) = split_mqtt_message(payload) else {
                        continue;
                    };
                    if timestamp <= newest_timestamp.as_str() {
                        continue;
                    }
                    timestamp.clone_into(&mut newest_timestamp);
                    let matches_header = headers.is_empty()
                        || headers.iter().any(|header| message.starts_with(header));
                    if matches_header {
                        tracing::info!(target: "pokecon.script.network", "MQTT message matched");
                        return Ok(Some(message.to_owned()));
                    }
                    if show_message {
                        tracing::info!(
                            target: "pokecon.script.network",
                            message,
                            "MQTT message did not match the requested header"
                        );
                    }
                }
                Ok(Ok(_)) | Err(rumqttc::RecvTimeoutError::Timeout) => {}
                Ok(Err(_)) | Err(rumqttc::RecvTimeoutError::Disconnected) => {
                    return Err(mqtt_unavailable());
                }
            }
        }
        Ok(None)
    }

    fn mqtt_options(&self, token: &str) -> Result<MqttOptions, ScriptHostError> {
        let (broker, port) = parse_mqtt_broker(&self.mqtt_broker_address)?;
        if self.mqtt_id.trim().is_empty()
            || self.mqtt_client_id.trim().is_empty()
            || token.trim().is_empty()
        {
            return Err(invalid_mqtt_configuration());
        }
        let mut options = MqttOptions::new(&self.mqtt_client_id, broker, port);
        options
            .set_keep_alive(Duration::from_secs(5))
            .set_credentials(&self.mqtt_id, token)
            .set_max_packet_size(MQTT_PACKET_BYTES, MQTT_PACKET_BYTES);
        Ok(options)
    }
}

fn parse_mqtt_broker(raw: &str) -> Result<(String, u16), ScriptHostError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_mqtt_configuration());
    }
    let candidate = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("mqtt://{raw}")
    };
    let url = Url::parse(&candidate).map_err(|_error| invalid_mqtt_configuration())?;
    if !matches!(url.scheme(), "mqtt" | "tcp")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(invalid_mqtt_configuration());
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(invalid_mqtt_configuration)?;
    Ok((host.to_owned(), url.port().unwrap_or(MQTT_PORT)))
}

fn validate_mqtt_topic(topic: &str) -> Result<(), ScriptHostError> {
    if topic.is_empty()
        || topic
            .chars()
            .any(|character| matches!(character, '\0' | '#' | '+'))
    {
        Err(invalid_mqtt_configuration())
    } else {
        Ok(())
    }
}

fn mqtt_timestamp() -> String {
    let now = Local::now();
    format!(
        "{}{:06}",
        now.format("%Y%m%d%H%M%S"),
        now.nanosecond() / 1_000
    )
}

fn mqtt_timestamped_message(message: &str) -> String {
    format!("[{}]{message}", mqtt_timestamp())
}

fn split_mqtt_message(payload: &str) -> Option<(&str, &str)> {
    let bytes = payload.as_bytes();
    if bytes.len() < 22
        || bytes[0] != b'['
        || bytes[21] != b']'
        || !bytes[1..21].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    Some((&payload[1..21], &payload[22..]))
}

fn initialize_source(
    arbiter: &ParkingMutex<InputArbiter>,
    source: &InputSourceId,
    generation: &InputGeneration,
) -> Result<(), crate::device::InputError> {
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
        Err(crate::device::InputError::UnknownSource)
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

fn validate_dialog_open_request(request: &HostDialogOpenRequest) -> Result<(), ScriptHostError> {
    if request.widgets.is_empty() && request.description.is_none() {
        return Err(host_error(
            "InvalidDialog",
            "dialog requires at least one widget or a message",
        ));
    }
    Ok(())
}

fn network_unavailable() -> ScriptHostError {
    host_error("NetworkUnavailable", "network operation failed")
}

fn invalid_mqtt_configuration() -> ScriptHostError {
    host_error(
        "InvalidMqttConfiguration",
        "MQTT configuration is incomplete or invalid",
    )
}

fn mqtt_unavailable() -> ScriptHostError {
    host_error("MqttUnavailable", "MQTT operation failed")
}

fn script_resource_error(error: &impl std::fmt::Display) -> CommandBackendError {
    CommandBackendError::new("ScriptResourceUnavailable", error.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    #[test]
    fn mqtt_legacy_envelope_is_exact_and_secret_free() {
        let encoded = mqtt_timestamped_message("ready");
        let (timestamp, message) = split_mqtt_message(&encoded).expect("envelope is valid");
        assert_eq!(timestamp.len(), 20);
        assert!(timestamp.bytes().all(|byte| byte.is_ascii_digit()));
        assert_eq!(message, "ready");
        assert!(split_mqtt_message("ready").is_none());
        assert!(split_mqtt_message("[2026072212345600000x]ready").is_none());
    }

    #[test]
    fn mqtt_broker_parser_accepts_legacy_hosts_and_explicit_ports() {
        assert_eq!(
            parse_mqtt_broker("broker.example").unwrap(),
            ("broker.example".to_owned(), MQTT_PORT)
        );
        assert_eq!(
            parse_mqtt_broker("mqtt://127.0.0.1:2883").unwrap(),
            ("127.0.0.1".to_owned(), 2883)
        );
        assert!(parse_mqtt_broker("https://broker.example").is_err());
        assert!(parse_mqtt_broker("mqtt://user:secret@broker.example").is_err());
    }

    #[test]
    fn socket_matching_is_independent_of_diagnostic_logging() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test listener binds");
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("test client connects");
            stream
                .write_all(b"ready:payload")
                .expect("test payload is written");
        });
        let accepting = AtomicBool::new(true);
        let mut state = NetworkState {
            socket_port: port,
            ..NetworkState::default()
        };
        state
            .apply(HostNetworkRequest::SocketConnect, &accepting)
            .expect("socket connects");
        let result = state
            .apply(
                HostNetworkRequest::SocketReceive {
                    headers: vec!["ready".to_owned()],
                    show_message: false,
                },
                &accepting,
            )
            .expect("socket receive succeeds");
        assert_eq!(result.message.as_deref(), Some("ready:payload"));
        server.join().expect("test server exits");
    }

    #[test]
    fn overlay_pointer_events_require_binding_and_bounded_coordinates() {
        let mut overlay = OverlayStore::default();
        assert!(
            overlay
                .pointer_event(
                    wire::ScriptPointerButton::Left,
                    wire::ScriptPointerPhase::Pressed,
                    12,
                    34,
                )
                .is_err()
        );
        overlay
            .apply(HostOverlayRequest::SetBinding {
                button: ScriptPointerButton::Left,
                enabled: true,
            })
            .unwrap();
        assert_eq!(
            overlay
                .pointer_event(
                    wire::ScriptPointerButton::Left,
                    wire::ScriptPointerPhase::Moved,
                    1279,
                    719,
                )
                .unwrap(),
            ScriptPointerEvent {
                button: ScriptPointerButton::Left,
                phase: ScriptPointerPhase::Moved,
                x: 1279,
                y: 719,
            }
        );
        assert!(
            overlay
                .pointer_event(
                    wire::ScriptPointerButton::Left,
                    wire::ScriptPointerPhase::Released,
                    1280,
                    719,
                )
                .is_err()
        );
    }

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

    #[test]
    fn tk_store_reconstructs_windows_and_validates_scale_updates() {
        let mut store = TkStore::default();
        store
            .apply(&HostTkRequest::CreateToplevel { window_id: 2 })
            .expect("window");
        store
            .apply(&HostTkRequest::SetTitle {
                window_id: 2,
                title: "Detector".to_owned(),
            })
            .expect("title");
        store
            .apply(&HostTkRequest::CreateScale {
                window_id: 2,
                widget_id: 5,
                from_value: 0.0,
                to_value: 255.0,
                orient: "horizontal".to_owned(),
                label: Some("Hue".to_owned()),
            })
            .expect("scale");
        store
            .apply(&HostTkRequest::SetScale {
                widget_id: 5,
                value: 42.0,
            })
            .expect("scale value");

        let snapshot = store.snapshot();
        assert_eq!(snapshot[0].title, "Detector");
        let wire::ScriptTkWidget::Scale(scale) = &snapshot[0].widgets[0] else {
            panic!("scale snapshot");
        };
        assert!((scale.value - 42.0).abs() < f64::EPSILON);
        assert!(
            store
                .apply(&HostTkRequest::SetScale {
                    widget_id: 5,
                    value: 256.0,
                })
                .is_err()
        );
    }

    #[test]
    fn dialog_results_preserve_types_and_reject_unlisted_options() {
        let widgets = vec![ScriptDialogWidget {
            kind: ScriptDialogWidgetKind::Combo,
            label: Some("Mode".to_owned()),
            value: ScriptDialogValue::String("safe".to_owned()),
            options: vec![
                ScriptDialogValue::String("safe".to_owned()),
                ScriptDialogValue::String("fast".to_owned()),
            ],
            minimum: None,
            maximum: None,
            precision: None,
        }];
        assert_eq!(
            validated_dialog_values(
                &widgets,
                vec![wire::ScriptDialogValue::String("fast".to_owned())]
            )
            .expect("listed option"),
            vec![ScriptDialogValue::String("fast".to_owned())]
        );
        assert!(
            validated_dialog_values(
                &widgets,
                vec![wire::ScriptDialogValue::String("unknown".to_owned())]
            )
            .is_err()
        );
    }

    #[test]
    fn message_only_dialogs_are_valid_but_empty_dialogs_are_rejected() {
        let request = HostDialogOpenRequest {
            title: "Notice".to_owned(),
            description: Some("Completed".to_owned()),
            widgets: Vec::new(),
        };
        validate_dialog_open_request(&request).expect("message-only dialog");
        assert!(
            validate_dialog_open_request(&HostDialogOpenRequest {
                description: None,
                ..request
            })
            .is_err()
        );
    }

    #[test]
    fn overlay_expiry_removes_only_the_inserted_shape() {
        let mut overlay = OverlayStore::default();
        let expiry = overlay
            .apply(HostOverlayRequest::Rectangle {
                x1: 1,
                y1: 2,
                x2: 3,
                y2: 4,
                outline: "red".to_owned(),
                tag: "match".to_owned(),
                expires_ms: Some(10),
            })
            .expect("overlay")
            .expect("expiry");
        assert_eq!(overlay.snapshot().shapes.len(), 1);
        assert!(overlay.expire(expiry.id));
        assert!(overlay.snapshot().shapes.is_empty());
    }
}
