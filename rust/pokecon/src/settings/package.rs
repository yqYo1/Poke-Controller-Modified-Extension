use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use pep440_rs::{Version, VersionSpecifier, VersionSpecifiers};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use version_ranges::Ranges;

use crate::settings::APPLICATION_REQUIREMENTS_JSON;
use crate::settings::path::lexical_normalize;

/// Python worker whose application requirement group is selected.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonWorker {
    Script,
    Dynamic,
}

/// Canonical priority layers for package provenance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSourceKind {
    Application,
    GlobalToml,
    ProfileToml,
    Environment,
    Dynamic,
    CommandLine,
}

/// Exact origin and order of a retained constraint unit.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: PackageSourceKind,
    pub source_index: usize,
    pub entry_index: usize,
    pub clause_index: usize,
}

const fn provenance_key(provenance: &Provenance) -> (usize, usize, usize) {
    (
        provenance.source_index,
        provenance.entry_index,
        provenance.clause_index,
    )
}

/// User/application requirement input.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementEntry {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub extras: Vec<String>,
}

impl fmt::Debug for RequirementEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequirementEntry")
            .field("name", &self.name)
            .field("version", &self.version.as_ref().map(|_| "<redacted>"))
            .field("extras", &self.extras)
            .finish()
    }
}

/// One settings layer, including the correct base for relative direct paths.
#[derive(Clone, Debug)]
pub struct PackageSource {
    pub kind: PackageSourceKind,
    pub entries: Vec<RequirementEntry>,
    pub relative_base: PathBuf,
}

/// A normalized PEP 440 comparison clause with lossless provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzedClause {
    pub normalized: String,
    pub provenance: Provenance,
}

/// A direct URL/VCS/local source, treated as one atomic version identity.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DirectSourceClause {
    pub source: String,
    pub identity: String,
    pub mutable: bool,
    pub provenance: Provenance,
}

impl fmt::Debug for DirectSourceClause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DirectSourceClause")
            .field("source", &"<redacted>")
            .field("identity", &"<redacted>")
            .field("mutable", &self.mutable)
            .field("provenance", &self.provenance)
            .finish()
    }
}

/// Unsupported/future syntax retained byte-for-byte for uv to accept/reject.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpaqueFragment {
    pub text: String,
    pub provenance: Provenance,
}

impl fmt::Debug for OpaqueFragment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaqueFragment")
            .field("text", &"<redacted>")
            .field("provenance", &self.provenance)
            .finish()
    }
}

/// Final mutually-exclusive version selector.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum VersionSelector {
    Unconstrained,
    Analyzed(Vec<AnalyzedClause>),
    Direct(DirectSourceClause),
}

/// Deterministically merged package requirement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPackage {
    pub normalized_name: String,
    pub display_name: String,
    pub extras: Vec<String>,
    pub selector: VersionSelector,
    pub opaque: Vec<OpaqueFragment>,
}

impl ResolvedPackage {
    /// Reconstructs the primary uv/PEP 508 requirement without dropping any
    /// analyzed or direct selector.
    #[must_use]
    pub fn requirement(&self) -> String {
        let name = if self.extras.is_empty() {
            self.display_name.clone()
        } else {
            format!("{}[{}]", self.display_name, self.extras.join(","))
        };
        let mut requirement = match &self.selector {
            VersionSelector::Unconstrained => name,
            VersionSelector::Analyzed(clauses) => format!(
                "{name}{}",
                clauses
                    .iter()
                    .map(|clause| clause.normalized.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            VersionSelector::Direct(direct) => format!("{name} @ {}", direct.source),
        };
        let mut opaque = self.opaque.iter().collect::<Vec<_>>();
        opaque.sort_by_key(|fragment| provenance_key(&fragment.provenance));
        for fragment in opaque {
            if fragment.text.starts_with(';') {
                requirement.push_str(&fragment.text);
            } else {
                if !matches!(self.selector, VersionSelector::Unconstrained)
                    && !fragment.text.starts_with(',')
                {
                    requirement.push(',');
                }
                requirement.push_str(&fragment.text);
            }
        }
        requirement
    }
}

/// Complete package resolution and the corresponding uv input channels.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageResolution {
    pub packages: BTreeMap<String, ResolvedPackage>,
    pub requirements: Vec<String>,
    pub constraints: Vec<String>,
    pub overrides: Vec<String>,
    pub opaque: Vec<String>,
}

impl fmt::Debug for PackageResolution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PackageResolution")
            .field("package_names", &self.packages.keys().collect::<Vec<_>>())
            .field("requirements", &"<redacted>")
            .field("constraints", &"<redacted>")
            .field("overrides", &"<redacted>")
            .field("opaque", &"<redacted>")
            .finish()
    }
}

