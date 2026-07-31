use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::OpenOptions;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::settings::hmac_key::{HmacKey, HmacKeyError};
use crate::settings::path::{PathError, canonical_identity};
use crate::settings::roots::EffectiveRoots;

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Non-secret identities participating in a venv input fingerprint.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FingerprintInput {
    pub python_build_id: String,
    pub uv_build_id: String,
    pub operating_system: String,
    pub kernel: String,
    pub architecture: String,
    pub application_requirements: Vec<String>,
    pub resolved_requirements: serde_json::Value,
    pub override_application_constraints: bool,
    pub override_package_metadata_constraints: bool,
    pub application_build_id: String,
    pub package_source_build_id: Option<String>,
    pub canonical_venv_path: String,
    pub revalidate_mutable_sources: bool,
    pub normalized_extras: BTreeMap<String, Vec<String>>,
}

impl fmt::Debug for FingerprintInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FingerprintInput")
            .field("python_build_id", &self.python_build_id)
            .field("uv_build_id", &self.uv_build_id)
            .field("operating_system", &self.operating_system)
            .field("kernel", &self.kernel)
            .field("architecture", &self.architecture)
            .field(
                "application_requirement_count",
                &self.application_requirements.len(),
            )
            .field("resolved_requirements", &"<redacted>")
            .field(
                "override_application_constraints",
                &self.override_application_constraints,
            )
            .field(
                "override_package_metadata_constraints",
                &self.override_package_metadata_constraints,
            )
            .field("application_build_id", &self.application_build_id)
            .field("package_source_build_id", &self.package_source_build_id)
            .field("canonical_venv_path", &self.canonical_venv_path)
            .field(
                "revalidate_mutable_sources",
                &self.revalidate_mutable_sources,
            )
            .field("normalized_extras", &self.normalized_extras)
            .finish()
    }
}

/// Secret inputs supplied only while calculating keyed digests.
#[derive(Clone, Default)]
pub struct SecretFingerprintInput {
    pub uv_config: Option<Vec<u8>>,
    pub uv_environment: BTreeMap<String, Vec<u8>>,
}

impl fmt::Debug for SecretFingerprintInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretFingerprintInput(<redacted>)")
    }
}

/// Persistable fingerprint containing no raw secret material.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FingerprintRecord {
    pub sha256: String,
    pub uv_config_hmac: Option<String>,
    pub uv_environment_hmac: BTreeMap<String, String>,
}

impl FingerprintRecord {
    /// Calculates a deterministic public digest plus keyed secret digests.
    ///
    /// # Errors
    ///
    /// Returns an error only if the typed fingerprint cannot serialize.
    pub fn calculate(
        input: &FingerprintInput,
        secrets: &SecretFingerprintInput,
        key: &HmacKey,
    ) -> Result<Self, ManifestError> {
        let uv_config_hmac = secrets.uv_config.as_deref().map(|value| key.digest(value));
        let uv_environment_hmac = secrets
            .uv_environment
            .iter()
            .map(|(name, value)| (name.clone(), key.digest(value)))
            .collect::<BTreeMap<_, _>>();
        let envelope = (&input, &uv_config_hmac, &uv_environment_hmac);
        let encoded = serde_json::to_vec(&envelope)?;
        Ok(Self {
            sha256: hex::encode(Sha256::digest(encoded)),
            uv_config_hmac,
            uv_environment_hmac,
        })
    }
}

/// One exact distribution installed in a successfully prepared venv.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledDistribution {
    pub name: String,
    pub version: String,
}

/// Verified output identity from uv and the target interpreter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestOutput {
    pub python_build_id: String,
    pub distributions: Vec<InstalledDistribution>,
    pub direct_source_identities: BTreeMap<String, String>,
    pub consistent: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UnsignedManifest {
    schema_version: u32,
    fingerprint: FingerprintRecord,
    output: ManifestOutput,
    completed_at: DateTime<Utc>,
}

/// Authenticated readiness manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedManifest {
    #[serde(flatten)]
    unsigned: UnsignedManifest,
    signature: String,
}

