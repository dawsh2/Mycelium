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
pub fn generate_from_yaml<P: AsRef<Path>>(
    yaml_path: P,
    output_path: P,
) -> Result<(), CodegenError> {
    // Read and parse YAML
    let yaml_content = fs::read_to_string(yaml_path.as_ref())?;
    let contracts: Contracts = serde_yaml::from_str(&yaml_content)?;

    // Validate tag ranges
    validate_tag_ranges(&contracts)?;

    // Generate code (always use crate:: prefix since this is internal)
    let generated_code = generate_messages(&contracts);

    // Write output
    fs::write(output_path.as_ref(), generated_code)?;

    Ok(())
}

/// Contract definition from contracts.yaml
#[derive(Debug, Deserialize)]
struct Contracts {
    #[serde(default)]
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

fn generate_messages(contracts: &Contracts) -> String {
    let mut code = String::new();

    // Add header
    code.push_str("// AUTO-GENERATED from contracts.yaml - DO NOT EDIT MANUALLY\n");
    code.push_str("//\n");
    code.push_str("// This file is generated at build time by build.rs\n");
    code.push_str("// To modify, edit contracts.yaml and rebuild\n\n");

    code.push_str(
        "use crate::fixed_vec::{FixedStr, FixedVec};\n",
    );
    code.push_str("use crate::Message;\n\n");
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

    code.push_str("/// Validation errors for message construction\n");
    code.push_str("#[derive(Debug, Clone, PartialEq, thiserror::Error)]\n");
    code.push_str("pub enum ValidationError {\n");
    code.push_str("    #[error(\"Symbol cannot be empty\")]\n");
    code.push_str("    EmptySymbol,\n\n");
    code.push_str("    #[error(\"Symbol starts with '0x' (indicates RPC error)\")]\n");
    code.push_str("    InvalidSymbolPrefix,\n\n");
    code.push_str("    #[error(\"Symbol too long: {0} chars (max 32)\")]\n");
    code.push_str("    SymbolTooLong(usize),\n\n");
    code.push_str("    #[error(\"Invalid decimals: {0} (must be 1-30)\")]\n");
    code.push_str("    InvalidDecimals(u8),\n\n");
    code.push_str("    #[error(\"Zero address not allowed\")]\n");
    code.push_str("    ZeroAddress,\n\n");
    code.push_str("    #[error(\"Arbitrage path too short: {0} pools (min 2)\")]\n");
    code.push_str("    PathTooShort(usize),\n\n");
    code.push_str("    #[error(\"Arbitrage path too long: {0} pools (max 4)\")]\n");
    code.push_str("    PathTooLong(usize),\n\n");
    code.push_str("    #[error(\"Negative profit not allowed: {0}\")]\n");
    code.push_str("    NegativeProfit(f64),\n");
    code.push_str("}\n\n");

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
        let visibility = if rust_type.contains("[u8; 32]") {
            ""
        } else {
            "pub "
        };

        code.push_str(&format!(
            "    {}{}: {},\n",
            visibility, field_name, rust_type
        ));
    }

    code.push_str("}\n\n");

    // Manual TryFromBytes/IntoBytes impls (unsafe but simple)
    code.push_str(&format!("unsafe impl zerocopy::TryFromBytes for {} {{}}\n", name));

    code.push_str(&format!("unsafe impl zerocopy::IntoBytes for {} {{\n", name));
    code.push_str("    fn only_derive_is_allowed_to_implement_this_trait() {}\n");
    code.push_str("}\n\n");

    // Manual FromBytes impl
    code.push_str(&format!(
        "unsafe impl zerocopy::FromBytes for {} {{\n",
        name
    ));
    code.push_str("    fn only_derive_is_allowed_to_implement_this_trait() {}\n");
    code.push_str("}\n\n");

    // Manual FromZeros impl
    code.push_str(&format!(
        "unsafe impl zerocopy::FromZeros for {} {{\n",
        name
    ));
    code.push_str("    fn only_derive_is_allowed_to_implement_this_trait() {}\n");
    code.push_str("}\n\n");

