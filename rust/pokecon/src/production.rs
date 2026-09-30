//! Construction and bounded shutdown of the production application services.

use std::str::FromStr as _;
use std::sync::Arc;
use std::time::Duration;

use crate::camera::{
    CameraBackend, CameraConfig, CameraManager, CaptureResolution, FlipMode, MappingDescriptor,
    NativeCameraBackend, ScreenshotFormat, ScreenshotRuntimeSettings, ScreenshotService,
    SharedFrameRing, UnstoppedCameraWriter,
};
use crate::desktop::DesktopRuntimeSettings;
use crate::device::ControllerState;
use crate::device::serial::SerialBackend;
use crate::device::{ControllerFormat, NativeSerialBackend, SerialConfig, SerialManager};
use crate::device::{
    DiscordTransport, NotificationService, ReqwestDiscordTransport, UnavailableDiscordTransport,
    WindowsNativeNotificationTransport,
};
use crate::server::api::{SerialData, SerialEncoding, StateChangeCause};
use crate::server::backend::RestBackend;
use crate::server::realtime::RealtimeTransportConfig;
use crate::server::realtime_connection::RealtimeConnectionConfig;
use crate::server::rest;
use crate::server::state::StateHub;
use crate::server::webrtc::{WebRtcMedia, WebRtcMediaConfig, WebRtcPeerConfig};
use crate::server::websocket::{
    MotionJpegFeed, WebSocketBackend, WebSocketConfig, WebSocketTransport,
};
use crate::settings::pipeline::{LoadedSettings, PipelineRequest};
use crate::settings::service::SettingsService;
use crate::worker::dynamic::DynamicWorkerClient;
use axum::Router;
use base64::Engine as _;
use serde::de::DeserializeOwned;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::application_backend::{
    ApplicationBackend, ApplicationBackendParts, initial_settings_snapshot, initial_state_snapshot,
};
use crate::camera::ScreenshotMode;
use crate::command_service::{
    CommandService, CommandServiceError, DynamicCommandBridge, ScriptSessionStop,
    StaticCommandBridge,
};
use crate::dynamic_host::StartupDynamicHost;
use crate::profile_service::ProfileService;
use crate::script_host::{ProductionScriptHostFactory, ScriptUiCoordinator};
use crate::script_runtime::ManagedUserScriptFactory;
use crate::settings_runtime::{
    CameraSettingsApplier, CompositeSettingsApplier, DesktopSettingsApplier, HostSettingsApplier,
    NotificationSettingsApplier, RealtimeSettingsApplier, SerialSettingsApplier,
    WebSocketSettingsApplier, notification_config, reconcile_desktop_settings,
};

const STATE_HISTORY_CAPACITY: usize = 256;
const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct BuildCleanup {
    camera: Option<CameraManager>,
    serial: Option<SerialManager>,
    tasks: Vec<JoinHandle<()>>,
}

impl BuildCleanup {
    async fn cleanup(mut self) {
        for mut task in self.tasks.drain(..) {
            task.abort();
            if timeout(SERVICE_STOP_TIMEOUT, &mut task).await.is_err() {
                tracing::error!("production build cleanup task did not stop before its deadline");
            }
        }
        if let Some(serial) = self.serial.take() {
            match timeout(SERVICE_STOP_TIMEOUT, serial.disconnect()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::error!(%error, "production build serial cleanup failed"),
                Err(_) => tracing::error!("production build serial cleanup timed out"),
            }
        }
        if let Some(camera) = self.camera.take() {
            let result =
                tokio::task::spawn_blocking(move || camera.shutdown(SERVICE_STOP_TIMEOUT)).await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(unstopped)) => {
                    tracing::error!(
                        diagnostic_id = "CAMERA_WRITER_UNSTOPPED",
                        mapping = ?unstopped.mapping_descriptor(),
                        "production build camera cleanup found an unstopped writer; retaining mapping until process exit without unmap"
                    );
                    // Build failure is immediately followed by process exit; retain the
                    // guard until then instead of dropping the mapping under a live writer.
                    std::mem::forget(unstopped);
                }
                Err(error) => {
                    tracing::error!(%error, "production build camera cleanup task failed");
                }
            }
        }
    }

    fn take_tasks(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.tasks)
    }

    fn disarm(&mut self) {
        self.camera = None;
        self.serial = None;
    }
}

/// Fully connected API router and every resource whose lifetime is bounded by
/// one application run.
pub(crate) struct ProductionRuntime {
    router: Router,
    backend: Arc<ApplicationBackend>,
    commands: Arc<CommandService>,
    camera: CameraManager,
    serial: SerialManager,
    tasks: Vec<JoinHandle<()>>,
    script_shutdown_timeout: Duration,
    /// Retained §15.6 steps 2/5/9 fallback ownership. `Some` while the camera
    /// writer never stopped: the shared mapping and writer join handle stay
    /// alive until OS process exit, and shared-memory release stays refused.
    camera_writer_fallback: Option<UnstoppedCameraWriter>,
    /// Retained §15.6 step 5 fallback ownership. `Some` when the script worker
    /// was not reaped or stale reader pins could not be proven recoverable;
    /// `Drop` intentionally forgets this mapping so a live external reader can
    /// never observe a Rust-side unmap before process exit.
    camera_reader_fallback: Option<SharedFrameRing>,
}

impl std::fmt::Debug for ProductionRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductionRuntime")
            .field("backend", &self.backend)
            .field("commands", &self.commands)
            .finish_non_exhaustive()
    }
}

impl ProductionRuntime {
    /// Builds native devices, settings adapters, REST/WebSocket routes, and
    /// lazy user-script orchestration from the final startup snapshot.
    // The composition root is deliberately linear so initialization and
    // ownership order can be compared directly with the shutdown contract.
    pub(crate) async fn build(
        request: PipelineRequest,
        loaded: LoadedSettings,
        host: Arc<StartupDynamicHost>,
        dynamic: Option<Arc<DynamicWorkerClient>>,
        screenshot_mode: ScreenshotMode,
        desktop_settings: Option<DesktopRuntimeSettings>,
    ) -> Result<Self, String> {
        let camera_backend: Arc<dyn CameraBackend> = Arc::new(NativeCameraBackend);
        let serial_backend: Arc<dyn SerialBackend> = Arc::new(NativeSerialBackend);
        Self::build_with_backends(
            request,
            loaded,
            host,
            dynamic,
            screenshot_mode,
            desktop_settings,
            camera_backend,
            serial_backend,
        )
        .await
    }