impl SignedManifest {
    /// Creates and signs a successful readiness record.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical serialization fails.
    pub fn new(
        fingerprint: FingerprintRecord,
        mut output: ManifestOutput,
        completed_at: DateTime<Utc>,
        key: &HmacKey,
    ) -> Result<Self, ManifestError> {
        output.distributions.sort();
        let unsigned = UnsignedManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            fingerprint,
            output,
            completed_at,
        };
        let signature = key.digest(&serde_json::to_vec(&unsigned)?);
        Ok(Self {
            unsigned,
            signature,
        })
    }

    #[must_use]
    pub fn fingerprint(&self) -> &FingerprintRecord {
        &self.unsigned.fingerprint
    }

    #[must_use]
    pub fn output(&self) -> &ManifestOutput {
        &self.unsigned.output
    }

    #[must_use]
    pub fn verify(&self, key: &HmacKey) -> bool {
        self.unsigned.schema_version == MANIFEST_SCHEMA_VERSION
            && serde_json::to_vec(&self.unsigned)
                .is_ok_and(|encoded| key.verify(&encoded, &self.signature))
    }
}

/// Missing/valid/corrupt manifest classification used for safe rebuilds.
#[derive(Clone, Debug)]
pub enum ManifestRead {
    Missing,
    Valid(SignedManifest),
    Corrupt,
}

/// Per-venv manifest storage in the effective Data root.
#[derive(Clone, Debug)]
pub struct ManifestStore {
    directory: PathBuf,
}

impl ManifestStore {
    #[must_use]
    pub fn new(roots: &EffectiveRoots) -> Self {
        Self {
            directory: roots.data.join("venv-manifests"),
        }
    }

    #[must_use]
    pub const fn directory(&self) -> &PathBuf {
        &self.directory
    }

    /// Loads or exclusively creates the per-install key.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing/corrupt/unreadable key.
    pub fn key(&self) -> Result<HmacKey, ManifestError> {
        HmacKey::load_or_create(&self.directory).map_err(ManifestError::Key)
    }

    /// Returns the hashed manifest path for a canonical venv identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the venv identity cannot be resolved.
    pub fn path_for(&self, venv: &Path) -> Result<PathBuf, ManifestError> {
        let identity = canonical_identity(venv)?;
        let digest = Sha256::digest(path_bytes(&identity));
        Ok(self.directory.join(format!("{}.json", hex::encode(digest))))
    }

    /// Reads and authenticates a manifest. Parse/signature/schema failures are
    /// classified as corrupt so callers rebuild rather than trust them.
    ///
    /// # Errors
    ///
    /// Returns an error only for non-NotFound I/O or key failure.
    pub fn read(&self, venv: &Path, key: &HmacKey) -> Result<ManifestRead, ManifestError> {
        let path = self.path_for(venv)?;
        let source = match fs::read(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ManifestRead::Missing);
            }
            Err(source) => return Err(ManifestError::Io { path, source }),
        };
        let Ok(manifest) = serde_json::from_slice::<SignedManifest>(&source) else {
            return Ok(ManifestRead::Corrupt);
        };
        if manifest.verify(key) {
            Ok(ManifestRead::Valid(manifest))
        } else {
            Ok(ManifestRead::Corrupt)
        }
    }

    /// Atomically replaces a manifest after complete preparation succeeds.
    /// Callers must hold the matching per-venv lock.
    ///
    /// # Errors
    ///
    /// Returns an error for serialization, I/O, sync, or commit failure.
    pub fn write(&self, venv: &Path, manifest: &SignedManifest) -> Result<(), ManifestError> {
        fs::create_dir_all(&self.directory).map_err(|source| ManifestError::Io {
            path: self.directory.clone(),
            source,
        })?;
        let path = self.path_for(venv)?;
        #[cfg(unix)]
        let options = {
            use std::os::unix::fs::OpenOptionsExt;

            let mut options = OpenOptions::new();
            options.mode(0o600);
            options
        };
        #[cfg(not(unix))]
        let options = OpenOptions::new();
        let mut file = options.open(&path).map_err(|source| ManifestError::Io {
            path: path.clone(),
            source,
        })?;
        file.write_all(&serde_json::to_vec_pretty(manifest)?)
            .map_err(|source| ManifestError::Io {
                path: path.clone(),
                source,
            })?;
        file.commit().map_err(|source| ManifestError::Io {
            path: path.clone(),
            source,
        })?;
        sync_parent(&self.directory)
    }
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

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), ManifestError> {
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| ManifestError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), ManifestError> {
    Ok(())
}

