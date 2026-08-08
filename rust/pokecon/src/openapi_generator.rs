use std::fs;
use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
struct Arguments {
    /// Refuse to write and fail if the tracked document differs.
    #[arg(long)]
    check: bool,
    /// `OpenAPI` artifact path, relative to the current directory by default.
    #[arg(long, default_value = "api/openapi.json")]
    output: PathBuf,
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse();
    let generated = crate::server::openapi::document_json()?;
    if arguments.check {
        let existing = fs::read_to_string(&arguments.output)?;
        if existing != generated {
            return Err(format!(
                "{} differs from the generated OpenAPI document",
                arguments.output.display()
            )
            .into());
        }
    } else {
        if let Some(parent) = arguments.output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&arguments.output, generated)?;
    }
    Ok(())
}
