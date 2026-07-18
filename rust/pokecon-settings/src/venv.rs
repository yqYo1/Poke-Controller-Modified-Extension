use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::Mutex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::process::Command;
use tokio::sync::Notify;

use crate::lock::LockManager;
use crate::manifest::{
    FingerprintInput, FingerprintRecord, InstalledDistribution, ManifestOutput, ManifestRead,
    ManifestStore, SecretFingerprintInput, SignedManifest,
};
use crate::package::{ApplicationRequirements, PackageResolution, PythonWorker, VersionSelector};
use crate::path::canonical_identity;
use crate::roots::EffectiveRoots;
use crate::uv::{ManagedUv, UvChildEnvironment, UvInvocation, UvPlan, UvPlanInput, venv_python};

/// Whether the application may stage/replace the entire venv directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VenvOwnership {
    AppManaged,
    UserSpecified,
}

/// Public preparation lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VenvPreparationState {
    Idle,
    SingleFlightWaiting,
    LockWaiting,
    Preparing,
    Canceling,
    Success,
    Failure,
    Unavailable,
}

/// Complete, immutable inputs for one exact-sync transaction.
#[derive(Clone, Debug)]
pub struct VenvPreparationRequest {
    pub worker: PythonWorker,
    pub venv: PathBuf,
    pub ownership: VenvOwnership,
    pub managed_uv: ManagedUv,
    pub python: PathBuf,
    pub python_build_id: String,
    pub packages: PackageResolution,
    pub override_application_constraints: bool,
    pub override_package_metadata_constraints: bool,
    pub uv_config: Option<PathBuf>,
    pub uv_environment: UvChildEnvironment,
    pub revalidate_mutable_sources: bool,
    pub application_build_id: String,
    pub kernel_id: String,
}

/// Context supplied to the production or test uv executor.
#[derive(Clone, Debug)]
pub struct UvExecutionContext {
    pub request: VenvPreparationRequest,
    pub canonical_venv: PathBuf,
    pub cache_root: PathBuf,
}

/// uv execution boundary, enabling deterministic single-flight and fault tests.
#[async_trait]
pub trait UvExecutor: Send + Sync {
    async fn prepare(&self, context: &UvExecutionContext) -> Result<ManifestOutput, VenvError>;
    async fn inspect(&self, context: &UvExecutionContext) -> Result<ManifestOutput, VenvError>;
    async fn revalidate_mutable_sources(
        &self,
        context: &UvExecutionContext,
    ) -> Result<BTreeMap<String, String>, VenvError>;
}

/// Real managed-uv executor. No shell is used and child output is never echoed
/// in failure messages.
#[derive(Clone, Debug, Default)]
pub struct CommandUvExecutor;

#[async_trait]
impl UvExecutor for CommandUvExecutor {
    async fn prepare(&self, context: &UvExecutionContext) -> Result<ManifestOutput, VenvError> {
        let workspace = create_workspace(&context.cache_root)?;
        let requirements = workspace.path().join("requirements.in");
        let constraints = workspace.path().join("constraints.txt");
        let overrides = workspace.path().join("overrides.txt");
        let compiled = workspace.path().join("requirements.lock");
        write_lines(&requirements, &context.request.packages.requirements)?;
        let constraints_path =
            write_optional_lines(&constraints, &context.request.packages.constraints)?;
        let overrides_path = write_optional_lines(&overrides, &context.request.packages.overrides)?;

        let (target, staging) = staging_target(context)?;
        let plan = UvPlan::new(&UvPlanInput {
            managed_uv: context.request.managed_uv.clone(),
            python: context.request.python.clone(),
            venv: target.clone(),
            cache: context.cache_root.join("uv"),
            requirements,
            compiled: compiled.clone(),
            constraints: constraints_path,
            overrides: overrides_path,
            uv_config: context.request.uv_config.clone(),
            environment: context.request.uv_environment.clone(),
        });
        run_invocation(&plan.compile, VenvStage::Resolve).await?;
        run_invocation(&plan.create_venv, VenvStage::Create).await?;
        run_invocation(&plan.sync, VenvStage::ExactSync).await?;
        run_invocation(&plan.check, VenvStage::Check).await?;
        let inventory = run_invocation_output(&plan.inventory, VenvStage::Inventory).await?;
        let distributions = parse_inventory(&inventory)?;
        let compiled_identity = digest_file(&compiled)?;
        if let Some(staging) = staging {
            commit_staging(staging, &context.canonical_venv)?;
        }
        Ok(ManifestOutput {
            python_build_id: context.request.python_build_id.clone(),
            distributions,
            direct_source_identities: direct_identity_map(
                &context.request.packages,
                &compiled_identity,
            ),
            consistent: true,
        })
    }

