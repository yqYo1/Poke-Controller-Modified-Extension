//! Exact, non-reentrant profile switching transaction.

use std::sync::Arc;

use crate::dynamic::protocol::DynamicProfileSwitchResult;
use thiserror::Error;

use crate::command_service::{CommandBackendError, CommandService, DynamicCommandBridge};
use crate::dynamic_host::StartupDynamicHost;

/// Successful or explicitly cancelled transaction result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileSwitchResult {
    Switched { forced_worker_stop: bool },
    Cancelled,
}

/// Failure that leaves either the complete old profile snapshot or the
/// complete new snapshot visible.
#[derive(Debug, Error)]
pub enum ProfileSwitchError {
    #[error(transparent)]
    Dynamic(#[from] CommandBackendError),
}

/// Coordinates settings, dynamic events, and the profile-scoped worker in the
/// normative twelve-step order.
pub struct ProfileService {
    host: Arc<StartupDynamicHost>,
    commands: Arc<CommandService>,
    dynamic: Arc<dyn DynamicCommandBridge>,
}

impl std::fmt::Debug for ProfileService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProfileService")
            .field("host", &self.host)
            .field("commands", &self.commands)
            .finish_non_exhaustive()
    }
}

impl ProfileService {
    #[must_use]
    pub fn new(
        host: Arc<StartupDynamicHost>,
        commands: Arc<CommandService>,
        dynamic: Arc<dyn DynamicCommandBridge>,
    ) -> Self {
        host.bind_command_service(&commands);
        Self {
            host,
            commands,
            dynamic,
        }
    }

    /// Switches one exact filesystem profile without ever spawning the target
    /// user worker.
    ///
    /// # Errors
    ///
    /// Rejects reentry, invalid target settings, event transport failure, or a
    /// worker that cannot be reaped. Every error releases the internal gate.
    pub async fn switch(&self, name: &str) -> Result<ProfileSwitchResult, ProfileSwitchError> {
        match self.dynamic.switch_profile(name).await? {
            DynamicProfileSwitchResult::Switched { forced_worker_stop } => {
                Ok(ProfileSwitchResult::Switched { forced_worker_stop })
            }
            DynamicProfileSwitchResult::Cancelled => Ok(ProfileSwitchResult::Cancelled),
            DynamicProfileSwitchResult::Rejected { code, message } => {
                Err(CommandBackendError::new(code, message).into())
            }
        }
    }
}

