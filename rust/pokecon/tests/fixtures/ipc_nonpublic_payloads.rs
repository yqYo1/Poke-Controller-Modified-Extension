// AR-11-37 intentional non-public wire fixture
// NOT a Cargo target; included as source by the codec unit test
// (`worker::ipc::codec` の `mod tests` が include! する) and read as text by
// contract_sync (`worker_ipc_deployment_boundary_is_pinned`).
// ワイヤ上に現れてはならない非公開ペイロードの byte 列を pin する。各 case は
// peer の decode 層で拒否されることを期待する。byte 列は既存 codec negative
// test と同一の値であり、新規の wire 値を発明しない。

/// Fixture case が期待する拒否層。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureExpectation {
    /// `CodecError::Decode` (malformed `MessagePack` または closed schema 不一致)。
    DecodeErr,
    /// decode は通るが `Envelope::validate` が `CodecError::Schema` で拒否。
    SchemaErr,
}

/// 非公開 wire payload fixture: (case 名, wire bytes, 期待する拒否層)。
pub const CASES: &[(&str, &[u8], FixtureExpectation)] = &[
    // (1) top-level fixext4 timestamp (codec negative test の timestamp_ext と同一)。
    (
        "top_level_fixext4_timestamp",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xac, b'w', b'o', b'r', b'k', b'e', b'r', b'.', b'r', b'e', b'a', b'd', b'y',
            0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xd6, 0xff, 0x00, 0x00, 0x00, 0x00,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (2) map 値に nested した ext (codec negative test の nested と同一)。
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
    // (3) 非文字列マップキー (codec negative test の LooseEnvelope 由来)。
    (
        "non_string_map_key",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xab, b'i', b'n', b'v', b'a', b'l', b'i', b'd', b'.', b'm', b'a', b'p', 0xa7,
            b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0x81, 0x01, 0xc3,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (4) 未知 envelope フィールド (codec negative test の UnknownFieldEnvelope 由来)。
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
    // (5) frame 末尾の第二 object (codec negative test の trailing 由来)。
    (
        "trailing_second_object",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xac, b'w', b'o', b'r', b'k', b'e', b'r', b'.', b'r', b'e', b'a', b'd', b'y',
            0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xc0, 0xc0,
        ],
        FixtureExpectation::DecodeErr,
    ),
    // (6) 未知 error payload フィールド (codec negative test の LooseErrorEnvelope 由来)。
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
    // (7) 空 op 文字列 (decode は通るが `Envelope::validate` が拒否)。
    (
        "empty_op_string",
        &[
            0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
            b'p', 0xa0, 0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xc0,
        ],
        FixtureExpectation::SchemaErr,
    ),
];
