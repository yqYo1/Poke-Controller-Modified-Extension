use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use clap::{Parser, ValueEnum};
use pokecon_app::{AppError, AppOptions, UiMode, run};
use pokecon_core::{TracingInitError, init_tracing};
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
    /// Listener IP. Wildcard-address validation is added with settings in phase three.
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    bind_address: IpAddr,
    /// Listener port. Use zero to request an ephemeral port.
    #[arg(long, default_value_t = 8020)]
    port: u16,
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
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let cli = Cli::parse();
    init_tracing("info")?;
    run(AppOptions {
        listen_address: SocketAddr::new(cli.bind_address, cli.port),
        ui_mode: cli.ui.into(),
        exit_after_startup: cli.exit_after_startup,
    })
    .await?;
    Ok(())
}