/// Build-time dependency source sets parsed from canonical `pyproject.toml`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationRequirements {
    pub common: Vec<String>,
    pub worker_script: Vec<String>,
    pub worker_dynamic: Vec<String>,
    pub optional: BTreeMap<String, Vec<String>>,
}

impl ApplicationRequirements {
    /// Loads the build-script projection.
    ///
    /// # Errors
    ///
    /// Returns an error if the generated build artifact is invalid.
    pub fn embedded() -> Result<Self, PackageError> {
        serde_json::from_str(APPLICATION_REQUIREMENTS_JSON).map_err(PackageError::EmbeddedManifest)
    }

    /// Returns common plus exactly one worker-specific source set.
    ///
    /// # Errors
    ///
    /// Returns an error when an embedded requirement cannot be parsed.
    pub fn entries(&self, worker: PythonWorker) -> Result<Vec<RequirementEntry>, PackageError> {
        self.common
            .iter()
            .chain(match worker {
                PythonWorker::Script => self.worker_script.iter(),
                PythonWorker::Dynamic => self.worker_dynamic.iter(),
            })
            .map(|requirement| parse_application_requirement(requirement))
            .collect()
    }
}

/// Package-aware merge and clause-level MUS resolver.
#[derive(Clone, Debug, Default)]
pub struct ConstraintResolver;

impl ConstraintResolver {
    /// Resolves application requirements and all user sources.
    ///
    /// Application/user priority is reversed only at their boundary when
    /// `override_application_constraints` is true. Metadata override output is
    /// independently mapped to uv overrides.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names/extras/direct source identities or an
    /// internally contradictory high-priority/static source.
    pub fn resolve(
        worker: PythonWorker,
        mut user_sources: Vec<PackageSource>,
        override_application_constraints: bool,
        override_package_metadata_constraints: bool,
    ) -> Result<PackageResolution, PackageError> {
        let application = ApplicationRequirements::embedded()?;
        let application_source = PackageSource {
            kind: PackageSourceKind::Application,
            entries: application.entries(worker)?,
            relative_base: PathBuf::new(),
        };
        let sources = if override_application_constraints {
            let mut sources = vec![application_source];
            sources.append(&mut user_sources);
            sources
        } else {
            user_sources.push(application_source);
            user_sources
        };
        let mut accumulated = BTreeMap::<String, PackageAccumulator>::new();
        for (source_index, source) in sources.iter().enumerate() {
            let resolved_source = resolve_single_source(source, source_index)?;
            for (name, high) in resolved_source {
                match accumulated.remove(&name) {
                    Some(low) => {
                        accumulated.insert(name, merge_across_sources(low, high)?);
                    }
                    None => {
                        accumulated.insert(name, high);
                    }
                }
            }
        }
        let packages = accumulated
            .into_iter()
            .map(|(name, package)| (name, package.finish()))
            .collect::<BTreeMap<_, _>>();
        let requirements = packages
            .values()
            .map(ResolvedPackage::requirement)
            .collect::<Vec<_>>();
        let mut constraints = Vec::new();
        let mut overrides = Vec::new();
        let mut opaque = Vec::new();
        for package in packages.values() {
            if matches!(package.selector, VersionSelector::Analyzed(_)) {
                if override_package_metadata_constraints
                    && package
                        .selector_provenance()
                        .iter()
                        .any(|provenance| provenance.source != PackageSourceKind::Application)
                {
                    overrides.push(package.requirement());
                } else {
                    constraints.push(package.requirement());
                }
            }
            opaque.extend(package.opaque.iter().map(|fragment| fragment.text.clone()));
        }
        Ok(PackageResolution {
            packages,
            requirements,
            constraints,
            overrides,
            opaque,
        })
    }
}

