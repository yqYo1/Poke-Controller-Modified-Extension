//! Production REST/WebSocket adapter over the Rust-owned runtime services.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::camera::{
    CameraManager, CameraSelector as RuntimeCameraSelector, ScreenshotDestination,
    ScreenshotFormat, ScreenshotMode, ScreenshotRequest as RuntimeScreenshotRequest,
    ScreenshotResult as RuntimeScreenshotResult, ScreenshotService,
};
use crate::device::input::ManualInterventionPolicy;
use crate::device::{
    ApplyResult, InputArbiter, InputEvent, InputGeneration as RuntimeInputGeneration,
    InputPriority, InputSequence, InputSnapshot as RuntimeInputSnapshot, InputSourceId,
    InputSourceKind, MouseButton, PressState, StickSide,
};
use crate::device::{Button, ControllerState, Hat, StickPosition, TouchPoint};
use crate::device::{NotificationChannel, NotificationOutcome, NotificationService};
use crate::device::{SerialError, SerialManager, enumerate_native_ports};
use crate::dynamic::{DynamicConfigControl, DynamicConfigLanguage};
use crate::server::api::{
    ApiErrorCode, CameraDevice, CameraSelector, ClientMessage, CommandControlRequest,
    DecimalString, DynamicConfigControlRequest, DynamicConfigResult, DynamicLanguage,
    GamepadButton, GamepadHat, GamepadInput, GenerateLauncherRequest, GenerateLauncherResult,
    ImageFormat, InputApplied, InputGeneration, LauncherDestination, NotificationTestRequest,
    NotificationTestResult, OperationResult, ScriptUiAction, ScriptUiActionResult,
    SerialControlRequest, SerialPort, SettingsChange, SettingsPatchRequest, SettingsReadValues,
    SettingsSnapshot, SettingsWriteValues, StateChangeCause, StatePatch, StateSnapshot,
    UpdateCheckResult,
};
use crate::server::backend::{
    ApiFailure, ApiFailureStatus, ApiResult, DownloadMediaType, DownloadPayload, LauncherOutput,
    RestBackend, ScreenshotOutput,
};
use crate::server::realtime_connection::RealtimeConnectionConfig;
use crate::server::state::{CommitOutcome, StateHub, StateTransaction};
use crate::server::websocket::{
    ConnectionId, MotionJpegFeed, MotionJpegStream, WebSocketBackend, WebSocketReply,
};
use crate::settings::roots::SafeComponent;
use crate::settings::service::{
    PatchClass, PatchError, PatchRequest, PatchResponse, SettingsService,
};
use crate::worker::dynamic::DynamicWorkerClient;
use async_trait::async_trait;
use parking_lot::Mutex as ParkingMutex;
use semver::Version;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use crate::command_service::{
    CommandActionResult, CommandIdentity, CommandReloadResult, CommandService, CommandServiceError,
};
use crate::dynamic_host::StartupDynamicHost;
use crate::profile_service::{ProfileService, ProfileSwitchResult};
use crate::script_host::ScriptUiCoordinator;

const RELEASES_URL: &str = "https://github.com/yqYo1/Poke-Controller-Modified-Extension/releases";
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/yqYo1/Poke-Controller-Modified-Extension/releases/latest";

/// All already-created services needed by the transport adapter.
pub(crate) struct ApplicationBackendParts {
    pub hub: StateHub,
    pub settings: SettingsService,
    pub host: Arc<StartupDynamicHost>,
    pub camera: CameraManager,
    pub serial: SerialManager,
    pub screenshots: ScreenshotService,
    pub notifications: Arc<NotificationService>,
    pub dynamic: Option<Arc<DynamicWorkerClient>>,
    pub realtime: Option<RealtimeConnectionConfig>,
    pub motion_jpeg: Option<MotionJpegFeed>,
    pub screenshot_mode: ScreenshotMode,
    pub script_ui: ScriptUiCoordinator,
}

/// Concrete backend shared by the REST and WebSocket transports.
pub(crate) struct ApplicationBackend {
    hub: StateHub,
    settings: Mutex<SettingsService>,
    host: Arc<StartupDynamicHost>,
    camera: CameraManager,
    serial: SerialManager,
    screenshots: ScreenshotService,
    notifications: Arc<NotificationService>,
    dynamic: Option<Arc<DynamicWorkerClient>>,
    realtime: Option<RealtimeConnectionConfig>,
    motion_jpeg: Option<MotionJpegFeed>,
    screenshot_mode: ScreenshotMode,
    script_ui: ScriptUiCoordinator,
    arbiter: Arc<ParkingMutex<InputArbiter>>,
    mutation_gate: Mutex<()>,
    commands: OnceLock<Arc<CommandService>>,
    profiles: OnceLock<Arc<ProfileService>>,
}

impl std::fmt::Debug for ApplicationBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplicationBackend")
            .field("camera", &self.camera)
            .field("serial", &self.serial)
            .field("dynamic_available", &self.dynamic.is_some())
            .finish_non_exhaustive()
    }
}

impl ApplicationBackend {
    pub(crate) fn new(parts: ApplicationBackendParts) -> Self {
        let arbiter = parts.host.controller_safety().arbiter();
        Self {
            hub: parts.hub,
            settings: Mutex::new(parts.settings),
            host: parts.host,
            camera: parts.camera,
            serial: parts.serial,
            screenshots: parts.screenshots,
            notifications: parts.notifications,
            dynamic: parts.dynamic,
            realtime: parts.realtime,
            motion_jpeg: parts.motion_jpeg,
            screenshot_mode: parts.screenshot_mode,
            script_ui: parts.script_ui,
            arbiter,
            mutation_gate: Mutex::new(()),
            commands: OnceLock::new(),
            profiles: OnceLock::new(),
        }
    }

    pub(crate) fn install_command_services(
        &self,
        commands: Arc<CommandService>,
        profiles: Arc<ProfileService>,
    ) -> Result<(), String> {
        self.commands
            .set(commands)
            .map_err(|_commands| "command service was already installed".to_owned())?;
        self.profiles
            .set(profiles)
            .map_err(|_profiles| "profile service was already installed".to_owned())
    }

    pub(crate) fn host(&self) -> Arc<StartupDynamicHost> {
        Arc::clone(&self.host)
    }

