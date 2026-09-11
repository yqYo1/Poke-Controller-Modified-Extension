use std::collections::BTreeSet;

use serde_json::Value;
use thiserror::Error;

use super::model::{
    Access, DefaultValue, Mutability, OptionalNamedSurface, Setting, SettingsRegistry, ValueSchema,
    WireEncoding,
};

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("registry JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("registry {0} must have a JSON object root")]
    NonObjectRegistry(&'static str),
    #[error("settings registry invariant failed: {0}")]
    Invariant(String),
}

#[derive(Debug)]
pub struct ValidatedSettingsRegistry(SettingsRegistry);

impl ValidatedSettingsRegistry {
    #[must_use]
    pub const fn registry(&self) -> &SettingsRegistry {
        &self.0
    }

    #[must_use]
    pub fn settings(&self) -> &[Setting] {
        &self.0.settings
    }
}

impl TryFrom<SettingsRegistry> for ValidatedSettingsRegistry {
    type Error = ContractError;

    fn try_from(registry: SettingsRegistry) -> Result<Self, Self::Error> {
        if registry.schema_version != 1 {
            return Err(invariant(format!(
                "unsupported schema_version {}",
                registry.schema_version
            )));
        }
        if registry.specification_version.is_empty() {
            return Err(invariant("specification_version must not be empty"));
        }
        if registry.settings.len() != registry.expected_setting_count {
            return Err(invariant(format!(
                "expected {} settings, found {}",
                registry.expected_setting_count,
                registry.settings.len()
            )));
        }

        let mut ids = BTreeSet::new();
        let mut cli_flags = BTreeSet::new();
        let mut env_names = BTreeSet::new();
        let mut toml_names = BTreeSet::new();
        let mut dynamic_names = BTreeSet::new();

        for setting in &registry.settings {
            validate_setting(setting)?;
            insert_unique(&mut ids, &setting.id, "setting id")?;
            for flag in &setting.surfaces.cli.flags {
                insert_unique(&mut cli_flags, flag, "CLI flag")?;
            }
            insert_unique(
                &mut env_names,
                &setting.surfaces.env.name,
                "environment name",
            )?;
            if let Some(name) = &setting.surfaces.toml.name {
                insert_unique(&mut toml_names, name, "TOML path")?;
            }
            if let Some(name) = &setting.surfaces.dynamic.name {
                insert_unique(&mut dynamic_names, name, "dynamic path")?;
            }
        }

        Ok(Self(registry))
    }
}

fn validate_setting(setting: &Setting) -> Result<(), ContractError> {
    if !valid_canonical_id(&setting.id) {
        return Err(invariant(format!(
            "{} is not a lowercase dotted snake_case canonical id",
            setting.id
        )));
    }
    if setting.spec_refs.is_empty() || setting.spec_refs.iter().any(String::is_empty) {
        return Err(invariant(format!(
            "{} must have non-empty specification references",
            setting.id
        )));
    }
    if setting.mutability == Mutability::RuntimeDeferred
        && setting.apply_trigger.as_deref().is_none_or(str::is_empty)
    {
        return Err(invariant(format!(
            "{} is runtime_deferred but has no apply_trigger",
            setting.id
        )));
    }
    if setting.mutability != Mutability::RuntimeDeferred && setting.apply_trigger.is_some() {
        return Err(invariant(format!(
            "{} has apply_trigger but is not runtime_deferred",
            setting.id
        )));
    }

    validate_optional_surface(&setting.id, "TOML", &setting.surfaces.toml)?;
    validate_optional_surface(&setting.id, "dynamic", &setting.surfaces.dynamic)?;

    if setting.surfaces.cli.flags.is_empty() {
        return Err(invariant(format!("{} has no CLI flag", setting.id)));
    }
    for flag in &setting.surfaces.cli.flags {
        if !(flag.starts_with("--") || (flag.starts_with('-') && flag.len() == 2)) {
            return Err(invariant(format!(
                "{} has invalid CLI flag {flag}",
                setting.id
            )));
        }
    }
    if !setting.surfaces.env.name.starts_with("POKECON_") {
        return Err(invariant(format!(
            "{} has invalid environment name {}",
            setting.id, setting.surfaces.env.name
        )));
    }

    let complex = matches!(
        setting.value,
        ValueSchema::Array { .. } | ValueSchema::Object { .. }
    );
    let expected_encoding = if complex {
        WireEncoding::StrictJson
    } else {
        WireEncoding::Scalar
    };
    if setting.surfaces.cli.encoding != expected_encoding
        || setting.surfaces.env.encoding != expected_encoding
    {
        return Err(invariant(format!(
            "{} has an encoding inconsistent with its value schema",
            setting.id
        )));
    }

    if setting.secret && setting.surfaces.openapi.access != Access::None {
        if !setting.surfaces.openapi.masked_read {
            return Err(invariant(format!(
                "{} is secret but OpenAPI reads are not masked",
                setting.id
            )));
        }
    } else if setting.surfaces.openapi.masked_read {
        return Err(invariant(format!(
            "{} is not secret but declares masked OpenAPI reads",
            setting.id
        )));
    }

    if let DefaultValue::Literal { value } = &setting.default {
        setting
            .value
            .validate_value(value)
            .map_err(|message| invariant(format!("{} default: {message}", setting.id)))?;
    }

    Ok(())
}

fn validate_optional_surface(
    id: &str,
    label: &str,
    surface: &OptionalNamedSurface,
) -> Result<(), ContractError> {
    match (&surface.name, &surface.unsupported_reason) {
        (Some(name), None) if !name.is_empty() => Ok(()),
        (None, Some(reason)) if !reason.is_empty() => Ok(()),
        _ => Err(invariant(format!(
            "{id} {label} surface must have exactly one non-empty name or unsupported_reason"
        ))),
    }
}

impl ValueSchema {
    /// Validates a JSON value against this canonical schema.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe description of the schema violation.
    pub fn validate(&self, value: &Value) -> Result<(), String> {
        self.validate_value(value)
    }

    fn validate_value(&self, value: &Value) -> Result<(), String> {
        match self {
            Self::Null => value.is_null().then_some(()).ok_or("expected null".into()),
            Self::Boolean => value
                .is_boolean()
                .then_some(())
                .ok_or("expected boolean".into()),
            Self::String { min_length, format } => {
                let string = value.as_str().ok_or("expected string")?;
                if min_length.is_some_and(|minimum| string.chars().count() < minimum) {
                    return Err(format!("string is shorter than {min_length:?}"));
                }
                validate_string_format(string, format.as_deref())
            }
            Self::Integer { minimum, maximum } => {
                let integer = value.as_i64().ok_or("expected signed 64-bit integer")?;
                if minimum.is_some_and(|bound| integer < bound) {
                    return Err(format!("integer is below minimum {minimum:?}"));
                }
                if maximum.is_some_and(|bound| integer > bound) {
                    return Err(format!("integer is above maximum {maximum:?}"));
                }
                Ok(())
            }
            Self::Number { minimum, maximum } => {
                if value.is_boolean() {
                    return Err("expected finite number, not boolean".into());
                }
                let number = value.as_f64().ok_or("expected finite number")?;
                if !number.is_finite() {
                    return Err("expected finite number".into());
                }
                if minimum.is_some_and(|bound| number < bound) {
                    return Err(format!("number is below minimum {minimum:?}"));
                }
                if maximum.is_some_and(|bound| number > bound) {
                    return Err(format!("number is above maximum {maximum:?}"));
                }
                Ok(())
            }
            Self::Enum { values, .. } => {
                let string = value.as_str().ok_or("expected enum string")?;
                values
                    .iter()
                    .any(|candidate| candidate == string)
                    .then_some(())
                    .ok_or_else(|| format!("{string:?} is not a canonical enum value"))
            }
            Self::Union { variants } => variants
                .iter()
                .any(|variant| variant.validate_value(value).is_ok())
                .then_some(())
                .ok_or("value matches no union variant".into()),
            Self::Array {
                items,
                min_items,
                unique_items,
            } => {
                let array = value.as_array().ok_or("expected array")?;
                if min_items.is_some_and(|minimum| array.len() < minimum) {
                    return Err(format!("array is shorter than {min_items:?}"));
                }
                for item in array {
                    items.validate_value(item)?;
                }
                if *unique_items {
                    let serialized = array.iter().map(Value::to_string).collect::<BTreeSet<_>>();
                    if serialized.len() != array.len() {
                        return Err("array items must be unique".into());
                    }
                }
                Ok(())
            }
            Self::Object {
                properties,
                additional_properties,
                constraints,
            } => {
                let object = value.as_object().ok_or("expected object")?;
                for (name, property) in properties {
                    match object.get(name) {
                        Some(property_value) => property.schema.validate_value(property_value)?,
                        None if property.required => {
                            return Err(format!("required property {name:?} is missing"));
                        }
                        None => {}
                    }
                }
                if !additional_properties {
                    for name in object.keys() {
                        if !properties.contains_key(name) {
                            return Err(format!("unknown property {name:?}"));
                        }
                    }
                }
                validate_object_constraints(object, constraints)
            }
        }
    }
}

fn validate_string_format(value: &str, format: Option<&str>) -> Result<(), String> {
    match format {
        None | Some("free") => Ok(()),
        Some("non_empty_no_nul") => (!value.is_empty() && !value.contains('\0'))
            .then_some(())
            .ok_or("value must be non-empty and contain no NUL".into()),
        Some("profile_name" | "app_name") => (!value.is_empty()
            && value != "."
            && value != ".."
            && !value.contains(['/', '\\', '\0', ':']))
        .then_some(())
        .ok_or("value is not a safe single path component".into()),
        Some("http_url_or_empty") => (value.is_empty() || is_http_url(value))
            .then_some(())
            .ok_or("value must be empty or an HTTP(S) URL".into()),
        Some("discord_webhook_or_empty") => (value.is_empty() || is_discord_webhook(value))
            .then_some(())
            .ok_or("value must be empty or a Discord webhook URL".into()),
        Some("stun_uri_or_empty") => (value.is_empty() || is_stun_uri(value))
            .then_some(())
            .ok_or("value must be empty or a STUN URI".into()),
        Some("ip_literal_non_wildcard") => {
            let address = value
                .parse::<std::net::IpAddr>()
                .map_err(|_| "value must be a numeric IPv4 or IPv6 literal")?;
            (!address.is_unspecified() && !address.is_multicast())
                .then_some(())
                .ok_or("wildcard and multicast IP literals are forbidden".into())
        }
        Some(unknown) => Err(format!("unknown string format {unknown:?}")),
    }
}

fn is_http_url(value: &str) -> bool {
    (value.starts_with("http://") || value.starts_with("https://"))
        && url::Url::parse(value).is_ok_and(|parsed| {
            matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
        })
}

fn is_discord_webhook(value: &str) -> bool {
    let Ok(parsed) = url::Url::parse(value) else {
        return false;
    };
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("discord.com")
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return false;
    }

    let Some(mut segments) = parsed.path_segments() else {
        return false;
    };
    let (Some("api"), Some("webhooks"), Some(id), Some(token), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return false;
    };

    !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && !token.is_empty()
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn is_stun_uri(value: &str) -> bool {
    let Ok(parsed) = url::Url::parse(value) else {
        return false;
    };
    if !matches!(parsed.scheme(), "stun" | "stuns")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return false;
    }

    let target = parsed.path();
    if target.is_empty()
        || target.starts_with("//")
        || target.contains(['/', '@'])
        || target.chars().any(char::is_whitespace)
    {
        return false;
    }

    if let Some(rest) = target.strip_prefix('[') {
        let Some((address, suffix)) = rest.split_once(']') else {
            return false;
        };
        if address.parse::<std::net::Ipv6Addr>().is_err()
            || (!suffix.is_empty() && !valid_stun_port(suffix.strip_prefix(':')))
        {
            return false;
        }
    } else if let Some((host, port)) = target.split_once(':')
        && (host.is_empty() || host.contains(':') || !valid_stun_port(Some(port)))
    {
        return false;
    }

    let Ok(authority) = url::Url::parse(&format!("http://{target}")) else {
        return false;
    };
    authority.host_str().is_some()
        && authority.username().is_empty()
        && authority.password().is_none()
        && authority.path() == "/"
        && authority.query().is_none()
        && authority.fragment().is_none()
        && authority.port().is_none_or(|port| port != 0)
}