    // Manual Immutable impl (struct contains only Copy fields)
    code.push_str(&format!("unsafe impl zerocopy::Immutable for {} {{}}\n\n", name));

    code
}

fn generate_message_impl(name: &str, contract: &MessageContract) -> String {
    let mut code = String::new();

    code.push_str(&format!("impl {} {{\n", name));

    // Generate constructor based on message type
    match name {
        "InstrumentMeta" => {
            code.push_str(&generate_instrument_meta_impl());
        }
        "PoolStateUpdate" => {
            code.push_str(&generate_pool_state_impl());
        }
        "ArbitrageSignal" => {
            code.push_str(&generate_arbitrage_signal_impl(contract));
        }
        _ => {
            // Generic constructor for unknown types
            code.push_str("    // TODO: Generate constructor\n");
        }
    }

    code.push_str("}\n\n");

    code
}

fn generate_instrument_meta_impl() -> String {
    let mut code = String::new();

    code.push_str("    /// Create new InstrumentMeta with validation\n");
    code.push_str("    pub fn new(\n");
    code.push_str("        token_address: [u8; 20],\n");
    code.push_str("        symbol: &str,\n");
    code.push_str("        decimals: u8,\n");
    code.push_str("        chain_id: u32,\n");
    code.push_str("    ) -> Result<Self, ValidationError> {\n");
    code.push_str("        // Validate symbol\n");
    code.push_str("        if symbol.is_empty() {\n");
    code.push_str("            return Err(ValidationError::EmptySymbol);\n");
    code.push_str("        }\n");
    code.push_str("        if symbol.starts_with(\"0x\") {\n");
    code.push_str("            return Err(ValidationError::InvalidSymbolPrefix);\n");
    code.push_str("        }\n\n");
    code.push_str("        let symbol_fixed = FixedStr::from_str(symbol)\n");
    code.push_str("            .map_err(|_| ValidationError::SymbolTooLong(symbol.len()))?;\n\n");
    code.push_str("        // Validate decimals\n");
    code.push_str("        if decimals == 0 || decimals > 30 {\n");
    code.push_str("            return Err(ValidationError::InvalidDecimals(decimals));\n");
    code.push_str("        }\n\n");
    code.push_str("        // Validate address is not zero\n");
    code.push_str("        if token_address == [0u8; 20] {\n");
    code.push_str("            return Err(ValidationError::ZeroAddress);\n");
    code.push_str("        }\n\n");
    code.push_str("        Ok(Self {\n");
    code.push_str("            token_address,\n");
    code.push_str("            symbol: symbol_fixed,\n");
    code.push_str("            decimals,\n");
    code.push_str("            chain_id,\n");
    code.push_str("        })\n");
    code.push_str("    }\n\n");
    code.push_str("    /// Get symbol as string\n");
    code.push_str("    pub fn symbol_str(&self) -> &str {\n");
    code.push_str("        self.symbol.as_str().unwrap_or(\"<invalid>\")\n");
    code.push_str("    }\n");

    code
}