    async fn inspect(&self, context: &UvExecutionContext) -> Result<ManifestOutput, VenvError> {
        if !venv_python(&context.canonical_venv).is_file() {
            return Err(VenvError::new(
                VenvStage::Inspect,
                VenvFailure::EnvironmentMissing,
            ));
        }
        let workspace = create_workspace(&context.cache_root)?;
        let placeholder = workspace.path().join("placeholder.txt");
        write_lines(&placeholder, &[])?;
        let plan = UvPlan::new(&UvPlanInput {
            managed_uv: context.request.managed_uv.clone(),
            python: context.request.python.clone(),
            venv: context.canonical_venv.clone(),
            cache: context.cache_root.join("uv"),
            requirements: placeholder.clone(),
            compiled: placeholder,
            constraints: None,
            overrides: None,
            uv_config: context.request.uv_config.clone(),
            environment: context.request.uv_environment.clone(),
        });
        run_invocation(&plan.check, VenvStage::Check).await?;
        let inventory = run_invocation_output(&plan.inventory, VenvStage::Inventory).await?;
        Ok(ManifestOutput {
            python_build_id: context.request.python_build_id.clone(),
            distributions: parse_inventory(&inventory)?,
            direct_source_identities: BTreeMap::new(),
            consistent: true,
        })
    }

    async fn revalidate_mutable_sources(
        &self,
        context: &UvExecutionContext,
    ) -> Result<BTreeMap<String, String>, VenvError> {
        let workspace = create_workspace(&context.cache_root)?;
        let requirements = workspace.path().join("requirements.in");
        let constraints = workspace.path().join("constraints.txt");
        let overrides = workspace.path().join("overrides.txt");
        let compiled = workspace.path().join("requirements.lock");
        write_lines(&requirements, &context.request.packages.requirements)?;
        let plan = UvPlan::new(&UvPlanInput {
            managed_uv: context.request.managed_uv.clone(),
            python: context.request.python.clone(),
            venv: context.canonical_venv.clone(),
            cache: context.cache_root.join("uv"),
            requirements,
            compiled: compiled.clone(),
            constraints: write_optional_lines(&constraints, &context.request.packages.constraints)?,
            overrides: write_optional_lines(&overrides, &context.request.packages.overrides)?,
            uv_config: context.request.uv_config.clone(),
            environment: context.request.uv_environment.clone(),
        });
        run_invocation(&plan.compile, VenvStage::Revalidate).await?;
        Ok(direct_identity_map(
            &context.request.packages,
            &digest_file(&compiled)?,
        ))
    }
}

/// Per-app-name venv preparation coordinator.
#[derive(Clone)]
pub struct VenvManager {
    inner: Arc<VenvManagerInner>,
}

struct VenvManagerInner {
    roots: EffectiveRoots,
    locks: LockManager,
    manifests: ManifestStore,
    executor: Arc<dyn UvExecutor>,
    flights: Mutex<HashMap<PathBuf, Arc<Flight>>>,
    states: Mutex<HashMap<PathBuf, VenvPreparationState>>,
    mutable_checked: Mutex<BTreeSet<String>>,
}

struct Flight {
    result: tokio::sync::Mutex<Option<Result<PreparationResult, VenvError>>>,
    notify: Notify,
}

impl std::fmt::Debug for VenvManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VenvManager")
            .field("app_name", &self.inner.roots.app_name)
            .field("active_flights", &self.inner.flights.lock().len())
            .finish_non_exhaustive()
    }
}

impl VenvManager {
    #[must_use]
    pub fn new(roots: EffectiveRoots, executor: Arc<dyn UvExecutor>) -> Self {
        Self {
            inner: Arc::new(VenvManagerInner {
                locks: LockManager::new(&roots),
                manifests: ManifestStore::new(&roots),
                roots,
                executor,
                flights: Mutex::new(HashMap::new()),
                states: Mutex::new(HashMap::new()),
                mutable_checked: Mutex::new(BTreeSet::new()),
            }),
        }
    }

