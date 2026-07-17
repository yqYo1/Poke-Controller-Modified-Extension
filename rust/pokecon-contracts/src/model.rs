use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsRegistry {
    pub schema_version: u32,
    pub specification_version: String,
    pub expected_setting_count: usize,
    pub settings: Vec<Setting>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub id: String,
    pub value: ValueSchema,
    pub default: DefaultValue,
    pub scope: Scope,
    pub mutability: Mutability,
    #[serde(default)]
    pub apply_trigger: Option<String>,
    pub surfaces: Surfaces,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub path: Option<PathMetadata>,
    pub spec_refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValueSchema {
    Null,
    Boolean,
    String {
        #[serde(default)]
        min_length: Option<usize>,
        #[serde(default)]
        format: Option<String>,
    },
    Integer {
        #[serde(default)]
        minimum: Option<i64>,
        #[serde(default)]
        maximum: Option<i64>,
    },
    Number {
        #[serde(default)]
        minimum: Option<f64>,
        #[serde(default)]
        maximum: Option<f64>,
    },
    Enum {
        values: Vec<String>,
        #[serde(default)]
        aliases: BTreeMap<String, String>,
        #[serde(default)]
        ascii_case_insensitive: bool,
    },
    Union {
        variants: Vec<ValueSchema>,
    },
    Array {
        items: Box<ValueSchema>,
        #[serde(default)]
        min_items: Option<usize>,
        #[serde(default)]
        unique_items: bool,
    },
    Object {
        properties: BTreeMap<String, ObjectProperty>,
        additional_properties: bool,
        #[serde(default)]
        constraints: Vec<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectProperty {
    pub schema: Box<ValueSchema>,
    pub required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DefaultValue {
    Literal { value: Value },
    DataPath { relative: String },
    ResourcePath { relative: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    Profile,
    Bootstrap,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Mutability {
    StartupOnly,
    RuntimeImmediate,
    RuntimeDeferred,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surfaces {
    pub toml: OptionalNamedSurface,
    pub dynamic: OptionalNamedSurface,
    pub cli: CliSurface,
    pub env: EnvSurface,
    pub ui: UiSurface,
    pub openapi: OpenApiSurface,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalNamedSurface {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub unsupported_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WireEncoding {
    Scalar,
    StrictJson,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliSurface {
    pub flags: Vec<String>,
    pub encoding: WireEncoding,
    #[serde(default)]
    pub discouraged: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvSurface {
    pub name: String,
    pub encoding: WireEncoding,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    None,
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSurface {
    pub access: Access,
    #[serde(default)]
    pub control: Option<String>,
    #[serde(default)]
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenApiSurface {
    pub access: Access,
    #[serde(default)]
    pub masked_read: bool,
    #[serde(default)]
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathMetadata {
    pub policy: PathKind,
    pub must_exist: bool,
    pub auto_create: bool,
    pub expected_type: PathKind,
    pub resolve_symlink: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    File,
    Directory,
}
