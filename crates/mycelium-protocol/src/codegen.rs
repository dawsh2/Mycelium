//! Mycelium Protocol Code Generator
//!
//! Generates Rust message types from YAML contract definitions.
//!
//! This module is used internally by build.rs to generate message types at compile time.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Error type for code generation
#[derive(Debug)]
pub enum CodegenError {
    IoError(std::io::Error),
    YamlError(serde_yaml::Error),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::IoError(e) => write!(f, "IO error: {}", e),
            CodegenError::YamlError(e) => write!(f, "YAML error: {}", e),
        }
    }
}

impl std::error::Error for CodegenError {}

impl From<std::io::Error> for CodegenError {
    fn from(e: std::io::Error) -> Self {
        CodegenError::IoError(e)
    }
}

impl From<serde_yaml::Error> for CodegenError {
    fn from(e: serde_yaml::Error) -> Self {
        CodegenError::YamlError(e)
    }
}

/// Generate Rust code from a YAML contract file
///
/// This is the main entry point for code generation used by build.rs.
#[allow(dead_code)]
pub fn generate_from_yaml<P: AsRef<Path>>(
    yaml_path: P,
    output_path: P,
) -> Result<(), CodegenError> {
    generate_from_yaml_with_imports(yaml_path, output_path, false)
}

/// Generate Rust code from a YAML contract file with external imports
///
/// Use this when generating code for external crates that depend on mycelium-protocol.
#[allow(dead_code)] // Public API for external code generation
pub fn generate_from_yaml_external<P: AsRef<Path>>(
    yaml_path: P,
    output_path: P,
) -> Result<(), CodegenError> {
    generate_from_yaml_with_imports(yaml_path, output_path, true)
}

fn generate_from_yaml_with_imports<P: AsRef<Path>>(
    yaml_path: P,
    output_path: P,
    use_external_imports: bool,
) -> Result<(), CodegenError> {
    // Read and parse YAML
    let yaml_content = fs::read_to_string(yaml_path.as_ref())?;
    let contracts: Contracts = serde_yaml::from_str(&yaml_content)?;

    // Validate tag ranges
    validate_tag_ranges(&contracts)?;

    // Generate code
    let generated_code = generate_messages(&contracts, use_external_imports);

    // Write output
    fs::write(output_path.as_ref(), generated_code)?;

    Ok(())
}

/// Generate Python helpers from a YAML contract file
#[allow(dead_code)]
pub fn generate_python_from_yaml<P: AsRef<Path>>(
    yaml_path: P,
    output_path: P,
) -> Result<(), CodegenError> {
    let yaml_content = fs::read_to_string(yaml_path.as_ref())?;
    let contracts: Contracts = serde_yaml::from_str(&yaml_content)?;

    validate_tag_ranges(&contracts)?;

    let python_module = generate_python_module(&contracts);
    fs::write(output_path.as_ref(), python_module)?;

    Ok(())
}

/// Contract definition from contracts.yaml
#[derive(Debug, Deserialize)]
struct Contracts {
    #[serde(default)]
    #[allow(dead_code)] // Reserved for future schema versioning
    schema: Option<SchemaConfig>,
    #[serde(default)]
    tag_ranges: Option<TagRanges>,
    messages: HashMap<String, MessageContract>,
}

/// Schema configuration for versioning
#[derive(Debug, Deserialize)]
struct SchemaConfig {
    #[allow(dead_code)]
    id: u32,
    #[allow(dead_code)]
    version: u16,
    #[serde(default)]
    #[allow(dead_code)]
    min_compatible: Option<u16>,
}

/// Tag ranges for preventing conflicts
#[derive(Debug, Deserialize)]
struct TagRanges {
    #[serde(default)]
    core: Option<String>, // e.g., "1-999"
    #[serde(default)]
    app: Option<String>, // e.g., "1000-49999"
    #[serde(default)]
    experimental: Option<String>, // e.g., "50000-65535"
}

/// Parsed range
#[derive(Debug, Clone)]
struct Range {
    start: u16,
    end: u16,
}

impl Range {
    fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err(format!(
                "Invalid range format: '{}' (expected 'start-end')",
                s
            ));
        }

        let start = parts[0]
            .parse::<u16>()
            .map_err(|e| format!("Invalid range start '{}': {}", parts[0], e))?;
        let end = parts[1]
            .parse::<u16>()
            .map_err(|e| format!("Invalid range end '{}': {}", parts[1], e))?;

        if start > end {
            return Err(format!("Invalid range {}-{}: start > end", start, end));
        }

        Ok(Range { start, end })
    }

    fn contains(&self, value: u16) -> bool {
        value >= self.start && value <= self.end
    }
}