    /// Prepares one venv with process-local single-flight followed by the
    /// cross-process OS lock and lock-after-wait manifest revalidation.
    ///
    /// Concurrent followers share both success and failure; only a subsequent,
    /// distinct call retries a failed preparation.
    ///
    /// # Errors
    ///
    /// Returns a fail-soft, worker-local error. The application process need
    /// not terminate.
    pub async fn prepare(
        &self,
        request: VenvPreparationRequest,
    ) -> Result<PreparationResult, VenvError> {
        let canonical = canonical_identity(&request.venv)
            .map_err(|_| VenvError::new(VenvStage::Identity, VenvFailure::Filesystem))?;
        let (flight, leader) = {
            let mut flights = self.inner.flights.lock();
            if let Some(flight) = flights.get(&canonical) {
                (Arc::clone(flight), false)
            } else {
                let flight = Arc::new(Flight {
                    result: tokio::sync::Mutex::new(None),
                    notify: Notify::new(),
                });
                flights.insert(canonical.clone(), Arc::clone(&flight));
                (flight, true)
            }
        };
        if !leader {
            self.set_state(&canonical, VenvPreparationState::SingleFlightWaiting);
            loop {
                let notified = flight.notify.notified();
                if let Some(result) = flight.result.lock().await.clone() {
                    return result;
                }
                notified.await;
            }
        }

        let result = self.prepare_leader(request, canonical.clone()).await;
        *flight.result.lock().await = Some(result.clone());
        flight.notify.notify_waiters();
        self.inner.flights.lock().remove(&canonical);
        result
    }

    /// Returns the last public state for a canonical path.
    #[must_use]
    pub fn state(&self, venv: &Path) -> VenvPreparationState {
        canonical_identity(venv)
            .ok()
            .and_then(|identity| self.inner.states.lock().get(&identity).copied())
            .unwrap_or(VenvPreparationState::Idle)
    }

    async fn prepare_leader(
        &self,
        request: VenvPreparationRequest,
        canonical: PathBuf,
    ) -> Result<PreparationResult, VenvError> {
        self.set_state(&canonical, VenvPreparationState::LockWaiting);
        let locks = self.inner.locks.clone();
        let lock_target = canonical.clone();
        let _guard = tokio::task::spawn_blocking(move || locks.venv(&lock_target))
            .await
            .map_err(|_| VenvError::new(VenvStage::Lock, VenvFailure::Internal))?
            .map_err(|_| VenvError::new(VenvStage::Lock, VenvFailure::Filesystem))?;
        let context = UvExecutionContext {
            request,
            canonical_venv: canonical.clone(),
            cache_root: self.inner.roots.cache.clone(),
        };
        let result = self.prepare_locked(&context).await;
        self.set_state(
            &canonical,
            if result.is_ok() {
                VenvPreparationState::Success
            } else {
                VenvPreparationState::Failure
            },
        );
        result
    }

    async fn prepare_locked(
        &self,
        context: &UvExecutionContext,
    ) -> Result<PreparationResult, VenvError> {
        let key = self
            .inner
            .manifests
            .key()
            .map_err(|_| VenvError::new(VenvStage::Manifest, VenvFailure::Authentication))?;
        let fingerprint = fingerprint(context, &key)?;
        if let ManifestRead::Valid(manifest) = self
            .inner
            .manifests
            .read(&context.canonical_venv, &key)
            .map_err(|_| VenvError::new(VenvStage::Manifest, VenvFailure::Filesystem))?
            && manifest.fingerprint() == &fingerprint
            && self
                .manifest_environment_matches(context, &manifest)
                .await?
            && self.mutable_sources_reusable(context, &manifest).await?
        {
            return Ok(PreparationResult {
                disposition: PreparationDisposition::SkippedVerified,
                manifest,
            });
        }

        self.set_state(&context.canonical_venv, VenvPreparationState::Preparing);
        let output = self.inner.executor.prepare(context).await?;
        if !output.consistent {
            return Err(VenvError::new(
                VenvStage::Check,
                VenvFailure::InconsistentEnvironment,
            ));
        }
        let manifest = SignedManifest::new(fingerprint, output, Utc::now(), &key)
            .map_err(|_| VenvError::new(VenvStage::Manifest, VenvFailure::Internal))?;
        self.inner
            .manifests
            .write(&context.canonical_venv, &manifest)
            .map_err(|_| VenvError::new(VenvStage::Manifest, VenvFailure::Filesystem))?;
        if context.request.revalidate_mutable_sources {
            self.inner
                .mutable_checked
                .lock()
                .insert(manifest.fingerprint().sha256.clone());
        }
        Ok(PreparationResult {
            disposition: PreparationDisposition::Prepared,
            manifest,
        })
    }

