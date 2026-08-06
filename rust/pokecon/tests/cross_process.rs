use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use pokecon::settings::hmac_key::HmacKey;
use pokecon::settings::lock::LockManager;
use pokecon::settings::manifest::ManifestOutput;
use pokecon::settings::package::{ConstraintResolver, PythonWorker};
use pokecon::settings::persistence::TomlStore;
use pokecon::settings::roots::{BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent};
use pokecon::settings::uv::{ManagedUv, UvChildEnvironment};
use pokecon::settings::venv::{
    PreparationDisposition, UvExecutionContext, UvExecutor, VenvError, VenvManager, VenvOwnership,
    VenvPreparationRequest,
};
use serde_json::json;
use tempfile::TempDir;

const HELPER_KIND: &str = "POKECON_CROSS_PROCESS_HELPER";
const HELPER_BASE: &str = "POKECON_CROSS_PROCESS_BASE";
const HELPER_INDEX: &str = "POKECON_CROSS_PROCESS_INDEX";

fn roots(base: &Path) -> EffectiveRoots {
    let bases = BaseDirectories::linux(&RootEnvironment::from_values([
        ("HOME", base.as_os_str().to_os_string()),
        ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
        ("XDG_DATA_HOME", base.join("data").into_os_string()),
        ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
        ("XDG_STATE_HOME", base.join("state").into_os_string()),
    ]))
    .expect("test base directories must resolve");
    EffectiveRoots::from_bases(
        SafeComponent::new("pokecon").expect("test app name must be safe"),
        &bases,
    )
}

fn helper_context(expected_kind: &str) -> Option<(PathBuf, usize)> {
    if std::env::var(HELPER_KIND).ok().as_deref() != Some(expected_kind) {
        return None;
    }
    let base = PathBuf::from(std::env::var_os(HELPER_BASE).expect("helper base must be set"));
    let index = std::env::var(HELPER_INDEX)
        .expect("helper index must be set")
        .parse()
        .expect("helper index must be an integer");
    Some((base, index))
}

