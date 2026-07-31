use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use fs4::FileExt;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::settings::path::{PathError, canonical_identity};
use crate::settings::roots::EffectiveRoots;

/// Domain-separated settings and venv lock paths.
#[derive(Clone, Debug)]
pub struct LockManager {
    settings: PathBuf,
    venvs: PathBuf,
    runtime: PathBuf,
}

impl LockManager {
    /// Creates a manager rooted exclusively in the effective State root.
    #[must_use]
    pub fn new(roots: &EffectiveRoots) -> Self {
        Self {
            settings: roots.state.join("settings-locks"),
            venvs: roots.state.join("venv-locks"),
            runtime: roots.state.join("runtime-locks"),
        }
    }

    /// Acquires the canonical settings-file lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock identity or OS lock cannot be created.
    pub fn settings(&self, target: &Path) -> Result<FileLockGuard, LockError> {
        Self::acquire(&self.settings, "settings::", target)
    }

    /// Acquires the canonical per-venv lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock identity or OS lock cannot be created.
    pub fn venv(&self, target: &Path) -> Result<FileLockGuard, LockError> {
        Self::acquire(&self.venvs, "venv::", target)
    }

    /// Acquires the canonical app-managed runtime installation lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock identity or OS lock cannot be created.
    pub fn runtime(&self, target: &Path) -> Result<FileLockGuard, LockError> {
        Self::acquire(&self.runtime, "runtime::", target)
    }

    /// Returns the non-reversible per-venv lock filename without acquiring it.
    ///
    /// # Errors
    ///
    /// Returns an error when the target cannot be made canonical.
    pub fn venv_lock_name(&self, target: &Path) -> Result<String, LockError> {
        Ok(format!("{}.lock", identity_hash("venv::", target)?))
    }

    fn acquire(directory: &Path, domain: &str, target: &Path) -> Result<FileLockGuard, LockError> {
        fs::create_dir_all(directory).map_err(|source| LockError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let lock_path = directory.join(format!("{}.lock", identity_hash(domain, target)?));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| LockError::Io {
                path: lock_path.clone(),
                source,
            })?;
        FileExt::lock(&file).map_err(|source| LockError::Io {
            path: lock_path.clone(),
            source,
        })?;
        Ok(FileLockGuard {
            _file: file,
            path: lock_path,
        })
    }
}

fn identity_hash(domain: &str, target: &Path) -> Result<String, LockError> {
    let canonical = canonical_identity(target)?;
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update([0]);
    digest.update(path_bytes(&canonical));
    Ok(hex::encode(digest.finalize()))
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

/// RAII ownership of an operating-system advisory exclusive lock.
#[derive(Debug)]
pub struct FileLockGuard {
    _file: File,
    path: PathBuf,
}

impl FileLockGuard {
    /// Returns the hashed lock path. It never contains the original target.
    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// Cross-process lock failures.
#[derive(Debug, Error)]
pub enum LockError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("lock operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