    /// Smallest typed API for the composition root to enforce manual intervention.
    /// `Allowed` (default) permits element-level manual override; `Denied` blocks
    /// manual press/move while keeping neutral release and `force_release_all`
    /// unconditional. Takes effect from the next input.
    pub fn set_manual_intervention(&self, policy: ManualInterventionPolicy) {
        self.arbiter.lock().set_manual_policy(policy);
    }

    #[allow(dead_code, reason = "public diagnostic API for alternate adapters")]
    #[must_use]
    pub fn manual_policy(&self) -> ManualInterventionPolicy {
        self.arbiter.lock().manual_policy()
    }

    #[allow(dead_code, reason = "public diagnostic API for alternate adapters")]
    #[must_use]
    pub fn is_manual_allowed(&self) -> bool {
        self.arbiter.lock().is_manual_allowed()
    }

    pub(crate) async fn reconcile_host(&self, cause: StateChangeCause) -> ApiResult<()> {
        let _gate = self.mutation_gate.lock().await;
        let loaded = self.host.loaded_settings();
        let manual_allowed = loaded
            .settings
            .boolean("input.allow_manual_intervention")
            .unwrap_or(true);
        self.settings.lock().await.adopt_runtime_loaded(loaded);
        self.set_manual_intervention(if manual_allowed {
            ManualInterventionPolicy::Allowed
        } else {
            ManualInterventionPolicy::Denied
        });
        self.commit_projection(cause, true, None)
            .await
            .map(|_outcome| ())
    }

    async fn ensure_expected(&self, expected: Option<&DecimalString>) -> ApiResult<()> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let current = self.hub.state_snapshot().await.revision;
        if expected == &current {
            Ok(())
        } else {
            Err(ApiFailure::new(
                ApiFailureStatus::Conflict,
                ApiErrorCode::RevisionConflict,
                format!("settings changed; current revision is {current}"),
            ))
        }
    }

    async fn commit_projection(
        &self,
        cause: StateChangeCause,
        include_settings: bool,
        expected_revision: Option<DecimalString>,
    ) -> ApiResult<CommitOutcome> {
        let state = self.projected_state().await?;
        let settings = if include_settings {
            Some(settings_change(
                self.settings.lock().await.public_response(),
            ))
        } else {
            None
        };
        let mut transaction = StateTransaction::new(cause);
        transaction.expected_revision = expected_revision;
        transaction.state = full_state_patch(state);
        transaction.settings = settings;
        self.hub.commit(transaction).await.map_err(state_failure)
    }

    async fn projected_state(&self) -> ApiResult<StateSnapshot> {
        let mut public = self.host.public_state_snapshot();
        let camera = self.camera.status();
        public.insert(
            "camera_opened".to_owned(),
            Value::Bool(camera.camera_opened),
        );
        public.insert("camera_fps".to_owned(), Value::from(camera.camera_fps));
        public.insert(
            "camera_resolution".to_owned(),
            Value::String(camera.camera_resolution),
        );
        public.insert(
            "camera_device".to_owned(),
            serde_json::to_value(camera.camera_device)
                .map_err(|_error| internal_failure("camera state projection failed"))?,
        );
        let serial_config = self.serial.current_config().await;
        public.insert(
            "serial_port".to_owned(),
            serial_config.as_ref().map_or(Value::Null, |config| {
                Value::String(config.selector.as_str().to_owned())
            }),
        );
        if let Some(config) = serial_config {
            public.insert("serial_baud_rate".to_owned(), Value::from(config.baud_rate));
        }
        public.insert(
            "serial_connected".to_owned(),
            Value::Bool(self.serial.is_connected().await),
        );
        if public.get("current_command").and_then(Value::as_str) == Some("") {
            public.insert("current_command".to_owned(), Value::Null);
        }
        public.insert("revision".to_owned(), Value::String("0".to_owned()));
        serde_json::from_value(Value::Object(public.into_iter().collect::<Map<_, _>>()))
            .map_err(|_error| internal_failure("runtime state projection failed"))
    }

    fn commands(&self) -> ApiResult<&Arc<CommandService>> {
        self.commands
            .get()
            .ok_or_else(|| internal_failure("command service is unavailable"))
    }

    fn profiles(&self) -> ApiResult<&Arc<ProfileService>> {
        self.profiles
            .get()
            .ok_or_else(|| internal_failure("profile service is unavailable"))
    }

    async fn publish_controller(&self, last_input: Option<String>) -> ApiResult<()> {
        let output = self.arbiter.lock().output();
        if let Err(error) = self.serial.send_controller_state(output).await
            && !matches!(
                error,
                SerialError::Disconnected | SerialError::SendCancelled
            )
        {
            tracing::warn!(error = %error, "controller output could not be written to serial");
        }
        self.host.notify_controller_change();
        let mut transaction = StateTransaction::new(StateChangeCause::Other);
        transaction.state.holding_buttons = Some(holding_buttons(output));
        transaction.state.last_input = Some(last_input);
        transaction.state.serial_connected = Some(self.serial.is_connected().await);
        self.hub
            .commit(transaction)
            .await
            .map_err(state_failure)
            .map(|_outcome| ())
    }

    fn source(connection: ConnectionId) -> InputSourceId {
        InputSourceId::new(format!("websocket-{}", connection.get()))
            .expect("a numeric WebSocket source identifier is valid")
    }
}

// This implementation intentionally keeps the complete HTTP surface together
// so endpoint-to-runtime mappings remain auditable against the specification.
#[allow(clippy::too_many_lines)]
#[async_trait]
impl RestBackend for ApplicationBackend {
    fn state_hub(&self) -> &StateHub {
        &self.hub
    }