fn wait_for_start(base: &Path, index: usize) {
    fs::write(base.join(format!("ready-{index}")), b"ready")
        .expect("helper readiness must be writable");
    let start = base.join("start");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !start.exists() {
        assert!(Instant::now() < deadline, "helper start gate timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn spawn_helpers(test_name: &str, kind: &str, base: &Path, count: usize) -> Vec<Child> {
    let executable = std::env::current_exe().expect("test executable must be available");
    (0..count)
        .map(|index| {
            Command::new(&executable)
                .args(["--exact", test_name, "--nocapture"])
                .env(HELPER_KIND, kind)
                .env(HELPER_BASE, base)
                .env(HELPER_INDEX, index.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("cross-process helper must start")
        })
        .collect()
}

fn release_and_join(mut children: Vec<Child>, base: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while (0..children.len()).any(|index| !base.join(format!("ready-{index}")).exists()) {
        assert!(Instant::now() < deadline, "helper readiness timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
    fs::write(base.join("start"), b"start").expect("start gate must be writable");
    for child in children.drain(..) {
        let output = child
            .wait_with_output()
            .expect("cross-process helper status must be readable");
        assert!(
            output.status.success(),
            "helper failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn hmac_process_helper() {
    let Some((base, index)) = helper_context("hmac") else {
        return;
    };
    wait_for_start(&base, index);
    let key = HmacKey::load_or_create(&base.join("manifests"))
        .expect("cross-process key creation must converge");
    fs::write(
        base.join(format!("digest-{index}")),
        key.digest(b"same input"),
    )
    .expect("helper digest must be writable");
}

#[test]
fn concurrent_processes_publish_one_hmac_key() {
    let temp = TempDir::new().expect("temporary directory must exist");
    let children = spawn_helpers("hmac_process_helper", "hmac", temp.path(), 8);
    release_and_join(children, temp.path());
    let digests = (0..8)
        .map(|index| {
            fs::read_to_string(temp.path().join(format!("digest-{index}")))
                .expect("helper digest must be readable")
        })
        .collect::<Vec<_>>();
    assert!(digests.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(
        fs::read(temp.path().join("manifests/.hmac-key"))
            .expect("winning key must be readable")
            .len(),
        32
    );
}

#[test]
fn toml_process_helper() {
    let Some((base, index)) = helper_context("toml") else {
        return;
    };
    wait_for_start(&base, index);
    let roots = roots(&base);
    roots.ensure().expect("helper roots must exist");
    TomlStore::new(LockManager::new(&roots))
        .update(
            &roots.config.join("settings.toml"),
            &[(format!("concurrent.value_{index}"), json!(index))],
        )
        .expect("cross-process TOML update must succeed");
}

#[test]
fn concurrent_processes_do_not_lose_toml_updates() {
    let temp = TempDir::new().expect("temporary directory must exist");
    let children = spawn_helpers("toml_process_helper", "toml", temp.path(), 8);
    release_and_join(children, temp.path());
    let roots = roots(temp.path());
    let document = TomlStore::new(LockManager::new(&roots))
        .read(&roots.config.join("settings.toml"))
        .expect("combined TOML must be readable");
    for index in 0..8 {
        assert_eq!(
            document
                .get(&format!("concurrent.value_{index}"))
                .and_then(toml::Value::as_integer),
            Some(index)
        );
    }
}

#[derive(Debug)]
struct ProcessExecutor {
    counter: PathBuf,
}

impl ProcessExecutor {
    fn output(context: &UvExecutionContext) -> ManifestOutput {
        ManifestOutput {
            python_build_id: context.request.python_build_id.clone(),
            distributions: Vec::new(),
            direct_source_identities: BTreeMap::new(),
            consistent: true,
        }
    }
}

#[async_trait]
impl UvExecutor for ProcessExecutor {
    async fn prepare(&self, context: &UvExecutionContext) -> Result<ManifestOutput, VenvError> {
        fs::create_dir_all(&context.canonical_venv).expect("test venv must be creatable");
        let mut counter = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.counter)
            .expect("prepare counter must be writable");
        counter
            .write_all(b"prepare\n")
            .and_then(|()| counter.sync_all())
            .expect("prepare counter must be durable");
        Ok(Self::output(context))
    }

    async fn inspect(&self, context: &UvExecutionContext) -> Result<ManifestOutput, VenvError> {
        Ok(Self::output(context))
    }

    async fn revalidate_mutable_sources(
        &self,
        _context: &UvExecutionContext,
    ) -> Result<BTreeMap<String, String>, VenvError> {
        Ok(BTreeMap::new())
    }
}

fn venv_request(roots: &EffectiveRoots) -> VenvPreparationRequest {
    VenvPreparationRequest {
        worker: PythonWorker::Dynamic,
        venv: roots.data.join("venv-dynamic"),
        ownership: VenvOwnership::AppManaged,
        managed_uv: ManagedUv {
            path: PathBuf::from("managed-uv"),
            build_id: "uv-test".to_owned(),
            sha256: "00".repeat(32),
        },
        python: PathBuf::from("managed-python"),
        python_build_id: "cpython-3.14-test".to_owned(),
        packages: ConstraintResolver::resolve(PythonWorker::Dynamic, Vec::new(), false, false)
            .expect("empty package resolution must succeed"),
        override_application_constraints: false,
        override_package_metadata_constraints: false,
        uv_config: None,
        find_links: None,
        no_index: false,
        package_source_build_id: None,
        uv_environment: UvChildEnvironment::build(&RootEnvironment::from_values([(
            "PATH",
            "test-path",
        )]))
        .expect("test uv environment must build"),
        revalidate_mutable_sources: false,
        application_build_id: "app-test".to_owned(),
        kernel_id: "kernel-test".to_owned(),
    }
}

#[test]
fn venv_process_helper() {
    let Some((base, index)) = helper_context("venv") else {
        return;
    };
    wait_for_start(&base, index);
    let roots = roots(&base);
    roots.ensure().expect("helper roots must exist");
    let executor = Arc::new(ProcessExecutor {
        counter: base.join("prepare-count"),
    });
    let manager = VenvManager::new(roots.clone(), executor);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("helper Tokio runtime must build");
    let result = runtime
        .block_on(manager.prepare(venv_request(&roots)))
        .expect("cross-process venv preparation must succeed");
    let disposition = match result.disposition {
        PreparationDisposition::Prepared => "prepared",
        PreparationDisposition::SkippedVerified => "skipped",
    };
    fs::write(base.join(format!("result-{index}")), disposition)
        .expect("helper result must be writable");
}

#[test]
fn concurrent_processes_share_one_completed_venv_manifest() {
    let temp = TempDir::new().expect("temporary directory must exist");
    let children = spawn_helpers("venv_process_helper", "venv", temp.path(), 2);
    release_and_join(children, temp.path());
    assert_eq!(
        fs::read_to_string(temp.path().join("prepare-count"))
            .expect("prepare count must be readable")
            .lines()
            .count(),
        1
    );
    let mut dispositions = (0..2)
        .map(|index| {
            fs::read_to_string(temp.path().join(format!("result-{index}")))
                .expect("helper disposition must be readable")
        })
        .collect::<Vec<_>>();
    dispositions.sort();
    assert_eq!(dispositions, ["prepared", "skipped"]);
}
