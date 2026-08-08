use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::contracts::{
    commands_python_typings, dynamic_lua_typings, dynamic_python_typings, settings_json_schema,
    settings_registry, settings_ui_metadata,
};

pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    let check = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [argument] if argument == "--check" => true,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: generate_contracts [--check]",
            )
            .into());
        }
    };
    let root = std::env::current_dir()?;
    if check {
        check_generated_artifacts(&root)
    } else {
        write_generated_artifacts(&root)
    }
}

/// Checks every tracked settings and scripting artifact under a repository root.
///
/// # Errors
///
/// Returns an error when the root is invalid, canonical contracts cannot be loaded, or a
/// generated output is missing or stale.
pub(crate) fn check_generated_artifacts(root: &Path) -> Result<(), Box<dyn Error>> {
    validate_repository_root(root)?;
    for (relative, source) in generated_outputs()? {
        check_output(&root.join(&relative), &source)?;
        println!("up to date: {}", relative.display());
    }
    Ok(())
}

fn write_generated_artifacts(root: &Path) -> Result<(), Box<dyn Error>> {
    validate_repository_root(root)?;
    for (relative, source) in generated_outputs()? {
        write_output(&root.join(&relative), &source)?;
        println!("generated: {}", relative.display());
    }
    Ok(())
}

fn validate_repository_root(root: &Path) -> Result<(), io::Error> {
    if root.join("Cargo.toml").is_file() && root.join("SPECIFICATION.md").is_file() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "generate_contracts must run from the repository root",
        ))
    }
}

fn generated_outputs() -> Result<Vec<(PathBuf, String)>, Box<dyn Error>> {
    let settings = settings_registry()?;
    let mut outputs = vec![
        (
            PathBuf::from("generated/settings.schema.json"),
            pretty_json(&settings_json_schema(settings.registry()))?,
        ),
        (
            PathBuf::from("generated/settings-ui.json"),
            pretty_json(&settings_ui_metadata(settings.registry()))?,
        ),
        (
            PathBuf::from("python/pokecon/typings/__init__.pyi"),
            dynamic_python_typings()?,
        ),
        (
            PathBuf::from("generated/lua/pokecon.d.lua"),
            dynamic_lua_typings()?,
        ),
    ];
    outputs.extend(commands_python_typings()?.into_iter().map(|typing| {
        (
            PathBuf::from("python/pokecon/typings").join(typing.relative_path()),
            typing.source().to_owned(),
        )
    }));
    Ok(outputs)
}

fn pretty_json(value: &serde_json::Value) -> Result<String, serde_json::Error> {
    let mut output = serde_json::to_string_pretty(value)?;
    output.push('\n');
    Ok(output)
}

fn check_output(path: &Path, expected: &str) -> Result<(), io::Error> {
    let actual = fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("generated output is missing at {}: {error}", path.display()),
        )
    })?;
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "generated output drift at {}; run nix run .#generate-contracts",
            path.display()
        )))
    }
}

fn write_output(path: &Path, source: &str) -> Result<(), io::Error> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("generated output has no parent: {}", path.display()),
        )
    })?;
    fs::create_dir_all(parent)?;
    fs::write(path, source)
}