impl ResolvedPackage {
    fn selector_provenance(&self) -> Vec<&Provenance> {
        match &self.selector {
            VersionSelector::Unconstrained => Vec::new(),
            VersionSelector::Analyzed(clauses) => {
                clauses.iter().map(|clause| &clause.provenance).collect()
            }
            VersionSelector::Direct(direct) => vec![&direct.provenance],
        }
    }
}

#[derive(Clone)]
struct PackageAccumulator {
    normalized_name: String,
    display_name: String,
    extras: BTreeSet<String>,
    clauses: Vec<AnalyzedClause>,
    direct: Option<DirectSourceClause>,
    opaque: Vec<OpaqueFragment>,
}

impl PackageAccumulator {
    fn new(normalized_name: String, display_name: String) -> Self {
        Self {
            normalized_name,
            display_name,
            extras: BTreeSet::new(),
            clauses: Vec::new(),
            direct: None,
            opaque: Vec::new(),
        }
    }

    fn finish(self) -> ResolvedPackage {
        let selector = if let Some(direct) = self.direct {
            VersionSelector::Direct(direct)
        } else if self.clauses.is_empty() {
            VersionSelector::Unconstrained
        } else {
            VersionSelector::Analyzed(self.clauses)
        };
        ResolvedPackage {
            normalized_name: self.normalized_name,
            display_name: self.display_name,
            extras: self.extras.into_iter().collect(),
            selector,
            opaque: self.opaque,
        }
    }
}

fn resolve_single_source(
    source: &PackageSource,
    source_index: usize,
) -> Result<BTreeMap<String, PackageAccumulator>, PackageError> {
    let mut packages = BTreeMap::<String, PackageAccumulator>::new();
    for (entry_index, entry) in source.entries.iter().enumerate() {
        let normalized_name = normalize_package_name(&entry.name)?;
        let parsed = parse_entry(entry, source, source_index, entry_index)?;
        let package = packages
            .entry(normalized_name.clone())
            .or_insert_with(|| PackageAccumulator::new(normalized_name, entry.name.clone()));
        package.display_name.clone_from(&entry.name);
        package.extras.extend(parsed.extras);
        for selector in parsed.selectors {
            match selector {
                ParsedSelector::Analyzed(clauses) => {
                    if package.direct.take().is_some() {
                        package.clauses.clear();
                    }
                    package.clauses.extend(clauses);
                    if !satisfiable(&package.clauses)? {
                        return Err(PackageError::ContradictorySource {
                            package: package.normalized_name.clone(),
                            layer: source.kind,
                        });
                    }
                }
                ParsedSelector::Direct(direct) => {
                    package.clauses.clear();
                    package.direct = Some(direct);
                }
            }
        }
        for fragment in parsed.opaque {
            if !package
                .opaque
                .iter()
                .any(|current| current.text == fragment.text)
            {
                package.opaque.push(fragment);
            }
        }
    }
    Ok(packages)
}

fn merge_across_sources(
    mut low: PackageAccumulator,
    high: PackageAccumulator,
) -> Result<PackageAccumulator, PackageError> {
    low.display_name = high.display_name;
    low.extras.extend(high.extras);
    if let Some(direct) = high.direct {
        low.clauses.clear();
        low.direct = Some(direct);
    } else if !high.clauses.is_empty() {
        low.direct = None;
        if !satisfiable(&high.clauses)? {
            return Err(PackageError::ContradictorySource {
                package: low.normalized_name,
                layer: high.clauses[0].provenance.source,
            });
        }
        if !satisfiable_combined(&low.clauses, &high.clauses)? {
            let removals = low_clauses_in_all_minimal_conflicts(&low.clauses, &high.clauses)?;
            low.clauses = low
                .clauses
                .into_iter()
                .enumerate()
                .filter_map(|(index, clause)| (!removals.contains(&index)).then_some(clause))
                .collect();
        }
        low.clauses.extend(high.clauses);
        if !satisfiable(&low.clauses)? {
            return Err(PackageError::InternalResolution(low.normalized_name));
        }
    }
    for fragment in high.opaque {
        if !low
            .opaque
            .iter()
            .any(|current| current.text == fragment.text)
        {
            low.opaque.push(fragment);
        }
    }
    Ok(low)
}

