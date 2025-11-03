//! Core TLV message types
//!
//! Implements the message contracts defined in contracts.yaml.
//! All types use rkyv for zero-copy serialization.

use crate::fixed_vec::{FixedStr, FixedVec, MAX_POOL_ADDRESSES, MAX_SYMBOL_LENGTH};
use crate::Message;
use rkyv::{Archive, Deserialize, Serialize};

// Re-export U256 for public API
pub use primitive_types::U256;

/// Token metadata (TLV type 18, MarketData domain)
///
/// Contains essential token information needed before processing pool updates.
/// Must be received before PoolStateUpdate for the same token.
///
/// Contract: contracts.yaml → InstrumentMeta
#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
#[archive(check_bytes)]
#[repr(C)]
pub struct InstrumentMeta {
    /// ERC-20 token address (20 bytes)
    pub token_address: [u8; 20],

    /// Token symbol (e.g., "WETH", "USDC")
    /// Max 32 characters, no "0x" prefix (indicates RPC error)
    pub symbol: FixedStr<MAX_SYMBOL_LENGTH>,

    /// Token decimals (1-30, typically 6 or 18)
    pub decimals: u8,

    /// Chain ID (e.g., 137 for Polygon)
    pub chain_id: u32,

    /// Padding for alignment
    _padding: [u8; 3],
}

impl InstrumentMeta {
    /// Create new InstrumentMeta with validation
    pub fn new(
        token_address: [u8; 20],
        symbol: &str,
        decimals: u8,
        chain_id: u32,
    ) -> Result<Self, ValidationError> {
        // Validate symbol
        if symbol.is_empty() {
            return Err(ValidationError::EmptySymbol);
        }
        if symbol.starts_with("0x") {
            return Err(ValidationError::InvalidSymbolPrefix);
        }

        let symbol_fixed = FixedStr::from_str(symbol)
            .map_err(|_| ValidationError::SymbolTooLong(symbol.len()))?;

        // Validate decimals
        if decimals == 0 || decimals > 30 {
            return Err(ValidationError::InvalidDecimals(decimals));
        }

        // Validate address is not zero
        if token_address == [0u8; 20] {
            return Err(ValidationError::ZeroAddress);
        }

        Ok(Self {
            token_address,
            symbol: symbol_fixed,
            decimals,
            chain_id,
            _padding: [0; 3],
        })
    }

    /// Get symbol as string
    pub fn symbol_str(&self) -> &str {
        self.symbol.as_str().unwrap_or("<invalid>")
    }
}

impl Message for InstrumentMeta {
    const TYPE_ID: u16 = 18;
    const TOPIC: &'static str = "market-data";
}

/// DEX pool state update (TLV type 11, MarketData domain)
///
/// Represents current reserves/liquidity for a DEX pool.
/// Supports both V2 (constant product) and V3 (concentrated liquidity).
///
/// Contract: contracts.yaml → PoolStateUpdate
///
/// **Note**: U256 values are stored as [u8; 32] for rkyv compatibility.
/// Use getter/setter methods or conversion helpers.
#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
#[archive(check_bytes)]
#[repr(C)]
pub struct PoolStateUpdate {
    /// Pool contract address
    pub pool_address: [u8; 20],

    /// Protocol identifier (1=Uniswap V2, 2=Uniswap V3, etc.)
    pub venue_id: u16,

    /// Reserve of token0 (V2 only, zero for V3) - stored as big-endian bytes
    reserve0: [u8; 32],

    /// Reserve of token1 (V2 only, zero for V3) - stored as big-endian bytes
    reserve1: [u8; 32],

    /// Total liquidity (V3 only, zero for V2) - stored as big-endian bytes
    liquidity: [u8; 32],

    /// Square root price in Q96 format (V3 only, zero for V2) - stored as big-endian bytes
    sqrt_price_x96: [u8; 32],

    /// Current tick (V3 only, zero for V2)
    pub tick: i32,

    /// Block number when state was observed
    pub block_number: u64,

    /// Padding for alignment
    _padding: [u8; 4],
}

