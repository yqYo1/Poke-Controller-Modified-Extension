use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[cfg(feature = "contract-generator")]
use pokecon::integration_test_support::contracts::generator::{
    check_generated_artifacts, check_openapi_artifact,
};
use pokecon::integration_test_support::contracts::model::{Access, Mutability, Scope, Setting};
use pokecon::integration_test_support::contracts::{PROTOCOL_REGISTRY_JSON, settings_registry};
use regex::Regex;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

// The root SPECIFICATION.md is only an index; contract checks search all three normative owners.
static SPECIFICATION: LazyLock<String> = LazyLock::new(|| {
    let mut documents = String::new();
    for path in [
        "docs/SPECIFICATION_BACKEND.md",
        "docs/SPECIFICATION_FRONTEND.md",
        "docs/SPECIFICATION_INTEGRATION.md",
    ] {
        documents.push_str(&repository_text(path));
        documents.push('\n');
    }
    documents
});
static ACCEPTANCE_SCHEMA: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/registry/acceptance-record.schema.json"));
static ACCEPTANCE_PROCEDURE: LazyLock<String> =
    LazyLock::new(|| repository_text("docs/ACCEPTANCE.md"));
static COMPATIBILITY_REGISTRY_JSON: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/registry/compatibility.json"));
static GENERATION_REGISTRY_JSON: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/registry/generation.json"));
static CI_REGISTRY_JSON: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/registry/ci.json"));
static CI_REGIONS: LazyLock<String> = LazyLock::new(|| repository_text("scripts/ci/regions.py"));
static CI_AGGREGATE: LazyLock<String> =
    LazyLock::new(|| repository_text("scripts/ci/aggregate.py"));
static CI_TIMING: LazyLock<String> = LazyLock::new(|| repository_text("scripts/ci/timing.py"));
static FOUNDATION_REGISTRY_JSON: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/registry/foundation.json"));
static FIXED_MANIFEST: LazyLock<String> =
    LazyLock::new(|| repository_text("compatibility/fixed-manifest.json"));
static FLAKE: LazyLock<String> = LazyLock::new(|| repository_text("flake.nix"));
static PYPROJECT: LazyLock<String> = LazyLock::new(|| repository_text("pyproject.toml"));
static CARGO_MANIFEST: LazyLock<String> = LazyLock::new(|| repository_text("Cargo.toml"));
static GITIGNORE: LazyLock<String> = LazyLock::new(|| repository_text(".gitignore"));
static WORKFLOWS: LazyLock<[(&'static str, String); 4]> = LazyLock::new(|| {
    [
        (
            "compatibility-roll",
            repository_text(".github/workflows/compatibility-roll.yml"),
        ),
        (
            "normal-ci",
            repository_text(".github/workflows/normal-ci.yml"),
        ),
        ("package", repository_text(".github/workflows/package.yml")),
        ("release", repository_text(".github/workflows/release.yml")),
    ]
});

#[cfg(feature = "contract-generator")]
#[test]
fn generated_settings_and_typing_artifacts_are_current() {
    check_generated_artifacts(repository_root())
        .expect("tracked settings and typing artifacts must match their generators");
}

#[cfg(feature = "contract-generator")]
#[test]
fn generated_openapi_artifact_is_current() {
    check_openapi_artifact(&repository_root().join("api/openapi.json"))
        .expect("tracked OpenAPI artifact must match the server schema generator");
}