struct ParsedEntry {
    selectors: Vec<ParsedSelector>,
    opaque: Vec<OpaqueFragment>,
    extras: BTreeSet<String>,
}

enum ParsedSelector {
    Analyzed(Vec<AnalyzedClause>),
    Direct(DirectSourceClause),
}

fn parse_entry(
    entry: &RequirementEntry,
    source: &PackageSource,
    source_index: usize,
    entry_index: usize,
) -> Result<ParsedEntry, PackageError> {
    let normalized_name = normalize_package_name(&entry.name)?;
    let mut extras = entry
        .extras
        .iter()
        .map(|extra| normalize_extra(extra))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let Some(version) = entry.version.as_deref() else {
        return Ok(ParsedEntry {
            selectors: Vec::new(),
            opaque: Vec::new(),
            extras,
        });
    };
    let (specifier_text, marker) = version
        .split_once(';')
        .map_or((version, None), |(specifiers, marker)| {
            (specifiers, Some(format!(";{marker}")))
        });
    let (specifier_text, direct_text) = split_direct_selector(specifier_text, &normalized_name)?;
    let mut clauses = Vec::new();
    let mut opaque = Vec::new();
    for (clause_index, unit) in specifier_text.split(',').enumerate() {
        let unit = unit.trim();
        if unit.is_empty() {
            continue;
        }
        let provenance = Provenance {
            source: source.kind,
            source_index,
            entry_index,
            clause_index,
        };
        if unit.contains("===") {
            opaque.push(OpaqueFragment {
                text: unit.to_owned(),
                provenance,
            });
        } else if let Ok(specifier) = VersionSpecifier::from_str(unit) {
            clauses.push(AnalyzedClause {
                normalized: specifier.to_string(),
                provenance,
            });
        } else {
            opaque.push(OpaqueFragment {
                text: unit.to_owned(),
                provenance,
            });
        }
    }
    if let Some(marker) = marker {
        opaque.push(OpaqueFragment {
            text: marker,
            provenance: Provenance {
                source: source.kind,
                source_index,
                entry_index,
                clause_index: clauses.len() + opaque.len(),
            },
        });
    }
    let direct_clause_index = specifier_text.split(',').count();
    let mut selectors = Vec::new();
    if !clauses.is_empty() {
        selectors.push(ParsedSelector::Analyzed(clauses));
    }
    if let Some(direct_text) = direct_text {
        let direct = parse_direct_source(
            direct_text,
            &normalized_name,
            source,
            source_index,
            entry_index,
            direct_clause_index,
            &mut extras,
        )?
        .ok_or_else(|| PackageError::InvalidDirectSource(normalized_name.clone()))?;
        selectors.push(ParsedSelector::Direct(direct));
    }
    Ok(ParsedEntry {
        selectors,
        opaque,
        extras,
    })
}

fn split_direct_selector<'a>(
    value: &'a str,
    outer_name: &str,
) -> Result<(&'a str, Option<&'a str>), PackageError> {
    let trimmed = value.trim();
    if trimmed.starts_with('@') || direct_source_syntax(trimmed) {
        return Ok(("", Some(trimmed)));
    }
    let Some((prefix, _direct)) = trimmed.split_once(" @ ") else {
        return Ok((trimmed, None));
    };
    let prefix = prefix.trim();
    if let Ok((inner_name, _)) = split_name_and_extras(prefix)
        && normalize_package_name(inner_name).is_ok()
    {
        return Ok(("", Some(trimmed)));
    }
    let direct_offset = trimmed
        .find(" @ ")
        .expect("split_once found the direct-source separator")
        + 1;
    if prefix.is_empty() {
        return Err(PackageError::InvalidDirectSource(outer_name.to_owned()));
    }
    Ok((prefix, Some(&trimmed[direct_offset..])))
}

