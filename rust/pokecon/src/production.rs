//! Construction and bounded shutdown of the production application services.

use std::str::FromStr as _;
use std::sync::Arc;
use std::time::Duration;

use crate::camera::{
    CameraConfig, CameraManager, CaptureResolution, FlipMode, NativeCameraBackend,
    ScreenshotFormat, ScreenshotMode, ScreenshotRuntimeSettings, ScreenshotService,
};
use crate::settings::pipeline::{LoadedSettings, PipelineRequest};
use crate::settings::service::SettingsService;
use axum::Router;
use base64::Engine as _;
use pokecon_desktop::DesktopRuntimeSettings;
use pokecon_device::controller::ControllerState;
use pokecon_device::notification::{
    DiscordTransport, NotificationService, ReqwestDiscordTransport, UnavailableDiscordTransport,
    WindowsNativeNotificationTransport,
};
use pokecon_device::serial::{
    ControllerFormat, NativeSerialBackend, SerialConfig, SerialManager, SerialSettingsApplier,
};
use pokecon_server::api::{SerialData, SerialEncoding, StateChangeCause};
use pokecon_server::backend::RestBackend;
use pokecon_server::realtime::RealtimeTransportConfig;
use pokecon_server::realtime_connection::RealtimeConnectionConfig;
use pokecon_server::rest;
use pokecon_server::state::StateHub;
use pokecon_server::webrtc::{WebRtcMedia, WebRtcMediaConfig, WebRtcPeerConfig};
use pokecon_server::websocket::{
    MotionJpegFeed, WebSocketBackend, WebSocketConfig, WebSocketTransport,
};
use pokecon_worker::dynamic::DynamicWorkerClient;
use serde::de::DeserializeOwned;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::UiMode;
use crate::application_backend::{
    ApplicationBackend, ApplicationBackendParts, initial_settings_snapshot, initial_state_snapshot,
};
use crate::command_service::{CommandService, DynamicCommandBridge, StaticCommandBridge};
use crate::dynamic_host::StartupDynamicHost;
use crate::profile_service::ProfileService;
use crate::script_host::{ProductionScriptHostFactory, ScriptUiCoordinator};
use crate::script_runtime::ManagedUserScriptFactory;
use crate::settings_runtime::{
    CameraSettingsApplier, CompositeSettingsApplier, DesktopSettingsApplier, HostSettingsApplier,
    NotificationSettingsApplier, RealtimeSettingsApplier, notification_config,
    reconcile_desktop_settings,
};

