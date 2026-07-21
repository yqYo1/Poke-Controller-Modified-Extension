use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use pokecon_contracts::model::{DefaultValue, Scope, Setting, ValueSchema, WireEncoding};
use pokecon_contracts::{ContractError, SettingsRegistry, settings_registry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::lock::LockManager;
use crate::package::{
    ConstraintResolver, PackageError, PackageResolution, PackageSource, PackageSourceKind,
    PythonWorker, RequirementEntry,
};
use crate::path::{PathError, PathSource, lexical_normalize, resolve_path};
use crate::persistence::{PersistenceError, SettingsDocument, TomlStore};
use crate::roots::{BaseDirectories, EffectiveRoots, RootEnvironment, RootError, SafeComponent};

pub const SECRET_MASK: &str = "********";
type PackageSources = BTreeMap<String, Vec<PackageSource>>;

struct LayerContext<'a> {
    registry: &'a SettingsRegistry,
    roots: &'a EffectiveRoots,
    request: &'a PipelineRequest,
}

struct LayerResolution {
    values: BTreeMap<String, ResolvedValue>,
    package_sources: PackageSources,
    ignored: Vec<String>,
}

struct BootstrapResolution {
    roots: EffectiveRoots,
    global: SettingsDocument,
    global_settings_path: PathBuf,
    active_profile: SafeComponent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResolutionStage {
    BeforeDynamic,
    Complete,
}

/// Provenance of one winning setting value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingSource {
    Default,
    GlobalToml,
    ProfileToml,
    Environment,
    Dynamic,
    CommandLine,
    OpenApi,
}

impl SettingSource {
    const fn path_source(self) -> Option<PathSource> {
        match self {
            Self::Default => None,
            Self::GlobalToml => Some(PathSource::GlobalToml),
            Self::ProfileToml => Some(PathSource::ProfileToml),
            Self::Environment => Some(PathSource::Environment),
            Self::Dynamic => Some(PathSource::Dynamic),
            Self::CommandLine => Some(PathSource::CommandLine),
            Self::OpenApi => Some(PathSource::OpenApi),
        }
    }
}

/// Canonical resolved value and the surface that supplied it.
#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedValue {
    pub value: Value,
    pub source: SettingSource,
}

impl fmt::Debug for ResolvedValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedValue")
            .field("value", &"<redacted>")
            .field("source", &self.source)
            .finish()
    }
}

/// Complete 78-setting snapshot.
#[derive(Clone)]
pub struct ResolvedSettings {
    registry: SettingsRegistry,
    values: BTreeMap<String, ResolvedValue>,
    package_sources: BTreeMap<String, Vec<PackageSource>>,
}

impl fmt::Debug for ResolvedSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedSettings")
            .field("registry", &"<embedded registry>")
            .field("setting_count", &self.values.len())
            .field("package_list_count", &self.package_sources.len())
            .field("values", &"<redacted>")
            .field("package_sources", &"<redacted>")
            .finish()
    }
}

impl ResolvedSettings {
    /// Looks up one canonical ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ResolvedValue> {
        self.values.get(id)
    }

    /// Returns all resolved values in canonical-ID order.
    #[must_use]
    pub const fn values(&self) -> &BTreeMap<String, ResolvedValue> {
        &self.values
    }

    /// Returns the exact metadata projection used to resolve this snapshot.
    #[must_use]
    pub const fn registry(&self) -> &SettingsRegistry {
        &self.registry
    }

    /// Returns the retained package-aware source layers for one canonical
    /// `packages.list` setting.
    #[must_use]
    pub fn package_sources(&self, id: &str) -> &[PackageSource] {
        self.package_sources.get(id).map_or(&[], Vec::as_slice)
    }

    /// Resolves application and user package requirements for one worker using
    /// the package-aware layers retained during settings precedence resolution.
    ///
    /// # Errors
    ///
    /// Returns a package parsing or constraint-resolution error.
    pub fn resolve_packages(
        &self,
        worker: PythonWorker,
    ) -> Result<PackageResolution, PipelineError> {
        let prefix = match worker {
            PythonWorker::Script => "python.script.packages",
            PythonWorker::Dynamic => "python.dynamic.packages",
        };
        ConstraintResolver::resolve(
            worker,
            self.package_sources(&format!("{prefix}.list")).to_vec(),
            self.boolean(&format!("{prefix}.override_application_constraints"))?,
            self.boolean(&format!("{prefix}.override_package_metadata_constraints"))?,
        )
        .map_err(PipelineError::Package)
    }

    /// Returns a string setting.
    ///
    /// # Errors
    ///
    /// Returns a typed lookup error if the canonical ID is absent or not a
    /// string.
    pub fn string(&self, id: &str) -> Result<&str, PipelineError> {
        self.get(id)
            .and_then(|resolved| resolved.value.as_str())
            .ok_or_else(|| PipelineError::TypedLookup(id.to_owned()))
    }

    /// Returns an integer setting.
    ///
    /// # Errors
    ///
    /// Returns a typed lookup error if absent or not an integer.
    pub fn integer(&self, id: &str) -> Result<i64, PipelineError> {
        self.get(id)
            .and_then(|resolved| resolved.value.as_i64())
            .ok_or_else(|| PipelineError::TypedLookup(id.to_owned()))
    }

    /// Returns a boolean setting.
    ///
    /// # Errors
    ///
    /// Returns a typed lookup error if absent or not a boolean.
    pub fn boolean(&self, id: &str) -> Result<bool, PipelineError> {
        self.get(id)
            .and_then(|resolved| resolved.value.as_bool())
            .ok_or_else(|| PipelineError::TypedLookup(id.to_owned()))
    }
}

/// All deterministic inputs to settings resolution.
#[derive(Clone)]
pub struct PipelineRequest {
    pub arguments: Vec<OsString>,
    pub environment: RootEnvironment,
    pub startup_cwd: PathBuf,
    pub resource_root: PathBuf,
    pub base_directories: Option<BaseDirectories>,
    pub dynamic_values: BTreeMap<String, Value>,
}