fn parse_direct_source(
    version: &str,
    outer_name: &str,
    source: &PackageSource,
    source_index: usize,
    entry_index: usize,
    clause_index: usize,
    extras: &mut BTreeSet<String>,
) -> Result<Option<DirectSourceClause>, PackageError> {
    let trimmed = version.trim();
    let direct_part = if let Some((inner, direct)) = trimmed.split_once(" @ ") {
        let (inner_name, inner_extras) = split_name_and_extras(inner)?;
        if normalize_package_name(inner_name)? != outer_name {
            return Err(PackageError::DirectNameMismatch(outer_name.to_owned()));
        }
        extras.extend(
            inner_extras
                .into_iter()
                .map(|extra| normalize_extra(&extra))
                .collect::<Result<BTreeSet<_>, _>>()?,
        );
        direct.trim()
    } else if let Some(direct) = trimmed.strip_prefix('@') {
        direct.trim()
    } else if direct_source_syntax(trimmed) {
        trimmed
    } else {
        return Ok(None);
    };
    if direct_part.is_empty() {
        return Err(PackageError::InvalidDirectSource(outer_name.to_owned()));
    }
    let identity = direct_identity(direct_part, &source.relative_base);
    let mutable = mutable_direct_source(direct_part);
    Ok(Some(DirectSourceClause {
        source: direct_part.to_owned(),
        identity,
        mutable,
        provenance: Provenance {
            source: source.kind,
            source_index,
            entry_index,
            clause_index,
        },
    }))
}

fn direct_source_syntax(value: &str) -> bool {
    value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("file://")
        || value.starts_with("git+")
        || value.starts_with("hg+")
        || value.starts_with("svn+")
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with(".\\")
        || value.starts_with("..\\")
}

fn direct_identity(source: &str, relative_base: &Path) -> String {
    if source.starts_with('/') || source.starts_with("./") || source.starts_with("../") {
        let path = PathBuf::from(source);
        lexical_normalize(if path.is_absolute() {
            path
        } else {
            relative_base.join(path)
        })
        .to_string_lossy()
        .into_owned()
    } else {
        source.to_owned()
    }
}

