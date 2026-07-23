use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pokecon_contracts::{
    commands_python_typings, dynamic_lua_typings, dynamic_python_typings, settings_json_schema,
    settings_registry, settings_ui_metadata,
};

fn main() -> Result<(), Box<dyn Error>> {
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
    if !root.join("Cargo.toml").is_file() || !root.join("SPECIFICATION.md").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "generate_contracts must run from the repository root",
        )
        .into());
    }
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
    for (relative, source) in outputs {
        let path = root.join(&relative);
        if check {
            check_output(&path, &source)?;
            println!("up to date: {}", relative.display());
        } else {
            write_output(&path, &source)?;
            println!("generated: {}", relative.display());
        }
    }
    Ok(())
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