    /// Test seam sharing the single production composition root
    /// ([`Self::build_inner`]) with caller-supplied camera/serial backends.
    /// The native [`Self::build`] path stays the production-used entry point;
    /// tests pass virtual backends here to reach the same backend, router,
    /// and settings wiring without duplicating composition logic.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn build_with_backends(
        request: PipelineRequest,
        loaded: LoadedSettings,
        host: Arc<StartupDynamicHost>,
        dynamic: Option<Arc<DynamicWorkerClient>>,
        screenshot_mode: ScreenshotMode,
        desktop_settings: Option<DesktopRuntimeSettings>,
        camera_backend: Arc<dyn CameraBackend>,
        serial_backend: Arc<dyn SerialBackend>,
    ) -> Result<Self, String> {
        let mut cleanup = BuildCleanup::default();
        match Self::build_inner(
            request,
            loaded,
            host,
            dynamic,
            screenshot_mode,
            desktop_settings,
            &mut cleanup,
            camera_backend,
            serial_backend,
        )
        .await
        {
            Ok(runtime) => {
                cleanup.disarm();
                Ok(runtime)
            }
            Err(error) => {
                cleanup.cleanup().await;
                Err(error)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    async fn build_inner(
        request: PipelineRequest,
        loaded: LoadedSettings,
        host: Arc<StartupDynamicHost>,
        dynamic: Option<Arc<DynamicWorkerClient>>,
        screenshot_mode: ScreenshotMode,
        desktop_settings: Option<DesktopRuntimeSettings>,
        cleanup: &mut BuildCleanup,
        camera_backend: Arc<dyn CameraBackend>,
        serial_backend: Arc<dyn SerialBackend>,
    ) -> Result<Self, String> {
        let runtime = tokio::runtime::Handle::current();
        let script_shutdown_timeout =
            Duration::from_millis(setting_u64(&loaded, "python.script.shutdown_timeout_ms")?);
        let camera_config = camera_config(&loaded)?;
        let flip = FlipMode::from_str(setting_text(&loaded, "camera.flip_mode")?)
            .map_err(|_error| "camera flip setting is invalid".to_owned())?;
        let screenshot_format =
            ScreenshotFormat::from_str(setting_text(&loaded, "camera.screenshot_format")?)
                .map_err(|_error| "screenshot format setting is invalid".to_owned())?;
        let jpeg_quality = setting_u8(&loaded, "jpeg_quality")?;
        let screenshot_settings = ScreenshotRuntimeSettings::new(screenshot_format, jpeg_quality)
            .map_err(|_error| "screenshot settings are invalid".to_owned())?;
        let serial_port = setting_text(&loaded, "serial.port")?.to_owned();
        let serial_baud_rate = setting_u32(&loaded, "serial.baud_rate")?;
        let serial_format = ControllerFormat::parse(setting_text(&loaded, "serial.data_format")?)
            .ok_or_else(|| "serial format setting is invalid".to_owned())?;
        let serial_config = if serial_port.is_empty() {
            None
        } else {
            Some(
                SerialConfig::new(serial_port.clone(), serial_baud_rate, serial_format)
                    .map_err(|_error| "serial settings are invalid".to_owned())?,
            )
        };
        let initial_notification_config = notification_config(&raw_values(&loaded))?;
        let camera = CameraManager::start(camera_backend, camera_config.clone(), flip)
            .map_err(|error| format!("camera runtime initialization failed: {error}"))?;
        cleanup.camera = Some(camera.clone());
        let screenshots = ScreenshotService::new(
            camera.frame_source(),
            loaded.roots.data.clone(),
            screenshot_mode,
            screenshot_settings.clone(),
        );

        let serial = SerialManager::new(serial_backend);
        cleanup.serial = Some(serial.clone());
        if let Some(serial_config) = serial_config {
            serial
                .update_config(serial_config)
                .await
                .map_err(|error| format!("serial runtime initialization failed: {error}"))?;
        }

        let notification_transport: Arc<dyn DiscordTransport> = match ReqwestDiscordTransport::new()
        {
            Ok(transport) => Arc::new(transport),
            Err(error) => {
                tracing::warn!(
                    diagnostic_id = "NOTIFICATION_TRANSPORT_UNAVAILABLE",
                    %error,
                    "Discord notifications are unavailable for this process"
                );
                Arc::new(UnavailableDiscordTransport)
            }
        };
        let notifications = Arc::new(NotificationService::new(
            initial_notification_config,
            notification_transport,
            Arc::new(WindowsNativeNotificationTransport),
        ));

        let (realtime_applier, realtime_settings) = RealtimeSettingsApplier::new(&loaded)?;
        let (websocket_applier, websocket_settings) = WebSocketSettingsApplier::new(&loaded)?;
        let (realtime, motion_jpeg, fallback_media_task) = start_media(
            camera.frame_source(),
            screenshot_settings.clone(),
            realtime_settings.clone(),
        )
        .await;
        // Register every already-spawned task before the next fallible build
        // step. If settings/service/router initialization fails below,
        // BuildCleanup must own and abort the media task as well.
        if let Some(task) = fallback_media_task {
            cleanup.tasks.push(task);
        }

        let mut applier = CompositeSettingsApplier::new(&loaded);
        applier.push(HostSettingsApplier::new(Arc::clone(&host)));
        if let Some(settings) = desktop_settings.clone() {
            applier.push(DesktopSettingsApplier::new(settings));
        }
        applier.push(
            CameraSettingsApplier::new(
                camera.clone(),
                camera_config,
                flip,
                screenshot_format,
                jpeg_quality,
                screenshot_settings.clone(),
            )
            .map_err(|error| format!("camera settings adapter initialization failed: {error}"))?,
        );
        applier.push(
            SerialSettingsApplier::new(
                serial.clone(),
                runtime.clone(),
                serial_port,
                serial_baud_rate,
                serial_format,
            )
            .map_err(|error| format!("serial settings adapter initialization failed: {error}"))?,
        );
        applier.push(NotificationSettingsApplier::new(
            Arc::clone(&notifications),
            runtime,
            &loaded,
        )?);
        applier.push(realtime_applier);
        applier.push(websocket_applier);
        let settings = SettingsService::new(loaded.clone(), Box::new(applier));
        let settings_snapshot = initial_settings_snapshot(&settings)?;
        let state_snapshot = initial_state_snapshot(&host, &camera, &serial).await?;
        let hub = StateHub::new(settings_snapshot, state_snapshot, STATE_HISTORY_CAPACITY)
            .map_err(|error| format!("application state initialization failed: {error}"))?;

        let script_ui = ScriptUiCoordinator::new();
        let backend = Arc::new(ApplicationBackend::new(ApplicationBackendParts {
            hub,
            settings,
            host: Arc::clone(&host),
            camera: camera.clone(),
            serial: serial.clone(),
            screenshots,
            notifications: Arc::clone(&notifications),
            dynamic: dynamic.clone(),
            realtime,
            motion_jpeg: Some(motion_jpeg),
            screenshot_mode,
            script_ui: script_ui.clone(),
        }));
        let websocket_backend: Arc<dyn WebSocketBackend> = backend.clone();
        let websocket = WebSocketTransport::new(websocket_backend, WebSocketConfig::default())
            .map_err(|error| format!("WebSocket transport initialization failed: {error}"))?
            .with_heartbeat_settings(websocket_settings);
        let broker = websocket.broker();
        script_ui.install_broker(broker.clone())?;

        let hosts = Arc::new(ProductionScriptHostFactory::new(
            tokio::runtime::Handle::current(),
            Arc::clone(&host),
            serial.clone(),
            camera.clone(),
            screenshot_settings,
            notifications,
            script_ui,
        ));
        let command_root = loaded.roots.data.join("Commands");
        std::fs::create_dir_all(&command_root)
            .map_err(|error| format!("user command root initialization failed: {error}"))?;
        let factory = Arc::new(ManagedUserScriptFactory::new(
            request,
            &loaded,
            command_root,
            hosts,
        ));
        let bridge: Arc<dyn DynamicCommandBridge> = dynamic.map_or_else(
            || Arc::new(StaticCommandBridge::new(Arc::clone(&host))) as Arc<_>,
            |client| client as Arc<_>,
        );
        let commands = Arc::new(CommandService::new(
            Arc::clone(&host),
            factory,
            Arc::clone(&bridge),
        ));
        let profiles = Arc::new(ProfileService::new(
            Arc::clone(&host),
            Arc::clone(&commands),
            bridge,
        ));
        backend.install_command_services(Arc::clone(&commands), profiles)?;

        let rest_backend: Arc<dyn RestBackend> = backend.clone();
        let router = rest::router(rest_backend).merge(websocket.router());
        cleanup.tasks.extend(spawn_device_state_events(
            Arc::clone(&backend),
            &camera,
            &serial,
        ));
        cleanup.tasks.push(spawn_serial_events(&serial, broker));
        cleanup.tasks.push(spawn_runtime_reconciler(
            Arc::clone(&backend),
            host.subscribe_runtime_changes(),
            desktop_settings,
        ));
        cleanup.tasks.push(spawn_dynamic_controller_publisher(
            Arc::clone(&backend),
            host.subscribe_controller_outputs(),
        ));
        cleanup.tasks.push(spawn_command_recompute(
            Arc::clone(&backend),
            Arc::clone(&commands),
            host.subscribe_command_recompute(),
        ));

        Ok(Self {
            router,
            backend,
            commands,
            camera,
            serial,
            tasks: cleanup.take_tasks(),
            script_shutdown_timeout,
            camera_writer_fallback: None,
            camera_reader_fallback: None,
        })
    }

    pub(crate) fn router(&self) -> Router {
        self.router.clone()
    }

    /// Executes shutdown steps 1 through 3 after `AppShutdownPre` has closed
    /// dynamic mutations.
    pub(crate) async fn stop_inputs_camera_and_scripts(&mut self) {
        abort_background_tasks(std::mem::take(&mut self.tasks)).await;
        let controller = {
            let arbiter = self.backend.host().controller_safety().arbiter();
            let mut arbiter = arbiter.lock();
            arbiter.force_release_all();
            arbiter.output()
        };
        send_released_controller_output(&self.serial, controller).await;
        let camera = self.camera.clone();
        match tokio::task::spawn_blocking(move || camera.shutdown(SERVICE_STOP_TIMEOUT)).await {
            Ok(result) => {
                self.camera_writer_fallback = retain_camera_fallback_on_timeout(result);
            }
            Err(error) => {
                tracing::error!(%error, "camera shutdown task failed; retaining mapping until process exit without unmap");
                // The join itself was lost, so the writer state is unknown.
                // Retain a fail-closed guard rather than leaving the mapping
                // unguarded while a writer may still be live.
                if self.camera_writer_fallback.is_none() {
                    self.camera_writer_fallback = Some(self.camera.mark_writer_unstopped());
                }
            }
        }
        let script_shutdown = self.commands.shutdown(self.script_shutdown_timeout).await;
        if let Err(error) = &script_shutdown {
            tracing::error!(%error, "user-script worker shutdown failed");
        }
        let recovered = recover_reader_pins_after_script_shutdown(&self.camera, &script_shutdown);
        if self.camera_reader_fallback.is_none() {
            self.camera_reader_fallback = retain_reader_mapping_after_script_shutdown(
                &self.camera,
                &script_shutdown,
                recovered,
            );
        }
    }

    /// Executes shutdown steps 6 and 7 after the dynamic worker is reaped.
    pub(crate) async fn stop_serial(&self) {
        {
            let arbiter = self.backend.host().controller_safety().arbiter();
            arbiter.lock().force_release_all();
        }
        send_released_controller_output(&self.serial, ControllerState::NEUTRAL).await;
        disconnect_serial_service(&self.serial).await;
    }

    /// Durable §15.6 `camera_writer_unstopped` state for this shutdown
    /// transaction. `true` once the camera writer has missed its deadline,
    /// whether observed here or inside [`CameraManager`].
    pub(crate) fn camera_writer_unstopped(&self) -> bool {
        self.camera.writer_unstopped() || self.camera_writer_fallback.is_some()
    }

    /// §15.6 step 5 gate: shared-memory release is allowed only when the
    /// writer-stopped state is false and the script reader was reaped. A
    /// `false` return forbids unmap; the fallback in
    /// [`ProductionRuntime::camera_mapping_fallback`] applies instead.
    pub(crate) fn shared_memory_release_allowed(&self) -> bool {
        !self.camera_writer_unstopped() && self.camera_reader_fallback.is_none()
    }

    /// §15.6 step 5 fail-closed persistence for the dynamic worker reap.
    ///
    /// Persists an unconfirmed dynamic reap (`dynamic_reaped == false`) as
    /// retained `camera_reader_fallback` ownership **before** the read-only
    /// [`Self::shared_memory_release_gate_after_dynamic_reap`] observation,
    /// so [`Self::shared_memory_release_allowed`] and [`Drop`] refuse
    /// shared-memory release/unmap even though the gate itself only
    /// observes. A confirmed reap, an already-retained fallback, or no
    /// worker at all retains nothing. Idempotent: a repeated call keeps
    /// the first retained mapping.
    pub(crate) fn retain_dynamic_mapping_after_worker_shutdown(&mut self, dynamic_reaped: bool) {
        if dynamic_reaped || self.camera_reader_fallback.is_some() {
            return;
        }
        tracing::error!(
            diagnostic_id = "DYNAMIC_WORKER_UNREAPED",
            mapping = ?self.camera.mapping_descriptor(),
            "dynamic worker was not reaped; retaining camera shared-memory mapping until process exit without unmap"
        );
        self.camera_reader_fallback = Some(self.camera.ring());
    }

    /// §15.6 step 5 read-only release gate, observed after the dynamic
    /// worker is reaped (step 4) and before serial shutdown (steps 6-7).
    ///
    /// Release is reported allowed only when the existing release state is
    /// safe (writer stopped and script reader reaped, via
    /// [`Self::shared_memory_release_allowed`]) **and** the caller confirms
    /// the dynamic worker reap (`dynamic_reaped`). A `false` return forbids
    /// unmap; the fallback in [`Self::camera_mapping_fallback`] applies
    /// instead (retained until process exit; see `Drop`). An unconfirmed
    /// dynamic reap must first be persisted via
    /// [`Self::retain_dynamic_mapping_after_worker_shutdown`]; this gate
    /// never performs that retention itself.
    ///
    /// Read-only: this never releases, unmaps, or retains anything, and the
    /// shutdown caller continues unconditionally either way. The verdict and
    /// the active fallback descriptor are logged for diagnostics.
    pub(crate) fn shared_memory_release_gate_after_dynamic_reap(
        &self,
        dynamic_reaped: bool,
    ) -> bool {
        let allowed = dynamic_reaped && self.shared_memory_release_allowed();
        if allowed {
            tracing::info!(
                "shared-memory release allowed after dynamic worker reap; no fallback retained"
            );
        } else {
            tracing::error!(
                diagnostic_id = "SHARED_MEMORY_RELEASE_REFUSED",
                dynamic_reaped,
                mapping = ?self.camera_mapping_fallback(),
                "shared-memory release refused at step 5; retaining mapping until process exit without unmap"
            );
        }
        allowed
    }

    /// §15.6 steps 5/9 explicit fallback teardown: while either the writer or
    /// external reader is unconfirmed, the Rust main mapping is retained until
    /// OS process exit reclaims it instead of being unmapped. POSIX unlinks
    /// only the name while the existing mapping stays; Windows retains the
    /// mapping handle. `Some` carries the retained mapping descriptor without
    /// claiming normal unmap completion; `None` means no fallback is active.
    pub(crate) fn camera_mapping_fallback(&self) -> Option<MappingDescriptor> {
        self.camera_writer_fallback
            .as_ref()
            .map(UnstoppedCameraWriter::mapping_descriptor)
            .or_else(|| {
                self.camera_reader_fallback
                    .as_ref()
                    .map(SharedFrameRing::descriptor)
            })
    }
}

impl Drop for ProductionRuntime {
    fn drop(&mut self) {
        if self.shared_memory_release_allowed() {
            return;
        }
        let fallback_descriptor = self.camera_mapping_fallback();
        let reader_fallback = self.camera_reader_fallback.take();
        let writer_fallback = self.camera_writer_fallback.take();
        if let Some(mapping) = reader_fallback {
            tracing::error!(
                diagnostic_id = "CAMERA_READER_UNREAPED_OR_UNCONFIRMED",
                mapping = ?mapping.descriptor(),
                "retaining camera shared-memory mapping until process exit because an external reader was not reaped or its pins were unconfirmed"
            );
            // A live external reader may still access the mapping. The process
            // is already on its shutdown path, so intentionally retain the OS
            // mapping until process exit rather than claiming normal unmap.
            std::mem::forget(mapping);
        }
        match writer_fallback {
            Some(writer) => {
                tracing::error!(
                    diagnostic_id = "CAMERA_MAPPING_FALLBACK_RETAINED",
                    mapping = ?fallback_descriptor.unwrap_or_else(|| writer.mapping_descriptor()),
                    "retaining camera shared-memory mapping until process exit because the writer was not reaped"
                );
                // Keep the guard (and its ManagerInner Arc) alive until process
                // exit. Dropping it before CameraManager would detach the writer
                // join handle and could release the mapping while the writer lives.
                std::mem::forget(writer);
            }
            // Durable writer flag with no guard (e.g. the shutdown join itself
            // was lost): retain a fresh guard so the mapping still survives
            // until process exit instead of being released under a live writer.
            None if self.camera.writer_unstopped() => {
                let writer = self.camera.mark_writer_unstopped();
                tracing::error!(
                    diagnostic_id = "CAMERA_MAPPING_FALLBACK_RETAINED",
                    mapping = ?writer.mapping_descriptor(),
                    "retaining camera shared-memory mapping until process exit because the writer was not reaped"
                );
                std::mem::forget(writer);
            }
            None => {}
        }
    }
}

/// Aborts every background task and joins it before its deadline (§15.6
/// step 1). An already-drained task list is a no-op, so a repeated shutdown
/// observes no tasks and stays idempotent.
async fn abort_background_tasks(tasks: Vec<JoinHandle<()>>) {
    for mut task in tasks {
        task.abort();
        if timeout(SERVICE_STOP_TIMEOUT, &mut task).await.is_err() {
            tracing::error!("production shutdown task did not join before its deadline");
        }
    }
}

/// Sends the released controller output without letting a wedged or already
/// disconnected port stall shutdown. Delivery stays best-effort: step 6
/// re-neutralizes and step 7 disconnects even when this send fails.
async fn send_released_controller_output(serial: &SerialManager, controller: ControllerState) {
    let _ = timeout(
        SERVICE_STOP_TIMEOUT,
        serial.send_controller_state(controller),
    )
    .await;
}

/// Executes the shutdown step 6/7 tail: NEUTRAL is sent by the caller, then
/// this disconnects under the fixed deadline. Idempotent: once the connection
/// is gone the manager reports success, and a faulted write still closes the
/// endpoint while the failure stays logged instead of stalling shutdown.
async fn disconnect_serial_service(serial: &SerialManager) {
    match timeout(SERVICE_STOP_TIMEOUT, serial.disconnect()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!(%error, "serial shutdown failed"),
        Err(_) => tracing::error!("serial shutdown timed out"),
    }
}

/// §15.6 step 2 to steps 5/9 propagation: converts a camera shutdown outcome
/// into retained fallback ownership. A timed-out writer is logged with a
/// critical diagnostic and kept alive (mapping plus join handle) so later
/// steps refuse shared-memory release; a stopped writer retains nothing.
fn retain_camera_fallback_on_timeout(
    result: Result<(), UnstoppedCameraWriter>,
) -> Option<UnstoppedCameraWriter> {
    match result {
        Ok(()) => None,
        Err(unstopped) => {
            tracing::error!(
                diagnostic_id = "CAMERA_WRITER_UNSTOPPED",
                mapping = ?unstopped.mapping_descriptor(),
                "camera writer did not stop before its deadline; retaining mapping until process exit without unmap"
            );
            Some(unstopped)
        }
    }
}

/// Recovers abandoned camera reader pins after the user-script worker stops
/// (§15.6 step 3, §7.9.4 worker-crash recovery).
///
/// Only `Ok(Some(_))` authorizes the reset. `UserScriptSession::shutdown`
/// returns `Ok` solely after `ManagedWorker::stop` yields `Ok(StopReport)`,
/// and every `Ok(StopReport)` path follows a completed OS `child.wait()`
/// (the unreaped forced-termination timeout returns `Err`, which propagates
/// here as `Err`). Shutdown additionally holds the profile gate with the
/// session taken, and nothing respawns during application shutdown, so no
/// replacement reader exists yet. `Ok(None)` (no worker was reaped by this
/// shutdown) and `Err(_)` (reap unproven, possibly still live) leave all
/// pins untouched and report `None`.
fn recover_reader_pins_after_script_shutdown(
    camera: &CameraManager,
    outcome: &Result<Option<ScriptSessionStop>, CommandServiceError>,
) -> Option<usize> {
    if outcome.as_ref().ok()?.is_some() {
        match camera.ring().recover_reader_pins(true, true) {
            Ok(recovered) => {
                tracing::info!(
                    recovered,
                    "camera reader pins recovered after script worker exit"
                );
                Some(recovered)
            }
            Err(error) => {
                tracing::error!(%error, "camera reader pin recovery failed after script worker exit");
                None
            }
        }
    } else {
        None
    }
}

/// Retains a mapping when step 3 did not prove that the external reader is
/// gone and its pins are safe to reset. A worker that was never present needs
/// no fallback; a reaped worker with successful recovery permits normal release.
fn retain_reader_mapping_after_script_shutdown(
    camera: &CameraManager,
    outcome: &Result<Option<ScriptSessionStop>, CommandServiceError>,
    recovered: Option<usize>,
) -> Option<SharedFrameRing> {
    let needs_fallback = match outcome {
        Err(_) => true,
        Ok(None) => false,
        Ok(Some(_)) => recovered.is_none(),
    };
    needs_fallback.then(|| camera.ring())
}

async fn start_media(
    frames: crate::camera::LatestFrameSource,
    screenshot_settings: ScreenshotRuntimeSettings,
    runtime_settings: tokio::sync::watch::Receiver<
        crate::server::realtime_connection::RealtimeRuntimeSettings,
    >,
) -> (
    Option<RealtimeConnectionConfig>,
    MotionJpegFeed,
    Option<JoinHandle<()>>,
) {
    let media = WebRtcMedia::new(
        frames.webrtc(),
        frames.motion_jpeg(screenshot_settings.clone()),
        WebRtcMediaConfig::default(),
    )
    .await;
    match media {
        Ok(media) => {
            let motion_jpeg = media.motion_jpeg();
            let initial = runtime_settings.borrow().clone();
            let peer = WebRtcPeerConfig {
                stun_server: initial.stun_server().to_owned(),
                ..WebRtcPeerConfig::default()
            };
            let transport = RealtimeTransportConfig {
                auto_recover: initial.auto_recover(),
                recovery_probe_interval: initial.recovery_probe_interval(),
                ..RealtimeTransportConfig::default()
            };
            match RealtimeConnectionConfig::new(media, peer, transport) {
                Ok(config) => (
                    Some(config.with_runtime_settings(runtime_settings)),
                    motion_jpeg,
                    None,
                ),
                Err(error) => {
                    tracing::warn!(%error, "realtime transport is unavailable; using Motion JPEG");
                    start_fallback_motion_jpeg(&frames, screenshot_settings)
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "WebRTC encoder is unavailable; using Motion JPEG");
            start_fallback_motion_jpeg(&frames, screenshot_settings)
        }
    }
}

fn start_fallback_motion_jpeg(
    frames: &crate::camera::LatestFrameSource,
    screenshot_settings: ScreenshotRuntimeSettings,
) -> (
    Option<RealtimeConnectionConfig>,
    MotionJpegFeed,
    Option<JoinHandle<()>>,
) {
    let feed = MotionJpegFeed::new();
    let published = feed.clone();
    let mut source = frames.motion_jpeg(screenshot_settings);
    let task = tokio::spawn(async move {
        loop {
            let snapshot = match source.changed().await {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) => {
                    published.suspend();
                    continue;
                }
                Err(_) => break,
            };
            let jpeg_source = source.clone();
            match tokio::task::spawn_blocking(move || jpeg_source.encode_jpeg(&snapshot)).await {
                Ok(Ok(jpeg)) => published.publish(jpeg.bytes),
                Ok(Err(error)) => tracing::warn!(%error, "Motion JPEG encoding failed"),
                Err(error) => tracing::warn!(%error, "Motion JPEG encoder task failed"),
            }
        }
    });
    (None, feed, Some(task))
}

fn spawn_serial_events(
    serial: &SerialManager,
    broker: crate::server::websocket::WebSocketBroker,
) -> JoinHandle<()> {
    let mut received = serial.subscribe_received();
    tokio::spawn(async move {
        loop {
            match received.recv().await {
                Ok(bytes) => {
                    broker.publish_serial(SerialData {
                        encoding: SerialEncoding::Base64,
                        data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                        byte_length: bytes.len(),
                    });
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!(count, "serial monitor dropped lagged receive chunks");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

fn spawn_device_state_events(
    backend: Arc<ApplicationBackend>,
    camera: &CameraManager,
    serial: &SerialManager,
) -> Vec<JoinHandle<()>> {
    let mut camera_status = camera.subscribe_status();
    let camera_backend = Arc::clone(&backend);
    let camera_task = tokio::spawn(async move {
        while camera_status.changed().await.is_ok() {
            if let Err(error) = camera_backend
                .reconcile_device_state(StateChangeCause::Camera)
                .await
            {
                tracing::error!(error = ?error.error(), "camera state reconciliation failed");
            }
        }
    });

    let mut serial_status = serial.subscribe_connection_status();
    let serial_backend = backend;
    let serial_task = tokio::spawn(async move {
        while serial_status.changed().await.is_ok() {
            if let Err(error) = serial_backend
                .reconcile_device_state(StateChangeCause::Serial)
                .await
            {
                tracing::error!(error = ?error.error(), "serial state reconciliation failed");
            }
        }
    });

    vec![camera_task, serial_task]
}

fn spawn_runtime_reconciler(
    backend: Arc<ApplicationBackend>,
    mut changes: tokio::sync::watch::Receiver<u64>,
    desktop_settings: Option<DesktopRuntimeSettings>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            if let Some(settings) = desktop_settings.as_ref()
                && let Err(error) =
                    reconcile_desktop_settings(settings, &backend.host().loaded_settings())
            {
                tracing::error!(%error, "desktop settings reconciliation failed");
            }
            if let Err(error) = backend
                .reconcile_host(StateChangeCause::DynamicConfig)
                .await
            {
                tracing::error!(error = ?error.error(), "runtime state reconciliation failed");
            }
        }
    })
}

/// Forwards pending dynamic controller outputs to hardware in arrival order.
///
/// Each wake publishes the authoritative merged arbiter output under the
/// backend mutation gate, so dynamic frames serialize with browser/script
/// mutations. The receiver is marked changed once up front to flush any
/// dynamic output that landed before this subscription (dynamic startup runs
/// before the production runtime subscribes). Later state commits only bump
/// the runtime generation, so the publisher never self-triggers.
fn spawn_dynamic_controller_publisher(
    backend: Arc<ApplicationBackend>,
    mut outputs: tokio::sync::watch::Receiver<u64>,
) -> JoinHandle<()> {
    outputs.mark_changed();
    tokio::spawn(async move {
        while outputs.changed().await.is_ok() {
            if let Err(error) = backend.publish_dynamic_controller().await {
                tracing::error!(error = ?error.error(), "dynamic controller output publication failed");
            }
        }
    })
}

fn spawn_command_recompute(
    backend: Arc<ApplicationBackend>,
    commands: Arc<CommandService>,
    mut changes: tokio::sync::watch::Receiver<u64>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            if let Err(error) = commands.recompute_display_cache().await {
                tracing::warn!(%error, "command display cache recomputation failed");
            }
            if let Err(error) = backend.reconcile_host(StateChangeCause::Commands).await {
                tracing::error!(error = ?error.error(), "command state reconciliation failed");
            }
        }
    })
}

fn camera_config(loaded: &LoadedSettings) -> Result<CameraConfig, String> {
    CameraConfig::new(
        setting_json(loaded, "camera.device")?,
        setting_u32(loaded, "camera.capture_fps")?,
        CaptureResolution::from_str(setting_text(loaded, "camera.capture_resolution")?)
            .map_err(|_error| "camera resolution setting is invalid".to_owned())?,
    )
    .map_err(|_error| "camera settings are invalid".to_owned())
}

fn setting_json<T>(loaded: &LoadedSettings, id: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let value = loaded
        .settings
        .get(id)
        .ok_or_else(|| format!("required setting {id} is missing"))?
        .value
        .clone();
    serde_json::from_value(value).map_err(|_error| format!("setting {id} has an invalid type"))
}

fn setting_text<'a>(loaded: &'a LoadedSettings, id: &str) -> Result<&'a str, String> {
    loaded
        .settings
        .string(id)
        .map_err(|_error| format!("setting {id} has an invalid type"))
}

fn setting_u64(loaded: &LoadedSettings, id: &str) -> Result<u64, String> {
    u64::try_from(
        loaded
            .settings
            .integer(id)
            .map_err(|_error| format!("setting {id} has an invalid type"))?,
    )
    .map_err(|_error| format!("setting {id} is outside its runtime range"))
}

fn setting_u32(loaded: &LoadedSettings, id: &str) -> Result<u32, String> {
    u32::try_from(setting_u64(loaded, id)?)
        .map_err(|_error| format!("setting {id} is outside its runtime range"))
}

fn setting_u8(loaded: &LoadedSettings, id: &str) -> Result<u8, String> {
    u8::try_from(setting_u64(loaded, id)?)
        .map_err(|_error| format!("setting {id} is outside its runtime range"))
}

