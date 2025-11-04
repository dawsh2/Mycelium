//! Schema compatibility checking for protocol evolution.
//!
//! This module provides tools to validate whether two schema versions are compatible.
//! Compatibility rules:
//! - Adding optional fields is compatible (forward/backward)
//! - Removing required fields is breaking
//! - Changing field types is breaking
//! - Adding new message types is compatible (with UnknownTypePolicy::Skip)
//! - Removing message types is breaking if consumers depend on them

use std::collections::HashMap;

/// Represents a message schema definition
#[derive(Debug, Clone, PartialEq)]
pub struct MessageSchema {
    pub type_id: u16,
    pub name: String,
    pub fields: Vec<FieldSchema>,
}

/// Represents a field within a message
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSchema {
    pub name: String,
    pub field_type: String,
    pub optional: bool,
}

/// Complete schema definition with version metadata
#[derive(Debug, Clone)]
pub struct Schema {
    pub schema_id: u32,
    pub version: u16,
    pub min_compatible: u16,
    pub messages: HashMap<String, MessageSchema>,
}

/// Types of compatibility between schemas
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityLevel {
    /// Fully compatible - can interoperate without issues
    FullyCompatible,
    /// Forward compatible - new decoder can read old messages
    ForwardCompatible,
    /// Backward compatible - old decoder can read new messages
    BackwardCompatible,
    /// Incompatible - breaking changes detected
    Incompatible,
}

/// Specific compatibility issue found during checking
#[derive(Debug, Clone, PartialEq)]
pub enum CompatibilityIssue {
    /// Schema IDs don't match (different protocols entirely)
    SchemaMismatch { old_id: u32, new_id: u32 },
    /// Version is lower than min_compatible
    VersionTooOld { version: u16, min_compatible: u16 },
    /// Message was removed
    MessageRemoved { name: String, type_id: u16 },
    /// Required field was removed
    RequiredFieldRemoved { message: String, field: String },
    /// Field type changed
    FieldTypeChanged {
        message: String,
        field: String,
        old_type: String,
        new_type: String,
    },
    /// Optional field became required
    FieldBecameRequired { message: String, field: String },
    /// Type ID changed for existing message
    TypeIdChanged {
        message: String,
        old_id: u16,
        new_id: u16,
    },
}

/// Result of compatibility check
#[derive(Debug)]
pub struct CompatibilityReport {
    pub level: CompatibilityLevel,
    pub issues: Vec<CompatibilityIssue>,
    pub warnings: Vec<String>,
}