impl From<crate::settings::pipeline::PipelineError> for ProfileSwitchError {
    fn from(error: crate::settings::pipeline::PipelineError) -> Self {
        Self::Dynamic(CommandBackendError::new(
            "InvalidSetting",
            error.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use crate::dynamic::{CommandCacheBuildResult, CommandInfo, DynamicHost};
    use crate::settings::pipeline::{LoadedSettings, PipelineRequest, SettingsPipeline};
    use crate::settings::roots::{BaseDirectories, RootEnvironment};
    use crate::worker::script::protocol::{
        ScriptDiscoveryResult, ScriptExecuteRequest, ScriptExecutionResult, ScriptPauseResult,
        ScriptStopResult,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::command_service::{
        CommandServiceError, ScriptSessionStop, StaticCommandBridge, UserScriptFactory,
        UserScriptSession, builtin_display_cache,
    };

    #[derive(Default)]
    struct ProfileSession {
        stopping: AtomicBool,
        shutdowns: AtomicUsize,
        fail_shutdown: AtomicBool,
        force_shutdown: AtomicBool,
    }

    #[async_trait]
    impl UserScriptSession for ProfileSession {
        async fn discover(&self) -> Result<ScriptDiscoveryResult, CommandBackendError> {
            Ok(ScriptDiscoveryResult {
                commands: Vec::new(),
            })
        }

        async fn execute(
            &self,
            _request: ScriptExecuteRequest,
        ) -> Result<ScriptExecutionResult, CommandBackendError> {
            Err(CommandBackendError::new("UnexpectedExecute", "not used"))
        }

        async fn pause(&self) -> Result<ScriptPauseResult, CommandBackendError> {
            Ok(ScriptPauseResult { changed: false })
        }

        async fn resume(&self) -> Result<ScriptPauseResult, CommandBackendError> {
            Ok(ScriptPauseResult { changed: false })
        }

        async fn stop_command(&self) -> Result<ScriptStopResult, CommandBackendError> {
            Ok(ScriptStopResult {
                stop_requested: false,
            })
        }

        fn begin_stopping(&self) {
            self.stopping.store(true, Ordering::Release);
        }

        async fn shutdown(
            &self,
            _deadline: Duration,
        ) -> Result<ScriptSessionStop, CommandBackendError> {
            self.shutdowns.fetch_add(1, Ordering::AcqRel);
            if self.fail_shutdown.load(Ordering::Acquire) {
                Err(CommandBackendError::new(
                    "WorkerReapFailed",
                    "worker could not be reaped",
                ))
            } else {
                Ok(ScriptSessionStop {
                    forced: self.force_shutdown.load(Ordering::Acquire),
                })
            }
        }
    }

    struct ProfileFactory {
        session: Arc<ProfileSession>,
        profiles: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl UserScriptFactory for ProfileFactory {
        async fn spawn(
            &self,
            settings: LoadedSettings,
        ) -> Result<Arc<dyn UserScriptSession>, CommandBackendError> {
            self.profiles
                .lock()
                .unwrap()
                .push(settings.active_profile.as_str().to_owned());
            Ok(self.session.clone())
        }
    }

    struct ProfileBridge {
        events: Mutex<Vec<&'static str>>,
        cancel_pre: AtomicBool,
        commands: Mutex<Option<Arc<CommandService>>>,
        reentry_rejected: AtomicBool,
        host: Arc<StartupDynamicHost>,
    }

    #[async_trait]
    impl DynamicCommandBridge for ProfileBridge {
        async fn emit(&self, event: &'static str) -> Result<bool, CommandBackendError> {
            self.events.lock().unwrap().push(event);
            if event == "ProfileSwitchPre"
                && let Some(commands) = self.commands.lock().unwrap().as_ref()
            {
                self.reentry_rejected.store(
                    matches!(
                        commands.try_begin_profile_switch(),
                        Err(CommandServiceError::ProfileSwitchInProgress)
                    ),
                    Ordering::Release,
                );
            }
            Ok(event == "ProfileSwitchPre" && self.cancel_pre.load(Ordering::Acquire))
        }

        async fn switch_profile(
            &self,
            name: &str,
        ) -> Result<DynamicProfileSwitchResult, CommandBackendError> {
            if let Err(error) = self.host.profile_switch_begin(name, &BTreeMap::new()).await {
                return Ok(DynamicProfileSwitchResult::Rejected {
                    code: error.code,
                    message: error.message,
                });
            }
            if self.emit("ProfileSwitchPre").await? {
                self.host
                    .profile_switch_abort()
                    .await
                    .map_err(|error| CommandBackendError::new(error.code, error.message))?;
                return Ok(DynamicProfileSwitchResult::Cancelled);
            }
            let committed = match self.host.profile_switch_commit().await {
                Ok(committed) => committed,
                Err(error) => {
                    let _abort = self.host.profile_switch_abort().await;
                    return Ok(DynamicProfileSwitchResult::Rejected {
                        code: error.code,
                        message: error.message,
                    });
                }
            };
            if let Err(error) = self.emit("ProfileSwitchPost").await {
                tracing::error!(error = %error, "test profile Post event failed");
            }
            self.host
                .profile_switch_end()
                .await
                .map_err(|error| CommandBackendError::new(error.code, error.message))?;
            Ok(DynamicProfileSwitchResult::Switched {
                forced_worker_stop: committed.forced_worker_stop,
            })
        }

        async fn build_cache(
            &self,
            generation: u64,
            candidates: Vec<CommandInfo>,
        ) -> Result<CommandCacheBuildResult, CommandBackendError> {
            let mode = self
                .host
                .loaded_settings()
                .settings
                .string("commands.tag_match_mode")
                .unwrap()
                .to_owned();
            Ok(CommandCacheBuildResult::Complete {
                cache: builtin_display_cache(generation, candidates, &mode)?,
            })
        }
    }

    struct Fixture {
        _temporary: TempDir,
        host: Arc<StartupDynamicHost>,
        session: Arc<ProfileSession>,
        commands: Arc<CommandService>,
        bridge: Arc<ProfileBridge>,
        profiles: ProfileService,
    }

    fn fixture() -> Fixture {
        let temporary = TempDir::new().unwrap();
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .unwrap();
        let config = base.join("config/pokecon/profiles");
        std::fs::create_dir_all(config.join("default")).unwrap();
        std::fs::create_dir_all(config.join("Other")).unwrap();
        std::fs::write(
            config.join("Other/settings.toml"),
            "[ui]\nui_fps_options = [5, 15, 60]\nui_fps = 60\n",
        )
        .unwrap();
        let request = PipelineRequest {
            arguments: ["pokecon"].into_iter().map(OsString::from).collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let loaded = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .unwrap();
        let host = Arc::new(StartupDynamicHost::new(request, loaded).unwrap());
        host.finish_startup().unwrap();
        let session = Arc::new(ProfileSession::default());
        let factory = Arc::new(ProfileFactory {
            session: session.clone(),
            profiles: Mutex::new(Vec::new()),
        });
        let bridge = Arc::new(ProfileBridge {
            events: Mutex::new(Vec::new()),
            cancel_pre: AtomicBool::new(false),
            commands: Mutex::new(None),
            reentry_rejected: AtomicBool::new(false),
            host: host.clone(),
        });
        let commands = Arc::new(CommandService::new(host.clone(), factory, bridge.clone()));
        *bridge.commands.lock().unwrap() = Some(commands.clone());
        let profiles = ProfileService::new(host.clone(), commands.clone(), bridge.clone());
        Fixture {
            _temporary: temporary,
            host,
            session,
            commands,
            bridge,
            profiles,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_and_validation_have_no_profile_or_worker_side_effects() {
        let fixture = fixture();
        assert!(fixture.profiles.switch("Missing").await.is_err());
        assert_eq!(fixture.host.profile_current().unwrap(), "default");
        assert_eq!(
            fixture.host.state_snapshot().unwrap()["pending_profile"],
            json!(null)
        );
        assert!(fixture.bridge.events.lock().unwrap().is_empty());

        fixture.bridge.cancel_pre.store(true, Ordering::Release);
        assert_eq!(
            fixture.profiles.switch("Other").await.unwrap(),
            ProfileSwitchResult::Cancelled
        );
        assert_eq!(fixture.host.profile_current().unwrap(), "default");
        assert_eq!(
            fixture.host.state_snapshot().unwrap()["pending_profile"],
            json!(null)
        );
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 0);
        assert!(fixture.bridge.reentry_rejected.load(Ordering::Acquire));
        fixture.commands.try_begin_profile_switch().unwrap();
        fixture.commands.finish_profile_switch();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn worker_is_reaped_before_atomic_commit_and_target_stays_lazy() {
        let fixture = fixture();
        fixture.commands.reload().await.unwrap();
        assert!(!fixture.session.stopping.load(Ordering::Acquire));
        let result = fixture.profiles.switch("Other").await.unwrap();
        assert_eq!(
            result,
            ProfileSwitchResult::Switched {
                forced_worker_stop: false,
            }
        );
        assert!(fixture.session.stopping.load(Ordering::Acquire));
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 1);
        assert_eq!(fixture.host.profile_current().unwrap(), "Other");
        let state = fixture.host.state_snapshot().unwrap();
        assert_eq!(state["active_profile"], json!("Other"));
        assert_eq!(state["pending_profile"], json!(null));
        assert_eq!(state["command_candidates"], json!([]));
        assert!(fixture.commands.commands().await.is_empty());
        assert_eq!(
            fixture.bridge.events.lock().unwrap().as_slice(),
            [
                "ScriptLoadPre",
                "ScriptLoadPost",
                "ProfileSwitchPre",
                "ProfileSwitchPost"
            ]
        );
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reap_failure_rolls_back_settings_and_releases_the_gate() {
        let fixture = fixture();
        fixture.commands.reload().await.unwrap();
        fixture.session.fail_shutdown.store(true, Ordering::Release);
        assert!(fixture.profiles.switch("Other").await.is_err());
        assert_eq!(fixture.host.profile_current().unwrap(), "default");
        assert_eq!(
            fixture.host.state_snapshot().unwrap()["pending_profile"],
            json!(null)
        );
        fixture.commands.try_begin_profile_switch().unwrap();
        fixture.commands.finish_profile_switch();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn forced_old_worker_reap_is_reported_but_commits_the_target_profile() {
        let fixture = fixture();
        fixture.commands.reload().await.unwrap();
        fixture
            .session
            .force_shutdown
            .store(true, Ordering::Release);
        assert_eq!(
            fixture.profiles.switch("Other").await.unwrap(),
            ProfileSwitchResult::Switched {
                forced_worker_stop: true,
            }
        );
        assert_eq!(fixture.host.profile_current().unwrap(), "Other");
        assert_eq!(
            fixture.host.state_snapshot().unwrap()["pending_profile"],
            json!(null)
        );
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 1);
    }

    #[test]
    fn static_bridge_remains_constructible_without_a_dynamic_worker() {
        let fixture = fixture();
        let _bridge = StaticCommandBridge::new(fixture.host);
    }
}