fn generate_pool_state_impl() -> String {
    let mut code = String::new();

    // new_v2 constructor
    code.push_str("    /// Create new V2 pool state\n");
    code.push_str("    pub fn new_v2(\n");
    code.push_str("        pool_address: [u8; 20],\n");
    code.push_str("        venue_id: u16,\n");
    code.push_str("        reserve0: U256,\n");
    code.push_str("        reserve1: U256,\n");
    code.push_str("        block_number: u64,\n");
    code.push_str("    ) -> Result<Self, ValidationError> {\n");
    code.push_str("        if pool_address == [0u8; 20] {\n");
    code.push_str("            return Err(ValidationError::ZeroAddress);\n");
    code.push_str("        }\n\n");
    code.push_str("        let mut reserve0_bytes = [0u8; 32];\n");
    code.push_str("        reserve0.to_big_endian(&mut reserve0_bytes);\n\n");
    code.push_str("        let mut reserve1_bytes = [0u8; 32];\n");
    code.push_str("        reserve1.to_big_endian(&mut reserve1_bytes);\n\n");
    code.push_str("        Ok(Self {\n");
    code.push_str("            pool_address,\n");
    code.push_str("            venue_id,\n");
    code.push_str("            reserve0: reserve0_bytes,\n");
    code.push_str("            reserve1: reserve1_bytes,\n");
    code.push_str("            liquidity: [0; 32],\n");
    code.push_str("            sqrt_price_x96: [0; 32],\n");
    code.push_str("            tick: 0,\n");
    code.push_str("            block_number,\n");
    code.push_str("        })\n");
    code.push_str("    }\n\n");

    // new_v3 constructor
    code.push_str("    /// Create new V3 pool state\n");
    code.push_str("    pub fn new_v3(\n");
    code.push_str("        pool_address: [u8; 20],\n");
    code.push_str("        venue_id: u16,\n");
    code.push_str("        liquidity: U256,\n");
    code.push_str("        sqrt_price_x96: U256,\n");
    code.push_str("        tick: i32,\n");
    code.push_str("        block_number: u64,\n");
    code.push_str("    ) -> Result<Self, ValidationError> {\n");
    code.push_str("        if pool_address == [0u8; 20] {\n");
    code.push_str("            return Err(ValidationError::ZeroAddress);\n");
    code.push_str("        }\n\n");
    code.push_str("        let mut liquidity_bytes = [0u8; 32];\n");
    code.push_str("        liquidity.to_big_endian(&mut liquidity_bytes);\n\n");
    code.push_str("        let mut sqrt_price_bytes = [0u8; 32];\n");
    code.push_str("        sqrt_price_x96.to_big_endian(&mut sqrt_price_bytes);\n\n");
    code.push_str("        Ok(Self {\n");
    code.push_str("            pool_address,\n");
    code.push_str("            venue_id,\n");
    code.push_str("            reserve0: [0; 32],\n");
    code.push_str("            reserve1: [0; 32],\n");
    code.push_str("            liquidity: liquidity_bytes,\n");
    code.push_str("            sqrt_price_x96: sqrt_price_bytes,\n");
    code.push_str("            tick,\n");
    code.push_str("            block_number,\n");
    code.push_str("        })\n");
    code.push_str("    }\n\n");

    // Getters
    code.push_str("    /// Get reserve0 as U256 (V2 pools)\n");
    code.push_str("    pub fn reserve0(&self) -> U256 {\n");
    code.push_str("        U256::from_big_endian(&self.reserve0)\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Get reserve1 as U256 (V2 pools)\n");
    code.push_str("    pub fn reserve1(&self) -> U256 {\n");
    code.push_str("        U256::from_big_endian(&self.reserve1)\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Get liquidity as U256 (V3 pools)\n");
    code.push_str("    pub fn liquidity(&self) -> U256 {\n");
    code.push_str("        U256::from_big_endian(&self.liquidity)\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Get sqrt_price_x96 as U256 (V3 pools)\n");
    code.push_str("    pub fn sqrt_price_x96(&self) -> U256 {\n");
    code.push_str("        U256::from_big_endian(&self.sqrt_price_x96)\n");
    code.push_str("    }\n\n");

    // Helper methods
    code.push_str("    /// Check if this is a V2 pool\n");
    code.push_str("    pub fn is_v2(&self) -> bool {\n");
    code.push_str("        self.reserve0 != [0; 32] || self.reserve1 != [0; 32]\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Check if this is a V3 pool\n");
    code.push_str("    pub fn is_v3(&self) -> bool {\n");
    code.push_str("        self.liquidity != [0; 32] || self.sqrt_price_x96 != [0; 32]\n");
    code.push_str("    }\n");

    code
}