impl fmt::Debug for PipelineRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PipelineRequest")
            .field("argument_count", &self.arguments.len())
            .field("arguments", &"<redacted>")
            .field("environment", &self.environment)
            .field("startup_cwd", &self.startup_cwd)
            .field("resource_root", &self.resource_root)
            .field("base_directories", &self.base_directories)
            .field("dynamic_assignment_count", &self.dynamic_values.len())
            .field("dynamic_values", &"<redacted>")
            .finish()
    }
}

impl PipelineRequest {
    /// Captures the current process without reading or writing settings yet.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the startup cwd or executable resource root is
    /// unavailable.
    pub fn current() -> Result<Self, PipelineError> {
        let startup_cwd = std::env::current_dir().map_err(PipelineError::CurrentDirectory)?;
        let executable = std::env::current_exe().map_err(PipelineError::CurrentExecutable)?;
        let resource_root = executable
            .parent()
            .map(Path::to_path_buf)
            .ok_or(PipelineError::MissingResourceRoot)?;
        Ok(Self {
            arguments: std::env::args_os().collect(),
            environment: RootEnvironment::current(),
            startup_cwd,
            resource_root,
            base_directories: None,
            dynamic_values: BTreeMap::new(),
        })
    }
}

/// Loaded settings plus path/bootstrap state needed by runtime services.
#[derive(Clone)]
pub struct LoadedSettings {
    pub roots: EffectiveRoots,
    pub settings: ResolvedSettings,
    pub remaining_arguments: Vec<OsString>,
    pub active_profile: SafeComponent,
    pub global_settings_path: PathBuf,
    pub profile_settings_path: PathBuf,
    pub ignored_profile_global_settings: Vec<String>,
    pub(crate) recipe: ResolutionRecipe,
}

impl fmt::Debug for LoadedSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoadedSettings")
            .field("roots", &self.roots)
            .field("settings", &self.settings)
            .field("remaining_argument_count", &self.remaining_arguments.len())
            .field("active_profile", &self.active_profile)
            .field("global_settings_path", &self.global_settings_path)
            .field("profile_settings_path", &self.profile_settings_path)
            .field(
                "ignored_profile_global_settings",
                &self.ignored_profile_global_settings,
            )
            .finish_non_exhaustive()
    }
}

/// Registry-driven settings pipeline.
#[derive(Clone, Debug)]
pub struct SettingsPipeline {
    request: PipelineRequest,
}

impl SettingsPipeline {
    #[must_use]
    pub const fn new(request: PipelineRequest) -> Self {
        Self { request }
    }

    /// Resolves bootstrap selectors before reading the profile and applies all
    /// layers in normative precedence order.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe error for registry, CLI, environment, TOML, path,
    /// type, scope, or cross-setting validation failures.
    pub fn load(self) -> Result<LoadedSettings, PipelineError> {
        self.load_stage(ResolutionStage::Complete)
    }

    /// Resolves the startup snapshot visible to the dynamic configuration
    /// worker before `init.py`/`init.lua` is evaluated.
    ///
    /// Bootstrap CLI selectors are already applied because they choose the
    /// profile and worker environment. Ordinary CLI settings remain excluded
    /// until [`Self::load`] performs the final, highest-priority layer.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe error for registry, bootstrap, environment,
    /// TOML, path, type, scope, or cross-setting validation failures.
    pub fn load_before_dynamic(self) -> Result<LoadedSettings, PipelineError> {
        self.load_stage(ResolutionStage::BeforeDynamic)
    }

    fn load_stage(self, stage: ResolutionStage) -> Result<LoadedSettings, PipelineError> {
        let validated = settings_registry()?;
        let registry = validated.registry().clone();
        let parsed_cli = ParsedCli::parse(&registry, &self.request.arguments)?;
        let bootstrap = resolve_bootstrap(&registry, &parsed_cli, &self.request)?;
        let store = TomlStore::new(LockManager::new(&bootstrap.roots));
        let profile = store.read(
            &bootstrap
                .roots
                .profile_settings(bootstrap.active_profile.as_str())?,
        )?;
        let mut resolution = resolve_layers(
            stage,
            &registry,
            &bootstrap.global,
            &profile,
            &bootstrap.roots,
            &self.request,
            &parsed_cli,
        )?;
        validate_snapshot(&resolution.values)?;
        let active_profile = resolution
            .values
            .get("active_profile")
            .and_then(|resolved| resolved.value.as_str())
            .ok_or_else(|| PipelineError::TypedLookup("active_profile".to_owned()))?;
        let mut active_profile = SafeComponent::new(active_profile)?;
        if active_profile != bootstrap.active_profile {
            let final_profile =
                store.read(&bootstrap.roots.profile_settings(active_profile.as_str())?)?;
            resolution = resolve_layers(
                stage,
                &registry,
                &bootstrap.global,
                &final_profile,
                &bootstrap.roots,
                &self.request,
                &parsed_cli,
            )?;
            validate_snapshot(&resolution.values)?;
            let resolved_profile = resolution
                .values
                .get("active_profile")
                .and_then(|resolved| resolved.value.as_str())
                .ok_or_else(|| PipelineError::TypedLookup("active_profile".to_owned()))?;
            active_profile = SafeComponent::new(resolved_profile)?;
        }
        let profile_settings_path = bootstrap.roots.profile_settings(active_profile.as_str())?;
        let roots = bootstrap.roots;
        let recipe = ResolutionRecipe {
            registry: registry.clone(),
            roots: roots.clone(),
            request: self.request,
            parsed_cli,
            global: bootstrap.global,
        };
        Ok(LoadedSettings {
            roots,
            settings: ResolvedSettings {
                registry,
                values: resolution.values,
                package_sources: resolution.package_sources,
            },
            remaining_arguments: recipe.parsed_cli.remaining.clone(),
            active_profile,
            global_settings_path: bootstrap.global_settings_path,
            profile_settings_path,
            ignored_profile_global_settings: resolution.ignored,
            recipe,
        })
    }
}

