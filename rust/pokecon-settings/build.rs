use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use serde::Serialize;
use sha2::{Digest, Sha256};
use toml::Value;

#[derive(Debug, Serialize)]
struct EmbeddedRequirements {
    common: Vec<String>,
    worker_script: Vec<String>,
    worker_dynamic: Vec<String>,
    optional: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Serialize)]
struct EmbeddedUvSource {
    path: String,
    version: String,
    sha256: String,
}

fn digest_file(path: &PathBuf) -> String {
    let mut file = fs::File::open(path).expect("bundled uv must be readable at build time");
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .expect("bundled uv must remain readable at build time");
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    hex::encode(digest.finalize())
}

fn string_array(root: &Value, path: &[&str]) -> Vec<String> {
    let mut value = root;
    for segment in path {
        value = value
            .get(*segment)
            .unwrap_or_else(|| panic!("missing pyproject key {}", path.join(".")));
    }
    value
        .as_array()
        .unwrap_or_else(|| panic!("{} must be an array", path.join(".")))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("{} entries must be strings", path.join(".")))
                .to_owned()
        })
        .collect()
}

fn main() {
    println!("cargo:rerun-if-env-changed=POKECON_BUILD_UV_PATH");
    println!("cargo:rerun-if-env-changed=POKECON_BUILD_UV_VERSION");
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be available"),
    );
    let pyproject_path = manifest_dir.join("../../pyproject.toml");
    println!("cargo:rerun-if-changed={}", pyproject_path.display());

    let source = fs::read_to_string(&pyproject_path).expect("pyproject.toml must be readable");
    let pyproject = source
        .parse::<Value>()
        .expect("pyproject.toml must be valid TOML");
    let optional_table = pyproject
        .get("project")
        .and_then(|project| project.get("optional-dependencies"))
        .and_then(Value::as_table)
        .expect("project.optional-dependencies must be a table");
    let optional = optional_table
        .iter()
        .map(|(name, value)| {
            let requirements = value
                .as_array()
                .unwrap_or_else(|| panic!("optional dependency group {name} must be an array"))
                .iter()
                .map(|item| {
                    item.as_str()
                        .unwrap_or_else(|| {
                            panic!("optional dependency group {name} entries must be strings")
                        })
                        .to_owned()
                })
                .collect();
            (name.clone(), requirements)
        })
        .collect();
    let requirements = EmbeddedRequirements {
        common: string_array(&pyproject, &["project", "dependencies"]),
        worker_script: string_array(&pyproject, &["dependency-groups", "worker-script"]),
        worker_dynamic: string_array(&pyproject, &["dependency-groups", "worker-dynamic"]),
        optional,
    };
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be available"))
        .join("application-requirements.json");
    fs::write(
        output,
        serde_json::to_vec_pretty(&requirements)
            .expect("embedded requirements must serialize as JSON"),
    )
    .expect("embedded requirements must be writable");

    let bundled_uv = match (
        env::var_os("POKECON_BUILD_UV_PATH"),
        env::var("POKECON_BUILD_UV_VERSION").ok(),
    ) {
        (None, None) => None,
        (Some(path), Some(version)) => {
            let path = PathBuf::from(path);
            assert!(
                path.is_file(),
                "POKECON_BUILD_UV_PATH must be a regular file"
            );
            Some(EmbeddedUvSource {
                path: path.to_string_lossy().into_owned(),
                version,
                sha256: digest_file(&path),
            })
        }
        _ => panic!("POKECON_BUILD_UV_PATH and POKECON_BUILD_UV_VERSION must be set together"),
    };
    let uv_output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be available"))
        .join("managed-uv-source.json");
    fs::write(
        uv_output,
        serde_json::to_vec_pretty(&bundled_uv)
            .expect("embedded uv metadata must serialize as JSON"),
    )
    .expect("embedded uv metadata must be writable");
}