    async fn patch_settings(&self, request: SettingsPatchRequest) -> ApiResult<SettingsSnapshot> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_expected(request.expected_revision.as_ref())
            .await?;
        let expected_revision = request.expected_revision.clone();
        let internal = PatchRequest {
            expected_revision: None,
            values: request.values.0,
        };
        let class = self
            .settings
            .lock()
            .await
            .validate_patch(&internal)
            .map_err(|error| patch_failure(&error))?;
        if class == PatchClass::Profile {
            let target = internal
                .values
                .get("active_profile")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_field("active_profile", "profile name must be a string"))?;
            let current = self.settings.lock().await.active_profile().to_owned();
            if target != current {
                match self
                    .profiles()?
                    .switch(target)
                    .await
                    .map_err(|_error| profile_failure())?
                {
                    ProfileSwitchResult::Switched { .. } => {
                        self.settings
                            .lock()
                            .await
                            .adopt_loaded(self.host.loaded_settings());
                    }
                    ProfileSwitchResult::Cancelled => {}
                }
            }
        } else {
            let response = self
                .settings
                .lock()
                .await
                .patch(&internal)
                .map_err(|error| patch_failure(&error))?;
            tracing::debug!(
                settings_revision = response.revision,
                "settings transaction applied"
            );
        }
        let outcome = self
            .commit_projection(
                if class == PatchClass::Profile {
                    StateChangeCause::Profile
                } else {
                    StateChangeCause::Settings
                },
                true,
                expected_revision,
            )
            .await?;
        // SPEC runtime_immediate: reflect committed effective setting in the arbiter.
        // Hold the mutation gate while updating so a failing commit never mutates the arbiter.
        let allowed = outcome
            .snapshots
            .settings
            .values
            .0
            .get("input.allow_manual_intervention")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        self.set_manual_intervention(if allowed {
            ManualInterventionPolicy::Allowed
        } else {
            ManualInterventionPolicy::Denied
        });
        Ok(outcome.snapshots.settings)
    }

    async fn control_command(&self, request: CommandControlRequest) -> ApiResult<OperationResult> {
        let _gate = self.mutation_gate.lock().await;
        let commands = Arc::clone(self.commands()?);
        let action = match request {
            CommandControlRequest::Start(fields) => {
                commands
                    .start(&CommandIdentity {
                        module_path: fields.command.module_path,
                        class_name: fields.command.class_name,
                    })
                    .await
            }
            CommandControlRequest::Stop {}
                if commands.status().await == crate::command_service::CommandStatus::Stopped =>
            {
                Ok(CommandActionResult::AlreadyInState)
            }
            CommandControlRequest::Stop {} => commands.stop().await,
            CommandControlRequest::Pause {} => commands.pause().await,
            CommandControlRequest::Resume {} => commands.resume().await,
        }
        .map_err(|error| command_failure(&error))?;
        let outcome = self
            .commit_projection(StateChangeCause::Command, false, None)
            .await?;
        Ok(OperationResult {
            changed: action == CommandActionResult::Applied && outcome.changed(),
            revision: outcome.revision().clone(),
        })
    }

    async fn reload_commands(&self) -> ApiResult<OperationResult> {
        let _gate = self.mutation_gate.lock().await;
        let result = self
            .commands()?
            .reload()
            .await
            .map_err(|error| command_failure(&error))?;
        let outcome = self
            .commit_projection(StateChangeCause::Commands, false, None)
            .await?;
        Ok(OperationResult {
            changed: matches!(result, CommandReloadResult::Published { .. }) && outcome.changed(),
            revision: outcome.revision().clone(),
        })
    }

    async fn enumerate_cameras(&self) -> ApiResult<Vec<CameraDevice>> {
        let camera = self.camera.clone();
        tokio::task::spawn_blocking(move || camera.enumerate())
            .await
            .map_err(|_error| internal_failure("camera enumeration task failed"))?
            .map_err(|_error| {
                ApiFailure::new(
                    ApiFailureStatus::InternalServerError,
                    ApiErrorCode::CameraUnavailable,
                    "camera enumeration failed",
                )
            })?
            .into_iter()
            .map(|device| {
                Ok(CameraDevice {
                    selector: camera_selector(device.selector),
                    label: device.label,
                    available: device.available,
                })
            })
            .collect()
    }

    async fn enumerate_serial_ports(&self) -> ApiResult<Vec<SerialPort>> {
        let mut ports = tokio::task::spawn_blocking(enumerate_native_ports)
            .await
            .map_err(|_error| internal_failure("serial enumeration task failed"))?
            .map_err(|_error| internal_failure("serial port enumeration failed"))?
            .into_iter()
            .map(|candidate| SerialPort {
                selector: candidate.selector.as_str().to_owned(),
                label: candidate.selector.as_str().to_owned(),
                available: true,
            })
            .collect::<Vec<_>>();
        let configured = self
            .serial
            .current_config()
            .await
            .map(|config| config.selector.as_str().to_owned());
        if let Some(configured) = configured
            && !ports.iter().any(|port| port.selector == configured)
        {
            ports.push(SerialPort {
                label: configured.clone(),
                selector: configured,
                available: false,
            });
        }
        Ok(ports)
    }

    async fn control_serial(&self, request: SerialControlRequest) -> ApiResult<OperationResult> {
        let _gate = self.mutation_gate.lock().await;
        let before = self.serial.is_connected().await;
        match request {
            SerialControlRequest::Connect {} if before => {}
            SerialControlRequest::Connect {} => {
                self.serial.reconnect().await.map_err(serial_failure)?;
            }
            SerialControlRequest::Disconnect {} if !before => {}
            SerialControlRequest::Disconnect {} => {
                self.serial.disconnect().await.map_err(serial_failure)?;
            }
        }
        let outcome = self
            .commit_projection(StateChangeCause::Serial, false, None)
            .await?;
        Ok(OperationResult {
            changed: before != self.serial.is_connected().await && outcome.changed(),
            revision: outcome.revision().clone(),
        })
    }

    async fn retry_camera(&self) -> ApiResult<OperationResult> {
        let _gate = self.mutation_gate.lock().await;
        let before = self.camera.status();
        let camera = self.camera.clone();
        tokio::task::spawn_blocking(move || camera.retry())
            .await
            .map_err(|_error| internal_failure("camera retry task failed"))?
            .map_err(|_error| camera_failure())?;
        let outcome = self
            .commit_projection(StateChangeCause::Camera, false, None)
            .await?;
        Ok(OperationResult {
            changed: before != self.camera.status() && outcome.changed(),
            revision: outcome.revision().clone(),
        })
    }

    async fn screenshot(
        &self,
        request: crate::server::api::ScreenshotRequest,
    ) -> ApiResult<ScreenshotOutput> {
        let request = runtime_screenshot_request(request)?;
        let screenshots = self.screenshots.clone();
        let result = tokio::task::spawn_blocking(move || screenshots.capture(&request))
            .await
            .map_err(|_error| internal_failure("screenshot task failed"))?
            .map_err(screenshot_failure)?;
        match result {
            RuntimeScreenshotResult::Saved(saved) => Ok(ScreenshotOutput::Saved(
                crate::server::api::SavedScreenshot {
                    display_path: saved.display_path,
                    format: image_format(saved.format),
                },
            )),
            RuntimeScreenshotResult::Download(download) => {
                Ok(ScreenshotOutput::Download(DownloadPayload::new(
                    download.filename,
                    DownloadMediaType::from(image_format(download.format)),
                    download.bytes,
                )))
            }
        }
    }

    async fn test_notification(
        &self,
        request: NotificationTestRequest,
    ) -> ApiResult<NotificationTestResult> {
        let channel = match request {
            NotificationTestRequest::Windows {} => NotificationChannel::Windows,
            NotificationTestRequest::Discord {} => NotificationChannel::Discord,
        };
        let report = self.notifications.test(channel).await;
        let outcome = report
            .outcomes
            .first()
            .map_or(NotificationOutcome::Failed, |(_channel, outcome)| *outcome);
        match outcome {
            NotificationOutcome::Delivered => Ok(NotificationTestResult { delivered: true }),
            NotificationOutcome::QueueFull => Ok(NotificationTestResult { delivered: false }),
            NotificationOutcome::Failed | NotificationOutcome::Disabled => {
                Ok(NotificationTestResult { delivered: false })
            }
            NotificationOutcome::SkippedMissingConfiguration => Err(ApiFailure::new(
                ApiFailureStatus::UnprocessableEntity,
                ApiErrorCode::NotificationUnsupported,
                "notification channel is not configured",
            )),
            NotificationOutcome::SkippedUnsupportedPlatform => Err(ApiFailure::new(
                ApiFailureStatus::Conflict,
                ApiErrorCode::NotificationUnsupported,
                "notification channel is unsupported on this platform",
            )),
        }
    }

    async fn script_ui_action(&self, request: ScriptUiAction) -> ApiResult<ScriptUiActionResult> {
        let _gate = self.mutation_gate.lock().await;
        let outcome = self
            .script_ui
            .apply_action(request)
            .map_err(|error| script_ui_failure(&error))?;
        if let Some(event) = outcome.tk_event {
            self.commands()?
                .dispatch_tk_event(&event)
                .await
                .map_err(|error| command_failure(&error))?;
        }
        if let Some(event) = outcome.pointer_event {
            self.commands()?
                .dispatch_pointer_event(&event)
                .await
                .map_err(|error| command_failure(&error))?;
        }
        if outcome.abort_command {
            self.commands()?
                .abort_from_script_ui()
                .await
                .map_err(|error| command_failure(&error))?;
            self.commit_projection(StateChangeCause::Command, false, None)
                .await?;
        }
        Ok(ScriptUiActionResult { accepted: true })
    }

    async fn control_dynamic_config(
        &self,
        request: DynamicConfigControlRequest,
    ) -> ApiResult<DynamicConfigResult> {
        let _gate = self.mutation_gate.lock().await;
        let client = self.dynamic.as_ref().ok_or_else(|| {
            ApiFailure::new(
                ApiFailureStatus::Conflict,
                ApiErrorCode::DynamicConfigUnavailable,
                "dynamic configuration worker is unavailable",
            )
        })?;
        let control = match request {
            DynamicConfigControlRequest::LoadPath(fields) => {
                DynamicConfigControl::LoadPath { path: fields.path }
            }
            DynamicConfigControlRequest::LoadContent(fields) => DynamicConfigControl::LoadContent {
                language: match fields.language {
                    DynamicLanguage::Python => DynamicConfigLanguage::Python,
                    DynamicLanguage::Lua => DynamicConfigLanguage::Lua,
                },
                content: fields.content,
            },
            DynamicConfigControlRequest::Reload {} => DynamicConfigControl::Reload {},
        };
        let result = client.control(&control).await.map_err(|_error| {
            ApiFailure::new(
                ApiFailureStatus::Conflict,
                ApiErrorCode::DynamicConfigUnavailable,
                "dynamic configuration operation failed",
            )
        })?;
        self.settings
            .lock()
            .await
            .adopt_runtime_loaded(self.host.loaded_settings());
        self.commit_projection(StateChangeCause::DynamicConfig, true, None)
            .await?;
        Ok(DynamicConfigResult {
            display_path: result.display_path,
            language: match result.language {
                DynamicConfigLanguage::Python => DynamicLanguage::Python,
                DynamicConfigLanguage::Lua => DynamicLanguage::Lua,
            },
            loaded: result.loaded,
        })
    }

    async fn generate_launcher(
        &self,
        request: GenerateLauncherRequest,
    ) -> ApiResult<LauncherOutput> {
        if !cfg!(target_os = "windows") {
            return Err(ApiFailure::new(
                ApiFailureStatus::Conflict,
                ApiErrorCode::UnsupportedPlatform,
                "profile launchers are available only on Windows",
            ));
        }
        let _gate = self.mutation_gate.lock().await;
        let profile = SafeComponent::new(&request.profile)
            .map_err(|_error| invalid_field("profile", "profile name is unsafe"))?;
        let loaded = self.host.loaded_settings();
        let profile_root = loaded.roots.config.join("profiles").join(profile.as_str());
        let bytes = launcher_bytes(profile.as_str())?;
        match &request.destination {
            LauncherDestination::Path(destination) => {
                if self.screenshot_mode != ScreenshotMode::Desktop {
                    return Err(ApiFailure::new(
                        ApiFailureStatus::Conflict,
                        ApiErrorCode::DestinationConflict,
                        "native launcher paths are unavailable in web mode",
                    ));
                }
                validate_launcher_path(Path::new(&destination.path))?;
            }
            LauncherDestination::Download(destination) => {
                if let Some(filename) = destination.filename.as_deref()
                    && !filename.is_empty()
                {
                    validate_download_filename(filename)?;
                }
            }
        }
        let profile_created = if profile_root.exists() {
            false
        } else {
            let creation = if request.copy_current {
                loaded.profile_settings_path.parent().map_or_else(
                    || Err(internal_failure("active profile path is invalid")),
                    |source| copy_directory(source, &profile_root),
                )
            } else {
                std::fs::create_dir(&profile_root)
                    .map_err(|_error| persistence_failure("profile directory creation failed"))
            };
            if let Err(error) = creation {
                let _cleanup = std::fs::remove_dir_all(&profile_root);
                return Err(error);
            }
            true
        };
        let mut launcher_created = false;
        let output = match request.destination {
            LauncherDestination::Path(destination) => {
                let path = PathBuf::from(destination.path);
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(mut file) => {
                        use std::io::Write as _;
                        if file.write_all(&bytes).is_err() {
                            drop(file);
                            let _cleanup = std::fs::remove_file(path);
                            if profile_created {
                                let _cleanup = std::fs::remove_dir_all(&profile_root);
                            }
                            return Err(persistence_failure("launcher file creation failed"));
                        }
                        launcher_created = true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_error) => {
                        if profile_created {
                            let _cleanup = std::fs::remove_dir_all(&profile_root);
                        }
                        return Err(persistence_failure("launcher file creation failed"));
                    }
                }
                None
            }
            LauncherDestination::Download(destination) => {
                let filename = destination
                    .filename
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| format!("{}.bat", profile.as_str()));
                validate_download_filename(&filename)?;
                launcher_created = true;
                Some(DownloadPayload::new(
                    filename,
                    DownloadMediaType::WindowsBatch,
                    bytes,
                ))
            }
        };
        let outcome = if profile_created {
            self.host
                .refresh_available_profiles()
                .map_err(|_error| internal_failure("profile list refresh failed"))?;
            self.commit_projection(StateChangeCause::Profile, false, None)
                .await?
        } else {
            let snapshots = self.hub.snapshots().await;
            CommitOutcome {
                snapshots,
                event: None,
            }
        };
        if let Some(download) = output {
            Ok(LauncherOutput::Download(download))
        } else {
            Ok(LauncherOutput::Generated(GenerateLauncherResult {
                profile_created,
                launcher_created,
                revision: outcome.revision().clone(),
            }))
        }
    }

    async fn check_update(&self) -> ApiResult<UpdateCheckResult> {
        #[derive(Deserialize)]
        struct LatestRelease {
            tag_name: String,
            html_url: String,
        }

        let client = reqwest::Client::builder()
            .user_agent(concat!("pokecon/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_error| backend_unavailable("update check is unavailable"))?;
        let release = client
            .get(LATEST_RELEASE_URL)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_error| backend_unavailable("update check failed"))?
            .json::<LatestRelease>()
            .await
            .map_err(|_error| backend_unavailable("update response was invalid"))?;
        let current = Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|_error| internal_failure("application version is invalid"))?;
        let latest_text = release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name);
        let latest = Version::parse(latest_text)
            .map_err(|_error| backend_unavailable("latest release version is invalid"))?;
        Ok(UpdateCheckResult {
            current_version: current.to_string(),
            latest_version: latest.to_string(),
            update_available: latest > current,
            release_url: if release.html_url.is_empty() {
                RELEASES_URL.to_owned()
            } else {
                release.html_url
            },
        })
    }
}

