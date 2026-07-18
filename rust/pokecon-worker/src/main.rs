use clap::{Parser, ValueEnum};
use pokecon_core::{TracingInitError, init_tracing_to_stderr};
use pokecon_worker::{WorkerError, WorkerKind, run};
use thiserror::Error;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum WorkerArgument {
    Script,
    Dynamic,
}

impl From<WorkerArgument> for WorkerKind {
    fn from(value: WorkerArgument) -> Self {
        match value {
            WorkerArgument::Script => Self::Script,
            WorkerArgument::Dynamic => Self::Dynamic,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about = "PokeCon managed worker process")]
struct Cli {
    /// Selects the isolated worker role.
    #[arg(long, value_enum)]
    kind: WorkerArgument,
    /// Exit successfully after the worker runtime starts.
    #[arg(long)]
    exit_after_startup: bool,
}

#[derive(Debug, Error)]
enum MainError {
    #[error(transparent)]
    Tracing(#[from] TracingInitError),
    #[error(transparent)]
    Worker(#[from] WorkerError),
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let cli = Cli::parse();
    init_tracing_to_stderr("info")?;
    run(cli.kind.into(), cli.exit_after_startup).await?;
    Ok(())
}