fn generate_arbitrage_signal_impl(contract: &MessageContract) -> String {
    let mut code = String::new();

    // Extract max_length from path field validation
    let max_pool_addresses = contract
        .fields
        .get("path")
        .and_then(|field| get_max_length_from_validation(field))
        .unwrap_or(4);

    code.push_str("    /// Create new arbitrage signal with validation\n");
    code.push_str("    pub fn new(\n");
    code.push_str("        opportunity_id: u64,\n");
    code.push_str("        path: &[[u8; 20]],\n");
    code.push_str("        estimated_profit_usd: f64,\n");
    code.push_str("        gas_estimate_wei: U256,\n");
    code.push_str("        deadline_block: u64,\n");
    code.push_str("    ) -> Result<Self, ValidationError> {\n");
    code.push_str(&format!("        // Validate path length (2-{} pools)\n", max_pool_addresses));
    code.push_str("        if path.len() < 2 {\n");
    code.push_str("            return Err(ValidationError::PathTooShort(path.len()));\n");
    code.push_str("        }\n");
    code.push_str(&format!("        if path.len() > {} {{\n", max_pool_addresses));
    code.push_str("            return Err(ValidationError::PathTooLong(path.len()));\n");
    code.push_str("        }\n\n");
    code.push_str("        // Validate profit is non-negative\n");
    code.push_str("        if estimated_profit_usd < 0.0 {\n");
    code.push_str(
        "            return Err(ValidationError::NegativeProfit(estimated_profit_usd));\n",
    );
    code.push_str("        }\n\n");
    code.push_str("        let path_fixed = FixedVec::from_slice(path)\n");
    code.push_str("            .map_err(|_| ValidationError::PathTooLong(path.len()))?;\n\n");
    code.push_str("        let mut gas_bytes = [0u8; 32];\n");
    code.push_str("        gas_estimate_wei.to_big_endian(&mut gas_bytes);\n\n");
    code.push_str("        Ok(Self {\n");
    code.push_str("            opportunity_id,\n");
    code.push_str("            path: path_fixed,\n");
    code.push_str("            estimated_profit_usd,\n");
    code.push_str("            gas_estimate_wei: gas_bytes,\n");
    code.push_str("            deadline_block,\n");
    code.push_str("        })\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Get gas estimate as U256\n");
    code.push_str("    pub fn gas_estimate_wei(&self) -> U256 {\n");
    code.push_str("        U256::from_big_endian(&self.gas_estimate_wei)\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Get path as slice\n");
    code.push_str("    pub fn path_slice(&self) -> &[[u8; 20]] {\n");
    code.push_str("        self.path.as_slice()\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Number of hops in the arbitrage path\n");
    code.push_str("    pub fn hop_count(&self) -> usize {\n");
    code.push_str("        self.path.len()\n");
    code.push_str("    }\n");

    code
}

fn generate_message_trait(name: &str, contract: &MessageContract) -> String {
    let mut code = String::new();

    let topic = contract
        .domain
        .to_lowercase()
        .replace("marketdata", "market-data");

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

    match name {
        "InstrumentMeta" => code.push_str(&generate_instrument_meta_tests()),
        "PoolStateUpdate" => code.push_str(&generate_pool_state_tests()),
        "ArbitrageSignal" => code.push_str(&generate_arbitrage_signal_tests()),
        _ => {
            // Generic test for unknown types
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
        }
    }

    code
}

