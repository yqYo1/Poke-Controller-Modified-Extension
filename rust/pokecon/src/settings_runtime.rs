//! Runtime side-effect composition for the canonical settings service.

mod camera;
mod device;

pub(crate) use camera::CameraSettingsApplier;
pub(crate) use device::SerialSettingsApplier;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use crate::desktop::{CloseBehavior, DesktopRuntimeSettings};
use crate::device::{
    DiscordNotificationConfig, DiscordWebhookUrl, NotificationConfig, NotificationService,
    WindowsNotificationConfig,
};
use crate::dynamic::DynamicHost as _;
use crate::server::realtime_connection::RealtimeRuntimeSettings;
use crate::server::websocket::WebSocketRuntimeSettings;
use crate::settings::pipeline::LoadedSettings;
use crate::settings::service::{PatchClass, RuntimeSettingsApplier};
use serde_json::Value;
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::sync::watch;
use url::Url;

use crate::dynamic_host::StartupDynamicHost;

/// Applies one settings class to a sequence of independent runtime adapters.
/// If a later adapter rejects the value, every earlier adapter is restored in
/// reverse order before control returns to the persistence service.
pub(crate) struct CompositeSettingsApplier {
    adapters: Vec<Box<dyn RuntimeSettingsApplier>>,
    current: BTreeMap<String, Value>,
}

impl CompositeSettingsApplier {
    #[must_use]
    pub(crate) fn new(loaded: &LoadedSettings) -> Self {
        Self {
            adapters: Vec::new(),
            current: raw_values(loaded),
        }
    }

    pub(crate) fn push(&mut self, adapter: impl RuntimeSettingsApplier + 'static) {
        self.adapters.push(Box::new(adapter));
    }
}

impl RuntimeSettingsApplier for CompositeSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        let previous = changes
            .keys()
            .filter_map(|id| {
                self.current
                    .get(id)
                    .map(|value| (id.clone(), value.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        for index in 0..self.adapters.len() {
            if let Err(error) = self.adapters[index].apply(class, changes) {
                for applied in (0..index).rev() {
                    self.adapters[applied].rollback(class, &previous);
                }
                return Err(error);
            }
        }
        self.current.extend(changes.clone());
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        for adapter in self.adapters.iter_mut().rev() {
            adapter.rollback(class, previous);
        }
        self.current.extend(previous.clone());
    }
}

pub(crate) struct HostSettingsApplier {
    host: Arc<StartupDynamicHost>,
}

impl HostSettingsApplier {
    pub(crate) const fn new(host: Arc<StartupDynamicHost>) -> Self {
        Self { host }
    }
}

impl RuntimeSettingsApplier for HostSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if class == PatchClass::Profile {
            return Ok(());
        }
        self.host
            .apply_settings(changes)
            .map(|_settings| ())
            .map_err(|_error| "application settings projection failed".to_owned())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        if class != PatchClass::Profile && self.host.apply_settings(previous).is_err() {
            tracing::error!(
                diagnostic_id = "HOST_SETTINGS_ROLLBACK_FAILED",
                "application settings projection could not be restored"
            );
        }
    }
}

pub(crate) struct DesktopSettingsApplier {
    settings: DesktopRuntimeSettings,
}

impl DesktopSettingsApplier {
    pub(crate) const fn new(settings: DesktopRuntimeSettings) -> Self {
        Self { settings }
    }
}

impl RuntimeSettingsApplier for DesktopSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if class != PatchClass::Ordinary {
            return Ok(());
        }
        if let Some(value) = changes.get("ui.desktop.close_behavior") {
            self.settings
                .set_close_behavior(parse_close_behavior(value)?);
        }
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        if class != PatchClass::Ordinary {
            return;
        }
        if let Some(behavior) = previous
            .get("ui.desktop.close_behavior")
            .and_then(|value| parse_close_behavior(value).ok())
        {
            self.settings.set_close_behavior(behavior);
        }
    }
}

pub(crate) fn reconcile_desktop_settings(
    settings: &DesktopRuntimeSettings,
    loaded: &LoadedSettings,
) -> Result<(), String> {
    let value = loaded
        .settings
        .get("ui.desktop.close_behavior")
        .ok_or_else(|| "desktop close setting is missing".to_owned())?;
    settings.set_close_behavior(parse_close_behavior(&value.value)?);
    Ok(())
}

fn parse_close_behavior(value: &Value) -> Result<CloseBehavior, String> {
    value
        .as_str()
        .ok_or_else(|| "desktop close setting has an invalid type".to_owned())?
        .parse()
        .map_err(|_error| "desktop close setting has an invalid value".to_owned())
}

pub(crate) struct NotificationSettingsApplier {
    service: Arc<NotificationService>,
    runtime: Handle,
    values: BTreeMap<String, Value>,
}