#[test]
fn canonical_settings_match_every_normative_spec_projection() {
    let validated = settings_registry().expect("canonical settings registry must be valid");
    let settings = validated.settings();
    assert_eq!(settings.len(), 79);

    let registry_by_id = settings
        .iter()
        .map(|setting| (setting.id.as_str(), setting))
        .collect::<BTreeMap<_, _>>();
    let spec_rows = setting_projection_rows();
    assert_eq!(spec_rows.len(), 79);
    assert_eq!(
        registry_by_id.keys().copied().collect::<BTreeSet<_>>(),
        spec_rows
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
    );

    for (id, row) in spec_rows {
        let setting = registry_by_id
            .get(id.as_str())
            .unwrap_or_else(|| panic!("missing canonical setting {id}"));
        assert_eq!(setting.surfaces.toml.name, row.toml, "{id} TOML path");
        assert_eq!(
            setting.surfaces.dynamic.name, row.dynamic,
            "{id} dynamic path"
        );
        assert_eq!(setting.scope, row.scope, "{id} scope");
        assert_eq!(setting.mutability, row.mutability, "{id} mutability");
        assert_eq!(setting.surfaces.ui.access, row.ui, "{id} UI access");
        assert_eq!(
            setting.surfaces.openapi.access, row.openapi,
            "{id} OpenAPI access"
        );
    }

    let registry_env = settings
        .iter()
        .map(|setting| setting.surfaces.env.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(registry_env, specification_environment_names());
}

#[test]
fn protocol_names_match_the_specification() {
    let protocol = parse_json(PROTOCOL_REGISTRY_JSON);

    let rest = protocol["rest"]
        .as_array()
        .expect("protocol.rest must be an array")
        .iter()
        .map(|endpoint| {
            format!(
                "{} {}",
                string_at(endpoint, "method"),
                string_at(endpoint, "path")
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(rest, specification_rest_endpoints());

    let websocket = protocol["websocket"]["json_variants"]
        .as_array()
        .expect("websocket variants must be an array")
        .iter()
        .map(|variant| string_at(variant, "type").to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(websocket, specification_websocket_variants());

    let ipc = protocol["ipc"]["kinds"]
        .as_array()
        .expect("IPC kinds must be an array")
        .iter()
        .map(|kind| kind.as_str().expect("IPC kind must be a string"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ipc,
        BTreeSet::from(["error", "event", "log", "request", "response"])
    );
    for kind in &ipc {
        assert!(
            SPECIFICATION.contains(&format!("`\"{kind}\"`")),
            "IPC kind {kind} is not documented"
        );
    }

    let events = protocol["builtin_events"]
        .as_array()
        .expect("builtin_events must be an array")
        .iter()
        .map(|event| {
            (
                string_at(event, "name").to_owned(),
                string_at(event, "phase").to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(events, specification_builtin_events());
}

#[test]
fn public_python_and_lua_names_are_classified_without_worker_leakage() {
    let protocol = parse_json(PROTOCOL_REGISTRY_JSON);
    let surfaces = protocol["public_surfaces"]
        .as_array()
        .expect("public_surfaces must be an array");
    let user = surface_for_worker(surfaces, "user_script");
    let dynamic = surface_for_worker(surfaces, "dynamic_config");

    let dynamic_namespaces = string_set(&dynamic["namespaces"]);
    assert_eq!(
        dynamic_namespaces,
        BTreeSet::from([
            "pokecon",
            "pokecon.autocmd",
            "pokecon.commands",
            "pokecon.controller",
            "pokecon.errors",
            "pokecon.event",
            "pokecon.opt",
            "pokecon.profile",
            "pokecon.state",
        ])
    );
    assert_eq!(
        dynamic["forbidden_namespaces"],
        serde_json::json!(["Commands"])
    );
    assert_eq!(
        dynamic["settings_projection"],
        "settings.json#settings[*].surfaces.dynamic.name"
    );

    let members = dynamic["members"]
        .as_object()
        .expect("dynamic members must be an object");
    for (namespace, names) in members {
        for name in names.as_array().expect("member list must be an array") {
            let member = name.as_str().expect("member must be a string");
            let full_name = format!("{namespace}.{member}");
            assert!(
                SPECIFICATION.contains(&full_name),
                "dynamic API {full_name} is not specified"
            );
        }
    }

    assert_fixed_commands_imports_are_classified(user);
    assert_user_script_members_have_a_normative_source(user);
}

#[test]
fn compatibility_baselines_are_immutable_and_match_the_specification() {
    let compatibility = parse_json(&COMPATIBILITY_REGISTRY_JSON);
    let baselines = compatibility["fixed_baselines"]
        .as_array()
        .expect("fixed_baselines must be an array");
    assert_eq!(baselines.len(), 3);

    let actual = baselines
        .iter()
        .map(|baseline| {
            (
                string_at(baseline, "repository").to_owned(),
                string_at(baseline, "commit").to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, specification_compatibility_baselines());

    for baseline in baselines {
        let sha = string_at(baseline, "commit");
        assert_eq!(sha.len(), 40);
        assert!(sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(
            baseline["script_roots"]
                .as_array()
                .is_some_and(|roots| !roots.is_empty())
        );
        assert_eq!(baseline["expectations"]["source_mutation"], "forbidden");
    }
}

#[test]
fn fixed_compatibility_inventory_is_complete_and_content_addressed() {
    let compatibility = parse_json(&COMPATIBILITY_REGISTRY_JSON);
    let inventory = parse_json(&FIXED_MANIFEST);
    let expected = compatibility["fixed_baselines"]
        .as_array()
        .expect("fixed_baselines must be an array")
        .iter()
        .map(|baseline| {
            (
                string_at(baseline, "id").to_owned(),
                string_at(baseline, "commit").to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    let baselines = inventory["baselines"]
        .as_array()
        .expect("inventory baselines must be an array");
    let actual = baselines
        .iter()
        .map(|baseline| {
            (
                string_at(baseline, "id").to_owned(),
                string_at(baseline, "commit").to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);

    let canonical = serde_json::to_vec(&canonical_json(&Value::Array(baselines.clone())))
        .expect("inventory must serialize");
    let digest = format!("{:x}", Sha256::digest(canonical));
    assert_eq!(inventory["inventory_sha256"], digest);

    for baseline in baselines {
        let scripts = baseline["scripts"]
            .as_array()
            .expect("baseline scripts must be an array");
        assert_eq!(baseline["script_count"], scripts.len());
        assert!(!scripts.is_empty());
        let paths = scripts
            .iter()
            .map(|script| string_at(script, "path"))
            .collect::<BTreeSet<_>>();
        assert_eq!(paths.len(), scripts.len(), "script paths must be unique");
        for script in scripts {
            let sha = string_at(script, "sha256");
            assert_eq!(sha.len(), 64);
            assert!(sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert_eq!(script["expected"]["python_3_14_parse"], "success");
            assert!(script["imports"].is_array());
            assert!(script["classes"].is_array());
            assert!(script["referenced_api"].is_array());
        }
    }
}

#[test]
fn generation_and_ci_registries_define_drift_and_applicability_gates() {
    let generation = parse_json(&GENERATION_REGISTRY_JSON);
    assert_generation_registry(&generation);

    let ci = parse_json(&CI_REGISTRY_JSON);
    assert_eq!(ci["schema_version"], 3);
    assert_ci_workflow_registry(&ci);

    assert_ci_job_registry(&ci);

    assert_ci_region_registry(&ci);

    assert_ci_classification_contract(&ci);
}

#[test]
fn ci_event_registry_elects_one_canonical_sha_and_scopes_cancellation() {
    let ci = parse_json(&CI_REGISTRY_JSON);
    let event = &ci["event_contract"];
    assert_eq!(
        event["integration_branches"],
        serde_json::json!(["main", "master", "refactor/rust-core"])
    );
    assert_eq!(
        event["canonical_events"],
        serde_json::json!([
            {
                "source": "push to a configured integration branch",
                "event": "push",
            },
            {
                "source": "same-repository pull request from a non-integration branch",
                "event": "pull_request",
            },
            {
                "source": "fork pull request",
                "event": "pull_request",
            },
        ])
    );
    assert_eq!(
        event["suppressed_event"],
        serde_json::json!({
            "source": "same-repository pull request from the main or master integration branch",
            "event": "pull_request",
            "canonical_event": "push",
        })
    );
    assert_eq!(
        event["sha"],
        serde_json::json!({
            "base": {
                "push": "github.event.before",
                "pull_request": "github.event.pull_request.base.sha",
            },
            "head": {
                "push": "github.sha",
                "pull_request": "github.sha",
            },
            "pull_request_head_semantics": "GitHub pull-request merge commit SHA",
            "remote_flake_head": {
                "push": "github.sha",
                "pull_request": "github.event.pull_request.head.sha",
            },
        })
    );
    assert_eq!(event["cancel_in_progress"], true);

    let workflow_names = string_set(&event["workflows"]);
    assert_eq!(workflow_names, BTreeSet::from(["normal-ci", "package"]));
    for concurrency in event["concurrency"]
        .as_array()
        .expect("event concurrency contracts must be an array")
    {
        let workflow = string_at(concurrency, "workflow");
        let source = WORKFLOWS
            .iter()
            .find_map(|(name, source)| (*name == workflow).then_some(source.as_str()))
            .unwrap_or_else(|| panic!("event workflow {workflow} must exist"));
        assert!(source.contains(&format!("    {}", string_at(concurrency, "group"))));
        assert!(source.contains("  cancel-in-progress: true"));
        assert!(source.contains("branches: [main, master, refactor/rust-core]"));
        assert!(
            source
                .contains("github.event.pull_request.head.repo.full_name != github.repository ||")
        );
        for branch in ["main", "master"] {
            assert!(source.contains(&format!("github.head_ref != '{branch}'")));
        }
        assert!(source.contains(
            "${{ github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.event.before }}"
        ));
        assert!(source.contains("HEAD_SHA: >-\n            ${{ github.sha }}"));
        if workflow == "normal-ci" {
            assert!(source.contains(
                "github.event_name == 'pull_request' && github.event.pull_request.head.sha || github.sha"
            ));
        } else {
            assert!(!source.contains("github.event.pull_request.head.sha"));
        }

        let elected_jobs = event["elected_jobs"][workflow]
            .as_array()
            .expect("elected jobs must be an array");
        assert_eq!(
            source.matches("github.event_name == 'push' ||").count(),
            elected_jobs.len()
        );
        for job in elected_jobs {
            let job = job.as_str().expect("elected job must be a string");
            assert!(source.contains(&format!("  {job}:\n")));
        }
    }
}

#[test]
fn ci_aggregate_registry_matches_both_required_workflow_gates() {
    let ci = parse_json(&CI_REGISTRY_JSON);
    let jobs = ci["jobs"].as_array().expect("CI jobs must be an array");
    let aggregate = &ci["aggregate_contract"];
    assert_ci_aggregate_schema(aggregate);
    assert_ci_aggregate_implementation(aggregate);

    assert_ci_aggregate_workflow_contracts(aggregate, jobs);
}

#[allow(clippy::too_many_lines)]
#[test]
fn ci_cache_and_timing_registry_tracks_implemented_boundaries() {
    let ci = parse_json(&CI_REGISTRY_JSON);
    let cache = &ci["binary_cache_contract"];
    assert_eq!(cache["scope"], "PokeCon-specific Nix derivations");
    assert_eq!(
        cache["desired_permissions"],
        serde_json::json!([
            {
                "event": "pull_request",
                "actor": "any pull-request actor",
                "read": true,
                "write": false,
            },
            {
                "event": "push",
                "actor": "trusted repository writer",
                "read": true,
                "write": true,
            },
        ])
    );
    assert_eq!(
        cache["current_implementation"]["status"],
        "signed_file_cache_optional"
    );
    assert_eq!(
        cache["current_implementation"]["pokecon_specific_read"],
        true
    );
    assert_eq!(
        cache["current_implementation"]["pokecon_specific_write"],
        true
    );
    assert_eq!(
        cache["current_implementation"]["validator_enforces_push_only_writes"],
        true
    );
    assert_eq!(
        cache["current_implementation"]["validator_enforces_trusted_actor_allowlist"],
        true
    );
    assert!(CI_TIMING.contains("if report.cache.write and report.cache.event != \"push\""));
    assert!(CI_TIMING.contains("TRUSTED_CACHE_WRITERS: Final = frozenset({\"yqYo1\"})"));
    assert!(CI_TIMING.contains("report.cache.actor not in TRUSTED_CACHE_WRITERS"));
    let normal_ci = WORKFLOWS
        .iter()
        .find_map(|(name, source)| (*name == "normal-ci").then_some(source.as_str()))
        .expect("Normal CI workflow must exist");
    assert_eq!(
        normal_ci
            .matches("uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9")
            .count(),
        6
    );
    assert_eq!(
        normal_ci
            .matches("uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9")
            .count(),
        3
    );
    assert_eq!(
        normal_ci
            .matches("contains(fromJSON('[\"yqYo1\"]'), github.actor)")
            .count(),
        6
    );
    assert_eq!(normal_ci.matches("github.event_name == 'push'").count(), 14);
    let normal_source = WORKFLOWS
        .iter()
        .find_map(|(name, source)| (*name == "normal-ci").then_some(source.as_str()))
        .expect("Normal CI workflow must exist");
    assert!(normal_source.contains(
        "uses: cachix/install-nix-action@13d8dd58da0234aa297dedd986986ccb8e7f3e24 # v31"
    ));
    assert!(!normal_source.contains("uses: cachix/install-nix-action@v31"));
    assert!(!normal_source.contains("uses: cachix/cachix-action@"));
    let package_source = WORKFLOWS
        .iter()
        .find_map(|(name, source)| (*name == "package").then_some(source.as_str()))
        .expect("Package workflow must exist");
    assert!(package_source.contains("uses: cachix/install-nix-action@v31"));
    assert!(!package_source.contains("uses: cachix/cachix-action@"));

    let timing = &ci["timing_contract"];
    assert_eq!(timing["schema_version"], 2);
    assert_eq!(timing["command"], "nix run .#ci-timing --");
    assert_eq!(
        timing["change_kind_threshold_seconds"],
        serde_json::json!({"fast": 180, "docs": 300, "product": 720})
    );
    assert_eq!(timing["p95"]["method"], "nearest-rank");
    assert_eq!(timing["p95"]["metric"], "critical_path_wall_seconds");
    assert_eq!(timing["p95"]["minimum_same_kind_samples"], 10);
    assert_eq!(
        timing["p95"]["history_scope"],
        serde_json::json!([
            "same workflow event",
            "same head branch",
            "same pull request number for pull_request events",
            "one latest completed run per head SHA",
            "current revision excluded",
        ])
    );
    assert_eq!(timing["current_implementation"]["validator"], "implemented");
    assert_eq!(
        timing["current_implementation"]["workflow_evidence_collection"],
        "implemented_with_upstream_completed_max"
    );
    assert_eq!(
        timing["current_implementation"]["workflow_p95_gate"],
        "blocking_for_fail_closed"
    );
    assert!(CI_TIMING.contains("ChangeKind.FAST: 180.0"));
    assert!(CI_TIMING.contains("ChangeKind.DOCS: 300.0"));
    assert!(CI_TIMING.contains("ChangeKind.PRODUCT: 720.0"));
    assert!(CI_TIMING.contains("MINIMUM_P95_SAMPLES: Final = 10"));
    assert!(CI_TIMING.contains("COLLECTION_KIND_UPSTREAM_COMPLETED_MAX"));
    assert!(CI_TIMING.contains("critical_path_wall_seconds"));
    assert!(CI_TIMING.contains("upstream_completed_max"));
    let normal_ci = WORKFLOWS
        .iter()
        .find_map(|(workflow, source)| (*workflow == "normal-ci").then_some(source))
        .expect("normal-ci workflow must exist");
    assert!(normal_ci.contains("nix run .#ci-timing -- collect"));
}

fn assert_generation_registry(generation: &Value) {
    assert_eq!(
        generation["generate_command"],
        "nix run .#generate-contracts"
    );
    assert_eq!(
        generation["drift_check_command"],
        "nix run .#contract-check"
    );
    let artifacts = generation["artifacts"]
        .as_array()
        .expect("generation artifacts must be an array");
    let outputs = artifacts
        .iter()
        .map(|artifact| string_at(artifact, "output"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        outputs.len(),
        artifacts.len(),
        "generated outputs must be unique"
    );
    for artifact in artifacts {
        assert!(string_at(artifact, "generate_command").starts_with("nix "));
        assert!(artifact["tracked"].is_boolean());
    }
    assert_generated_artifact_contracts(artifacts);
}

fn assert_ci_workflow_registry(ci: &Value) {
    let workflows = ci["workflows"]
        .as_array()
        .expect("CI workflows must be an array");
    let registered_workflows = workflows
        .iter()
        .map(|workflow| string_at(workflow, "id"))
        .collect::<BTreeSet<_>>();
    let active_workflows = WORKFLOWS
        .iter()
        .map(|(workflow, _)| *workflow)
        .collect::<BTreeSet<_>>();
    assert_eq!(registered_workflows, active_workflows);
    for workflow in workflows {
        let id = string_at(workflow, "id");
        assert_eq!(
            string_at(workflow, "file"),
            format!(".github/workflows/{id}.yml")
        );
        let source = WORKFLOWS
            .iter()
            .find_map(|(workflow, source)| (*workflow == id).then_some(source.as_str()))
            .unwrap_or_else(|| panic!("registered workflow {id} must be embedded"));
        assert!(
            source.starts_with(&format!("name: {}\n", string_at(workflow, "display_name"))),
            "workflow display name must match {id}"
        );
    }
}

fn assert_ci_job_registry(ci: &Value) {
    let jobs = ci["jobs"].as_array().expect("CI jobs must be an array");
    let jobs_by_name = jobs
        .iter()
        .map(|job| (string_at(job, "name"), job))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        jobs_by_name.len(),
        jobs.len(),
        "CI job names must be unique"
    );
    assert_eq!(
        jobs_by_name
            .keys()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        workflow_job_names()
    );

    let regions = ci["regions"]
        .as_array()
        .expect("CI regions must be an array");
    let region_names = regions
        .iter()
        .map(|region| string_at(region, "name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        region_names,
        BTreeSet::from([
            "contracts",
            "docs",
            "product",
            "python",
            "remote_flake",
            "routing",
            "rust",
            "web",
        ])
    );

    for job in jobs {
        let workflow = string_at(job, "workflow");
        let job_id = string_at(job, "job");
        assert_eq!(string_at(job, "name"), format!("{workflow}/{job_id}"));
        let command = string_at(job, "command");
        let windows_native = job["execution_environment"] == "windows-native";
        // The same-SHA target upload job is a GitHub-managed cache/artifacts
        // transfer with no build to run: it must not invoke Nix, so it is the
        // single documented exception to the Nix-command rule.
        let transfer_only = string_at(job, "name") == "normal-ci/production_perf_target_upload";
        assert!(
            command.starts_with("nix ")
                || (windows_native && command.starts_with("cargo "))
                || (transfer_only && command.starts_with("actions/cache/restore")),
            "CI commands must use Nix except for explicit Windows-native jobs: {command}"
        );
        assert!(
            job["owns"]
                .as_array()
                .is_some_and(|ownership| !ownership.is_empty()),
            "CI job {} must own at least one check",
            string_at(job, "name")
        );
        assert!(matches!(
            string_at(job, "aggregate_role"),
            "none" | "plan" | "input" | "required_gate"
        ));
        for region in job["selected_by_regions"]
            .as_array()
            .expect("selected_by_regions must be an array")
        {
            let region = region
                .as_str()
                .expect("selected region names must be strings");
            assert!(
                region_names.contains(region),
                "job {} selects unknown region {region}",
                string_at(job, "name")
            );
        }
        assert!(!string_at(job, "applicable_when").is_empty());
        assert!(!string_at(job, "not_applicable").is_empty());
        assert!(
            job["phase"]
                .as_u64()
                .is_some_and(|phase| (1..=15).contains(&phase))
        );
    }
}

fn assert_ci_region_registry(ci: &Value) {
    assert_ci_region_exports_and_owners(ci);
    assert_ci_region_ownership_matrix(ci);
}

fn assert_ci_region_exports_and_owners(ci: &Value) {
    let regions = ci["regions"]
        .as_array()
        .expect("CI regions must be an array");
    let jobs = ci["jobs"].as_array().expect("CI jobs must be an array");
    let jobs_by_name = jobs
        .iter()
        .map(|job| (string_at(job, "name"), job))
        .collect::<BTreeMap<_, _>>();

    for region in regions {
        let name = string_at(region, "name");
        let enum_name = name.to_ascii_uppercase();
        assert!(
            CI_REGIONS.contains(&format!("    {enum_name} = \"{name}\"")),
            "region {name} must be exported by ci-regions"
        );
        for (_, workflow) in WORKFLOWS
            .iter()
            .filter(|(workflow, _)| matches!(*workflow, "normal-ci" | "package"))
        {
            assert!(
                workflow.contains(&format!(
                    "      {name}: ${{{{ steps.regions.outputs.{name} }}}}"
                )),
                "workflow must export region {name}"
            );
        }
        for owner_key in ["normal_ci_owner_jobs", "package_ci_owner_jobs"] {
            for owner in region[owner_key]
                .as_array()
                .expect("region owner jobs must be an array")
            {
                let owner = owner.as_str().expect("region owner job must be a string");
                assert!(
                    jobs_by_name.contains_key(owner),
                    "region {name} references unknown owner {owner}"
                );
            }
        }
    }
}

fn assert_ci_region_ownership_matrix(ci: &Value) {
    assert_eq!(
        ci["regions"],
        serde_json::json!([
            {
                "name": "docs",
                "normal_ci_owner_jobs": ["normal-ci/fast"],
                "package_ci_owner_jobs": [],
            },
            {
                "name": "contracts",
                "normal_ci_owner_jobs": ["normal-ci/rust_contracts"],
                "package_ci_owner_jobs": [],
            },
            {
                "name": "rust",
                "normal_ci_owner_jobs": [
                    "normal-ci/rust_contracts",
                    "normal-ci/rust_clippy",
                    "normal-ci/windows"
                ],
                "package_ci_owner_jobs": [],
            },
            {
                "name": "python",
                "normal_ci_owner_jobs": ["normal-ci/python_tests"],
                "package_ci_owner_jobs": [],
            },
            {
                "name": "routing",
                "normal_ci_owner_jobs": ["normal-ci/routing_mutations"],
                "package_ci_owner_jobs": [],
            },
            {
                "name": "web",
                "normal_ci_owner_jobs": ["normal-ci/web"],
                "package_ci_owner_jobs": [],
            },
            {
                "name": "product",
                "normal_ci_owner_jobs": ["normal-ci/product_flake", "normal-ci/performance"],
                "package_ci_owner_jobs": [
                    "package/linux",
                    "package/linux_repro",
                    "package/repro_check",
                    "package/windows",
                    "package/windows_repro",
                    "package/windows_repro_check",
                ],
            },
            {
                "name": "remote_flake",
                "normal_ci_owner_jobs": ["normal-ci/remote_flake"],
                "package_ci_owner_jobs": [],
            },
        ])
    );
}

fn assert_ci_classification_contract(ci: &Value) {
    let classification = &ci["classification_contract"];
    let region_names = ci["regions"]
        .as_array()
        .expect("CI regions must be an array")
        .iter()
        .map(|region| string_at(region, "name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(classification["schema_version"], 1);
    assert_eq!(classification["command"], "nix run .#ci-regions --");
    assert_eq!(string_set(&classification["region_names"]), region_names);
    assert_eq!(
        classification["github_outputs"]["structured"],
        "regions_json"
    );
    let mut expected_scalars = string_set(&classification["region_names"]);
    expected_scalars.insert("performance_baseline");
    assert_eq!(
        string_set(&classification["github_outputs"]["scalars"]),
        expected_scalars
    );
    assert_eq!(
        classification["github_outputs"]["scalar_values"],
        serde_json::json!(["false", "true"])
    );
    assert!(
        CI_REGIONS
            .contains("f\"{region.value}={str(region in classification.applicable).lower()}\"")
    );
    assert!(CI_REGIONS.contains("f\"regions_json={regions_json}\""));
    assert!(CI_REGIONS.contains("f\"performance_baseline="));

    assert_ci_fail_closed_classification(classification);
    assert_ci_compatibility_classification(classification);
    assert_ci_routing_classification(classification);
    assert_ci_workflow_region_outputs(classification);
}

fn assert_ci_fail_closed_classification(classification: &Value) {
    let fail_closed = &classification["fail_closed"];
    assert_eq!(
        string_set(&fail_closed["selected_regions"]),
        string_set(&classification["region_names"])
    );
    for path in fail_closed["paths"]
        .as_array()
        .expect("fail-closed paths must be an array")
    {
        let path = path.as_str().expect("fail-closed path must be a string");
        assert!(
            CI_REGIONS.contains(&format!("        \"{path}\",")),
            "ci-regions must fail closed for {path}"
        );
    }
    for prefix in fail_closed["prefixes"]
        .as_array()
        .expect("fail-closed prefixes must be an array")
    {
        let prefix = prefix
            .as_str()
            .expect("fail-closed prefix must be a string");
        assert!(
            CI_REGIONS.contains(&format!("\"{prefix}\"")),
            "ci-regions must fail closed below {prefix}"
        );
    }
}

fn assert_ci_compatibility_classification(classification: &Value) {
    let compatibility = &classification["compatibility"];
    assert_eq!(
        compatibility["forced_regions"],
        serde_json::json!(["contracts", "rust", "product"])
    );
    assert_eq!(compatibility["python_sources_also_select"], "python");
    for prefix in compatibility["prefixes"]
        .as_array()
        .expect("compatibility prefixes must be an array")
    {
        let prefix = prefix
            .as_str()
            .expect("compatibility prefix must be a string");
        assert!(CI_REGIONS.contains(&format!("    \"{prefix}\",")));
    }
    assert!(CI_REGIONS.contains("regions.update((Region.RUST, Region.PRODUCT))"));

    for path in classification["openapi_contract_paths"]
        .as_array()
        .expect("OpenAPI contract paths must be an array")
    {
        let path = path.as_str().expect("OpenAPI path must be a string");
        assert!(
            CI_REGIONS.contains(&format!("        \"{path}\",")),
            "ci-regions must classify exact OpenAPI path {path}"
        );
    }
}

fn assert_ci_routing_classification(classification: &Value) {
    let routing = &classification["routing_audit"];
    for path in routing["direct_paths"]
        .as_array()
        .expect("routing direct paths must be an array")
    {
        let path = path.as_str().expect("routing direct path must be a string");
        assert!(
            CI_REGIONS.contains(&format!("        \"{path}\",")),
            "ci-regions must classify direct routing input {path}"
        );
    }
    assert_eq!(routing["rust_source_prefix"], "rust/pokecon/src/");
    assert_eq!(routing["rust_source_suffix"], ".rs");
    assert!(CI_REGIONS.contains("ROUTING_RUST_SOURCE_PREFIX"));
    assert!(CI_REGIONS.contains("path.endswith(\".rs\")"));
    for prefix in routing["acl_prefixes"]
        .as_array()
        .expect("routing ACL prefixes must be an array")
    {
        let prefix = prefix
            .as_str()
            .expect("routing ACL prefix must be a string");
        assert!(CI_REGIONS.contains(&format!("    \"{prefix}\",")));
    }
    for prefix in routing["excluded_prefixes"]
        .as_array()
        .expect("routing excluded prefixes must be an array")
    {
        let prefix = prefix
            .as_str()
            .expect("routing excluded prefix must be a string");
        assert!(CI_REGIONS.contains(&format!("\"{prefix}\"")));
    }
    assert_eq!(routing["tauri_config_parent"], "rust/pokecon");
    assert_eq!(
        routing["tauri_config_pattern"],
        r"(?:tauri(?:\.[^.]+)?\.conf\.(?:json|json5)|Tauri(?:\.[^.]+)?\.toml)"
    );
    assert!(CI_REGIONS.contains("ROUTING_TAURI_CONFIG_PATTERN.fullmatch"));
    for name in routing["cargo_config_names"]
        .as_array()
        .expect("routing Cargo config names must be an array")
    {
        let name = name
            .as_str()
            .expect("routing Cargo config name must be a string");
        assert!(CI_REGIONS.contains(&format!("\"{name}\"")));
    }
    for part in routing["cargo_config_excluded_parts"]
        .as_array()
        .expect("routing Cargo config exclusions must be an array")
    {
        let part = part
            .as_str()
            .expect("routing Cargo config exclusion must be a string");
        assert!(CI_REGIONS.contains(&format!("        \"{part}\",")));
    }
    assert_eq!(
        routing["uncertain_ci_control_policy"],
        "select every region"
    );
    assert!(CI_REGIONS.contains("if is_routing_path(path):"));
    assert!(CI_REGIONS.contains("regions.add(Region.ROUTING)"));
}

fn assert_ci_workflow_region_outputs(classification: &Value) {
    for (_, workflow) in WORKFLOWS
        .iter()
        .filter(|(workflow, _)| matches!(*workflow, "normal-ci" | "package"))
    {
        assert!(workflow.contains("      regions_json: ${{ steps.regions.outputs.regions_json }}"));
        assert!(workflow.contains("\"regions\":${{ needs.plan.outputs.regions_json || 'null' }}"));
        for region in string_set(&classification["region_names"]) {
            assert!(workflow.contains(&format!(
                "\"{region}\":\"${{{{ needs.plan.outputs.{region} }}}}\""
            )));
        }
    }
}

fn assert_ci_aggregate_schema(aggregate: &Value) {
    assert_eq!(aggregate["command"], "nix run .#ci-aggregate --");
    assert_eq!(
        aggregate["plan_keys"],
        serde_json::json!(["jobs", "plan_status", "region_outputs", "regions"])
    );
    assert_eq!(
        aggregate["planned_job_keys"],
        serde_json::json!(["applicable", "reason"])
    );
    assert_eq!(
        aggregate["region_keys"],
        serde_json::json!([
            "docs",
            "contracts",
            "rust",
            "python",
            "routing",
            "web",
            "product",
            "remote_flake",
        ])
    );
    assert_eq!(aggregate["region_value_type"], "boolean");
    assert_eq!(
        aggregate["region_output_values"],
        serde_json::json!(["false", "true"])
    );
    assert_eq!(aggregate["region_outputs_must_match_regions"], true);
    assert_eq!(
        aggregate["report_keys"],
        serde_json::json!([
            "conclusion",
            "extra_results",
            "jobs",
            "plan_status",
            "region_outputs",
            "regions",
            "violations",
        ])
    );
    assert_eq!(
        aggregate["job_report_keys"],
        serde_json::json!(["applicable", "expected_result", "name", "reason", "result",])
    );
    assert_eq!(
        aggregate["canonical_serialization"],
        serde_json::json!({
            "sort_keys": true,
            "ensure_ascii": false,
            "separators": [",", ":"],
        })
    );
    assert_eq!(
        aggregate["accepted_results"],
        serde_json::json!(["success", "skipped", "failure", "cancelled", "timed_out",])
    );
    assert_eq!(aggregate["required_plan_result"], "success");
    assert_eq!(aggregate["applicable_job_result"], "success");
    assert_eq!(aggregate["inapplicable_job_result"], "skipped");
    assert_eq!(aggregate["missing_results_are_errors"], true);
    assert_eq!(aggregate["extra_results_are_errors"], true);
}

fn assert_ci_aggregate_implementation(aggregate: &Value) {
    assert!(CI_AGGREGATE.contains(
        "PLAN_KEYS: Final = frozenset({\"jobs\", \"plan_status\", \"region_outputs\", \"regions\"})"
    ));
    assert!(CI_AGGREGATE.contains("REGION_KEYS: Final = frozenset(REGION_NAMES)"));
    assert!(CI_AGGREGATE.contains("\"region_outputs\": self.region_outputs"));
    assert!(CI_AGGREGATE.contains("\"regions\": self.regions"));
    assert!(CI_AGGREGATE.contains("ensure_ascii=False"));
    assert!(CI_AGGREGATE.contains("separators=(\",\", \":\")"));
    assert!(CI_AGGREGATE.contains("sort_keys=True"));
    for region in string_set(&aggregate["region_keys"]) {
        assert!(CI_AGGREGATE.contains(&format!("    \"{region}\",")));
    }
    for result in string_set(&aggregate["accepted_results"]) {
        assert!(
            CI_AGGREGATE.contains(&format!("= \"{result}\"")),
            "aggregate result {result} must be implemented"
        );
    }
}

fn assert_ci_aggregate_workflow_contracts(aggregate: &Value, jobs: &[Value]) {
    let workflow_contracts = aggregate["workflows"]
        .as_array()
        .expect("aggregate workflows must be an array");
    assert_eq!(workflow_contracts.len(), 2);
    for contract in workflow_contracts {
        let workflow = string_at(contract, "workflow");
        let source = WORKFLOWS
            .iter()
            .find_map(|(name, source)| (*name == workflow).then_some(source.as_str()))
            .unwrap_or_else(|| panic!("aggregate workflow {workflow} must exist"));
        let expected_inputs = jobs
            .iter()
            .filter(|job| job["workflow"] == workflow && job["aggregate_role"] == "input")
            .map(|job| string_at(job, "name"))
            .collect::<BTreeSet<_>>();
        assert_eq!(string_set(&contract["input_jobs"]), expected_inputs);

        let plan_job = string_at(contract, "plan_job");
        let required_job = string_at(contract, "required_job");
        let plan = jobs
            .iter()
            .find(|job| job["name"] == plan_job)
            .unwrap_or_else(|| panic!("aggregate plan job {plan_job} must exist"));
        let required = jobs
            .iter()
            .find(|job| job["name"] == required_job)
            .unwrap_or_else(|| panic!("aggregate required job {required_job} must exist"));
        assert_eq!(plan["aggregate_role"], "plan");
        assert_eq!(required["aggregate_role"], "required_gate");
        assert!(source.contains(&format!(
            "    name: {}",
            string_at(contract, "required_context")
        )));
        assert!(source.contains("\"plan_status\":\"${{ needs.plan.result }}\""));
        assert!(source.contains("\"regions\":${{ needs.plan.outputs.regions_json || 'null' }}"));
        assert!(source.contains("\"region_outputs\":{"));
        for region in string_set(&aggregate["region_keys"]) {
            assert!(source.contains(&format!(
                "\"{region}\":\"${{{{ needs.plan.outputs.{region} }}}}\""
            )));
        }
        for input in contract["input_jobs"].as_array().expect("input jobs") {
            let input = input.as_str().expect("aggregate input must be a string");
            let (_, job_id) = input
                .split_once('/')
                .expect("aggregate input must be workflow-qualified");
            assert!(
                source.contains(&format!("{job_id}=${{{{ needs.{job_id}.result }}}}")),
                "workflow {workflow} must pass result for {job_id}"
            );
        }
    }
}

fn assert_generated_artifact_contracts(artifacts: &[Value]) {
    assert_eq!(
        artifact_for_name(artifacts, "typescript_api")["output"],
        "web/src/lib/api/openapi.ts"
    );
    assert!(SPECIFICATION.contains("src/lib/api/openapi.ts"));
    assert_eq!(
        artifact_for_name(artifacts, "dynamic_python_typings")["output"],
        "python/pokecon/typings/__init__.pyi"
    );
    assert_eq!(
        artifact_for_name(artifacts, "commands_python_typings")["output"],
        "python/pokecon/typings/commands.pyi"
    );
    assert_eq!(
        artifact_for_name(artifacts, "commands_python_package_typings")["output"],
        "python/pokecon/typings/Commands/"
    );
    assert_eq!(
        artifact_for_name(artifacts, "settings_json_schema")["output"],
        "generated/settings.schema.json"
    );
    assert_eq!(
        artifact_for_name(artifacts, "settings_ui_metadata")["output"],
        "generated/settings-ui.json"
    );
    assert_eq!(
        artifact_for_name(artifacts, "openapi")["output"],
        "api/openapi.json"
    );
    assert_eq!(artifact_for_name(artifacts, "openapi")["tracked"], true);
    assert_eq!(
        artifact_for_name(artifacts, "web_openapi_schema")["output"],
        "web/src/lib/api/openapi.json"
    );

    let repository = repository_root();
    for artifact in artifacts
        .iter()
        .filter(|artifact| artifact["tracked"] == true)
    {
        let output = string_at(artifact, "output");
        let path = repository.join(output.trim_end_matches('/'));
        if output.ends_with('/') {
            assert!(
                path.is_dir(),
                "tracked output directory is missing: {output}"
            );
            assert!(
                path.read_dir()
                    .expect("tracked output directory must be readable")
                    .next()
                    .is_some(),
                "tracked output directory is empty: {output}"
            );
        } else {
            assert!(path.is_file(), "tracked output file is missing: {output}");
        }
    }
}

fn repository_text(relative_path: &str) -> String {
    fs::read_to_string(repository_root().join(relative_path))
        .unwrap_or_else(|error| panic!("contract input {relative_path} must be readable: {error}"))
}

fn repository_root() -> &'static Path {
    static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
        let candidate = env::var_os("POKECON_CONTRACT_TEST_ROOT").map_or_else(
            || {
                env::current_dir()
                    .expect("contract test current directory must be readable")
                    .ancestors()
                    .find(|ancestor| {
                        ancestor.join("Cargo.toml").is_file()
                            && ancestor.join("SPECIFICATION.md").is_file()
                    })
                    .expect("contract tests must run below the repository root")
                    .to_path_buf()
            },
            PathBuf::from,
        );
        assert!(
            candidate.is_absolute(),
            "contract test root must be absolute"
        );
        let canonical = candidate
            .canonicalize()
            .expect("contract test root must resolve to an existing directory");
        assert!(
            canonical.join("Cargo.toml").is_file() && canonical.join("SPECIFICATION.md").is_file(),
            "contract test root must contain Cargo.toml and SPECIFICATION.md"
        );
        canonical
    });
    ROOT.as_path()
}

#[test]
fn verification_taxonomy_is_complete() {
    let foundation = parse_json(&FOUNDATION_REGISTRY_JSON);
    let categories = foundation["test_categories"]
        .as_array()
        .expect("test_categories must be an array");
    let category_ids = categories
        .iter()
        .map(|category| string_at(category, "id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        category_ids,
        BTreeSet::from([
            "compatibility",
            "contract",
            "fault_injection",
            "hardware",
            "integration",
            "unit",
        ])
    );
    for category in categories {
        assert!(string_at(category, "fixture_pattern").starts_with("tests/fixtures/"));
        assert!(
            category["entrypoint_patterns"]
                .as_array()
                .is_some_and(|patterns| !patterns.is_empty())
        );
    }
    let hardware = categories
        .iter()
        .find(|category| category["id"] == "hardware")
        .expect("hardware category must exist");
    assert_eq!(
        hardware["record_schema"],
        "rust/pokecon/registry/acceptance-record.schema.json"
    );
    assert_eq!(hardware["procedure"], "docs/ACCEPTANCE.md");
    assert_eq!(
        foundation["fixture_naming"]["segment_regex"],
        "^[a-z][a-z0-9_]*$"
    );
}

#[test]
fn future_path_audit_is_complete() {
    let foundation = parse_json(&FOUNDATION_REGISTRY_JSON);
    let audit = foundation["path_audit"]
        .as_array()
        .expect("path_audit must be an array");
    let ids = audit
        .iter()
        .map(|entry| string_at(entry, "id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), audit.len(), "path audit IDs must be unique");
    for expected in [
        "rust_workspace",
        "python_package",
        "legacy_python_native_extension",
        "web_package",
        "python_tests",
        "legacy_src_server",
        "legacy_typescript_output",
        "legacy_release_crates",
        "generated_contracts",
        "external_acceptance",
    ] {
        assert!(
            ids.contains(expected),
            "missing path audit entry {expected}"
        );
    }

    let mut legacy_sources = format!(
        "{}\n{}\n{}\n{}",
        FLAKE.as_str(),
        PYPROJECT.as_str(),
        CARGO_MANIFEST.as_str(),
        GITIGNORE.as_str()
    );
    for (_, source) in WORKFLOWS.iter() {
        legacy_sources.push('\n');
        legacy_sources.push_str(source);
    }
    for entry in audit.iter().filter(|entry| entry["status"] == "obsolete") {
        assert!(entry["phase"].as_u64().is_some());
        assert!(!string_at(entry, "resolution").is_empty());
        for path in entry["paths"]
            .as_array()
            .expect("audited paths must be an array")
        {
            let path = path.as_str().expect("audited path must be a string");
            assert!(
                legacy_sources.contains(path),
                "obsolete path marker {path} is not present in an observed manifest/workflow"
            );
        }
    }
    for entry in audit.iter().filter(|entry| entry["status"] == "resolved") {
        assert!(entry["phase"].as_u64().is_some());
        assert!(!string_at(entry, "resolution").is_empty());
        for path in entry["paths"]
            .as_array()
            .expect("audited paths must be an array")
        {
            let path = path.as_str().expect("audited path must be a string");
            assert!(
                !legacy_sources.contains(path),
                "resolved path marker {path} remains in an active manifest or workflow"
            );
            let is_legacy_release_crate = string_at(entry, "id") == "legacy_release_crates";
            if (path.contains('/') || is_legacy_release_crate) && !path.contains('*') {
                let resolved_path = repository_root().join(path);
                assert!(
                    !resolved_path.exists() && !resolved_path.is_symlink(),
                    "resolved literal path still exists in the repository: {path}"
                );
            }
        }
    }
}

#[test]
fn external_acceptance_contract_closes_steps_and_release_matrix() {
    let schema = parse_json(&ACCEPTANCE_SCHEMA);
    let matrix = &schema["x-pokecon-release-matrix"];
    assert_eq!(matrix["specification_version"], "2.2.0");
    assert_eq!(matrix["platforms"], serde_json::json!(["linux", "windows"]));
    assert_eq!(
        matrix["browsers"],
        serde_json::json!(["chrome", "edge", "firefox", "safari"])
    );
    let capabilities = matrix["capabilities"]
        .as_array()
        .expect("acceptance capabilities must be an array");
    let capability_ids = capabilities
        .iter()
        .map(|capability| string_at(capability, "id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        capability_ids,
        BTreeSet::from([
            "audio_input_device",
            "browser_matrix",
            "capture_device_with_known_frame_fixture",
            "credentialed_network_notification_endpoints",
            "desktop_lifecycle",
            "integrated_load_stress",
            "mcu_serial_device_and_target_console",
            "performance",
            "security_acceptance",
        ])
    );
    for capability in capabilities {
        let capability_id = string_at(capability, "id");
        let expected_release_required = capability_id != "performance";
        assert_eq!(
            capability["release_required"], expected_release_required,
            "only performance is CI-only"
        );
        let steps = capability["required_steps"]
            .as_array()
            .expect("acceptance capability steps must be an array");
        let step_ids = steps
            .iter()
            .map(|step| step.as_str().expect("acceptance step must be a string"))
            .collect::<Vec<_>>();
        let unique_steps = step_ids.iter().copied().collect::<BTreeSet<_>>();
        assert!(!steps.is_empty());
        assert_eq!(steps.len(), unique_steps.len());
        for step in step_ids {
            assert!(ACCEPTANCE_PROCEDURE.contains(&format!("`{step}`")));
        }
    }
    assert!(FLAKE.contains("python -m scripts.acceptance.records"));
    assert!(FLAKE.contains("pkgs.check-jsonschema"));
}

#[derive(Debug)]
struct ProjectionRow {
    toml: Option<String>,
    dynamic: Option<String>,
    scope: Scope,
    mutability: Mutability,
    ui: Access,
    openapi: Access,
}

fn setting_projection_rows() -> BTreeMap<String, ProjectionRow> {
    let section = between(&SPECIFICATION, "#### 11.4.2", "**注**");
    let mut rows = BTreeMap::new();
    for line in section.lines().filter(|line| line.starts_with("| `")) {
        let columns = split_markdown_row(line);
        assert_eq!(columns.len(), 10, "unexpected setting table row: {line}");
        let ids = expand_projection_cell(&columns[0]);
        let toml_keys = expand_projection_cell(&columns[2]);
        let dynamic_paths = expand_projection_cell(&columns[3]);
        assert_eq!(ids.len(), toml_keys.len());
        assert_eq!(ids.len(), dynamic_paths.len());

        for ((id, toml_key), dynamic_path) in ids.into_iter().zip(toml_keys).zip(dynamic_paths) {
            let toml = if columns[1] == "—" {
                None
            } else {
                Some(format!(
                    "{}.{}",
                    columns[1].trim_matches(['[', ']']),
                    toml_key
                ))
            };
            let dynamic = (dynamic_path != "—").then_some(dynamic_path);
            let row = ProjectionRow {
                toml,
                dynamic,
                scope: parse_scope(&columns[5]),
                mutability: parse_mutability(&columns[6]),
                ui: parse_access(&columns[7]),
                openapi: parse_access(&columns[8]),
            };
            assert!(
                rows.insert(id, row).is_none(),
                "duplicate projected setting"
            );
        }
    }
    rows
}

fn expand_projection_cell(cell: &str) -> Vec<String> {
    if cell.contains("button_1–") || cell.contains("button_1 –") {
        let prefix = if cell.starts_with("shortcuts.") {
            "shortcuts.button_"
        } else if cell.starts_with("pokecon.") {
            "pokecon.opt.shortcuts.button_"
        } else {
            "button_"
        };
        return (1..=10).map(|index| format!("{prefix}{index}")).collect();
    }
    vec![cell.to_owned()]
}

fn specification_environment_names() -> BTreeSet<&'static str> {
    between(&SPECIFICATION, "## 12. [必須要件] 環境変数", "## 13.")
        .lines()
        .filter(|line| line.starts_with("| `POKECON_") && !line.contains("POKECON_UV_*"))
        .map(|line| {
            line.split('|')
                .nth(1)
                .expect("environment projection row must have a first cell")
                .trim()
                .trim_matches('`')
        })
        .collect()
}

fn specification_rest_endpoints() -> BTreeSet<String> {
    between(&SPECIFICATION, "## 7.4 [必須要件] HTTP REST API", "## 7.5")
        .lines()
        .filter_map(|line| {
            let columns = split_markdown_row(line);
            (columns.len() >= 2 && matches!(columns[0].as_str(), "GET" | "PATCH" | "POST"))
                .then(|| format!("{} {}", columns[0], columns[1]))
        })
        .collect()
}

fn specification_websocket_variants() -> BTreeSet<String> {
    let section = between(&SPECIFICATION, "### 7.3.2", "## 8.");
    let union_table = between(
        section,
        "- **イベント／メッセージunion**:",
        "**共通JSON外形**",
    );
    let table_pattern = Regex::new(r"(?m)^\s*\|\s*`([^`]+)`").expect("table regex must compile");
    let mut variants = table_pattern
        .captures_iter(union_table)
        .map(|captures| captures[1].to_owned())
        .filter(|variant| variant != "type")
        .collect::<BTreeSet<_>>();
    let type_pattern = Regex::new(r#""type": "([a-z_.]+)""#).expect("type regex must compile");
    variants.extend(
        type_pattern
            .captures_iter(section)
            .map(|captures| captures[1].to_owned()),
    );
    variants
}

fn specification_builtin_events() -> BTreeSet<(String, String)> {
    between(
        &SPECIFICATION,
        "###### 11.5.6.1.5 組み込みイベント一覧",
        "**1. ScriptLoadPre/ScriptLoadPostのタイミング**",
    )
    .lines()
    .filter_map(|line| {
        let columns = split_markdown_row(line);
        (columns.len() == 3 && (columns[1] == "Pre" || columns[1] == "Post"))
            .then(|| (columns[0].clone(), columns[1].to_ascii_lowercase()))
    })
    .collect()
}

fn specification_compatibility_baselines() -> BTreeSet<(String, String)> {
    between(&SPECIFICATION, "### 4.6.1", "### 4.6.2")
        .lines()
        .filter_map(|line| {
            let columns = split_markdown_row(line);
            (columns.len() == 3 && columns[0].starts_with("https://"))
                .then(|| (columns[0].clone(), columns[2].clone()))
        })
        .collect()
}

fn split_markdown_row(line: &str) -> Vec<String> {
    let line = line.trim_start();
    if !line.starts_with('|') {
        return Vec::new();
    }
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for character in line[1..].chars() {
        if escaped {
            cell.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
            cell.push(character);
        } else if character == '|' {
            cells.push(clean_cell(&cell));
            cell.clear();
        } else {
            cell.push(character);
        }
    }
    if !cell.trim().is_empty() {
        cells.push(clean_cell(&cell));
    }
    cells
}

fn clean_cell(cell: &str) -> String {
    cell.trim()
        .trim_matches('`')
        .replace('`', "")
        .replace("\\|", "|")
        .replace("**", "")
        .replace('*', "")
}

fn parse_scope(value: &str) -> Scope {
    match value {
        "global" => Scope::Global,
        "profile" => Scope::Profile,
        "bootstrap" => Scope::Bootstrap,
        _ => panic!("unknown scope {value}"),
    }
}

fn parse_mutability(value: &str) -> Mutability {
    match value {
        "startup_only" => Mutability::StartupOnly,
        "runtime_immediate" => Mutability::RuntimeImmediate,
        "runtime_deferred" => Mutability::RuntimeDeferred,
        _ => panic!("unknown mutability {value}"),
    }
}

fn parse_access(value: &str) -> Access {
    if value.starts_with("R/W") || value.starts_with("R（マスク）/W") {
        Access::ReadWrite
    } else if value.starts_with('R') {
        Access::Read
    } else if value.starts_with('W') {
        Access::Write
    } else {
        Access::None
    }
}

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("embedded registry must be valid JSON")
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        Value::Object(object) => {
            let mut keys = object.keys().cloned().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut sorted = Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical_json(&object[&key]));
            }
            Value::Object(sorted)
        }
        scalar => scalar.clone(),
    }
}

fn string_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} must be a string"))
}

fn string_set(value: &Value) -> BTreeSet<&str> {
    value
        .as_array()
        .expect("value must be a string array")
        .iter()
        .map(|item| item.as_str().expect("array item must be a string"))
        .collect()
}

fn surface_for_worker<'a>(surfaces: &'a [Value], worker: &str) -> &'a Value {
    surfaces
        .iter()
        .find(|surface| surface["worker"] == worker)
        .unwrap_or_else(|| panic!("public surface for {worker} must exist"))
}

fn assert_fixed_commands_imports_are_classified(user: &Value) {
    let declared_imports = surface_imports(user, "required_imports");
    let external_imports = surface_imports(user, "external_imports");
    assert!(declared_imports.contains(&(
        "Commands.McuCommandBase".to_owned(),
        "McuCommand".to_owned()
    )));
    assert!(
        !declared_imports
            .iter()
            .any(|(_, symbol)| symbol == "McuCommandBase")
    );
    let bridge_import = (
        "Commands.PythonCommands.bridge_functions.bridge_functions".to_owned(),
        "BridgeFunctions".to_owned(),
    );
    assert!(!declared_imports.contains(&bridge_import));
    assert_eq!(external_imports, BTreeSet::from([bridge_import]));
    let bridge = &user["external_imports"][0];
    assert_eq!(bridge["bundled"], false);
    assert_eq!(bridge["reason"], "separate_license_and_distribution");
    let classified_imports = declared_imports
        .union(&external_imports)
        .cloned()
        .collect::<BTreeSet<_>>();
    for required in fixed_commands_imports() {
        assert!(
            classified_imports.contains(&required),
            "fixed corpus import {required:?} is not classified"
        );
    }
}

fn surface_imports(user: &Value, field: &str) -> BTreeSet<(String, String)> {
    user[field]
        .as_array()
        .unwrap_or_else(|| panic!("{field} must be an array"))
        .iter()
        .flat_map(|entry| {
            let module = string_at(entry, "module");
            entry["symbols"]
                .as_array()
                .expect("import symbols must be an array")
                .iter()
                .map(move |symbol| {
                    (
                        module.to_owned(),
                        symbol
                            .as_str()
                            .expect("import symbol must be a string")
                            .to_owned(),
                    )
                })
        })
        .collect()
}

fn assert_user_script_members_have_a_normative_source(user: &Value) {
    let class_members = user["class_members"]
        .as_object()
        .expect("class_members must be an object");
    let module_functions = user["module_functions"]
        .as_object()
        .expect("module_functions must be an object");
    for names in class_members.values().chain(module_functions.values()) {
        for name in names
            .as_array()
            .expect("public member list must be an array")
        {
            let name = name.as_str().expect("public member must be a string");
            assert!(
                SPECIFICATION.contains(name) || FIXED_MANIFEST.contains(name),
                "user-script API name {name} has no specification or corpus source"
            );
        }
    }
}

fn fixed_commands_imports() -> BTreeSet<(String, String)> {
    let inventory = parse_json(&FIXED_MANIFEST);
    inventory["baselines"]
        .as_array()
        .expect("inventory baselines must be an array")
        .iter()
        .flat_map(|baseline| {
            baseline["scripts"]
                .as_array()
                .expect("baseline scripts must be an array")
        })
        .flat_map(|script| {
            script["imports"]
                .as_array()
                .expect("script imports must be an array")
        })
        .filter(|entry| string_at(entry, "module").starts_with("Commands"))
        .flat_map(|entry| {
            let module = string_at(entry, "module");
            entry["names"]
                .as_array()
                .expect("import names must be an array")
                .iter()
                .map(move |name| {
                    (
                        module.to_owned(),
                        name.as_str()
                            .expect("import name must be a string")
                            .to_owned(),
                    )
                })
        })
        .collect()
}

fn artifact_for_name<'a>(artifacts: &'a [Value], name: &str) -> &'a Value {
    artifacts
        .iter()
        .find(|artifact| artifact["name"] == name)
        .unwrap_or_else(|| panic!("generated artifact {name} must exist"))
}

fn workflow_job_names() -> BTreeSet<String> {
    WORKFLOWS
        .iter()
        .flat_map(|(workflow, source)| {
            let jobs = source
                .split_once("\njobs:\n")
                .unwrap_or_else(|| panic!("workflow {workflow} must define jobs"))
                .1;
            jobs.lines()
                .filter(|line| {
                    line.starts_with("  ") && !line.starts_with("    ") && line.ends_with(':')
                })
                .map(move |line| format!("{workflow}/{}", line.trim().trim_end_matches(':')))
        })
        .collect()
}

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split_once(start)
        .unwrap_or_else(|| panic!("start marker {start:?} not found"))
        .1
        .split_once(end)
        .unwrap_or_else(|| panic!("end marker {end:?} not found"))
        .0
}

#[test]
fn design_principles_map_to_runtime_mechanisms_without_drift() {
    // AR-11-24: docs/ARCHITECTURE.md の設計原則→module／queue／lock／task／thread
    // 対応表が、実装の実在symbolと一致し続けることを保証する。表の行の欠落・
    // 変更、symbolのリネーム、moduleの公開API化のいずれもこのtestを失敗させる。
    let architecture = repository_text("docs/ARCHITECTURE.md");
    let section = architecture
        .split_once("## 設計原則と実行時機構の対応")
        .expect("ARCHITECTURE.md must define the principle-to-mechanism section")
        .1;
    let section = section
        .split_once("\n## ")
        .map_or(section, |(head, _)| head);
    let rows: Vec<&str> = section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .filter(|line| !line.contains("設計原則"))
        .filter(|line| !line.contains("---"))
        .collect();
    assert_eq!(
        rows.len(),
        9,
        "principle-to-mechanism table must keep exactly 9 principle rows"
    );

    // (table module fragment, required mechanism symbol, owning source file).
    // Every symbol below was traced to its source before being asserted here.
    let required: &[(&str, &str, &str)] = &[
        (
            "worker::supervisor",
            "ManagedWorker",
            "rust/pokecon/src/worker/supervisor.rs",
        ),
        (
            "server::state",
            "StateHub",
            "rust/pokecon/src/server/state.rs",
        ),
        (
            "server::websocket",
            "ephemeral_queue_capacity",
            "rust/pokecon/src/server/websocket.rs",
        ),
        (
            "device::input",
            "InputArbiter",
            "rust/pokecon/src/device/input.rs",
        ),
        (
            "device::serial::manager",
            "SerialManager",
            "rust/pokecon/src/device/serial/manager.rs",
        ),
        (
            "camera::manager",
            "CameraManager",
            "rust/pokecon/src/camera/manager.rs",
        ),
        (
            "worker::generation",
            "WorkerGeneration",
            "rust/pokecon/src/worker/generation.rs",
        ),
        (
            "runtime::shutdown",
            "ShutdownCoordinator",
            "rust/pokecon/src/runtime/shutdown.rs",
        ),
        (
            "server::security",
            "RequestSecurity",
            "rust/pokecon/src/server/security.rs",
        ),
        (
            "server::api",
            "ApiErrorCode",
            "rust/pokecon/src/server/api.rs",
        ),
    ];
    for (module, symbol, source) in required {
        assert!(
            rows.iter()
                .any(|row| row.contains(module) && row.contains(symbol)),
            "mapping row for {module} must name mechanism {symbol}"
        );
        let implementation = repository_text(source);
        assert!(
            implementation.contains(symbol),
            "mechanism {symbol} must exist in {source}"
        );
    }

    // Table module paths are private implementation facts, not public API
    // promises: the owning top-level modules must stay crate-private.
    let lib = repository_text("rust/pokecon/src/lib.rs");
    for module in ["camera", "device", "dynamic", "runtime", "server", "worker"] {
        assert!(
            lib.contains(&format!("mod {module};")),
            "lib.rs must declare module {module}"
        );
        assert!(
            !lib.contains(&format!("pub mod {module};")),
            "module {module} must stay crate-private, not a public API promise"
        );
    }
}

#[test]
fn module_ownership_table_has_unique_owners_without_drift() {
    // docs/ARCHITECTURE_HANDOFF.md 3.1 module ownership manifest の drift guard。
    // 表の行の欠落・重複所有行・module名の変更のいずれもこのtestを失敗させる。
    // worker境界の一点のforbidden edgeのみを検査し、全forbidden edgeの被覆は主張しない。
    let handoff = repository_text("docs/ARCHITECTURE_HANDOFF.md");
    let section = handoff
        .split_once("### 3.1 Module ownership manifest")
        .expect("ARCHITECTURE_HANDOFF.md must define the 3.1 ownership manifest section")
        .1;
    let section = section
        .split_once("\n### ")
        .map_or(section, |(head, _)| head);
    let rows: Vec<&str> = section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .filter(|line| !line.contains("所有する責務"))
        .filter(|line| !line.contains("---"))
        .collect();
    assert_eq!(
        rows.len(),
        12,
        "ownership manifest table must keep exactly 12 module/process rows, found: {rows:?}"
    );

    let cells: Vec<&str> = rows
        .iter()
        .map(|row| {
            row.split('|')
                .nth(1)
                .unwrap_or_else(|| panic!("ownership manifest row must have a module cell: {row}"))
                .trim()
        })
        .collect();
    let expected = [
        "`entrypoint`／`production`",
        "`application_backend`",
        "`server`",
        "`device::input`",
        "`device::serial`",
        "`camera`",
        "`worker::supervisor`／`worker::generation`",
        "`worker_binary`",
        "`dynamic::transaction`／dynamic host",
        "`settings`／`contracts`／`registry`",
        "`runtime::shutdown`",
        "frontend／generated client",
    ];
    for module in expected {
        let occurrences = cells.iter().filter(|cell| ***cell == *module).count();
        assert_eq!(
            occurrences, 1,
            "ownership manifest must list {module} exactly once, found {occurrences} in {cells:?}"
        );
    }

    // Every symbol below was traced to its source before being asserted here.
    let required: &[(&str, &str)] = &[
        ("CameraManager", "rust/pokecon/src/camera/manager.rs"),
        ("SerialManager", "rust/pokecon/src/device/serial/manager.rs"),
        ("StateHub", "rust/pokecon/src/server/state.rs"),
        ("InputArbiter", "rust/pokecon/src/device/input.rs"),
        (
            "ShutdownCoordinator",
            "rust/pokecon/src/runtime/shutdown.rs",
        ),
        ("WorkerGeneration", "rust/pokecon/src/worker/generation.rs"),
    ];
    for (symbol, source) in required {
        let implementation = repository_text(source);
        assert!(
            implementation.contains(*symbol),
            "owner symbol {symbol} must exist in {source}"
        );
    }

    // Narrow forbidden edge for the manifest's worker boundary only:
    // the worker supervisor/generation must not own native/resource-owner symbols.
    for source in [
        "rust/pokecon/src/worker/supervisor.rs",
        "rust/pokecon/src/worker/generation.rs",
    ] {
        let implementation = repository_text(source);
        for forbidden in ["CameraManager", "SerialManager", "StateHub"] {
            assert!(
                !implementation.contains(forbidden),
                "worker boundary {source} must not own {forbidden}"
            );
        }
    }
}

#[test]
fn public_wire_contracts_exclude_native_handles_and_private_paths() {
    // AR-11-27 negative contract for docs/ARCHITECTURE_HANDOFF.md 5.2
    // (public schema / native-object boundary): the genuinely public wire
    // surfaces must not expose camera/serial native handles, Python/Lua/Rust
    // private objects, or Rust private module paths, and must not overclaim
    // dynamic callback ownership. Only canonical public inputs are scanned:
    // rust/pokecon/src/server/api.rs (public HTTP/WS wire types),
    // rust/pokecon/registry/protocol.json (canonical protocol registry),
    // api/openapi.json (generated OpenAPI surface), and
    // web/src/lib/api/openapi.ts (generated frontend client).
    // server/rest/mod.rs legitimately uses the internal StateHub and is
    // intentionally NOT scanned, so this test cannot misfire on it.
    //
    // Owner symbols are source-cited, not invented: CameraManager lives in
    // rust/pokecon/src/camera/manager.rs, SerialManager lives in
    // rust/pokecon/src/device/serial/manager.rs, and StateHub lives in
    // rust/pokecon/src/server/state.rs. The public descriptors in
    // rust/pokecon/src/server/api.rs (CameraDevice/CameraSelector as
    // index-or-name, SerialPort as selector/label/available strings) carry
    // no such owner, which is exactly what the assertions below pin.
    const NATIVE_HANDLE_MARKERS: &[&str] = &[
        "CameraManager",
        "SerialManager",
        "StateHub",
        "JoinHandle",
        "CameraHandle",
        "SerialHandle",
    ];
    // Precise FFI/private-object markers. Deliberately NOT bare "Lua"/"lua":
    // rust/pokecon/src/server/api.rs legitimately exposes
    // DynamicLanguage::Lua and rust/pokecon/registry/protocol.json
    // legitimately lists "lua" as a dynamic language, so a bare substring
    // would misfire on public descriptors.
    const PRIVATE_OBJECT_MARKERS: &[&str] = &["PyObject", "pyo3::", "mlua::", "LuaValue"];
    // Rust private module paths. Deliberately NOT bare "crate::server":
    // rust/pokecon/src/server/api.rs doc comments legitimately reference
    // `crate::server::openapi` (the schema replacer), so only genuinely
    // internal owner paths are forbidden here.
    const PRIVATE_PATH_MARKERS: &[&str] = &[
        "crate::camera",
        "crate::device",
        "crate::worker",
        "crate::runtime",
        "crate::dynamic",
        "crate::server::state",
        "crate::server::security",
        "rust/pokecon/src/",
    ];
    // Overclaimed dynamic-callback ownership shapes. None appear in any
    // public surface today; asserting their absence keeps the boundary
    // honest without inferring ownership from mere source text (callback
    // scheduling lives in rust/pokecon/src/dynamic/callback.rs, which is
    // NOT scanned here).
    const OWNERSHIP_OVERCLAIM_MARKERS: &[&str] = &[
        "callback owns",
        "owns the device",
        "owns shutdown",
        "callback_owner",
        "owns_device_thread",
        "shutdown_owner",
    ];

    let public_surfaces = [
        (
            "rust/pokecon/src/server/api.rs",
            repository_text("rust/pokecon/src/server/api.rs"),
        ),
        (
            "rust/pokecon/registry/protocol.json",
            repository_text("rust/pokecon/registry/protocol.json"),
        ),
        ("api/openapi.json", repository_text("api/openapi.json")),
        (
            "web/src/lib/api/openapi.ts",
            repository_text("web/src/lib/api/openapi.ts"),
        ),
    ];
    let marker_sets = [
        ("native-handle", NATIVE_HANDLE_MARKERS),
        ("private-object", PRIVATE_OBJECT_MARKERS),
        ("private-path", PRIVATE_PATH_MARKERS),
        ("ownership-overclaim", OWNERSHIP_OVERCLAIM_MARKERS),
    ];

    let find_forbidden_marker = |text: &str| {
        marker_sets.iter().find_map(|(set_name, markers)| {
            markers
                .iter()
                .find_map(|marker| text.contains(marker).then_some((*set_name, *marker)))
        })
    };

    // Fail-closed control: exercise the exact predicate used below with a
    // synthetic injection, so a broken or empty scanner cannot pass this test
    // vacuously.
    for (set_name, markers) in &marker_sets {
        assert!(
            !markers.is_empty(),
            "{set_name} marker set must not be empty"
        );
        let marker = markers[0];
        let injected = format!("AR-11-27 sentinel {marker}");
        assert_eq!(
            find_forbidden_marker(&injected),
            Some((*set_name, marker)),
            "AR-11-27 scanner control must detect injected {set_name} marker {marker:?}"
        );
    }

    for (path, text) in &public_surfaces {
        assert_eq!(
            find_forbidden_marker(text),
            None,
            "public wire surface {path} must not expose a forbidden marker \
             (AR-11-27, docs/ARCHITECTURE_HANDOFF.md 5.2)"
        );
    }
}

#[test]
fn camera_shared_mapping_release_has_single_owner() {
    // AR-11-25 check #2 (docs/ARCHITECTURE_HANDOFF.md 3.2 item 2):
    // only `CameraManager` may release the camera native handle or the
    // shared mapping. The production shutdown fallbacks in
    // `stop_inputs_camera_and_scripts` (§15.6 steps 2-3:
    // `retain_camera_fallback_on_timeout` / `camera_writer_fallback` for a
    // timed-out writer, `recover_reader_pins_after_script_shutdown` /
    // `retain_reader_mapping_after_script_shutdown` /
    // `camera_reader_fallback` for an unreaped script reader) and the
    // step-5 fail-closed persistence
    // (`retain_dynamic_mapping_after_worker_shutdown`, observed read-only
    // by `shared_memory_release_gate_after_dynamic_reap`) retain their
    // guards until OS process exit and never unmap, so they hold ownership
    // without a release marker. The documented owners
    // are `camera::manager` (`CameraManager`) plus `camera::media`
    // (`SharedFrameRing`) per docs/ARCHITECTURE.md (camera frame lifetime
    // row); the release itself lives one layer down in
    // `camera/shared_ring.rs` (`MappedRing::unlink_name` -> `shm_unlink`),
    // reached only via `ManagerInner::record_writer_unstopped`
    // in `camera::manager`.
    //
    // Release markers are deliberately narrow: bare `CloseHandle` is
    // excluded because rust/pokecon/src/settings/hmac_key.rs legitimately
    // closes a non-camera OS key handle, and `SharedFrameRing::open` (used
    // by rust/pokecon/src/worker_binary/script/python.rs) opens a mapping
    // for reading without releasing it.
    const RELEASE_MARKERS: &[&str] = &["shm_unlink", "unlink_name", "munmap", "UnmapViewOfFile"];
    // Allowlist = camera manager + shared-ring owner impl. The production
    // shutdown fallback file is asserted separately below: it must
    // reference the retained guard yet contain no release marker.
    const RELEASE_OWNERS: &[&str] = &[
        "rust/pokecon/src/camera/manager.rs",
        "rust/pokecon/src/camera/shared_ring.rs",
    ];
    const FALLBACK_FILE: &str = "rust/pokecon/src/production.rs";
    const FALLBACK_OWNERSHIP_MARKERS: &[&str] = &[
        "retain_camera_fallback_on_timeout",
        "camera_writer_fallback",
        "UnstoppedCameraWriter",
        "camera_reader_fallback",
        "retain_reader_mapping_after_script_shutdown",
        "recover_reader_pins_after_script_shutdown",
        "shared_memory_release_gate_after_dynamic_reap",
        "retain_dynamic_mapping_after_worker_shutdown",
    ];

    let find_release_marker = |text: &str| {
        RELEASE_MARKERS
            .iter()
            .find_map(|marker| text.contains(marker).then_some(*marker))
    };

    // Fail-closed control: exercise the exact predicate used below with a
    // synthetic injection, so a broken or empty scanner cannot pass this
    // test vacuously.
    assert!(
        !RELEASE_MARKERS.is_empty(),
        "camera release marker set must not be empty"
    );
    let sentinel = RELEASE_MARKERS[0];
    let injected = format!("AR-11-25 sentinel {sentinel}");
    assert_eq!(
        find_release_marker(&injected),
        Some(sentinel),
        "AR-11-25 scanner control must detect injected release marker {sentinel:?}"
    );

    // The documented owners pin the allowlist to docs/ARCHITECTURE.md, not
    // to invented symbols.
    let architecture = repository_text("docs/ARCHITECTURE.md");
    assert!(
        architecture.contains("`camera::manager` (`CameraManager`)"),
        "docs/ARCHITECTURE.md must document CameraManager as the camera owner (AR-11-25)"
    );

    // The production shutdown fallback must hold (not release) the mapping:
    // it references the retained guard and contains no release marker.
    let fallback = repository_text(FALLBACK_FILE);
    for marker in FALLBACK_OWNERSHIP_MARKERS {
        assert!(
            fallback.contains(marker),
            "production shutdown fallback {FALLBACK_FILE} must reference {marker} (AR-11-25)"
        );
    }
    assert_eq!(
        find_release_marker(&fallback),
        None,
        "production shutdown fallback {FALLBACK_FILE} must not release the camera shared mapping (AR-11-25)"
    );

    // Every other Rust source file must contain no camera release marker;
    // only the allowlisted owner impls may.
    let mut scanned = 0;
    let mut stack = vec![repository_root().join("rust/pokecon/src")];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).unwrap_or_else(|error| {
            panic!("contract input {} must be readable: {error}", dir.display())
        });
        for entry in entries {
            let entry = entry.expect("contract input directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs")) {
                scanned += 1;
                let relative = path
                    .strip_prefix(repository_root())
                    .expect("contract input must be below the repository root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!("contract input {relative} must be readable: {error}")
                });
                if RELEASE_OWNERS.contains(&relative.as_str()) {
                    assert!(
                        find_release_marker(&text).is_some(),
                        "allowlisted owner {relative} must still contain a camera release marker; update the allowlist instead of keeping a stale entry (AR-11-25)"
                    );
                } else {
                    assert_eq!(
                        find_release_marker(&text),
                        None,
                        "non-owner source {relative} must not release the camera shared mapping (AR-11-25, docs/ARCHITECTURE_HANDOFF.md 3.2 item 2)"
                    );
                }
            }
        }
    }
    assert!(
        scanned > 100,
        "camera ownership scan must cover rust/pokecon/src (scanned {scanned} files)"
    );
}

#[test]
fn serial_port_ownership_has_single_owner() {
    // AR-11-25 check #3 (docs/ARCHITECTURE_HANDOFF.md 3.2 item 3):
    // only `SerialManager` may hold the native serial port. The native
    // handle (`SerialStream`) is acquired in the native serial backend
    // (`tokio_serial::new` -> `open_native_async`) and held split inside
    // `NativeSerialIo`; it is reached only via
    // `SerialManager::open_initialized` through the `SerialBackend` trait
    // (rust/pokecon/src/device/serial/manager.rs), so the manager itself
    // never names the native handle type. The documented owner is
    // `device::serial::manager` (`SerialManager`) per docs/ARCHITECTURE.md
    // (serial ownership row).
    //
    // Hold markers are deliberately narrow: `tokio_serial::available_ports`
    // in rust/pokecon/src/device/serial/selector.rs only enumerates port
    // names without opening them, and the `NativeSerialBackend` re-exports
    // (`device::serial::mod`, `device::mod`) plus the
    // `SerialManager::new(Arc::new(NativeSerialBackend))` wiring in
    // rust/pokecon/src/production.rs and
    // rust/pokecon/src/integration_test_support.rs never name the native
    // handle, so bare `tokio_serial`, `available_ports`,
    // `NativeSerialBackend`, and `SerialManager` are excluded.
    const HOLD_MARKERS: &[&str] = &["open_native_async", "tokio_serial::new", "SerialStream"];
    // Allowlist = the single backend impl file owned by SerialManager.
    // `manager.rs` holds only `Arc<dyn SerialIo>` through the
    // `SerialBackend` trait and contains no hold marker, so it belongs to
    // the negative assertion set rather than the allowlist.
    const HOLD_OWNERS: &[&str] = &["rust/pokecon/src/device/serial/native.rs"];

    let find_hold_marker = |text: &str| {
        HOLD_MARKERS
            .iter()
            .find_map(|marker| text.contains(marker).then_some(*marker))
    };

    // Fail-closed control: exercise the exact predicate used below with a
    // synthetic injection, so a broken or empty scanner cannot pass this
    // test vacuously.
    assert!(
        !HOLD_MARKERS.is_empty(),
        "serial hold marker set must not be empty"
    );
    let sentinel = HOLD_MARKERS[0];
    let injected = format!("AR-11-25 sentinel {sentinel}");
    assert_eq!(
        find_hold_marker(&injected),
        Some(sentinel),
        "AR-11-25 scanner control must detect injected hold marker {sentinel:?}"
    );

    // The documented owner pins the allowlist to docs/ARCHITECTURE.md, not
    // to invented symbols.
    let architecture = repository_text("docs/ARCHITECTURE.md");
    assert!(
        architecture.contains("`device::serial::manager` (`SerialManager`"),
        "docs/ARCHITECTURE.md must document SerialManager as the serial owner (AR-11-25)"
    );

    // Every other Rust source file must contain no serial hold marker;
    // only the allowlisted owner impl may.
    let mut scanned = 0;
    let mut stack = vec![repository_root().join("rust/pokecon/src")];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).unwrap_or_else(|error| {
            panic!("contract input {} must be readable: {error}", dir.display())
        });
        for entry in entries {
            let entry = entry.expect("contract input directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs")) {
                scanned += 1;
                let relative = path
                    .strip_prefix(repository_root())
                    .expect("contract input must be below the repository root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!("contract input {relative} must be readable: {error}")
                });
                if HOLD_OWNERS.contains(&relative.as_str()) {
                    assert!(
                        find_hold_marker(&text).is_some(),
                        "allowlisted owner {relative} must still contain a serial hold marker; update the allowlist instead of keeping a stale entry (AR-11-25)"
                    );
                } else {
                    assert_eq!(
                        find_hold_marker(&text),
                        None,
                        "non-owner source {relative} must not hold the native serial port (AR-11-25, docs/ARCHITECTURE_HANDOFF.md 3.2 item 3)"
                    );
                }
            }
        }
    }
    assert!(
        scanned > 100,
        "serial ownership scan must cover rust/pokecon/src (scanned {scanned} files)"
    );
}

#[test]
fn visible_revision_commit_has_single_owner() {
    // AR-11-25 check #1 (docs/ARCHITECTURE_HANDOFF.md:82, 3.2 item 1):
    // only `ApplicationBackend` may commit the visible revision. The two
    // product commit paths both live in
    // rust/pokecon/src/application_backend.rs: `commit_projection`
    // (defined :254, called throughout the file) funnels through
    // `self.hub.commit(transaction)` (:272) and the split-receiver
    // `self.hub\n.commit(transaction)` (:341-342).
    //
    // `#[cfg(test)]` truncation is applied for the three TEST_TAIL_FILES
    // below because their inline test modules call `hub.commit` /
    // `.commit(transaction)` as fixtures (state.rs:509+, websocket.rs:2316+,
    // rest/mod.rs:292+); scanning only the production prefix keeps those
    // test-only commits from tripping the negative assert.
    const COMMIT_MARKERS: &[&str] = &["commit_projection", "hub.commit", ".commit(transaction)"];
    // Allowlist = the single visible-revision commit owner. The
    // split-receiver path (:341-342) contains no contiguous `hub.commit`
    // substring, so `.commit(transaction)` is matched separately.
    const COMMIT_OWNERS: &[&str] = &["rust/pokecon/src/application_backend.rs"];
    // Files whose `#[cfg(test)] mod tests` module runs to EOF and exercises
    // commit markers as fixtures: scan only the production prefix.
    const TEST_TAIL_FILES: &[&str] = &[
        "rust/pokecon/src/server/state.rs",
        "rust/pokecon/src/server/websocket.rs",
        "rust/pokecon/src/server/rest/mod.rs",
    ];

    let find_commit_marker = |text: &str| {
        COMMIT_MARKERS
            .iter()
            .find_map(|marker| text.contains(marker).then_some(*marker))
    };

    // Fail-closed control: exercise the exact predicate used below with a
    // synthetic injection, so a broken or empty scanner cannot pass this
    // test vacuously.
    assert!(
        !COMMIT_MARKERS.is_empty(),
        "visible revision commit marker set must not be empty"
    );
    let sentinel = COMMIT_MARKERS[0];
    let injected = format!("AR-11-25 sentinel {sentinel}");
    assert_eq!(
        find_commit_marker(&injected),
        Some(sentinel),
        "AR-11-25 scanner control must detect injected commit marker {sentinel:?}"
    );

    // The documented owner pins the allowlist to docs/ARCHITECTURE.md, not
    // to invented symbols.
    let architecture = repository_text("docs/ARCHITECTURE.md");
    assert!(
        architecture.contains("`ApplicationBackend`が`StateHub`と各resource serviceを所有してprojectionを更新し、`server`と`desktop`はhardware handleまたはinterpreter stateを直接所有しません。"),
        "docs/ARCHITECTURE.md must document ApplicationBackend as the visible revision owner (AR-11-25)"
    );

    // Every other Rust source file must contain no visible revision commit
    // marker; only the allowlisted owner may.
    let mut scanned = 0;
    let mut stack = vec![repository_root().join("rust/pokecon/src")];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).unwrap_or_else(|error| {
            panic!("contract input {} must be readable: {error}", dir.display())
        });
        for entry in entries {
            let entry = entry.expect("contract input directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs")) {
                scanned += 1;
                let relative = path
                    .strip_prefix(repository_root())
                    .expect("contract input must be below the repository root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!("contract input {relative} must be readable: {error}")
                });
                let effective: &str = if TEST_TAIL_FILES.contains(&relative.as_str()) {
                    let occurrences = text.match_indices("#[cfg(test)]").count();
                    assert_eq!(
                        occurrences, 1,
                        "truncation assumption broke for {relative}: expected exactly one #[cfg(test)] before the trailing test module (AR-11-25)"
                    );
                    let marker = text.find("#[cfg(test)]").unwrap_or_else(|| {
                        panic!(
                            "truncation assumption broke for {relative}: #[cfg(test)] marker vanished (AR-11-25)"
                        )
                    });
                    let after_marker = &text[marker + "#[cfg(test)]".len()..];
                    let first_nonblank = after_marker
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or_else(|| {
                            panic!(
                                "truncation assumption broke for {relative}: no code follows #[cfg(test)] (AR-11-25)"
                            )
                        });
                    assert_eq!(
                        first_nonblank.trim(),
                        "mod tests {",
                        "truncation assumption broke for {relative}: #[cfg(test)] must be followed by `mod tests {{` (AR-11-25)"
                    );
                    &text[..marker]
                } else {
                    &text
                };
                if COMMIT_OWNERS.contains(&relative.as_str()) {
                    assert!(
                        find_commit_marker(effective).is_some(),
                        "allowlisted owner {relative} must still contain a visible revision commit marker; update the allowlist instead of keeping a stale entry (AR-11-25)"
                    );
                } else {
                    assert_eq!(
                        find_commit_marker(effective),
                        None,
                        "non-owner source {relative} must not commit the visible revision (AR-11-25, docs/ARCHITECTURE_HANDOFF.md 3.2 item 1)"
                    );
                }
            }
        }
    }
    assert!(
        scanned > 100,
        "visible revision ownership scan must cover rust/pokecon/src (scanned {scanned} files)"
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn module_ownership_manifest_pins_owner_symbols() {
    // AR-11-33: docs/ARCHITECTURE_HANDOFF.md 3.1 module ownership manifest の
    // machine-readable pin (rust/pokecon/registry/ownership.json)。
    // manifest の module id と 3.1 テーブルの行が 1:1 に対応し、各 owner symbol が
    // 列挙 source 内に実在し、対応する top module が crate-private のままであることを検査する。
    static OWNERSHIP_JSON: LazyLock<String> =
        LazyLock::new(|| repository_text("rust/pokecon/registry/ownership.json"));
    let manifest = parse_json(&OWNERSHIP_JSON);
    assert_eq!(
        manifest["schema_version"], 1,
        "ownership manifest schema_version must be 1 (AR-11-33)"
    );
    let modules = manifest["modules"]
        .as_array()
        .expect("ownership manifest must define a modules array");
    assert!(
        !modules.is_empty(),
        "ownership manifest must pin at least one module"
    );

    // Fail-closed predicate helpers: the exact predicates asserted below.
    let is_valid_symbol = |symbol: &str| !symbol.trim().is_empty();
    let symbol_found = |symbol: &str, texts: &[&str]| {
        !symbol.is_empty() && texts.iter().any(|text| text.contains(symbol))
    };

    // Fail-closed control 1: an empty manifest entry or an empty symbol must be
    // rejected, so a vacuous manifest cannot pass this test.
    assert!(
        !is_valid_symbol(""),
        "empty owner symbol must be rejected (AR-11-33 control)"
    );
    assert!(
        !symbol_found("", &["anything"]),
        "empty symbol must never match any text (AR-11-33 control)"
    );
    assert!(
        !symbol_found(
            "AR-11-33-sentinel-symbol-that-exists-nowhere",
            &["some unrelated source text"]
        ),
        "unknown symbol must not match unrelated text (AR-11-33 control)"
    );

    // Fail-closed control 2 (sentinel injection): the symbol-scan predicate used
    // below must detect a synthetic symbol, proving the scan is not broken.
    let sentinel = "AR-11-33-sentinel-symbol";
    let injected = format!("synthetic owner {sentinel} marker");
    assert!(
        symbol_found(sentinel, &[&injected]),
        "symbol scan predicate must detect an injected sentinel symbol (AR-11-33 control)"
    );

    // (a) Manifest ids are unique and bijective with the 3.1 table rows: each
    // manifest id appears in the table section, and each table row id appears
    // in the manifest. Parsing is deliberately pragmatic: split the module
    // cell on slashes and normalize `::`/space/`-` to `_`.
    let handoff = repository_text("docs/ARCHITECTURE_HANDOFF.md");
    let section = handoff
        .split_once("### 3.1 Module ownership manifest")
        .expect("ARCHITECTURE_HANDOFF.md must define the 3.1 ownership manifest section")
        .1;
    let section = section
        .split_once("\n### ")
        .map_or(section, |(head, _)| head);
    let normalize = |token: &str| {
        token
            .trim()
            .trim_matches('`')
            .replace("::", "_")
            .replace([' ', '-'], "_")
            .to_lowercase()
    };
    let mut expected = BTreeSet::new();
    for line in section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
    {
        if line.contains("---") {
            continue;
        }
        let module_cell = line
            .split('|')
            .nth(1)
            .expect("3.1 table row must have a module cell");
        let cell_ids: Vec<String> = module_cell
            .split(['\u{FF0F}', '/'])
            .map(&normalize)
            .filter(|id| !id.is_empty())
            .collect();
        // Skip the header row (`module／process`); match on the module cell
        // only, since body cells may legitimately mention "module".
        if cell_ids.iter().all(|id| id == "module" || id == "process") {
            continue;
        }
        for id in cell_ids {
            expected.insert(id);
        }
    }
    assert!(
        expected.len() >= 12,
        "3.1 table parse must cover the ownership rows (parsed {})",
        expected.len()
    );
    let mut ids = BTreeSet::new();
    for module in modules {
        let id = module["id"]
            .as_str()
            .expect("ownership manifest module must have a string id");
        assert!(
            ids.insert(id.to_owned()),
            "ownership manifest module id {id} must be unique"
        );
        assert!(
            expected.contains(id),
            "ownership manifest id {id} must appear in the 3.1 table section"
        );
    }
    assert_eq!(
        ids, expected,
        "ownership manifest ids must be bijective with the 3.1 table rows"
    );

    // (b) Every owner symbol occurs in at least one listed source file.
    // Entries without Rust owner symbols (registry inputs, frontend, generated
    // client) must explain the gap in `note` instead of inventing symbols.
    for module in modules {
        let id = module["id"]
            .as_str()
            .expect("ownership manifest module must have a string id");
        let owns = module["owns"]
            .as_array()
            .expect("ownership manifest module must declare owns");
        assert!(
            !owns.is_empty(),
            "module {id} must declare at least one owned responsibility"
        );
        let symbols = module["owner_symbols"]
            .as_array()
            .expect("ownership manifest module must declare owner_symbols");
        let files = module["source_files"]
            .as_array()
            .expect("ownership manifest module must declare source_files");
        assert!(
            !files.is_empty(),
            "module {id} must list at least one source file"
        );
        let texts: Vec<String> = files
            .iter()
            .map(|file| {
                let path = file
                    .as_str()
                    .expect("ownership manifest source file must be a string");
                repository_text(path)
            })
            .collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        if symbols.is_empty() {
            let note = module
                .get("note")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(
                !note.trim().is_empty(),
                "module {id} without owner symbols must explain why in `note` \
                 instead of inventing symbols (AR-11-33)"
            );
            continue;
        }
        for symbol in symbols {
            let symbol = symbol
                .as_str()
                .expect("ownership manifest owner symbol must be a string");
            assert!(
                is_valid_symbol(symbol),
                "module {id} owner symbol must not be empty"
            );
            assert!(
                symbol_found(symbol, &refs),
                "owner symbol {symbol} of module {id} must occur in one of its source_files"
            );
        }
    }

    // (c) The owning top-level modules stay crate-private: derive the top
    // module segments from the manifest source files and pin lib.rs.
    let lib = repository_text("rust/pokecon/src/lib.rs");
    let mut top_modules = BTreeSet::new();
    for module in modules {
        for file in module["source_files"]
            .as_array()
            .expect("ownership manifest module must declare source_files")
        {
            let path = file
                .as_str()
                .expect("ownership manifest source file must be a string");
            if let Some(rest) = path.strip_prefix("rust/pokecon/src/") {
                // Skip lib.rs itself: it is the composition root that declares
                // the modules, not a module.
                if rest == "lib.rs" {
                    continue;
                }
                let top = rest
                    .split('/')
                    .next()
                    .expect("source path below src/ must be non-empty");
                top_modules.insert(top.strip_suffix(".rs").unwrap_or(top).to_owned());
            }
        }
    }
    assert!(
        !top_modules.is_empty(),
        "ownership manifest must pin at least one Rust top module"
    );
    for top in &top_modules {
        assert!(
            lib.contains(&format!("mod {top};")),
            "lib.rs must declare module {top}"
        );
        assert!(
            !lib.contains(&format!("pub mod {top};")),
            "module {top} must stay crate-private, not a public API promise"
        );
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn forbidden_ownership_manifest_is_enforced() {
    // AR-11-34: docs/ARCHITECTURE_HANDOFF.md 3.1「所有しない責務」columnの
    // machine-readable pin (rust/pokecon/registry/ownership.json の
    // non_responsibilities／forbidden_symbols／forbidden_scan_files)。
    // 各 module が所有しない責務の代表 symbol を列挙 source 内で参照しないこと、
    // かつ代表 symbol が他所に実在すること（typo で検査が空回りしないこと）を検査する。
    // §9.2 の全 forbidden-edge 被覆は主張しない: manifest に列挙された edge のみを
    // 検査し、§3.2 の重複所有検査 1〜5 の残りは既存 test の範囲に留める
    // （module_ownership_table_has_unique_owners_without_drift の限定と同じ流儀）。
    static OWNERSHIP_JSON: LazyLock<String> =
        LazyLock::new(|| repository_text("rust/pokecon/registry/ownership.json"));
    let manifest = parse_json(&OWNERSHIP_JSON);
    let modules = manifest["modules"]
        .as_array()
        .expect("ownership manifest must define a modules array");
    assert!(
        !modules.is_empty(),
        "ownership manifest must pin at least one module"
    );

    // Fail-closed predicate helpers: the exact predicates asserted below.
    let is_valid_symbol = |symbol: &str| !symbol.trim().is_empty();
    let forbidden_absent = |symbol: &str, texts: &[&str]| {
        !symbol.is_empty() && texts.iter().all(|text| !text.contains(symbol))
    };

    // Fail-closed control 1 (empty-input rejection): an empty symbol must be
    // rejected, so a vacuous manifest entry cannot pass this test.
    assert!(
        !is_valid_symbol(""),
        "empty forbidden symbol must be rejected (AR-11-34 control)"
    );
    assert!(
        !forbidden_absent("", &["some source text"]),
        "empty symbol must never count as absent (AR-11-34 control)"
    );

    // Fail-closed control 2 (sentinel injection): the absence predicate used
    // below must detect a synthetic forbidden occurrence, proving the scan is
    // not broken.
    let sentinel = "AR-11-34-sentinel-forbidden";
    let injected = format!("synthetic forbidden {sentinel} reference");
    assert!(
        !forbidden_absent(sentinel, &[&injected]),
        "absence predicate must detect an injected sentinel occurrence (AR-11-34 control)"
    );

    // (a) Every entry declares its non-responsibilities from the 3.1 table and
    // pairs them with forbidden symbols, or explains the gap in `note` instead
    // of inventing symbols (AR-11-33 honesty pattern).
    // (b) Each forbidden symbol is absent from every forbidden_scan_file.
    let mut required_pins = BTreeMap::from([
        ("server", vec!["CameraManager", "SerialManager"]),
        ("application_backend", vec!["TcpListener"]),
        ("worker_supervisor", vec!["StateHub"]),
    ]);
    for module in modules {
        let id = module["id"]
            .as_str()
            .expect("ownership manifest module must have a string id");
        let non_responsibilities = module["non_responsibilities"]
            .as_array()
            .unwrap_or_else(|| panic!("module {id} must declare non_responsibilities (AR-11-34)"));
        assert!(
            !non_responsibilities.is_empty(),
            "module {id} must declare at least one non-responsibility"
        );
        let symbols = module["forbidden_symbols"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if symbols.is_empty() {
            let note = module
                .get("note")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(
                !note.trim().is_empty(),
                "module {id} without forbidden symbols must explain why in `note` \
                 instead of inventing symbols (AR-11-34)"
            );
            continue;
        }
        let files = module["forbidden_scan_files"]
            .as_array()
            .unwrap_or_else(|| panic!("module {id} must declare forbidden_scan_files (AR-11-34)"));
        assert!(
            !files.is_empty(),
            "module {id} must list at least one forbidden_scan_file"
        );
        let declared_sources: BTreeSet<&str> = module["source_files"]
            .as_array()
            .expect("ownership manifest module must declare source_files")
            .iter()
            .map(|file| {
                file.as_str()
                    .expect("ownership manifest source file must be a string")
            })
            .collect();
        let mut texts = Vec::new();
        for file in files {
            let path = file
                .as_str()
                .expect("ownership manifest forbidden scan file must be a string");
            assert!(
                declared_sources.contains(path),
                "module {id} forbidden_scan_file {path} must be one of its source_files"
            );
            texts.push(repository_text(path));
        }
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        for symbol in &symbols {
            let symbol = symbol
                .as_str()
                .expect("ownership manifest forbidden symbol must be a string");
            assert!(
                is_valid_symbol(symbol),
                "module {id} forbidden symbol must not be empty"
            );
            assert!(
                forbidden_absent(symbol, &refs),
                "module {id} must not reference forbidden symbol {symbol} in its forbidden_scan_files"
            );
        }
        if let Some(required) = required_pins.remove(id) {
            let listed: BTreeSet<&str> = symbols
                .iter()
                .map(|symbol| {
                    symbol
                        .as_str()
                        .expect("ownership manifest forbidden symbol must be a string")
                })
                .collect();
            for symbol in required {
                assert!(
                    listed.contains(symbol),
                    "module {id} must pin required forbidden symbol {symbol} (AR-11-34)"
                );
            }
        }
    }
    assert!(
        required_pins.is_empty(),
        "required forbidden pins must all be present in the manifest, missing: {required_pins:?}"
    );

    // (c) Liveness: every forbidden symbol occurs somewhere in
    // rust/pokecon/src outside its own forbidden_scan_files, so a symbol typo
    // cannot make the absence check vacuous.
    let root = repository_root();
    let mut rust_sources: Vec<(String, String)> = Vec::new();
    let mut directories = vec![root.join("rust/pokecon/src")];
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory).unwrap_or_else(|error| {
            panic!(
                "rust source directory {} must be readable: {error}",
                directory.display()
            )
        });
        for entry in entries {
            let entry = entry.expect("rust source directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path
                    .strip_prefix(root)
                    .expect("rust source path must be below the repository root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!("rust source {} must be readable: {error}", path.display())
                });
                rust_sources.push((relative, text));
            }
        }
    }
    assert!(
        !rust_sources.is_empty(),
        "rust/pokecon/src must contain at least one Rust source file"
    );
    for module in modules {
        let id = module["id"]
            .as_str()
            .expect("ownership manifest module must have a string id");
        let symbols = module["forbidden_symbols"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if symbols.is_empty() {
            continue;
        }
        let scanned: BTreeSet<&str> = module["forbidden_scan_files"]
            .as_array()
            .expect("module with forbidden symbols must declare forbidden_scan_files")
            .iter()
            .map(|file| {
                file.as_str()
                    .expect("ownership manifest forbidden scan file must be a string")
            })
            .collect();
        for symbol in &symbols {
            let symbol = symbol
                .as_str()
                .expect("ownership manifest forbidden symbol must be a string");
            let live = rust_sources.iter().any(|(relative, text)| {
                !scanned.contains(relative.as_str()) && text.contains(symbol)
            });
            assert!(
                live,
                "forbidden symbol {symbol} of module {id} must occur somewhere in \
                 rust/pokecon/src outside its forbidden_scan_files (liveness, AR-11-34)"
            );
        }
    }
}

static DEPENDENCY_USE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"use crate::([a-z0-9_]+)").expect("dependency edge regex must compile")
});