#[derive(Clone)]
pub(crate) struct ResolutionRecipe {
    pub(crate) registry: SettingsRegistry,
    pub(crate) roots: EffectiveRoots,
    pub(crate) request: PipelineRequest,
    parsed_cli: ParsedCli,
    pub(crate) global: SettingsDocument,
}

impl fmt::Debug for ResolutionRecipe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolutionRecipe")
            .field("roots", &self.roots)
            .field("request", &self.request)
            .field("parsed_cli", &self.parsed_cli)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl ResolutionRecipe {
    pub(crate) fn refresh_global(&mut self) -> Result<(), PipelineError> {
        self.global = TomlStore::new(LockManager::new(&self.roots))
            .read(&self.roots.config.join("settings.toml"))?;
        Ok(())
    }

    pub(crate) fn resolve_profile(
        &self,
        profile_name: &str,
    ) -> Result<ResolvedSettings, PipelineError> {
        let profile_name = SafeComponent::new(profile_name)?;
        let store = TomlStore::new(LockManager::new(&self.roots));
        let profile = store.read(&self.roots.profile_settings(profile_name.as_str())?)?;
        let mut resolution = resolve_full_layers(
            &self.registry,
            &self.global,
            &profile,
            &self.roots,
            &self.request,
            &self.parsed_cli,
        )?;
        resolution.values.insert(
            "active_profile".to_owned(),
            ResolvedValue {
                value: Value::String(profile_name.as_str().to_owned()),
                source: SettingSource::OpenApi,
            },
        );
        validate_snapshot(&resolution.values)?;
        Ok(ResolvedSettings {
            registry: self.registry.clone(),
            values: resolution.values,
            package_sources: resolution.package_sources,
        })
    }
}

#[derive(Clone)]
struct ParsedCli {
    assignments: BTreeMap<String, OsString>,
    remaining: Vec<OsString>,
}

impl fmt::Debug for ParsedCli {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedCli")
            .field("assignment_count", &self.assignments.len())
            .field("assignments", &"<redacted>")
            .field("remaining_argument_count", &self.remaining.len())
            .field("remaining", &"<redacted>")
            .finish()
    }
}

impl ParsedCli {
    fn parse(registry: &SettingsRegistry, arguments: &[OsString]) -> Result<Self, PipelineError> {
        let flags = registry
            .settings
            .iter()
            .flat_map(|setting| {
                setting
                    .surfaces
                    .cli
                    .flags
                    .iter()
                    .map(move |flag| (flag.as_str(), setting.id.as_str()))
            })
            .collect::<BTreeMap<_, _>>();
        let mut assignments = BTreeMap::new();
        let mut remaining = Vec::new();
        if let Some(program) = arguments.first() {
            remaining.push(program.clone());
        }
        let mut cursor = 1;
        while cursor < arguments.len() {
            let argument = arguments[cursor]
                .to_str()
                .ok_or(PipelineError::NonUnicodeArgument)?;
            if argument == "--" {
                remaining.extend(arguments[cursor..].iter().cloned());
                break;
            }
            let (flag, inline_value) = argument
                .split_once('=')
                .map_or((argument, None), |(flag, value)| (flag, Some(value)));
            if let Some(id) = flags.get(flag) {
                let value = if let Some(value) = inline_value {
                    OsString::from(value)
                } else {
                    cursor += 1;
                    arguments
                        .get(cursor)
                        .cloned()
                        .ok_or_else(|| PipelineError::MissingCliValue(flag.to_owned()))?
                };
                assignments.insert((*id).to_owned(), value);
                cursor += 1;
                continue;
            }
            remaining.push(arguments[cursor].clone());
            cursor += 1;
        }
        Ok(Self {
            assignments,
            remaining,
        })
    }
}

fn setting_by_id<'a>(
    registry: &'a SettingsRegistry,
    id: &str,
) -> Result<&'a Setting, PipelineError> {
    registry
        .settings
        .iter()
        .find(|setting| setting.id == id)
        .ok_or_else(|| PipelineError::MissingCanonicalSetting(id.to_owned()))
}

fn defaults(
    registry: &SettingsRegistry,
    roots: &EffectiveRoots,
    resource_root: &Path,
) -> Result<BTreeMap<String, ResolvedValue>, PipelineError> {
    registry
        .settings
        .iter()
        .map(|setting| {
            let value = default_value(setting, Some(roots), resource_root, SettingSource::Default)?;
            Ok((
                setting.id.clone(),
                ResolvedValue {
                    value,
                    source: SettingSource::Default,
                },
            ))
        })
        .collect()
}

fn default_value(
    setting: &Setting,
    roots: Option<&EffectiveRoots>,
    resource_root: &Path,
    source: SettingSource,
) -> Result<Value, PipelineError> {
    let value = match &setting.default {
        DefaultValue::Literal { value } => value.clone(),
        DefaultValue::DataPath { relative } => Value::String(
            roots
                .ok_or_else(|| PipelineError::BootstrapDefault(setting.id.clone()))?
                .data
                .join(relative)
                .to_string_lossy()
                .into_owned(),
        ),
        DefaultValue::ResourcePath { relative } => Value::String(
            lexical_normalize(resource_root.join(relative))
                .to_string_lossy()
                .into_owned(),
        ),
    };
    setting
        .value
        .validate(&value)
        .map_err(|reason| invalid(&setting.id, source, reason))?;
    Ok(value)
}

