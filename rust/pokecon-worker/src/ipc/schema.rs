use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Error as _, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Closed recursive value union accepted by the control-plane protocol.
///
/// `MessagePack` extension values and maps with non-string keys are deliberately
/// absent from this type.
#[derive(Clone, Debug, PartialEq)]
pub enum IpcValue {
    /// `MessagePack` nil.
    Nil,
    /// Boolean scalar.
    Bool(bool),
    /// Signed integer scalar.
    Integer(i64),
    /// Unsigned integer scalar.
    Unsigned(u64),
    /// Floating-point scalar.
    Float(f64),
    /// UTF-8 string scalar.
    String(String),
    /// Binary scalar.
    Binary(Vec<u8>),
    /// Ordered sequence of closed values.
    Array(Vec<Self>),
    /// String-keyed map of closed values.
    Map(BTreeMap<String, Self>),
}

impl Serialize for IpcValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Nil => serializer.serialize_none(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Integer(value) => serializer.serialize_i64(*value),
            Self::Unsigned(value) => serializer.serialize_u64(*value),
            Self::Float(value) => serializer.serialize_f64(*value),
            Self::String(value) => serializer.serialize_str(value),
            Self::Binary(value) => serializer.serialize_bytes(value),
            Self::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(value)?;
                }
                sequence.end()
            }
            Self::Map(values) => {
                let mut map = serializer.serialize_map(Some(values.len()))?;
                for (key, value) in values {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

struct IpcValueVisitor;

impl<'de> Visitor<'de> for IpcValueVisitor {
    type Value = IpcValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a closed IPC scalar, array, or string-keyed map")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(IpcValue::Nil)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(IpcValue::Nil)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(IpcValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(IpcValue::Integer(value))
    }

    fn visit_i8<E>(self, value: i8) -> Result<Self::Value, E> {
        Ok(IpcValue::Integer(i64::from(value)))
    }

    fn visit_i16<E>(self, value: i16) -> Result<Self::Value, E> {
        Ok(IpcValue::Integer(i64::from(value)))
    }

    fn visit_i32<E>(self, value: i32) -> Result<Self::Value, E> {
        Ok(IpcValue::Integer(i64::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(IpcValue::Unsigned(value))
    }

    fn visit_u8<E>(self, value: u8) -> Result<Self::Value, E> {
        Ok(IpcValue::Unsigned(u64::from(value)))
    }

    fn visit_u16<E>(self, value: u16) -> Result<Self::Value, E> {
        Ok(IpcValue::Unsigned(u64::from(value)))
    }

    fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E> {
        Ok(IpcValue::Unsigned(u64::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(IpcValue::Float(value))
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E> {
        Ok(IpcValue::Float(f64::from(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(IpcValue::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(IpcValue::String(value))
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E> {
        Ok(IpcValue::Binary(value.to_vec()))
    }

    fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E> {
        Ok(IpcValue::Binary(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        // Do not trust an attacker-controlled collection length for allocation.
        // The outer frame is bounded, and the vector grows only for values that
        // are actually present in that bounded body.
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(IpcValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, IpcValue>()? {
            if values.insert(key.clone(), value).is_some() {
                return Err(A::Error::custom(format_args!(
                    "duplicate IPC map key `{key}`"
                )));
            }
        }
        Ok(IpcValue::Map(values))
    }
}

impl<'de> Deserialize<'de> for IpcValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(IpcValueVisitor)
    }
}

impl From<()> for IpcValue {
    fn from((): ()) -> Self {
        Self::Nil
    }
}

impl From<bool> for IpcValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for IpcValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<u64> for IpcValue {
    fn from(value: u64) -> Self {
        Self::Unsigned(value)
    }
}

impl From<String> for IpcValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for IpcValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

/// Stable error response body. Unknown fields are rejected during decoding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IpcErrorPayload {
    /// Stable, non-empty ASCII identifier used for exception mapping.
    pub code: String,
    /// Human-readable, secret-free diagnostic.
    pub message: String,
}

impl IpcErrorPayload {
    /// Creates and validates an IPC error payload.
    ///
    /// # Errors
    ///
    /// Returns an error when `code` is not a non-empty ASCII identifier.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Result<Self, SchemaError> {
        let payload = Self {
            code: code.into(),
            message: message.into(),
        };
        payload.validate()?;
        Ok(payload)
    }

    fn validate(&self) -> Result<(), SchemaError> {
        let mut bytes = self.code.bytes();
        let Some(first) = bytes.next() else {
            return Err(SchemaError::InvalidErrorCode(self.code.clone()));
        };
        if !(first.is_ascii_alphabetic() || first == b'_')
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(SchemaError::InvalidErrorCode(self.code.clone()));
        }
        Ok(())
    }
}

/// Closed log severity set used by worker stdout interception.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Debug diagnostic.
    Debug,
    /// Informational diagnostic.
    Info,
    /// Recoverable warning.
    Warning,
    /// Operation error.
    Error,
    /// Critical diagnostic; it does not independently terminate the app.
    Critical,
}

/// Closed destination set for intercepted worker output.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LogTarget {
    /// No explicit destination.
    #[serde(rename = "")]
    Empty,
    /// Standard output panel.
    #[serde(rename = "stdout")]
    Stdout,
    /// First UI log panel.
    #[serde(rename = "panel1")]
    Panel1,
    /// Second UI log panel.
    #[serde(rename = "panel2")]
    Panel2,
    /// Persistent application log.
    #[serde(rename = "log")]
    Log,
}

/// Closed structured log payload. Unknown fields are rejected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogPayload {
    /// Log severity.
    pub level: LogLevel,
    /// Message text.
    pub message: String,
    /// Destination selected by the compatibility API.
    pub target: LogTarget,
}

/// Fully typed control-protocol envelope.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Envelope {
    /// Correlated operation request.
    Request {
        /// Correlation identifier.
        id: u64,
        /// Operation name.
        op: String,
        /// Closed operation payload.
        payload: IpcValue,
    },
    /// Correlated successful response.
    Response {
        /// Correlation identifier.
        id: u64,
        /// Optional operation name.
        #[serde(skip_serializing_if = "Option::is_none")]
        op: Option<String>,
        /// Closed success payload.
        payload: IpcValue,
    },
    /// Correlated error response.
    Error {
        /// Correlation identifier.
        id: u64,
        /// Operation that failed.
        op: String,
        /// Stable closed error payload.
        payload: IpcErrorPayload,
    },
    /// Uncorrelated event notification.
    Event {
        /// Event name.
        op: String,
        /// Closed event payload.
        payload: IpcValue,
    },
    /// Structured intercepted output or log message.
    Log {
        /// Closed log payload.
        payload: LogPayload,
    },
}

impl Envelope {
    /// Returns the correlation ID when this envelope has one.
    #[must_use]
    pub const fn id(&self) -> Option<u64> {
        match self {
            Self::Request { id, .. } | Self::Response { id, .. } | Self::Error { id, .. } => {
                Some(*id)
            }
            Self::Event { .. } | Self::Log { .. } => None,
        }
    }

    /// Validates constraints that cannot be expressed by Serde field types.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty operation name or invalid error code.
    pub fn validate(&self) -> Result<(), SchemaError> {
        match self {
            Self::Request { op, .. } | Self::Error { op, .. } | Self::Event { op, .. } => {
                validate_operation(op)?;
            }
            Self::Response { op: Some(op), .. } => validate_operation(op)?,
            Self::Response { op: None, .. } | Self::Log { .. } => {}
        }
        if let Self::Error { payload, .. } = self {
            payload.validate()?;
        }
        Ok(())
    }
}

fn validate_operation(operation: &str) -> Result<(), SchemaError> {
    if operation.is_empty() {
        return Err(SchemaError::EmptyOperation);
    }
    Ok(())
}

/// Envelope schema validation failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SchemaError {
    /// Operations must have a name.
    #[error("IPC operation names must not be empty")]
    EmptyOperation,
    /// Error codes are stable ASCII identifiers.
    #[error("IPC error code `{0}` is not a non-empty ASCII identifier")]
    InvalidErrorCode(String),
}

#[cfg(test)]
mod tests {
    use super::{Envelope, IpcErrorPayload, IpcValue, LogLevel, LogPayload, LogTarget};

    #[test]
    fn error_codes_are_closed_ascii_identifiers() {
        assert!(IpcErrorPayload::new("SerialDisconnected", "gone").is_ok());
        assert!(IpcErrorPayload::new("", "gone").is_err());
        assert!(IpcErrorPayload::new("not-valid", "gone").is_err());
        assert!(IpcErrorPayload::new("秘密", "gone").is_err());
    }

    #[test]
    fn every_envelope_variant_is_typed() {
        let messages = [
            Envelope::Request {
                id: 1,
                op: "worker.ping".to_owned(),
                payload: IpcValue::Nil,
            },
            Envelope::Response {
                id: 1,
                op: None,
                payload: IpcValue::Bool(true),
            },
            Envelope::Error {
                id: 2,
                op: "worker.unknown".to_owned(),
                payload: IpcErrorPayload::new("NotFound", "unknown operation")
                    .expect("valid payload"),
            },
            Envelope::Event {
                op: "worker.ready".to_owned(),
                payload: IpcValue::Nil,
            },
            Envelope::Log {
                payload: LogPayload {
                    level: LogLevel::Info,
                    message: "ready".to_owned(),
                    target: LogTarget::Log,
                },
            },
        ];
        assert!(messages.iter().all(|message| message.validate().is_ok()));
    }
}