fn generate_instrument_meta_tests() -> String {
    let mut code = String::new();

    // Test creation
    code.push_str("    #[test]\n");
    code.push_str("    fn test_instrument_meta_creation() {\n");
    code.push_str("        let meta = InstrumentMeta::new([1; 20], \"WETH\", 18, 137).unwrap();\n");
    code.push_str("        assert_eq!(meta.symbol_str(), \"WETH\");\n");
    code.push_str("        assert_eq!(meta.decimals, 18);\n");
    code.push_str("        assert_eq!(meta.chain_id, 137);\n");
    code.push_str("        assert_eq!(InstrumentMeta::TYPE_ID, 1018);\n");
    code.push_str("    }\n\n");

    // Test validation
    code.push_str("    #[test]\n");
    code.push_str("    fn test_instrument_meta_validation() {\n");
    code.push_str("        assert!(matches!(InstrumentMeta::new([1; 20], \"\", 18, 137), Err(ValidationError::EmptySymbol)));\n");
    code.push_str("        assert!(matches!(InstrumentMeta::new([1; 20], \"0xWETH\", 18, 137), Err(ValidationError::InvalidSymbolPrefix)));\n");
    code.push_str("        assert!(matches!(InstrumentMeta::new([1; 20], \"WETH\", 0, 137), Err(ValidationError::InvalidDecimals(0))));\n");
    code.push_str("        assert!(matches!(InstrumentMeta::new([0; 20], \"WETH\", 18, 137), Err(ValidationError::ZeroAddress)));\n");
    code.push_str("    }\n\n");

    // Test zerocopy roundtrip
    code.push_str("    #[test]\n");
    code.push_str("    fn test_instrument_meta_zerocopy() {\n");
    code.push_str("        use zerocopy::{IntoBytes, FromBytes, Immutable};\n");
    code.push_str(
        "        let original = InstrumentMeta::new([1; 20], \"USDC\", 6, 137).unwrap();\n",
    );
    code.push_str("        let bytes = original.as_bytes();\n");
    code.push_str("        let deserialized = InstrumentMeta::ref_from(bytes).unwrap();\n");
    code.push_str("        assert_eq!(deserialized.symbol_str(), \"USDC\");\n");
    code.push_str("        assert_eq!(deserialized.decimals, 6);\n");
    code.push_str("    }\n\n");

    code
}

fn generate_pool_state_tests() -> String {
    let mut code = String::new();

    // Test V2 creation
    code.push_str("    #[test]\n");
    code.push_str("    fn test_pool_state_v2() {\n");
    code.push_str("        let state = PoolStateUpdate::new_v2([1; 20], 1, U256::from(1000), U256::from(2000), 12345).unwrap();\n");
    code.push_str("        assert!(state.is_v2());\n");
    code.push_str("        assert!(!state.is_v3());\n");
    code.push_str("        assert_eq!(state.reserve0(), U256::from(1000));\n");
    code.push_str("        assert_eq!(state.reserve1(), U256::from(2000));\n");
    code.push_str("    }\n\n");

    // Test V3 creation
    code.push_str("    #[test]\n");
    code.push_str("    fn test_pool_state_v3() {\n");
    code.push_str("        let state = PoolStateUpdate::new_v3([1; 20], 2, U256::from(50000), U256::from(1234567890), 100, 12345).unwrap();\n");
    code.push_str("        assert!(!state.is_v2());\n");
    code.push_str("        assert!(state.is_v3());\n");
    code.push_str("        assert_eq!(state.liquidity(), U256::from(50000));\n");
    code.push_str("        assert_eq!(state.sqrt_price_x96(), U256::from(1234567890));\n");
    code.push_str("    }\n\n");

    // Test zerocopy roundtrip
    code.push_str("    #[test]\n");
    code.push_str("    fn test_pool_state_zerocopy() {\n");
    code.push_str("        use zerocopy::{IntoBytes, FromBytes, Immutable};\n");
    code.push_str("        let original = PoolStateUpdate::new_v2([5; 20], 1, U256::from(1000000), U256::from(2000000), 54321).unwrap();\n");
    code.push_str("        let bytes = original.as_bytes();\n");
    code.push_str("        let deserialized = PoolStateUpdate::ref_from(bytes).unwrap();\n");
    code.push_str("        assert_eq!(deserialized.reserve0(), U256::from(1000000));\n");
    code.push_str("        assert_eq!(deserialized.block_number, 54321);\n");
    code.push_str("    }\n\n");

    code
}