fn valid_stun_port(port: Option<&str>) -> bool {
    port.is_some_and(|port| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port != 0)
    })
}

fn validate_object_constraints(
    object: &serde_json::Map<String, Value>,
    constraints: &[String],
) -> Result<(), String> {
    for constraint in constraints {
        match constraint.as_str() {
            "left_lt_right" => compare_f64(object, "left", "right")?,
            "top_lt_bottom" => compare_f64(object, "top", "bottom")?,
            unknown => return Err(format!("unknown object constraint {unknown:?}")),
        }
    }
    Ok(())
}

fn compare_f64(
    object: &serde_json::Map<String, Value>,
    left: &str,
    right: &str,
) -> Result<(), String> {
    let left_value = object
        .get(left)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{left:?} must be numeric"))?;
    let right_value = object
        .get(right)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{right:?} must be numeric"))?;
    (left_value < right_value)
        .then_some(())
        .ok_or_else(|| format!("{left} must be less than {right}"))
}

fn valid_canonical_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('.').all(|segment| {
            !segment.is_empty()
                && segment.chars().all(|character| {
                    character.is_ascii_lowercase() || character == '_' || character.is_ascii_digit()
                })
                && segment
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_lowercase())
        })
}

fn insert_unique<'a>(
    set: &mut BTreeSet<&'a str>,
    value: &'a str,
    label: &str,
) -> Result<(), ContractError> {
    if set.insert(value) {
        Ok(())
    } else {
        Err(invariant(format!("duplicate {label}: {value}")))
    }
}

