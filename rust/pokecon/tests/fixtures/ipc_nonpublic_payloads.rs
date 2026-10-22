// AR-11-37 intentional non-public wire fixture
// NOT a Cargo target; included as source by the codec unit test
// (included by `worker::ipc::codec`'s `mod tests` via include!) and read as text by
// contract_sync (`worker_ipc_deployment_boundary_is_pinned`).
// Pins byte sequences of non-public payloads that must never appear on the
// wire. Each case is expected to be rejected at the peer's decode layer. The
// byte sequences match the existing codec negative tests; no new wire values
// are invented.

/// Rejection layer a fixture case expects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureExpectation {
    /// `CodecError::Decode` (malformed `MessagePack` or closed-schema mismatch).
    DecodeErr,
    /// Decode succeeds but `Envelope::validate` rejects with `CodecError::Schema`.
    SchemaErr,
}

/// Non-public wire payload fixture: (case name, wire bytes, expected rejection layer).
pub const CASES: &[(&str, &[u8], FixtureExpectation)] = &[
    // (1) top-level fixext4 timestamp (same as the codec negative test's timestamp_ext).
    (
        "top_level_fixext4_timestamp",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xac, b'w', b'o', b'r', b'k', b'e', b'r', b'.', b'r', b'e', b'a', b'd', b'y',
            0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xd6, 0xff, 0x00, 0x00, 0x00, 0x00,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (2) ext nested in a map value (same as the codec negative test's nested).
    (
        "nested_ext_in_map_value",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xac, b'w', b'o', b'r', b'k', b'e', b'r', b'.', b'r', b'e', b'a', b'd', b'y',
            0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0x81, 0xa1, b'k', 0xd6, 0xff, 0x00,
            0x00, 0x00, 0x00,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (3) non-string map key (from the codec negative test's LooseEnvelope).
    (
        "non_string_map_key",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xab, b'i', b'n', b'v', b'a', b'l', b'i', b'd', b'.', b'm', b'a', b'p', 0xa7,
            b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0x81, 0x01, 0xc3,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (4) unknown envelope field (from the codec negative test's UnknownFieldEnvelope).
    (
        "unknown_envelope_field",
        &[
            0x84, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xac, b'w', b'o', b'r', b'k', b'e', b'r', b'.', b'r', b'e', b'a', b'd', b'y',
            0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xc0, 0xaa, b'u', b'n', b'e', b'x',
            b'p', b'e', b'c', b't', b'e', b'd', 0xc3,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (5) second object at the end of the frame (from the codec negative test's trailing).
    (
        "trailing_second_object",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xac, b'w', b'o', b'r', b'k', b'e', b'r', b'.', b'r', b'e', b'a', b'd', b'y',
            0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xc0, 0xc0,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (6) unknown error payload field (from the codec negative test's LooseErrorEnvelope).
    (
        "unknown_error_payload_field",
        &[
            0x84, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'r', b'r', b'o', b'r', 0xa2, b'i',
            b'd', 0x07, 0xa2, b'o', b'p', 0xab, b's', b'e', b'r', b'i', b'a', b'l', b'.', b's',
            b'e', b'n', b'd', 0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0x83, 0xa4, b'c',
            b'o', b'd', b'e', 0xab, b'S', b'e', b'r', b'i', b'a', b'l', b'E', b'r', b'r', b'o',
            b'r', 0xa7, b'm', b'e', b's', b's', b'a', b'g', b'e', 0xa6, b'f', b'a', b'i', b'l',
            b'e', b'd', 0xa6, b's', b'e', b'c', b'r', b'e', b't', 0xb4, b'm', b'u', b's', b't',
            b'-', b'n', b'o', b't', b'-', b'p', b'a', b's', b's', b'-', b's', b'c', b'h', b'e',
            b'm', b'a',
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (7) empty op string (decodes, but `Envelope::validate` rejects it).
    (
        "empty_op_string",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xa0, 0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xc0,
        ],
        FixtureExpectation::SchemaErr,
    ),
];