fn resolve_bootstrap(
    registry: &SettingsRegistry,
    parsed_cli: &ParsedCli,
    request: &PipelineRequest,
) -> Result<BootstrapResolution, PipelineError> {
    let app_setting = setting_by_id(registry, "app_name")?;
    let mut app_name = default_value(
        app_setting,
        None,
        &request.resource_root,
        SettingSource::Default,
    )?;
    if let Some(raw) = request.environment.get(&app_setting.surfaces.env.name) {
        app_name = parse_wire_value(app_setting, raw, SettingSource::Environment)?;
    }
    if let Some(raw) = parsed_cli.assignments.get("app_name") {
        app_name = parse_wire_value(app_setting, raw, SettingSource::CommandLine)?;
    }
    let app_name = app_name
        .as_str()
        .ok_or_else(|| invalid("app_name", SettingSource::Default, "expected string"))?;
    let app_name = SafeComponent::new(app_name)?;
    let bases = match &request.base_directories {
        Some(bases) => bases.clone(),
        None => BaseDirectories::native(&request.environment)?,
    };
    let roots = EffectiveRoots::from_bases(app_name, &bases);
    roots.ensure()?;
    let global_settings_path = roots.config.join("settings.toml");
    let global = TomlStore::new(LockManager::new(&roots)).read(&global_settings_path)?;
    let mut values = defaults(registry, &roots, &request.resource_root)?;
    let mut ignored = Vec::new();
    let context = LayerContext {
        registry,
        roots: &roots,
        request,
    };
    apply_toml(
        &context,
        &global,
        SettingSource::GlobalToml,
        &mut values,
        &mut ignored,
        None,
    )?;
    apply_environment(
        registry,
        &request.environment,
        true,
        &roots,
        request,
        &mut values,
        None,
    )?;
    apply_cli(
        registry,
        &parsed_cli.assignments,
        true,
        &roots,
        request,
        &mut values,
        None,
    )?;
    let active_profile = values
        .get("active_profile")
        .and_then(|resolved| resolved.value.as_str())
        .ok_or_else(|| PipelineError::TypedLookup("active_profile".to_owned()))?;
    Ok(BootstrapResolution {
        roots,
        global,
        global_settings_path,
        active_profile: SafeComponent::new(active_profile)?,
    })
}

fn resolve_full_layers(
    registry: &SettingsRegistry,
    global: &SettingsDocument,
    profile: &SettingsDocument,
    roots: &EffectiveRoots,
    request: &PipelineRequest,
    parsed_cli: &ParsedCli,
) -> Result<LayerResolution, PipelineError> {
    resolve_layers(
        ResolutionStage::Complete,
        registry,
        global,
        profile,
        roots,
        request,
        parsed_cli,
    )
}

fn resolve_layers(
    stage: ResolutionStage,
    registry: &SettingsRegistry,
    global: &SettingsDocument,
    profile: &SettingsDocument,
    roots: &EffectiveRoots,
    request: &PipelineRequest,
    parsed_cli: &ParsedCli,
) -> Result<LayerResolution, PipelineError> {
    let mut values = defaults(registry, roots, &request.resource_root)?;
    let mut package_sources = PackageSources::new();
    let mut ignored = Vec::new();
    let context = LayerContext {
        registry,
        roots,
        request,
    };
    apply_toml(
        &context,
        global,
        SettingSource::GlobalToml,
        &mut values,
        &mut ignored,
        Some(&mut package_sources),
    )?;
    apply_toml(
        &context,
        profile,
        SettingSource::ProfileToml,
        &mut values,
        &mut ignored,
        Some(&mut package_sources),
    )?;
    apply_environment(
        registry,
        &request.environment,
        false,
        roots,
        request,
        &mut values,
        Some(&mut package_sources),
    )?;
    match stage {
        ResolutionStage::BeforeDynamic => apply_cli(
            registry,
            &parsed_cli.assignments,
            true,
            roots,
            request,
            &mut values,
            Some(&mut package_sources),
        )?,
        ResolutionStage::Complete => {
            apply_dynamic(
                registry,
                &request.dynamic_values,
                roots,
                request,
                &mut values,
                Some(&mut package_sources),
            )?;
            apply_cli(
                registry,
                &parsed_cli.assignments,
                false,
                roots,
                request,
                &mut values,
                Some(&mut package_sources),
            )?;
        }
    }
    Ok(LayerResolution {
        values,
        package_sources,
        ignored,
    })
}

fn apply_toml(
    context: &LayerContext<'_>,
    document: &SettingsDocument,
    source: SettingSource,
    values: &mut BTreeMap<String, ResolvedValue>,
    ignored: &mut Vec<String>,
    mut package_sources: Option<&mut PackageSources>,
) -> Result<(), PipelineError> {
    for setting in &context.registry.settings {
        let Some(path) = setting.surfaces.toml.name.as_deref() else {
            continue;
        };
        let Some(toml_value) = document.get(path) else {
            continue;
        };
        if source == SettingSource::ProfileToml && setting.scope != Scope::Profile {
            ignored.push(setting.id.clone());
            continue;
        }
        let raw = serde_json::to_value(toml_value)
            .map_err(|_| invalid(&setting.id, source, "TOML value conversion failed"))?;
        let normalized = normalize_value(setting, raw, source, context.roots, context.request)?;
        insert_resolved(
            setting,
            normalized,
            source,
            context.roots,
            context.request,
            values,
            package_sources.as_deref_mut(),
        )?;
    }
    Ok(())
}

fn apply_environment(
    registry: &SettingsRegistry,
    environment: &RootEnvironment,
    bootstrap_only: bool,
    roots: &EffectiveRoots,
    request: &PipelineRequest,
    values: &mut BTreeMap<String, ResolvedValue>,
    mut package_sources: Option<&mut PackageSources>,
) -> Result<(), PipelineError> {
    for setting in &registry.settings {
        if bootstrap_only && !bootstrap_selector(setting) {
            continue;
        }
        let Some(raw) = environment.get(&setting.surfaces.env.name) else {
            continue;
        };
        let value = parse_wire_value(setting, raw, SettingSource::Environment)?;
        let value = normalize_value(setting, value, SettingSource::Environment, roots, request)?;
        insert_resolved(
            setting,
            value,
            SettingSource::Environment,
            roots,
            request,
            values,
            package_sources.as_deref_mut(),
        )?;
    }
    Ok(())
}