    async fn manifest_environment_matches(
        &self,
        context: &UvExecutionContext,
        manifest: &SignedManifest,
    ) -> Result<bool, VenvError> {
        let Ok(mut actual) = self.inner.executor.inspect(context).await else {
            return Ok(false);
        };
        actual.distributions.sort();
        let expected = manifest.output();
        Ok(actual.python_build_id == expected.python_build_id
            && actual.distributions == expected.distributions
            && actual.consistent)
    }

    async fn mutable_sources_reusable(
        &self,
        context: &UvExecutionContext,
        manifest: &SignedManifest,
    ) -> Result<bool, VenvError> {
        if !context.request.revalidate_mutable_sources || !has_mutable_sources(&context.request) {
            return Ok(true);
        }
        let fingerprint = &manifest.fingerprint().sha256;
        if self.inner.mutable_checked.lock().contains(fingerprint) {
            return Ok(true);
        }
        let current = self
            .inner
            .executor
            .revalidate_mutable_sources(context)
            .await?;
        if current == manifest.output().direct_source_identities {
            self.inner
                .mutable_checked
                .lock()
                .insert(fingerprint.clone());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn set_state(&self, canonical: &Path, state: VenvPreparationState) {
        self.inner
            .states
            .lock()
            .insert(canonical.to_path_buf(), state);
    }
}

fn fingerprint(
    context: &UvExecutionContext,
    key: &crate::hmac_key::HmacKey,
) -> Result<FingerprintRecord, VenvError> {
    let application = ApplicationRequirements::embedded()
        .map_err(|_| VenvError::new(VenvStage::Fingerprint, VenvFailure::Internal))?;
    let application_requirements = application
        .entries(context.request.worker)
        .map_err(|_| VenvError::new(VenvStage::Fingerprint, VenvFailure::Internal))?
        .into_iter()
        .map(|entry| {
            entry.version.map_or(entry.name.clone(), |version| {
                format!("{}{}", entry.name, version)
            })
        })
        .collect();
    let normalized_extras = context
        .request
        .packages
        .packages
        .iter()
        .map(|(name, package)| (name.clone(), package.extras.clone()))
        .collect();
    let input = FingerprintInput {
        python_build_id: context.request.python_build_id.clone(),
        uv_build_id: context.request.managed_uv.build_id.clone(),
        operating_system: std::env::consts::OS.to_owned(),
        kernel: context.request.kernel_id.clone(),
        architecture: std::env::consts::ARCH.to_owned(),
        application_requirements,
        resolved_requirements: serde_json::to_value(&context.request.packages)
            .map_err(|_| VenvError::new(VenvStage::Fingerprint, VenvFailure::Internal))?,
        override_application_constraints: context.request.override_application_constraints,
        override_package_metadata_constraints: context
            .request
            .override_package_metadata_constraints,
        application_build_id: context.request.application_build_id.clone(),
        canonical_venv_path: context.canonical_venv.to_string_lossy().into_owned(),
        revalidate_mutable_sources: context.request.revalidate_mutable_sources,
        normalized_extras,
    };
    let uv_config = context
        .request
        .uv_config
        .as_ref()
        .map(fs::read)
        .transpose()
        .map_err(|_| VenvError::new(VenvStage::Fingerprint, VenvFailure::Filesystem))?;
    FingerprintRecord::calculate(
        &input,
        &SecretFingerprintInput {
            uv_config,
            uv_environment: context
                .request
                .uv_environment
                .bridged_secret_values()
                .clone(),
        },
        key,
    )
    .map_err(|_| VenvError::new(VenvStage::Fingerprint, VenvFailure::Internal))
}

fn has_mutable_sources(request: &VenvPreparationRequest) -> bool {
    request.packages.packages.values().any(|package| {
        matches!(
            &package.selector,
            VersionSelector::Direct(direct) if direct.mutable
        )
    })
}

fn create_workspace(cache_root: &Path) -> Result<TempDir, VenvError> {
    let directory = cache_root.join("venv-preparation");
    fs::create_dir_all(&directory)
        .map_err(|_| VenvError::new(VenvStage::Workspace, VenvFailure::Filesystem))?;
    tempfile::Builder::new()
        .prefix("prepare-")
        .tempdir_in(directory)
        .map_err(|_| VenvError::new(VenvStage::Workspace, VenvFailure::Filesystem))
}

fn write_lines(path: &Path, lines: &[String]) -> Result<(), VenvError> {
    let mut source = lines.join("\n");
    source.push('\n');
    fs::write(path, source)
        .map_err(|_| VenvError::new(VenvStage::Workspace, VenvFailure::Filesystem))
}

fn write_optional_lines(path: &Path, lines: &[String]) -> Result<Option<PathBuf>, VenvError> {
    if lines.is_empty() {
        Ok(None)
    } else {
        write_lines(path, lines)?;
        Ok(Some(path.to_path_buf()))
    }
}

struct VenvStaging {
    container: TempDir,
    target: PathBuf,
}

fn staging_target(
    context: &UvExecutionContext,
) -> Result<(PathBuf, Option<VenvStaging>), VenvError> {
    if context.request.ownership != VenvOwnership::AppManaged {
        return Ok((context.canonical_venv.clone(), None));
    }
    if context.canonical_venv.exists() && !context.canonical_venv.is_dir() {
        return Err(VenvError::new(VenvStage::Create, VenvFailure::Filesystem));
    }
    let parent = context
        .canonical_venv
        .parent()
        .ok_or_else(|| VenvError::new(VenvStage::Create, VenvFailure::Filesystem))?;
    fs::create_dir_all(parent)
        .map_err(|_| VenvError::new(VenvStage::Create, VenvFailure::Filesystem))?;
    let staging = tempfile::Builder::new()
        .prefix(".venv-stage-")
        .tempdir_in(parent)
        .map_err(|_| VenvError::new(VenvStage::Create, VenvFailure::Filesystem))?;
    let target = staging.path().join("venv");
    Ok((
        target.clone(),
        Some(VenvStaging {
            container: staging,
            target,
        }),
    ))
}

fn commit_staging(staging: VenvStaging, destination: &Path) -> Result<(), VenvError> {
    if !staging.target.is_dir() {
        return Err(VenvError::new(VenvStage::Commit, VenvFailure::Filesystem));
    }
    if !destination.exists() {
        return fs::rename(&staging.target, destination)
            .map_err(|_| VenvError::new(VenvStage::Commit, VenvFailure::Filesystem));
    }

    let parent = destination
        .parent()
        .ok_or_else(|| VenvError::new(VenvStage::Commit, VenvFailure::Filesystem))?;
    let backup = tempfile::Builder::new()
        .prefix(".venv-backup-")
        .tempdir_in(parent)
        .map_err(|_| VenvError::new(VenvStage::Commit, VenvFailure::Filesystem))?;
    let previous = backup.path().join("previous");
    fs::rename(destination, &previous)
        .map_err(|_| VenvError::new(VenvStage::Commit, VenvFailure::Filesystem))?;
    if let Err(error) = fs::rename(&staging.target, destination) {
        if fs::rename(&previous, destination).is_err() {
            let _preserved_previous = backup.keep();
            return Err(VenvError::new(
                VenvStage::Commit,
                VenvFailure::ConcurrentMutation,
            ));
        }
        return Err(VenvError::new(
            VenvStage::Commit,
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                VenvFailure::ConcurrentMutation
            } else {
                VenvFailure::Filesystem
            },
        ));
    }
    backup
        .close()
        .map_err(|_| VenvError::new(VenvStage::Commit, VenvFailure::Filesystem))?;
    drop(staging.container);
    Ok(())
}

async fn run_invocation(invocation: &UvInvocation, stage: VenvStage) -> Result<(), VenvError> {
    let output = run(invocation, stage).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(VenvError::new(stage, VenvFailure::UvFailed))
    }
}