/// AR-11-35/AR-11-36 shared extractor: first-segment capture of a
/// `use crate::<top>::...` line. Both the allowed-manifest test and the
/// forbidden-edge test call this same function so the two scans cannot drift
/// apart.
fn extract_dependency_top_module(line: &str) -> Option<String> {
    DEPENDENCY_USE_PATTERN
        .captures(line)
        .map(|captures| captures[1].to_owned())
}

/// AR-11-35/AR-11-36 shared source scan: fs-walk of
/// rust/pokecon/src/**/*.rs, one captured edge per `use crate::<top>` line.
/// The importer top module is the first path segment below src/; root files
/// (e.g. production.rs) map via file stem (production). Self-edges (e.g.
/// `use crate::worker::...` inside worker/) are excluded.
fn scan_source_dependency_edges() -> (BTreeSet<(String, String)>, usize) {
    let src_root = repository_root().join("rust/pokecon/src");
    let mut observed = BTreeSet::new();
    let mut scanned = 0;
    let mut stack = vec![src_root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).unwrap_or_else(|error| {
            panic!("contract input {} must be readable: {error}", dir.display())
        });
        for entry in entries {
            let entry = entry.expect("contract input directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs")) {
                scanned += 1;
                let relative = path
                    .strip_prefix(repository_root())
                    .expect("contract input must be below the repository root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let under_src = path
                    .strip_prefix(&src_root)
                    .expect("contract input must be below rust/pokecon/src");
                let mut components = under_src.components();
                let first = components
                    .next()
                    .expect("source path below src/ must be non-empty");
                let importer = if components.next().is_none() {
                    path.file_stem()
                        .and_then(|stem| stem.to_str())
                        .expect("root source file must have a UTF-8 stem")
                        .to_owned()
                } else {
                    first
                        .as_os_str()
                        .to_str()
                        .expect("source directory name must be UTF-8")
                        .to_owned()
                };
                let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!("contract input {relative} must be readable: {error}")
                });
                for line in text.lines() {
                    if let Some(top) = extract_dependency_top_module(line)
                        && top != importer
                    {
                        observed.insert((importer.clone(), top));
                    }
                }
            }
        }
    }
    (observed, scanned)
}