impl CompatibilityReport {
    /// Creates a fully compatible report
    pub fn compatible() -> Self {
        Self {
            level: CompatibilityLevel::FullyCompatible,
            issues: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Checks if schemas are compatible (not incompatible)
    pub fn is_compatible(&self) -> bool {
        self.level != CompatibilityLevel::Incompatible
    }

    /// Adds a breaking issue
    pub fn add_issue(&mut self, issue: CompatibilityIssue) {
        self.level = CompatibilityLevel::Incompatible;
        self.issues.push(issue);
    }

    /// Adds a warning (non-breaking)
    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }
}

/// Checks compatibility between two schema versions
pub fn check_compatibility(old: &Schema, new: &Schema) -> CompatibilityReport {
    let mut report = CompatibilityReport::compatible();

    // Check schema ID match
    if old.schema_id != new.schema_id {
        report.add_issue(CompatibilityIssue::SchemaMismatch {
            old_id: old.schema_id,
            new_id: new.schema_id,
        });
        return report; // No point checking further
    }

    // Check version compatibility range
    if new.version < old.min_compatible {
        report.add_issue(CompatibilityIssue::VersionTooOld {
            version: new.version,
            min_compatible: old.min_compatible,
        });
    }

    // Check for removed messages
    for (msg_name, old_msg) in &old.messages {
        if !new.messages.contains_key(msg_name) {
            report.add_issue(CompatibilityIssue::MessageRemoved {
                name: msg_name.clone(),
                type_id: old_msg.type_id,
            });
        }
    }

    // Check for message-level changes
    for (msg_name, old_msg) in &old.messages {
        if let Some(new_msg) = new.messages.get(msg_name) {
            check_message_compatibility(&mut report, msg_name, old_msg, new_msg);
        }
    }

    // Check for added messages (just a warning)
    for msg_name in new.messages.keys() {
        if !old.messages.contains_key(msg_name) {
            report.add_warning(format!("New message type added: {}", msg_name));
        }
    }

    // Determine final compatibility level if not already incompatible
    if report.level != CompatibilityLevel::Incompatible {
        report.level = determine_compatibility_level(&report, old, new);
    }

    report
}

/// Checks compatibility between two message schemas
fn check_message_compatibility(
    report: &mut CompatibilityReport,
    msg_name: &str,
    old_msg: &MessageSchema,
    new_msg: &MessageSchema,
) {
    // Check if type ID changed
    if old_msg.type_id != new_msg.type_id {
        report.add_issue(CompatibilityIssue::TypeIdChanged {
            message: msg_name.to_string(),
            old_id: old_msg.type_id,
            new_id: new_msg.type_id,
        });
    }

    // Build field maps for easier lookup
    let old_fields: HashMap<&str, &FieldSchema> = old_msg
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();

    let new_fields: HashMap<&str, &FieldSchema> = new_msg
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();

    // Check for removed or modified fields
    for (field_name, old_field) in &old_fields {
        match new_fields.get(field_name) {
            None => {
                // Field was removed
                if !old_field.optional {
                    report.add_issue(CompatibilityIssue::RequiredFieldRemoved {
                        message: msg_name.to_string(),
                        field: field_name.to_string(),
                    });
                } else {
                    report.add_warning(format!(
                        "Optional field '{}' removed from message '{}'",
                        field_name, msg_name
                    ));
                }
            }
            Some(new_field) => {
                // Field exists, check for changes
                if old_field.field_type != new_field.field_type {
                    report.add_issue(CompatibilityIssue::FieldTypeChanged {
                        message: msg_name.to_string(),
                        field: field_name.to_string(),
                        old_type: old_field.field_type.clone(),
                        new_type: new_field.field_type.clone(),
                    });
                }

                if old_field.optional && !new_field.optional {
                    report.add_issue(CompatibilityIssue::FieldBecameRequired {
                        message: msg_name.to_string(),
                        field: field_name.to_string(),
                    });
                }

                if !old_field.optional && new_field.optional {
                    report.add_warning(format!(
                        "Field '{}' became optional in message '{}'",
                        field_name, msg_name
                    ));
                }
            }
        }
    }

    // Check for added fields
    for (field_name, new_field) in &new_fields {
        if !old_fields.contains_key(field_name) {
            if new_field.optional {
                report.add_warning(format!(
                    "Optional field '{}' added to message '{}'",
                    field_name, msg_name
                ));
            } else {
                report.add_warning(format!(
                    "Required field '{}' added to message '{}' (may break old decoders)",
                    field_name, msg_name
                ));
            }
        }
    }
}

/// Determines the compatibility level based on the report
fn determine_compatibility_level(
    report: &CompatibilityReport,
    old: &Schema,
    new: &Schema,
) -> CompatibilityLevel {
    if !report.issues.is_empty() {
        return CompatibilityLevel::Incompatible;
    }

    // If version increased, we're forward compatible (new can read old)
    // If messages were only added, we're backward compatible (old can read new with Skip policy)
    if new.version > old.version {
        CompatibilityLevel::ForwardCompatible
    } else if new.version == old.version {
        CompatibilityLevel::FullyCompatible
    } else {
        CompatibilityLevel::BackwardCompatible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_schema(version: u16, messages: Vec<MessageSchema>) -> Schema {
        Schema {
            schema_id: 0x4D594345,
            version,
            min_compatible: 1,
            messages: messages.into_iter().map(|m| (m.name.clone(), m)).collect(),
        }
    }

    fn create_test_message(type_id: u16, name: &str, fields: Vec<FieldSchema>) -> MessageSchema {
        MessageSchema {
            type_id,
            name: name.to_string(),
            fields,
        }
    }

    fn required_field(name: &str, field_type: &str) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            field_type: field_type.to_string(),
            optional: false,
        }
    }