async fn run_invocation_output(
    invocation: &UvInvocation,
    stage: VenvStage,
) -> Result<Vec<u8>, VenvError> {
    let output = run(invocation, stage).await?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(VenvError::new(stage, VenvFailure::UvFailed))
    }
}

async fn run(
    invocation: &UvInvocation,
    stage: VenvStage,
) -> Result<std::process::Output, VenvError> {
    let mut command = Command::new(&invocation.program);
    command
        .args(&invocation.arguments)
        .env_clear()
        .envs(invocation.environment.iter())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
        .output()
        .await
        .map_err(|_| VenvError::new(stage, VenvFailure::UvFailed))
}

#[derive(Deserialize)]
struct UvListEntry {
    name: String,
    version: String,
}

fn parse_inventory(source: &[u8]) -> Result<Vec<InstalledDistribution>, VenvError> {
    let mut distributions = serde_json::from_slice::<Vec<UvListEntry>>(source)
        .map_err(|_| VenvError::new(VenvStage::Inventory, VenvFailure::InvalidUvOutput))?
        .into_iter()
        .map(|entry| InstalledDistribution {
            name: entry.name,
            version: entry.version,
        })
        .collect::<Vec<_>>();
    distributions.sort();
    Ok(distributions)
}

fn digest_file(path: &Path) -> Result<String, VenvError> {
    fs::read(path)
        .map(|source| hex::encode(Sha256::digest(source)))
        .map_err(|_| VenvError::new(VenvStage::Fingerprint, VenvFailure::Filesystem))
}

