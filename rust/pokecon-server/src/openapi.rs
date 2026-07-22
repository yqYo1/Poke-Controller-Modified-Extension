//! Deterministic `OpenAPI` document generation.

use pokecon_contracts::model::{Access, ObjectProperty, Setting, ValueSchema};
use serde_json::{Map, Value, json};
use thiserror::Error;
use utoipa::OpenApi;

use crate::api::{
    ApiError, ApiErrorCode, ButtonState, CameraDevice, CameraSelector, ClientMessage,
    CommandControlRequest, CommandDisplayItem, CommandIdentity, CommandInfo, CommandState,
    DecimalString, DynamicConfigControlRequest, DynamicConfigResult, DynamicLanguage, EmptyRequest,
    ErrorEnvelope, GamepadInput, GenerateLauncherRequest, GenerateLauncherResult, Hat,
    IceCandidate, ImageFormat, InputApplied, InputGeneration, InputSnapshot, KeyboardInput,
    LauncherDestination, LogData, LogLevel, LogTarget, MessageData, MouseButton, MouseButtons,
    MouseInput, MouseStickInput, Nonce, NormalizedRegion, NotificationTestRequest,
    NotificationTestResult, OperationResult, PressState, RevisionedStateChange, SavedScreenshot,
    ScreenshotRequest, SerialControlRequest, SerialData, SerialEncoding, SerialPort, ServerMessage,
    SessionDescription, SettingsChange, SettingsPatchRequest, SettingsReadValues, SettingsSnapshot,
    SettingsWriteValues, StateChangeCause, StatePatch, StateSnapshot, StickName, StickPosition,
    Success, TouchPoint, UiStateChange, UpdateCheckResult,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "PokeCon API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Closed REST and WebSocket contracts for PokeCon"
    ),
    components(schemas(
        ApiError,
        ApiErrorCode,
        ButtonState,
        CameraDevice,
        CameraSelector,
        ClientMessage,
        CommandControlRequest,
        CommandDisplayItem,
        CommandIdentity,
        CommandInfo,
        CommandState,
        DecimalString,
        DynamicConfigControlRequest,
        DynamicConfigResult,
        DynamicLanguage,
        EmptyRequest,
        ErrorEnvelope,
        GamepadInput,
        GenerateLauncherRequest,
        GenerateLauncherResult,
        Hat,
        IceCandidate,
        ImageFormat,
        InputApplied,
        InputGeneration,
        InputSnapshot,
        KeyboardInput,
        LauncherDestination,
        LogData,
        LogLevel,
        LogTarget,
        MessageData<Nonce>,
        MouseButton,
        MouseButtons,
        MouseInput,
        MouseStickInput,
        Nonce,
        NormalizedRegion,
        NotificationTestRequest,
        NotificationTestResult,
        OperationResult,
        PressState,
        RevisionedStateChange,
        SavedScreenshot,
        ScreenshotRequest,
        SerialControlRequest,
        SerialData,
        SerialEncoding,
        SerialPort,
        ServerMessage,
        SessionDescription,
        SettingsChange,
        SettingsPatchRequest,
        SettingsReadValues,
        SettingsSnapshot,
        SettingsWriteValues,
        StateChangeCause,
        StatePatch,
        StateSnapshot,
        StickName,
        StickPosition,
        Success<SettingsSnapshot>,
        Success<StateSnapshot>,
        TouchPoint,
        UiStateChange,
        UpdateCheckResult
    )),
    paths(
        crate::paths::get_settings,
        crate::paths::patch_settings,
        crate::paths::get_state,
        crate::paths::control_command,
        crate::paths::reload_commands,
        crate::paths::get_cameras,
        crate::paths::get_serial_ports,
        crate::paths::control_serial,
        crate::paths::retry_camera,
        crate::paths::screenshot,
        crate::paths::test_notification,
        crate::paths::control_dynamic_config,
        crate::paths::generate_launcher,
        crate::paths::check_update,
        crate::paths::websocket
    ),
    tags(
        (name = "settings", description = "Canonical settings"),
        (name = "state", description = "Runtime state"),
        (name = "commands", description = "User command lifecycle"),
        (name = "devices", description = "Camera and serial devices")
    )
)]
struct ApiDocument;

/// Builds the `OpenAPI` JSON value and projects the canonical setting registry
/// into literal closed properties.
///
/// # Errors
///
/// Returns an error if either utoipa or the canonical registry cannot be
/// represented as the expected `OpenAPI` object.
pub fn document() -> Result<Value, OpenApiError> {
    let mut document = serde_json::to_value(ApiDocument::openapi())?;
    let registry = pokecon_contracts::settings_registry()?;
    let schemas = document
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
        .ok_or(OpenApiError::MissingSchemas)?;
    schemas.insert(
        "SettingsReadValues".to_owned(),
        settings_object(registry.settings(), SettingProjection::Read),
    );
    schemas.insert(
        "SettingsWriteValues".to_owned(),
        settings_object(registry.settings(), SettingProjection::Write),
    );
    add_discriminators(schemas)?;
    close_named_and_inline_objects(&mut document);
    Ok(document)
}