#[async_trait]
impl WebSocketBackend for ApplicationBackend {
    fn state_hub(&self) -> &StateHub {
        &self.hub
    }

    async fn connected(
        &self,
        connection: ConnectionId,
        generation: &InputGeneration,
    ) -> ApiResult<()> {
        let _gate = self.mutation_gate.lock().await;
        let generation = RuntimeInputGeneration::new(generation.generation.clone())
            .map_err(|_error| invalid_input())?;
        self.arbiter.lock().begin_generation(
            Self::source(connection),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            generation,
        );
        self.publish_controller(None).await
    }

    async fn message(
        &self,
        connection: ConnectionId,
        message: ClientMessage,
    ) -> ApiResult<Vec<WebSocketReply>> {
        let _gate = self.mutation_gate.lock().await;
        let source = Self::source(connection);
        let (result, acknowledgement, last_input) = {
            let mut arbiter = self.arbiter.lock();
            match message {
                ClientMessage::InputSnapshot(message) => {
                    let snapshot = serde_json::from_value::<RuntimeInputSnapshot>(
                        serde_json::to_value(message.data).map_err(|_error| invalid_input())?,
                    )
                    .map_err(|_error| invalid_input())?;
                    let (result, acknowledgement) = arbiter
                        .apply_snapshot(&source, snapshot)
                        .map_err(|_error| invalid_input())?;
                    (result, acknowledgement, Some("input.snapshot".to_owned()))
                }
                ClientMessage::KeyboardInput(message) => {
                    let data = message.data;
                    let result = arbiter
                        .apply_event(
                            &source,
                            &runtime_generation(&data.generation)?,
                            runtime_sequence(&data.sequence)?,
                            InputEvent::Keyboard {
                                key: data.key.clone(),
                                state: press_state(data.state),
                            },
                        )
                        .map_err(|_error| invalid_input())?;
                    (result, None, Some(format!("keyboard:{}", data.key)))
                }
                ClientMessage::MouseStickInput(message) => {
                    let data = message.data;
                    let result = arbiter
                        .apply_event(
                            &source,
                            &runtime_generation(&data.generation)?,
                            runtime_sequence(&data.sequence)?,
                            InputEvent::Stick {
                                stick: match data.stick {
                                    crate::server::api::StickName::LStick => StickSide::Left,
                                    crate::server::api::StickName::RStick => StickSide::Right,
                                },
                                position: StickPosition {
                                    x: data.x,
                                    y: data.y,
                                },
                            },
                        )
                        .map_err(|_error| invalid_input())?;
                    (result, None, Some("mouse:stick".to_owned()))
                }
                ClientMessage::MouseInput(message) => {
                    let data = message.data;
                    let result = arbiter
                        .apply_event(
                            &source,
                            &runtime_generation(&data.generation)?,
                            runtime_sequence(&data.sequence)?,
                            InputEvent::MouseButton {
                                button: match data.button {
                                    crate::server::api::MouseButton::Left => MouseButton::Left,
                                    crate::server::api::MouseButton::Right => MouseButton::Right,
                                    crate::server::api::MouseButton::Middle => MouseButton::Middle,
                                },
                                state: press_state(data.state),
                            },
                        )
                        .map_err(|_error| invalid_input())?;
                    (result, None, Some("mouse:button".to_owned()))
                }
                ClientMessage::GamepadInput(message) => {
                    let (generation, sequence, event, label) = gamepad_event(message.data)?;
                    let result = arbiter
                        .apply_event(&source, &generation, sequence, event)
                        .map_err(|_error| invalid_input())?;
                    (result, None, Some(label))
                }
                ClientMessage::WebRtcOffer(_)
                | ClientMessage::WebRtcAnswer(_)
                | ClientMessage::WebRtcIceCandidate(_)
                | ClientMessage::Pong(_) => return Err(invalid_input()),
            }
        };
        if result == ApplyResult::Applied {
            self.publish_controller(last_input).await?;
        }
        Ok(acknowledgement
            .map(|acknowledgement| {
                vec![WebSocketReply::InputSnapshotApplied(InputApplied {
                    generation: acknowledgement.generation.as_str().to_owned(),
                    sequence: acknowledgement
                        .sequence
                        .as_str()
                        .parse::<DecimalString>()
                        .expect("device input sequences are canonical"),
                })]
            })
            .unwrap_or_default())
    }

