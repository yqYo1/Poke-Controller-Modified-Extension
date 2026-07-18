use std::io;
use std::io::Cursor;

use serde::Deserialize as _;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::schema::Envelope;

/// Maximum `MessagePack` body size mandated by the IPC specification.
pub const MAX_PAYLOAD_BYTES: usize = 1_048_576;

/// Framing, schema, or pipe I/O failure.
#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
pub enum CodecError {
    /// A frame advertised an allocation larger than the protocol limit.
    #[error("IPC payload length {actual} exceeds the {maximum}-byte limit")]
    PayloadTooLarge {
        /// Advertised or encoded payload size.
        actual: usize,
        /// Protocol limit.
        maximum: usize,
    },
    /// EOF split the four-byte prefix.
    #[error("IPC frame ended after {received} of 4 length-prefix bytes")]
    TruncatedHeader {
        /// Prefix bytes received before EOF.
        received: usize,
    },
    /// EOF split a `MessagePack` body.
    #[error("IPC frame ended after {received} of {expected} payload bytes")]
    TruncatedPayload {
        /// Advertised payload size.
        expected: usize,
        /// Bytes received before EOF.
        received: usize,
    },
    /// `MessagePack` serialization failed.
    #[error("failed to encode IPC MessagePack: {0}")]
    Encode(String),
    /// `MessagePack` decoding or the closed envelope schema failed.
    #[error("failed to decode IPC MessagePack: {0}")]
    Decode(String),
    /// An envelope violated a semantic schema constraint.
    #[error("invalid IPC envelope: {0}")]
    Schema(String),
    /// Pipe read or write failed.
    #[error("IPC pipe I/O failed: {0}")]
    Io(String),
}

impl From<io::Error> for CodecError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Encodes one complete length-prefixed frame.
///
/// # Errors
///
/// Returns an error for an invalid envelope, serialization failure, or a body
/// larger than 1 MiB.
pub fn encode_frame(envelope: &Envelope) -> Result<Vec<u8>, CodecError> {
    envelope
        .validate()
        .map_err(|error| CodecError::Schema(error.to_string()))?;
    let payload =
        rmp_serde::to_vec_named(envelope).map_err(|error| CodecError::Encode(error.to_string()))?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(CodecError::PayloadTooLarge {
            actual: payload.len(),
            maximum: MAX_PAYLOAD_BYTES,
        });
    }
    let payload_length = u32::try_from(payload.len()).map_err(|_| CodecError::PayloadTooLarge {
        actual: payload.len(),
        maximum: MAX_PAYLOAD_BYTES,
    })?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&payload_length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decodes and validates an already bounded `MessagePack` body.
///
/// # Errors
///
/// Returns an error for an oversized body, malformed `MessagePack`, unknown
/// envelope fields, or invalid closed payload values.
pub fn decode_payload(payload: &[u8]) -> Result<Envelope, CodecError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(CodecError::PayloadTooLarge {
            actual: payload.len(),
            maximum: MAX_PAYLOAD_BYTES,
        });
    }
    let mut deserializer = rmp_serde::Deserializer::new(Cursor::new(payload));
    deserializer.set_max_depth(128);
    let envelope = Envelope::deserialize(&mut deserializer)
        .map_err(|error| CodecError::Decode(error.to_string()))?;
    let consumed = usize::try_from(deserializer.into_inner().position())
        .map_err(|error| CodecError::Decode(error.to_string()))?;
    if consumed != payload.len() {
        return Err(CodecError::Decode(format!(
            "frame contains {} trailing bytes after the top-level object",
            payload.len() - consumed
        )));
    }
    envelope
        .validate()
        .map_err(|error| CodecError::Schema(error.to_string()))?;
    Ok(envelope)
}

/// Reads one frame, returning `None` only for clean EOF before a new prefix.
///
/// The length limit is checked before allocating the payload buffer.
///
/// # Errors
///
/// Returns an error for partial frames, oversized lengths, pipe failures, or an
/// invalid `MessagePack` envelope.
pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Envelope>, CodecError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0_u8; 4];
    let header_bytes = read_until_full_or_eof(reader, &mut header).await?;
    if header_bytes == 0 {
        return Ok(None);
    }
    if header_bytes != header.len() {
        return Err(CodecError::TruncatedHeader {
            received: header_bytes,
        });
    }

    let payload_length = u32::from_be_bytes(header) as usize;
    if payload_length > MAX_PAYLOAD_BYTES {
        return Err(CodecError::PayloadTooLarge {
            actual: payload_length,
            maximum: MAX_PAYLOAD_BYTES,
        });
    }
    let mut payload = vec![0_u8; payload_length];
    let payload_bytes = read_until_full_or_eof(reader, &mut payload).await?;
    if payload_bytes != payload_length {
        return Err(CodecError::TruncatedPayload {
            expected: payload_length,
            received: payload_bytes,
        });
    }
    decode_payload(&payload).map(Some)
}

