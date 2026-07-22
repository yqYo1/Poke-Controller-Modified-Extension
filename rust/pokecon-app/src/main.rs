use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::num::TryFromIntError;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use pokecon_app::dynamic_runtime::bootstrap_dynamic;
use pokecon_app::{AppError, AppOptions, UiMode, run_configured};
use pokecon_core::{TracingInitError, init_tracing};
use pokecon_settings::pipeline::{PipelineError, PipelineRequest, SettingsPipeline};
use pokecon_settings::scaffold::{ScaffoldError, ScaffoldManager};
use thiserror::Error;

#[derive(Clone, Copy, Debug, ValueEnum)]
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
enum MainError {
    #[error(transparent)]
    Tracing(#[from] TracingInitError),
    #[error(transparent)]
    App(#[from] AppError),
    #[error(transparent)]
    Settings(#[from] PipelineError),
    #[error(transparent)]
    Scaffold(#[from] ScaffoldError),
    #[error("canonical server.bind_address is not a numeric IP literal")]
    BindAddress(#[from] AddrParseError),
    #[error("canonical server.port is outside the u16 range")]
    Port(#[from] TryFromIntError),
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let request = PipelineRequest::current()?;
    let before_dynamic = SettingsPipeline::new(request.clone()).load_before_dynamic()?;
    let cli = Cli::parse_from(&before_dynamic.remaining_arguments);
    init_tracing("info")?;
    ScaffoldManager::new(before_dynamic.roots.clone())
        .ensure(before_dynamic.active_profile.as_str())?;
    let bootstrap = bootstrap_dynamic(request.clone(), before_dynamic).await?;
    if let Some(error) = bootstrap.startup_failure.as_ref() {
        tracing::error!(
            error = %error,
            "dynamic configuration is unavailable; continuing with static settings"
        );
    }
    let loaded = bootstrap.loaded;
    let bind_address = loaded
        .settings
        .string("server.bind_address")?
        .parse::<IpAddr>()?;
    let port = u16::try_from(loaded.settings.integer("server.port")?)?;
    let web_root = PathBuf::from(loaded.settings.string("server.web_dir")?);
    run_configured(
        AppOptions {
            listen_address: SocketAddr::new(bind_address, port),
            ui_mode: cli.ui.into(),
            web_root,
            exit_after_startup: cli.exit_after_startup,
        },
        request,
        loaded,
        bootstrap.host,
        bootstrap.runtime,
    )
    .await?;
    Ok(())
}
