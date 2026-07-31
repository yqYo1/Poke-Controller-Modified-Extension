use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::num::TryFromIntError;
use std::path::PathBuf;

#[cfg(feature = "tauri-shell")]
use std::sync::mpsc;

use crate::diagnostics::{TracingInitError, init_tracing};
use crate::dynamic_runtime::bootstrap_dynamic;
use crate::runtime::ShutdownCoordinator;
#[cfg(feature = "tauri-shell")]
use crate::runtime::ShutdownReason;
use crate::settings::pipeline::{LoadedSettings, PipelineError, PipelineRequest, SettingsPipeline};
use crate::settings::scaffold::{ScaffoldError, ScaffoldManager};
use crate::{AppError, AppOptions, RunControl, UiMode, run_configured_controlled};
use clap::{Parser, ValueEnum};
use pokecon_desktop::{CloseBehavior, DesktopError, DesktopRuntimeSettings};
#[cfg(feature = "tauri-shell")]
use pokecon_desktop::{DesktopLifecycle, DesktopShellConfig, run_tauri_shell};
use thiserror::Error;

#[cfg(all(feature = "tauri-shell", target_os = "linux"))]
const COMPOSITING_REEXEC_MARKER: &str = "PCME_DESKTOP_COMPOSITING_CONFIGURED";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum UiArgument {
    Web,
    Desktop,
}

