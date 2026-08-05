use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

const RESOURCE_PROVENANCE_ENVIRONMENT: &str = "POKECON_RESOURCE_PROVENANCE";

#[derive(Debug)]
enum ResourceProvenanceEnvironmentError {
    Missing,
    NonUnicode,
    Malformed,
}

impl fmt::Display for ResourceProvenanceEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str(
                "POKECON_RESOURCE_PROVENANCE is required when tauri-shell is enabled",
            ),
            Self::NonUnicode => formatter.write_str(
                "POKECON_RESOURCE_PROVENANCE must be valid Unicode when tauri-shell is enabled",
            ),
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
    make_copied_file_owner_writable(destination, metadata.permissions())
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
) -> io::Result<()> {
    match fs::symlink_metadata(source_root) {
        Ok(_) => copy_directory(source_root, destination_root, excluded_relative_root),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
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
    copy_optional_directory(
        &manifest_directory.join("permissions"),
        &context_manifest_directory.join("permissions"),
        Some(Path::new("autogenerated")),
    )?;
    copy_optional_directory(
        &manifest_directory.join("capabilities"),
        &context_manifest_directory.join("capabilities"),
        None,
    )?;
    println!(
        "cargo:rerun-if-changed={}",
        manifest_directory.join("permissions").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_directory.join("capabilities").display()
    );
    Ok(context_manifest_directory)
}

fn run_tauri_build() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_directory = fs::canonicalize(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or("Cargo did not provide CARGO_MANIFEST_DIR to the PokeCon build script")?,
    )?;
    let out_directory = fs::canonicalize(
        env::var_os("OUT_DIR")
            .ok_or("Cargo did not provide OUT_DIR to the PokeCon build script")?,
    )?;
    let context_manifest_directory =
        prepare_tauri_build_context(&manifest_directory, &out_directory)?;
    env::set_current_dir(&context_manifest_directory)?;
    let manifest =
        tauri_build::AppManifest::new().commands(&["choose_save_path", "open_config_directory"]);
    let build_result =
        tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest));
    let restore_result = env::set_current_dir(&manifest_directory);
    restore_result?;
    build_result?;
    Ok(())
}

fn main() {
    println!("cargo:rerun-if-env-changed=POKECON_BUILD_PYTHON");
    if env::var_os("CARGO_FEATURE_TAURI_SHELL").is_some() {
        println!("cargo:rerun-if-env-changed={RESOURCE_PROVENANCE_ENVIRONMENT}");
        let provenance = validated_resource_provenance().unwrap_or_else(|error| panic!("{error}"));
        println!("cargo:rustc-env={RESOURCE_PROVENANCE_ENVIRONMENT}={provenance}");
        run_tauri_build().expect("Tauri application metadata must be valid");
    }
}