fn generate_arbitrage_signal_tests() -> String {
    let mut code = String::new();

    // Test creation
    code.push_str("    #[test]\n");
    code.push_str("    fn test_arbitrage_signal_creation() {\n");
    code.push_str("        let path = [[1; 20], [2; 20], [3; 20]];\n");
    code.push_str("        let signal = ArbitrageSignal::new(123, &path, 100.5, U256::from(21000), 67890).unwrap();\n");
    code.push_str("        assert_eq!(signal.opportunity_id, 123);\n");
    code.push_str("        assert_eq!(signal.hop_count(), 3);\n");
    code.push_str("        assert_eq!(signal.estimated_profit_usd, 100.5);\n");
    code.push_str("    }\n\n");

    // Test validation
    code.push_str("    #[test]\n");
    code.push_str("    fn test_arbitrage_signal_validation() {\n");
    code.push_str("        let path_short = [[1; 20]];\n");
    code.push_str("        assert!(matches!(ArbitrageSignal::new(1, &path_short, 100.0, U256::zero(), 1000), Err(ValidationError::PathTooShort(1))));\n");
    code.push_str("        let path_long = [[1; 20], [2; 20], [3; 20], [4; 20], [5; 20]];\n");
    code.push_str("        assert!(matches!(ArbitrageSignal::new(1, &path_long, 100.0, U256::zero(), 1000), Err(ValidationError::PathTooLong(5))));\n");
    code.push_str("        let path_ok = [[1; 20], [2; 20]];\n");
    code.push_str("        assert!(matches!(ArbitrageSignal::new(1, &path_ok, -10.0, U256::zero(), 1000), Err(ValidationError::NegativeProfit(_))));\n");
    code.push_str("    }\n\n");

    // Test zerocopy roundtrip
    code.push_str("    #[test]\n");
    code.push_str("    fn test_arbitrage_signal_zerocopy() {\n");
    code.push_str("        use zerocopy::{IntoBytes, FromBytes, Immutable};\n");
    code.push_str("        let path = [[1; 20], [2; 20], [3; 20]];\n");
    code.push_str("        let original = ArbitrageSignal::new(999, &path, 250.75, U256::from(42000), 99999).unwrap();\n");
    code.push_str("        let bytes = original.as_bytes();\n");
    code.push_str("        let deserialized = ArbitrageSignal::ref_from(bytes).unwrap();\n");
    code.push_str("        assert_eq!(deserialized.opportunity_id, 999);\n");
    code.push_str("        assert_eq!(deserialized.hop_count(), 3);\n");
    code.push_str("    }\n\n");

    code
}

/// Calculate the total serialized size of a message in bytes
fn calculate_message_size(contract: &MessageContract) -> usize {
    let mut total = 0;

    for (_field_name, field_contract) in &contract.fields {
        let field_type = &field_contract.field_type;

        let size = match field_type.as_str() {
            "u8" => 1,
            "u16" => 2,
            "u32" => 4,
            "u64" => 8,
            "i32" => 4,
            "f64" => 8,
            "[u8; 20]" => 20,
            "U256" => 32,                        // Stored as [u8; 32]
            "String" => 2 + 6 + 32, // FixedStr<32>: [count: u16][_padding: [u8; 6]][chars: [u8; 32]]
            "Vec<[u8; 20]>" => 2 + 6 + (20 * 4), // FixedVec<[u8; 20], 4>: [count: u16][_padding: [u8; 6]][elements: [[u8; 20]; 4]]
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
        "[u8; 20]" => "[u8; 20]".to_string(),
        "U256" => "[u8; 32]".to_string(), // U256 stored as bytes for zerocopy
        "String" => {
            let max_len = get_max_length_from_validation(field).unwrap_or(32);
            format!("FixedStr<{}>", max_len)
        },
        "Vec<[u8; 20]>" => {
            let max_len = get_max_length_from_validation(field).unwrap_or(4);
            format!("FixedVec<[u8; 20], {}>", max_len)
        },
        _ => panic!("Unknown contract type: {}", contract_type),
    }
}