impl From<UiArgument> for UiMode {
    fn from(value: UiArgument) -> Self {
        match value {
            UiArgument::Web => Self::Web,
            UiArgument::Desktop => Self::Desktop,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about = "PokeCon Rust runtime")]
struct Cli {
    /// Selects the web-only or desktop lifecycle mode.
    #[arg(long, value_enum, default_value_t = UiArgument::Web)]
    ui: UiArgument,
    /// Exit successfully after the runtime boundaries have started.
    #[arg(long)]
    exit_after_startup: bool,
}

#[derive(Debug, Error)]
pub enum MainError {
    #[error(transparent)]
    Tracing(#[from] TracingInitError),
    #[error(transparent)]
    App(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Desktop(#[from] DesktopError),
    #[error(transparent)]
    Settings(#[from] PipelineError),
    #[error(transparent)]
    Scaffold(#[from] ScaffoldError),
    #[error("canonical server.bind_address is not a numeric IP literal")]
    BindAddress(#[from] AddrParseError),
    #[error("canonical server.port is outside the u16 range")]
    Port(#[from] TryFromIntError),
    #[cfg(feature = "tauri-shell")]
    #[error("desktop backend task failed: {0}")]
    BackendTask(#[source] tokio::task::JoinError),
    #[cfg(feature = "tauri-shell")]
    #[error("the primary desktop instance did not start its backend")]
    BackendNotStarted,
    #[cfg(not(feature = "tauri-shell"))]
    #[error("desktop mode is unavailable in this binary; rebuild with feature `tauri-shell`")]
    DesktopUnavailable,
    #[cfg(all(feature = "tauri-shell", target_os = "linux"))]
    #[error("failed to relaunch with Linux WebView compositing disabled: {0}")]
    CompositingRelaunch(#[source] std::io::Error),
    #[cfg(all(feature = "tauri-shell", target_os = "linux"))]
    #[error("the compositing-configured child process exited unsuccessfully: {0}")]
    CompositingChild(std::process::ExitStatus),
}

impl From<AppError> for MainError {
    fn from(error: AppError) -> Self {
        Self::App(Box::new(error))
    }
}

/// Runs the canonical command-line and desktop application lifecycle.
///
/// # Errors
///
/// Returns an error if settings, tracing, scaffolding, desktop startup, address
/// parsing, or application runtime execution fails.
pub async fn run_cli() -> Result<(), MainError> {
    let request = PipelineRequest::current()?;
    #[cfg(feature = "tauri-shell")]
    let request = {
        let mut request = request;
        if let Some(resource_root) = packaged_resource_root() {
            request.resource_root = resource_root;
        }
        request
    };
    let before_dynamic = SettingsPipeline::new(request.clone()).load_before_dynamic()?;
    let cli = Cli::parse_from(&before_dynamic.remaining_arguments);

    #[cfg(all(feature = "tauri-shell", target_os = "linux"))]
    if relaunch_for_linux_compositing(&cli, &before_dynamic)? {
        return Ok(());
    }

    init_tracing("info")?;
    ScaffoldManager::new(before_dynamic.roots.clone())
        .ensure(before_dynamic.active_profile.as_str())?;

    if cli.ui == UiArgument::Desktop && !cli.exit_after_startup {
        #[cfg(feature = "tauri-shell")]
        return run_desktop(request, before_dynamic).await;
        #[cfg(not(feature = "tauri-shell"))]
        return Err(MainError::DesktopUnavailable);
    }

    run_backend(
        request,
        before_dynamic,
        cli.ui.into(),
        cli.exit_after_startup,
        RunControl::new(ShutdownCoordinator::new()),
        None,
    )
    .await
}

#[cfg(feature = "tauri-shell")]
fn packaged_resource_root() -> Option<PathBuf> {
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let resource_root =
        tauri::utils::platform::resource_dir(context.package_info(), &tauri::utils::Env::default())
            .ok()?;
    resource_root
        .join("resource-manifest.json")
        .is_file()
        .then_some(resource_root)
}

async fn run_backend(
    request: PipelineRequest,
    before_dynamic: LoadedSettings,
    ui_mode: UiMode,
    exit_after_startup: bool,
    control: RunControl,
    desktop_settings: Option<DesktopRuntimeSettings>,
) -> Result<(), MainError> {
    let bootstrap = bootstrap_dynamic(request.clone(), before_dynamic).await?;
    if let Some(error) = bootstrap.startup_failure.as_ref() {
        tracing::error!(
            error = %error,
            "dynamic configuration is unavailable; continuing with static settings"
        );
    }
    let loaded = bootstrap.loaded;
    if let Some(settings) = desktop_settings {
        settings.set_close_behavior(
            loaded
                .settings
                .string("ui.desktop.close_behavior")?
                .parse::<CloseBehavior>()?,
        );
    }
    let bind_address = loaded
        .settings
        .string("server.bind_address")?
        .parse::<IpAddr>()?;
    let port = u16::try_from(loaded.settings.integer("server.port")?)?;
    let web_root = PathBuf::from(loaded.settings.string("server.web_dir")?);
    run_configured_controlled(
        AppOptions {
            listen_address: SocketAddr::new(bind_address, port),
            ui_mode,
            web_root,
            exit_after_startup,
        },
        request,
        loaded,
        bootstrap.host,
        bootstrap.runtime,
        control,
    )
    .await?;
    Ok(())
}

#[cfg(feature = "tauri-shell")]
async fn run_desktop(
    request: PipelineRequest,
    before_dynamic: LoadedSettings,
) -> Result<(), MainError> {
    let bind_address = before_dynamic
        .settings
        .string("server.bind_address")?
        .parse::<IpAddr>()?;
    let port = u16::try_from(before_dynamic.settings.integer("server.port")?)?;
    let configured_address = SocketAddr::new(bind_address, port);
    let runtime_settings = DesktopRuntimeSettings::new(
        before_dynamic
            .settings
            .string("ui.desktop.close_behavior")?
            .parse::<CloseBehavior>()?,
    );
    let disable_compositing = before_dynamic
        .settings
        .boolean("ui.desktop.disable_compositing")?;
    let shell_config = DesktopShellConfig {
        app_url: format!("http://{configured_address}"),
        config_directory: before_dynamic.roots.config.clone(),
        runtime_settings: runtime_settings.clone(),
        disable_compositing,
    };
    let shutdown = ShutdownCoordinator::new();
    let lifecycle = DesktopLifecycle::new(shutdown.clone());
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (task_sender, task_receiver) =
        mpsc::sync_channel::<tokio::task::JoinHandle<Result<(), MainError>>>(1);
    let runtime = tokio::runtime::Handle::current();
    let task_shutdown = shutdown.clone();
    let control = RunControl::new(shutdown.clone())
        .with_ready_sender(ready_sender)
        .with_desktop_settings(runtime_settings.clone());

    let shell_result = tokio::task::block_in_place(|| {
        run_tauri_shell(
            tauri::generate_context!(),
            shell_config,
            &lifecycle,
            move |resource_root| {
                let mut request = request;
                request.resource_root = resource_root;
                let before_dynamic = SettingsPipeline::new(request.clone())
                    .load_before_dynamic()
                    .map_err(|error| DesktopError::BackendStartup(error.to_string()))?;
                let failure_shutdown = task_shutdown.clone();
                let task = runtime.spawn(async move {
                    let result = run_backend(
                        request,
                        before_dynamic,
                        UiMode::Desktop,
                        false,
                        control,
                        Some(runtime_settings),
                    )
                    .await;
                    if let Err(error) = result.as_ref() {
                        failure_shutdown.request(ShutdownReason::FatalError(error.to_string()));
                    }
                    result
                });
                task_sender.send(task).map_err(|_error| {
                    DesktopError::BackendStartup("backend task receiver was dropped".to_owned())
                })?;
                let actual_address = ready_receiver.recv().map_err(|_error| {
                    DesktopError::BackendStartup(
                        "backend stopped before publishing its listener".to_owned(),
                    )
                })?;
                if actual_address != configured_address {
                    return Err(DesktopError::BackendStartup(
                        "backend listener did not match canonical startup settings".to_owned(),
                    ));
                }
                Ok(())
            },
        )
    });

    if let Err(error) = shell_result.as_ref() {
        shutdown.request(ShutdownReason::FatalError(error.to_string()));
    }
    let backend_task = task_receiver.try_recv().ok();
    if let Some(task) = backend_task {
        task.await.map_err(MainError::BackendTask)??;
    } else if shell_result.is_ok() {
        return Err(MainError::BackendNotStarted);
    }
    shell_result?;
    Ok(())
}

#[cfg(all(feature = "tauri-shell", target_os = "linux"))]
fn relaunch_for_linux_compositing(
    cli: &Cli,
    before_dynamic: &LoadedSettings,
) -> Result<bool, MainError> {
    if cli.ui != UiArgument::Desktop
        || cli.exit_after_startup
        || !before_dynamic
            .settings
            .boolean("ui.desktop.disable_compositing")?
        || std::env::var_os(COMPOSITING_REEXEC_MARKER).is_some()
    {
        return Ok(false);
    }
    let executable = std::env::current_exe().map_err(MainError::CompositingRelaunch)?;
    let status = std::process::Command::new(executable)
        .args(std::env::args_os().skip(1))
        .env(COMPOSITING_REEXEC_MARKER, "1")
        .env("WEBKIT_DISABLE_COMPOSITING_MODE", "1")
        .status()
        .map_err(MainError::CompositingRelaunch)?;
    if status.success() {
        Ok(true)
    } else {
        Err(MainError::CompositingChild(status))
    }
}