impl NotificationSettingsApplier {
    /// Builds and validates the startup notification projection.
    ///
    /// # Errors
    ///
    /// Returns a fixed diagnostic if a URL or typed setting is invalid.
    pub(crate) fn new(
        service: Arc<NotificationService>,
        runtime: Handle,
        loaded: &LoadedSettings,
    ) -> Result<Self, String> {
        let values = raw_values(loaded);
        notification_config(&values)?;
        Ok(Self {
            service,
            runtime,
            values,
        })
    }

    fn update(&self, values: &BTreeMap<String, Value>) -> Result<(), String> {
        let config = notification_config(values)?;
        run_sync(&self.runtime, self.service.update_config(config));
        Ok(())
    }
}

impl RuntimeSettingsApplier for NotificationSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if class != PatchClass::Ordinary
            || !changes.keys().any(|id| id.starts_with("notifications."))
        {
            return Ok(());
        }
        let mut next = self.values.clone();
        next.extend(changes.clone());
        self.update(&next)?;
        self.values = next;
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        if class != PatchClass::Ordinary
            || !previous.keys().any(|id| id.starts_with("notifications."))
        {
            return;
        }
        let mut restored = self.values.clone();
        restored.extend(previous.clone());
        if self.update(&restored).is_ok() {
            self.values = restored;
        } else {
            tracing::error!(
                diagnostic_id = "NOTIFICATION_SETTINGS_ROLLBACK_FAILED",
                "notification settings could not be restored"
            );
        }
    }
}

pub(crate) struct RealtimeSettingsApplier {
    sender: watch::Sender<RealtimeRuntimeSettings>,
    values: BTreeMap<String, Value>,
}

impl RealtimeSettingsApplier {
    /// Creates the live settings channel used by new WebRTC attempts.
    ///
    /// # Errors
    ///
    /// Returns a fixed diagnostic for an invalid STUN URI or interval.
    pub(crate) fn new(
        loaded: &LoadedSettings,
    ) -> Result<(Self, watch::Receiver<RealtimeRuntimeSettings>), String> {
        let values = raw_values(loaded);
        let initial = realtime_settings(&values)?;
        let (sender, receiver) = watch::channel(initial);
        Ok((Self { sender, values }, receiver))
    }
}

impl RuntimeSettingsApplier for RealtimeSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if class != PatchClass::Ordinary
            || !changes.keys().any(|id| {
                matches!(
                    id.as_str(),
                    "stun_server" | "webrtc.auto_recover" | "webrtc.recovery_probe_interval_sec"
                )
            })
        {
            return Ok(());
        }
        let mut next = self.values.clone();
        next.extend(changes.clone());
        let settings = realtime_settings(&next)?;
        self.sender.send_replace(settings);
        self.values = next;
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        if class != PatchClass::Ordinary {
            return;
        }
        let mut restored = self.values.clone();
        restored.extend(previous.clone());
        if let Ok(settings) = realtime_settings(&restored) {
            self.sender.send_replace(settings);
            self.values = restored;
        } else {
            tracing::error!(
                diagnostic_id = "REALTIME_SETTINGS_ROLLBACK_FAILED",
                "realtime settings could not be restored"
            );
        }
    }
}

pub(crate) struct WebSocketSettingsApplier {
    sender: watch::Sender<WebSocketRuntimeSettings>,
    values: BTreeMap<String, Value>,
}

impl WebSocketSettingsApplier {
    /// Creates the live settings channel used by active WebSocket heartbeats.
    ///
    /// # Errors
    ///
    /// Returns a fixed diagnostic for invalid intervals or cross-setting violations.
    pub(crate) fn new(
        loaded: &LoadedSettings,
    ) -> Result<(Self, watch::Receiver<WebSocketRuntimeSettings>), String> {
        let values = raw_values(loaded);
        let initial = websocket_settings(&values)?;
        let (sender, receiver) = watch::channel(initial);
        Ok((Self { sender, values }, receiver))
    }
}

impl RuntimeSettingsApplier for WebSocketSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if class != PatchClass::Ordinary
            || !changes.keys().any(|id| {
                matches!(
                    id.as_str(),
                    "websocket.ping_interval_sec" | "websocket.pong_timeout_sec"
                )
            })
        {
            return Ok(());
        }
        let mut next = self.values.clone();
        next.extend(changes.clone());
        let settings = websocket_settings(&next)?;
        self.sender.send_replace(settings);
        self.values = next;
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        if class != PatchClass::Ordinary {
            return;
        }
        let mut restored = self.values.clone();
        restored.extend(previous.clone());
        self.values = restored;
        if let Ok(settings) = websocket_settings(&self.values) {
            self.sender.send_replace(settings);
        } else {
            tracing::error!(
                diagnostic_id = "WEBSOCKET_SETTINGS_ROLLBACK_FAILED",
                "websocket settings could not be restored"
            );
        }
    }
}