fn invariant(message: impl Into<String>) -> ContractError {
    ContractError::Invariant(message.into())
}

#[cfg(test)]
mod string_format_tests {
    use super::validate_string_format;

    #[test]
    fn validates_http_urls_structurally() {
        for valid in ["", "http://example.com", "https://example.com/avatar.png"] {
            assert!(validate_string_format(valid, Some("http_url_or_empty")).is_ok());
        }
        for invalid in [
            "ftp://example.com/avatar.png",
            "https://",
            "https:not-an-authority",
        ] {
            assert!(validate_string_format(invalid, Some("http_url_or_empty")).is_err());
        }
    }

    #[test]
    fn accepts_only_canonical_discord_webhook_urls() {
        for valid in [
            "",
            "https://discord.com/api/webhooks/1234567890/token_A-b.c",
        ] {
            assert!(validate_string_format(valid, Some("discord_webhook_or_empty")).is_ok());
        }
        for invalid in [
            "http://discord.com/api/webhooks/123/token",
            "https://discordapp.com/api/webhooks/123/token",
            "https://discord.com/api/webhooks/not-a-number/token",
            "https://discord.com/api/webhooks/123/token/extra",
            "https://discord.com/api/webhooks/123/token?wait=true",
        ] {
            assert!(validate_string_format(invalid, Some("discord_webhook_or_empty")).is_err());
        }
    }

    #[test]
    fn validates_stun_uri_authorities() {
        for valid in [
            "",
            "stun:stun.example.com",
            "stun:stun.example.com:3478",
            "stuns:[2001:db8::1]:5349",
        ] {
            assert!(validate_string_format(valid, Some("stun_uri_or_empty")).is_ok());
        }
        for invalid in [
            "http:stun.example.com",
            "stun:",
            "stun://stun.example.com",
            "stun:user@stun.example.com",
            "stun:stun.example.com/path",
            "stun:stun.example.com:",
            "stun:stun.example.com:0",
            "stun:stun.example.com:65536",
        ] {
            assert!(validate_string_format(invalid, Some("stun_uri_or_empty")).is_err());
        }
    }
}
