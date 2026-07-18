use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::num::TryFromIntError;

use clap::{Parser, ValueEnum};
use pokecon_app::{AppError, AppOptions, UiMode, run};
use pokecon_core::{TracingInitError, init_tracing};
use pokecon_settings::pipeline::{PipelineError, PipelineRequest, SettingsPipeline};
use pokecon_settings::scaffold::{ScaffoldError, ScaffoldManager};
use pokecon_settings::service::{NoopSettingsApplier, SettingsService};
use pokecon_settings::uv::{ManagedUv, ManagedUvSource, UvError};
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
    #[error(transparent)]
    Uv(#[from] UvError),
    #[error("packaged application is missing its pinned managed uv executable")]
    MissingManagedUv,
    #[error("canonical server.bind_address is not a numeric IP literal")]
    BindAddress(#[from] AddrParseError),
    #[error("canonical server.port is outside the u16 range")]
    Port(#[from] TryFromIntError),
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let loaded = SettingsPipeline::new(PipelineRequest::current()?).load()?;
    ScaffoldManager::new(loaded.roots.clone()).ensure(loaded.active_profile.as_str())?;
    let managed_uv_source = ManagedUvSource::bundled()?.ok_or(MainError::MissingManagedUv)?;
    let _managed_uv = ManagedUv::prepare(&loaded.roots, &managed_uv_source)?;
    let cli = Cli::parse_from(&loaded.remaining_arguments);
    let bind_address = loaded
        .settings
        .string("server.bind_address")?
        .parse::<IpAddr>()?;
    let port = u16::try_from(loaded.settings.integer("server.port")?)?;
    let _settings_service = SettingsService::new(loaded, Box::<NoopSettingsApplier>::default());
    init_tracing("info")?;
    run(AppOptions {
        listen_address: SocketAddr::new(bind_address, port),
        ui_mode: cli.ui.into(),
        exit_after_startup: cli.exit_after_startup,
    })
    .await?;
    Ok(())
}
