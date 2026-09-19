use std::{
    collections::BTreeMap,
    env, fmt, fs, io,
    io::Read as _,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use toml::Value;

const RESOURCE_PROVENANCE_ENVIRONMENT: &str = "POKECON_RESOURCE_PROVENANCE";

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

fn digest_build_input(path: &Path) -> String {
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

fn settings_string_array(root: &Value, path: &[&str]) -> Vec<String> {
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

fn generate_settings_resources() {
    println!("cargo:rerun-if-env-changed=POKECON_BUILD_UV_PATH");
    println!("cargo:rerun-if-env-changed=POKECON_BUILD_UV_VERSION");
    let manifest_directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be available"),
    );
    let pyproject_path = manifest_directory.join("../../pyproject.toml");
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
        common: settings_string_array(&pyproject, &["project", "dependencies"]),
        worker_script: settings_string_array(&pyproject, &["dependency-groups", "worker-script"]),
        worker_dynamic: settings_string_array(&pyproject, &["dependency-groups", "worker-dynamic"]),
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
            let executable_name = if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
                "uv.exe"
            } else {
                "uv"
            };
            Some(EmbeddedUvSource {
                path: format!("uv/{executable_name}"),
                version,
                sha256: digest_build_input(&path),
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

#[derive(Debug)]
enum ResourceProvenanceEnvironmentError {
    Missing,
    NonUnicode,
    Malformed,
}

impl fmt::Display for ResourceProvenanceEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("POKECON_RESOURCE_PROVENANCE is required"),
            Self::NonUnicode => {
                formatter.write_str("POKECON_RESOURCE_PROVENANCE must be valid Unicode")
            }
            Self::Malformed => formatter.write_str(
                "POKECON_RESOURCE_PROVENANCE must be exactly development, nix-exact, or packaged: followed by 64 lowercase hexadecimal characters",
            ),
        }
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validated_resource_provenance() -> Result<String, ResourceProvenanceEnvironmentError> {
    let value = match env::var(RESOURCE_PROVENANCE_ENVIRONMENT) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => {
            return Err(ResourceProvenanceEnvironmentError::Missing);
        }
        Err(env::VarError::NotUnicode(_value)) => {
            return Err(ResourceProvenanceEnvironmentError::NonUnicode);
        }
    };
    let valid = matches!(value.as_str(), "development" | "nix-exact")
        || value
            .strip_prefix("packaged:")
            .is_some_and(is_lowercase_sha256);
    if valid {
        Ok(value)
    } else {
        Err(ResourceProvenanceEnvironmentError::Malformed)
    }
}

fn make_copied_file_owner_writable(
    destination: &Path,
    mut permissions: fs::Permissions,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    {
        permissions.set_readonly(false);
    }
    fs::set_permissions(destination, permissions)
}

fn copy_regular_file(source: &Path, destination: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Tauri build input is not a real regular file: {}",
                source.display()
            ),
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    make_copied_file_owner_writable(destination, metadata.permissions())?;
    fs::OpenOptions::new()
        .write(true)
        .open(destination)?
        .set_times(fs::FileTimes::new().set_modified(metadata.modified()?))
}

fn copy_directory(
    source_root: &Path,
    destination_root: &Path,
    excluded_relative_root: Option<&Path>,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source_root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Tauri build input is not a real directory: {}",
                source_root.display()
            ),
        ));
    }
    fs::create_dir_all(destination_root)?;
    let mut pending = vec![source_root.to_path_buf()];
    while let Some(source_directory) = pending.pop() {
        let mut entries = fs::read_dir(&source_directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let source = entry.path();
            let relative = source.strip_prefix(source_root).map_err(io::Error::other)?;
            if excluded_relative_root.is_some_and(|excluded| relative.starts_with(excluded)) {
                continue;
            }
            let destination = destination_root.join(relative);
            let entry_metadata = fs::symlink_metadata(&source)?;
            if entry_metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Tauri build input contains a symlink: {}", source.display()),
                ));
            }
            if entry_metadata.is_dir() {
                fs::create_dir(&destination)?;
                pending.push(source);
            } else if entry_metadata.is_file() {
                copy_regular_file(&source, &destination)?;
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Tauri build input has an unsupported type: {}",
                        source.display()
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn copy_optional_directory(
    source_root: &Path,
    destination_root: &Path,
    excluded_relative_root: Option<&Path>,
) -> io::Result<bool> {
    match fs::symlink_metadata(source_root) {
        Ok(_) => {
            copy_directory(source_root, destination_root, excluded_relative_root)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn prepare_tauri_build_context(
    manifest_directory: &Path,
    out_directory: &Path,
) -> io::Result<PathBuf> {
    let workspace_root = manifest_directory
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("PokeCon manifest has no workspace root"))?;
    let context_root = out_directory.join("tauri-build-context");
    match fs::symlink_metadata(&context_root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Tauri build context is redirected or invalid: {}",
                    context_root.display()
                ),
            ));
        }
        Ok(_) => fs::remove_dir_all(&context_root)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let context_manifest_directory = context_root.join("rust/pokecon");
    fs::create_dir_all(&context_manifest_directory)?;
    for relative in ["Cargo.toml", "tauri.conf.json"] {
        copy_regular_file(
            &manifest_directory.join(relative),
            &context_manifest_directory.join(relative),
        )?;
        println!(
            "cargo:rerun-if-changed={}",
            manifest_directory.join(relative).display()
        );
    }
    for relative in ["Cargo.toml", "LICENSE"] {
        copy_regular_file(&workspace_root.join(relative), &context_root.join(relative))?;
        println!(
            "cargo:rerun-if-changed={}",
            workspace_root.join(relative).display()
        );
    }
    for relative in [
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
        "icons/icon.ico",
        "linux/70-pokecon-controller.rules",
        "linux/reload-udev.sh",
    ] {
        copy_regular_file(
            &manifest_directory.join(relative),
            &context_manifest_directory.join(relative),
        )?;
        println!(
            "cargo:rerun-if-changed={}",
            manifest_directory.join(relative).display()
        );
    }
    let mut missing_optional_directory = false;
    for (relative, excluded_relative_root) in [
        ("permissions", Some(Path::new("autogenerated"))),
        ("capabilities", None),
    ] {
        let source = manifest_directory.join(relative);
        let destination = context_manifest_directory.join(relative);
        if copy_optional_directory(&source, &destination, excluded_relative_root)? {
            println!("cargo:rerun-if-changed={}", source.display());
        } else {
            missing_optional_directory = true;
        }
    }
    if missing_optional_directory {
        println!("cargo:rerun-if-changed={}", manifest_directory.display());
    }
    Ok(context_manifest_directory)
}