impl PoolStateUpdate {
    /// Create new V2 pool state
    pub fn new_v2(
        pool_address: [u8; 20],
        venue_id: u16,
        reserve0: U256,
        reserve1: U256,
        block_number: u64,
    ) -> Result<Self, ValidationError> {
        if pool_address == [0u8; 20] {
            return Err(ValidationError::ZeroAddress);
        }

        let mut reserve0_bytes = [0u8; 32];
        reserve0.to_big_endian(&mut reserve0_bytes);

        let mut reserve1_bytes = [0u8; 32];
        reserve1.to_big_endian(&mut reserve1_bytes);

        Ok(Self {
            pool_address,
            venue_id,
            reserve0: reserve0_bytes,
            reserve1: reserve1_bytes,
            liquidity: [0; 32],
            sqrt_price_x96: [0; 32],
            tick: 0,
            block_number,
            _padding: [0; 4],
        })
    }

    /// Create new V3 pool state
    pub fn new_v3(
        pool_address: [u8; 20],
        venue_id: u16,
        liquidity: U256,
        sqrt_price_x96: U256,
        tick: i32,
        block_number: u64,
    ) -> Result<Self, ValidationError> {
        if pool_address == [0u8; 20] {
            return Err(ValidationError::ZeroAddress);
        }

        let mut liquidity_bytes = [0u8; 32];
        liquidity.to_big_endian(&mut liquidity_bytes);

        let mut sqrt_price_bytes = [0u8; 32];
        sqrt_price_x96.to_big_endian(&mut sqrt_price_bytes);

        Ok(Self {
            pool_address,
            venue_id,
            reserve0: [0; 32],
            reserve1: [0; 32],
            liquidity: liquidity_bytes,
            sqrt_price_x96: sqrt_price_bytes,
            tick,
            block_number,
            _padding: [0; 4],
        })
    }

    /// Get reserve0 as U256 (V2 pools)
    pub fn reserve0(&self) -> U256 {
        U256::from_big_endian(&self.reserve0)
    }

    /// Get reserve1 as U256 (V2 pools)
    pub fn reserve1(&self) -> U256 {
        U256::from_big_endian(&self.reserve1)
    }

    /// Get liquidity as U256 (V3 pools)
    pub fn liquidity(&self) -> U256 {
        U256::from_big_endian(&self.liquidity)
    }

    /// Get sqrt_price_x96 as U256 (V3 pools)
    pub fn sqrt_price_x96(&self) -> U256 {
        U256::from_big_endian(&self.sqrt_price_x96)
    }

    /// Check if this is a V2 pool
    pub fn is_v2(&self) -> bool {
        self.reserve0 != [0; 32] || self.reserve1 != [0; 32]
    }

    /// Check if this is a V3 pool
    pub fn is_v3(&self) -> bool {
        self.liquidity != [0; 32] || self.sqrt_price_x96 != [0; 32]
    }
}

impl Message for PoolStateUpdate {
    const TYPE_ID: u16 = 11;
    const TOPIC: &'static str = "market-data";
}

/// Arbitrage signal (TLV type 20, Signal domain)
///
/// Represents a profitable arbitrage opportunity detected by strategy.
/// CRITICAL: Contains financially sensitive data - never log payload!
///
/// Contract: contracts.yaml → ArbitrageSignal
///
/// **Note**: U256 values are stored as [u8; 32] for rkyv compatibility.
/// Use getter/setter methods or conversion helpers.
#[derive(Debug, Clone, PartialEq, Archive, Serialize, Deserialize)]
#[archive(check_bytes)]
#[repr(C)]
pub struct ArbitrageSignal {
    /// Unique identifier for this opportunity
    pub opportunity_id: u64,

    /// Sequence of pool addresses for arbitrage path (2-4 hops)
    pub path: FixedVec<[u8; 20], MAX_POOL_ADDRESSES>,

    /// Estimated profit in USD
    pub estimated_profit_usd: f64,

    /// Estimated gas cost in wei - stored as big-endian bytes
    gas_estimate_wei: [u8; 32],

    /// Block number after which opportunity expires
    pub deadline_block: u64,
}