    fn motion_jpeg(&self, _connection: ConnectionId) -> Option<MotionJpegStream> {
        self.motion_jpeg.as_ref().map(MotionJpegFeed::subscribe)
    }

    fn realtime(&self, _connection: ConnectionId) -> Option<RealtimeConnectionConfig> {
        self.realtime.clone()
    }

    async fn disconnected(&self, connection: ConnectionId) {
        let _gate = self.mutation_gate.lock().await;
        if self
            .arbiter
            .lock()
            .disconnect_source(&Self::source(connection))
            && let Err(error) = self.publish_controller(None).await
        {
            tracing::warn!(error = ?error.error(), "WebSocket input release could not be published");
        }
    }
}

fn settings_change(response: PatchResponse) -> SettingsChange {
    let restart_required = response
        .pending_restart_values
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    SettingsChange {
        values: SettingsWriteValues(response.values),
        pending_restart_values: SettingsWriteValues(response.pending_restart_values),
        restart_required,
        apply_failures: response.apply_failures,
    }
}

pub(crate) fn initial_settings_snapshot(service: &SettingsService) -> SettingsSnapshot {
    let response = service.public_response();
    SettingsSnapshot {
        revision: DecimalString::zero(),
        values: SettingsReadValues(response.values),
        restart_required: response.pending_restart_values.keys().cloned().collect(),
        pending_restart_values: SettingsWriteValues(response.pending_restart_values),
        apply_failures: response.apply_failures,
    }
}

