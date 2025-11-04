//! Integration tests for schema evolution
//!
//! These tests verify that the entire schema evolution workflow works correctly,
//! including encoding/decoding across versions and unknown type handling.

use mycelium_protocol::{
    check_compatibility, decode_any_message_with_policy, CompatibilityLevel, FieldSchema,
    MessageSchema, Schema, UnknownTypePolicy,
};

/// Helper to create a test schema
fn create_schema(version: u16, messages: Vec<MessageSchema>) -> Schema {
    Schema {
        schema_id: 0x4D594345,
        version,
        min_compatible: 1,
        messages: messages.into_iter().map(|m| (m.name.clone(), m)).collect(),
    }
}

#[test]
fn test_forward_compatibility_with_unknown_policy() {
    // Scenario: Old client (v1) receives message from new server (v2)
    // Old client doesn't know about new message types, but can handle them gracefully

    // Define v1 schema (only has InstrumentMeta)
    let v1_schema = create_schema(
        1,
        vec![MessageSchema {
            type_id: 1018,
            name: "InstrumentMeta".to_string(),
            fields: vec![FieldSchema {
                name: "symbol".to_string(),
                field_type: "String".to_string(),
                optional: false,
            }],
        }],
    );

    // Define v2 schema (adds ArbitrageSignal)
    let v2_schema = create_schema(
        2,
        vec![
            MessageSchema {
                type_id: 1018,
                name: "InstrumentMeta".to_string(),
                fields: vec![FieldSchema {
                    name: "symbol".to_string(),
                    field_type: "String".to_string(),
                    optional: false,
                }],
            },
            MessageSchema {
                type_id: 1020,
                name: "ArbitrageSignal".to_string(),
                fields: vec![FieldSchema {
                    name: "opportunity_id".to_string(),
                    field_type: "u64".to_string(),
                    optional: false,
                }],
            },
        ],
    );

    // Check compatibility
    let compat = check_compatibility(&v1_schema, &v2_schema);
    assert_eq!(compat.level, CompatibilityLevel::ForwardCompatible);
    assert!(compat.is_compatible());
    assert!(compat.issues.is_empty());

    // Simulate old client receiving unknown type
    // Use type_id 9999 which doesn't exist in current contracts
    let unknown_bytes = vec![
        0x0F, 0x27, // type_id: 9999 as u16 LE - truly unknown type
        0x10, 0x00, 0x00, 0x00, // length: 16 as u32 LE
        0x01, 0x02, 0x03, 0x04, // payload (arbitrary)
        0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
    ];

    // Old client with Skip policy can handle unknown messages
    let result = decode_any_message_with_policy(&unknown_bytes, UnknownTypePolicy::Skip);
    assert!(result.is_ok());

    // Old client with Fail policy would reject
    let result = decode_any_message_with_policy(&unknown_bytes, UnknownTypePolicy::Fail);
    assert!(result.is_err());
}

#[test]
fn test_backward_compatibility_optional_fields() {
    // Scenario: New client (v2) receives message from old server (v1)
    // v2 added optional fields that v1 doesn't send

    let v1_schema = create_schema(
        1,
        vec![MessageSchema {
            type_id: 1018,
            name: "InstrumentMeta".to_string(),
            fields: vec![FieldSchema {
                name: "symbol".to_string(),
                field_type: "String".to_string(),
                optional: false,
            }],
        }],
    );

    let v2_schema = create_schema(
        2,
        vec![MessageSchema {
            type_id: 1018,
            name: "InstrumentMeta".to_string(),
            fields: vec![
                FieldSchema {
                    name: "symbol".to_string(),
                    field_type: "String".to_string(),
                    optional: false,
                },
                FieldSchema {
                    name: "decimals".to_string(),
                    field_type: "u8".to_string(),
                    optional: true, // New optional field
                },
            ],
        }],
    );

    let compat = check_compatibility(&v1_schema, &v2_schema);
    assert!(compat.is_compatible());
    assert!(compat.issues.is_empty());
    assert_eq!(compat.warnings.len(), 1); // Warning about added field
}

#[test]
fn test_breaking_change_removed_required_field() {
    let v1_schema = create_schema(
        1,
        vec![MessageSchema {
            type_id: 1018,
            name: "InstrumentMeta".to_string(),
            fields: vec![
                FieldSchema {
                    name: "symbol".to_string(),
                    field_type: "String".to_string(),
                    optional: false,
                },
                FieldSchema {
                    name: "decimals".to_string(),
                    field_type: "u8".to_string(),
                    optional: false,
                },
            ],
        }],
    );

    let v2_schema = create_schema(
        2,
        vec![MessageSchema {
            type_id: 1018,
            name: "InstrumentMeta".to_string(),
            fields: vec![FieldSchema {
                name: "symbol".to_string(),
                field_type: "String".to_string(),
                optional: false,
            }],
        }],
    );

    let compat = check_compatibility(&v1_schema, &v2_schema);
    assert_eq!(compat.level, CompatibilityLevel::Incompatible);
    assert!(!compat.is_compatible());
    assert_eq!(compat.issues.len(), 1);
}