    fn optional_field(name: &str, field_type: &str) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            field_type: field_type.to_string(),
            optional: true,
        }
    }

    #[test]
    fn test_identical_schemas_fully_compatible() {
        let msg = create_test_message(
            1018,
            "InstrumentMeta",
            vec![
                required_field("symbol", "String"),
                required_field("decimals", "u8"),
            ],
        );

        let schema1 = create_test_schema(1, vec![msg.clone()]);
        let schema2 = create_test_schema(1, vec![msg]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::FullyCompatible);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn test_schema_id_mismatch() {
        let msg = create_test_message(1018, "Test", vec![]);
        let mut schema1 = create_test_schema(1, vec![msg.clone()]);
        let mut schema2 = create_test_schema(1, vec![msg]);

        schema1.schema_id = 0x1111;
        schema2.schema_id = 0x2222;

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::Incompatible);
        assert!(matches!(
            report.issues[0],
            CompatibilityIssue::SchemaMismatch { .. }
        ));
    }

    #[test]
    fn test_adding_optional_field_compatible() {
        let msg1 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![required_field("symbol", "String")],
        );

        let msg2 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![
                required_field("symbol", "String"),
                optional_field("decimals", "u8"),
            ],
        );

        let schema1 = create_test_schema(1, vec![msg1]);
        let schema2 = create_test_schema(2, vec![msg2]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::ForwardCompatible);
        assert!(report.issues.is_empty());
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn test_removing_required_field_incompatible() {
        let msg1 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![
                required_field("symbol", "String"),
                required_field("decimals", "u8"),
            ],
        );

        let msg2 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![required_field("symbol", "String")],
        );

        let schema1 = create_test_schema(1, vec![msg1]);
        let schema2 = create_test_schema(2, vec![msg2]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::Incompatible);
        assert!(matches!(
            report.issues[0],
            CompatibilityIssue::RequiredFieldRemoved { .. }
        ));
    }

    #[test]
    fn test_changing_field_type_incompatible() {
        let msg1 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![required_field("decimals", "u8")],
        );

        let msg2 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![required_field("decimals", "u16")],
        );

        let schema1 = create_test_schema(1, vec![msg1]);
        let schema2 = create_test_schema(2, vec![msg2]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::Incompatible);
        assert!(matches!(
            report.issues[0],
            CompatibilityIssue::FieldTypeChanged { .. }
        ));
    }

    #[test]
    fn test_optional_to_required_incompatible() {
        let msg1 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![optional_field("decimals", "u8")],
        );

        let msg2 = create_test_message(
            1018,
            "InstrumentMeta",
            vec![required_field("decimals", "u8")],
        );

        let schema1 = create_test_schema(1, vec![msg1]);
        let schema2 = create_test_schema(2, vec![msg2]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::Incompatible);
        assert!(matches!(
            report.issues[0],
            CompatibilityIssue::FieldBecameRequired { .. }
        ));
    }

    #[test]
    fn test_removing_message_incompatible() {
        let msg1 = create_test_message(1018, "InstrumentMeta", vec![]);
        let msg2 = create_test_message(1020, "ArbitrageSignal", vec![]);

        let schema1 = create_test_schema(1, vec![msg1.clone(), msg2]);
        let schema2 = create_test_schema(2, vec![msg1]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::Incompatible);
        assert!(matches!(
            report.issues[0],
            CompatibilityIssue::MessageRemoved { .. }
        ));
    }

    #[test]
    fn test_adding_message_compatible() {
        let msg1 = create_test_message(1018, "InstrumentMeta", vec![]);
        let msg2 = create_test_message(1020, "ArbitrageSignal", vec![]);

        let schema1 = create_test_schema(1, vec![msg1.clone()]);
        let schema2 = create_test_schema(2, vec![msg1, msg2]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::ForwardCompatible);
        assert!(report.issues.is_empty());
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn test_type_id_changed_incompatible() {
        let msg1 = create_test_message(1018, "InstrumentMeta", vec![]);
        let msg2 = create_test_message(2000, "InstrumentMeta", vec![]);

        let schema1 = create_test_schema(1, vec![msg1]);
        let schema2 = create_test_schema(2, vec![msg2]);

        let report = check_compatibility(&schema1, &schema2);
        assert_eq!(report.level, CompatibilityLevel::Incompatible);
        assert!(matches!(
            report.issues[0],
            CompatibilityIssue::TypeIdChanged { .. }
        ));
    }
}
