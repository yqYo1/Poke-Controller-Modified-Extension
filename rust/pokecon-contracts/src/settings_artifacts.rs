use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::model::{DefaultValue, ObjectProperty, SettingsRegistry, ValueSchema};

/// Projects the complete canonical setting registry into a closed JSON Schema.
///
/// # Panics
///
/// Panics if the internal value-schema projection stops producing JSON objects.
#[must_use]
pub fn settings_json_schema(registry: &SettingsRegistry) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::with_capacity(registry.settings.len());
    for setting in &registry.settings {
        let mut schema = value_json_schema(&setting.value);
        let schema_object = schema
            .as_object_mut()
            .expect("every canonical value schema projects to an object");
        if let DefaultValue::Literal { value } = &setting.default {
            schema_object.insert("default".to_owned(), value.clone());
        }
        schema_object.insert(
            "x-pokecon".to_owned(),
            json!({
                "apply_trigger": setting.apply_trigger,
                "default": setting.default,
                "mutability": setting.mutability,
                "path": setting.path,
                "scope": setting.scope,
                "secret": setting.secret,
                "spec_refs": setting.spec_refs,
                "surfaces": setting.surfaces,
            }),
        );
        properties.insert(setting.id.clone(), schema);
        required.push(Value::String(setting.id.clone()));
    }

    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "PokeCon canonical settings values",
        "description": "Closed flat object keyed by canonical setting ID.",
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
        "x-pokecon-registry-schema-version": registry.schema_version,
        "x-pokecon-specification-version": registry.specification_version,
    })
}

/// Projects canonical setting metadata consumed by UI controls and audits.
#[must_use]
pub fn settings_ui_metadata(registry: &SettingsRegistry) -> Value {
    let settings = registry
        .settings
        .iter()
        .map(|setting| {
            json!({
                "access": setting.surfaces.ui.access,
                "control": setting.surfaces.ui.control,
                "default": setting.default,
                "id": setting.id,
                "mutability": setting.mutability,
                "scope": setting.scope,
                "secret": setting.secret,
                "spec_refs": setting.spec_refs,
                "unsupported_reason": setting.surfaces.ui.unsupported_reason,
                "value_schema": value_json_schema(&setting.value),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "schema_version": registry.schema_version,
        "specification_version": registry.specification_version,
        "settings": settings,
    })
}

/// Converts one canonical setting value schema to JSON Schema/OpenAPI syntax.
#[must_use]
pub fn value_json_schema(schema: &ValueSchema) -> Value {
    match schema {
        ValueSchema::Null => json!({"type": "null"}),
        ValueSchema::Boolean => json!({"type": "boolean"}),
        ValueSchema::String { min_length, format } => with_optional(
            json!({"type": "string"}),
            [
                ("minLength", min_length.map(|value| json!(value))),
                ("format", format.as_ref().map(|value| json!(value))),
            ],
        ),
        ValueSchema::Integer { minimum, maximum } => with_optional(
            json!({"type": "integer", "format": "int64"}),
            [
                ("minimum", minimum.map(|value| json!(value))),
                ("maximum", maximum.map(|value| json!(value))),
            ],
        ),
        ValueSchema::Number { minimum, maximum } => with_optional(
            json!({"type": "number", "format": "double"}),
            [
                ("minimum", minimum.map(|value| json!(value))),
                ("maximum", maximum.map(|value| json!(value))),
            ],
        ),
        ValueSchema::Enum { values, .. } => json!({"type": "string", "enum": values}),
        ValueSchema::Union { variants } => json!({
            "oneOf": variants.iter().map(value_json_schema).collect::<Vec<_>>()
        }),
        ValueSchema::Array {
            items,
            min_items,
            unique_items,
        } => with_optional(
            json!({
                "type": "array",
                "items": value_json_schema(items),
                "uniqueItems": unique_items
            }),
            [("minItems", min_items.map(|value| json!(value)))],
        ),
        ValueSchema::Object {
            properties,
            additional_properties,
            ..
        } => object_schema(properties, *additional_properties),
    }
}

fn object_schema(
    properties: &BTreeMap<String, ObjectProperty>,
    additional_properties: bool,
) -> Value {
    let schemas = properties
        .iter()
        .map(|(name, property)| (name.clone(), value_json_schema(&property.schema)))
        .collect::<Map<_, _>>();
    let required = properties
        .iter()
        .filter(|(_, property)| property.required)
        .map(|(name, _)| Value::String(name.clone()))
        .collect::<Vec<_>>();
    json!({
        "type": "object",
        "properties": schemas,
        "required": required,
        "additionalProperties": additional_properties
    })
}

fn with_optional<const N: usize>(mut schema: Value, fields: [(&str, Option<Value>); N]) -> Value {
    let object = schema
        .as_object_mut()
        .expect("schema helper always receives an object");
    for (name, value) in fields {
        if let Some(value) = value {
            object.insert(name.to_owned(), value);
        }
    }
    schema
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{settings_json_schema, settings_ui_metadata};

    #[test]
    fn generated_setting_documents_cover_the_validated_registry_exactly() {
        let validated = crate::settings_registry().expect("settings registry must be valid");
        let registry = validated.registry();
        let schema = settings_json_schema(registry);
        let metadata = settings_ui_metadata(registry);

        let properties = schema["properties"]
            .as_object()
            .expect("settings schema properties must be an object");
        let required = schema["required"]
            .as_array()
            .expect("settings schema required must be an array");
        assert_eq!(properties.len(), registry.expected_setting_count);
        assert_eq!(required.len(), registry.expected_setting_count);
        assert_eq!(schema["additionalProperties"], false);

        let metadata_rows = metadata["settings"]
            .as_array()
            .expect("UI settings metadata must be an array");
        let metadata_ids = metadata_rows
            .iter()
            .map(|row| row["id"].as_str().expect("metadata ID must be a string"))
            .collect::<BTreeSet<_>>();
        let registry_ids = registry
            .settings
            .iter()
            .map(|setting| setting.id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(metadata_ids, registry_ids);
    }
}