fn tauri_current_directory_is_compatible(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        matches!(
            path.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
        )
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        true
    }
}

fn run_tauri_build() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_directory = dunce::canonicalize(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or("Cargo did not provide CARGO_MANIFEST_DIR to the PokeCon build script")?,
    )?;
    let out_directory = dunce::canonicalize(
        env::var_os("OUT_DIR")
            .ok_or("Cargo did not provide OUT_DIR to the PokeCon build script")?,
    )?;
    let context_manifest_directory =
        prepare_tauri_build_context(&manifest_directory, &out_directory)?;
    if !tauri_current_directory_is_compatible(&context_manifest_directory) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Tauri build context cannot be represented as a conventional Windows disk path: {}",
                context_manifest_directory.display()
            ),
        )
        .into());
    }
    env::set_current_dir(&context_manifest_directory)?;
    let manifest = tauri_build::AppManifest::new()
        .commands(&["choose_save_path", "open_config_directory"])
        .permissions_path_pattern("./permissions/**/*");
    let build_result = tauri_build::try_build(
        tauri_build::Attributes::new()
            .capabilities_path_pattern("./capabilities/**/*")
            .app_manifest(manifest),
    );
    let restore_result = env::set_current_dir(&manifest_directory);
    restore_result?;
    build_result?;
    Ok(())
}

fn main() {
    println!("cargo:rerun-if-env-changed=POKECON_BUILD_PYTHON");
    println!("cargo:rerun-if-env-changed={RESOURCE_PROVENANCE_ENVIRONMENT}");
    let provenance = validated_resource_provenance().unwrap_or_else(|error| panic!("{error}"));
    println!("cargo:rustc-env={RESOURCE_PROVENANCE_ENVIRONMENT}={provenance}");
    generate_settings_resources();
    run_tauri_build().expect("Tauri application metadata must be valid");
}
