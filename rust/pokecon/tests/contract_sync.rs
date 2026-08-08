use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use pokecon::integration_test_support::contracts::model::{Access, Mutability, Scope, Setting};
use pokecon::integration_test_support::contracts::{PROTOCOL_REGISTRY_JSON, settings_registry};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};

const SPECIFICATION: &str = include_str!("../../../SPECIFICATION.md");
const ACCEPTANCE_SCHEMA: &str = include_str!("../registry/acceptance-record.schema.json");
const ACCEPTANCE_PROCEDURE: &str = include_str!("../../../docs/ACCEPTANCE.md");
const COMPATIBILITY_REGISTRY_JSON: &str = include_str!("../registry/compatibility.json");
const GENERATION_REGISTRY_JSON: &str = include_str!("../registry/generation.json");
const CI_REGISTRY_JSON: &str = include_str!("../registry/ci.json");
const FOUNDATION_REGISTRY_JSON: &str = include_str!("../registry/foundation.json");
const FIXED_MANIFEST: &str = include_str!("../../../compatibility/fixed-manifest.json");
const FLAKE: &str = include_str!("../../../flake.nix");
const PYPROJECT: &str = include_str!("../../../pyproject.toml");
const CARGO_MANIFEST: &str = include_str!("../../../Cargo.toml");
const GITIGNORE: &str = include_str!("../../../.gitignore");
const WORKFLOWS: &[(&str, &str)] = &[
    (
        "compatibility-roll",
        include_str!("../../../.github/workflows/compatibility-roll.yml"),
    ),
    ("lint", include_str!("../../../.github/workflows/lint.yml")),
    (
        "pytest",
        include_str!("../../../.github/workflows/pytest.yml"),
    ),
    (
        "package",
        include_str!("../../../.github/workflows/package.yml"),
    ),
    (
        "release",
        include_str!("../../../.github/workflows/release.yml"),
    ),
    (
        "remote-flake",
        include_str!("../../../.github/workflows/remote-flake.yml"),
    ),
    (
        "rust-ci",
        include_str!("../../../.github/workflows/rust-ci.yml"),
    ),
];

#[test]
fn canonical_settings_match_every_normative_spec_projection() {
    let validated = settings_registry().expect("canonical settings registry must be valid");
    let settings = validated.settings();
    assert_eq!(settings.len(), 78);

    let registry_by_id = settings
        .iter()
        .map(|setting| (setting.id.as_str(), setting))
        .collect::<BTreeMap<_, _>>();
    let spec_rows = setting_projection_rows();
    assert_eq!(spec_rows.len(), 78);
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
    let compatibility = parse_json(COMPATIBILITY_REGISTRY_JSON);
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
    let compatibility = parse_json(COMPATIBILITY_REGISTRY_JSON);
    let inventory = parse_json(FIXED_MANIFEST);
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

    let canonical = serde_json::to_vec(baselines).expect("inventory must serialize");
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
    let generation = parse_json(GENERATION_REGISTRY_JSON);
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

    let ci = parse_json(CI_REGISTRY_JSON);
    let jobs = ci["jobs"].as_array().expect("CI jobs must be an array");
    let names = jobs
        .iter()
        .map(|job| string_at(job, "name").to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(names.len(), jobs.len(), "CI job names must be unique");
    assert_eq!(names, workflow_job_names());
    assert!(names.contains("lint/contracts"));
    for job in jobs {
        let command = string_at(job, "command");
        let windows_native = job["execution_environment"] == "windows-native";
        assert!(
            command.starts_with("nix ") || (windows_native && command.starts_with("cargo ")),
            "CI commands must use Nix except for the explicit Windows-native build: {command}"
        );
        assert!(!string_at(job, "applicable_when").is_empty());
        assert!(!string_at(job, "not_applicable").is_empty());
        assert!(
            job["phase"]
                .as_u64()
                .is_some_and(|phase| (1..=15).contains(&phase))
        );
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

    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("pokecon package must be nested under the repository root");
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

#[test]
fn verification_taxonomy_is_complete() {
    let foundation = parse_json(FOUNDATION_REGISTRY_JSON);
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
    let foundation = parse_json(FOUNDATION_REGISTRY_JSON);
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

    let mut legacy_sources = format!("{FLAKE}\n{PYPROJECT}\n{CARGO_MANIFEST}\n{GITIGNORE}");
    for (_, source) in WORKFLOWS {
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
            if path.contains('/') && !path.contains('*') {
                let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(Path::parent)
                    .expect("pokecon package must be nested under the repository root");
                let resolved_path = repository.join(path);
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
    let schema = parse_json(ACCEPTANCE_SCHEMA);
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
        assert_eq!(capability["release_required"], true);
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
    let section = between(SPECIFICATION, "#### 11.4.2", "**注**");
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
    between(SPECIFICATION, "## 12. 環境変数", "## 13.")
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
    between(SPECIFICATION, "### 7.4 HTTP REST API", "### 7.5")
        .lines()
        .filter_map(|line| {
            let columns = split_markdown_row(line);
            (columns.len() >= 2 && matches!(columns[0].as_str(), "GET" | "PATCH" | "POST"))
                .then(|| format!("{} {}", columns[0], columns[1]))
        })
        .collect()
}

fn specification_websocket_variants() -> BTreeSet<String> {
    let section = between(SPECIFICATION, "#### 7.3.2", "### 7.8");
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
        SPECIFICATION,
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
    between(SPECIFICATION, "#### 4.6.1", "#### 4.6.2")
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
    let inventory = parse_json(FIXED_MANIFEST);
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

#[allow(dead_code)]
fn _assert_setting_is_public(_: &Setting) {}