pub(crate) async fn initial_state_snapshot(
    host: &StartupDynamicHost,
    camera: &CameraManager,
    serial: &SerialManager,
) -> Result<StateSnapshot, String> {
    let mut public = host.public_state_snapshot();
    let camera = camera.status();
    public.insert(
        "camera_opened".to_owned(),
        Value::Bool(camera.camera_opened),
    );
    public.insert("camera_fps".to_owned(), Value::from(camera.camera_fps));
    public.insert(
        "camera_resolution".to_owned(),
        Value::String(camera.camera_resolution),
    );
    public.insert(
        "camera_device".to_owned(),
        serde_json::to_value(camera.camera_device)
            .map_err(|_error| "initial camera state projection failed".to_owned())?,
    );
    let serial_config = serial.current_config().await;
    public.insert(
        "serial_port".to_owned(),
        serial_config.as_ref().map_or(Value::Null, |config| {
            Value::String(config.selector.as_str().to_owned())
        }),
    );
    if let Some(config) = serial_config {
        public.insert("serial_baud_rate".to_owned(), Value::from(config.baud_rate));
    }
    public.insert(
        "serial_connected".to_owned(),
        Value::Bool(serial.is_connected().await),
    );
    if public.get("current_command").and_then(Value::as_str) == Some("") {
        public.insert("current_command".to_owned(), Value::Null);
    }
    public.insert("revision".to_owned(), Value::String("0".to_owned()));
    serde_json::from_value(Value::Object(public.into_iter().collect()))
        .map_err(|_error| "initial state projection failed".to_owned())
}

fn full_state_patch(state: StateSnapshot) -> StatePatch {
    StatePatch {
        serial_port: Some(state.serial_port),
        serial_baud_rate: Some(state.serial_baud_rate),
        serial_connected: Some(state.serial_connected),
        camera_opened: Some(state.camera_opened),
        camera_fps: Some(state.camera_fps),
        camera_resolution: Some(state.camera_resolution),
        camera_device: Some(state.camera_device),
        is_running: Some(state.is_running),
        command_state: Some(state.command_state),
        current_command: Some(state.current_command),
        command_candidates: Some(state.command_candidates),
        tags: Some(state.tags),
        active_profile: Some(state.active_profile),
        pending_profile: Some(state.pending_profile),
        available_profiles: Some(state.available_profiles),
        last_input: Some(state.last_input),
        holding_buttons: Some(state.holding_buttons),
        pid: Some(state.pid),
        command_display_lists: Some(state.command_display_lists),
        command_display_cache_loading: Some(state.command_display_cache_loading),
    }
}

fn camera_selector(selector: RuntimeCameraSelector) -> CameraSelector {
    match selector {
        RuntimeCameraSelector::Index(index) => CameraSelector::Index(index),
        RuntimeCameraSelector::Native(name) => CameraSelector::Name(name),
    }
}

fn runtime_screenshot_request(
    request: crate::server::api::ScreenshotRequest,
) -> ApiResult<RuntimeScreenshotRequest> {
    let (region, destination) = match request {
        crate::server::api::ScreenshotRequest::Captures(fields) => (
            fields.region,
            ScreenshotDestination::Captures {
                filename: fields.filename,
                format: fields.format.map(screenshot_format),
                overwrite: fields.overwrite,
            },
        ),
        crate::server::api::ScreenshotRequest::Path(fields) => (
            fields.region,
            ScreenshotDestination::Path {
                path: fields.path,
                format: screenshot_format(fields.format),
                overwrite: fields.overwrite,
            },
        ),
        crate::server::api::ScreenshotRequest::Download(fields) => (
            fields.region,
            ScreenshotDestination::Download {
                filename: fields.filename,
                format: screenshot_format(fields.format),
            },
        ),
    };
    let region = region
        .map(|region| {
            crate::camera::NormalizedRegion::new(region.x, region.y, region.width, region.height)
        })
        .transpose()
        .map_err(|_error| invalid_field("region", "screenshot region is invalid"))?;
    Ok(RuntimeScreenshotRequest {
        region,
        destination,
    })
}