fn add_discriminators(schemas: &mut Map<String, Value>) -> Result<(), OpenApiError> {
    for (name, property_name) in [
        ("ClientMessage", "type"),
        ("CommandControlRequest", "action"),
        ("CommandDisplayItem", "kind"),
        ("DynamicConfigControlRequest", "action"),
        ("GamepadInput", "kind"),
        ("LauncherDestination", "kind"),
        ("NotificationTestRequest", "channel"),
        ("ScreenshotRequest", "destination"),
        ("SerialControlRequest", "action"),
        ("ServerMessage", "type"),
    ] {
        let schema = schemas
            .get_mut(name)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| OpenApiError::MissingSchema(name.to_owned()))?;
        schema.insert(
            "discriminator".to_owned(),
            json!({"propertyName": property_name}),
        );
    }
    Ok(())
}

/// Stable pretty-printed JSON used by both git-tracked artifacts and tests.
///
/// # Errors
///
/// Returns an `OpenAPI` construction or JSON serialization error.
pub fn document_json() -> Result<String, OpenApiError> {
    let mut output = serde_json::to_string_pretty(&document()?)?;
    output.push('\n');
    Ok(output)
}

#[derive(Clone, Copy)]
enum SettingProjection {
    Read,
    Write,
}

fn settings_object(settings: &[Setting], projection: SettingProjection) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for setting in settings {
        let included = match projection {
            SettingProjection::Read => matches!(
                setting.surfaces.openapi.access,
                Access::Read | Access::ReadWrite
            ),
            SettingProjection::Write => matches!(
                setting.surfaces.openapi.access,
                Access::Write | Access::ReadWrite
            ),
        };
        if !included {
            continue;
        }
        let schema = if setting.secret && matches!(projection, SettingProjection::Read) {
            json!({
                "type": "string",
                "description": "Secret-safe masked value; configured secrets are returned as ********"
            })
        } else {
            value_schema(&setting.value)
        };
        properties.insert(setting.id.clone(), schema);
        if matches!(projection, SettingProjection::Read) {
            required.push(Value::String(setting.id.clone()));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn value_schema(schema: &ValueSchema) -> Value {
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
            "oneOf": variants.iter().map(value_schema).collect::<Vec<_>>()
        }),
        ValueSchema::Array {
            items,
            min_items,
            unique_items,
        } => with_optional(
            json!({
                "type": "array",
                "items": value_schema(items),
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
    properties: &std::collections::BTreeMap<String, ObjectProperty>,
    additional_properties: bool,
) -> Value {
    let schemas = properties
        .iter()
        .map(|(name, property)| (name.clone(), value_schema(&property.schema)))
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

fn close_named_and_inline_objects(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                close_named_and_inline_objects(value);
            }
        }
        Value::Object(object) => {
            let has_properties = object.get("properties").is_some_and(Value::is_object);
            if object.get("type").and_then(Value::as_str) == Some("object") && has_properties {
                object
                    .entry("additionalProperties")
                    .or_insert(Value::Bool(false));
            }
            for value in object.values_mut() {
                close_named_and_inline_objects(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[derive(Debug, Error)]
pub enum OpenApiError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Contract(#[from] pokecon_contracts::ContractError),
    #[error("utoipa document is missing components.schemas")]
    MissingSchemas,
    #[error("utoipa document is missing schema {0}")]
    MissingSchema(String),
}

#[cfg(test)]
mod tests {
    use pokecon_contracts::model::Access;

    use super::document;

    #[test]
    fn setting_schemas_are_literal_closed_and_access_aware() {
        let document = document().unwrap();
        let read = document
            .pointer("/components/schemas/SettingsReadValues")
            .unwrap();
        let write = document
            .pointer("/components/schemas/SettingsWriteValues")
            .unwrap();
        assert_eq!(read["additionalProperties"], false);
        assert_eq!(write["additionalProperties"], false);

        let registry = pokecon_contracts::settings_registry().unwrap();
        for setting in registry.settings() {
            let readable = matches!(
                setting.surfaces.openapi.access,
                Access::Read | Access::ReadWrite
            );
            let writable = matches!(
                setting.surfaces.openapi.access,
                Access::Write | Access::ReadWrite
            );
            assert_eq!(
                read["properties"].get(&setting.id).is_some(),
                readable,
                "{} read projection",
                setting.id
            );
            assert_eq!(
                write["properties"].get(&setting.id).is_some(),
                writable,
                "{} write projection",
                setting.id
            );
        }
    }

    #[test]
    fn every_schema_object_with_properties_is_closed() {
        fn visit(value: &serde_json::Value) {
            match value {
                serde_json::Value::Array(values) => values.iter().for_each(visit),
                serde_json::Value::Object(object) => {
                    if object.get("type").and_then(serde_json::Value::as_str) == Some("object")
                        && object
                            .get("properties")
                            .is_some_and(serde_json::Value::is_object)
                    {
                        assert_eq!(object.get("additionalProperties"), Some(&false.into()));
                    }
                    object.values().for_each(visit);
                }
                _ => {}
            }
        }

        visit(&document().unwrap());
    }

    #[test]
    fn tagged_unions_have_explicit_discriminators() {
        let document = document().unwrap();
        for (name, property) in [
            ("ClientMessage", "type"),
            ("CommandControlRequest", "action"),
            ("CommandDisplayItem", "kind"),
            ("DynamicConfigControlRequest", "action"),
            ("GamepadInput", "kind"),
            ("LauncherDestination", "kind"),
            ("NotificationTestRequest", "channel"),
            ("ScreenshotRequest", "destination"),
            ("SerialControlRequest", "action"),
            ("ServerMessage", "type"),
        ] {
            assert_eq!(
                document.pointer(&format!(
                    "/components/schemas/{name}/discriminator/propertyName"
                )),
                Some(&property.into())
            );
        }
    }
}