/// AR-11-36 shared forbidden-membership predicate: an edge is rejected if and
/// only if it is a member of the manifest `forbidden` set. The real-tree scan
/// check, the fixture positive control, and the sentinel controls below all
/// call this same predicate.
fn is_forbidden_dependency_edge(
    edge: &(String, String),
    forbidden: &BTreeSet<(String, String)>,
) -> bool {
    forbidden.contains(edge)
}

#[allow(clippy::too_many_lines)]
#[test]
fn source_dependency_edges_match_allowed_manifest() {
    // AR-11-35: 内部依存方向の許可 edge manifest
    // (rust/pokecon/registry/dependency_edges.json) と
    // rust/pokecon/src/**/*.rs の `use crate::<top>::...` 行から抽出した実測 edge 集合が
    // 双方向に一致することを検査する。未知 edge (manifest 漏れ) と stale entry
    // (実装に存在しない許可) のいずれもこの test を失敗させる。
    //
    // 誠実な範囲限定 (scope):
    // (1) `use` 行の文字列走査のみ — インライン完全修飾パス
    //     (例: lib.rs の `crate::platform::PlatformKind`)、
    //     `use crate::{...}` の brace-group import、
    //     include!/macro 生成参照は捕捉しない。
    // (2) `#[cfg(test)]` 末尾を含む全文走査 (test-only の use も edge になる)。
    // (3) Cargo package 境界の検査は対象外 (AR-10.8-05)。
    //     single-package 境界は Cargo.toml:1-8 (members = ["rust/pokecon"])。
    static DEPENDENCY_EDGES_JSON: LazyLock<String> =
        LazyLock::new(|| repository_text("rust/pokecon/registry/dependency_edges.json"));
    let manifest = parse_json(&DEPENDENCY_EDGES_JSON);
    assert_eq!(
        manifest["schema_version"], 1,
        "dependency edge manifest schema_version must be 1 (AR-11-35)"
    );
    let allowed = manifest["allowed"]
        .as_array()
        .expect("dependency edge manifest must define an allowed array");
    assert!(
        !allowed.is_empty(),
        "dependency edge manifest must allow at least one edge"
    );
    let mut allowed_set = BTreeSet::new();
    let mut previous: Option<(String, String)> = None;
    for edge in allowed {
        let pair = edge
            .as_array()
            .unwrap_or_else(|| panic!("manifest edge must be a [from, to] pair, found {edge}"));
        assert_eq!(
            pair.len(),
            2,
            "manifest edge must be a [from, to] pair, found {edge}"
        );
        let from = pair[0]
            .as_str()
            .expect("manifest edge from must be a string")
            .to_owned();
        let to = pair[1]
            .as_str()
            .expect("manifest edge to must be a string")
            .to_owned();
        assert!(
            !from.is_empty() && !to.is_empty(),
            "manifest edge modules must not be empty, found [{from}, {to}]"
        );
        assert_ne!(
            from, to,
            "manifest must not list self-edges, found {from} -> {to} (AR-11-35)"
        );
        assert!(
            allowed_set.insert((from.clone(), to.clone())),
            "manifest edges must be unique, duplicate {from} -> {to} (AR-11-35)"
        );
        if let Some(previous_edge) = &previous {
            assert!(
                *previous_edge < (from.clone(), to.clone()),
                "manifest edges must be sorted, {from} -> {to} follows {previous_edge:?} (AR-11-35)"
            );
        }
        previous = Some((from, to));
    }

    // (a) Fail-closed sentinel control: the exact extraction predicate used
    // below must detect a synthetic `use crate::sentinel_module::X` line and
    // must not match a plain non-crate import, so a broken or empty scanner
    // cannot pass this test vacuously.
    assert_eq!(
        extract_dependency_top_module("use crate::sentinel_module::X;"),
        Some("sentinel_module".to_owned()),
        "AR-11-35 scanner control must detect injected `use crate::sentinel_module::X`"
    );
    assert_eq!(
        extract_dependency_top_module("use std::collections::BTreeSet;"),
        None,
        "AR-11-35 scanner control must not match a non-crate import"
    );

    // Observed edge set via the AR-11-35/AR-11-36 shared scan (fs-walk of
    // rust/pokecon/src/**/*.rs, importer mapping and self-edge exclusion as
    // documented on `scan_source_dependency_edges`).
    let (observed, scanned) = scan_source_dependency_edges();
    let src_root = repository_root().join("rust/pokecon/src");
    assert!(
        scanned > 100,
        "dependency edge scan must cover rust/pokecon/src (scanned {scanned} files)"
    );

    // §9.1 drift guard: the allowed-direction table keeps exactly 6 data rows,
    // and every manifest `from` is discoverable in the section text.
    // 連動検査の意味: §9.1 テーブルは composition root／backend／transport／
    // supervisor／frontend／tool の6概念しか列挙しない coarse な語彙であり、
    // 実装の全 top module (desktop・test 支援・service 群など) を網羅しない。
    // よって各 manifest `from` は (a) 正規化 (小文字化・`_`除去) 後に §9.1 節
    // テキストの部分文字列として発見できるか、(b) rust/pokecon/src/ 直下に同名の
    // 実在 top module (dir または .rs、cfg-gated を含む) に対応すること。
    // どちらにも該当しない from は文書にも実装にも存在しない未知 module として
    // 失敗させ、manifest の typo を検出する。
    let handoff = repository_text("docs/ARCHITECTURE_HANDOFF.md");
    let section = handoff
        .split_once("### 9.1 許可する方向")
        .expect("ARCHITECTURE_HANDOFF.md must define the 9.1 allowed-direction section")
        .1;
    let section = section
        .split_once("\n### ")
        .map_or(section, |(head, _)| head);
    let rows: Vec<&str> = section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .filter(|line| !line.contains("allowed dependency"))
        .filter(|line| !line.contains("---"))
        .collect();
    assert_eq!(
        rows.len(),
        6,
        "9.1 allowed-direction table must keep exactly 6 data rows, found: {rows:?}"
    );
    let normalized_section = section.to_lowercase().replace('_', "");
    let mut from_modules = BTreeSet::new();
    for (from, _) in &allowed_set {
        from_modules.insert(from.clone());
    }
    for from in &from_modules {
        let normalized = from.to_lowercase().replace('_', "");
        if normalized_section.contains(&normalized) {
            continue;
        }
        assert!(
            src_root.join(from).is_dir() || src_root.join(format!("{from}.rs")).is_file(),
            "manifest from {from} must be discoverable in the 9.1 section text \
             or name a real top module under rust/pokecon/src (AR-11-35)"
        );
    }

    // Both-direction diff: unknown observed edges and stale manifest entries
    // each fail, the stale-entry liveness following the same pattern as the
    // camera ownership test.
    let unknown: Vec<(&String, &String)> = observed
        .difference(&allowed_set)
        .map(|(from, to)| (from, to))
        .collect();
    assert!(
        unknown.is_empty(),
        "unknown dependency edges missing from the manifest (AR-11-35): {unknown:?}"
    );
    let stale: Vec<(&String, &String)> = allowed_set
        .difference(&observed)
        .map(|(from, to)| (from, to))
        .collect();
    assert!(
        stale.is_empty(),
        "stale manifest entries with no observed edge; update the manifest instead of \
         keeping a dead allowance (AR-11-35): {stale:?}"
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn forbidden_dependency_edges_are_rejected() {
    // AR-11-36: FORBIDDEN 方向禁止 edge manifest
    // (rust/pokecon/registry/dependency_edges.json の `forbidden` 配列) に
    // 対する検査。実測 edge 集合に禁止 edge が1つも含まれないこと、
    // 意図的な逆流 fixture が同じ抽出器・同じ述語で拒否されること、
    // fail-closed の番兵制御、lib→production 例外の liveness を検査する。
    //
    // 誠実な範囲限定 (scope): `use` 行の文字列走査のみであり、
    // brace-group import (`use crate::{...}`) やインライン完全修飾パスは
    // 捕捉しない。fixture は file-as-text の positive control であり、
    // Cargo target としてはコンパイルされない。§9.2 全行の被覆は主張しない。
    static FORBIDDEN_EDGES_JSON: LazyLock<String> =
        LazyLock::new(|| repository_text("rust/pokecon/registry/dependency_edges.json"));
    let manifest = parse_json(&FORBIDDEN_EDGES_JSON);
    assert_eq!(
        manifest["schema_version"], 1,
        "dependency edge manifest schema_version must be 1 (AR-11-36)"
    );
    let forbidden = manifest["forbidden"]
        .as_array()
        .expect("dependency edge manifest must define a forbidden array (AR-11-36)");
    assert!(
        !forbidden.is_empty(),
        "forbidden edge list must be non-empty so the check cannot pass vacuously (AR-11-36)"
    );
    let mut forbidden_set = BTreeSet::new();
    let mut previous: Option<(String, String)> = None;
    for edge in forbidden {
        let pair = edge
            .as_array()
            .unwrap_or_else(|| panic!("forbidden edge must be a [from, to] pair, found {edge}"));
        assert_eq!(
            pair.len(),
            2,
            "forbidden edge must be a [from, to] pair, found {edge}"
        );
        let from = pair[0]
            .as_str()
            .expect("forbidden edge from must be a string")
            .to_owned();
        let to = pair[1]
            .as_str()
            .expect("forbidden edge to must be a string")
            .to_owned();
        assert!(
            !from.is_empty() && !to.is_empty(),
            "forbidden edge modules must not be empty, found [{from}, {to}]"
        );
        assert_ne!(
            from, to,
            "forbidden manifest must not list self-edges, found {from} -> {to} (AR-11-36)"
        );
        assert!(
            forbidden_set.insert((from.clone(), to.clone())),
            "forbidden edges must be unique, duplicate {from} -> {to} (AR-11-36)"
        );
        if let Some(previous_edge) = &previous {
            assert!(
                *previous_edge < (from.clone(), to.clone()),
                "forbidden edges must be sorted, {from} -> {to} follows {previous_edge:?} (AR-11-36)"
            );
        }
        previous = Some((from, to));
    }

    // Policy pin: the reviewed forbidden policy is the exact 27-pair set
    // below. A silent deletion, addition, or swap in the manifest fails here;
    // intentional policy changes update the manifest and this expected set
    // together.
    let lower_layers = [
        "device",
        "camera",
        "worker",
        "runtime",
        "server",
        "settings",
        "dynamic",
        "dynamic_host",
        "dynamic_runtime",
        "script_host",
        "script_runtime",
    ];
    let mut expected_forbidden = BTreeSet::new();
    for from in lower_layers {
        for to in ["production", "entrypoint"] {
            expected_forbidden.insert((from.to_owned(), to.to_owned()));
        }
    }
    for from in ["device", "camera", "runtime"] {
        expected_forbidden.insert((from.to_owned(), "server".to_owned()));
    }
    expected_forbidden.insert(("worker".to_owned(), "device".to_owned()));
    expected_forbidden.insert(("worker".to_owned(), "server".to_owned()));
    assert_eq!(
        forbidden_set, expected_forbidden,
        "forbidden policy must match the reviewed 27-pair set exactly (AR-11-36)"
    );

    // (d) Exception liveness: lib.rs keeps the composition-root reference to
    // production, which is the documented manifest exception. If this line is
    // removed, the manifest note must be updated, so fail here first.
    let lib = repository_text("rust/pokecon/src/lib.rs");
    assert!(
        lib.contains("use crate::production::ProductionRuntime"),
        "lib.rs must keep the documented `use crate::production::ProductionRuntime` \
         exception (AR-11-36); if it was removed, update the manifest note"
    );

    // (a) Real-tree scan with the SAME extractor as AR-11-35: no observed
    // edge may be a member of the forbidden set.
    let (observed, scanned) = scan_source_dependency_edges();
    assert!(
        scanned > 100,
        "forbidden edge scan must cover rust/pokecon/src (scanned {scanned} files)"
    );
    let violations: Vec<&(String, String)> = observed
        .iter()
        .filter(|edge| is_forbidden_dependency_edge(edge, &forbidden_set))
        .collect();
    assert!(
        violations.is_empty(),
        "forbidden dependency edges observed in rust/pokecon/src (AR-11-36): {violations:?}"
    );

    // (b) Fixture positive control: the intentional reverse-flow fixture is
    // read as text and run through the SAME extractor and the SAME
    // forbidden-membership predicate. The fixture models a worker-context
    // file, so its extracted top module pairs with the `worker` importer.
    let fixture = repository_text("rust/pokecon/tests/fixtures/forbidden_dependency_fixture.rs");
    assert!(
        fixture.contains("AR-11-36 intentional reverse-flow fixture"),
        "forbidden fixture must carry its AR-11-36 header marker"
    );
    let mut fixture_edges = BTreeSet::new();
    for line in fixture.lines() {
        if let Some(top) = extract_dependency_top_module(line) {
            fixture_edges.insert(("worker".to_owned(), top));
        }
    }
    let fixture_edge = ("worker".to_owned(), "server".to_owned());
    assert!(
        fixture_edges.contains(&fixture_edge),
        "fixture must model the worker -> server reverse flow, found {fixture_edges:?} (AR-11-36)"
    );
    assert!(
        is_forbidden_dependency_edge(&fixture_edge, &forbidden_set),
        "fixture edge worker -> server must be REJECTED by the forbidden manifest (AR-11-36)"
    );

    // (c) Fail-closed sentinel controls with the SAME predicate: a synthetic
    // worker-context `use crate::production::X` line must be detected as
    // forbidden, while a synthetic allowed edge must NOT be rejected.
    assert_eq!(
        extract_dependency_top_module("use crate::production::X;"),
        Some("production".to_owned()),
        "AR-11-36 scanner control must detect injected `use crate::production::X`"
    );
    assert!(
        is_forbidden_dependency_edge(
            &("worker".to_owned(), "production".to_owned()),
            &forbidden_set
        ),
        "synthetic worker -> production reverse flow must be rejected (AR-11-36 control)"
    );
    assert!(
        !is_forbidden_dependency_edge(&("server".to_owned(), "camera".to_owned()), &forbidden_set),
        "synthetic allowed edge server -> camera must NOT be rejected (AR-11-36 control)"
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn worker_ipc_deployment_boundary_is_pinned() {
    // AR-11-37: worker IPC の deployment-boundary pin
    // (rust/pokecon/registry/ipc_boundary.json)。envelope kind・closed 値集合・
    // 最大 payload・非公開 wire marker・非公開 payload fixture case 名を固定し、
    // 実装 source との drift を検査する。
    //
    // 誠実な範囲限定 (scope): op 名の allowlist、IPC payload 全体の corpus、
    // プロセス内 (encode 側) 拒否は主張しない。拒否は peer の decode 層で行われ、
    // 同一プロセス内の呼び出し元は任意の IpcValue を構築できる。
    static IPC_BOUNDARY_JSON: LazyLock<String> =
        LazyLock::new(|| repository_text("rust/pokecon/registry/ipc_boundary.json"));
    let manifest = parse_json(&IPC_BOUNDARY_JSON);
    assert_eq!(
        manifest["schema_version"], 1,
        "IPC boundary manifest schema_version must be 1 (AR-11-37)"
    );

    // (a) Manifest exact-pin: envelope kind・closed 値・最大 payload・非公開
    // marker・非公開 case 名のいずれの無言の変更もここで失敗させる。意図的な
    // 変更は manifest とこの期待集合を同時に更新する (AR-11-36 policy pin 流儀)。
    let envelope_kinds = manifest["envelope_kinds"]
        .as_array()
        .expect("IPC boundary manifest must define an envelope_kinds array")
        .iter()
        .map(|kind| {
            kind.as_str()
                .expect("IPC boundary envelope kind must be a string")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        envelope_kinds,
        BTreeSet::from(["request", "response", "error", "event", "log"]),
        "envelope kinds must match the reviewed 5-kind set exactly (AR-11-37)"
    );
    let closed_values = manifest["closed_values"]
        .as_array()
        .expect("IPC boundary manifest must define a closed_values array")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("IPC boundary closed value must be a string")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        closed_values,
        BTreeSet::from([
            "nil", "bool", "int", "uint", "float", "str", "bin", "array", "map_str"
        ]),
        "closed values must match the reviewed 9-variant set exactly (AR-11-37)"
    );
    assert_eq!(
        manifest["maximum_payload_bytes"], 1_048_576,
        "maximum payload must be 1048576 bytes (AR-11-37)"
    );
    let forbidden_markers = manifest["forbidden_wire_markers"]
        .as_array()
        .expect("IPC boundary manifest must define a forbidden_wire_markers array")
        .iter()
        .map(|marker| {
            marker
                .as_str()
                .expect("IPC boundary forbidden wire marker must be a string")
        })
        .collect::<BTreeSet<_>>();
    assert!(
        !forbidden_markers.is_empty(),
        "forbidden wire markers must be non-empty so the pin cannot pass vacuously (AR-11-37)"
    );
    assert_eq!(
        forbidden_markers,
        BTreeSet::from([
            "lua_value",
            "native_handle",
            "python_object",
            "rust_arc",
            "socket",
            "token"
        ]),
        "forbidden wire markers must match the reviewed set exactly (AR-11-37)"
    );
    let nonpublic_cases = manifest["nonpublic_cases"]
        .as_array()
        .expect("IPC boundary manifest must define a nonpublic_cases array")
        .iter()
        .map(|case| {
            case.as_str()
                .expect("IPC boundary non-public case must be a string")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        nonpublic_cases,
        BTreeSet::from([
            "empty_op_string",
            "nested_ext_in_map_value",
            "non_string_map_key",
            "top_level_fixext4_timestamp",
            "trailing_second_object",
            "unknown_envelope_field",
            "unknown_error_payload_field"
        ]),
        "non-public cases must match the reviewed 7-case set exactly (AR-11-37)"
    );

    // (b) Source drift: manifest の各値が実 source に対応すること。
    let schema = repository_text("rust/pokecon/src/worker/ipc/schema.rs");
    for (short, declaration) in [
        ("nil", "    Nil,"),
        ("bool", "    Bool(bool),"),
        ("int", "    Integer(i64),"),
        ("uint", "    Unsigned(u64),"),
        ("float", "    Float(f64),"),
        ("str", "    String(String),"),
        ("bin", "    Binary(Vec<u8>),"),
        ("array", "    Array(Vec<Self>),"),
        ("map_str", "    Map(BTreeMap<String, Self>),"),
    ] {
        assert!(
            closed_values.contains(short),
            "closed value {short} must be pinned in the manifest (AR-11-37)"
        );
        assert!(
            schema.contains(declaration),
            "schema.rs must declare the IpcValue variant {declaration} for closed value {short} (AR-11-37)"
        );
    }
    let codec = repository_text("rust/pokecon/src/worker/ipc/codec.rs");
    assert!(
        codec.contains("pub const MAX_PAYLOAD_BYTES: usize = 1_048_576;"),
        "codec.rs must pin MAX_PAYLOAD_BYTES to the manifest value (AR-11-37)"
    );
    let protocol = parse_json(&repository_text("rust/pokecon/registry/protocol.json"));
    let protocol_kinds = protocol["ipc"]["kinds"]
        .as_array()
        .expect("protocol registry must define ipc.kinds")
        .iter()
        .map(|kind| {
            kind.as_str()
                .expect("protocol registry ipc kind must be a string")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        protocol_kinds, envelope_kinds,
        "protocol.json ipc.kinds must match the manifest envelope kinds (AR-11-37)"
    );

    // (c) Fixture liveness: manifest の各非公開 case 名が fixture 内に実在し、
    // 1:1 に対応すること (typo で検査が空回りしないこと)。
    let fixture = repository_text("rust/pokecon/tests/fixtures/ipc_nonpublic_payloads.rs");
    assert!(
        fixture.contains("AR-11-37 intentional non-public wire fixture"),
        "non-public fixture must carry its AR-11-37 header marker"
    );
    assert!(
        fixture.contains("pub const CASES"),
        "non-public fixture must define the pub const CASES array (AR-11-37)"
    );
    for name in &nonpublic_cases {
        assert!(
            fixture.contains(*name),
            "non-public case {name} must appear in the fixture 1:1 without typos (AR-11-37)"
        );
    }

    // (d) Fail-closed sentinel controls: 空名の拒否と未知 case の検出。
    let is_valid_case_name = |name: &str| !name.trim().is_empty();
    assert!(
        !is_valid_case_name(""),
        "empty case name must be rejected (AR-11-37 control)"
    );
    assert!(
        is_valid_case_name("empty_op_string"),
        "pinned case name must count as valid (AR-11-37 control)"
    );
    assert!(
        !nonpublic_cases.contains("AR-11-37-sentinel-unknown-case"),
        "unknown case must not match the pinned set (AR-11-37 control)"
    );
    assert!(
        nonpublic_cases.contains("empty_op_string"),
        "pinned case must be found in the set (AR-11-37 control)"
    );
}

static STRONG_BOUNDARIES_JSON: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/registry/strong_boundaries.json"));
static ABSTRACTION_BASELINE_JSON: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/registry/abstraction_baseline.json"));
static POKECON_MANIFEST: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/Cargo.toml"));
static CONTRACT_SYNC_SOURCE: LazyLock<String> =
    LazyLock::new(|| repository_text("rust/pokecon/tests/contract_sync.rs"));

/// AR-10.8-04 shared predicate: a necessity entry is one non-empty line.
fn is_valid_necessity(text: &str) -> bool {
    !text.trim().is_empty() && !text.contains('\n')
}

/// AR-10.8-04 shared helper: split a `path[:NN[-MM]]` evidence anchor into
/// its file path and optional first line number.
fn split_source_anchor(anchor: &str) -> (String, Option<usize>) {
    if let Some((head, tail)) = anchor.rsplit_once(':') {
        let first = tail.split('-').next().unwrap_or(tail);
        if let Ok(line) = first.parse::<usize>() {
            return (head.to_owned(), Some(line));
        }
    }
    (anchor.to_owned(), None)
}

/// AR-10.8-04 shared predicate: a `check_ref` is live when it names either a
/// `contract_sync` test function or a flake task/check name.
fn check_ref_is_live(name: &str, contract_sync_text: &str, flake_text: &str) -> bool {
    contract_sync_text.contains(&format!("fn {name}")) || flake_text.contains(name)
}

/// AR-10.8-04 shared predicate: a binary name is covered when the packaged
/// bin set contains it.
fn bin_is_covered(name: &str, packaged: &BTreeSet<String>) -> bool {
    packaged.contains(name)
}

#[allow(clippy::too_many_lines)]
#[test]
fn strong_boundaries_have_necessity_and_evidence() {
    // AR-10.8-04: 強い境界ごとの必要性／信頼／配布根拠表
    // (rust/pokecon/registry/strong_boundaries.json) の pin。各境界が一行の
    // 非空 necessity と実在する証拠 (source／registry／check 参照) を持ち、
    // 実装走査の closed-world marker がいずれも根拠付き境界に被覆されること
    // を検査する。未知の marker class や根拠のない境界はここで失敗させる。
    //
    // 誠実な範囲限定 (scope):
    // (1) source_refs の行番号は存在範囲の検査に留める (1 <= NN <= 総行数)。
    //     行内容の一致までは主張しない — 内容側の liveness は symbol_anchors
    //     の正規表現が担う (brittle な行内容 pin を避けるため)。
    // (2) test-only bin (fault fixture) は closed-world の対象外とし、path で
    //     明示除外する。macro 生成コードは走査しない。
    // (3) check_refs は contract_sync test 名または flake task/check 名の
    //     いずれかへの解決を要求する。
    let manifest = parse_json(&STRONG_BOUNDARIES_JSON);
    assert_eq!(
        manifest["schema_version"], 1,
        "strong boundary manifest schema_version must be 1 (AR-10.8-04)"
    );
    let boundaries = manifest["boundaries"]
        .as_array()
        .expect("strong boundary manifest must define a boundaries array");
    assert!(
        !boundaries.is_empty(),
        "strong boundary manifest must pin at least one boundary"
    );

    // Fail-closed sentinel controls on the exact predicates below.
    assert!(
        !is_valid_necessity(""),
        "empty necessity must be rejected (AR-10.8-04 control)"
    );
    assert!(
        !is_valid_necessity("first line\nsecond line"),
        "multi-line necessity must be rejected (AR-10.8-04 control)"
    );
    assert!(
        is_valid_necessity("one line of necessity"),
        "single-line necessity must count as valid (AR-10.8-04 control)"
    );
    let (anchor_path, anchor_line) = split_source_anchor("rust/pokecon/src/lib.rs:41-43");
    assert_eq!(anchor_path, "rust/pokecon/src/lib.rs");
    assert_eq!(anchor_line, Some(41));
    let (bare_path, bare_line) = split_source_anchor("docs/ARTIFACT_MANIFEST.md");
    assert_eq!(bare_path, "docs/ARTIFACT_MANIFEST.md");
    assert_eq!(bare_line, None);

    // Policy pin: the reviewed boundary set is exactly these five ids.
    // Intentional additions update the manifest and this set together.
    let expected_ids = BTreeSet::from([
        "worker-process",
        "ipc-wire-value",
        "lan-http-trust",
        "os-platform-conditional",
        "os-bundle-distribution",
    ]);
    let mut ids = BTreeSet::new();
    for boundary in boundaries {
        let id = boundary["id"]
            .as_str()
            .expect("strong boundary must have a string id");
        assert!(
            ids.insert(id.to_owned()),
            "strong boundary id {id} must be unique"
        );
        for field in ["kind", "necessity", "trust", "distribution"] {
            let text = boundary[field]
                .as_str()
                .unwrap_or_else(|| panic!("boundary {id} must declare {field} (AR-10.8-04)"));
            assert!(
                is_valid_necessity(text),
                "boundary {id} field {field} must be one non-empty line"
            );
        }
        let evidence = &boundary["evidence"];
        let source_refs = evidence["source_refs"]
            .as_array()
            .unwrap_or_else(|| panic!("boundary {id} must declare evidence.source_refs"));
        assert!(
            !source_refs.is_empty(),
            "boundary {id} must list at least one evidence source_ref"
        );
        let mut evidence_texts = Vec::new();
        for source_ref in source_refs {
            let anchor = source_ref
                .as_str()
                .expect("evidence source_ref must be a string");
            let (path, line) = split_source_anchor(anchor);
            let text = repository_text(&path);
            evidence_texts.push(text);
            if let Some(line) = line {
                let total = evidence_texts
                    .last()
                    .expect("evidence text must exist")
                    .lines()
                    .count();
                assert!(
                    line >= 1 && line <= total,
                    "evidence anchor {anchor} line {line} must resolve within {path} ({total} lines)"
                );
            }
        }
        let joined = evidence_texts.join("\n");
        for symbol in evidence["symbol_anchors"]
            .as_array()
            .unwrap_or_else(|| panic!("boundary {id} must declare evidence.symbol_anchors"))
        {
            let symbol = symbol
                .as_str()
                .expect("evidence symbol_anchor must be a string");
            assert!(
                !symbol.trim().is_empty(),
                "boundary {id} symbol_anchor must not be empty"
            );
            assert!(
                joined.contains(symbol),
                "symbol anchor {symbol} of boundary {id} must occur in its evidence sources"
            );
        }
        for registry_ref in evidence["registry_refs"]
            .as_array()
            .unwrap_or_else(|| panic!("boundary {id} must declare evidence.registry_refs"))
        {
            let path = registry_ref
                .as_str()
                .expect("evidence registry_ref must be a string");
            repository_text(path);
        }
        let contract_sync_text = CONTRACT_SYNC_SOURCE.clone();
        for check_ref in evidence["check_refs"]
            .as_array()
            .unwrap_or_else(|| panic!("boundary {id} must declare evidence.check_refs"))
        {
            let name = check_ref
                .as_str()
                .expect("evidence check_ref must be a string");
            assert!(
                check_ref_is_live(name, &contract_sync_text, &FLAKE),
                "check ref {name} of boundary {id} must name a contract_sync test or a flake task"
            );
        }
    }
    assert_eq!(
        ids,
        expected_ids
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>(),
        "strong boundary ids must match the reviewed five-boundary set exactly (AR-10.8-04)"
    );
    assert!(
        !check_ref_is_live(
            "ar-10-8-04-sentinel-missing-check",
            &CONTRACT_SYNC_SOURCE,
            &FLAKE
        ),
        "unknown check ref must not resolve (AR-10.8-04 control)"
    );

    // Non-boundaries: the closed world is explicit — these must stay a plain
    // crate-internal or retired surface, never a justified boundary.
    let non_boundaries = manifest["non_boundaries"]
        .as_array()
        .expect("strong boundary manifest must define a non_boundaries array");
    let non_ids: BTreeSet<&str> = non_boundaries
        .iter()
        .map(|item| item.as_str().expect("non_boundary must be a string"))
        .collect();
    assert_eq!(
        non_ids,
        BTreeSet::from([
            "separate-crate",
            "public-rust-api",
            "server-pub-mod",
            "python-cdylib"
        ]),
        "non-boundary set must match the reviewed four-item set exactly (AR-10.8-04)"
    );

    // Closed world: every strong-boundary marker class observed in the
    // implementation is covered by a justified boundary entry.
    let markers: [(&str, &str, &str); 10] = [
        (
            "worker binary packaged",
            "rust/pokecon/Cargo.toml",
            "name = \"pokecon-worker\"",
        ),
        (
            "worker spawn constructor",
            "rust/pokecon/src/dynamic_runtime.rs",
            "WorkerLaunch::managed",
        ),
        (
            "worker supervisor",
            "rust/pokecon/src/worker/supervisor.rs",
            "WorkerSupervisor",
        ),
        (
            "os-conditional code",
            "rust/pokecon/src/device/serial/selector.rs",
            "cfg(target_os",
        ),
        (
            "windows notification",
            "rust/pokecon/src/device/notification.rs",
            "show_windows_toast",
        ),
        (
            "shutdown unlink gate",
            "rust/pokecon/src/camera/shared_ring.rs",
            "unlink_name_for_shutdown",
        ),
        (
            "lan trust enforcement",
            "rust/pokecon/src/server/security.rs",
            "enforce_security",
        ),
        (
            "ipc payload ceiling",
            "rust/pokecon/src/worker/ipc/codec.rs",
            "MAX_PAYLOAD_BYTES",
        ),
        (
            "distribution manifest",
            "docs/ARTIFACT_MANIFEST.md",
            "dist/tauri",
        ),
        ("signing provenance", "flake.nix", "signing-input-manifest"),
    ];
    let cover: BTreeSet<(&str, &str)> = [
        ("name = \"pokecon-worker\"", "worker-process"),
        ("WorkerLaunch::managed", "worker-process"),
        ("WorkerSupervisor", "worker-process"),
        ("cfg(target_os", "os-platform-conditional"),
        ("show_windows_toast", "os-platform-conditional"),
        ("unlink_name_for_shutdown", "os-platform-conditional"),
        ("enforce_security", "lan-http-trust"),
        ("MAX_PAYLOAD_BYTES", "ipc-wire-value"),
        ("dist/tauri", "os-bundle-distribution"),
        ("signing-input-manifest", "os-bundle-distribution"),
    ]
    .into_iter()
    .collect();
    for (label, file, needle) in markers {
        let text = repository_text(file);
        assert!(
            text.contains(needle),
            "closed-world marker {label} ({needle} in {file}) must stay live (AR-10.8-04)"
        );
        let (covered, boundary) = cover
            .iter()
            .find(|(marker, _)| *marker == needle)
            .expect("closed-world marker must declare its covering boundary");
        assert!(
            ids.contains(*boundary),
            "closed-world marker {label} ({covered}) must be covered by justified boundary {boundary}"
        );
    }

    // Fail-closed bin control: the packaged bin set covers pokecon-worker,
    // while a synthetic unlisted bin is uncovered.
    let packaged: BTreeSet<String> = parse_pokecon_bin_names(&POKECON_MANIFEST)
        .into_iter()
        .collect();
    assert!(
        bin_is_covered("pokecon-worker", &packaged),
        "packaged bins must cover pokecon-worker (AR-10.8-04 control)"
    );
    assert!(
        !bin_is_covered("pokecon-sentinel-extra-bin", &packaged),
        "synthetic unlisted bin must NOT be covered (AR-10.8-04 control)"
    );
}

/// AR-10.8-05 shared helper: names of every `[[bin]]` section.
fn parse_pokecon_bin_names(manifest: &str) -> Vec<String> {
    parse_pokecon_bins(manifest)
        .into_iter()
        .map(|(name, _, _)| name)
        .collect()
}

/// AR-10.8-05 shared helper: `(name, path, required_features)` per `[[bin]]`.
fn parse_pokecon_bins(manifest: &str) -> Vec<(String, String, Vec<String>)> {
    static BIN_NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"name\s*=\s*"([^"]+)""#).expect("bin name regex must compile")
    });
    static BIN_PATH: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"path\s*=\s*"([^"]+)""#).expect("bin path regex must compile")
    });
    let mut bins = Vec::new();
    for section in manifest.split("[[bin]]").skip(1) {
        let section = section.split("[[test]]").next().unwrap_or(section);
        let section = section.split("[[bench]]").next().unwrap_or(section);
        let section = section.split("\n[features]").next().unwrap_or(section);
        let name = BIN_NAME
            .captures(section)
            .unwrap_or_else(|| panic!("[[bin]] section must declare a name: {section}"))
            .get(1)
            .expect("bin name capture must exist")
            .as_str()
            .to_owned();
        let path = BIN_PATH
            .captures(section)
            .unwrap_or_else(|| panic!("[[bin]] {name} must declare a path"))
            .get(1)
            .expect("bin path capture must exist")
            .as_str()
            .to_owned();
        let features = section
            .find("required-features")
            .map_or_else(Vec::new, |start| {
                let tail = &section[start..];
                let end = tail
                    .find(']')
                    .unwrap_or_else(|| panic!("[[bin]] {name} features must close"));
                tail[..end]
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .map(ToString::to_string)
                    .collect()
            });
        bins.push((name, path, features));
    }
    bins
}

/// AR-10.8-05 shared helper: every `members = [...]` array in a manifest.
fn parse_workspace_member_arrays(manifest: &str) -> Vec<Vec<String>> {
    static MEMBERS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^[a-z-]*members\s*=\s*\[([^\]]*)\]").expect("members regex must compile")
    });
    static QUOTED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""([^"]+)""#).expect("quoted regex must compile"));
    MEMBERS
        .captures_iter(manifest)
        .map(|captures| {
            QUOTED
                .captures_iter(&captures[1])
                .map(|quoted| {
                    quoted
                        .get(1)
                        .expect("member capture must exist")
                        .as_str()
                        .to_owned()
                })
                .collect()
        })
        .collect()
}

/// AR-10.8-05 shared predicate: exactly the single `rust/pokecon` member.
fn is_single_member_set(members: &[String]) -> bool {
    members == ["rust/pokecon"]
}

/// AR-10.8-05 shared helper: dependency names of a `[workspace.dependencies]` section.
fn parse_workspace_dependency_names(manifest: &str) -> Vec<String> {
    static NAME_LINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^([a-z0-9_-]+)\s*=").expect("dependency name regex must compile")
    });
    let section = manifest
        .split_once("[workspace.dependencies]")
        .expect("workspace manifest must define [workspace.dependencies]")
        .1;
    let section = section.split_once("\n[").map_or(section, |(head, _)| head);
    NAME_LINE
        .captures_iter(section)
        .map(|captures| {
            captures
                .get(1)
                .expect("dependency name capture must exist")
                .as_str()
                .to_owned()
        })
        .collect()
}

/// AR-10.8-05 shared helper: keys of the `[features]` section.
fn parse_feature_keys(manifest: &str) -> Vec<String> {
    static KEY_LINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^([A-Za-z0-9_-]+)\s*=").expect("feature key regex must compile")
    });
    let section = manifest
        .split_once("\n[features]")
        .expect("package manifest must define [features]")
        .1;
    let section = section.split_once("\n[").map_or(section, |(head, _)| head);
    KEY_LINE
        .captures_iter(section)
        .map(|captures| {
            captures
                .get(1)
                .expect("feature key capture must exist")
                .as_str()
                .to_owned()
        })
        .collect()
}

static TRAIT_DECLARATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"pub\s+trait\s+([A-Za-z0-9_]+)").expect("trait declaration regex must compile")
});