impl ArbitrageSignal {
    /// Create new arbitrage signal with validation
    pub fn new(
        opportunity_id: u64,
        path: &[[u8; 20]],
        estimated_profit_usd: f64,
        gas_estimate_wei: U256,
        deadline_block: u64,
    ) -> Result<Self, ValidationError> {
        // Validate path length (2-4 pools)
        if path.len() < 2 {
            return Err(ValidationError::PathTooShort(path.len()));
        }
        if path.len() > MAX_POOL_ADDRESSES {
            return Err(ValidationError::PathTooLong(path.len()));
        }

        // Validate profit is non-negative
        if estimated_profit_usd < 0.0 {
            return Err(ValidationError::NegativeProfit(estimated_profit_usd));
        }

        let path_fixed = FixedVec::from_slice(path)
            .map_err(|_| ValidationError::PathTooLong(path.len()))?;

        let mut gas_bytes = [0u8; 32];
        gas_estimate_wei.to_big_endian(&mut gas_bytes);

        Ok(Self {
            opportunity_id,
            path: path_fixed,
            estimated_profit_usd,
            gas_estimate_wei: gas_bytes,
            deadline_block,
        })
    }

    /// Get gas estimate as U256
    pub fn gas_estimate_wei(&self) -> U256 {
        U256::from_big_endian(&self.gas_estimate_wei)
    }

    /// Get path as slice
    pub fn path_slice(&self) -> &[[u8; 20]] {
        self.path.as_slice()
    }

    /// Number of hops in the arbitrage path
    pub fn hop_count(&self) -> usize {
        self.path.len()
    }
}

impl Message for ArbitrageSignal {
    const TYPE_ID: u16 = 20;
    const TOPIC: &'static str = "signals";
}