const fn screenshot_format(format: ImageFormat) -> ScreenshotFormat {
    match format {
        ImageFormat::Png => ScreenshotFormat::Png,
        ImageFormat::Jpeg => ScreenshotFormat::Jpeg,
    }
}

const fn image_format(format: ScreenshotFormat) -> ImageFormat {
    match format {
        ScreenshotFormat::Png => ImageFormat::Png,
        ScreenshotFormat::Jpeg => ImageFormat::Jpeg,
    }
}

fn holding_buttons(controller: ControllerState) -> Vec<String> {
    controller
        .buttons
        .pressed()
        .into_iter()
        .map(|button| format!("{button:?}").to_uppercase())
        .collect()
}

fn runtime_generation(value: &str) -> ApiResult<RuntimeInputGeneration> {
    RuntimeInputGeneration::new(value).map_err(|_error| invalid_input())
}

fn runtime_sequence(value: &DecimalString) -> ApiResult<InputSequence> {
    InputSequence::new(value.as_str()).map_err(|_error| invalid_input())
}

const fn press_state(value: crate::server::api::PressState) -> PressState {
    match value {
        crate::server::api::PressState::Pressed => PressState::Pressed,
        crate::server::api::PressState::Released => PressState::Released,
    }
}

fn gamepad_event(
    input: GamepadInput,
) -> ApiResult<(RuntimeInputGeneration, InputSequence, InputEvent, String)> {
    match input {
        GamepadInput::Button(data) => Ok((
            runtime_generation(&data.generation)?,
            runtime_sequence(&data.sequence)?,
            InputEvent::ControllerButton {
                button: gamepad_button(data.button),
                state: press_state(data.state),
            },
            "gamepad:button".to_owned(),
        )),
        GamepadInput::Stick(data) => Ok((
            runtime_generation(&data.generation)?,
            runtime_sequence(&data.sequence)?,
            InputEvent::Stick {
                stick: match data.stick {
                    crate::server::api::StickName::LStick => StickSide::Left,
                    crate::server::api::StickName::RStick => StickSide::Right,
                },
                position: StickPosition {
                    x: data.x,
                    y: data.y,
                },
            },
            "gamepad:stick".to_owned(),
        )),
        GamepadInput::Hat(data) => Ok((
            runtime_generation(&data.generation)?,
            runtime_sequence(&data.sequence)?,
            InputEvent::Hat(gamepad_hat(data.hat)),
            "gamepad:hat".to_owned(),
        )),
        GamepadInput::Touch(data) => Ok((
            runtime_generation(&data.generation)?,
            runtime_sequence(&data.sequence)?,
            InputEvent::Touch(
                data.touch
                    .map(|touch| TouchPoint::new(touch.x, touch.y))
                    .transpose()
                    .map_err(|_error| invalid_input())?,
            ),
            "gamepad:touch".to_owned(),
        )),
    }
}

const fn gamepad_button(button: GamepadButton) -> Button {
    match button {
        GamepadButton::A => Button::A,
        GamepadButton::B => Button::B,
        GamepadButton::X => Button::X,
        GamepadButton::Y => Button::Y,
        GamepadButton::L => Button::L,
        GamepadButton::R => Button::R,
        GamepadButton::Zl => Button::Zl,
        GamepadButton::Zr => Button::Zr,
        GamepadButton::Lclick => Button::Lclick,
        GamepadButton::Rclick => Button::Rclick,
        GamepadButton::Minus => Button::Minus,
        GamepadButton::Plus => Button::Plus,
        GamepadButton::Home => Button::Home,
        GamepadButton::Capture => Button::Capture,
    }
}

const fn gamepad_hat(hat: GamepadHat) -> Hat {
    match hat {
        GamepadHat::Up => Hat::Up,
        GamepadHat::Down => Hat::Down,
        GamepadHat::Left => Hat::Left,
        GamepadHat::Right => Hat::Right,
        GamepadHat::TopRight => Hat::UpRight,
        GamepadHat::BottomRight => Hat::DownRight,
        GamepadHat::BottomLeft => Hat::DownLeft,
        GamepadHat::TopLeft => Hat::UpLeft,
        GamepadHat::Center => Hat::Neutral,
    }
}

fn launcher_bytes(profile: &str) -> ApiResult<Vec<u8>> {
    let executable = std::env::current_exe()
        .map_err(|_error| internal_failure("current executable is unavailable"))?;
    let executable = executable.to_string_lossy().replace('%', "%%");
    let profile = profile.replace('%', "%%");
    Ok(format!("@echo off\r\n\"{executable}\" --profile \"{profile}\" %*\r\n").into_bytes())
}

fn validate_launcher_path(path: &Path) -> ApiResult<()> {
    if !path.is_absolute() || path.extension().and_then(std::ffi::OsStr::to_str) != Some("bat") {
        return Err(invalid_field(
            "destination.path",
            "launcher path must be an absolute .bat path",
        ));
    }
    Ok(())
}

fn validate_download_filename(filename: &str) -> ApiResult<()> {
    if filename.is_empty()
        || filename.contains(['/', '\\', '\r', '\n', '\0'])
        || Path::new(filename)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            != Some(filename)
    {
        return Err(invalid_field(
            "destination.filename",
            "download filename must be a single safe basename",
        ));
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> ApiResult<()> {
    std::fs::create_dir_all(destination)
        .map_err(|_error| persistence_failure("profile directory creation failed"))?;
    for entry in std::fs::read_dir(source)
        .map_err(|_error| persistence_failure("active profile could not be read"))?
    {
        let entry =
            entry.map_err(|_error| persistence_failure("active profile could not be read"))?;
        let file_type = entry
            .file_type()
            .map_err(|_error| persistence_failure("active profile entry could not be read"))?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), target)
                .map_err(|_error| persistence_failure("profile copy failed"))?;
        } else {
            return Err(persistence_failure("profile contains an unsupported entry"));
        }
    }
    Ok(())
}