fn direct_identity_map(
    packages: &PackageResolution,
    resolved_identity: &str,
) -> BTreeMap<String, String> {
    packages
        .packages
        .iter()
        .filter(|(_name, package)| matches!(package.selector, VersionSelector::Direct(_)))
        .map(|(name, _package)| (name.clone(), resolved_identity.to_owned()))
        .collect()
}

/// Whether preparation ran or a fully verified manifest was reused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparationDisposition {
    Prepared,
    SkippedVerified,
}

/// Successful preparation record shared by all single-flight waiters.
#[derive(Clone, Debug)]
pub struct PreparationResult {
    pub disposition: PreparationDisposition,
    pub manifest: SignedManifest,
}

/// Fail-soft preparation stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VenvStage {
    Identity,
    Lock,
    Manifest,
    Fingerprint,
    Workspace,
    Create,
    Resolve,
    ExactSync,
    Check,
    Inventory,
    Inspect,
    Revalidate,
    Commit,
}

/// Secret-safe failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VenvFailure {
    Filesystem,
    Authentication,
    EnvironmentMissing,
    InconsistentEnvironment,
    ConcurrentMutation,
    UvFailed,
    InvalidUvOutput,
    Internal,
}

/// Cloneable worker-local error shared by concurrent single-flight waiters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VenvError {
    pub stage: VenvStage,
    pub failure: VenvFailure,
}

impl VenvError {
    const fn new(stage: VenvStage, failure: VenvFailure) -> Self {
        Self { stage, failure }
    }
}

impl std::fmt::Display for VenvError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "venv preparation failed at {:?}: {:?}",
            self.stage, self.failure
        )
    }
}