/// AR-10.8-05 shared scan: `(relative path, trait name)` per `pub trait` line
/// under `rust/pokecon/src`. `#[cfg(test)]` modules are included; fixture
/// paths outside `src/` never enter this walk by construction.
fn scan_source_traits() -> BTreeSet<(String, String)> {
    let src_root = repository_root().join("rust/pokecon/src");
    let mut observed = BTreeSet::new();
    let mut stack = vec![src_root];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory).unwrap_or_else(|error| {
            panic!(
                "contract input {} must be readable: {error}",
                directory.display()
            )
        });
        for entry in entries {
            let entry = entry.expect("contract input directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs")) {
                let relative = path
                    .strip_prefix(repository_root())
                    .expect("contract input must be below the repository root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!(
                        "contract input {} must be readable: {error}",
                        path.display()
                    )
                });
                for line in text.lines() {
                    if let Some(captures) = TRAIT_DECLARATION.captures(line) {
                        observed.insert((
                            relative.clone(),
                            captures
                                .get(1)
                                .expect("trait name capture must exist")
                                .as_str()
                                .to_owned(),
                        ));
                    }
                }
            }
        }
    }
    observed
}

/// AR-10.8-05 shared scan: files under `rust/pokecon/src` whose file name
/// marks the service/host/runtime layer.
fn scan_service_layer_files() -> BTreeSet<String> {
    let src_root = repository_root().join("rust/pokecon/src");
    let mut observed = BTreeSet::new();
    let mut stack = vec![src_root];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory).unwrap_or_else(|error| {
            panic!(
                "contract input {} must be readable: {error}",
                directory.display()
            )
        });
        for entry in entries {
            let entry = entry.expect("contract input directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs")) {
                let stem = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default();
                if stem.contains("service") || stem.contains("host") || stem.contains("runtime") {
                    observed.insert(
                        path.strip_prefix(repository_root())
                            .expect("contract input must be below the repository root")
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
    }
    observed
}

/// AR-10.8-05 shared scan: files under `rust/pokecon/src` containing an
/// `impl From<` / `impl TryFrom<` conversion (file-level, not item-level).
fn scan_conversion_files() -> BTreeSet<String> {
    let src_root = repository_root().join("rust/pokecon/src");
    let mut observed = BTreeSet::new();
    let mut stack = vec![src_root];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory).unwrap_or_else(|error| {
            panic!(
                "contract input {} must be readable: {error}",
                directory.display()
            )
        });
        for entry in entries {
            let entry = entry.expect("contract input directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs")) {
                let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!(
                        "contract input {} must be readable: {error}",
                        path.display()
                    )
                });
                if text
                    .lines()
                    .any(|line| line.contains("impl From<") || line.contains("impl TryFrom<"))
                {
                    observed.insert(
                        path.strip_prefix(repository_root())
                            .expect("contract input must be below the repository root")
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
    }
    observed
}

