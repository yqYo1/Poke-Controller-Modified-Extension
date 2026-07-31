use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use getrandom::fill;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;

const KEY_LENGTH: usize = 32;

/// Per-install HMAC-SHA-256 key whose debug representation is always redacted.
#[derive(Clone)]
pub struct HmacKey([u8; KEY_LENGTH]);

impl HmacKey {
    /// Loads the winning key or creates it with an exclusive, no-replace race.
    ///
    /// Candidate keys are fully written and synced before an atomic hard-link
    /// publish. Losing processes discard their candidate and reread the
    /// winner, so concurrent startup cannot produce split manifest signatures.
    ///
    /// # Errors
    ///
    /// Returns an error for randomness, I/O, a non-regular key, or a key whose
    /// length is not exactly 32 bytes. A corrupt existing key is never replaced.
    pub fn load_or_create(directory: &Path) -> Result<Self, HmacKeyError> {
        fs::create_dir_all(directory).map_err(|source| HmacKeyError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let key_path = directory.join(".hmac-key");
        match Self::read(&key_path) {
            Ok(key) => return Ok(key),
            Err(HmacKeyError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let mut key = [0_u8; KEY_LENGTH];
        fill(&mut key).map_err(HmacKeyError::Random)?;
        let mut suffix = [0_u8; 12];
        fill(&mut suffix).map_err(HmacKeyError::Random)?;
        let candidate_path = directory.join(format!(
            ".hmac-key.{}.{}.tmp",
            std::process::id(),
            hex::encode(suffix)
        ));
        let result = publish_candidate(&candidate_path, &key_path, &key);
        let _cleanup_result = fs::remove_file(&candidate_path);
        match result {
            Ok(true) => {
                sync_parent(directory)?;
                Ok(Self(key))
            }
            Ok(false) => Self::read(&key_path),
            Err(error) => Err(error),
        }
    }

    /// Computes a lowercase HMAC-SHA-256 digest.
    ///
    /// # Panics
    ///
    /// Panics only if the HMAC implementation rejects this type's fixed
    /// 32-byte key, which is an invariant of HMAC-SHA-256.
    #[must_use]
    pub fn digest(&self, value: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0)
            .expect("HMAC accepts every key length supported by this type");
        mac.update(value);
        hex::encode(mac.finalize().into_bytes())
    }

    /// Verifies a lowercase hexadecimal HMAC in constant time.
    ///
    /// # Panics
    ///
    /// Panics only if the HMAC implementation rejects this type's fixed
    /// 32-byte key, which is an invariant of HMAC-SHA-256.
    #[must_use]
    pub fn verify(&self, value: &[u8], expected: &str) -> bool {
        let Ok(expected) = hex::decode(expected) else {
            return false;
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0)
            .expect("HMAC accepts every key length supported by this type");
        mac.update(value);
        mac.verify_slice(&expected).is_ok()
    }

    fn read(path: &Path) -> Result<Self, HmacKeyError> {
        let metadata = fs::symlink_metadata(path).map_err(|source| HmacKeyError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if !metadata.file_type().is_file() {
            return Err(HmacKeyError::NotRegular(path.to_path_buf()));
        }
        let mut bytes = Vec::with_capacity(KEY_LENGTH);
        File::open(path)
            .and_then(|mut file| file.read_to_end(&mut bytes))
            .map_err(|source| HmacKeyError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        let key: [u8; KEY_LENGTH] =
            bytes
                .try_into()
                .map_err(|bytes: Vec<u8>| HmacKeyError::InvalidLength {
                    path: path.to_path_buf(),
                    actual: bytes.len(),
                })?;
        Ok(Self(key))
    }
}

impl fmt::Debug for HmacKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HmacKey(<redacted>)")
    }
}

fn publish_candidate(
    candidate_path: &Path,
    key_path: &Path,
    key: &[u8; KEY_LENGTH],
) -> Result<bool, HmacKeyError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut candidate = options
        .open(candidate_path)
        .map_err(|source| HmacKeyError::Io {
            path: candidate_path.to_path_buf(),
            source,
        })?;
    candidate
        .write_all(key)
        .and_then(|()| candidate.sync_all())
        .map_err(|source| HmacKeyError::Io {
            path: candidate_path.to_path_buf(),
            source,
        })?;
    match fs::hard_link(candidate_path, key_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(source) => Err(HmacKeyError::Io {
            path: key_path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), HmacKeyError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| HmacKeyError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), HmacKeyError> {
    Ok(())
}

/// Exclusive HMAC-key creation and validation failures.
#[derive(Debug, Error)]
pub enum HmacKeyError {
    #[error("secure random generation failed")]
    Random(#[source] getrandom::Error),
    #[error("HMAC key is not a regular file: {0}")]
    NotRegular(PathBuf),
    #[error("HMAC key has invalid length {actual}, expected {KEY_LENGTH}: {path}")]
    InvalidLength { path: PathBuf, actual: usize },
    #[error("HMAC key I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::HmacKey;

    #[test]
    fn concurrent_key_creation_converges_on_one_winner() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let directory = temporary.path().join("venv-manifests");
        let barrier = Arc::new(Barrier::new(12));
        let handles = (0..12)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let directory = directory.clone();
                thread::spawn(move || {
                    barrier.wait();
                    HmacKey::load_or_create(&directory)
                        .expect("key creation must converge")
                        .digest(b"same input")
                })
            })
            .collect::<Vec<_>>();
        let digests = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread must finish"))
            .collect::<Vec<_>>();
        assert!(digests.windows(2).all(|pair| pair[0] == pair[1]));
    }
}