impl std::error::Error for VenvError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use tempfile::TempDir;

    use super::{
        ManifestOutput, PreparationDisposition, UvExecutionContext, UvExecutor, VenvError,
        VenvFailure, VenvManager, VenvOwnership, VenvPreparationRequest, VenvStage, commit_staging,
        staging_target,
    };
    use crate::package::{ConstraintResolver, PythonWorker};
    use crate::roots::{BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent};
    use crate::uv::{ManagedUv, UvChildEnvironment};

    #[derive(Debug, Default)]
    struct FakeExecutor {
        prepares: AtomicUsize,
        output: parking_lot::Mutex<Option<ManifestOutput>>,
    }

    #[async_trait]
    impl UvExecutor for FakeExecutor {
        async fn prepare(&self, context: &UvExecutionContext) -> Result<ManifestOutput, VenvError> {
            self.prepares.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            fs::create_dir_all(&context.canonical_venv)
                .map_err(|_| VenvError::new(VenvStage::Create, VenvFailure::Filesystem))?;
            let output = ManifestOutput {
                python_build_id: context.request.python_build_id.clone(),
                distributions: vec![],
                direct_source_identities: BTreeMap::new(),
                consistent: true,
            };
            *self.output.lock() = Some(output.clone());
            Ok(output)
        }

        async fn inspect(
            &self,
            _context: &UvExecutionContext,
        ) -> Result<ManifestOutput, VenvError> {
            self.output
                .lock()
                .clone()
                .ok_or_else(|| VenvError::new(VenvStage::Inspect, VenvFailure::EnvironmentMissing))
        }

        async fn revalidate_mutable_sources(
            &self,
            _context: &UvExecutionContext,
        ) -> Result<BTreeMap<String, String>, VenvError> {
            Ok(BTreeMap::new())
        }
    }

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

    fn request(roots: &EffectiveRoots) -> VenvPreparationRequest {
        VenvPreparationRequest {
            worker: PythonWorker::Dynamic,
            venv: roots.data.join("venv-dynamic"),
            ownership: VenvOwnership::AppManaged,
            managed_uv: ManagedUv {
                path: PathBuf::from("/managed/uv"),
                build_id: "uv-test".to_owned(),
                sha256: "00".repeat(32),
            },
            python: PathBuf::from("/managed/python"),
            python_build_id: "cpython-3.14-test".to_owned(),
            packages: ConstraintResolver::resolve(PythonWorker::Dynamic, vec![], false, false)
                .expect("empty dynamic requirements must resolve"),
            override_application_constraints: false,
            override_package_metadata_constraints: false,
            uv_config: None,
            uv_environment: UvChildEnvironment::build(&RootEnvironment::from_values([(
                "PATH", "/bin",
            )]))
            .expect("uv environment must build"),
            revalidate_mutable_sources: false,
            application_build_id: "app-test".to_owned(),
            kernel_id: "kernel-test".to_owned(),
        }
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_preparation_and_manifest() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must exist");
        let executor = Arc::new(FakeExecutor::default());
        let manager = VenvManager::new(roots.clone(), executor.clone());
        let tasks = (0..16)
            .map(|_| {
                let manager = manager.clone();
                let request = request(&roots);
                tokio::spawn(async move { manager.prepare(request).await })
            })
            .collect::<Vec<_>>();
        let mut signatures = Vec::new();
        for task in tasks {
            let result = task
                .await
                .expect("task must finish")
                .expect("preparation must succeed");
            signatures.push(result.manifest.fingerprint().sha256.clone());
        }
        assert_eq!(executor.prepares.load(Ordering::SeqCst), 1);
        assert!(signatures.windows(2).all(|pair| pair[0] == pair[1]));

        let skipped = manager
            .prepare(request(&roots))
            .await
            .expect("verified repeat must succeed");
        assert_eq!(skipped.disposition, PreparationDisposition::SkippedVerified);
        assert_eq!(executor.prepares.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn corrupt_manifest_rebuilds_instead_of_being_trusted() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must exist");
        let executor = Arc::new(FakeExecutor::default());
        let manager = VenvManager::new(roots.clone(), executor.clone());
        manager
            .prepare(request(&roots))
            .await
            .expect("initial preparation must succeed");
        let manifests = roots.data.join("venv-manifests");
        let manifest = fs::read_dir(&manifests)
            .expect("manifest directory must exist")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .expect("manifest must exist");
        fs::write(manifest, b"{corrupt").expect("manifest must be corruptible");
        let rebuilt = manager
            .prepare(request(&roots))
            .await
            .expect("corrupt manifest must rebuild");
        assert_eq!(rebuilt.disposition, PreparationDisposition::Prepared);
        assert_eq!(executor.prepares.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn app_managed_rebuild_stages_and_replaces_the_complete_directory() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temporary);
        roots.ensure().expect("roots must exist");
        let destination = roots.data.join("venv-dynamic");
        fs::create_dir_all(&destination).expect("old venv must exist");
        fs::write(destination.join("old-marker"), b"old").expect("old marker must exist");
        let context = UvExecutionContext {
            request: request(&roots),
            canonical_venv: destination.clone(),
            cache_root: roots.cache.clone(),
        };
        let (target, staging) = staging_target(&context).expect("staging must be created");
        let staging = staging.expect("app-managed venv must always stage");
        fs::create_dir_all(&target).expect("new venv must exist in staging");
        fs::write(target.join("new-marker"), b"new").expect("new marker must exist");
        commit_staging(staging, &destination).expect("staging must commit");
        assert!(!destination.join("old-marker").exists());
        assert_eq!(
            fs::read(destination.join("new-marker")).expect("new marker must be readable"),
            b"new"
        );
    }
}