fn apply_cli(
    registry: &SettingsRegistry,
    assignments: &BTreeMap<String, OsString>,
    bootstrap_only: bool,
    roots: &EffectiveRoots,
    request: &PipelineRequest,
    values: &mut BTreeMap<String, ResolvedValue>,
    mut package_sources: Option<&mut PackageSources>,
) -> Result<(), PipelineError> {
    for setting in &registry.settings {
        if bootstrap_only && !bootstrap_selector(setting) {
            continue;
        }
        let Some(raw) = assignments.get(&setting.id) else {
            continue;
        };
        let value = parse_wire_value(setting, raw, SettingSource::CommandLine)?;
        let value = normalize_value(setting, value, SettingSource::CommandLine, roots, request)?;
        insert_resolved(
            setting,
            value,
            SettingSource::CommandLine,
            roots,
            request,
            values,
            package_sources.as_deref_mut(),
        )?;
    }
    Ok(())
}

fn apply_dynamic(
    registry: &SettingsRegistry,
    assignments: &BTreeMap<String, Value>,
    roots: &EffectiveRoots,
    request: &PipelineRequest,
    values: &mut BTreeMap<String, ResolvedValue>,
    mut package_sources: Option<&mut PackageSources>,
) -> Result<(), PipelineError> {
    for (id, raw) in assignments {
        let setting = setting_by_id(registry, id)?;
        if setting.scope == Scope::Bootstrap || setting.surfaces.dynamic.name.is_none() {
            return Err(PipelineError::UnsupportedDynamic(id.clone()));
        }
        let value = normalize_value(setting, raw.clone(), SettingSource::Dynamic, roots, request)?;
        insert_resolved(
            setting,
            value,
            SettingSource::Dynamic,
            roots,
            request,
            values,
            package_sources.as_deref_mut(),
        )?;
    }
    Ok(())
}

fn insert_resolved(
    setting: &Setting,
    value: Value,
    source: SettingSource,
    roots: &EffectiveRoots,
    request: &PipelineRequest,
    values: &mut BTreeMap<String, ResolvedValue>,
    package_sources: Option<&mut PackageSources>,
) -> Result<(), PipelineError> {
    if package_list_setting(&setting.id)
        && let Some(package_sources) = package_sources
    {
        let entries = serde_json::from_value::<Vec<RequirementEntry>>(value.clone())
            .map_err(|_| PipelineError::InvalidPackageList(setting.id.clone()))?;
        if entries.is_empty() {
            package_sources.remove(&setting.id);
        } else {
            let kind = package_source_kind(source)
                .ok_or_else(|| PipelineError::InvalidPackageList(setting.id.clone()))?;
            let relative_base = if source == SettingSource::CommandLine {
                request.startup_cwd.clone()
            } else {
                roots.config.clone()
            };
            let package_source = PackageSource {
                kind,
                entries,
                relative_base,
            };
            let mut prospective = package_sources
                .get(&setting.id)
                .cloned()
                .unwrap_or_default();
            prospective.push(package_source);
            ConstraintResolver::resolve(PythonWorker::Dynamic, prospective.clone(), true, false)?;
            package_sources.insert(setting.id.clone(), prospective);
        }
    }
    values.insert(setting.id.clone(), ResolvedValue { value, source });
    Ok(())
}

fn package_list_setting(id: &str) -> bool {
    matches!(
        id,
        "python.script.packages.list" | "python.dynamic.packages.list"
    )
}

const fn package_source_kind(source: SettingSource) -> Option<PackageSourceKind> {
    match source {
        SettingSource::GlobalToml => Some(PackageSourceKind::GlobalToml),
        SettingSource::ProfileToml => Some(PackageSourceKind::ProfileToml),
        SettingSource::Environment => Some(PackageSourceKind::Environment),
        SettingSource::Dynamic => Some(PackageSourceKind::Dynamic),
        SettingSource::CommandLine => Some(PackageSourceKind::CommandLine),
        SettingSource::Default | SettingSource::OpenApi => None,
    }
}

fn bootstrap_selector(setting: &Setting) -> bool {
    setting.scope == Scope::Bootstrap || setting.id == "active_profile"
}

fn parse_wire_value(
    setting: &Setting,
    raw: &OsStr,
    source: SettingSource,
) -> Result<Value, PipelineError> {
    let raw = raw
        .to_str()
        .ok_or_else(|| invalid(&setting.id, source, "value is not valid Unicode"))?;
    if setting.surfaces.cli.encoding == WireEncoding::StrictJson
        || setting.surfaces.env.encoding == WireEncoding::StrictJson
    {
        return serde_json::from_str(raw)
            .map_err(|_| invalid(&setting.id, source, "expected strict JSON"));
    }
    match &setting.value {
        ValueSchema::Boolean => parse_bool(raw)
            .map(Value::Bool)
            .ok_or_else(|| invalid(&setting.id, source, "expected explicit true or false")),
        ValueSchema::Integer { .. } => raw
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| invalid(&setting.id, source, "expected base-10 integer")),
        ValueSchema::Number { .. } => raw
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| invalid(&setting.id, source, "expected finite number")),
        ValueSchema::Union { variants }
            if setting.id == "camera.device"
                && variants
                    .iter()
                    .any(|variant| matches!(variant, ValueSchema::Integer { .. })) =>
        {
            if raw.bytes().all(|byte| byte.is_ascii_digit()) {
                raw.parse::<i64>()
                    .map(Value::from)
                    .map_err(|_| invalid(&setting.id, source, "camera index is out of range"))
            } else {
                Ok(Value::String(raw.to_owned()))
            }
        }
        ValueSchema::Null
        | ValueSchema::String { .. }
        | ValueSchema::Enum { .. }
        | ValueSchema::Union { .. } => Ok(Value::String(raw.to_owned())),
        ValueSchema::Array { .. } | ValueSchema::Object { .. } => {
            Err(invalid(&setting.id, source, "expected strict JSON"))
        }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Validates and normalizes a value originating from any settings surface.