/// Validation errors for message construction
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ValidationError {
    #[error("Symbol cannot be empty")]
    EmptySymbol,

    #[error("Symbol starts with '0x' (indicates RPC error)")]
    InvalidSymbolPrefix,

    #[error("Symbol too long: {0} chars (max 32)")]
    SymbolTooLong(usize),

    #[error("Invalid decimals: {0} (must be 1-30)")]
    InvalidDecimals(u8),

    #[error("Zero address not allowed")]
    ZeroAddress,

    #[error("Arbitrage path too short: {0} pools (min 2)")]
    PathTooShort(usize),

    #[error("Arbitrage path too long: {0} pools (max 4)")]
    PathTooLong(usize),

    #[error("Negative profit not allowed: {0}")]
    NegativeProfit(f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_meta_creation() {
        let meta = InstrumentMeta::new(
            [1; 20],
            "WETH",
            18,
            137,
        ).unwrap();

        assert_eq!(meta.symbol_str(), "WETH");
        assert_eq!(meta.decimals, 18);
        assert_eq!(meta.chain_id, 137);
        assert_eq!(InstrumentMeta::TYPE_ID, 18);
    }

    #[test]
    fn test_instrument_meta_validation() {
        // Empty symbol
        assert!(matches!(
            InstrumentMeta::new([1; 20], "", 18, 137),
            Err(ValidationError::EmptySymbol)
        ));

        // 0x prefix (RPC error indicator)
        assert!(matches!(
            InstrumentMeta::new([1; 20], "0xWETH", 18, 137),
            Err(ValidationError::InvalidSymbolPrefix)
        ));

        // Invalid decimals
        assert!(matches!(
            InstrumentMeta::new([1; 20], "WETH", 0, 137),
            Err(ValidationError::InvalidDecimals(0))
        ));
        assert!(matches!(
            InstrumentMeta::new([1; 20], "WETH", 31, 137),
            Err(ValidationError::InvalidDecimals(31))
        ));

        // Zero address
        assert!(matches!(
            InstrumentMeta::new([0; 20], "WETH", 18, 137),
            Err(ValidationError::ZeroAddress)
        ));
    }

    #[test]
    fn test_pool_state_v2() {
        let state = PoolStateUpdate::new_v2(
            [1; 20],
            1,  // Uniswap V2
            U256::from(1000),
            U256::from(2000),
            12345,
        ).unwrap();

        assert!(state.is_v2());
        assert!(!state.is_v3());
        assert_eq!(state.reserve0(), U256::from(1000));
        assert_eq!(state.reserve1(), U256::from(2000));
        assert_eq!(PoolStateUpdate::TYPE_ID, 11);
    }

    #[test]
    fn test_pool_state_v3() {
        let state = PoolStateUpdate::new_v3(
            [1; 20],
            2,  // Uniswap V3
            U256::from(50000),
            U256::from(1234567890),
            100,
            12345,
        ).unwrap();

        assert!(!state.is_v2());
        assert!(state.is_v3());
        assert_eq!(state.liquidity(), U256::from(50000));
        assert_eq!(state.sqrt_price_x96(), U256::from(1234567890));
        assert_eq!(state.tick, 100);
    }

    #[test]
    fn test_arbitrage_signal_creation() {
        let path = [[1; 20], [2; 20], [3; 20]];
        let signal = ArbitrageSignal::new(
            123,
            &path,
            100.5,
            U256::from(21000),
            67890,
        ).unwrap();

        assert_eq!(signal.opportunity_id, 123);
        assert_eq!(signal.hop_count(), 3);
        assert_eq!(signal.estimated_profit_usd, 100.5);
        assert_eq!(ArbitrageSignal::TYPE_ID, 20);
    }

    #[test]
    fn test_arbitrage_signal_validation() {
        // Path too short
        let path = [[1; 20]];
        assert!(matches!(
            ArbitrageSignal::new(1, &path, 100.0, U256::zero(), 1000),
            Err(ValidationError::PathTooShort(1))
        ));

        // Path too long
        let path = [[1; 20], [2; 20], [3; 20], [4; 20], [5; 20]];
        assert!(matches!(
            ArbitrageSignal::new(1, &path, 100.0, U256::zero(), 1000),
            Err(ValidationError::PathTooLong(5))
        ));

        // Negative profit
        let path = [[1; 20], [2; 20]];
        assert!(matches!(
            ArbitrageSignal::new(1, &path, -10.0, U256::zero(), 1000),
            Err(ValidationError::NegativeProfit(_))
        ));
    }

    #[test]
    fn test_rkyv_roundtrip_instrument_meta() {
        let original = InstrumentMeta::new([1; 20], "USDC", 6, 137).unwrap();

        // Serialize
        let bytes = rkyv::to_bytes::<_, 256>(&original).unwrap();

        // Deserialize
        let archived = unsafe { rkyv::archived_root::<InstrumentMeta>(&bytes) };
        let deserialized: InstrumentMeta = archived.deserialize(&mut rkyv::Infallible).unwrap();

        assert_eq!(deserialized.symbol_str(), "USDC");
        assert_eq!(deserialized.decimals, 6);
    }

    #[test]
    fn test_rkyv_roundtrip_pool_state() {
        let original = PoolStateUpdate::new_v2(
            [5; 20],
            1,
            U256::from(1000000),
            U256::from(2000000),
            54321,
        ).unwrap();

        // Serialize
        let bytes = rkyv::to_bytes::<_, 1024>(&original).unwrap();

        // Deserialize
        let archived = unsafe { rkyv::archived_root::<PoolStateUpdate>(&bytes) };
        let deserialized: PoolStateUpdate = archived.deserialize(&mut rkyv::Infallible).unwrap();

        assert_eq!(deserialized.reserve0(), U256::from(1000000));
        assert_eq!(deserialized.reserve1(), U256::from(2000000));
        assert_eq!(deserialized.block_number, 54321);
    }

    #[test]
    fn test_rkyv_roundtrip_arbitrage_signal() {
        let path = [[1; 20], [2; 20], [3; 20]];
        let original = ArbitrageSignal::new(
            999,
            &path,
            250.75,
            U256::from(42000),
            99999,
        ).unwrap();

        // Serialize
        let bytes = rkyv::to_bytes::<_, 512>(&original).unwrap();

        // Deserialize
        let archived = unsafe { rkyv::archived_root::<ArbitrageSignal>(&bytes) };
        let deserialized: ArbitrageSignal = archived.deserialize(&mut rkyv::Infallible).unwrap();

        assert_eq!(deserialized.opportunity_id, 999);
        assert_eq!(deserialized.hop_count(), 3);
        assert_eq!(deserialized.estimated_profit_usd, 250.75);
    }
}