const STATE_HISTORY_CAPACITY: usize = 256;
const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(2);

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
    #[allow(clippy::too_many_lines)]
    pub(crate) async fn build(
        request: PipelineRequest,
        loaded: LoadedSettings,
        host: Arc<StartupDynamicHost>,
        dynamic: Option<Arc<DynamicWorkerClient>>,
        ui_mode: UiMode,
        desktop_settings: Option<DesktopRuntimeSettings>,
    ) -> Result<Self, String> {
        let runtime = tokio::runtime::Handle::current();
        let camera_config = camera_config(&loaded)?;
        let flip = FlipMode::from_str(setting_text(&loaded, "camera.flip_mode")?)
            .map_err(|_error| "camera flip setting is invalid".to_owned())?;
        let screenshot_format =
            ScreenshotFormat::from_str(setting_text(&loaded, "camera.screenshot_format")?)
                .map_err(|_error| "screenshot format setting is invalid".to_owned())?;
        let jpeg_quality = setting_u8(&loaded, "jpeg_quality")?;
        let screenshot_settings = ScreenshotRuntimeSettings::new(screenshot_format, jpeg_quality)
            .map_err(|_error| "screenshot settings are invalid".to_owned())?;
        let camera =
            CameraManager::start(Arc::new(NativeCameraBackend), camera_config.clone(), flip)
                .map_err(|_error| "camera runtime initialization failed".to_owned())?;
        let screenshot_mode = match ui_mode {
            UiMode::Web => ScreenshotMode::Web,
            UiMode::Desktop => ScreenshotMode::Desktop,
        };
        let screenshots = ScreenshotService::new(
            camera.frame_source(),
            loaded.roots.data.clone(),
            screenshot_mode,
            screenshot_settings.clone(),
        );

        let serial = SerialManager::new(Arc::new(NativeSerialBackend));
        let serial_port = setting_text(&loaded, "serial.port")?.to_owned();
        let serial_baud_rate = setting_u32(&loaded, "serial.baud_rate")?;
        let serial_format = ControllerFormat::parse(setting_text(&loaded, "serial.data_format")?)
            .ok_or_else(|| "serial format setting is invalid".to_owned())?;
        if !serial_port.is_empty() {
            serial
                .update_config(
                    SerialConfig::new(serial_port.clone(), serial_baud_rate, serial_format)
                        .map_err(|_error| "serial settings are invalid".to_owned())?,
                )
                .await
                .map_err(|_error| "serial runtime initialization failed".to_owned())?;
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
        let initial_notification_config = notification_config(&raw_values(&loaded))?;
        let notifications = Arc::new(NotificationService::new(
            initial_notification_config,
            notification_transport,
            Arc::new(WindowsNativeNotificationTransport),
        ));

        let (realtime_applier, realtime_settings) = RealtimeSettingsApplier::new(&loaded)?;
        let (realtime, motion_jpeg, fallback_media_task) = start_media(
            camera.frame_source(),
            screenshot_settings.clone(),
            realtime_settings.clone(),
        )
        .await;

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
            .map_err(|_error| "camera settings adapter initialization failed".to_owned())?,
        );
        applier.push(
            SerialSettingsApplier::new(
                serial.clone(),
                runtime.clone(),
                serial_port,
                serial_baud_rate,
                serial_format,
            )
            .map_err(|_error| "serial settings adapter initialization failed".to_owned())?,
        );
        applier.push(NotificationSettingsApplier::new(
            Arc::clone(&notifications),
            runtime,
            &loaded,
        )?);
        applier.push(realtime_applier);
        let settings = SettingsService::new(loaded.clone(), Box::new(applier));
        let settings_snapshot = initial_settings_snapshot(&settings);
        let state_snapshot = initial_state_snapshot(&host, &camera, &serial).await?;
        let hub = StateHub::new(settings_snapshot, state_snapshot, STATE_HISTORY_CAPACITY)
            .map_err(|_error| "application state initialization failed".to_owned())?;

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
            .map_err(|_error| "WebSocket transport initialization failed".to_owned())?;
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
            .map_err(|_error| "user command root initialization failed".to_owned())?;
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
        let mut tasks = Vec::new();
        if let Some(task) = fallback_media_task {
            tasks.push(task);
        }
        tasks.push(spawn_serial_events(&serial, broker));
        tasks.push(spawn_runtime_reconciler(
            Arc::clone(&backend),
            host.subscribe_runtime_changes(),
            desktop_settings,
        ));
        tasks.push(spawn_command_recompute(
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
            tasks,
            script_shutdown_timeout: Duration::from_millis(setting_u64(
                &loaded,
                "python.script.shutdown_timeout_ms",
            )?),
        })
    }

    pub(crate) fn router(&self) -> Router {
        self.router.clone()
    }

    /// Executes shutdown steps 1 through 3 after `AppShutdownPre` has closed
    /// dynamic mutations.
    pub(crate) async fn stop_inputs_camera_and_scripts(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        let controller = {
            let arbiter = self.backend.host().controller_safety().arbiter();
            let mut arbiter = arbiter.lock();
            arbiter.force_release_all();
            arbiter.output()
        };
        let _ = timeout(
            SERVICE_STOP_TIMEOUT,
            self.serial.send_controller_state(controller),
        )
        .await;
        let camera = self.camera.clone();
        match tokio::task::spawn_blocking(move || camera.shutdown(SERVICE_STOP_TIMEOUT)).await {
            Ok(Ok(())) => {}
            Ok(Err(unstopped)) => tracing::error!(
                mapping = ?unstopped.mapping_descriptor(),
                "camera writer did not stop before its deadline"
            ),
            Err(error) => tracing::error!(%error, "camera shutdown task failed"),
        }
        if let Err(error) = self.commands.shutdown(self.script_shutdown_timeout).await {
            tracing::error!(%error, "user-script worker shutdown failed");
        }
    }

    /// Executes shutdown steps 6 and 7 after the dynamic worker is reaped.
    pub(crate) async fn stop_serial(&self) {
        {
            let arbiter = self.backend.host().controller_safety().arbiter();
            arbiter.lock().force_release_all();
        }
        let _ = timeout(
            SERVICE_STOP_TIMEOUT,
            self.serial.send_controller_state(ControllerState::NEUTRAL),
        )
        .await;
        match timeout(SERVICE_STOP_TIMEOUT, self.serial.disconnect()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::error!(%error, "serial shutdown failed"),
            Err(_) => tracing::error!("serial shutdown timed out"),
        }
    }
}

async fn start_media(
    frames: crate::camera::LatestFrameSource,
    screenshot_settings: ScreenshotRuntimeSettings,
    runtime_settings: tokio::sync::watch::Receiver<
        pokecon_server::realtime_connection::RealtimeRuntimeSettings,
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
    broker: pokecon_server::websocket::WebSocketBroker,
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