fn mutable_direct_source(source: &str) -> bool {
    if source.starts_with("file://")
        || source.starts_with('/')
        || source.starts_with("./")
        || source.starts_with("../")
    {
        return true;
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        return !source.contains("hash=sha256:");
    }
    if source.starts_with("git+") || source.starts_with("hg+") || source.starts_with("svn+") {
        let revision = source.rsplit_once('@').map(|(_, revision)| revision);
        return revision.is_none_or(|revision| {
            let revision = revision.split(['#', '?']).next().unwrap_or(revision);
            revision.len() < 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
    }
    true
}

fn split_name_and_extras(value: &str) -> Result<(&str, Vec<String>), PackageError> {
    let value = value.trim();
    let Some(open) = value.find('[') else {
        return Ok((value, Vec::new()));
    };
    let close = value
        .strip_suffix(']')
        .map(str::len)
        .ok_or_else(|| PackageError::InvalidRequirementName(value.to_owned()))?;
    let extras = value[open + 1..close]
        .split(',')
        .map(str::trim)
        .map(ToOwned::to_owned)
        .collect();
    Ok((&value[..open], extras))
}

fn parse_application_requirement(requirement: &str) -> Result<RequirementEntry, PackageError> {
    let operator = requirement
        .find(['<', '>', '=', '!', '~', '@'])
        .unwrap_or(requirement.len());
    let (name_and_extras, version) = requirement.split_at(operator);
    let (name, extras) = split_name_and_extras(name_and_extras)?;
    normalize_package_name(name)?;
    Ok(RequirementEntry {
        name: name.to_owned(),
        version: (!version.is_empty()).then(|| version.to_owned()),
        extras,
    })
}

/// PEP 503 distribution-name normalization.
///
/// # Errors
///
/// Returns an error for empty names or characters outside the PEP 508 name
/// alphabet.
pub fn normalize_package_name(name: &str) -> Result<String, PackageError> {
    normalize_identifier(name, false)
}

fn normalize_extra(extra: &str) -> Result<String, PackageError> {
    normalize_identifier(extra, true)
}

fn normalize_identifier(value: &str, extra: bool) -> Result<String, PackageError> {
    if value.is_empty()
        || !value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(if extra {
            PackageError::InvalidExtra
        } else {
            PackageError::InvalidRequirementName(value.to_owned())
        });
    }
    let mut normalized = String::with_capacity(value.len());
    let mut separator = false;
    for byte in value.bytes() {
        if matches!(byte, b'-' | b'_' | b'.') {
            separator = true;
        } else {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(char::from(byte.to_ascii_lowercase()));
            separator = false;
        }
    }
    debug_assert!(
        !separator,
        "validated identifiers end in an alphanumeric byte"
    );
    Ok(normalized)
}

fn satisfiable(clauses: &[AnalyzedClause]) -> Result<bool, PackageError> {
    let specifiers = clauses
        .iter()
        .map(|clause| {
            VersionSpecifier::from_str(&clause.normalized)
                .map_err(|_| PackageError::InternalClause(clause.normalized.clone()))
        })
        .collect::<Result<VersionSpecifiers, _>>()?;
    let ranges = Ranges::<Version>::from(specifiers);
    Ok(!ranges.is_empty())
}

fn satisfiable_combined(
    low: &[AnalyzedClause],
    high: &[AnalyzedClause],
) -> Result<bool, PackageError> {
    let combined = low.iter().chain(high).cloned().collect::<Vec<_>>();
    satisfiable(&combined)
}

fn low_clauses_in_all_minimal_conflicts(
    low: &[AnalyzedClause],
    high: &[AnalyzedClause],
) -> Result<BTreeSet<usize>, PackageError> {
    let combined = low.iter().chain(high).cloned().collect::<Vec<_>>();
    let mut minimal_sets = Vec::<Vec<usize>>::new();
    for cardinality in 1..=combined.len() {
        let mut selected = Vec::with_capacity(cardinality);
        enumerate_combinations(&combined, cardinality, 0, &mut selected, &mut minimal_sets)?;
    }
    let removals = minimal_sets
        .iter()
        .flatten()
        .copied()
        .filter(|index| *index < low.len())
        .collect::<BTreeSet<_>>();
    if removals.is_empty() {
        return Err(PackageError::InternalResolution(
            low.first()
                .or_else(|| high.first())
                .map_or_else(|| "unknown".to_owned(), |clause| clause.normalized.clone()),
        ));
    }
    Ok(removals)
}

fn enumerate_combinations(
    clauses: &[AnalyzedClause],
    target_length: usize,
    start: usize,
    selected: &mut Vec<usize>,
    minimal_sets: &mut Vec<Vec<usize>>,
) -> Result<(), PackageError> {
    if selected.len() == target_length {
        if minimal_sets
            .iter()
            .any(|minimal| minimal.iter().all(|index| selected.contains(index)))
        {
            return Ok(());
        }
        let subset = selected
            .iter()
            .map(|index| clauses[*index].clone())
            .collect::<Vec<_>>();
        if !satisfiable(&subset)? {
            minimal_sets.push(selected.clone());
        }
        return Ok(());
    }
    let remaining = target_length - selected.len();
    let last_start = clauses.len().saturating_sub(remaining);
    for index in start..=last_start {
        selected.push(index);
        enumerate_combinations(clauses, target_length, index + 1, selected, minimal_sets)?;
        selected.pop();
    }
    Ok(())
}

/// Package constraint parsing and resolution failures. Direct source values and
/// opaque fragments never appear in error messages.
#[derive(Debug, Error)]
pub enum PackageError {
    #[error("embedded application requirement manifest is invalid")]
    EmbeddedManifest(#[source] serde_json::Error),
    #[error("invalid package distribution name {0}")]
    InvalidRequirementName(String),
    #[error("package extra is empty or contains invalid characters")]
    InvalidExtra,
    #[error("direct-source inner name does not match package {0}")]
    DirectNameMismatch(String),
    #[error("direct source for package {0} is invalid")]
    InvalidDirectSource(String),
    #[error("package {package} has contradictory clauses within {layer:?}")]
    ContradictorySource {
        package: String,
        layer: PackageSourceKind,
    },
    #[error("internal normalized PEP 440 clause is invalid: {0}")]
    InternalClause(String),
    #[error("package constraint resolution failed for {0}")]
    InternalResolution(String),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        ApplicationRequirements, ConstraintResolver, PackageSource, PackageSourceKind,
        PythonWorker, RequirementEntry, VersionSelector, normalize_package_name,
    };

    fn source(kind: PackageSourceKind, entries: Vec<RequirementEntry>) -> PackageSource {
        PackageSource {
            kind,
            entries,
            relative_base: PathBuf::from("/config"),
        }
    }

    fn requirement(name: &str, version: &str) -> RequirementEntry {
        RequirementEntry {
            name: name.to_owned(),
            version: Some(version.to_owned()),
            extras: Vec::new(),
        }
    }

    #[test]
    fn canonical_pyproject_groups_match_the_specified_worker_sets() {
        let manifest = ApplicationRequirements::embedded().expect("manifest must parse");
        assert!(manifest.common.is_empty());
        assert!(manifest.worker_dynamic.is_empty());
        assert_eq!(manifest.worker_script.len(), 13);
        for required in [
            "numpy",
            "opencv-python",
            "pandas",
            "pillow",
            "pygubu",
            "pynput",
            "pyserial",
            "pythonnet",
            "requests",
            "scipy",
            "pyaudio",
            "icecream",
            "loguru",
        ] {
            assert!(
                manifest
                    .worker_script
                    .iter()
                    .any(|entry| entry.starts_with(required)),
                "missing {required}"
            );
        }
        for excluded in ["setuptools", "gitpython", "paho-mqtt"] {
            assert!(
                !manifest
                    .worker_script
                    .iter()
                    .any(|entry| entry.starts_with(excluded))
            );
        }
    }

    #[test]
    fn pep503_names_and_pep685_extras_are_normalized_and_union_merged() {
        assert_eq!(
            normalize_package_name("My_Pkg.Name").expect("name must parse"),
            "my-pkg-name"
        );
        for invalid in ["-demo", "demo-", ".demo", "demo_"] {
            assert!(normalize_package_name(invalid).is_err());
        }
        let resolution = ConstraintResolver::resolve(
            PythonWorker::Dynamic,
            vec![
                source(
                    PackageSourceKind::GlobalToml,
                    vec![RequirementEntry {
                        name: "My_Pkg".to_owned(),
                        version: Some(">=1".to_owned()),
                        extras: vec!["Socks".to_owned()],
                    }],
                ),
                source(
                    PackageSourceKind::CommandLine,
                    vec![RequirementEntry {
                        name: "my-pkg".to_owned(),
                        version: Some("<3".to_owned()),
                        extras: vec!["security".to_owned(), "socks".to_owned()],
                    }],
                ),
            ],
            true,
            false,
        )
        .expect("resolution must succeed");
        let package = &resolution.packages["my-pkg"];
        assert_eq!(package.extras, ["security", "socks"]);
        assert_eq!(package.requirement(), "my-pkg[security,socks]>=1,<3");
    }

    #[test]
    fn mus_removes_only_conflicting_low_clause_and_retains_provenance() {
        let resolution = ConstraintResolver::resolve(
            PythonWorker::Dynamic,
            vec![
                source(
                    PackageSourceKind::GlobalToml,
                    vec![requirement("demo", ">=2,<5,!=3")],
                ),
                source(
                    PackageSourceKind::CommandLine,
                    vec![requirement("demo", "<2")],
                ),
            ],
            true,
            false,
        )
        .expect("resolution must succeed");
        let package = &resolution.packages["demo"];
        let VersionSelector::Analyzed(clauses) = &package.selector else {
            panic!("expected analyzed selector");
        };
        assert_eq!(
            clauses
                .iter()
                .map(|clause| clause.normalized.as_str())
                .collect::<Vec<_>>(),
            ["<5", "!=3", "<2"]
        );
        assert_eq!(clauses[0].provenance.source, PackageSourceKind::GlobalToml);
        assert_eq!(clauses[2].provenance.source, PackageSourceKind::CommandLine);
    }

    #[test]
    fn multi_clause_mus_removes_the_complete_low_conflict_set() {
        let resolution = ConstraintResolver::resolve(
            PythonWorker::Dynamic,
            vec![
                source(
                    PackageSourceKind::GlobalToml,
                    vec![requirement("demo", ">=1,<2")],
                ),
                source(
                    PackageSourceKind::CommandLine,
                    vec![requirement("demo", "!=1.*")],
                ),
            ],
            true,
            false,
        )
        .expect("resolution must succeed");
        assert_eq!(resolution.packages["demo"].requirement(), "demo!=1.*");
    }

    #[test]
    fn direct_sources_follow_order_and_application_override_direction() {
        let user = source(
            PackageSourceKind::CommandLine,
            vec![requirement(
                "numpy",
                "numpy[fast] @ https://example.invalid/numpy.whl",
            )],
        );
        let protected =
            ConstraintResolver::resolve(PythonWorker::Script, vec![user.clone()], false, false)
                .expect("protected application resolution must succeed");
        assert!(matches!(
            protected.packages["numpy"].selector,
            VersionSelector::Analyzed(_)
        ));
        assert_eq!(protected.packages["numpy"].extras, ["fast"]);

        let overridden = ConstraintResolver::resolve(PythonWorker::Script, vec![user], true, false)
            .expect("user override resolution must succeed");
        assert!(matches!(
            overridden.packages["numpy"].selector,
            VersionSelector::Direct(_)
        ));
    }

    #[test]
    fn tokenizer_separates_safe_clauses_markers_and_a_trailing_direct_source() {
        let marker = ConstraintResolver::resolve(
            PythonWorker::Dynamic,
            vec![source(
                PackageSourceKind::GlobalToml,
                vec![requirement("demo", ">=2,<5; python_version >= \"3.10\"")],
            )],
            true,
            false,
        )
        .expect("marker-bearing partial PEP 440 input must resolve");
        assert_eq!(
            marker.packages["demo"].requirement(),
            "demo>=2,<5; python_version >= \"3.10\""
        );

        let direct = ConstraintResolver::resolve(
            PythonWorker::Dynamic,
            vec![source(
                PackageSourceKind::GlobalToml,
                vec![requirement(
                    "demo",
                    ">=2,<5 @ https://example.invalid/demo.tar.gz",
                )],
            )],
            true,
            false,
        )
        .expect("safe clauses and a trailing direct source must tokenize");
        let VersionSelector::Direct(selector) = &direct.packages["demo"].selector else {
            panic!("the later direct selector must win within the entry");
        };
        assert_eq!(selector.provenance.clause_index, 2);
        assert_eq!(
            direct.packages["demo"].requirement(),
            "demo @ https://example.invalid/demo.tar.gz"
        );
    }

    #[test]
    fn package_resolution_debug_never_exposes_direct_or_opaque_text() {
        let secret = "https://user:password@example.invalid/demo.whl";
        let resolution = ConstraintResolver::resolve(
            PythonWorker::Dynamic,
            vec![source(
                PackageSourceKind::GlobalToml,
                vec![requirement("demo", secret)],
            )],
            true,
            false,
        )
        .expect("direct requirement must resolve");
        let debug = format!("{resolution:?}");
        assert!(!debug.contains(secret));
        assert!(!debug.contains("password"));
    }

    #[test]
    fn contradictory_static_source_is_rejected_instead_of_repaired() {
        let result = ConstraintResolver::resolve(
            PythonWorker::Dynamic,
            vec![source(
                PackageSourceKind::GlobalToml,
                vec![requirement("demo", ">=2"), requirement("demo", "<2")],
            )],
            true,
            false,
        );
        assert!(result.is_err());
    }
}