fn patch_failure(error: &PatchError) -> ApiFailure {
    let status = match error.status_code() {
        409 => ApiFailureStatus::Conflict,
        500 => ApiFailureStatus::InternalServerError,
        _ => ApiFailureStatus::UnprocessableEntity,
    };
    let code = match error {
        PatchError::RevisionMismatch { .. } => ApiErrorCode::RevisionConflict,
        PatchError::ProfileSwitchInProgress => ApiErrorCode::ProfileSwitchConflict,
        PatchError::DeviceConflict => ApiErrorCode::BackendUnavailable,
        PatchError::Persistence(_) | PatchError::ProfileRollback | PatchError::Scaffold(_) => {
            ApiErrorCode::PersistenceFailed
        }
        _ => ApiErrorCode::InvalidRequest,
    };
    ApiFailure::new(status, code, error.to_string())
}

fn command_failure(error: &CommandServiceError) -> ApiFailure {
    match error {
        CommandServiceError::CommandNotFound => ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::CommandNotFound,
            "command identity was not discovered",
        ),
        CommandServiceError::CommandBusy
        | CommandServiceError::CommandNotRunning
        | CommandServiceError::CommandGenerationChanged => ApiFailure::new(
            ApiFailureStatus::Conflict,
            ApiErrorCode::CommandStateConflict,
            error.to_string(),
        ),
        CommandServiceError::ProfileSwitchInProgress
        | CommandServiceError::ProfileSwitchGateNotHeld => profile_failure(),
        _ => backend_unavailable("command service operation failed"),
    }
}

fn script_ui_failure(error: &crate::worker::script::ScriptHostError) -> ApiFailure {
    let (status, code) = match error.code.as_str() {
        "StaleScriptUiGeneration" | "ScriptUiObjectNotFound" | "WorkerStopping" => (
            ApiFailureStatus::Conflict,
            ApiErrorCode::CommandStateConflict,
        ),
        "InvalidScriptUiAction" | "InvalidTkValue" => (
            ApiFailureStatus::UnprocessableEntity,
            ApiErrorCode::InvalidRequest,
        ),
        _ => (
            ApiFailureStatus::InternalServerError,
            ApiErrorCode::BackendUnavailable,
        ),
    };
    ApiFailure::new(status, code, error.message.clone())
}

fn state_failure(error: crate::server::state::StateTransactionError) -> ApiFailure {
    match error {
        crate::server::state::StateTransactionError::RevisionConflict { current, .. } => {
            ApiFailure::new(
                ApiFailureStatus::Conflict,
                ApiErrorCode::RevisionConflict,
                format!("settings changed; current revision is {current}"),
            )
        }
        _ => internal_failure("state transaction failed"),
    }
}

fn serial_failure(_error: SerialError) -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::Conflict,
        ApiErrorCode::SerialConnectionFailed,
        "serial connection operation failed",
    )
}

fn camera_failure() -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::Conflict,
        ApiErrorCode::CameraUnavailable,
        "camera is unavailable",
    )
}

fn screenshot_failure(error: crate::camera::ScreenshotError) -> ApiFailure {
    use crate::camera::ScreenshotError;
    match error {
        ScreenshotError::InvalidFormat
        | ScreenshotError::InvalidJpegQuality
        | ScreenshotError::InvalidRegion
        | ScreenshotError::UnsafePath
        | ScreenshotError::UnsafeTargetType => invalid_field("request", &error.to_string()),
        ScreenshotError::NoPublishedFrame | ScreenshotError::PathDestinationUnavailable => {
            ApiFailure::new(
                ApiFailureStatus::Conflict,
                ApiErrorCode::ScreenshotUnavailable,
                error.to_string(),
            )
        }
        ScreenshotError::Conflict => ApiFailure::new(
            ApiFailureStatus::Conflict,
            ApiErrorCode::DestinationConflict,
            error.to_string(),
        ),
        ScreenshotError::EncodingFailed
        | ScreenshotError::IoFailed
        | ScreenshotError::ClockFailed => internal_failure("screenshot operation failed"),
    }
}

fn invalid_input() -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::UnprocessableEntity,
        ApiErrorCode::InvalidRequest,
        "controller input is invalid for the active generation",
    )
}

fn invalid_field(field: &str, message: &str) -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::UnprocessableEntity,
        ApiErrorCode::InvalidRequest,
        message,
    )
    .with_fields(BTreeMap::from([(
        field.to_owned(),
        vec![message.to_owned()],
    )]))
}

fn profile_failure() -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::Conflict,
        ApiErrorCode::ProfileSwitchConflict,
        "profile switch could not be completed",
    )
}

fn persistence_failure(message: &str) -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::InternalServerError,
        ApiErrorCode::PersistenceFailed,
        message,
    )
}

fn backend_unavailable(message: &str) -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::InternalServerError,
        ApiErrorCode::BackendUnavailable,
        message,
    )
}

fn internal_failure(message: &str) -> ApiFailure {
    ApiFailure::new(
        ApiFailureStatus::InternalServerError,
        ApiErrorCode::InternalError,
        message,
    )
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn gamepad_wire_values_map_to_canonical_controller_values() {
        assert_eq!(gamepad_button(GamepadButton::Zr), Button::Zr);
        assert_eq!(gamepad_hat(GamepadHat::TopLeft), Hat::UpLeft);
    }

    #[test]
    fn attachment_filename_rejects_header_and_path_injection() {
        assert!(validate_download_filename("profile.bat").is_ok());
        assert!(validate_download_filename("../profile.bat").is_err());
        assert!(validate_download_filename("profile\r\nX: bad.bat").is_err());
    }

    #[test]
    fn profile_copy_preserves_nested_files() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let source = temporary.path().join("source");
        let nested = source.join("nested");
        std::fs::create_dir_all(&nested).expect("source directories must exist");
        std::fs::write(source.join("settings.toml"), "language = \"en\"\n")
            .expect("source settings must be written");
        std::fs::write(nested.join("data.txt"), "payload").expect("nested source must be written");
        let destination = temporary.path().join("destination");

        copy_directory(&source, &destination).expect("profile copy must succeed");

        assert_eq!(
            std::fs::read_to_string(destination.join("settings.toml")).unwrap(),
            "language = \"en\"\n"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("nested/data.txt")).unwrap(),
            "payload"
        );
    }
}
