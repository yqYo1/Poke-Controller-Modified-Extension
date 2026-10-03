use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
    let arguments = Arguments::try_parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    if arguments.check {
        check_openapi_artifact(&arguments.output)
    } else {
        write_openapi_artifact(&arguments.output)
    }
}

/// Checks an `OpenAPI` artifact against the server schema generator.
///
/// # Errors
///
/// Returns an error when the document cannot be generated or the artifact is missing or stale.
pub(crate) fn check_openapi_artifact(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let generated = crate::server::openapi::document_json()?;
    let existing = fs::read_to_string(output).map_err(|source| {
        io::Error::new(
            source.kind(),
            format!(
                "failed to read OpenAPI artifact {}: {source}",
                output.display()
            ),
        )
    })?;
    if existing != generated {
        return Err(format!(
            "{} differs from the generated OpenAPI document",
            output.display()
        )
        .into());
    }
    Ok(())
}

fn write_openapi_artifact(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let generated = crate::server::openapi::document_json()?;
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| {
            io::Error::new(
                source.kind(),
                format!(
                    "failed to create OpenAPI artifact directory {}: {source}",
                    parent.display()
                ),
            )
        })?;
    }
    fs::write(output, generated).map_err(|source| {
        io::Error::new(
            source.kind(),
            format!(
                "failed to write OpenAPI artifact {}: {source}",
                output.display()
            ),
        )
    })?;
    Ok(())
}