pub(crate) fn notification_config(
    values: &BTreeMap<String, Value>,
) -> Result<NotificationConfig, String> {
    let webhook = optional_text(values, "notifications.discord.webhook_url")?
        .map(|raw| DiscordWebhookUrl::parse(&raw))
        .transpose()
        .map_err(|_error| "notification settings are invalid".to_owned())?;
    let username = optional_text(values, "notifications.discord.username")?;
    let avatar_url = optional_text(values, "notifications.discord.avatar_url")?
        .map(|raw| Url::parse(&raw))
        .transpose()
        .map_err(|_error| "notification settings are invalid".to_owned())?;
    Ok(NotificationConfig {
        discord: DiscordNotificationConfig {
            webhook,
            username,
            avatar_url,
            on_script_start: boolean(values, "notifications.discord.on_script_start")?,
            on_script_end: boolean(values, "notifications.discord.on_script_end")?,
        },
        windows: WindowsNotificationConfig {
            on_script_start: boolean(values, "notifications.windows.on_script_start")?,
            on_script_end: boolean(values, "notifications.windows.on_script_end")?,
        },
    })
}

fn realtime_settings(values: &BTreeMap<String, Value>) -> Result<RealtimeRuntimeSettings, String> {
    let interval = positive_integer(values, "webrtc.recovery_probe_interval_sec")?;
    RealtimeRuntimeSettings::new(
        text(values, "stun_server")?,
        boolean(values, "webrtc.auto_recover")?,
        Duration::from_secs(interval),
    )
    .map_err(|_error| "realtime settings are invalid".to_owned())
}

#[allow(clippy::similar_names)]
fn websocket_settings(
    values: &BTreeMap<String, Value>,
) -> Result<WebSocketRuntimeSettings, String> {
    let ping = positive_integer(values, "websocket.ping_interval_sec")?;
    let pong = positive_integer(values, "websocket.pong_timeout_sec")?;
    WebSocketRuntimeSettings::new(Duration::from_secs(ping), Duration::from_secs(pong))
        .map_err(|_error| "websocket settings are invalid".to_owned())
}

fn raw_values(loaded: &LoadedSettings) -> BTreeMap<String, Value> {
    loaded
        .settings
        .values()
        .iter()
        .map(|(id, resolved)| (id.clone(), resolved.value.clone()))
        .collect()
}

fn text(values: &BTreeMap<String, Value>, id: &str) -> Result<String, String> {
    values
        .get(id)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "runtime setting has an invalid type".to_owned())
}

fn optional_text(values: &BTreeMap<String, Value>, id: &str) -> Result<Option<String>, String> {
    text(values, id).map(|value| (!value.is_empty()).then_some(value))
}

fn boolean(values: &BTreeMap<String, Value>, id: &str) -> Result<bool, String> {
    values
        .get(id)
        .and_then(Value::as_bool)
        .ok_or_else(|| "runtime setting has an invalid type".to_owned())
}

fn positive_integer(values: &BTreeMap<String, Value>, id: &str) -> Result<u64, String> {
    values
        .get(id)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "runtime setting has an invalid value".to_owned())
}