fn raw_values(loaded: &LoadedSettings) -> std::collections::BTreeMap<String, serde_json::Value> {
    loaded
        .settings
        .values()
        .iter()
        .map(|(id, resolved)| (id.clone(), resolved.value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::{
        abort_background_tasks, disconnect_serial_service,
        recover_reader_pins_after_script_shutdown, retain_camera_fallback_on_timeout,
        retain_reader_mapping_after_script_shutdown, send_released_controller_output,
    };
    use crate::camera::CaptureResolution;
    use crate::camera::MappingDescriptor;
    #[cfg(unix)]
    use crate::camera::SharedFrameRing;
    use crate::camera::backend::CameraConfig;
    use crate::camera::selector::CameraSelector;
    use crate::camera::shared_ring::RingError;
    use crate::camera::virtual_camera::{
        RecordedFrame, VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
    };
    use crate::camera::{CameraManager, FlipMode};
    use crate::command_service::{CommandServiceError, ScriptSessionStop};
    use crate::device::serial::{
        ControllerFormat, SerialConfig, SerialManager, VirtualOpenPlan as SerialOpenPlan,
        VirtualSerialBackend, VirtualSerialEndpoint,
    };
    use crate::device::{ControllerState, StickPosition};
    use serde::{Deserialize, Serialize};

    const CONTENTION_ROUNDS: usize = 5;

    /// Returns a writable artifact path for production evidence tests. Direct
    /// dirty-worktree runs keep the durable Hermes scratch location used by
    /// the traceability documents; sandboxed Nix test runs cannot write into
    /// `/home`, so they use a process-local temporary directory instead.
    fn production_artifact_path(file_name: &str) -> std::path::PathBuf {
        let preferred = std::env::var_os("POKECON_PRODUCTION_ARTIFACT_DIR").map_or_else(
            || {
                std::path::PathBuf::from(
                    "/home/yayoi/.hermes/cache/scratch/ar1129-production-fault",
                )
            },
            std::path::PathBuf::from,
        );
        if std::fs::create_dir_all(&preferred).is_ok() {
            return preferred.join(file_name);
        }
        let fallback = std::env::temp_dir().join("pokecon-ar1129-production-fault");
        std::fs::create_dir_all(&fallback)
            .expect("temporary production artifact directory must exist");
        eprintln!(
            "production artifact directory is not writable; using {}",
            fallback.display()
        );
        fallback.join(file_name)
    }

    fn production_contention_snapshot(
        generation: crate::device::InputGeneration,
        stick: StickPosition,
    ) -> crate::device::InputSnapshot {
        crate::device::InputSnapshot {
            generation,
            sequence: crate::device::InputSequence::zero(),
            keyboard_keys: Vec::new(),
            mouse_buttons: crate::device::MouseButtons::default(),
            buttons: ControllerState::NEUTRAL.buttons,
            hat: ControllerState::NEUTRAL.hat,
            left_stick: stick,
            right_stick: ControllerState::NEUTRAL.right_stick,
            touch: ControllerState::NEUTRAL.touch,
        }
    }

    fn contention_stat(samples: &[u64], percentile: usize) -> u64 {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        if sorted.is_empty() {
            return 0;
        }
        let index = ((sorted.len() - 1) * percentile / 100).min(sorted.len() - 1);
        sorted[index]
    }

    fn manager_with_abandoned_pin() -> CameraManager {
        let backend = VirtualCameraBackend::default();
        backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([4, 5, 6])],
        )));
        let manager = CameraManager::start(
            Arc::new(backend),
            CameraConfig::new(CameraSelector::Index(0), 30, CaptureResolution::R640x360).unwrap(),
            FlipMode::None,
        )
        .unwrap();
        manager
            .ring()
            .pin_current_for_diagnostics()
            .unwrap()
            .unwrap()
            .abandon_for_crash_simulation();
        manager
    }

    #[test]
    fn confirmed_worker_exit_resets_stale_reader_pin() {
        let manager = manager_with_abandoned_pin();
        let outcome: Result<Option<ScriptSessionStop>, CommandServiceError> =
            Ok(Some(ScriptSessionStop { forced: false }));
        assert_eq!(
            recover_reader_pins_after_script_shutdown(&manager, &outcome),
            Some(1)
        );
        assert_eq!(manager.ring().recover_reader_pins(true, true).unwrap(), 0);
    }

    #[test]
    fn unconfirmed_or_absent_worker_exit_preserves_reader_pin() {
        for outcome in [Err(CommandServiceError::ProfileSwitchGateNotHeld), Ok(None)] {
            let manager = manager_with_abandoned_pin();
            assert_eq!(
                recover_reader_pins_after_script_shutdown(&manager, &outcome),
                None
            );
            assert_eq!(
                manager.ring().pin_current_for_diagnostics().unwrap_err(),
                RingError::ReaderAlreadyPinned
            );
        }
    }

    #[test]
    fn unconfirmed_reader_shutdown_retains_mapping_until_process_exit() {
        let manager = manager_with_abandoned_pin();
        let descriptor = manager.mapping_descriptor();
        let unreaped: Result<Option<ScriptSessionStop>, CommandServiceError> =
            Err(CommandServiceError::ProfileSwitchGateNotHeld);
        let fallback = retain_reader_mapping_after_script_shutdown(&manager, &unreaped, None)
            .expect("an unreaped reader must retain the mapping");
        assert_eq!(fallback.descriptor(), descriptor);
        drop(fallback);

        let absent: Result<Option<ScriptSessionStop>, CommandServiceError> = Ok(None);
        assert!(retain_reader_mapping_after_script_shutdown(&manager, &absent, None).is_none());

        let reaped: Result<Option<ScriptSessionStop>, CommandServiceError> =
            Ok(Some(ScriptSessionStop { forced: true }));
        assert!(retain_reader_mapping_after_script_shutdown(&manager, &reaped, Some(1)).is_none());
        assert!(retain_reader_mapping_after_script_shutdown(&manager, &reaped, None).is_some());
    }

    #[test]
    fn camera_shutdown_timeout_is_retained_as_fallback() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([4, 5, 6]), RecordedFrame::Hang],
        )));
        let manager = CameraManager::start(
            Arc::new(backend),
            CameraConfig::new(CameraSelector::Index(0), 30, CaptureResolution::R640x360).unwrap(),
            FlipMode::None,
        )
        .unwrap();
        for _ in 0..1_000 {
            if manager.ring().read_published().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(50));
        // This is the exact outcome type `stop_inputs_camera_and_scripts`
        // feeds into the fallback helper via `spawn_blocking`.
        let outcome = manager.shutdown(Duration::from_millis(20));
        assert!(outcome.is_err());
        let fallback = retain_camera_fallback_on_timeout(outcome);
        let guard = fallback.expect("timeout must be retained");
        assert!(guard.writer_unstopped());
        assert_eq!(guard.mapping_descriptor(), manager.mapping_descriptor());
        // The retained guard keeps the mapping observable instead of
        // claiming any unmap completion.
        assert!(manager.ring().read_published().unwrap().is_some());
        drop(guard);
    }

    #[test]
    fn camera_shutdown_success_retains_no_fallback() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([4, 5, 6])],
        )));
        let manager = CameraManager::start(
            Arc::new(backend),
            CameraConfig::new(CameraSelector::Index(0), 30, CaptureResolution::R640x360).unwrap(),
            FlipMode::None,
        )
        .unwrap();
        let outcome = manager.shutdown(Duration::from_secs(1));
        assert!(outcome.is_ok());
        assert!(retain_camera_fallback_on_timeout(outcome).is_none());
        assert!(!manager.writer_unstopped());
    }

    fn deflected_controller() -> ControllerState {
        let mut state = ControllerState::NEUTRAL;
        state.buttons.a = true;
        state
    }

    async fn connected_serial_manager() -> (SerialManager, VirtualSerialEndpoint) {
        let backend = VirtualSerialBackend::default();
        let endpoint = VirtualSerialEndpoint::new();
        backend
            .push_plan(SerialOpenPlan::Success(endpoint.clone()))
            .await;
        let manager = SerialManager::new(Arc::new(backend));
        manager
            .apply_config(SerialConfig::new("virtual", 9600, ControllerFormat::Default).unwrap())
            .await
            .expect("virtual serial port opens");
        (manager, endpoint)
    }

    #[tokio::test]
    async fn background_task_abort_stops_hung_tasks_and_tolerates_empty_repeats() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let hung_completed = Arc::new(AtomicBool::new(false));
        let hung_flag = Arc::clone(&hung_completed);
        let hung = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_mins(1)).await;
            hung_flag.store(true, Ordering::Release);
        });
        let quick_completed = Arc::new(AtomicBool::new(false));
        let quick_flag = Arc::clone(&quick_completed);
        let quick = tokio::spawn(async move {
            quick_flag.store(true, Ordering::Release);
        });
        // This is the exact helper `stop_inputs_camera_and_scripts` step 1 calls.
        abort_background_tasks(vec![hung, quick]).await;
        assert!(
            !hung_completed.load(Ordering::Acquire),
            "a hung background task must be aborted rather than joined to completion"
        );
        assert!(
            quick_completed.load(Ordering::Acquire),
            "an already-finished task must join cleanly"
        );
        // Repeated shutdown observes a drained task list and stays a no-op.
        abort_background_tasks(Vec::new()).await;
    }

    #[tokio::test]
    async fn released_controller_output_reaches_connected_serial() {
        let (manager, endpoint) = connected_serial_manager().await;
        // This is the exact helper both production shutdown methods call for
        // the force-released controller output (steps 1 and 6).
        send_released_controller_output(&manager, deflected_controller()).await;
        assert!(
            !endpoint.written().await.is_empty(),
            "released output must reach the wire while connected"
        );
        assert!(!endpoint.is_closed());
    }

    #[tokio::test]
    async fn released_send_after_disconnect_fails_closed_without_stalling() {
        let (manager, endpoint) = connected_serial_manager().await;
        disconnect_serial_service(&manager).await;
        assert!(endpoint.is_closed());
        // Production ignores the send outcome; the manager itself must refuse it.
        send_released_controller_output(&manager, deflected_controller()).await;
        assert!(
            manager
                .send_controller_state(deflected_controller())
                .await
                .is_err(),
            "sends after disconnect must fail closed"
        );
    }

    #[tokio::test]
    async fn serial_disconnect_appends_neutral_and_is_idempotent() {
        let (manager, endpoint) = connected_serial_manager().await;
        manager
            .send_controller_state(deflected_controller())
            .await
            .expect("deflected frame is accepted while connected");
        let sent = endpoint.written().await.len();
        assert!(sent > 0);
        // This is the exact helper `stop_serial` step 7 calls.
        disconnect_serial_service(&manager).await;
        let after_first = endpoint.written().await.len();
        assert!(
            after_first > sent,
            "disconnect must append the neutral frame after prior output"
        );
        assert!(endpoint.is_closed());
        disconnect_serial_service(&manager).await;
        assert_eq!(
            endpoint.written().await.len(),
            after_first,
            "repeated disconnect must not emit extra bytes"
        );
        assert!(endpoint.is_closed());
    }

    #[tokio::test]
    async fn serial_disconnect_write_fault_still_closes_endpoint() {
        let (manager, endpoint) = connected_serial_manager().await;
        endpoint
            .fail_next_write(std::io::ErrorKind::BrokenPipe)
            .await;
        // The failure stays logged inside the helper; shutdown must not stall
        // and the endpoint must still reach its closed terminal state.
        disconnect_serial_service(&manager).await;
        assert!(endpoint.is_closed());
        assert!(
            manager
                .send_controller_state(deflected_controller())
                .await
                .is_err(),
            "sends after a faulted disconnect must fail closed"
        );
    }

    /// AR-11-26/AR-11-29 slice: the real [`super::ProductionRuntime`]
    /// composition root on virtual backends with a real
    /// [`SettingsPipeline`]-loaded startup snapshot, then production-owned
    /// shutdown ordering and idempotence.
    ///
    /// This is not an independent mock lifecycle: settings travel the real
    /// pipeline (`load_before_dynamic` plus the real [`StartupDynamicHost`]
    /// `finish_startup`), construction runs the single production
    /// `build_inner` via [`super::ProductionRuntime::build_with_backends`],
    /// the serial port opens through the production-wired `SerialManager`
    /// (the same `connect` the UI connect action calls), and shutdown runs
    /// the exact production methods in production order
    /// (`stop_inputs_camera_and_scripts` then `stop_serial`), repeated to
    /// prove idempotence.
    ///
    /// The same fixture also invokes the native constructor below; the
    /// virtual-backend run is retained for deterministic successful camera and
    /// serial I/O. Remaining limitation: no live-hardware coverage and no
    /// forced native writer-timeout injection; the timeout-retention path is
    /// covered by the virtual-backend helper tests above.
    ///
    /// Build-failure rollback is covered separately below so this successful
    /// lifecycle fixture stays focused on the terminal production sequence.
    #[allow(clippy::too_many_lines)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_runtime_with_virtual_backends_shuts_down_in_production_order() {
        use std::collections::BTreeMap;
        use std::ffi::OsString;

        use tempfile::TempDir;

        use crate::camera::ScreenshotMode;
        use crate::dynamic_host::StartupDynamicHost;
        use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
        use crate::settings::roots::{BaseDirectories, RootEnvironment};

        // Isolated temporary roots with registry defaults: every setting
        // below comes from the real pipeline, never from hand-built state.
        let temporary = TempDir::new().expect("isolated roots must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("isolated base directories must resolve");
        std::fs::create_dir_all(base.join("config/pokecon/profiles/default"))
            .expect("isolated default profile must exist");
        // `--serial-port virtual` travels the real CLI layer so the
        // composition root stores a serial config; disconnected ports stay
        // closed until an explicit connect, exactly as in production.
        let request = PipelineRequest {
            arguments: ["pokecon", "--serial-port", "virtual"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let before_dynamic = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("isolated pipeline must load the startup snapshot");
        let host = Arc::new(
            StartupDynamicHost::new(request.clone(), before_dynamic)
                .expect("startup host must accept the pipeline snapshot"),
        );
        let loaded = host
            .finish_startup()
            .expect("startup host must finish the real pipeline");
        assert_eq!(
            loaded
                .settings
                .string("serial.port")
                .expect("serial.port must resolve"),
            "virtual",
            "the CLI value must survive the real pipeline, not test scaffolding"
        );

        // Exercise the production entry point itself with the same real
        // settings pipeline. The isolated runner has no camera device, so the
        // native camera manager remains closed by its normal fail-closed
        // startup status; the serial selector is stored but not connected.
        // This proves the native constructor path and its bounded shutdown
        // without pretending that hardware I/O was measured here.
        let native_before_dynamic = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("native fixture pipeline must load the startup snapshot");
        let native_host = Arc::new(
            StartupDynamicHost::new(request.clone(), native_before_dynamic)
                .expect("native fixture host must accept the pipeline snapshot"),
        );
        let native_loaded = native_host
            .finish_startup()
            .expect("native fixture host must finish the real pipeline");
        let mut native_runtime = super::ProductionRuntime::build(
            request.clone(),
            native_loaded,
            native_host,
            None,
            ScreenshotMode::Web,
            None,
        )
        .await
        .expect("native production composition root must build without hardware");
        assert!(!native_runtime.serial.is_connected().await);
        assert!(!native_runtime.camera_writer_unstopped());
        native_runtime.stop_inputs_camera_and_scripts().await;
        assert!(native_runtime.shared_memory_release_gate_after_dynamic_reap(true));
        native_runtime.stop_serial().await;
        assert!(!native_runtime.camera_writer_unstopped());
        assert!(native_runtime.shared_memory_release_allowed());

        let camera_backend = Arc::new(VirtualCameraBackend::default());
        camera_backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            60,
            [RecordedFrame::Solid([7, 8, 9])],
        )));
        let serial_backend = Arc::new(VirtualSerialBackend::default());
        let endpoint = VirtualSerialEndpoint::new();
        serial_backend
            .push_plan(SerialOpenPlan::Success(endpoint.clone()))
            .await;

        // The single production composition root, with virtual backends.
        let mut runtime = super::ProductionRuntime::build_with_backends(
            request,
            loaded,
            host,
            None,
            ScreenshotMode::Web,
            None,
            camera_backend,
            serial_backend.clone(),
        )
        .await
        .expect("production composition root must build on virtual backends");

        // Construction reached the real backend/router/settings wiring.
        let _router = runtime.router();
        let _mapping = runtime.camera.mapping_descriptor();
        assert!(!runtime.camera_writer_unstopped());
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());
        // §15.6 step 5 gate on the clean path: dynamic reap confirmed and
        // no fallback retained, so release is allowed with no fallback.
        assert!(runtime.shared_memory_release_gate_after_dynamic_reap(true));
        // The gate is read-only: an unconfirmed dynamic reap refuses
        // release without retaining anything or changing shutdown state.
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(false));
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());
        // The confirmed-reap persistence is a no-op on the clean path:
        // nothing is retained and release stays allowed.
        runtime.retain_dynamic_mapping_after_worker_shutdown(true);
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());
        let stored = runtime
            .serial
            .current_config()
            .await
            .expect("composition root must store the pipeline serial config");
        assert_eq!(stored.selector.as_str(), "virtual");
        assert!(
            !runtime.serial.is_connected().await,
            "startup must store the serial config without opening the port"
        );
        assert_eq!(serial_backend.attempt_count(), 0);

        // Explicit connect through the production-wired manager (the same
        // `SerialManager::connect` the UI connect action calls).
        runtime
            .serial
            .connect()
            .await
            .expect("virtual serial must open");
        assert!(runtime.serial.is_connected().await);
        assert_eq!(serial_backend.attempt_count(), 1);
        assert_eq!(
            serial_backend.opened_selectors().await,
            vec![("virtual".to_owned(), 9600)]
        );

        // AR-11-02/AR-11-05 production-owned contention slice: use the
        // process-wide arbiter that the composition root gives to dynamic,
        // script, and browser paths. Two concurrent source tasks contend on
        // the real parking_lot mutex, then the merged output traverses the
        // production-wired SerialManager. This is deliberately separate from
        // the test-only bounded queue model in controller_serial_contract.rs.
        let production_arbiter = runtime.backend.host().controller_safety().arbiter();
        let source_specs = [
            (
                "production-hardware",
                crate::device::InputSourceKind::HardwareController,
                crate::device::InputPriority::HARDWARE_CONTROLLER,
                StickPosition { x: 64, y: 128 },
            ),
            (
                "production-user-script",
                crate::device::InputSourceKind::UserScript,
                crate::device::InputPriority::USER_SCRIPT,
                StickPosition { x: 192, y: 128 },
            ),
            (
                "production-keyboard",
                crate::device::InputSourceKind::Keyboard,
                crate::device::InputPriority::KEYBOARD_MOUSE,
                StickPosition { x: 32, y: 128 },
            ),
            (
                "production-mouse",
                crate::device::InputSourceKind::Mouse,
                crate::device::InputPriority::KEYBOARD_MOUSE,
                StickPosition { x: 48, y: 128 },
            ),
            (
                "production-browser-gamepad",
                crate::device::InputSourceKind::BrowserGamepad,
                crate::device::InputPriority::BROWSER_GAMEPAD,
                StickPosition { x: 96, y: 128 },
            ),
            (
                "production-dynamic-config",
                crate::device::InputSourceKind::DynamicConfig,
                crate::device::InputPriority::DYNAMIC_CONFIG,
                StickPosition { x: 160, y: 128 },
            ),
        ];
        // Before comparison: with equal priorities, the historical arrival-order
        // tie-breaker chooses whichever source updates last. Reversing arrival
        // order must therefore reverse the winner; this is the baseline that
        // explicit priority is required to improve, not a claim about an old
        // binary release.
        let mut baseline_arbiter = production_arbiter.lock().clone();
        let mut baseline_winners = Vec::new();
        for (round, arrival_order) in [[0_usize, 1_usize], [1, 0]].into_iter().enumerate() {
            baseline_arbiter.force_release_all();
            for index in arrival_order {
                let (name, kind, _priority, stick) = source_specs[index];
                let source =
                    crate::device::InputSourceId::new(format!("production-before-{round}-{name}"))
                        .expect("production baseline source id must be valid");
                let generation = crate::device::InputGeneration::new(format!(
                    "production-before-generation-{round}-{index}"
                ))
                .expect("production baseline generation must be valid");
                baseline_arbiter.begin_generation(
                    source.clone(),
                    kind,
                    crate::device::InputPriority::USER_SCRIPT,
                    generation.clone(),
                );
                let (result, acknowledgement) = baseline_arbiter
                    .apply_snapshot(&source, production_contention_snapshot(generation, stick))
                    .expect("production baseline snapshot must validate");
                assert_eq!(result, crate::device::ApplyResult::Applied);
                assert!(acknowledgement.is_some());
            }
            baseline_winners.push(baseline_arbiter.output().left_stick.x);
        }
        assert_ne!(
            baseline_winners[0], baseline_winners[1],
            "same-priority arrival-order baseline must expose winner inversion"
        );
        let baseline_winner_changes_with_arrival_order = baseline_winners[0] != baseline_winners[1];
        let mut before_lock_wait_nanos = Vec::new();
        let mut before_apply_nanos = Vec::new();
        let mut before_winners = Vec::new();
        let mut before_starvation_count = 0_usize;
        for round in 0..CONTENTION_ROUNDS {
            production_arbiter.lock().force_release_all();
            let barrier = Arc::new(tokio::sync::Barrier::new(source_specs.len() + 1));
            let mut tasks = Vec::with_capacity(source_specs.len());
            for (index, (name, kind, _priority, stick)) in source_specs.iter().copied().enumerate()
            {
                let arbiter = Arc::clone(&production_arbiter);
                let barrier = Arc::clone(&barrier);
                let source = crate::device::InputSourceId::new(format!(
                    "production-before-{round}-{index}-{name}"
                ))
                .expect("production before source id must be valid");
                let generation = crate::device::InputGeneration::new(format!(
                    "production-before-timing-{round}-{index}"
                ))
                .expect("production before generation must be valid");
                let snapshot = production_contention_snapshot(generation.clone(), stick);
                tasks.push(tokio::spawn(async move {
                    barrier.wait().await;
                    let wait_started = Instant::now();
                    let mut arbiter = arbiter.lock();
                    let lock_wait =
                        u64::try_from(wait_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    let apply_started = Instant::now();
                    arbiter.begin_generation(
                        source.clone(),
                        kind,
                        crate::device::InputPriority::USER_SCRIPT,
                        generation,
                    );
                    let (result, acknowledgement) = arbiter
                        .apply_snapshot(&source, snapshot)
                        .expect("production before snapshot must validate");
                    let apply =
                        u64::try_from(apply_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    (result, acknowledgement.is_some(), lock_wait, apply)
                }));
            }
            barrier.wait().await;
            for task in tasks {
                let (result, acknowledged, lock_wait, apply) =
                    task.await.expect("production before task must join");
                if result != crate::device::ApplyResult::Applied || !acknowledged {
                    before_starvation_count += 1;
                }
                assert_eq!(
                    result,
                    crate::device::ApplyResult::Applied,
                    "every before source must be serviced in round {round}"
                );
                assert!(acknowledged, "every before snapshot must be acknowledged");
                before_lock_wait_nanos.push(lock_wait);
                before_apply_nanos.push(apply);
            }
            before_winners.push(production_arbiter.lock().output().left_stick.x);
        }
        let mut lock_wait_nanos = Vec::new();
        let mut apply_nanos = Vec::new();
        let mut high_priority_wins = 0_usize;
        let mut starvation_count = 0_usize;
        for round in 0..CONTENTION_ROUNDS {
            production_arbiter.lock().force_release_all();
            let barrier = Arc::new(tokio::sync::Barrier::new(source_specs.len() + 1));
            let mut tasks = Vec::with_capacity(source_specs.len());
            for (index, (name, kind, priority, stick)) in source_specs.iter().copied().enumerate() {
                let arbiter = Arc::clone(&production_arbiter);
                let barrier = Arc::clone(&barrier);
                let source = crate::device::InputSourceId::new(name)
                    .expect("production contention source id must be valid");
                let generation = crate::device::InputGeneration::new(format!(
                    "production-contention-{round}-{index}"
                ))
                .expect("production contention generation must be valid");
                let snapshot = production_contention_snapshot(generation.clone(), stick);
                tasks.push(tokio::spawn(async move {
                    barrier.wait().await;
                    let wait_started = Instant::now();
                    let mut arbiter = arbiter.lock();
                    let lock_wait =
                        u64::try_from(wait_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    let apply_started = Instant::now();
                    arbiter.begin_generation(source.clone(), kind, priority, generation);
                    let (result, acknowledgement) = arbiter
                        .apply_snapshot(&source, snapshot)
                        .expect("production contention snapshot must validate");
                    let apply =
                        u64::try_from(apply_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    (index, result, acknowledgement.is_some(), lock_wait, apply)
                }));
            }
            barrier.wait().await;
            for task in tasks {
                let (_index, result, acknowledged, lock_wait, apply) =
                    task.await.expect("production contention task must join");
                if result != crate::device::ApplyResult::Applied || !acknowledged {
                    starvation_count += 1;
                }
                assert_eq!(
                    result,
                    crate::device::ApplyResult::Applied,
                    "every production source must be serviced in round {round}"
                );
                assert!(
                    acknowledged,
                    "every production snapshot must be acknowledged"
                );
                lock_wait_nanos.push(lock_wait);
                apply_nanos.push(apply);
            }
            assert_eq!(
                production_arbiter.lock().output().left_stick,
                source_specs[1].3,
                "explicit production priority must win regardless of lock arrival order"
            );
            high_priority_wins += 1;
        }
        assert_eq!(starvation_count, 0);
        assert_eq!(high_priority_wins, CONTENTION_ROUNDS);
        assert_eq!(before_starvation_count, 0);
        let before_lock_wait_mean = u64::try_from(
            before_lock_wait_nanos
                .iter()
                .map(|sample| u128::from(*sample))
                .sum::<u128>()
                / u128::try_from(before_lock_wait_nanos.len()).unwrap(),
        )
        .unwrap_or(u64::MAX);
        let before_apply_mean = u64::try_from(
            before_apply_nanos
                .iter()
                .map(|sample| u128::from(*sample))
                .sum::<u128>()
                / u128::try_from(before_apply_nanos.len()).unwrap(),
        )
        .unwrap_or(u64::MAX);
        let lock_wait_mean = u64::try_from(
            lock_wait_nanos
                .iter()
                .map(|sample| u128::from(*sample))
                .sum::<u128>()
                .div_ceil(u128::try_from(lock_wait_nanos.len()).unwrap()),
        )
        .unwrap_or(u64::MAX);
        let apply_mean = u64::try_from(
            apply_nanos
                .iter()
                .map(|sample| u128::from(*sample))
                .sum::<u128>()
                .div_ceil(u128::try_from(apply_nanos.len()).unwrap()),
        )
        .unwrap_or(u64::MAX);
        let before_lock_wait_p50 = contention_stat(&before_lock_wait_nanos, 50);
        let before_lock_wait_p95 = contention_stat(&before_lock_wait_nanos, 95);
        let before_lock_wait_max = before_lock_wait_nanos.iter().copied().max().unwrap_or(0);
        let before_apply_p50 = contention_stat(&before_apply_nanos, 50);
        let before_apply_p95 = contention_stat(&before_apply_nanos, 95);
        let before_apply_max = before_apply_nanos.iter().copied().max().unwrap_or(0);
        let lock_wait_p50 = contention_stat(&lock_wait_nanos, 50);
        let lock_wait_p95 = contention_stat(&lock_wait_nanos, 95);
        let lock_wait_max = lock_wait_nanos.iter().copied().max().unwrap_or(0);
        let apply_p50 = contention_stat(&apply_nanos, 50);
        let apply_p95 = contention_stat(&apply_nanos, 95);
        let apply_max = apply_nanos.iter().copied().max().unwrap_or(0);
        let contention_state = production_arbiter.lock().output();
        runtime
            .serial
            .send_controller_state(contention_state)
            .await
            .expect("production contention winner must reach serial");
        let serial_bytes = endpoint.written().await.len();
        assert!(
            serial_bytes > 0,
            "production contention must reach the virtual wire"
        );
        if let Ok(output) = std::env::var("POKECON_PRODUCTION_CONTENTION_REPORT_OUT")
            && !output.is_empty()
        {
            let report = serde_json::json!({
                "schema": "production-priority-contention/1",
                "fixture": "production-runtime-shared-arbiter",
                "scope_note": "ProductionRuntime composition root, shared InputArbiter mutex, explicit priority, bounded concurrent source tasks, and serial output. The separate controller_serial_contract queue model remains test-only; timing is advisory.",
                "rounds": CONTENTION_ROUNDS,
                "sources": [
                    {"name": "production-hardware", "kind": "HardwareController", "priority": 300},
                    {"name": "production-user-script", "kind": "UserScript", "priority": 500},
                    {"name": "production-keyboard", "kind": "Keyboard", "priority": 100},
                    {"name": "production-mouse", "kind": "Mouse", "priority": 100},
                    {"name": "production-browser-gamepad", "kind": "BrowserGamepad", "priority": 200},
                    {"name": "production-dynamic-config", "kind": "DynamicConfig", "priority": 400},
                ],
                "before": {
                    "policy": "same-priority arrival-order baseline (no explicit precedence)",
                    "winner_left_stick_x": baseline_winners,
                    "timing_winner_left_stick_x": before_winners,
                    "winner_changes_with_arrival_order": baseline_winner_changes_with_arrival_order,
                    "starvation_count": before_starvation_count,
                    "timing": {
                        "lock_wait_samples": before_lock_wait_nanos.len(),
                        "lock_wait_mean_nanos": before_lock_wait_mean,
                        "lock_wait_p50_nanos": before_lock_wait_p50,
                        "lock_wait_p95_nanos": before_lock_wait_p95,
                        "lock_wait_max_nanos": before_lock_wait_max,
                        "apply_samples": before_apply_nanos.len(),
                        "apply_mean_nanos": before_apply_mean,
                        "apply_p50_nanos": before_apply_p50,
                        "apply_p95_nanos": before_apply_p95,
                        "apply_max_nanos": before_apply_max,
                    },
                },
                "after": {
                    "policy": "production InputArbiter explicit priority (priority, update_order, source_id)",
                    "high_priority_wins": high_priority_wins,
                    "starvation_count": starvation_count,
                    "timing": {
                        "lock_wait_samples": lock_wait_nanos.len(),
                        "lock_wait_mean_nanos": lock_wait_mean,
                        "lock_wait_p50_nanos": lock_wait_p50,
                        "lock_wait_p95_nanos": lock_wait_p95,
                        "lock_wait_max_nanos": lock_wait_max,
                        "apply_samples": apply_nanos.len(),
                        "apply_mean_nanos": apply_mean,
                        "apply_p50_nanos": apply_p50,
                        "apply_p95_nanos": apply_p95,
                        "apply_max_nanos": apply_max,
                    },
                },
                "contention": {
                    "lock_wait_samples": lock_wait_nanos.len(),
                    "lock_wait_mean_nanos": lock_wait_mean,
                    "apply_samples": apply_nanos.len(),
                    "apply_mean_nanos": apply_mean,
                    "advisory_only": true,
                },
                "serial": {"wire_bytes": serial_bytes},
                "build": {
                    "crate": env!("CARGO_PKG_NAME"),
                    "version": env!("CARGO_PKG_VERSION"),
                    "github_sha": std::env::var("GITHUB_SHA").ok().filter(|item| !item.is_empty()),
                },
            });
            let path = std::path::Path::new(&output);
            if let Some(parent) = path.parent().filter(|item| !item.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).expect("contention report directory must exist");
            }
            std::fs::write(path, serde_json::to_string_pretty(&report).unwrap())
                .expect("contention report must be written");
        }
        // AR-11-05 adoption decision: test-only record derived from this
        // fixture's computed values (no hard-coded numbers, no production
        // behavior change). Emitted only on request, like the report above.
        if let Ok(output) = std::env::var("POKECON_PRODUCTION_CONTENTION_ADOPTION_REPORT_OUT")
            && !output.is_empty()
        {
            let lock_wait_p95_improved = lock_wait_p95 < before_lock_wait_p95;
            let apply_p95_improved = apply_p95 < before_apply_p95;
            let decision_rationale = format!(
                "DEFER: explicit priority shows fairness (high-priority wins {high_priority_wins}/{CONTENTION_ROUNDS}, starvation {starvation_count}) but this fixture shows no consistent latency improvement across both metrics: after lock-wait p95 {lock_wait_p95}ns vs before {before_lock_wait_p95}ns (improved: {lock_wait_p95_improved}), after apply p95 {apply_p95}ns vs before {before_apply_p95}ns (improved: {apply_p95_improved}). Jitter and real WebSocket queue stall are unmeasured, so the mechanism is not adopted as a latency/jitter improvement regardless of this run's advisory latency samples. Fairness evidence is kept separate from performance adoption."
            );
            let adoption = serde_json::json!({
                "schema": "production-contention-adoption-record/1",
                "fixture": "production-runtime-shared-arbiter",
                "scope_note": "AR-11-05 adoption decision for the production-owned InputArbiter contention slice. The test-only bounded queue model in controller_serial_contract.rs is a separate fairness artifact, not production throughput evidence.",
                "sources": [
                    {"name": "production-hardware", "kind": "HardwareController", "priority": 300},
                    {"name": "production-user-script", "kind": "UserScript", "priority": 500},
                    {"name": "production-keyboard", "kind": "Keyboard", "priority": 100},
                    {"name": "production-mouse", "kind": "Mouse", "priority": 100},
                    {"name": "production-browser-gamepad", "kind": "BrowserGamepad", "priority": 200},
                    {"name": "production-dynamic-config", "kind": "DynamicConfig", "priority": 400},
                ],
                "rounds": CONTENTION_ROUNDS,
                "before": {
                    "policy": "same-priority arrival-order baseline (no explicit precedence)",
                    "lock_wait_p50_p95_max_nanos": [before_lock_wait_p50, before_lock_wait_p95, before_lock_wait_max],
                    "apply_p50_p95_max_nanos": [before_apply_p50, before_apply_p95, before_apply_max],
                    "winner_changes_with_arrival_order": baseline_winner_changes_with_arrival_order,
                    "starvation_count": before_starvation_count,
                },
                "after": {
                    "policy": "production InputArbiter explicit priority (priority, update_order, source_id)",
                    "high_priority_wins": high_priority_wins,
                    "starvation_count": starvation_count,
                    "lock_wait_p50_p95_max_nanos": [lock_wait_p50, lock_wait_p95, lock_wait_max],
                    "apply_p50_p95_max_nanos": [apply_p50, apply_p95, apply_max],
                    "serial_wire_bytes": serial_bytes,
                },
                "jitter_comparison": {
                    "status": "unmeasured",
                    "note": "This fixture collects lock-wait/apply latency samples only; jitter was not measured before/after, so no jitter improvement is claimed.",
                },
                "queue_stall_comparison": {
                    "status": "unmeasured",
                    "note": "Real WebSocket queue backpressure was not exercised here; the bounded mpsc queue in controller_serial_contract.rs is a separate test-only model and is not production queue-stall evidence.",
                },
                "decision": "DEFER",
                "decision_rationale": decision_rationale,
                "advisory_only": true,
                "build": {
                    "crate": env!("CARGO_PKG_NAME"),
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "local_only_note": "Dirty-local virtual-backend evidence only; not clean-source or remote-CI evidence and not a remote performance claim.",
            });
            let path = std::path::Path::new(&output);
            if let Some(parent) = path.parent().filter(|item| !item.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).expect("adoption report directory must exist");
            }
            std::fs::write(path, serde_json::to_string_pretty(&adoption).unwrap())
                .expect("adoption report must be written");
        }
        runtime
            .serial
            .send_controller_state(deflected_controller())
            .await
            .expect("deflected frame must reach the virtual wire");
        assert!(!endpoint.written().await.is_empty());
        assert!(!endpoint.is_closed());
        let wired_bytes = endpoint.written().await.len();

        // Production shutdown order, then a repeated shutdown proving
        // idempotence.
        runtime.stop_inputs_camera_and_scripts().await;
        // §15.6 step 5 in production order: after script/dynamic reap (the
        // dynamic half is confirmed by the test driver here, as
        // `shutdown_production` does after `shutdown_worker`), before serial.
        assert!(runtime.shared_memory_release_gate_after_dynamic_reap(true));
        runtime.stop_serial().await;
        runtime.stop_inputs_camera_and_scripts().await;
        runtime.stop_serial().await;

        // Terminal state: writer stopped with no shared-memory fallback,
        // serial neutralized and closed, further sends fail closed.
        assert!(!runtime.camera_writer_unstopped());
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());
        assert!(!runtime.serial.is_connected().await);
        assert!(endpoint.is_closed());
        assert!(
            endpoint.written().await.len() > wired_bytes,
            "production shutdown must append neutral frame(s) after prior output"
        );
        assert!(
            runtime
                .serial
                .send_controller_state(deflected_controller())
                .await
                .is_err(),
            "sends after production shutdown must fail closed"
        );

        // §15.6 step 5 fail-closed persistence at production level: the
        // persistence step (what `shutdown_production` calls after
        // `shutdown_worker`) retains an unconfirmed dynamic reap as reader
        // fallback ownership, so release is refused and the mapping stays
        // retained even though the gate itself only observes.
        runtime.retain_dynamic_mapping_after_worker_shutdown(true);
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());
        runtime.retain_dynamic_mapping_after_worker_shutdown(false);
        assert!(!runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_some());
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(false));
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(true));
        assert!(runtime.camera_mapping_fallback().is_some());
        // A repeated unconfirmed reap keeps the retained mapping
        // (idempotent persistence, first mapping wins).
        runtime.retain_dynamic_mapping_after_worker_shutdown(false);
        assert!(runtime.camera_mapping_fallback().is_some());

        // JoinError/flag-only path at production level: the durable writer
        // flag is set while its guard is already gone, so no guard exists
        // to retain. Release stays refused with no writer descriptor, and
        // Drop must mint a fresh guard (never unmap) instead of releasing.
        let lost_guard = runtime.camera.mark_writer_unstopped();
        drop(lost_guard);
        assert!(runtime.camera_writer_unstopped());
        assert!(runtime.camera_writer_fallback.is_none());
        assert!(!runtime.shared_memory_release_allowed());
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(true));
        drop(runtime);
    }

    async fn build_hung_camera_fault_runtime() -> (
        tempfile::TempDir,
        super::ProductionRuntime,
        VirtualSerialEndpoint,
    ) {
        build_camera_runtime(true).await
    }

    /// Clean-camera counterpart of [`build_hung_camera_fault_runtime`]: the
    /// same production composition root with a writer that stops, so the
    /// dynamic shutdown path can prove `dynamic_reaped == true` and a `true`
    /// step-5 gate instead of writer-fallback dominance.
    async fn build_clean_camera_runtime() -> (
        tempfile::TempDir,
        super::ProductionRuntime,
        VirtualSerialEndpoint,
    ) {
        build_camera_runtime(false).await
    }

    async fn build_camera_runtime(
        hang_writer: bool,
    ) -> (
        tempfile::TempDir,
        super::ProductionRuntime,
        VirtualSerialEndpoint,
    ) {
        use std::collections::BTreeMap;
        use std::ffi::OsString;

        use tempfile::TempDir;

        use crate::camera::ScreenshotMode;
        use crate::camera::backend::CameraBackend;
        use crate::camera::virtual_camera::{
            VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
        };
        use crate::device::serial::SerialBackend;
        use crate::dynamic_host::StartupDynamicHost;
        use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
        use crate::settings::roots::{BaseDirectories, RootEnvironment};

        let temporary = TempDir::new().expect("isolated roots must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("isolated base directories must resolve");
        std::fs::create_dir_all(base.join("config/pokecon/profiles/default"))
            .expect("isolated default profile must exist");
        let request = PipelineRequest {
            arguments: ["pokecon", "--serial-port", "virtual"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let before_dynamic = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("isolated pipeline must load the startup snapshot");
        let host = Arc::new(
            StartupDynamicHost::new(request.clone(), before_dynamic)
                .expect("startup host must accept the pipeline snapshot"),
        );
        let loaded = host
            .finish_startup()
            .expect("startup host must finish the real pipeline");

        let camera_backend = Arc::new(VirtualCameraBackend::default());
        let frames = if hang_writer {
            vec![RecordedFrame::Solid([9, 8, 7]), RecordedFrame::Hang]
        } else {
            vec![RecordedFrame::Solid([9, 8, 7])]
        };
        camera_backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30, frames,
        )));
        let serial_backend = Arc::new(VirtualSerialBackend::default());
        let endpoint = VirtualSerialEndpoint::new();
        serial_backend
            .push_plan(SerialOpenPlan::Success(endpoint.clone()))
            .await;
        let camera_backend_for_runtime: Arc<dyn CameraBackend> = camera_backend.clone();
        let serial_backend_for_runtime: Arc<dyn SerialBackend> = serial_backend.clone();
        let runtime = super::ProductionRuntime::build_with_backends(
            request,
            loaded,
            host,
            None,
            ScreenshotMode::Web,
            None,
            camera_backend_for_runtime,
            serial_backend_for_runtime,
        )
        .await
        .expect("production composition root must build on virtual fault backends");
        runtime
            .serial
            .connect()
            .await
            .expect("virtual serial fault endpoint must open");
        assert!(runtime.serial.is_connected().await);

        for _ in 0..1_000 {
            if runtime.camera.ring().read_published().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            runtime.camera.ring().read_published().unwrap().is_some(),
            "the camera writer must publish before the Hang fault is exercised"
        );
        endpoint
            .fail_next_write(std::io::ErrorKind::BrokenPipe)
            .await;
        (temporary, runtime, endpoint)
    }

    /// Resolves the prebuilt fault-worker fixture without inventing a fake
    /// worker. `option_env!` is used instead of `env!` because crate unit
    /// tests are not guaranteed the integration-test `CARGO_BIN_EXE_*`
    /// compile-time variables; the `current_exe` derivation covers the
    /// repository's `cargo test --all-features` harness (the fixture binary
    /// lands next to the test binary's `deps` parent), and
    /// `POKECON_TEST_FAULT_WORKER_BINARY` covers out-of-band runners. A
    /// missing binary fails the test; there is never a fake fallback.
    fn fault_worker_binary_path() -> std::path::PathBuf {
        const OVERRIDE_ENV: &str = "POKECON_TEST_FAULT_WORKER_BINARY";
        if let Some(path) = std::env::var_os(OVERRIDE_ENV) {
            let path = std::path::PathBuf::from(path);
            assert!(
                path.is_absolute(),
                "{OVERRIDE_ENV} must name an absolute path"
            );
            assert!(path.is_file(), "{OVERRIDE_ENV} must name a regular file");
            return path;
        }
        if let Some(path) = option_env!("CARGO_BIN_EXE_pokecon-worker-fault-fixture") {
            return std::path::PathBuf::from(path);
        }
        let current = std::env::current_exe().expect("test executable path must be known");
        let target_dir = current
            .parent()
            .and_then(|dir| dir.parent())
            .expect("test executable must run from a Cargo target profile directory");
        let candidate = target_dir.join(format!(
            "pokecon-worker-fault-fixture{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert!(
            candidate.is_file(),
            "fault-worker fixture binary must be built before this test runs: {}",
            candidate.display()
        );
        candidate
    }

    /// Resolves the real `pokecon-worker` script binary without inventing a
    /// fake worker. Mirrors [`fault_worker_binary_path`]: `option_env!` is
    /// used instead of `env!` because crate unit tests are not guaranteed the
    /// integration-test `CARGO_BIN_EXE_*` compile-time variables; the
    /// `current_exe` derivation covers the repository's
    /// `cargo test --all-features` harness (the worker binary lands next to
    /// the test binary's `deps` parent), and
    /// `POKECON_TEST_WORKER_BINARY` covers out-of-band runners. A missing
    /// binary fails the test; there is never a fake fallback.
    fn script_worker_binary_path() -> std::path::PathBuf {
        const OVERRIDE_ENV: &str = "POKECON_TEST_WORKER_BINARY";
        if let Some(path) = std::env::var_os(OVERRIDE_ENV) {
            let path = std::path::PathBuf::from(path);
            assert!(
                path.is_absolute(),
                "{OVERRIDE_ENV} must name an absolute path"
            );
            assert!(path.is_file(), "{OVERRIDE_ENV} must name a regular file");
            return path;
        }
        if let Some(path) = option_env!("CARGO_BIN_EXE_pokecon-worker") {
            return std::path::PathBuf::from(path);
        }
        let current = std::env::current_exe().expect("test executable path must be known");
        let target_dir = current
            .parent()
            .and_then(|dir| dir.parent())
            .expect("test executable must run from a Cargo target profile directory");
        let candidate = target_dir.join(format!("pokecon-worker{}", std::env::consts::EXE_SUFFIX));
        assert!(
            candidate.is_file(),
            "script worker binary must be built before this test runs: {}",
            candidate.display()
        );
        candidate
    }

    /// AR-11-29 fault slice: the real production composition root reaches the
    /// shutdown methods with a hung camera writer and a broken serial endpoint.
    /// The writer timeout must retain its mapping, the serial fault must close
    /// fail-closed, and the step-5 release observation must never turn a false
    /// dynamic reap into an unmap. This stays crate-internal because
    /// `ProductionRuntime` is intentionally `pub(crate)` and the test uses the
    /// same `build_with_backends` root as native construction.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_runtime_shutdown_faults_retain_mapping_and_fail_closed() {
        let (_temporary, mut runtime, endpoint) = build_hung_camera_fault_runtime().await;

        crate::shutdown_production(&mut runtime, None).await;
        assert!(runtime.camera_writer_unstopped());
        assert!(!runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_some());
        let bytes_after_input_stop = endpoint.written().await.len();

        runtime.retain_dynamic_mapping_after_worker_shutdown(false);
        assert!(!runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_some());
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(false));
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(true));

        assert!(!runtime.serial.is_connected().await);
        assert!(endpoint.is_closed());
        let bytes_after_serial_stop = endpoint.written().await.len();
        assert!(bytes_after_serial_stop >= bytes_after_input_stop);
        assert!(
            runtime
                .serial
                .send_controller_state(deflected_controller())
                .await
                .is_err(),
            "serial sends after the production fault shutdown must fail closed"
        );

        // Repeated production shutdown calls must not revive the endpoint or
        // append additional bytes after the fault has closed the service.
        crate::shutdown_production(&mut runtime, None).await;
        assert_eq!(endpoint.written().await.len(), bytes_after_serial_stop);
        assert!(endpoint.is_closed());
        drop(runtime);
    }

    /// AR-11-29 dynamic slice: the real composition-root shutdown reaps an
    /// actual fault-worker child in the dynamic role.
    ///
    /// The `ack-then-hang` fixture acknowledges the cooperative shutdown
    /// request but never exits, so `ManagedWorker::stop` must force-kill and
    /// reap it through the real `DynamicRuntime::shutdown_worker` path. The
    /// test then verifies `prepare_shutdown` closed host mutations, the real
    /// `StopReport` carried through the production shutdown report
    /// (`cooperative_acknowledged`, `forced`, non-success exit), the OS
    /// reap receipt, step-5 gate behavior under the hung camera writer, and
    /// fail-closed serial shutdown ordering. This stays crate-internal
    /// because `ProductionRuntime` and `shutdown_production` are
    /// intentionally not public; no fake worker is ever constructed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_shutdown_reaps_dynamic_fault_worker_before_serial_stop() {
        use crate::dynamic_runtime::DynamicRuntime;
        use crate::worker::WorkerKind;
        use crate::worker::generation::GenerationPhase;
        use crate::worker::supervisor::{WorkerLaunch, WorkerSupervisor};

        let (_temporary, mut runtime, endpoint) = build_hung_camera_fault_runtime().await;
        let host = runtime.backend.host();
        host.refresh_available_profiles()
            .expect("host must accept mutations before shutdown");

        let supervisor = Arc::new(WorkerSupervisor::new());
        let worker = supervisor
            .spawn(
                WorkerLaunch::custom(fault_worker_binary_path(), WorkerKind::Dynamic)
                    .argument("ack-then-hang"),
                host.controller_safety(),
            )
            .await
            .expect("ack-then-hang fixture must start in the dynamic role");
        let observed = Arc::clone(&worker);
        let dynamic =
            DynamicRuntime::from_test_worker(Arc::clone(&supervisor), worker, Arc::clone(&host));

        let bytes_before_serial_stop = endpoint.written().await.len();
        let report = crate::shutdown_production(&mut runtime, Some(dynamic)).await;

        let stopped = host
            .refresh_available_profiles()
            .expect_err("prepare_shutdown must close host mutations");
        assert_eq!(stopped.code, "HostStopping");

        // The real `DynamicRuntime::shutdown_worker` outcome, observed
        // through the production shutdown path: the fixture acked
        // cooperative shutdown yet never exited, so the reap must be
        // acknowledged, forced, and reaped with a non-success exit.
        assert!(
            report.dynamic_reaped,
            "the ack-then-hang worker must be reaped through the production shutdown path"
        );
        let stop = report
            .dynamic_stop
            .expect("a dynamic worker existed, so the stop outcome must be reported")
            .expect("the ack-then-hang worker reap must be proven, not fail-closed");
        assert!(
            stop.cooperative_acknowledged,
            "the fixture acknowledged cooperative shutdown: {stop:?}"
        );
        assert!(
            stop.forced,
            "the hung worker required forced termination: {stop:?}"
        );
        assert!(
            !stop.exit.success,
            "a forced stop must not look like a clean exit: {stop:?}"
        );

        assert_eq!(observed.generation().phase(), GenerationPhase::Stopped);
        let exit = observed
            .wait()
            .await
            .expect("dynamic fault worker must be reaped");
        assert!(
            !exit.success,
            "a forced stop must not look like a clean exit: {exit:?}"
        );

        assert!(runtime.camera_writer_unstopped());
        assert!(!runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_some());
        assert!(!report.step5_release_allowed);
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(true));
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(false));

        assert!(!runtime.serial.is_connected().await);
        assert!(endpoint.is_closed());
        assert!(endpoint.written().await.len() >= bytes_before_serial_stop);
        assert!(
            runtime
                .serial
                .send_controller_state(deflected_controller())
                .await
                .is_err(),
            "serial sends after the production dynamic shutdown must fail closed"
        );
        drop(runtime);
    }

    /// AR-11-29 clean-camera dynamic slice: the same composition-root
    /// shutdown with a camera writer that stops, so the step-5 gate is not
    /// dominated by the writer fallback.
    ///
    /// The real `ack-then-hang` dynamic fixture is still reaped through the
    /// production path (same `StopReport` assertions as the hung-camera
    /// test above), but with the writer stopped the confirmed reap must
    /// yield `dynamic_reaped == true` and a `true` step-5 release
    /// observation with no fallback retained. The hung-camera test retains
    /// the fail-closed counterpart where both gate readings stay `false`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_shutdown_with_clean_camera_reaps_dynamic_and_allows_release() {
        use crate::dynamic_runtime::DynamicRuntime;
        use crate::worker::WorkerKind;
        use crate::worker::generation::GenerationPhase;
        use crate::worker::supervisor::{WorkerLaunch, WorkerSupervisor};

        let (_temporary, mut runtime, endpoint) = build_clean_camera_runtime().await;
        let host = runtime.backend.host();
        host.refresh_available_profiles()
            .expect("host must accept mutations before shutdown");

        let supervisor = Arc::new(WorkerSupervisor::new());
        let worker = supervisor
            .spawn(
                WorkerLaunch::custom(fault_worker_binary_path(), WorkerKind::Dynamic)
                    .argument("ack-then-hang"),
                host.controller_safety(),
            )
            .await
            .expect("ack-then-hang fixture must start in the dynamic role");
        let observed = Arc::clone(&worker);
        let dynamic =
            DynamicRuntime::from_test_worker(Arc::clone(&supervisor), worker, Arc::clone(&host));

        let bytes_before_serial_stop = endpoint.written().await.len();
        let report = crate::shutdown_production(&mut runtime, Some(dynamic)).await;

        let stopped = host
            .refresh_available_profiles()
            .expect_err("prepare_shutdown must close host mutations");
        assert_eq!(stopped.code, "HostStopping");

        assert!(
            report.dynamic_reaped,
            "the ack-then-hang worker must be reaped through the production shutdown path"
        );
        let stop = report
            .dynamic_stop
            .expect("a dynamic worker existed, so the stop outcome must be reported")
            .expect("the ack-then-hang worker reap must be proven, not fail-closed");
        assert!(
            stop.cooperative_acknowledged,
            "the fixture acknowledged cooperative shutdown: {stop:?}"
        );
        assert!(
            stop.forced,
            "the hung worker required forced termination: {stop:?}"
        );
        assert!(
            !stop.exit.success,
            "a forced stop must not look like a clean exit: {stop:?}"
        );
        assert_eq!(observed.generation().phase(), GenerationPhase::Stopped);
        let exit = observed
            .wait()
            .await
            .expect("dynamic fault worker must be reaped");
        assert!(
            !exit.success,
            "a forced stop must not look like a clean exit: {exit:?}"
        );

        // With the writer stopped, the confirmed dynamic reap flows through
        // to a `true` step-5 release observation with no fallback retained.
        assert!(!runtime.camera_writer_unstopped());
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());
        assert!(
            report.step5_release_allowed,
            "a confirmed reap with a stopped writer must allow step-5 release"
        );
        assert!(runtime.shared_memory_release_gate_after_dynamic_reap(true));

        assert!(!runtime.serial.is_connected().await);
        assert!(endpoint.is_closed());
        assert!(endpoint.written().await.len() >= bytes_before_serial_stop);
        assert!(
            runtime
                .serial
                .send_controller_state(deflected_controller())
                .await
                .is_err(),
            "serial sends after the production dynamic shutdown must fail closed"
        );
        drop(runtime);
    }

    /// AR-11-29 clean Python-reader slice: a real `pokecon-worker` script
    /// worker reads through `RingReader` against the actual
    /// `ProductionRuntime` camera mapping, is reaped cooperatively, and
    /// leaves no reader pin behind, so the §15.6 step-5 release gate stays
    /// open with no fallback retained.
    ///
    /// The worker executes a `TRACE_SOURCE`-like script calling `readFrame`
    /// and `getCameraImage`, which proves the `ScriptWorkerClient`
    /// round-trip completed and therefore `initialize_python` used
    /// `SharedFrameRing::open` plus `RingReader::read` against the
    /// production mapping (no separate worker-side ring is created). The
    /// reaped `StopReport` is then wrapped as `Ok(Some(ScriptSessionStop))`
    /// exactly as `UserScriptSession::shutdown` maps
    /// `ManagedWorker::stop`, and fed to the real
    /// reader-pin recovery/retention helpers. This stays crate-internal
    /// because `ProductionRuntime` is intentionally `pub(crate)`.
    #[allow(clippy::too_many_lines)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_python_reader_on_runtime_mapping_leaves_no_pin() {
        use std::collections::BTreeMap;
        use std::ffi::OsString;
        use std::io::Write as _;

        use tempfile::TempDir;

        use crate::camera::ScreenshotMode;
        use crate::camera::backend::CameraBackend;
        use crate::camera::virtual_camera::{
            VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
        };
        use crate::camera::{BgrFrame, MappingDescriptor, ScreenshotFormat};
        use crate::device::serial::{SerialBackend, VirtualSerialBackend};
        use crate::dynamic_host::StartupDynamicHost;
        use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
        use crate::settings::roots::{BaseDirectories, RootEnvironment};
        use crate::worker::WorkerKind;
        use crate::worker::generation::GenerationPhase;
        use crate::worker::ipc::ResourceSafety;
        use crate::worker::script::protocol::{
            HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
            HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
            HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
            HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
            HostTkRequest, HostTkResult, PYTHON_SITE_PACKAGES_ENV, ScriptDialogState,
            ScriptExecuteRequest, ScriptExecutionOutcome, ScriptInitializeRequest,
            ScriptWorkerStatus,
        };
        use crate::worker::script::{ScriptHost, ScriptHostError, ScriptWorkerClient};
        use crate::worker::supervisor::{StopPurpose, WorkerLaunch, WorkerSupervisor};

        /// Minimal [`ScriptHost`] serving the production camera mapping and
        /// state. Every other surface is an inert stub because the reader
        /// script only opens the camera, reads frames, and prints.
        struct RuntimeMappingScriptHost {
            mapping: MappingDescriptor,
            state: HostCameraState,
        }

        impl ScriptHost for RuntimeMappingScriptHost {
            fn controller_input(
                &self,
                _request: HostControllerInputRequest,
            ) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn controller_neutral(&self) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn serial_write(&self, _data: Vec<u8>) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn serial_write_row(&self, _row: String) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn serial_reload(&self) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn output(&self, _request: HostOutputRequest) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn dialog_open(
                &self,
                _request: HostDialogOpenRequest,
            ) -> Result<HostDialogOpenResult, ScriptHostError> {
                Ok(HostDialogOpenResult { dialog_id: 1 })
            }

            fn dialog_status(
                &self,
                _request: HostDialogStatusRequest,
            ) -> Result<HostDialogStatusResult, ScriptHostError> {
                Ok(HostDialogStatusResult {
                    state: ScriptDialogState::Aborted,
                })
            }

            fn dialog_close_all(&self) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn network(
                &self,
                _request: HostNetworkRequest,
            ) -> Result<HostNetworkResult, ScriptHostError> {
                Ok(HostNetworkResult { message: None })
            }

            fn notification(
                &self,
                _request: HostNotificationRequest,
            ) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn camera_initialize(&self) -> Result<HostCameraInitializeResult, ScriptHostError> {
                Ok(HostCameraInitializeResult {
                    mapping: Some(self.mapping.clone()),
                    state: self.state,
                })
            }

            fn camera_control(
                &self,
                _request: HostCameraControlRequest,
            ) -> Result<HostCameraState, ScriptHostError> {
                Ok(self.state)
            }

            fn overlay(&self, _request: HostOverlayRequest) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn popup_image(&self, _request: HostPopupImageRequest) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn tk(&self, _request: HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
                Ok(HostTkResult::default())
            }
        }

        impl ResourceSafety for RuntimeMappingScriptHost {
            fn force_release(&self) {}
        }

        let temporary = TempDir::new().expect("isolated roots must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("isolated base directories must resolve");
        std::fs::create_dir_all(base.join("config/pokecon/profiles/default"))
            .expect("isolated default profile must exist");
        let request = PipelineRequest {
            arguments: ["pokecon", "--serial-port", "virtual"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let before_dynamic = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("isolated pipeline must load the startup snapshot");
        let host = Arc::new(
            StartupDynamicHost::new(request.clone(), before_dynamic)
                .expect("startup host must accept the pipeline snapshot"),
        );
        let loaded = host
            .finish_startup()
            .expect("startup host must finish the real pipeline");

        let camera_backend = Arc::new(VirtualCameraBackend::default());
        camera_backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([3, 2, 1])],
        )));
        let serial_backend = Arc::new(VirtualSerialBackend::default());
        let camera_backend_for_runtime: Arc<dyn CameraBackend> = camera_backend.clone();
        let serial_backend_for_runtime: Arc<dyn SerialBackend> = serial_backend.clone();
        let mut runtime = super::ProductionRuntime::build_with_backends(
            request,
            loaded,
            host,
            None,
            ScreenshotMode::Web,
            None,
            camera_backend_for_runtime,
            serial_backend_for_runtime,
        )
        .await
        .expect("production composition root must build on virtual backends");
        assert!(
            runtime.camera.status().camera_opened,
            "the virtual camera must open before the reader worker starts"
        );

        // The production mapping and one solid frame published directly on
        // the production ring. No separate worker-side ring is created.
        let descriptor = runtime.camera.mapping_descriptor();
        let ring = runtime.camera.ring();
        let byte_len = usize::try_from(descriptor.frame_width)
            .expect("mapping width must fit usize")
            * usize::try_from(descriptor.frame_height).expect("mapping height must fit usize")
            * 3;
        let solid = BgrFrame::new(
            descriptor.frame_width,
            descriptor.frame_height,
            vec![11_u8; byte_len],
        )
        .expect("solid frame must fit the production mapping");
        match ring.publish(&solid) {
            Ok(_) => {}
            Err(error) => {
                assert!(
                    ring.read_published()
                        .expect("published-frame observation must succeed")
                        .is_some(),
                    "the production ring must hold a frame when our publish races the writer: {error:?}"
                );
            }
        }
        for _ in 0..1_000 {
            if ring
                .read_published()
                .expect("published-frame observation must succeed")
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            ring.read_published()
                .expect("published-frame observation must succeed")
                .is_some(),
            "the production ring must publish before the reader worker starts"
        );

        let resolution = CaptureResolution::ALL
            .iter()
            .copied()
            .find(|candidate| {
                let size = candidate.size();
                size.width() == descriptor.frame_width && size.height() == descriptor.frame_height
            })
            .expect("production mapping dimensions must name a closed capture resolution");
        let script_host = Arc::new(RuntimeMappingScriptHost {
            mapping: descriptor.clone(),
            state: HostCameraState {
                opened: true,
                fps: runtime.camera.status().camera_fps,
                capture_resolution: resolution,
                flip_mode: FlipMode::None,
                screenshot_format: ScreenshotFormat::Png,
            },
        });

        let command_root = base.join("reader-commands");
        let data_root = base.join("reader-data");
        std::fs::create_dir_all(&command_root).expect("reader command root must exist");
        std::fs::create_dir_all(&data_root).expect("reader data root must exist");
        let script_source = format!(
            "from Commands.PythonCommandBase import ImageProcPythonCommand\n\
             \n\
             \n\
             class Reader(ImageProcPythonCommand):\n\
             \x20   def do(self):\n\
             \x20       assert self.camera.isOpened()\n\
             \x20       frame = self.camera.readFrame()\n\
             \x20       assert frame.shape == ({height}, {width}, 3)\n\
             \x20       cropped = self.getCameraImage(\"1\", [60, 50, 90, 70])\n\
             \x20       assert cropped.shape == (20, 30, 3)\n\
             \x20       self.print_t1(\"reader-frame-ok\")\n",
            height = descriptor.frame_height,
            width = descriptor.frame_width,
        );
        std::fs::write(command_root.join("reader.py"), script_source)
            .expect("reader script must be written");

        // Drive the real `pokecon-worker` script binary through the existing
        // supervisor/client patterns against the production mapping.
        let worker_binary = script_worker_binary_path();
        let supervisor = WorkerSupervisor::new();
        let mut launch =
            WorkerLaunch::managed(&worker_binary, WorkerKind::Script).clear_environment();
        if let Some(site_packages) = std::env::var_os(PYTHON_SITE_PACKAGES_ENV) {
            launch = launch.environment(PYTHON_SITE_PACKAGES_ENV, site_packages);
        }
        let worker = supervisor
            .spawn(launch, script_host.clone())
            .await
            .expect("real script worker must start");
        let client = ScriptWorkerClient::attach(worker.clone(), script_host)
            .expect("script client must attach");
        assert_eq!(
            client.status().await.expect("initial status must succeed"),
            ScriptWorkerStatus::uninitialized()
        );
        let initialized = client
            .initialize(&ScriptInitializeRequest {
                profile: "default".to_owned(),
                command_root: command_root.clone(),
                data_root: data_root.clone(),
            })
            .await
            .expect("script runtime must initialize against the production mapping");
        assert!(initialized.status.initialized);
        assert_eq!(initialized.status.profile.as_deref(), Some("default"));

        // The completed round-trip proves `initialize_python` opened the
        // production mapping via `SharedFrameRing::open` and the script read
        // it via `RingReader::read`.
        let execution = tokio::time::timeout(
            Duration::from_mins(2),
            client.execute(&ScriptExecuteRequest {
                path: "reader.py".into(),
                class_name: "Reader".to_owned(),
                tags: Vec::new(),
            }),
        )
        .await
        .expect("reader execution must settle before its deadline")
        .expect("reader execution must succeed");
        assert_eq!(
            execution.outcome,
            ScriptExecutionOutcome::Completed,
            "the reader script must complete against the production mapping"
        );

        // Bounded cooperative reap of the real reader worker.
        let stop = worker
            .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(10))
            .await
            .expect("real script worker must stop cooperatively");
        assert!(
            stop.cooperative_acknowledged,
            "the clean reader stop must be acknowledged: {stop:?}"
        );
        assert!(
            !stop.forced,
            "the clean reader stop must not force: {stop:?}"
        );
        assert!(
            stop.exit.success,
            "the clean reader exit must succeed: {stop:?}"
        );
        let exit = worker
            .wait()
            .await
            .expect("the script reader process must be reaped");
        assert_eq!(worker.generation().phase(), GenerationPhase::Stopped);
        assert!(
            exit.success,
            "the reaped reader exit must succeed: {exit:?}"
        );

        // The real reap receipt, wrapped exactly as
        // `UserScriptSession::shutdown` maps `ManagedWorker::stop` (only the
        // thin session wrapper is synthesized; the full
        // UserScriptSession/CommandService wiring stays out of scope for this
        // packet and is recorded in the artifact limitations).
        let outcome: Result<Option<ScriptSessionStop>, CommandServiceError> =
            Ok(Some(ScriptSessionStop {
                forced: stop.forced,
            }));
        let recovered = recover_reader_pins_after_script_shutdown(&runtime.camera, &outcome);
        assert_eq!(
            recovered,
            Some(0),
            "a cooperatively reaped Python reader must not leave an abandoned pin"
        );
        if runtime.camera_reader_fallback.is_none() {
            runtime.camera_reader_fallback =
                retain_reader_mapping_after_script_shutdown(&runtime.camera, &outcome, recovered);
        }
        assert!(runtime.camera_reader_fallback.is_none());
        assert!(runtime.shared_memory_release_allowed());
        let step5_release_allowed = runtime.shared_memory_release_gate_after_dynamic_reap(true);
        assert!(
            step5_release_allowed,
            "the confirmed reap with a stopped-writer-pending camera must allow step-5 release"
        );
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(false));
        assert!(runtime.camera_mapping_fallback().is_none());
        let recovered_pin_count = recovered.expect("the clean reap must report a pin count");
        eprintln!(
            "production reader lifecycle: script-worker-reaped=true, replacement-reader-absent=true, recovered-pins={recovered_pin_count}"
        );

        // Production shutdown tail stays clean after the reader reap.
        runtime.stop_inputs_camera_and_scripts().await;
        assert!(runtime.shared_memory_release_gate_after_dynamic_reap(true));
        runtime.stop_serial().await;
        assert!(!runtime.camera_writer_unstopped());
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());

        // Measured artifact: every value below is read from this execution.
        // The dynamic half of the step-5 gate is confirmed by the test driver
        // (no dynamic worker exists in this fixture), mirroring
        // `shutdown_production`'s vacuous-true convention for an absent
        // dynamic worker.
        let artifact_path = production_artifact_path("production-reader-lifecycle.json");
        let document = serde_json::json!({
            "schema": "production-reader-lifecycle/1",
            "fixture_id": "production-python-reader-clean-reap",
            "mapping": {
                "shm_handle": descriptor.shm_handle,
                "total_size": descriptor.total_size,
                "frame_width": descriptor.frame_width,
                "frame_height": descriptor.frame_height,
                "slot_byte_size": descriptor.slot_byte_size,
            },
            "worker_kind": "script",
            "worker_binary": worker_binary.display().to_string(),
            "script_surface": "rust/pokecon/src/worker_binary/script/python.rs::initialize_python",
            "reader_surface": "readFrame/getCameraImage via RingReader::read against the ProductionRuntime camera mapping",
            "script": {
                "path": "reader.py",
                "class_name": "Reader",
                "outcome": "Completed",
            },
            "shutdown": {
                "cooperative_acknowledged": stop.cooperative_acknowledged,
                "forced": stop.forced,
                "process_reaped": true,
                "exit_success": stop.exit.success,
                "exit_code": stop.exit.code,
                "replacement_reader_absent": true,
            },
            "recovered_pin_count": recovered_pin_count,
            "step5_release_allowed": step5_release_allowed,
            "camera_mapping_fallback": serde_json::to_value(runtime.camera_mapping_fallback())
                .expect("fallback descriptor must serialize"),
            "result": "passed",
            "limitations": [
                "virtual camera/serial backends; no native hardware is claimed",
                "the script-session outcome is Ok(Some(ScriptSessionStop)) built from the real ManagedWorker StopReport; full UserScriptSession/CommandService wiring is out of scope",
                "no dynamic worker exists in this fixture; dynamic_reaped=true is confirmed by the test driver, mirroring shutdown_production's vacuous-true convention for an absent dynamic worker",
                "covers only the clean Python-reader reap: no crash-abandoned-pin recovery, no process-exit reclaim, no Drop/mem::forget lifecycle, no step-9 teardown",
            ],
        });
        let bytes =
            serde_json::to_string_pretty(&document).expect("artifact document must serialize");
        {
            let mut file = atomic_write_file::AtomicWriteFile::open(&artifact_path)
                .expect("artifact must open for atomic write");
            file.write_all(bytes.as_bytes())
                .expect("artifact must write completely");
            file.commit().expect("artifact must commit atomically");
        }
        eprintln!(
            "production reader lifecycle artifact: {}",
            artifact_path.display()
        );
        drop(runtime);
    }

    /// AR-11-29 crash-abandoned-pin slice: a real `pokecon-worker` script
    /// worker first proves the real Python reader path against the actual
    /// `ProductionRuntime` camera mapping, then hangs inside a sleep script
    /// while one crash-abandoned reader pin is planted through the existing
    /// production-mapping diagnostic seam. The forced OS reap must recover
    /// exactly that pin with no fallback retained and the §15.6 step-5
    /// release gate open.
    ///
    /// Phase 1 executes a `TRACE_SOURCE`-like script calling `readFrame`
    /// and `getCameraImage` to `Completed`, which proves the
    /// `ScriptWorkerClient` round-trip completed and therefore
    /// `initialize_python` used `SharedFrameRing::open` plus
    /// `RingReader::read` against the production mapping (no separate
    /// worker-side ring is created). Phase 2 starts a `time.sleep(600)`
    /// script and keeps its execution in-flight, then plants one abandoned
    /// pin via `pin_current_for_diagnostics` /
    /// `abandon_for_crash_simulation` on the production ring: a hung
    /// script's transient `RingReader` pins never survive on their own, so
    /// the diagnostic seam stands in for the crashed reader while the
    /// worker itself stays a real OS process reaped by a real forced stop
    /// (`SCRIPT_SHUTDOWN_ACK_TIMEOUT` keeps the hung worker from
    /// acknowledging, so the supervisor SIGKILLs past the deadline). The
    /// reaped `StopReport` is wrapped as `Ok(Some(ScriptSessionStop))`
    /// exactly as `UserScriptSession::shutdown` maps `ManagedWorker::stop`,
    /// and fed to the real reader-pin recovery/retention helpers with
    /// `worker_process_reaped=true` and `replacement_reader_absent=true`.
    /// This stays crate-internal because `ProductionRuntime` is
    /// intentionally `pub(crate)`.
    #[allow(clippy::too_many_lines)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_python_reader_crash_abandoned_pin_recovers_after_reap() {
        use std::collections::BTreeMap;
        use std::ffi::OsString;
        use std::io::Write as _;

        use tempfile::TempDir;

        use crate::camera::ScreenshotMode;
        use crate::camera::backend::CameraBackend;
        use crate::camera::virtual_camera::{
            VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
        };
        use crate::camera::{BgrFrame, MappingDescriptor, ScreenshotFormat};
        use crate::device::serial::{SerialBackend, VirtualSerialBackend};
        use crate::dynamic_host::StartupDynamicHost;
        use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
        use crate::settings::roots::{BaseDirectories, RootEnvironment};
        use crate::worker::WorkerKind;
        use crate::worker::generation::GenerationPhase;
        use crate::worker::ipc::ResourceSafety;
        use crate::worker::script::protocol::{
            HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
            HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
            HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
            HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
            HostTkRequest, HostTkResult, PYTHON_SITE_PACKAGES_ENV, ScriptDialogState,
            ScriptExecuteRequest, ScriptExecutionOutcome, ScriptInitializeRequest,
            ScriptWorkerStatus,
        };
        use crate::worker::script::{ScriptHost, ScriptHostError, ScriptWorkerClient};
        use crate::worker::supervisor::{StopPurpose, WorkerLaunch, WorkerSupervisor};

        /// Minimal [`ScriptHost`] serving the production camera mapping and
        /// state. Every other surface is an inert stub because the reader
        /// script only opens the camera, reads frames, and prints, while
        /// the hang script only sleeps.
        struct RuntimeMappingScriptHost {
            mapping: MappingDescriptor,
            state: HostCameraState,
        }

        impl ScriptHost for RuntimeMappingScriptHost {
            fn controller_input(
                &self,
                _request: HostControllerInputRequest,
            ) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn controller_neutral(&self) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn serial_write(&self, _data: Vec<u8>) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn serial_write_row(&self, _row: String) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn serial_reload(&self) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn output(&self, _request: HostOutputRequest) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn dialog_open(
                &self,
                _request: HostDialogOpenRequest,
            ) -> Result<HostDialogOpenResult, ScriptHostError> {
                Ok(HostDialogOpenResult { dialog_id: 1 })
            }

            fn dialog_status(
                &self,
                _request: HostDialogStatusRequest,
            ) -> Result<HostDialogStatusResult, ScriptHostError> {
                Ok(HostDialogStatusResult {
                    state: ScriptDialogState::Aborted,
                })
            }

            fn dialog_close_all(&self) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn network(
                &self,
                _request: HostNetworkRequest,
            ) -> Result<HostNetworkResult, ScriptHostError> {
                Ok(HostNetworkResult { message: None })
            }

            fn notification(
                &self,
                _request: HostNotificationRequest,
            ) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn camera_initialize(&self) -> Result<HostCameraInitializeResult, ScriptHostError> {
                Ok(HostCameraInitializeResult {
                    mapping: Some(self.mapping.clone()),
                    state: self.state,
                })
            }

            fn camera_control(
                &self,
                _request: HostCameraControlRequest,
            ) -> Result<HostCameraState, ScriptHostError> {
                Ok(self.state)
            }

            fn overlay(&self, _request: HostOverlayRequest) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn popup_image(&self, _request: HostPopupImageRequest) -> Result<(), ScriptHostError> {
                Ok(())
            }

            fn tk(&self, _request: HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
                Ok(HostTkResult::default())
            }
        }

        impl ResourceSafety for RuntimeMappingScriptHost {
            fn force_release(&self) {}
        }

        let temporary = TempDir::new().expect("isolated roots must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("isolated base directories must resolve");
        std::fs::create_dir_all(base.join("config/pokecon/profiles/default"))
            .expect("isolated default profile must exist");

        let request = PipelineRequest {
            arguments: ["pokecon", "--serial-port", "virtual"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let before_dynamic = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("isolated pipeline must load the startup snapshot");
        let host = Arc::new(
            StartupDynamicHost::new(request.clone(), before_dynamic)
                .expect("startup host must accept the pipeline snapshot"),
        );
        let loaded = host
            .finish_startup()
            .expect("startup host must finish the real pipeline");

        let camera_backend = Arc::new(VirtualCameraBackend::default());
        camera_backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([3, 2, 1])],
        )));
        let serial_backend = Arc::new(VirtualSerialBackend::default());
        let camera_backend_for_runtime: Arc<dyn CameraBackend> = camera_backend.clone();
        let serial_backend_for_runtime: Arc<dyn SerialBackend> = serial_backend.clone();
        let mut runtime = super::ProductionRuntime::build_with_backends(
            request,
            loaded,
            host,
            None,
            ScreenshotMode::Web,
            None,
            camera_backend_for_runtime,
            serial_backend_for_runtime,
        )
        .await
        .expect("production composition root must build on virtual backends");
        assert!(
            runtime.camera.status().camera_opened,
            "the virtual camera must open before the reader worker starts"
        );

        // The production mapping and one solid frame published directly on
        // the production ring. No separate worker-side ring is created.
        let descriptor = runtime.camera.mapping_descriptor();
        let ring = runtime.camera.ring();
        let byte_len = usize::try_from(descriptor.frame_width)
            .expect("mapping width must fit usize")
            * usize::try_from(descriptor.frame_height).expect("mapping height must fit usize")
            * 3;
        let solid = BgrFrame::new(
            descriptor.frame_width,
            descriptor.frame_height,
            vec![11_u8; byte_len],
        )
        .expect("solid frame must fit the production mapping");
        match ring.publish(&solid) {
            Ok(_) => {}
            Err(error) => {
                assert!(
                    ring.read_published()
                        .expect("published-frame observation must succeed")
                        .is_some(),
                    "the production ring must hold a frame when our publish races the writer: {error:?}"
                );
            }
        }
        for _ in 0..1_000 {
            if ring
                .read_published()
                .expect("published-frame observation must succeed")
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            ring.read_published()
                .expect("published-frame observation must succeed")
                .is_some(),
            "the production ring must publish before the reader worker starts"
        );

        let resolution = CaptureResolution::ALL
            .iter()
            .copied()
            .find(|candidate| {
                let size = candidate.size();
                size.width() == descriptor.frame_width && size.height() == descriptor.frame_height
            })
            .expect("production mapping dimensions must name a closed capture resolution");
        let script_host = Arc::new(RuntimeMappingScriptHost {
            mapping: descriptor.clone(),
            state: HostCameraState {
                opened: true,
                fps: runtime.camera.status().camera_fps,
                capture_resolution: resolution,
                flip_mode: FlipMode::None,
                screenshot_format: ScreenshotFormat::Png,
            },
        });

        let command_root = base.join("reader-commands");
        let data_root = base.join("reader-data");
        std::fs::create_dir_all(&command_root).expect("reader command root must exist");
        std::fs::create_dir_all(&data_root).expect("reader data root must exist");
        let script_source = format!(
            "from Commands.PythonCommandBase import ImageProcPythonCommand\n\
             \n\
             \n\
             class Reader(ImageProcPythonCommand):\n\
             \x20   def do(self):\n\
             \x20       assert self.camera.isOpened()\n\
             \x20       frame = self.camera.readFrame()\n\
             \x20       assert frame.shape == ({height}, {width}, 3)\n\
             \x20       cropped = self.getCameraImage(\"1\", [60, 50, 90, 70])\n\
             \x20       assert cropped.shape == (20, 30, 3)\n\
             \x20       self.print_t1(\"reader-frame-ok\")\n",
            height = descriptor.frame_height,
            width = descriptor.frame_width,
        );
        std::fs::write(command_root.join("reader.py"), script_source)
            .expect("reader script must be written");
        std::fs::write(
            command_root.join("hang.py"),
            "from Commands.PythonCommandBase import ImageProcPythonCommand\n\
             import time\n\
             \n\
             \n\
             class Hanger(ImageProcPythonCommand):\n\
             \x20   def do(self):\n\
             \x20       self.print_t1(\"hang-entered\")\n\
             \x20       time.sleep(600)\n",
        )
        .expect("hang script must be written");

        // Drive the real `pokecon-worker` script binary through the existing
        // supervisor/client patterns against the production mapping.
        let worker_binary = script_worker_binary_path();
        let supervisor = WorkerSupervisor::new();
        let mut launch =
            WorkerLaunch::managed(&worker_binary, WorkerKind::Script).clear_environment();
        if let Some(site_packages) = std::env::var_os(PYTHON_SITE_PACKAGES_ENV) {
            launch = launch.environment(PYTHON_SITE_PACKAGES_ENV, site_packages);
        }
        let worker = supervisor
            .spawn(launch, script_host.clone())
            .await
            .expect("real script worker must start");
        let client = ScriptWorkerClient::attach(worker.clone(), script_host)
            .expect("script client must attach");
        assert_eq!(
            client.status().await.expect("initial status must succeed"),
            ScriptWorkerStatus::uninitialized()
        );
        let initialized = client
            .initialize(&ScriptInitializeRequest {
                profile: "default".to_owned(),
                command_root: command_root.clone(),
                data_root: data_root.clone(),
            })
            .await
            .expect("script runtime must initialize against the production mapping");
        assert!(initialized.status.initialized);
        assert_eq!(initialized.status.profile.as_deref(), Some("default"));

        // Phase 1: the completed round-trip proves `initialize_python`
        // opened the production mapping via `SharedFrameRing::open` and the
        // script read it via `RingReader::read`.
        let execution = tokio::time::timeout(
            Duration::from_mins(2),
            client.execute(&ScriptExecuteRequest {
                path: "reader.py".into(),
                class_name: "Reader".to_owned(),
                tags: Vec::new(),
            }),
        )
        .await
        .expect("reader execution must settle before its deadline")
        .expect("reader execution must succeed");
        assert_eq!(
            execution.outcome,
            ScriptExecutionOutcome::Completed,
            "the reader script must complete against the production mapping"
        );

        // Phase 2: keep a sleep-script execution in-flight so the worker is
        // provably busy, then crash it with a forced stop. The hang must
        // still be pending after the startup grace period; a fast
        // settlement would mean the hang never started and the forced-stop
        // proof below would be vacuous.
        let hang_request = ScriptExecuteRequest {
            path: "hang.py".into(),
            class_name: "Hanger".to_owned(),
            tags: Vec::new(),
        };
        let hang_future = client.execute(&hang_request);
        tokio::pin!(hang_future);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(
            worker.generation().phase(),
            GenerationPhase::Running,
            "the hanging reader worker must still be running"
        );
        tokio::select! {
            biased;
            _ = &mut hang_future => {
                panic!("the hang script must stay in-flight until the forced stop");
            }
            () = tokio::time::sleep(Duration::from_millis(200)) => {}
        }

        // The abandoned pin is planted through the existing
        // production-mapping diagnostic seam: a hung script's transient
        // `RingReader` pins never survive on their own, so this stands in
        // for the crashed reader's pin while the worker itself stays a real
        // OS process. The completed phase-1 execution above remains the
        // proof that the real Python reader path was driven first.
        runtime
            .camera
            .ring()
            .pin_current_for_diagnostics()
            .expect("diagnostic pin must succeed on the production ring")
            .expect("the production ring must hold a published frame for the crash pin")
            .abandon_for_crash_simulation();

        // Narrowest source-backed forced-stop seam: the hung worker cannot
        // acknowledge cooperative shutdown within its actor ack timeout, so
        // the supervisor SIGKILLs past the deadline and reaps a non-success
        // exit through the real `ManagedWorker::stop`/`wait` path.
        let stop = worker
            .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(3))
            .await
            .expect("the hung script worker must be force-stopped");
        assert!(
            stop.forced,
            "the hung reader stop must force termination: {stop:?}"
        );
        assert!(
            !stop.cooperative_acknowledged,
            "the hung reader must not acknowledge cooperative shutdown: {stop:?}"
        );
        assert!(
            !stop.exit.success,
            "a forced stop must not look like a clean exit: {stop:?}"
        );
        let exit = worker
            .wait()
            .await
            .expect("the crashed reader process must be reaped");
        assert_eq!(worker.generation().phase(), GenerationPhase::Stopped);
        assert!(
            !exit.success,
            "the reaped crash exit must not succeed: {exit:?}"
        );

        // The interrupted hang execution must settle once its worker is
        // gone, and it must never report a completed script.
        let hang_settled = tokio::time::timeout(Duration::from_secs(10), hang_future)
            .await
            .expect("the hang execution must settle after the forced stop");
        let hang_outcome = match &hang_settled {
            Ok(result) => format!("completed-with-outcome:{:?}", result.outcome),
            Err(error) => format!("client-error-after-forced-stop:{error}"),
        };
        assert!(
            !matches!(
                &hang_settled,
                Ok(result) if result.outcome == ScriptExecutionOutcome::Completed
            ),
            "the force-killed hang script must not report completion"
        );

        // The real reap receipt, wrapped exactly as
        // `UserScriptSession::shutdown` maps `ManagedWorker::stop` (only the
        // thin session wrapper is synthesized; the full
        // UserScriptSession/CommandService wiring stays out of scope for this
        // packet and is recorded in the artifact limitations).
        let outcome: Result<Option<ScriptSessionStop>, CommandServiceError> =
            Ok(Some(ScriptSessionStop {
                forced: stop.forced,
            }));
        let recovered = recover_reader_pins_after_script_shutdown(&runtime.camera, &outcome);
        assert_eq!(
            recovered,
            Some(1),
            "the forced reap must recover the single crash-abandoned pin"
        );
        if runtime.camera_reader_fallback.is_none() {
            runtime.camera_reader_fallback =
                retain_reader_mapping_after_script_shutdown(&runtime.camera, &outcome, recovered);
        }
        assert!(runtime.camera_reader_fallback.is_none());
        assert!(runtime.shared_memory_release_allowed());
        let step5_release_allowed = runtime.shared_memory_release_gate_after_dynamic_reap(true);
        assert!(
            step5_release_allowed,
            "the confirmed crash reap with a stopped-writer-pending camera must allow step-5 release"
        );
        assert!(!runtime.shared_memory_release_gate_after_dynamic_reap(false));
        assert!(runtime.camera_mapping_fallback().is_none());
        let recovered_pin_count = recovered.expect("the crash reap must report a pin count");
        eprintln!(
            "production reader crash: script-worker-reaped=true, replacement-reader-absent=true, recovered-pins={recovered_pin_count}"
        );

        // Production shutdown tail stays clean after the crash reap.
        runtime.stop_inputs_camera_and_scripts().await;
        assert!(runtime.shared_memory_release_gate_after_dynamic_reap(true));
        runtime.stop_serial().await;
        assert!(!runtime.camera_writer_unstopped());
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());

        // Measured artifact: every value below is read from this execution.
        // The dynamic half of the step-5 gate is confirmed by the test driver
        // (no dynamic worker exists in this fixture), mirroring
        // `shutdown_production`'s vacuous-true convention for an absent
        // dynamic worker.
        let artifact_path = production_artifact_path("production-reader-crash-recovery.json");
        let document = serde_json::json!({
            "schema": "production-reader-crash-recovery/1",
            "fixture_id": "production-python-reader-crash-abandoned-pin",
            "mapping": {
                "shm_handle": descriptor.shm_handle,
                "total_size": descriptor.total_size,
                "frame_width": descriptor.frame_width,
                "frame_height": descriptor.frame_height,
                "slot_byte_size": descriptor.slot_byte_size,
            },
            "worker_kind": "script",
            "worker_binary": worker_binary.display().to_string(),
            "script_surface": "rust/pokecon/src/worker_binary/script/python.rs::initialize_python",
            "reader_surface": "readFrame/getCameraImage via RingReader::read against the ProductionRuntime camera mapping",
            "crash_injection": "reader.py/Reader ran to Completed first (real RingReader::read proof); hang.py/Hanger time.sleep(600) kept in-flight; one pin planted via pin_current_for_diagnostics().abandon_for_crash_simulation() on the production ring; ManagedWorker::stop(ApplicationShutdown, 3s) forced SIGKILL past the worker shutdown ack timeout",
            "script": {
                "path": "hang.py",
                "class_name": "Hanger",
                "outcome": hang_outcome,
            },
            "shutdown": {
                "cooperative_acknowledged": stop.cooperative_acknowledged,
                "forced": stop.forced,
                "process_reaped": true,
                "exit_success": stop.exit.success,
                "exit_code": stop.exit.code,
                "replacement_reader_absent": true,
            },
            "recovered_pin_count": recovered_pin_count,
            "step5_release_allowed": step5_release_allowed,
            "camera_mapping_fallback": serde_json::to_value(runtime.camera_mapping_fallback())
                .expect("fallback descriptor must serialize"),
            "result": "passed",
            "limitations": [
                "virtual camera/serial backends; no native hardware is claimed",
                "the script-session outcome is Ok(Some(ScriptSessionStop)) built from the real ManagedWorker StopReport; full UserScriptSession/CommandService wiring is out of scope",
                "no dynamic worker exists in this fixture; dynamic_reaped=true is confirmed by the test driver, mirroring shutdown_production's vacuous-true convention for an absent dynamic worker",
                "the abandoned pin is planted through the existing production-mapping diagnostic seam (pin_current_for_diagnostics/abandon_for_crash_simulation) because a hung script's transient RingReader pins do not survive on their own; the real Python reader path is proven by the completed reader.py execution first",
                "injected forced stop (SIGKILL past the shutdown deadline), not a spontaneous OS crash",
                "no Drop/mem::forget or process-exit reclaim proof, no step-9 teardown",
                "dirty-local evidence only; not clean-source or remote-CI proof",
            ],
        });
        let bytes =
            serde_json::to_string_pretty(&document).expect("artifact document must serialize");
        {
            let mut file = atomic_write_file::AtomicWriteFile::open(&artifact_path)
                .expect("artifact must open for atomic write");
            file.write_all(bytes.as_bytes())
                .expect("artifact must write completely");
            file.commit().expect("artifact must commit atomically");
        }
        eprintln!(
            "production reader crash artifact: {}",
            artifact_path.display()
        );
        drop(runtime);
    }

    /// AR-11-29 Drop slice: the real `ProductionRuntime` Drop path retains
    /// an unconfirmed dynamic-reader mapping instead of releasing/unmapping
    /// it.
    ///
    /// A virtual `ProductionRuntime` is built through the real
    /// `SettingsPipeline` + `build_with_backends` composition root, then the
    /// production shutdown tail (`stop_inputs_camera_and_scripts`) aborts the
    /// background tasks and stops the camera writer so no other owner keeps
    /// the mapping alive past `drop`. One known frame is published, the
    /// unconfirmed dynamic reap is persisted via
    /// `retain_dynamic_mapping_after_worker_shutdown(false)` (release gate
    /// false, reader fallback descriptor present), and the real
    /// `drop(runtime)` runs the `Drop` impl (`mem::forget` of the reader
    /// mapping). The OS mapping must then stay observable through the
    /// existing `SharedFrameRing::open` descriptor API with the pre-drop
    /// frame intact. This stays crate-internal because `ProductionRuntime`
    /// is intentionally `pub(crate)`.
    #[allow(clippy::too_many_lines)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_reader_drop_retains_unconfirmed_mapping() {
        use std::collections::BTreeMap;
        use std::ffi::OsString;
        use std::io::Write as _;

        use tempfile::TempDir;

        use crate::camera::ScreenshotMode;
        use crate::camera::backend::CameraBackend;
        use crate::camera::virtual_camera::{
            RecordedFrame, VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
        };
        use crate::camera::{BgrFrame, SharedFrameRing};
        use crate::device::serial::{SerialBackend, VirtualSerialBackend};
        use crate::dynamic_host::StartupDynamicHost;
        use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
        use crate::settings::roots::{BaseDirectories, RootEnvironment};

        let temporary = TempDir::new().expect("isolated roots must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("isolated base directories must resolve");
        std::fs::create_dir_all(base.join("config/pokecon/profiles/default"))
            .expect("isolated default profile must exist");

        let request = PipelineRequest {
            arguments: ["pokecon", "--serial-port", "virtual"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let before_dynamic = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("isolated pipeline must load the startup snapshot");
        let host = Arc::new(
            StartupDynamicHost::new(request.clone(), before_dynamic)
                .expect("startup host must accept the pipeline snapshot"),
        );
        let loaded = host
            .finish_startup()
            .expect("startup host must finish the real pipeline");

        let camera_backend = Arc::new(VirtualCameraBackend::default());
        camera_backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([3, 2, 1])],
        )));
        let serial_backend = Arc::new(VirtualSerialBackend::default());
        let camera_backend_for_runtime: Arc<dyn CameraBackend> = camera_backend.clone();
        let serial_backend_for_runtime: Arc<dyn SerialBackend> = serial_backend.clone();
        let mut runtime = super::ProductionRuntime::build_with_backends(
            request,
            loaded,
            host,
            None,
            ScreenshotMode::Web,
            None,
            camera_backend_for_runtime,
            serial_backend_for_runtime,
        )
        .await
        .expect("production composition root must build on virtual backends");
        assert!(
            runtime.camera.status().camera_opened,
            "the virtual camera must open before the Drop fixture runs"
        );

        // Production shutdown tail first: background tasks hold backend/camera
        // Arcs past `drop`, and the live writer holds the mapping on its own,
        // so both must be gone before the retained fallback becomes the
        // mapping's last owner. No script worker exists here, so the script
        // path retains nothing and the dynamic retain below is the sole
        // retainer.
        runtime.stop_inputs_camera_and_scripts().await;
        assert!(!runtime.camera_writer_unstopped());
        assert!(runtime.shared_memory_release_allowed());
        assert!(runtime.camera_mapping_fallback().is_none());

        // One known frame published after the writer stopped, so the mapping
        // reopened after `drop` has observable content from this execution.
        let descriptor = runtime.camera.mapping_descriptor();
        let ring = runtime.camera.ring();
        let byte_len = usize::try_from(descriptor.frame_width)
            .expect("mapping width must fit usize")
            * usize::try_from(descriptor.frame_height).expect("mapping height must fit usize")
            * 3;
        let solid = BgrFrame::new(
            descriptor.frame_width,
            descriptor.frame_height,
            vec![13_u8; byte_len],
        )
        .expect("solid frame must fit the production mapping");
        ring.publish(&solid)
            .expect("post-shutdown publish must succeed with the writer stopped");
        // Do not let this test-side clone make the post-Drop observation
        // vacuous: after this point the runtime's retained fallback must be
        // the only pre-drop mapping owner.
        drop(ring);

        // The unconfirmed dynamic reap is persisted as retained reader
        // fallback ownership before any release observation.
        runtime.retain_dynamic_mapping_after_worker_shutdown(false);
        assert!(
            !runtime.shared_memory_release_allowed(),
            "an unconfirmed dynamic reap must forbid shared-memory release"
        );
        let fallback = runtime
            .camera_mapping_fallback()
            .expect("an unconfirmed dynamic reap must retain a fallback descriptor");
        assert_eq!(
            fallback, descriptor,
            "the retained fallback must describe the production mapping"
        );

        // Exercise the real `Drop` path: with the gate false it takes the
        // reader fallback and `mem::forget`s the mapping instead of unmapping.
        drop(runtime);

        // The OS mapping must remain observable through the existing
        // descriptor API after `Drop`, with the pre-drop frame intact.
        let reopened = SharedFrameRing::open(descriptor.clone())
            .expect("the retained mapping must stay observable after ProductionRuntime Drop");
        let frame = reopened
            .read_published()
            .expect("published-frame observation must succeed on the retained mapping")
            .expect("the pre-drop frame must survive ProductionRuntime Drop");
        assert_eq!(frame.size().width(), descriptor.frame_width);
        assert_eq!(frame.size().height(), descriptor.frame_height);
        assert_eq!(
            frame.pixels()[0],
            13,
            "the reopened mapping must still hold the pre-drop frame"
        );

        // Measured artifact: every value below is read from this execution.
        // No dynamic worker exists in this fixture; `dynamic_reaped=false`
        // is the unconfirmed path under test, not the vacuous-true absent
        // convention. Process-exit reclaim is explicitly unproven: this
        // packet observes the mapping inside the same process after `Drop`.
        let artifact_path = production_artifact_path("production-reader-drop-retention.json");
        let document = serde_json::json!({
            "schema": "production-reader-drop-retention/1",
            "fixture_id": "production-reader-drop-retains-unconfirmed-mapping",
            "mapping": {
                "shm_handle": descriptor.shm_handle,
                "total_size": descriptor.total_size,
                "frame_width": descriptor.frame_width,
                "frame_height": descriptor.frame_height,
                "slot_byte_size": descriptor.slot_byte_size,
            },
            "fallback_before_drop": serde_json::to_value(&fallback)
                .expect("fallback descriptor must serialize"),
            "writer_unstopped_before_drop": false,
            "release_allowed_before_drop": false,
            "drop_exercised": true,
            "mapping_observable_after_drop": true,
            "frame_observable_after_drop": {
                "present": true,
                "frame_width": frame.size().width(),
                "frame_height": frame.size().height(),
                "first_byte": frame.pixels()[0],
            },
            "process_exit_reclaim_proven": false,
            "result": "passed",
            "limitations": [
                "virtual camera/serial backends; no native hardware is claimed",
                "no dynamic worker exists in this fixture; dynamic_reaped=false exercises the unconfirmed-reap retention path, not the absent-worker vacuous-true convention",
                "the script path retains nothing here (no script worker); the dynamic retain is the sole retainer",
                "the shutdown tail runs before retain so background tasks and the stopped writer hold no mapping Arc past drop; the retained fallback is the mapping's last owner by construction, not by refcount assertion",
                "observes the mapping inside the same process after Drop via SharedFrameRing::open/read_published; actual process-exit reclaim is explicitly unproven and no child-process proof is claimed",
                "cross-platform observation uses the portable descriptor open API; POSIX name-unlink versus Windows handle-retention OS semantics are not distinguished here",
                "no step-9 teardown, no crash-abandoned-pin recovery, no live hardware, dirty-local evidence only",
            ],
        });
        let bytes =
            serde_json::to_string_pretty(&document).expect("artifact document must serialize");
        {
            let mut file = atomic_write_file::AtomicWriteFile::open(&artifact_path)
                .expect("artifact must open for atomic write");
            file.write_all(bytes.as_bytes())
                .expect("artifact must write completely");
            file.commit().expect("artifact must commit atomically");
        }
        eprintln!(
            "production reader Drop retention artifact: {}",
            artifact_path.display()
        );
    }

    /// Child-process observation record for the AR-11-29 process-exit
    /// reclaim proof. Written by the observer child, validated by the
    /// orchestrator; never hand-edited.
    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct ReclaimObservation {
        stdin_eof_observed: bool,
        child_open_succeeded: bool,
        frame_present: bool,
        first_byte: Option<u8>,
        open_error: Option<String>,
        child_pid: u32,
        descriptor: MappingDescriptor,
    }

    fn process_exit_reclaim_proven(observation: &ReclaimObservation) -> bool {
        observation.stdin_eof_observed
            && !observation.child_open_succeeded
            && !observation.frame_present
            && observation.first_byte.is_none()
            && observation.open_error.is_some()
    }

    /// Environment role for the reclaim helpers. `None` is the
    /// orchestrator; `Some("parent")` creates, publishes, forgets, and
    /// exits; `Some("child")` observes after the parent's exit.
    fn reclaim_role() -> Option<String> {
        std::env::var("POKECON_RECLAIM_ROLE").ok()
    }

    fn reclaim_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(
            std::env::var_os("POKECON_RECLAIM_DIR").expect("reclaim helper directory must be set"),
        )
    }

    /// Full libtest path of a helper so `--exact` re-execution selects one
    /// test even though every helper lives in this module.
    fn reclaim_test_path(function: &str) -> String {
        let module = module_path!();
        let relative = module.split_once("::").map_or(module, |(_, rest)| rest);
        format!("{relative}::{function}")
    }

    fn spawn_reclaim_helper(
        role: &str,
        dir: &std::path::Path,
        function: &str,
        stdin: std::process::Stdio,
    ) -> std::process::Child {
        let executable = std::env::current_exe().expect("test executable must be available");
        std::process::Command::new(executable)
            .args(["--exact", &reclaim_test_path(function), "--nocapture"])
            .env("POKECON_RECLAIM_ROLE", role)
            .env("POKECON_RECLAIM_DIR", dir)
            .stdin(stdin)
            .spawn()
            .expect("reclaim helper must start")
    }

    #[cfg(unix)]
    fn unlink_reclaim_mapping(descriptor: &MappingDescriptor) -> bool {
        SharedFrameRing::open(descriptor.clone())
            .and_then(|ring| ring.reclaim_probe_cleanup())
            .is_ok()
    }

    #[cfg(not(unix))]
    fn unlink_reclaim_mapping(_descriptor: &MappingDescriptor) -> bool {
        false
    }

    struct ChildReaper {
        child: std::process::Child,
    }

    impl ChildReaper {
        fn new(child: std::process::Child) -> Self {
            Self { child }
        }

        fn id(&self) -> u32 {
            self.child.id()
        }

        fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
            self.child.stdin.take()
        }

        fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
            self.child.try_wait()
        }
    }

    impl Drop for ChildReaper {
        fn drop(&mut self) {
            if !matches!(self.child.try_wait(), Ok(Some(_))) {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    struct ReclaimMappingCleanup {
        directory: std::path::PathBuf,
        active: bool,
    }

    impl ReclaimMappingCleanup {
        fn new(directory: &std::path::Path) -> Self {
            Self {
                directory: directory.to_owned(),
                active: true,
            }
        }

        fn disarm(&mut self) {
            self.active = false;
        }
    }

    impl Drop for ReclaimMappingCleanup {
        fn drop(&mut self) {
            if !self.active {
                return;
            }
            let Ok(bytes) = std::fs::read(self.directory.join("descriptor.json")) else {
                return;
            };
            let Ok(descriptor) = serde_json::from_slice::<MappingDescriptor>(&bytes) else {
                return;
            };
            let _ = unlink_reclaim_mapping(&descriptor);
        }
    }

    const RECLAIM_PUBLISHED_FIRST_BYTE: u8 = 13;

    /// Reclaim parent helper: creates one mapping, publishes one known
    /// frame, then retains the mapping exactly like `ProductionRuntime` Drop
    /// (`mem::forget`, no unlink) and waits for the orchestrator to release
    /// it. The orchestrator closes this pipe after spawning the observer, so
    /// the parent exits without running destructors after the observer launch;
    /// the observer's own EOF gate is released only after the parent is reaped.
    #[test]
    fn production_process_exit_reclaim_parent_helper() {
        use std::io::Read as _;

        use crate::camera::{BgrFrame, CaptureResolution, SharedFrameRing};

        if reclaim_role().as_deref() != Some("parent") {
            return;
        }
        let dir = reclaim_dir();
        let ring = SharedFrameRing::create(CaptureResolution::R640x360)
            .expect("reclaim parent must create its mapping");
        ring.publish(&BgrFrame::solid(
            CaptureResolution::R640x360,
            [RECLAIM_PUBLISHED_FIRST_BYTE, 14, 15],
        ))
        .expect("reclaim parent must publish its known frame");
        let descriptor = ring.descriptor();
        std::fs::write(
            dir.join("descriptor.json"),
            serde_json::to_string_pretty(&descriptor).expect("descriptor must serialize"),
        )
        .expect("descriptor file must be writable");
        // Retain the OS mapping until process exit instead of unmapping,
        // mirroring the `ProductionRuntime` reader-fallback `Drop` path.
        std::mem::forget(ring);
        // The orchestrator closes this pipe after spawning the observer, so
        // the parent cannot exit before the observer launch.
        let mut stdin_sink = Vec::new();
        std::io::stdin()
            .read_to_end(&mut stdin_sink)
            .expect("reclaim parent must wait for orchestrator release");
        std::process::exit(0);
    }

    /// Reclaim observer helper: blocks on stdin until the orchestrator closes
    /// its pipe after reaping the parent (EOF), then opens the published
    /// descriptor in this process and records whether the stale mapping/frame
    /// survived. Every outcome is recorded; nothing is asserted here so a
    /// surprising platform contract becomes data, not a masked failure.
    #[test]
    fn production_process_exit_reclaim_child_observer() {
        use std::io::Read as _;

        use crate::camera::SharedFrameRing;

        if reclaim_role().as_deref() != Some("child") {
            return;
        }
        let dir = reclaim_dir();
        let descriptor: MappingDescriptor = serde_json::from_slice(
            &std::fs::read(dir.join("descriptor.json")).expect("observer must read its descriptor"),
        )
        .expect("observer descriptor must parse");
        let mut stdin_sink = Vec::new();
        let stdin_eof_observed = std::io::stdin().read_to_end(&mut stdin_sink).is_ok();
        let (child_open_succeeded, frame_present, first_byte, open_error) =
            match SharedFrameRing::open(descriptor.clone()) {
                Ok(ring) => match ring.read_published() {
                    Ok(Some(frame)) => (true, true, frame.pixels().first().copied(), None),
                    Ok(None) => (true, false, None, None),
                    Err(error) => (true, false, None, Some(format!("{error:?}"))),
                },
                Err(error) => (false, false, None, Some(format!("{error:?}"))),
            };
        let observation = ReclaimObservation {
            stdin_eof_observed,
            child_open_succeeded,
            frame_present,
            first_byte,
            open_error,
            child_pid: std::process::id(),
            descriptor,
        };
        std::fs::write(
            dir.join("result.json"),
            serde_json::to_string_pretty(&observation).expect("observation must serialize"),
        )
        .expect("observation file must be writable");
    }

    /// AR-11-29 child-process slice: a parent process publishes one known
    /// frame, deliberately retains/forgets its mapping, and exits; a
    /// spawned child observes the descriptor only after the parent is gone
    /// and records whether the stale mapping/frame remains. This is the
    /// adversarial counterpart to
    /// `production_reader_drop_retains_unconfirmed_mapping`, which only
    /// reopens the mapping in the same process: here the creator's death is
    /// proven twice (the orchestrator reaps the parent, and the observer
    /// blocks on stdin EOF before opening), the observer proves it runs in
    /// a different process via its pid, and the verdict is derived from
    /// this execution instead of asserting a platform contract. Fail-closed:
    /// a missing, unreadable, mismatched, or pre-exit observation fails the
    /// test rather than claiming proof.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn production_process_exit_reclaim_across_processes() {
        use std::io::Write as _;
        use tempfile::TempDir;

        if reclaim_role().is_some() {
            return;
        }
        let temporary = TempDir::new().expect("isolated reclaim directory must exist");
        let dir = temporary.path().to_path_buf();
        let mut mapping_cleanup = ReclaimMappingCleanup::new(&dir);
        let mut parent = ChildReaper::new(spawn_reclaim_helper(
            "parent",
            &dir,
            "production_process_exit_reclaim_parent_helper",
            std::process::Stdio::piped(),
        ));
        let parent_pid = parent.id();
        let parent_stdin = parent
            .take_stdin()
            .expect("reclaim parent stdin must be piped");
        let descriptor_deadline = Instant::now() + Duration::from_secs(30);
        let published: MappingDescriptor = loop {
            if let Ok(bytes) = std::fs::read(dir.join("descriptor.json"))
                && let Ok(descriptor) = serde_json::from_slice::<MappingDescriptor>(&bytes)
            {
                break descriptor;
            }
            assert!(
                Instant::now() < descriptor_deadline,
                "reclaim parent must publish its descriptor before its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        };

        let mut observer = ChildReaper::new(spawn_reclaim_helper(
            "child",
            &dir,
            "production_process_exit_reclaim_child_observer",
            std::process::Stdio::piped(),
        ));
        let observer_stdin = observer
            .take_stdin()
            .expect("reclaim observer stdin must be piped");
        // Release the parent after the observer is spawned. The observer's
        // independent EOF gate remains open, so it cannot open the mapping
        // until the orchestrator closes that gate after reaping the parent.
        drop(parent_stdin);
        let parent_deadline = Instant::now() + Duration::from_secs(30);
        let parent_status = loop {
            if let Some(status) = parent.try_wait().expect("parent status must be readable") {
                break status;
            }
            assert!(
                Instant::now() < parent_deadline,
                "reclaim parent must exit before its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            parent_status.success(),
            "reclaim parent must exit successfully after forgetting its mapping"
        );

        // Closing the observer's stdin now proves that it opens the mapping
        // only after the orchestrator has reaped the parent.
        drop(observer_stdin);
        let result_path = dir.join("result.json");
        let result_deadline = Instant::now() + Duration::from_secs(30);
        let observation: ReclaimObservation = loop {
            if let Ok(bytes) = std::fs::read(&result_path)
                && let Ok(observation) = serde_json::from_slice::<ReclaimObservation>(&bytes)
            {
                break observation;
            }
            assert!(
                Instant::now() < result_deadline,
                "reclaim observer must record its post-exit observation before its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let observer_deadline = Instant::now() + Duration::from_secs(30);
        let observer_status = loop {
            if let Some(status) = observer
                .try_wait()
                .expect("observer status must be readable")
            {
                break status;
            }
            assert!(
                Instant::now() < observer_deadline,
                "reclaim observer must exit before its deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            observer_status.success(),
            "reclaim observer must exit successfully after recording its observation"
        );

        // Fail-closed: the observation only counts when the observer proved
        // it ran in another process after the parent's exit, opened this
        // run's descriptor, and reported a self-consistent verdict.
        assert!(
            observation.stdin_eof_observed,
            "observer must prove parent exit via stdin EOF before opening the mapping"
        );
        assert_ne!(
            observation.child_pid,
            std::process::id(),
            "observer must run in a different process than the orchestrator"
        );
        assert_ne!(
            observation.child_pid, parent_pid,
            "observer must run in a different process than the exited parent"
        );
        assert_eq!(
            observation.descriptor, published,
            "observer must have opened the descriptor this run published"
        );
        assert!(
            !published.shm_handle.is_empty(),
            "published descriptor must name a real mapping"
        );
        let process_exit_reclaim_proven = process_exit_reclaim_proven(&observation);

        // Best-effort cleanup of the deliberately leaked POSIX name so
        // repeated runs do not accumulate mappings; the observation above
        // is already recorded and unaffected. The guard retries cleanup if a
        // later assertion or artifact write panics.
        let cleanup_unlinked = unlink_reclaim_mapping(&published);
        if cleanup_unlinked {
            mapping_cleanup.disarm();
        }

        // Measured artifact: every value below is read from this execution.
        let artifact_path = production_artifact_path("production-process-exit-reclaim.json");
        let document = serde_json::json!({
            "schema": "production-process-exit-reclaim/1",
            "fixture_id": "production-process-exit-reclaim-across-processes",
            "platform": std::env::consts::OS,
            "mapping": {
                "shm_handle": published.shm_handle,
                "total_size": published.total_size,
                "frame_width": published.frame_width,
                "frame_height": published.frame_height,
                "slot_byte_size": published.slot_byte_size,
            },
            "published_first_byte": RECLAIM_PUBLISHED_FIRST_BYTE,
            "parent_pid": parent_pid,
            "parent_exit_observed": parent_status.success(),
            "child_pid": observation.child_pid,
            "child_stdin_eof_observed": observation.stdin_eof_observed,
            "child_open_succeeded": observation.child_open_succeeded,
            "child_open_error": observation.open_error,
            "child_frame_observed": {
                "present": observation.frame_present,
                "first_byte": observation.first_byte,
            },
            "process_exit_reclaim_proven": process_exit_reclaim_proven,
            "cleanup_unlinked": cleanup_unlinked,
            "result": "passed",
            "limitations": [
                "no camera backend or hardware: the parent exercises the raw SharedFrameRing create/publish/forget primitive that ProductionRuntime Drop uses, while the same-process production test covers the real Drop composition",
                "the forgotten mapping is never unlinked, mirroring the reader-fallback Drop path (unlike the writer path, which unlinks via record_writer_unstopped); on POSIX the name can therefore outlive process exit and the stale frame stays observable",
                "parent death is proven twice: the orchestrator reaps the parent process, and the observer blocks on stdin EOF before opening the mapping",
                "no Windows/POSIX reclaim semantics are asserted beyond the recorded observation; process_exit_reclaim_proven is derived from this execution only",
                "no step-9 teardown, no crash-abandoned-pin recovery, no live hardware, dirty-local evidence only",
            ],
        });
        let bytes =
            serde_json::to_string_pretty(&document).expect("artifact document must serialize");
        {
            let mut file = atomic_write_file::AtomicWriteFile::open(&artifact_path)
                .expect("artifact must open for atomic write");
            file.write_all(bytes.as_bytes())
                .expect("artifact must write completely");
            file.commit().expect("artifact must commit atomically");
        }
        eprintln!(
            "production process-exit reclaim artifact: {}",
            artifact_path.display()
        );
    }

    #[test]
    fn reclaim_observation_serialization_round_trip() {
        let observation = ReclaimObservation {
            stdin_eof_observed: true,
            child_open_succeeded: true,
            frame_present: true,
            first_byte: Some(13),
            open_error: None,
            child_pid: 1,
            descriptor: MappingDescriptor {
                shm_handle: "test-mapping".to_owned(),
                total_size: 9,
                frame_width: 640,
                frame_height: 360,
                slot_byte_size: 3,
            },
        };
        let bytes = serde_json::to_vec(&observation).expect("observation must serialize");
        assert_eq!(
            serde_json::from_slice::<ReclaimObservation>(&bytes).expect("observation must parse"),
            observation
        );
    }

    #[test]
    fn reclaim_verdict_requires_consistent_open_failure() {
        let descriptor = MappingDescriptor {
            shm_handle: "test-mapping".to_owned(),
            total_size: 9,
            frame_width: 640,
            frame_height: 360,
            slot_byte_size: 3,
        };
        let mut observation = ReclaimObservation {
            stdin_eof_observed: true,
            child_open_succeeded: false,
            frame_present: false,
            first_byte: None,
            open_error: Some("mapping not found".to_owned()),
            child_pid: 1,
            descriptor,
        };
        assert!(
            process_exit_reclaim_proven(&observation),
            "consistent post-exit open failure must prove reclaim: {observation:?}"
        );

        observation.open_error = None;
        assert!(
            !process_exit_reclaim_proven(&observation),
            "open failure without an error must not prove reclaim: {observation:?}"
        );
        observation.open_error = Some("mapping not found".to_owned());
        observation.child_open_succeeded = true;
        assert!(
            !process_exit_reclaim_proven(&observation),
            "a successful mapping open must not prove reclaim: {observation:?}"
        );
        observation.child_open_succeeded = false;
        observation.first_byte = Some(13);
        assert!(
            !process_exit_reclaim_proven(&observation),
            "an observed frame byte must not prove reclaim: {observation:?}"
        );
        observation.first_byte = None;
        observation.frame_present = true;
        assert!(
            !process_exit_reclaim_proven(&observation),
            "an observed frame must not prove reclaim: {observation:?}"
        );
        observation.frame_present = false;
        observation.stdin_eof_observed = false;
        assert!(
            !process_exit_reclaim_proven(&observation),
            "an observation before parent EOF must not prove reclaim: {observation:?}"
        );
    }

    /// AR-11-29 rollback slice: a failure after the production camera session
    /// has started must close that session before the build future returns.
    ///
    /// The failure is injected at the real command-root initialization point,
    /// after the shared camera and serial managers have been installed into
    /// `BuildCleanup`. The virtual backend is only an I/O witness; construction
    /// still runs the same `ProductionRuntime::build_with_backends` root used
    /// by the native constructor. `closed_sessions` makes the resource release
    /// observable instead of treating an `Err` return as cleanup proof.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_runtime_build_failure_rolls_back_started_resources() {
        use std::collections::BTreeMap;
        use std::ffi::OsString;

        use tempfile::TempDir;

        use crate::camera::ScreenshotMode;
        use crate::camera::backend::CameraBackend;
        use crate::camera::virtual_camera::{
            VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
        };
        use crate::device::serial::{SerialBackend, VirtualSerialBackend};
        use crate::dynamic_host::StartupDynamicHost;
        use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
        use crate::settings::roots::{BaseDirectories, RootEnvironment};

        let temporary = TempDir::new().expect("isolated roots must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("isolated base directories must resolve");
        std::fs::create_dir_all(base.join("config/pokecon/profiles/default"))
            .expect("isolated default profile must exist");

        let request = PipelineRequest {
            arguments: ["pokecon", "--serial-port", "virtual"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let before_dynamic = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("isolated pipeline must load the startup snapshot");
        let host = Arc::new(
            StartupDynamicHost::new(request.clone(), before_dynamic)
                .expect("startup host must accept the pipeline snapshot"),
        );
        let loaded = host
            .finish_startup()
            .expect("startup host must finish the real pipeline");

        // Force the real post-camera command-root step to fail. The file is
        // removed with the temporary roots after the build and cannot mask
        // cleanup in a later test.
        let command_root = loaded.roots.data.join("Commands");
        std::fs::create_dir_all(
            command_root
                .parent()
                .expect("command root must have a data parent"),
        )
        .expect("data root must be writable for the fault fixture");
        std::fs::write(&command_root, b"fault: command root is not a directory")
            .expect("fault fixture must occupy the command root path");

        let camera_backend = Arc::new(VirtualCameraBackend::default());
        camera_backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
            30,
            [RecordedFrame::Solid([1, 2, 3])],
        )));
        let serial_backend = Arc::new(VirtualSerialBackend::default());
        let camera_backend_for_runtime: Arc<dyn CameraBackend> = camera_backend.clone();
        let serial_backend_for_runtime: Arc<dyn SerialBackend> = serial_backend.clone();

        let error = super::ProductionRuntime::build_with_backends(
            request,
            loaded,
            host,
            None,
            ScreenshotMode::Web,
            None,
            camera_backend_for_runtime,
            serial_backend_for_runtime,
        )
        .await
        .expect_err("command-root fault must reject the production build");
        assert!(
            error.contains("user command root initialization failed"),
            "fault must be reported from the production initialization step: {error}"
        );
        assert_eq!(
            camera_backend.closed_sessions(),
            1,
            "BuildCleanup must close the started camera session before returning Err"
        );
        assert_eq!(
            serial_backend.attempt_count(),
            0,
            "startup config stores the serial selector but does not open hardware"
        );
    }
}