///
/// # Errors
///
/// Returns a secret-safe error with canonical ID and source surface.
pub fn normalize_value(
    setting: &Setting,
    mut value: Value,
    source: SettingSource,
    roots: &EffectiveRoots,
    request: &PipelineRequest,
) -> Result<Value, PipelineError> {
    if let ValueSchema::Enum {
        values,
        aliases,
        ascii_case_insensitive,
    } = &setting.value
    {
        let candidate = value
            .as_str()
            .ok_or_else(|| invalid(&setting.id, source, "expected enum string"))?;
        let canonical = values
            .iter()
            .find(|canonical| {
                if *ascii_case_insensitive {
                    canonical.eq_ignore_ascii_case(candidate)
                } else {
                    canonical.as_str() == candidate
                }
            })
            .or_else(|| {
                aliases.iter().find_map(|(alias, canonical)| {
                    let matched = if *ascii_case_insensitive {
                        alias.eq_ignore_ascii_case(candidate)
                    } else {
                        alias == candidate
                    };
                    matched.then_some(canonical)
                })
            })
            .ok_or_else(|| {
                invalid(
                    &setting.id,
                    source,
                    format!("expected one of: {}", values.join(", ")),
                )
            })?;
        value = Value::String(canonical.clone());
    }
    setting
        .value
        .validate(&value)
        .map_err(|reason| invalid(&setting.id, source, reason))?;
    if let Some(metadata) = &setting.path {
        if let Some(raw) = value.as_str() {
            let path_source = source
                .path_source()
                .ok_or_else(|| invalid(&setting.id, source, "invalid default path source"))?;
            let resolved = resolve_path(
                raw,
                path_source,
                roots,
                &request.startup_cwd,
                &request.environment,
                metadata,
            )?;
            value = Value::String(resolved.to_string_lossy().into_owned());
        } else if !value.is_null() {
            return Err(invalid(&setting.id, source, "expected path string or null"));
        }
    }
    Ok(value)
}

pub(crate) fn validate_snapshot(
    values: &BTreeMap<String, ResolvedValue>,
) -> Result<(), PipelineError> {
    let integer = |id: &str| {
        values
            .get(id)
            .and_then(|value| value.value.as_i64())
            .ok_or_else(|| PipelineError::TypedLookup(id.to_owned()))
    };
    let soft = integer("dynamic.callback_soft_timeout_ms")?;
    let grace = integer("dynamic.callback_soft_timeout_grace_ms")?;
    let hard = integer("dynamic.callback_hard_timeout_ms")?;
    if soft != 0 && hard != 0 && hard < soft.saturating_add(grace) {
        return Err(PipelineError::CrossSetting(
            "dynamic.callback_hard_timeout_ms must be at least soft timeout plus grace".to_owned(),
        ));
    }
    let fps = integer("ui.fps")?;
    let options = values
        .get("ui.fps_options")
        .and_then(|value| value.value.as_array())
        .ok_or_else(|| PipelineError::TypedLookup("ui.fps_options".to_owned()))?;
    if !options.iter().any(|option| option.as_i64() == Some(fps)) {
        return Err(PipelineError::CrossSetting(
            "ui.fps must be present in ui.fps_options".to_owned(),
        ));
    }
    Ok(())
}

/// Returns a REST-safe value for a setting, masking every secret.
#[must_use]
pub fn public_value(setting: &Setting, value: &Value) -> Value {
    if setting.secret {
        serde_json::json!({"configured": value.as_str().is_some_and(|value| !value.is_empty())})
    } else {
        value.clone()
    }
}

/// Returns a dynamic-language-safe string for a secret getter.
#[must_use]
pub fn dynamic_secret_value(setting: &Setting, value: &Value) -> Value {
    if setting.secret && value.as_str().is_some_and(|value| !value.is_empty()) {
        Value::String(SECRET_MASK.to_owned())
    } else {
        value.clone()
    }
}

fn invalid(
    id: impl Into<String>,
    source: SettingSource,
    reason: impl Into<String>,
) -> PipelineError {
    PipelineError::InvalidValue {
        id: id.into(),
        surface: source,
        reason: reason.into(),
    }
}