/// Validate that message type IDs are in the correct tag ranges
fn validate_tag_ranges(contracts: &Contracts) -> Result<(), CodegenError> {
    // If no tag_ranges defined, skip validation
    let Some(tag_ranges) = &contracts.tag_ranges else {
        return Ok(());
    };

    // Parse ranges
    let core_range = tag_ranges
        .core
        .as_ref()
        .map(|s| Range::parse(s))
        .transpose()
        .map_err(|e| {
            CodegenError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;

    let app_range = tag_ranges
        .app
        .as_ref()
        .map(|s| Range::parse(s))
        .transpose()
        .map_err(|e| {
            CodegenError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;

    let experimental_range = tag_ranges
        .experimental
        .as_ref()
        .map(|s| Range::parse(s))
        .transpose()
        .map_err(|e| {
            CodegenError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;

    // Validate each message
    for (name, contract) in &contracts.messages {
        let type_id = contract.tlv_type;

        // Check if in core range (reserved for framework)
        if let Some(ref range) = core_range {
            if range.contains(type_id) {
                return Err(CodegenError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Message '{}' uses type_id {} which is in 'core' range ({}-{}) - reserved for Mycelium framework",
                        name, type_id, range.start, range.end
                    )
                )));
            }
        }

        // Warn if in experimental range
        if let Some(ref range) = experimental_range {
            if range.contains(type_id) {
                eprintln!(
                    "cargo:warning=Message '{}' uses type_id {} in 'experimental' range ({}-{}) - can change without version bump",
                    name, type_id, range.start, range.end
                );
            }
        }

        // Check if in app range (if defined)
        if let Some(ref range) = app_range {
            if !range.contains(type_id) {
                // Not in app range - check if it's experimental
                if let Some(ref exp_range) = experimental_range {
                    if !exp_range.contains(type_id) {
                        eprintln!(
                            "cargo:warning=Message '{}' uses type_id {} which is outside 'app' range ({}-{}) and not in 'experimental' range",
                            name, type_id, range.start, range.end
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Individual message contract
#[derive(Debug, Deserialize)]
struct MessageContract {
    tlv_type: u16,
    domain: String,
    description: String,
    #[serde(default)]
    #[allow(dead_code)]
    required_prior_messages: Vec<String>,
    #[serde(default)]
    sensitivity: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    log_payload: Option<bool>,
    // Use IndexMap to preserve YAML field order (important for #[repr(C)] layout!)
    fields: indexmap::IndexMap<String, FieldContract>,
}

/// Field definition in a message
#[derive(Debug, Deserialize)]
struct FieldContract {
    #[serde(rename = "type")]
    field_type: String,
    description: String,
    #[serde(default)]
    #[allow(dead_code)]
    optional: Option<bool>,
    #[serde(default)]
    #[allow(dead_code)]
    validation: Vec<HashMap<String, serde_yaml::Value>>,
}

fn generate_messages(contracts: &Contracts, use_external_imports: bool) -> String {
    let mut code = String::new();

    // Add header
    code.push_str("// AUTO-GENERATED from contracts.yaml - DO NOT EDIT MANUALLY\n");
    code.push_str("//\n");
    code.push_str("// This file is generated at build time by build.rs\n");
    code.push_str("// To modify, edit contracts.yaml and rebuild\n\n");

    // Use appropriate import paths
    if use_external_imports {
        code.push_str("#[allow(unused_imports)]\n");
        code.push_str("use mycelium_protocol::fixed_vec::{FixedStr, FixedVec};\n");
        code.push_str("use mycelium_protocol::Message;\n\n");
    } else {
        code.push_str("#[allow(unused_imports)]\n");
        code.push_str("use crate::fixed_vec::{FixedStr, FixedVec};\n");
        code.push_str("use crate::Message;\n\n");
    }
    code.push_str("pub use primitive_types::U256;\n\n");

    // Generate validation error enum
    code.push_str(&generate_validation_error(contracts));

    // Generate each message type
    for (name, contract) in &contracts.messages {
        code.push_str(&generate_message_struct(name, contract));
        code.push_str(&generate_message_impl(name, contract));
        code.push_str(&generate_message_trait(name, contract));
    }

    // Generate buffer pool configuration
    code.push_str(&generate_buffer_pool_config(contracts));

    // Generate tests
    code.push_str("\n#[cfg(test)]\n");
    code.push_str("mod generated_tests {\n");
    code.push_str("    use super::*;\n\n");

    for (name, contract) in &contracts.messages {
        code.push_str(&generate_message_tests(name, contract));
    }

    code.push_str("}\n");

    code
}

// ... rest of the codegen functions (keeping the exact same implementation)
// I'll include the full implementation but note this is identical to the original

fn generate_validation_error(_contracts: &Contracts) -> String {
    let mut code = String::new();

    code.push_str("/// Validation errors for message construction and deserialization\n");
    code.push_str("#[derive(Debug, Clone, PartialEq, thiserror::Error)]\n");
    code.push_str("pub enum ValidationError {\n");
    code.push_str("    /// Generic field validation error with message\n");
    code.push_str("    #[error(\"Field validation failed: {0}\")]\n");
    code.push_str("    InvalidField(String),\n");
    code.push_str("}\n\n");

    code
}

fn generate_validate_method(_name: &str, contract: &MessageContract) -> String {
    let mut code = String::new();

    code.push_str("\n    /// Validate message after deserialization\n");
    code.push_str("    ///\n");
    code.push_str("    /// This MUST be called after zerocopy deserialization to ensure\n");
    code.push_str("    /// the message satisfies all invariants, since deserialization\n");
    code.push_str("    /// bypasses the constructor validation.\n");
    code.push_str("    pub fn validate(&self) -> Result<(), ValidationError> {\n");

    // Generate validation logic based on field validation rules from contracts.yaml
    let mut has_validation = false;

    for (field_name, field_contract) in &contract.fields {
        if field_contract.validation.is_empty() {
            continue;
        }

        has_validation = true;

        for validation_rule in &field_contract.validation {
            // Handle not_empty validation for String fields
            if validation_rule.contains_key("not_empty") {
                code.push_str(&format!(
                    "        // Validate {} is not empty\n",
                    field_name
                ));
                code.push_str(&format!(
                    "        let {}_str = self.{}.as_str()\n",
                    field_name, field_name
                ));
                code.push_str(&format!("            .map_err(|_| ValidationError::InvalidField(\"{}\".to_string()))?;\n", field_name));
                code.push_str(&format!("        if {}_str.is_empty() {{\n", field_name));
                code.push_str(&format!("            return Err(ValidationError::InvalidField(\"{} cannot be empty\".to_string()));\n", field_name));
                code.push_str("        }\n\n");
            }

            // Handle min_length validation for Vec fields
            if let Some(min_len) = validation_rule.get("min_length") {
                if let Some(min_val) = min_len.as_u64() {
                    code.push_str(&format!(
                        "        // Validate {} has minimum length\n",
                        field_name
                    ));
                    code.push_str(&format!(
                        "        if self.{}.len() < {} {{\n",
                        field_name, min_val
                    ));
                    code.push_str(&format!(
                        "            return Err(ValidationError::InvalidField(\n"
                    ));
                    code.push_str(&format!("                format!(\"{} length {{}} below minimum {}\", self.{}.len())\n", field_name, min_val, field_name));
                    code.push_str("            ));\n");
                    code.push_str("        }\n\n");
                }
            }

            // Handle max_length validation for Vec fields
            if let Some(max_len) = validation_rule.get("max_length") {
                if let Some(max_val) = max_len.as_u64() {
                    code.push_str(&format!(
                        "        // Validate {} does not exceed maximum length\n",
                        field_name
                    ));
                    code.push_str(&format!(
                        "        if self.{}.len() > {} {{\n",
                        field_name, max_val
                    ));
                    code.push_str(&format!(
                        "            return Err(ValidationError::InvalidField(\n"
                    ));
                    code.push_str(&format!("                format!(\"{} length {{}} exceeds maximum {}\", self.{}.len())\n", field_name, max_val, field_name));
                    code.push_str("            ));\n");
                    code.push_str("        }\n\n");
                }
            }
        }
    }

    if !has_validation {
        code.push_str("        // No validation rules defined in contract\n");
    }

    code.push_str("        Ok(())\n");
    code.push_str("    }\n");

    code
}

fn generate_message_struct(name: &str, contract: &MessageContract) -> String {
    let mut code = String::new();

    // Documentation comment
    code.push_str(&format!(
        "/// {} (TLV type {}, {} domain)\n",
        contract.description, contract.tlv_type, contract.domain
    ));
    code.push_str("///\n");
    code.push_str(&format!("/// Contract: contracts.yaml → {}\n", name));

    if contract.sensitivity.is_some() {
        code.push_str("///\n");
        code.push_str("/// CRITICAL: Contains financially sensitive data - never log payload!\n");
    }

    code.push_str("///\n");
    code.push_str("/// **Note**: U256 values are stored as [u8; 32] for zerocopy compatibility.\n");
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq)]\n");
    code.push_str("#[repr(C)]\n");
    code.push_str(&format!("pub struct {} {{\n", name));

    // Generate fields
    for (field_name, field_contract) in &contract.fields {
        code.push_str(&format!("    /// {}\n", field_contract.description));

        let rust_type = map_contract_type_to_rust(&field_contract.field_type, field_contract);
        code.push_str(&format!("    pub {}: {},\n", field_name, rust_type));
    }

    code.push_str("}\n\n");

    // Manual AsBytes impl (unsafe but simple)
    code.push_str(&format!("unsafe impl zerocopy::AsBytes for {} {{\n", name));
    code.push_str("    fn only_derive_is_allowed_to_implement_this_trait() {}\n");
    code.push_str("}\n\n");

    // Manual FromBytes impl
    code.push_str(&format!(
        "unsafe impl zerocopy::FromBytes for {} {{\n",
        name
    ));
    code.push_str("    fn only_derive_is_allowed_to_implement_this_trait() {}\n");
    code.push_str("}\n\n");

    // Manual FromZeroes impl
    code.push_str(&format!(
        "unsafe impl zerocopy::FromZeroes for {} {{\n",
        name
    ));
    code.push_str("    fn only_derive_is_allowed_to_implement_this_trait() {}\n");
    code.push_str("}\n\n");

    code
}

fn generate_message_impl(name: &str, contract: &MessageContract) -> String {
    let mut code = String::new();

    code.push_str(&format!("impl {} {{\n", name));

    // Generate validate() method for all message types
    code.push_str(&generate_validate_method(name, contract));

    code.push_str("}\n\n");

    code
}

fn generate_message_trait(name: &str, contract: &MessageContract) -> String {
    let mut code = String::new();

    let topic = topic_from_domain(&contract.domain);

    code.push_str(&format!("impl Message for {} {{\n", name));
    code.push_str(&format!(
        "    const TYPE_ID: u16 = {};\n",
        contract.tlv_type
    ));
    code.push_str(&format!("    const TOPIC: &'static str = \"{}\";\n", topic));
    code.push_str("}\n\n");

    code
}

fn generate_message_tests(name: &str, contract: &MessageContract) -> String {
    let mut code = String::new();

    // Generate basic type ID test for all messages
    code.push_str(&format!("    #[test]\n"));
    code.push_str(&format!(
        "    fn test_{}_type_id() {{\n",
        name.to_lowercase()
    ));
    code.push_str(&format!(
        "        assert_eq!({}::TYPE_ID, {});\n",
        name, contract.tlv_type
    ));
    code.push_str("    }\n\n");

    // Generate topic test
    let topic = contract
        .domain
        .to_lowercase()
        .replace("marketdata", "market-data");
    code.push_str(&format!("    #[test]\n"));
    code.push_str(&format!("    fn test_{}_topic() {{\n", name.to_lowercase()));
    code.push_str(&format!(
        "        assert_eq!({}::TOPIC, \"{}\");\n",
        name, topic
    ));
    code.push_str("    }\n\n");

    code
}

fn calculate_message_size(contract: &MessageContract) -> usize {
    let mut total = 0;

    for (_field_name, field_contract) in &contract.fields {
        let field_type = &field_contract.field_type;
        let max_length = |default: usize| -> usize {
            get_max_length_from_validation(field_contract).unwrap_or(default)
        };

        let size = match field_type.as_str() {
            "u8" => 1,
            "u16" => 2,
            "u32" => 4,
            "u64" => 8,
            "i32" => 4,
            "f64" => 8,
            "[u8; 16]" => 16,
            "[u8; 20]" => 20,
            "[u8; 32]" => 32,
            "[u8; 6]" => 6,
            "U256" => 32,           // Stored as [u8; 32]
            "String" => 2 + 6 + 32, // FixedStr<32>: [count: u16][_padding: [u8; 6]][chars: [u8; 32]]
            "Vec<[u8; 20]>" => {
                let max_len = max_length(4);
                2 + 6 + (20 * max_len)
            }
            "Vec<[u8; 32]>" => {
                let max_len = max_length(4);
                2 + 6 + (32 * max_len)
            }
            "Vec<u8>" => {
                let max_len = max_length(16);
                2 + 6 + (1 * max_len)
            }
            "[[u8; 32]; 256]" => 32 * 256, // Fixed array: 256 elements of 32 bytes each
            "[[u8; 32]; 6]" => 32 * 6,
            "[[u8; 20]; 6]" => 20 * 6,
            _ => panic!("Unknown field type for size calculation: {}", field_type),
        };

        total += size;
    }

    total
}

/// Round up to next power of 2, minimum 128 bytes
fn next_power_of_two(n: usize) -> usize {
    if n <= 128 {
        return 128;
    }

    let mut power = 1;
    while power < n {
        power *= 2;
    }
    power
}

/// Generate buffer pool configuration from message definitions
fn generate_buffer_pool_config(contracts: &Contracts) -> String {
    let mut code = String::new();

    // Calculate sizes and group by size class
    let mut size_to_messages: HashMap<usize, Vec<(String, usize)>> = HashMap::new();

    for (name, contract) in &contracts.messages {
        let size = calculate_message_size(contract);
        let buffer_class = next_power_of_two(size);

        size_to_messages
            .entry(buffer_class)
            .or_insert_with(Vec::new)
            .push((name.clone(), size));
    }

    // Generate buffer pool configuration code
    code.push_str("\n");
    code.push_str("// ============================================================\n");
    code.push_str("// Buffer Pool Configuration (Auto-generated)\n");
    code.push_str("// ============================================================\n\n");

    code.push_str("use std::collections::HashMap;\n\n");

    code.push_str("/// Create buffer pool configuration derived from message sizes\n");
    code.push_str("///\n");
    code.push_str("/// Returns HashMap<size_class, capacity> where:\n");
    code.push_str("/// - size_class: Buffer size in bytes (power of 2, min 128)\n");
    code.push_str("/// - capacity: Max number of buffers to pool for this size class\n");
    code.push_str("///\n");
    code.push_str(
        "/// Capacity heuristic: 64KB per size class (e.g., 128-byte buffers get 512 capacity)\n",
    );
    code.push_str("pub fn create_buffer_pool_config() -> HashMap<usize, usize> {\n");
    code.push_str("    let mut config = HashMap::new();\n\n");

    // Sort by size class for predictable output
    let mut size_classes: Vec<_> = size_to_messages.keys().collect();
    size_classes.sort();

    for size_class in size_classes {
        let messages = &size_to_messages[size_class];

        // Generate capacity heuristic: 64KB per size class, divided by buffer size
        let capacity = 65536 / size_class;

        // Build comment with message names and actual sizes
        let msg_list: Vec<String> = messages
            .iter()
            .map(|(name, actual_size)| format!("{} ({}B)", name, actual_size))
            .collect();
        let comment = msg_list.join(", ");

        code.push_str(&format!("    // {}\n", comment));
        code.push_str(&format!(
            "    config.insert({}, {});\n\n",
            size_class, capacity
        ));
    }

    code.push_str("    config\n");
    code.push_str("}\n\n");

    // Generate per-message size constants for debugging/validation
    code.push_str("// Per-message size constants for debugging and validation\n");

    // Sort messages by name for predictable output
    let mut sorted_messages: Vec<_> = contracts.messages.iter().collect();
    sorted_messages.sort_by_key(|(name, _)| *name);

    for (name, contract) in sorted_messages {
        let size = calculate_message_size(contract);
        let buffer_class = next_power_of_two(size);

        let name_upper = name.to_uppercase();
        code.push_str(&format!(
            "pub const {}_MAX_SIZE: usize = {};\n",
            name_upper, size
        ));
        code.push_str(&format!(
            "pub const {}_BUFFER_CLASS: usize = {};\n",
            name_upper, buffer_class
        ));
    }

    code.push_str("\n");

    code
}

/// Extract max_length validation from field contract
fn get_max_length_from_validation(field: &FieldContract) -> Option<usize> {
    for validation_rule in &field.validation {
        if let Some(max_len) = validation_rule.get("max_length") {
            if let Some(val) = max_len.as_u64() {
                return Some(val as usize);
            }
        }
    }
    None
}

fn map_contract_type_to_rust(contract_type: &str, field: &FieldContract) -> String {
    match contract_type {
        "u8" => "u8".to_string(),
        "u16" => "u16".to_string(),
        "u32" => "u32".to_string(),
        "u64" => "u64".to_string(),
        "i32" => "i32".to_string(),
        "f64" => "f64".to_string(),
        "[u8; 16]" => "[u8; 16]".to_string(), // For u128 values
        "[u8; 20]" => "[u8; 20]".to_string(),
        "[u8; 32]" => "[u8; 32]".to_string(), // For hashes and raw bytes
        "[u8; 6]" => "[u8; 6]".to_string(),
        "U256" => "[u8; 32]".to_string(), // U256 stored as bytes for zerocopy
        "String" => {
            let max_len = get_max_length_from_validation(field).unwrap_or(32);
            format!("FixedStr<{}>", max_len)
        }
        "Vec<[u8; 20]>" => {
            let max_len = get_max_length_from_validation(field).unwrap_or(4);
            format!("FixedVec<[u8; 20], {}>", max_len)
        }
        "Vec<[u8; 32]>" => {
            let max_len = get_max_length_from_validation(field).unwrap_or(4);
            format!("FixedVec<[u8; 32], {}>", max_len)
        }
        "Vec<u8>" => {
            let max_len = get_max_length_from_validation(field).unwrap_or(16);
            format!("FixedVec<u8, {}>", max_len)
        }
        // Support for fixed-size arrays of byte arrays
        s if s.starts_with("[[u8;") && s.ends_with("]") => contract_type.to_string(),
        _ => panic!("Unknown contract type: {}", contract_type),
    }
}

fn topic_from_domain(domain: &str) -> String {
    domain.to_lowercase().replace("marketdata", "market-data")
}

struct PythonFieldSpec {
    type_hint: String,
    field_expr: String,
}

fn generate_python_module(contracts: &Contracts) -> String {
    let mut code = String::new();

    code.push_str("\"\"\"AUTO-GENERATED from contracts.yaml - DO NOT EDIT\n");
    code.push_str("Generated by mycelium-protocol codegen.\n");
    code.push_str("Edit contracts.yaml and rerun code generation.\n\"\"\"\n\n");
    code.push_str("from __future__ import annotations\n\n");
    code.push_str("from dataclasses import dataclass\n");
    code.push_str("from typing import ClassVar, List\n\n");
    code.push_str("from mycelium.protocol.runtime import (\n");
    code.push_str("    FieldDef,\n");
    code.push_str("    FixedBytes,\n");
    code.push_str("    FixedStr,\n");
    code.push_str("    FixedVecBytes,\n");
    code.push_str("    MessageDef,\n");
    code.push_str("    Primitive,\n");
    code.push_str("    register_message,\n");
    code.push_str(")\n\n");

    let mut sorted: Vec<_> = contracts.messages.iter().collect();
    sorted.sort_by_key(|(name, _)| *name);

    code.push_str("__all__ = [");
    for (idx, (name, _)) in sorted.iter().enumerate() {
        if idx > 0 {
            code.push_str(", ");
        }
        code.push_str(&format!("\"{}\"", name));
    }
    code.push_str("]\n\n");

    for (name, contract) in sorted {
        generate_python_class(&mut code, name, contract);
    }

    code
}

fn generate_python_class(code: &mut String, name: &str, contract: &MessageContract) {
    let topic = topic_from_domain(&contract.domain);
    let mut field_specs = Vec::new();

    code.push_str("@dataclass\n");
    code.push_str(&format!("class {}:\n", name));
    code.push_str(&format!("    \"\"\"{} (domain: {})\"\"\"\n\n", contract.description, contract.domain));
    code.push_str(&format!("    TYPE_ID: ClassVar[int] = {}\n", contract.tlv_type));
    code.push_str(&format!("    TOPIC: ClassVar[str] = \"{}\"\n", topic));
    code.push_str("    __definition__: ClassVar[MessageDef]\n\n");

    for (field_name, field_contract) in &contract.fields {
        let spec = map_contract_type_to_python(&field_contract.field_type, field_contract);
        code.push_str(&format!("    {}: {}\n", field_name, spec.type_hint));
        field_specs.push((field_name, spec));
    }

    if !contract.fields.is_empty() {
        code.push_str("\n");
    }

    code.push_str("    def to_bytes(self) -> bytes:\n");
    code.push_str("        return self.__definition__.encode(self)\n\n");
    code.push_str("    @classmethod\n");
    code.push_str(&format!(
        "    def from_bytes(cls, payload: bytes) -> \"{}\":\n",
        name
    ));
    code.push_str("        return cls.__definition__.decode(cls, payload)\n\n");

    code.push_str(&format!("{}.__definition__ = MessageDef(\n", name));
    code.push_str(&format!("    name=\"{}\",\n", name));
    code.push_str(&format!("    type_id={},\n", contract.tlv_type));
    code.push_str(&format!("    topic=\"{}\",\n", topic));
    code.push_str("    fields=[\n");
    if field_specs.is_empty() {
        code.push_str("    ],\n");
    } else {
        for (field_name, spec) in field_specs {
            code.push_str(&format!(
                "        FieldDef(\"{}\", {}),\n",
                field_name, spec.field_expr
            ));
        }
        code.push_str("    ],\n");
    }
    code.push_str(&format!("    dataclass={},\n", name));
    code.push_str(")\n");
    code.push_str(&format!("register_message({}.__definition__)\n\n", name));
}

fn map_contract_type_to_python(contract_type: &str, field: &FieldContract) -> PythonFieldSpec {
    let normalized = contract_type.replace(' ', "");
    match normalized.as_str() {
        "u8" => PythonFieldSpec {
            type_hint: "int".to_string(),
            field_expr: "Primitive.U8".to_string(),
        },
        "u16" => PythonFieldSpec {
            type_hint: "int".to_string(),
            field_expr: "Primitive.U16".to_string(),
        },
        "u32" => PythonFieldSpec {
            type_hint: "int".to_string(),
            field_expr: "Primitive.U32".to_string(),
        },
        "u64" => PythonFieldSpec {
            type_hint: "int".to_string(),
            field_expr: "Primitive.U64".to_string(),
        },
        "i32" => PythonFieldSpec {
            type_hint: "int".to_string(),
            field_expr: "Primitive.I32".to_string(),
        },
        "f64" => PythonFieldSpec {
            type_hint: "float".to_string(),
            field_expr: "Primitive.F64".to_string(),
        },
        "U256" => PythonFieldSpec {
            type_hint: "bytes".to_string(),
            field_expr: "FixedBytes(32)".to_string(),
        },
        "String" => {
            let max_len = get_max_length_from_validation(field).unwrap_or(32);
            PythonFieldSpec {
                type_hint: "str".to_string(),
                field_expr: format!("FixedStr({})", max_len),
            }
        }
        "Vec<u8>" => {
            let capacity = get_max_length_from_validation(field).unwrap_or(16);
            PythonFieldSpec {
                type_hint: "List[int]".to_string(),
                field_expr: format!("FixedVecBytes(element_size=1, capacity={})", capacity),
            }
        }
        other => {
            if let Some(len) = parse_fixed_u8_array(other) {
                return PythonFieldSpec {
                    type_hint: "bytes".to_string(),
                    field_expr: format!("FixedBytes({})", len),
                };
            }
            if let Some(element_len) = parse_vec_fixed_u8_array(other) {
                let capacity = get_max_length_from_validation(field).unwrap_or(4);
                return PythonFieldSpec {
                    type_hint: "List[bytes]".to_string(),
                    field_expr: format!(
                        "FixedVecBytes(element_size={}, capacity={})",
                        element_len, capacity
                    ),
                };
            }
            panic!("Unknown contract type: {}", contract_type);
        }
    }
}

fn parse_fixed_u8_array(spec: &str) -> Option<usize> {
    let trimmed = spec.trim();
    if trimmed.starts_with("[u8;") && trimmed.ends_with(']') {
        let inner = &trimmed[4..trimmed.len() - 1];
        let len_str = inner.trim_matches(|c: char| c == ' ');
        return len_str.parse::<usize>().ok();
    }
    None
}

fn parse_vec_fixed_u8_array(spec: &str) -> Option<usize> {
    if spec.starts_with("Vec<[u8;") && spec.ends_with("]>") {
        let inner = &spec[9..spec.len() - 2];
        return inner.parse::<usize>().ok();
    }
    None
}