fn run_sync<F>(runtime: &Handle, future: F)
where
    F: Future<Output = ()>,
{
    if Handle::try_current().is_ok() && runtime.runtime_flavor() == RuntimeFlavor::MultiThread {
        tokio::task::block_in_place(|| runtime.block_on(future));
    } else {
        runtime.block_on(future);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
    use crate::settings::roots::{BaseDirectories, RootEnvironment};
    use tempfile::TempDir;

    use super::*;

    fn loaded(temporary: &TempDir) -> LoadedSettings {
        let base = temporary.path();
        let environment = RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]);
        let request = PipelineRequest {
            arguments: vec![OsString::from("pokecon")],
            base_directories: Some(BaseDirectories::linux(&environment).unwrap()),
            environment,
            startup_cwd: base.to_path_buf(),
            resource_root: base.to_path_buf(),
            dynamic_values: BTreeMap::new(),
        };
        SettingsPipeline::new(request).load().unwrap()
    }

    #[test]
    fn startup_notification_and_realtime_projections_are_valid() {
        let temporary = TempDir::new().unwrap();
        let loaded = loaded(&temporary);
        let values = raw_values(&loaded);
        let notification = notification_config(&values).unwrap();
        assert!(notification.discord.webhook.is_none());
        assert_eq!(
            realtime_settings(&values)
                .unwrap()
                .recovery_probe_interval(),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn desktop_close_policy_is_applied_and_rolled_back_immediately() {
        let settings = DesktopRuntimeSettings::new(CloseBehavior::Ask);
        let mut applier = DesktopSettingsApplier::new(settings.clone());
        applier
            .apply(
                PatchClass::Ordinary,
                &BTreeMap::from([(
                    "ui.desktop.close_behavior".to_owned(),
                    Value::String("keep_backend".to_owned()),
                )]),
            )
            .unwrap();
        assert_eq!(settings.close_behavior(), CloseBehavior::KeepBackend);

        applier.rollback(
            PatchClass::Ordinary,
            &BTreeMap::from([(
                "ui.desktop.close_behavior".to_owned(),
                Value::String("ask".to_owned()),
            )]),
        );
        assert_eq!(settings.close_behavior(), CloseBehavior::Ask);
    }

    #[test]
    fn websocket_settings_are_validated_and_runtime_channel_is_live() {
        let temporary = TempDir::new().unwrap();
        let loaded = loaded(&temporary);
        let values = raw_values(&loaded);
        let settings = websocket_settings(&values).unwrap();
        assert_eq!(settings.ping_interval(), Duration::from_secs(15));
        assert_eq!(settings.pong_timeout(), Duration::from_secs(10));

        let (mut applier, receiver) = WebSocketSettingsApplier::new(&loaded).unwrap();
        assert_eq!(receiver.borrow().ping_interval(), Duration::from_secs(15));

        // Valid single-field change reschedules via live channel.
        // Keep cross constraint valid: new ping 12 still >= pong 10.
        applier
            .apply(
                PatchClass::Ordinary,
                &BTreeMap::from([(
                    "websocket.ping_interval_sec".to_owned(),
                    Value::Number(serde_json::Number::from(12)),
                )]),
            )
            .unwrap();
        assert_eq!(receiver.borrow().ping_interval(), Duration::from_secs(12));
        assert_eq!(receiver.borrow().pong_timeout(), Duration::from_secs(10));

        applier
            .apply(
                PatchClass::Ordinary,
                &BTreeMap::from([(
                    "websocket.pong_timeout_sec".to_owned(),
                    Value::Number(serde_json::Number::from(4)),
                )]),
            )
            .unwrap();
        assert_eq!(receiver.borrow().pong_timeout(), Duration::from_secs(4));
    }

    #[test]
    fn websocket_settings_reject_invalid_updates_without_changing_current_value() {
        let temporary = TempDir::new().unwrap();
        let loaded = loaded(&temporary);
        let (mut applier, receiver) = WebSocketSettingsApplier::new(&loaded).unwrap();
        let initial = receiver.borrow().clone();

        // Zero duration must be rejected.
        let zero = applier.apply(
            PatchClass::Ordinary,
            &BTreeMap::from([(
                "websocket.ping_interval_sec".to_owned(),
                Value::Number(serde_json::Number::from(0)),
            )]),
        );
        assert!(zero.is_err());
        assert_eq!(*receiver.borrow(), initial);

        // Cross-setting violation (pong > ping) must be rejected atomically.
        let cross = applier.apply(
            PatchClass::Ordinary,
            &BTreeMap::from([(
                "websocket.pong_timeout_sec".to_owned(),
                Value::Number(serde_json::Number::from(20)),
            )]),
        );
        assert!(cross.is_err());
        assert_eq!(*receiver.borrow(), initial);

        // Paired update that violates pong <= ping must also be rejected.
        let paired = applier.apply(
            PatchClass::Ordinary,
            &BTreeMap::from([
                (
                    "websocket.ping_interval_sec".to_owned(),
                    Value::Number(serde_json::Number::from(3)),
                ),
                (
                    "websocket.pong_timeout_sec".to_owned(),
                    Value::Number(serde_json::Number::from(5)),
                ),
            ]),
        );
        assert!(paired.is_err());
        assert_eq!(*receiver.borrow(), initial);

        // Rollback restores previous snapshot.
        applier
            .apply(
                PatchClass::Ordinary,
                &BTreeMap::from([(
                    "websocket.ping_interval_sec".to_owned(),
                    Value::Number(serde_json::Number::from(12)),
                )]),
            )
            .unwrap();
        assert_eq!(receiver.borrow().ping_interval(), Duration::from_secs(12));
        applier.rollback(
            PatchClass::Ordinary,
            &BTreeMap::from([(
                "websocket.ping_interval_sec".to_owned(),
                Value::Number(serde_json::Number::from(15)),
            )]),
        );
        assert_eq!(receiver.borrow().ping_interval(), Duration::from_secs(15));
        assert_eq!(receiver.borrow().pong_timeout(), Duration::from_secs(10));
    }
}