/// AR-10.8-05: fixture paths excluded from the abstraction scans by
/// construction (outside `rust/pokecon/src/`).
fn is_baseline_excluded_fixture(path: &str) -> bool {
    path.starts_with("rust/pokecon/tests/fixtures/")
}

/// AR-10.8-05 shared predicate: a worker-gated binary must reference a
/// justified strong boundary, unless it is a test-only fixture path.
fn worker_bin_boundary_ok(
    name: &str,
    features: &[String],
    boundary_ref: Option<&str>,
    boundary_ids: &BTreeSet<String>,
    bin_path: &str,
) -> bool {
    if is_baseline_excluded_fixture(bin_path) {
        return true;
    }
    if !features.iter().any(|feature| feature == "worker-binary") {
        return true;
    }
    boundary_ref.is_some_and(|id| boundary_ids.contains(id)) && !name.is_empty()
}

#[test]
fn workspace_has_single_member() {
    // AR-10.8-05: workspace member は `rust/pokecon` 一件であること。
    // 第二 member の追加はこの test を失敗させる (fail-closed)。
    let arrays = parse_workspace_member_arrays(&CARGO_MANIFEST);
    assert!(
        !arrays.is_empty(),
        "workspace manifest must declare at least one members array"
    );
    for members in &arrays {
        assert!(
            is_single_member_set(members),
            "workspace members must stay exactly [\"rust/pokecon\"], found {members:?} (AR-10.8-05)"
        );
    }
    // Fail-closed sentinel controls on the exact predicate.
    assert!(
        is_single_member_set(&["rust/pokecon".to_owned()]),
        "single member set must count as valid (AR-10.8-05 control)"
    );
    assert!(
        !is_single_member_set(&["rust/pokecon".to_owned(), "rust/pokecon-extra".to_owned()]),
        "second workspace member must be rejected (AR-10.8-05 control)"
    );
    assert!(
        !is_single_member_set(&[]),
        "empty member set must be rejected (AR-10.8-05 control)"
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn abstraction_baseline_matches_manifest() {
    // AR-10.8-05: current Cargo.toml manifest の baseline 化
    // (rust/pokecon/registry/abstraction_baseline.json)。workspace member、
    // 依存名、bin、feature、trait、service 層、変換型のいずれの無言の追加も
    // 双方向 diff で失敗させる。未知の追加 (manifest 漏れ) と stale entry
    // (実装に存在しない宣言) のいずれもこの test を失敗させる。
    //
    // 誠実な範囲限定 (scope):
    // (1) 依存は sorted NAME のみを pin し version は見ない (upgrade-proof。
    //     version は Cargo.lock + workspace-lock-check が担う)。
    // (2) trait 走査は `pub trait NAME` 行の文字列走査のみ —
    //     コメント化行、macro 生成 trait は捕捉しない。`#[cfg(test)]`
    //     module は含め、tests/fixtures 配下は path 除外する。
    // (3) service／conversion inventory は file-level であり item-level
    //     diff は主張しない (AR-11-39)。
    // (4) test 内部で cargo を起動しない (hermeticity)。`cargo metadata`
    //     は handoff 10.2 の手動検証手順に留める。
    let manifest = parse_json(&ABSTRACTION_BASELINE_JSON);
    assert_eq!(
        manifest["schema_version"], 1,
        "abstraction baseline schema_version must be 1 (AR-10.8-05)"
    );

    // (a) Workspace members: baseline mirrors the single-member pin.
    let baseline_members: BTreeSet<String> = manifest["workspace_members"]
        .as_array()
        .expect("abstraction baseline must define workspace_members")
        .iter()
        .map(|item| {
            item.as_str()
                .expect("workspace member must be a string")
                .to_owned()
        })
        .collect();
    assert_eq!(
        baseline_members,
        BTreeSet::from(["rust/pokecon".to_owned()]),
        "baseline workspace members must stay exactly [\"rust/pokecon\"]"
    );

    // (b) Workspace dependencies: exact-set bidirectional diff on sorted names.
    let baseline_deps: BTreeSet<String> = manifest["workspace_dependency_names"]
        .as_array()
        .expect("abstraction baseline must define workspace_dependency_names")
        .iter()
        .map(|item| {
            item.as_str()
                .expect("workspace dependency name must be a string")
                .to_owned()
        })
        .collect();
    let baseline_dep_list: Vec<&str> = manifest["workspace_dependency_names"]
        .as_array()
        .expect("abstraction baseline must define workspace_dependency_names")
        .iter()
        .map(|item| {
            item.as_str()
                .expect("workspace dependency name must be a string")
        })
        .collect();
    let mut sorted_deps = baseline_dep_list.clone();
    sorted_deps.sort_unstable();
    assert_eq!(
        baseline_dep_list, sorted_deps,
        "baseline dependency names must be sorted (AR-10.8-05)"
    );
    let observed_deps: BTreeSet<String> = parse_workspace_dependency_names(&CARGO_MANIFEST)
        .into_iter()
        .collect();
    let unknown_deps: Vec<&String> = observed_deps.difference(&baseline_deps).collect();
    assert!(
        unknown_deps.is_empty(),
        "unknown workspace dependencies missing from the baseline (AR-10.8-05): {unknown_deps:?}"
    );
    let stale_deps: Vec<&String> = baseline_deps.difference(&observed_deps).collect();
    assert!(
        stale_deps.is_empty(),
        "stale baseline dependencies with no manifest entry (AR-10.8-05): {stale_deps:?}"
    );
    // Fail-closed sentinel: a synthetic extra dep must break the predicate.
    let mut with_extra = baseline_deps.clone();
    with_extra.insert("ar-10-8-05-sentinel-crate".to_owned());
    assert_ne!(
        with_extra, observed_deps,
        "synthetic extra dependency must break the baseline pin (AR-10.8-05 control)"
    );

    // (c) Bins: exact-set vs [[bin]] plus the 04<->05 cross-file link.
    let strong = parse_json(&STRONG_BOUNDARIES_JSON);
    let boundary_ids: BTreeSet<String> = strong["boundaries"]
        .as_array()
        .expect("strong boundary manifest must define boundaries")
        .iter()
        .map(|boundary| {
            boundary["id"]
                .as_str()
                .expect("strong boundary must have a string id")
                .to_owned()
        })
        .collect();
    let baseline_bins = manifest["bins"]
        .as_array()
        .expect("abstraction baseline must define bins");
    let mut baseline_bin_names = BTreeSet::new();
    for bin in baseline_bins {
        let name = bin["name"]
            .as_str()
            .expect("baseline bin must have a string name");
        assert!(
            baseline_bin_names.insert(name.to_owned()),
            "baseline bin {name} must be unique"
        );
        let path = bin["path"]
            .as_str()
            .expect("baseline bin must have a string path");
        repository_text(path);
        let features: Vec<String> = bin["required_features"]
            .as_array()
            .unwrap_or_else(|| panic!("baseline bin {name} must declare required_features"))
            .iter()
            .map(|feature| {
                feature
                    .as_str()
                    .expect("baseline bin feature must be a string")
                    .to_owned()
            })
            .collect();
        let boundary_ref = bin["boundary_ref"].as_str();
        if let Some(id) = boundary_ref {
            assert!(
                boundary_ids.contains(id),
                "baseline bin {name} boundary_ref {id} must name a strong boundary id"
            );
        }
        assert!(
            worker_bin_boundary_ok(name, &features, boundary_ref, &boundary_ids, path),
            "worker-gated bin {name} must reference a justified strong boundary (AR-10.8-05)"
        );
    }
    let observed_bins = parse_pokecon_bins(&POKECON_MANIFEST);
    let observed_bin_names: BTreeSet<String> = observed_bins
        .iter()
        .map(|(name, _, _)| name.clone())
        .collect();
    assert_eq!(
        baseline_bin_names, observed_bin_names,
        "baseline bins must match Cargo.toml [[bin]] exactly (AR-10.8-05)"
    );
    for (name, path, features) in &observed_bins {
        let entry = baseline_bins
            .iter()
            .find(|bin| bin["name"] == *name)
            .unwrap_or_else(|| panic!("baseline must pin observed bin {name}"));
        // Baseline bin paths are repository-relative; the package manifest
        // records them relative to rust/pokecon/.
        let expected_path = format!("rust/pokecon/{path}");
        assert_eq!(
            entry["path"],
            expected_path.as_str(),
            "baseline bin {name} path must match Cargo.toml"
        );
        let baseline_features: BTreeSet<&str> = entry["required_features"]
            .as_array()
            .expect("baseline bin must declare required_features")
            .iter()
            .map(|feature| feature.as_str().expect("bin feature must be a string"))
            .collect();
        let observed_features: BTreeSet<&str> = features.iter().map(String::as_str).collect();
        assert_eq!(
            baseline_features, observed_features,
            "baseline bin {name} features must match Cargo.toml"
        );
    }
    // Fail-closed sentinel: a synthetic worker-gated bin without a
    // boundary_ref must fail the cross-file predicate.
    assert!(
        !worker_bin_boundary_ok(
            "pokecon-sentinel-bin",
            &["worker-binary".to_owned()],
            None,
            &boundary_ids,
            "rust/pokecon/src/bin/sentinel.rs",
        ),
        "synthetic worker bin without boundary_ref must be rejected (AR-10.8-05 control)"
    );
    assert!(
        worker_bin_boundary_ok(
            "pokecon-worker",
            &["worker-binary".to_owned()],
            Some("worker-process"),
            &boundary_ids,
            "rust/pokecon/src/bin/worker.rs",
        ),
        "pinned worker bin must count as covered (AR-10.8-05 control)"
    );

    // (d) Features: exact-set vs [features] keys.
    let baseline_features: BTreeSet<String> = manifest["features"]
        .as_array()
        .expect("abstraction baseline must define features")
        .iter()
        .map(|item| item.as_str().expect("feature must be a string").to_owned())
        .collect();
    let observed_features: BTreeSet<String> =
        parse_feature_keys(&POKECON_MANIFEST).into_iter().collect();
    assert_eq!(
        baseline_features, observed_features,
        "baseline features must match Cargo.toml [features] exactly (AR-10.8-05)"
    );

    // (e) Traits: exact-set vs `pub trait` scan, each with a justification.
    let baseline_traits: BTreeSet<(String, String)> = manifest["traits"]
        .as_array()
        .expect("abstraction baseline must define traits")
        .iter()
        .map(|entry| {
            let path = entry["path"]
                .as_str()
                .expect("baseline trait must have a string path");
            let name = entry["name"]
                .as_str()
                .expect("baseline trait must have a string name");
            let justification = entry["justification"]
                .as_str()
                .unwrap_or_else(|| panic!("baseline trait {name} must declare a justification"));
            assert!(
                is_valid_necessity(justification),
                "baseline trait {name} justification must be one non-empty line"
            );
            assert!(
                !is_baseline_excluded_fixture(path),
                "baseline trait {name} must not point at an excluded fixture path"
            );
            repository_text(path);
            (path.to_owned(), name.to_owned())
        })
        .collect();
    let observed_traits = scan_source_traits();
    assert!(
        !observed_traits.is_empty(),
        "trait scan must observe rust/pokecon/src (AR-10.8-05)"
    );
    let unknown_traits: Vec<&(String, String)> =
        observed_traits.difference(&baseline_traits).collect();
    assert!(
        unknown_traits.is_empty(),
        "new traits without a justification entry (AR-10.8-05): {unknown_traits:?}"
    );
    let stale_traits: Vec<&(String, String)> =
        baseline_traits.difference(&observed_traits).collect();
    assert!(
        stale_traits.is_empty(),
        "stale baseline trait entries with no observed declaration (AR-10.8-05): {stale_traits:?}"
    );
    // Fail-closed sentinel: a synthetic extra trait must break the pin, and
    // the trait-declaration regex must detect an injected declaration while
    // ignoring a non-trait line.
    let mut with_trait = baseline_traits.clone();
    with_trait.insert((
        "rust/pokecon/src/sentinel.rs".to_owned(),
        "SentinelTrait".to_owned(),
    ));
    assert_ne!(
        with_trait, observed_traits,
        "synthetic extra trait must break the baseline pin (AR-10.8-05 control)"
    );
    assert_eq!(
        TRAIT_DECLARATION
            .captures("pub trait SentinelTrait: Send + Sync {")
            .expect("trait regex must match a trait line")
            .get(1)
            .expect("trait name capture must exist")
            .as_str(),
        "SentinelTrait"
    );
    assert!(
        TRAIT_DECLARATION
            .captures("pub struct NotATrait;")
            .is_none(),
        "trait regex must not match a non-trait line (AR-10.8-05 control)"
    );

    // (f) Service layer: exact-set vs service/host/runtime file glob.
    let baseline_services: BTreeSet<String> = manifest["services"]
        .as_array()
        .expect("abstraction baseline must define services")
        .iter()
        .map(|entry| {
            let path = entry["path"]
                .as_str()
                .expect("baseline service must have a string path");
            let justification = entry["justification"]
                .as_str()
                .unwrap_or_else(|| panic!("baseline service {path} must declare a justification"));
            assert!(
                is_valid_necessity(justification),
                "baseline service {path} justification must be one non-empty line"
            );
            repository_text(path);
            path.to_owned()
        })
        .collect();
    let observed_services = scan_service_layer_files();
    assert_eq!(
        baseline_services, observed_services,
        "baseline services must match the service/host/runtime file inventory exactly (AR-10.8-05)"
    );

    // (g) Conversion files: exact-set vs impl From/TryFrom file scan.
    let baseline_conversions: BTreeSet<String> = manifest["conversion_files"]
        .as_array()
        .expect("abstraction baseline must define conversion_files")
        .iter()
        .map(|item| {
            item.as_str()
                .expect("conversion file must be a string")
                .to_owned()
        })
        .collect();
    let observed_conversions = scan_conversion_files();
    assert_eq!(
        baseline_conversions, observed_conversions,
        "baseline conversion files must match the From/TryFrom scan exactly (AR-10.8-05)"
    );

    // (h) Fixture-exclusion liveness: the excluded fixture paths exist, so
    // the exclusion list cannot pass vacuously, and the src/ scans never
    // observe them.
    for fixture in [
        "rust/pokecon/tests/fixtures/fault_worker.rs",
        "rust/pokecon/tests/fixtures/forbidden_dependency_fixture.rs",
        "rust/pokecon/tests/fixtures/ipc_nonpublic_payloads.rs",
    ] {
        assert!(
            is_baseline_excluded_fixture(fixture),
            "fixture {fixture} must be excluded by path (AR-10.8-05 control)"
        );
        repository_text(fixture);
        assert!(
            !observed_traits.iter().any(|(path, _)| path == fixture),
            "excluded fixture {fixture} must never enter the trait scan"
        );
    }
    assert!(
        !is_baseline_excluded_fixture("rust/pokecon/src/server/backend.rs"),
        "src/ files must NOT count as excluded fixtures (AR-10.8-05 control)"
    );
}

#[allow(dead_code)]
fn _assert_setting_is_public(_: &Setting) {}

/// Positions of `markers` inside `body`, in order. Each marker must occur
/// exactly once so a duplicated or split shutdown step fails closed instead
/// of passing on its first occurrence.
fn ordered_marker_positions(body: &str, markers: &[&str]) -> Vec<usize> {
    markers
        .iter()
        .map(|marker| {
            let positions: Vec<usize> =
                body.match_indices(marker).map(|(index, _)| index).collect();
            assert_eq!(
                positions.len(),
                1,
                "shutdown call-graph marker {marker:?} must occur exactly once in its pinned body"
            );
            positions[0]
        })
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)]
fn production_shutdown_call_graph_pins_fault_responsibility_table() {
    // AR-11-29: docs/ARCHITECTURE_HANDOFF.md section 7 (fault, timeout,
    // rollback, shutdown responsibility) is backed by this call-graph pin,
    // not by prose alone. Any reorder, removal, or duplication of a
    // production shutdown step breaks this test before the table can drift.
    //
    // Fail-closed controls: the ordering predicate is first exercised on a
    // synthetic body (positive control) and on a body with a duplicated
    // marker, which must panic (negative control via `catch_unwind`).

    // (a) `shutdown_production` (rust/pokecon/src/lib.rs) is the single
    // production shutdown order: dynamic shutdown-pre, production steps
    // 1-3, dynamic worker reap, the step-5 release-gate observation,
    // production steps 6-7.
    let lib = repository_text("rust/pokecon/src/lib.rs");
    let shutdown_production = between(&lib, "async fn shutdown_production(", "\n}\n");
    assert!(
        !shutdown_production.is_empty(),
        "lib.rs must define shutdown_production (AR-11-29)"
    );
    let lib_order = ordered_marker_positions(
        shutdown_production,
        &[
            "prepare_shutdown",
            "stop_inputs_camera_and_scripts",
            "shutdown_worker",
            "retain_dynamic_mapping_after_worker_shutdown",
            "shared_memory_release_gate_after_dynamic_reap",
            "stop_serial",
        ],
    );
    assert!(
        lib_order.windows(2).all(|pair| pair[0] < pair[1]),
        "shutdown_production must run prepare_shutdown, stop_inputs_camera_and_scripts, shutdown_worker, retain_dynamic_mapping_after_worker_shutdown, shared_memory_release_gate_after_dynamic_reap, stop_serial in that order (AR-11-29)"
    );

    // (a3) Fail-closed step 5 is a persistence-then-observation pair: an
    // unconfirmed dynamic reap is first retained as `camera_reader_fallback`
    // ownership (ordered above), and only then is the step-5 verdict
    // observed. The verdict itself stays diagnostic-only: it is bound only
    // to build `ProductionShutdownReport` and shutdown continues to
    // stop_serial unconditionally, so the gate observation never controls
    // shutdown flow and its semantics are not overstated.
    assert!(
        shutdown_production.contains("let step5_release_allowed =")
            && shutdown_production.contains("step5_release_allowed,"),
        "shutdown_production must retain the step-5 verdict only as report data so it stays diagnostic-only (AR-11-29)"
    );
    assert!(
        !shutdown_production.contains("if step5_release_allowed"),
        "shutdown_production must continue to stop_serial unconditionally; the step-5 verdict must not gate control flow (AR-11-29)"
    );

    // (a2) The step-5 gate itself (rust/pokecon/src/production.rs) is a
    // read-only observation: it consumes the existing release state plus
    // an explicit dynamic-reap confirmation, and it must never release,
    // unmap, or retain anything itself.
    let production_rs = repository_text("rust/pokecon/src/production.rs");
    let release_gate = between(
        &production_rs,
        "pub(crate) fn shared_memory_release_gate_after_dynamic_reap(",
        "\n    }\n",
    );
    assert!(
        !release_gate.is_empty(),
        "production.rs must define shared_memory_release_gate_after_dynamic_reap (AR-11-29)"
    );
    assert!(
        release_gate.contains("&self,"),
        "the step-5 gate must take a shared receiver so shutdown state cannot be mutated (AR-11-29)"
    );
    assert!(
        !release_gate.contains("&mut self"),
        "the step-5 gate must not take a mutable receiver (AR-11-29)"
    );
    assert!(
        release_gate.contains("dynamic_reaped")
            && release_gate.contains("shared_memory_release_allowed"),
        "the step-5 gate must consume both the dynamic-reap confirmation and the existing release state (AR-11-29)"
    );
    assert!(
        !release_gate.contains("mem::forget"),
        "the step-5 gate must never retain a mapping itself; retention stays owned by Drop (AR-11-29)"
    );
    // The gate body must contain none of the known mutation/retention
    // operations: pin recovery, writer shutdown, reader or dynamic
    // retention, fallback state, forget, mapping release, or drop. Each is
    // asserted separately so a future edit cannot smuggle exactly one of
    // them past a grouped check.
    for marker in [
        "recover_reader_pins",
        "shutdown",
        "retain_reader",
        "retain_dynamic",
        "camera_reader_fallback",
        "DYNAMIC_WORKER_UNREAPED",
        "forget",
        "release_mapping",
        "drop",
    ] {
        assert!(
            !release_gate.contains(marker),
            "the step-5 gate must never call {marker:?}; it is read-only and retention stays owned by Drop (AR-11-29)"
        );
    }
    // The single known diagnostic phrase "without unmap" states the
    // fail-closed outcome in prose; any other unmap operation is refused.
    assert!(
        !release_gate.replace("without unmap", "").contains("unmap"),
        "the step-5 gate must never unmap itself outside the documented diagnostic phrase; retention stays owned by Drop (AR-11-29)"
    );

    // (b) `stop_inputs_camera_and_scripts` (rust/pokecon/src/production.rs)
    // is production steps 1-3: abort tasks, force-release + neutral send,
    // camera shutdown with fallback retention, script shutdown, reader-pin
    // recovery. Method bodies close indented (`    }`), so the end marker
    // must not be the column-0 `}` used for top-level functions.
    let production = repository_text("rust/pokecon/src/production.rs");
    let stop_inputs = between(
        &production,
        "pub(crate) async fn stop_inputs_camera_and_scripts(&mut self)",
        "\n    }\n",
    );
    assert!(
        !stop_inputs.is_empty(),
        "production.rs must define stop_inputs_camera_and_scripts (AR-11-29)"
    );
    let inputs_order = ordered_marker_positions(
        stop_inputs,
        &[
            "abort_background_tasks",
            "force_release_all",
            "send_released_controller_output",
            "camera.shutdown(SERVICE_STOP_TIMEOUT)",
            "retain_camera_fallback_on_timeout",
            "self.commands.shutdown",
            "recover_reader_pins_after_script_shutdown",
        ],
    );
    assert!(
        inputs_order.windows(2).all(|pair| pair[0] < pair[1]),
        "stop_inputs_camera_and_scripts must keep the step 1-3 order (AR-11-29)"
    );

    // (c) `stop_serial` (rust/pokecon/src/production.rs) is production
    // steps 6-7: second force-release, NEUTRAL send, bounded disconnect.
    let stop_serial = between(
        &production,
        "pub(crate) async fn stop_serial(&self)",
        "\n    }\n",
    );
    assert!(
        !stop_serial.is_empty(),
        "production.rs must define stop_serial (AR-11-29)"
    );
    let serial_order = ordered_marker_positions(
        stop_serial,
        &[
            "force_release_all",
            "ControllerState::NEUTRAL",
            "disconnect_serial_service",
        ],
    );
    assert!(
        serial_order.windows(2).all(|pair| pair[0] < pair[1]),
        "stop_serial must keep the step 6-7 order (AR-11-29)"
    );

    // (d) `DynamicRuntime` shutdown halves (rust/pokecon/src/dynamic_runtime.rs):
    // prepare emits AppShutdownPre under a timeout then closes mutations;
    // the worker half reaps by the fixed deadline with ApplicationShutdown.
    let dynamic = repository_text("rust/pokecon/src/dynamic_runtime.rs");
    let prepare = between(
        &dynamic,
        "pub async fn prepare_shutdown(&self)",
        "\n    }\n",
    );
    assert!(
        prepare.contains("AppShutdownPre") && prepare.contains("begin_stopping"),
        "DynamicRuntime::prepare_shutdown must emit AppShutdownPre then close mutations (AR-11-29)"
    );
    let reap = between(&dynamic, "pub async fn shutdown_worker(", "\n    }\n");
    assert!(
        reap.contains("StopPurpose::ApplicationShutdown"),
        "DynamicRuntime::shutdown_worker must reap with ApplicationShutdown (AR-11-29)"
    );
    // The worker half returns the real `StopReport` result. The caller maps
    // that result to the step-5 gate, so a forced-stop or an unproven reap is
    // not collapsed into an invented bool inside DynamicRuntime.
    assert!(
        reap.contains("-> Result<crate::worker::supervisor::StopReport, SupervisorError>")
            && reap.contains("log_stop_result(&stop)")
            && reap.lines().any(|line| line.trim() == "stop"),
        "DynamicRuntime::shutdown_worker must return the real StopReport result (AR-11-29)"
    );

    // (e) The handoff section 7 table names the pinned entry points, so the
    // table cannot silently detach from this call graph.
    let handoff = repository_text("docs/ARCHITECTURE_HANDOFF.md");
    let section7 = handoff
        .split_once("## 7. Fault、timeout、rollback、shutdownの責任")
        .expect("ARCHITECTURE_HANDOFF.md must define section 7 (AR-11-29)")
        .1;
    let section7 = section7
        .split_once("\n## ")
        .map_or(section7, |(head, _)| head);
    for anchor in [
        "stop_inputs_camera_and_scripts",
        "stop_serial",
        "retain_camera_fallback_on_timeout",
        "AppShutdownPre",
    ] {
        assert!(
            section7.contains(anchor),
            "handoff section 7 must name the pinned shutdown entry point {anchor:?} (AR-11-29)"
        );
    }

    // (f) Fail-closed controls for the ordering predicate itself.
    let synthetic = "alpha prepare_shutdown beta stop_inputs_camera_and_scripts gamma";
    let positions = ordered_marker_positions(
        synthetic,
        &["prepare_shutdown", "stop_inputs_camera_and_scripts"],
    );
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "AR-11-29 ordering control must accept ordered markers"
    );
    let duplicated = "prepare_shutdown prepare_shutdown stop_inputs_camera_and_scripts";
    assert!(
        std::panic::catch_unwind(|| ordered_marker_positions(
            duplicated,
            &["prepare_shutdown", "stop_inputs_camera_and_scripts"],
        ))
        .is_err(),
        "AR-11-29 ordering control must reject a duplicated marker"
    );

    // (g) `ProductionRuntime::Drop` keeps a flag-only fallback path for a
    // lost shutdown join (e.g. the `spawn_blocking` camera shutdown task
    // fails): a durable `camera_writer_unstopped` flag with no guard mints
    // a fresh guard instead of releasing. No OS-level post-Drop mapping
    // proof is claimed here; this pins the source shape only, while the
    // virtual-backend shutdown test below drives the flag-only state at
    // production level.
    let drop_impl = between(&production_rs, "impl Drop for ProductionRuntime", "\n}\n");
    assert!(
        !drop_impl.is_empty(),
        "production.rs must define Drop for ProductionRuntime (AR-11-29)"
    );
    assert!(
        drop_impl.contains("None if self.camera.writer_unstopped()"),
        "Drop must keep a flag-only arm so a durable writer-unstopped flag without a guard still retains the mapping (AR-11-29)"
    );
    assert!(
        drop_impl.contains("mark_writer_unstopped"),
        "Drop flag-only arm must mint a fresh guard via mark_writer_unstopped instead of releasing (AR-11-29)"
    );
    assert!(
        drop_impl
            .matches("CAMERA_MAPPING_FALLBACK_RETAINED")
            .count()
            >= 2,
        "Drop must log retention for both the guard path and the flag-only path (AR-11-29)"
    );
    assert!(
        drop_impl.matches("mem::forget").count() >= 2,
        "Drop must forget (never unmap) on both the reader and writer fallback paths (AR-11-29)"
    );
}