/// Writes and flushes one complete serialized frame.
///
/// # Errors
///
/// Returns an error from encoding or pipe I/O.
pub async fn write_frame<W>(writer: &mut W, envelope: &Envelope) -> Result<(), CodecError>
where
    W: AsyncWrite + Unpin,
{
    let frame = encode_frame(envelope)?;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_until_full_or_eof<R>(reader: &mut R, buffer: &mut [u8]) -> io::Result<usize>
where
    R: AsyncRead + Unpin,
{
    let mut filled = 0;
    while filled < buffer.len() {
        let read = reader.read(&mut buffer[filled..]).await?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::Serialize;
    use tokio::io::{AsyncWriteExt, duplex};

    use super::{CodecError, MAX_PAYLOAD_BYTES, decode_payload, encode_frame, read_frame};
    use crate::ipc::{Envelope, IpcValue};

    #[test]
    fn frame_round_trip_uses_big_endian_length() {
        let envelope = Envelope::Event {
            op: "worker.ready".to_owned(),
            payload: IpcValue::Binary(vec![0, 1, 2]),
        };
        let frame = encode_frame(&envelope).expect("frame encodes");
        let length = u32::from_be_bytes(frame[..4].try_into().expect("four-byte prefix"));
        assert_eq!(length as usize, frame.len() - 4);
        assert_eq!(
            decode_payload(&frame[4..]).expect("payload decodes"),
            envelope
        );
    }

    #[tokio::test]
    async fn oversize_prefix_is_rejected_before_payload_read() {
        let (mut sender, mut receiver) = duplex(8);
        sender
            .write_all(&u32::try_from(MAX_PAYLOAD_BYTES + 1).unwrap().to_be_bytes())
            .await
            .expect("prefix writes");
        let error = read_frame(&mut receiver)
            .await
            .expect_err("oversize is rejected immediately");
        assert!(matches!(error, CodecError::PayloadTooLarge { .. }));
    }

    #[tokio::test]
    async fn partial_payload_is_not_dispatched() {
        let (mut sender, mut receiver) = duplex(32);
        sender.write_all(&10_u32.to_be_bytes()).await.unwrap();
        sender.write_all(&[1, 2, 3]).await.unwrap();
        sender.shutdown().await.unwrap();
        assert_eq!(
            read_frame(&mut receiver).await.unwrap_err(),
            CodecError::TruncatedPayload {
                expected: 10,
                received: 3,
            }
        );
    }

    #[derive(Serialize)]
    struct UnknownFieldEnvelope<'a> {
        kind: &'a str,
        op: &'a str,
        payload: (),
        unexpected: bool,
    }

    #[derive(Serialize)]
    struct LooseEnvelope<'a, T> {
        kind: &'a str,
        op: &'a str,
        payload: T,
    }

    #[derive(Serialize)]
    struct LooseErrorPayload<'a> {
        code: &'a str,
        message: &'a str,
        secret: &'a str,
    }

    #[derive(Serialize)]
    struct LooseErrorEnvelope<'a> {
        kind: &'a str,
        id: u64,
        op: &'a str,
        payload: LooseErrorPayload<'a>,
    }

    #[test]
    fn unknown_envelope_fields_are_rejected() {
        let payload = rmp_serde::to_vec_named(&UnknownFieldEnvelope {
            kind: "event",
            op: "worker.ready",
            payload: (),
            unexpected: true,
        })
        .unwrap();
        assert!(matches!(
            decode_payload(&payload),
            Err(CodecError::Decode(_))
        ));
    }

    #[test]
    fn trailing_second_object_is_rejected() {
        let envelope = Envelope::Event {
            op: "worker.ready".to_owned(),
            payload: IpcValue::Nil,
        };
        let frame = encode_frame(&envelope).unwrap();
        let mut payload = frame[4..].to_vec();
        payload.push(0xc0);
        assert!(matches!(
            decode_payload(&payload),
            Err(CodecError::Decode(_))
        ));
    }

    #[test]
    fn non_string_payload_map_keys_are_rejected() {
        let payload = rmp_serde::to_vec_named(&LooseEnvelope {
            kind: "event",
            op: "invalid.map",
            payload: BTreeMap::from([(1_u8, true)]),
        })
        .unwrap();
        assert!(matches!(
            decode_payload(&payload),
            Err(CodecError::Decode(_))
        ));
    }

    #[test]
    fn error_payload_unknown_fields_are_rejected() {
        let payload = rmp_serde::to_vec_named(&LooseErrorEnvelope {
            kind: "error",
            id: 7,
            op: "serial.send",
            payload: LooseErrorPayload {
                code: "SerialError",
                message: "failed",
                secret: "must-not-pass-schema",
            },
        })
        .unwrap();
        assert!(matches!(
            decode_payload(&payload),
            Err(CodecError::Decode(_))
        ));
    }
}