/// Registry-driven settings resolution failures. Raw setting input is never
/// stored, which prevents secret values from entering error/debug surfaces.
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Root(#[from] RootError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error("command-line argument is not valid Unicode")]
    NonUnicodeArgument,
    #[error("missing explicit value for command-line flag {0}")]
    MissingCliValue(String),
    #[error("canonical registry is missing setting {0}")]
    MissingCanonicalSetting(String),
    #[error("bootstrap default requires resolved roots for {0}")]
    BootstrapDefault(String),
    #[error("invalid {id} value from {surface:?}: {reason}")]
    InvalidValue {
        id: String,
        surface: SettingSource,
        reason: String,
    },
    #[error("dynamic settings surface does not support {0}")]
    UnsupportedDynamic(String),
    #[error("canonical package list {0} is structurally invalid")]
    InvalidPackageList(String),
    #[error("cross-setting validation failed: {0}")]
    CrossSetting(String),
    #[error("canonical setting {0} has an unexpected resolved type")]
    TypedLookup(String),
    #[error("startup current directory is unavailable: {0}")]
    CurrentDirectory(std::io::Error),
    #[error("current executable is unavailable: {0}")]
    CurrentExecutable(std::io::Error),
    #[error("current executable has no resource root")]
    MissingResourceRoot,
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    use serde_json::json;
    use tempfile::TempDir;

    use super::{PipelineRequest, ResolvedValue, SettingSource, SettingsPipeline};
    use crate::package::{PackageSourceKind, PythonWorker, VersionSelector};
    use crate::roots::{BaseDirectories, RootEnvironment};

    fn request(
        temp: &TempDir,
        arguments: &[&str],
        environment: &[(&str, &str)],
    ) -> PipelineRequest {
        let base = temp.path();
        let root_environment = RootEnvironment::from_values(
            [("HOME", base.to_string_lossy().as_ref())]
                .into_iter()
                .chain(environment.iter().copied()),
        );
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("bases must resolve");
        PipelineRequest {
            arguments: arguments.iter().map(OsString::from).collect(),
            environment: root_environment,
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        }
    }

    #[test]
    fn bootstrap_app_name_and_profile_select_files_before_full_resolution() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let config = temp.path().join("config/App");
        fs::create_dir_all(config.join("profiles/Caps")).expect("fixture dirs must exist");
        fs::write(
            config.join("settings.toml"),
            "[profiles]\nactive_profile = \"from-toml\"\n[global]\nlanguage = \"ja\"\n",
        )
        .expect("global fixture must be writable");
        fs::write(
            config.join("profiles/Caps/settings.toml"),
            "[global]\nlanguage = \"ja\"\n[ui]\nui_fps = 60\nui_fps_options = [5, 15, 30, 60]\n",
        )
        .expect("profile fixture must be writable");
        let loaded = SettingsPipeline::new(request(
            &temp,
            &["pokecon", "--app-name", "App", "-p", "Caps", "--ui", "web"],
            &[("POKECON_PROFILE", "from-env")],
        ))
        .load()
        .expect("pipeline must resolve");
        assert_eq!(loaded.roots.app_name.as_str(), "App");
        assert_eq!(loaded.active_profile.as_str(), "Caps");
        assert_eq!(
            loaded.settings.integer("ui.fps").expect("fps must exist"),
            60
        );
        assert_eq!(
            loaded.remaining_arguments,
            vec![
                OsString::from("pokecon"),
                OsString::from("--ui"),
                OsString::from("web")
            ]
        );
        assert!(
            loaded
                .ignored_profile_global_settings
                .contains(&"language".to_owned())
        );
    }

    #[test]
    fn pre_dynamic_snapshot_applies_only_bootstrap_cli_settings() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let profile = temp.path().join("config/pokecon/profiles/CliProfile");
        fs::create_dir_all(&profile).expect("fixture dirs must exist");
        fs::write(
            profile.join("settings.toml"),
            "[ui]\nui_fps = 60\nui_fps_options = [15, 30, 60]\n",
        )
        .expect("profile fixture must be writable");
        let pipeline_request = request(
            &temp,
            &[
                "pokecon",
                "--profile",
                "CliProfile",
                "--dynamic-config-language",
                "python",
                "--python-dynamic-packages-list",
                r#"[{"name":"startup-only","version":">=1"}]"#,
                "--port",
                "9000",
            ],
            &[("POKECON_PORT", "8123")],
        );

        let before_dynamic = SettingsPipeline::new(pipeline_request.clone())
            .load_before_dynamic()
            .expect("pre-dynamic settings must resolve");
        assert_eq!(before_dynamic.active_profile.as_str(), "CliProfile");
        assert_eq!(
            before_dynamic
                .settings
                .string("dynamic_config_language")
                .expect("language must exist"),
            "python"
        );
        assert_eq!(
            before_dynamic
                .settings
                .get("dynamic_config_language")
                .expect("language must exist")
                .source,
            SettingSource::CommandLine
        );
        assert_eq!(
            before_dynamic
                .settings
                .integer("server.port")
                .expect("port must exist"),
            8123
        );
        assert_eq!(
            before_dynamic
                .settings
                .get("server.port")
                .expect("port must exist")
                .source,
            SettingSource::Environment
        );
        let dynamic_packages = before_dynamic
            .settings
            .package_sources("python.dynamic.packages.list");
        assert_eq!(dynamic_packages.len(), 1);
        assert_eq!(dynamic_packages[0].kind, PackageSourceKind::CommandLine);

        let complete = SettingsPipeline::new(pipeline_request)
            .load()
            .expect("complete settings must resolve");
        assert_eq!(
            complete
                .settings
                .integer("server.port")
                .expect("port must exist"),
            9000
        );
        assert_eq!(
            complete
                .settings
                .get("server.port")
                .expect("port must exist")
                .source,
            SettingSource::CommandLine
        );
    }

    #[test]
    fn precedence_enum_bool_json_and_path_bases_are_surface_aware() {
        let temp = TempDir::new().expect("temporary directory must exist");
        fs::create_dir_all(temp.path().join("config/pokecon/profiles/default"))
            .expect("fixture dirs must exist");
        fs::create_dir_all(temp.path().join("cwd/cli-venv")).expect("CLI venv must exist");
        fs::write(
            temp.path().join("config/pokecon/settings.toml"),
            "[camera]\nflip_mode = \"vertical\"\n[input]\ntouchscreen_area = { left = 0.0, top = 0.0, right = 0.8, bottom = 0.8 }\n",
        )
        .expect("fixture must be writable");
        let loaded = SettingsPipeline::new(request(
            &temp,
            &[
                "pokecon",
                "--camera-flip-mode",
                "BOTH",
                "--auto-reload-config",
                "TRUE",
                "--input-touchscreen-area",
                "{\"left\":0.1,\"top\":0.1,\"right\":0.9,\"bottom\":0.9}",
                "--python-script-venv",
                "cli-venv",
            ],
            &[("POKECON_CAMERA_FLIP_MODE", "horizontal")],
        ))
        .load()
        .expect("pipeline must resolve");
        assert_eq!(
            loaded
                .settings
                .string("camera.flip_mode")
                .expect("enum must exist"),
            "both"
        );
        assert!(
            loaded
                .settings
                .boolean("auto_reload_config")
                .expect("bool must exist")
        );
        let venv = loaded
            .settings
            .string("python.script.venv")
            .expect("path must exist");
        assert_eq!(PathBuf::from(venv), temp.path().join("cwd/cli-venv"));
        assert_eq!(
            loaded
                .settings
                .get("camera.flip_mode")
                .expect("setting must exist")
                .source,
            SettingSource::CommandLine
        );
    }

    #[test]
    fn package_lists_merge_by_distribution_across_every_precedence_layer() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let profile = temp.path().join("config/pokecon/profiles/default");
        fs::create_dir_all(&profile).expect("fixture dirs must exist");
        fs::write(
            temp.path().join("config/pokecon/settings.toml"),
            "[python.script.packages]\nlist = [{ name = \"Demo_Pkg\", version = \">=1\", extras = [\"a\"] }]\n",
        )
        .expect("global fixture must be writable");
        fs::write(
            profile.join("settings.toml"),
            "[python.script.packages]\nlist = [{ name = \"demo-pkg\", version = \"<5\", extras = [\"b\"] }]\n",
        )
        .expect("profile fixture must be writable");
        let mut pipeline_request = request(
            &temp,
            &[
                "pokecon",
                "--python-script-packages-list",
                r#"[{"name":"demo.pkg","version":"<4","extras":["e"]}]"#,
            ],
            &[(
                "POKECON_PYTHON_SCRIPT_PACKAGES_LIST",
                r#"[{"name":"demo-pkg","version":"!=2","extras":["c"]}]"#,
            )],
        );
        pipeline_request.dynamic_values.insert(
            "python.script.packages.list".to_owned(),
            json!([{"name": "demo-pkg", "version": "!=3", "extras": ["d"]}]),
        );
        let loaded = SettingsPipeline::new(pipeline_request)
            .load()
            .expect("package-aware pipeline must resolve");
        assert_eq!(
            loaded
                .settings
                .package_sources("python.script.packages.list")
                .len(),
            5
        );
        let resolution = loaded
            .settings
            .resolve_packages(PythonWorker::Script)
            .expect("merged packages must resolve with application requirements");
        let package = &resolution.packages["demo-pkg"];
        assert_eq!(package.extras, ["a", "b", "c", "d", "e"]);
        assert!(matches!(package.selector, VersionSelector::Analyzed(_)));
        assert_eq!(
            package.requirement(),
            "demo.pkg[a,b,c,d,e]>=1,<5,!=2,!=3,<4"
        );
    }

    #[test]
    fn dynamic_profile_selection_reloads_the_selected_profile_before_startup_finishes() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let target = temp.path().join("config/pokecon/profiles/from-dynamic");
        fs::create_dir_all(&target).expect("target profile must exist");
        fs::write(
            target.join("settings.toml"),
            "[ui]\nui_fps = 60\nui_fps_options = [15, 30, 60]\n",
        )
        .expect("target profile must be writable");
        let mut pipeline_request = request(&temp, &["pokecon"], &[]);
        pipeline_request
            .dynamic_values
            .insert("active_profile".to_owned(), json!("from-dynamic"));
        let loaded = SettingsPipeline::new(pipeline_request)
            .load()
            .expect("dynamic profile selection must resolve");
        assert_eq!(loaded.active_profile.as_str(), "from-dynamic");
        assert_eq!(
            loaded.settings.integer("ui.fps").expect("fps must exist"),
            60
        );
        assert!(
            loaded
                .profile_settings_path
                .ends_with("profiles/from-dynamic/settings.toml")
        );
    }

    #[test]
    fn explicit_empty_package_list_clears_lower_layers_before_higher_sources() {
        let temp = TempDir::new().expect("temporary directory must exist");
        fs::create_dir_all(temp.path().join("config/pokecon/profiles/default"))
            .expect("fixture dirs must exist");
        fs::write(
            temp.path().join("config/pokecon/settings.toml"),
            "[python.script.packages]\nlist = [{ name = \"old\", version = \">=1\" }]\n",
        )
        .expect("global fixture must be writable");
        let loaded = SettingsPipeline::new(request(
            &temp,
            &[
                "pokecon",
                "--python-script-packages-list",
                r#"[{"name":"new","version":">=10"}]"#,
            ],
            &[("POKECON_PYTHON_SCRIPT_PACKAGES_LIST", "[]")],
        ))
        .load()
        .expect("clear followed by a higher source must resolve");
        let sources = loaded
            .settings
            .package_sources("python.script.packages.list");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].entries[0].name, "new");
        let resolution = loaded
            .settings
            .resolve_packages(PythonWorker::Script)
            .expect("cleared package set must resolve");
        assert!(!resolution.packages.contains_key("old"));
        assert_eq!(resolution.packages["new"].requirement(), "new>=10");
    }

    #[test]
    fn unsafe_bootstrap_names_never_produce_roots() {
        let temp = TempDir::new().expect("temporary directory must exist");
        for invalid in ["", ".", "..", "../escape", "C:\\escape", "\\\\server"] {
            let result =
                SettingsPipeline::new(request(&temp, &["pokecon", "--app-name", invalid], &[]))
                    .load();
            assert!(result.is_err(), "accepted {invalid:?}");
            assert!(!temp.path().join("escape").exists());
        }
    }

    #[test]
    fn debug_surfaces_redact_cli_environment_dynamic_and_resolved_values() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let secret = "super-secret-token";
        let mut pipeline_request = request(
            &temp,
            &["pokecon", "--notifications-discord-webhook-url", secret],
            &[("POKECON_UV_INDEX_URL", secret)],
        );
        pipeline_request.dynamic_values.insert(
            "notifications.discord.webhook_url".to_owned(),
            json!(secret),
        );
        assert!(!format!("{pipeline_request:?}").contains(secret));
        let resolved = ResolvedValue {
            value: json!(secret),
            source: SettingSource::Environment,
        };
        assert!(!format!("{resolved:?}").contains(secret));
    }

    #[test]
    fn every_canonical_setting_and_environment_name_is_projected_once() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let loaded = SettingsPipeline::new(request(&temp, &["pokecon"], &[]))
            .load()
            .expect("pipeline must resolve");
        assert_eq!(loaded.settings.values().len(), 78);
        let names = loaded
            .settings
            .registry()
            .settings
            .iter()
            .map(|setting| setting.surfaces.env.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), 78);
    }
}