#[test]
fn test_breaking_change_type_modified() {
    let v1_schema = create_schema(
        1,
        vec![MessageSchema {
            type_id: 1018,
            name: "InstrumentMeta".to_string(),
            fields: vec![FieldSchema {
                name: "decimals".to_string(),
                field_type: "u8".to_string(),
                optional: false,
            }],
        }],
    );

    let v2_schema = create_schema(
        2,
        vec![MessageSchema {
            type_id: 1018,
            name: "InstrumentMeta".to_string(),
            fields: vec![FieldSchema {
                name: "decimals".to_string(),
                field_type: "u16".to_string(), // Type changed!
                optional: false,
            }],
        }],
    );

    let compat = check_compatibility(&v1_schema, &v2_schema);
    assert_eq!(compat.level, CompatibilityLevel::Incompatible);
    assert!(!compat.is_compatible());
}

#[test]
fn test_schema_id_mismatch_different_protocols() {
    let mut schema1 = create_schema(1, vec![]);
    let mut schema2 = create_schema(1, vec![]);

    schema1.schema_id = 0x11111111;
    schema2.schema_id = 0x22222222;

    let compat = check_compatibility(&schema1, &schema2);
    assert_eq!(compat.level, CompatibilityLevel::Incompatible);
    assert!(!compat.is_compatible());
}

#[test]
fn test_version_within_compatible_range() {
    let mut old_schema = create_schema(5, vec![]);
    old_schema.min_compatible = 3;

    let new_schema_compat = create_schema(4, vec![]);
    let compat = check_compatibility(&old_schema, &new_schema_compat);
    assert!(compat.is_compatible());

    let new_schema_incompat = create_schema(2, vec![]);
    let compat = check_compatibility(&old_schema, &new_schema_incompat);
    assert!(!compat.is_compatible());
}

#[test]
fn test_multiple_messages_compatibility() {
    // Complex scenario with multiple message types
    let v1_schema = create_schema(
        1,
        vec![
            MessageSchema {
                type_id: 1018,
                name: "InstrumentMeta".to_string(),
                fields: vec![FieldSchema {
                    name: "symbol".to_string(),
                    field_type: "String".to_string(),
                    optional: false,
                }],
            },
            MessageSchema {
                type_id: 1011,
                name: "PoolState".to_string(),
                fields: vec![FieldSchema {
                    name: "reserves".to_string(),
                    field_type: "U256".to_string(),
                    optional: false,
                }],
            },
        ],
    );

    let v2_schema = create_schema(
        2,
        vec![
            MessageSchema {
                type_id: 1018,
                name: "InstrumentMeta".to_string(),
                fields: vec![
                    FieldSchema {
                        name: "symbol".to_string(),
                        field_type: "String".to_string(),
                        optional: false,
                    },
                    FieldSchema {
                        name: "decimals".to_string(),
                        field_type: "u8".to_string(),
                        optional: true, // Added optional field
                    },
                ],
            },
            MessageSchema {
                type_id: 1011,
                name: "PoolState".to_string(),
                fields: vec![FieldSchema {
                    name: "reserves".to_string(),
                    field_type: "U256".to_string(),
                    optional: false,
                }],
            },
            MessageSchema {
                type_id: 1020,
                name: "ArbitrageSignal".to_string(), // New message type
                fields: vec![FieldSchema {
                    name: "opportunity_id".to_string(),
                    field_type: "u64".to_string(),
                    optional: false,
                }],
            },
        ],
    );

    let compat = check_compatibility(&v1_schema, &v2_schema);
    assert_eq!(compat.level, CompatibilityLevel::ForwardCompatible);
    assert!(compat.is_compatible());
    assert!(compat.issues.is_empty());
    assert_eq!(compat.warnings.len(), 2); // One for added field, one for added message
}

#[test]
fn test_unknown_type_policy_store() {
    // Test that Store policy preserves unknown message data
    let unknown_bytes = vec![
        0x88, 0x13, // type_id: 5000 (0x1388) as u16 LE
        0x08, 0x00, 0x00, 0x00, // length: 8 as u32 LE
        0xDE, 0xAD, 0xBE, 0xEF, // payload
        0xCA, 0xFE, 0xBA, 0xBE,
    ];

    let result = decode_any_message_with_policy(&unknown_bytes, UnknownTypePolicy::Store);
    assert!(result.is_ok());

    // With Store policy, we can inspect the unknown message
    if let Ok(msg) = result {
        match msg {
            mycelium_protocol::AnyMessage::Unknown(unknown) => {
                assert_eq!(unknown.type_id, 5000);
                assert_eq!(unknown.payload.len(), 8);
                assert_eq!(&unknown.payload[0..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
            }
            _ => panic!("Expected Unknown variant"),
        }
    }
}