/// Authenticated readiness-manifest failures.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error(transparent)]
    Key(#[from] HmacKeyError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("manifest serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("manifest I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use chrono::Utc;
    use tempfile::TempDir;

    use super::{
        FingerprintInput, FingerprintRecord, InstalledDistribution, ManifestOutput, ManifestRead,
        ManifestStore, SecretFingerprintInput, SignedManifest,
    };
    use crate::settings::roots::{BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent};

    fn roots(temp: &TempDir) -> EffectiveRoots {
        let base = temp.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("bases must resolve");
        EffectiveRoots::from_bases(
            SafeComponent::new("pokecon").expect("name must be safe"),
            &bases,
        )
    }

    #[test]
    fn raw_secrets_never_enter_a_signed_manifest_and_tampering_is_rejected() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must be created");
        let store = ManifestStore::new(&roots);
        let key = store.key().expect("key must be available");
        let raw_resolved = "https://user:raw-secret@example.invalid/package.whl";
        let input = FingerprintInput {
            python_build_id: "cpython-3.14-test".to_owned(),
            uv_build_id: "uv-test".to_owned(),
            operating_system: "test".to_owned(),
            kernel: "test".to_owned(),
            architecture: "test".to_owned(),
            application_requirements: vec![],
            resolved_requirements: serde_json::json!([raw_resolved]),
            override_application_constraints: false,
            override_package_metadata_constraints: false,
            application_build_id: "app-test".to_owned(),
            package_source_build_id: None,
            canonical_venv_path: "/test/venv".to_owned(),
            revalidate_mutable_sources: false,
            normalized_extras: BTreeMap::new(),
        };
        let secret = b"https://token@example.invalid/simple".to_vec();
        let secret_input = SecretFingerprintInput {
            uv_config: Some(secret.clone()),
            uv_environment: BTreeMap::from([("POKECON_UV_INDEX_URL".to_owned(), secret.clone())]),
        };
        assert!(!format!("{input:?}").contains(raw_resolved));
        assert!(!format!("{secret_input:?}").contains("token"));
        let fingerprint = FingerprintRecord::calculate(&input, &secret_input, &key)
            .expect("fingerprint must be created");
        let venv = roots.data.join("venv-script");
        let manifest = SignedManifest::new(
            fingerprint,
            ManifestOutput {
                python_build_id: "cpython-3.14-test".to_owned(),
                distributions: vec![InstalledDistribution {
                    name: "example".to_owned(),
                    version: "1.0".to_owned(),
                }],
                direct_source_identities: BTreeMap::new(),
                consistent: true,
            },
            Utc::now(),
            &key,
        )
        .expect("manifest must be signed");
        store
            .write(&venv, &manifest)
            .expect("manifest must be written");
        let path = store.path_for(&venv).expect("path must resolve");
        let persisted = fs::read(&path).expect("manifest must be readable");
        assert!(
            !persisted
                .windows(secret.len())
                .any(|window| window == secret)
        );
        assert!(!String::from_utf8_lossy(&persisted).contains(raw_resolved));
        assert!(matches!(
            store.read(&venv, &key).expect("read must succeed"),
            ManifestRead::Valid(_)
        ));

        let mut tampered = persisted;
        let position = tampered
            .iter()
            .position(|byte| *byte == b'1')
            .expect("fixture must contain a digit");
        tampered[position] = b'2';
        fs::write(path, tampered).expect("tampered fixture must be writable");
        assert!(matches!(
            store.read(&venv, &key).expect("read must succeed"),
            ManifestRead::Corrupt
        ));
    }
}
