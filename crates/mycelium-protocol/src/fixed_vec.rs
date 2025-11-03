//! Fixed-capacity collections for zero-copy TLV serialization
//!
//! Provides `FixedVec<T, N>` and `FixedStr<N>` - stack-allocated collections
//! with runtime length tracking, enabling zero-copy serialization with zerocopy.
//!
//! ## Why FixedVec Instead of Vec?
//!
//! ```text
//! // ❌ Vec<T> - Heap allocated, pointer indirection → NOT zero-copy
//! Vec<[u8; 20]>  // Pool addresses - requires heap allocation
//!
//! // ✅ FixedVec<T, N> - Stack allocated, inline storage → Zero-copy!
//! FixedVec<[u8; 20], 4>  // Up to 4 pool addresses, no heap
//! ```
//!
//! ## Key Design
//!
//! - `#[repr(C)]` for deterministic memory layout
//! - Inline storage (no heap allocation)
//! - Runtime length tracking (0 to N elements)
//! - Unused slots are zeroed for deterministic serialization
//! - Manual zerocopy trait impls (can't derive generically)

use std::convert::TryFrom;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Compile-time constants for array sizing
pub const MAX_POOL_ADDRESSES: usize = 4;  // Max pools in arbitrage path
pub const MAX_SYMBOL_LENGTH: usize = 32;   // Max token symbol length

/// Error type for fixed collection operations
#[derive(Debug, Clone, PartialEq)]
pub enum FixedVecError {
    /// Attempted to insert more elements than maximum capacity
    CapacityExceeded {
        max_capacity: usize,
        attempted: usize,
    },
    /// Invalid slice length for conversion
    InvalidLength { expected_max: usize, got: usize },
}

impl std::fmt::Display for FixedVecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FixedVecError::CapacityExceeded {
                max_capacity,
                attempted,
            } => {
                write!(
                    f,
                    "Capacity exceeded: max={}, attempted={}",
                    max_capacity, attempted
                )
            }
            FixedVecError::InvalidLength { expected_max, got } => {
                write!(
                    f,
                    "Invalid length: expected max {}, got {}",
                    expected_max, got
                )
            }
        }
    }
}

impl std::error::Error for FixedVecError {}

/// Fixed-capacity vector with zero-copy serialization support
///
/// **Why custom FixedVec instead of heapless::Vec or arrayvec?**
///
/// 1. Zero-copy serialization with zerocopy requires manual trait impls
/// 2. Standard crates can't provide zerocopy support generically
/// 3. HFT demands sub-microsecond serialization (<1μs target)
/// 4. Only need support for specific types used in TLV messages
///
/// **Key Distinction: FixedVec ≠ Vec**
/// - `Vec<T>`: Heap-allocated, pointer indirection → **Cannot be zero-copy**
/// - `FixedVec<T, N>`: Stack-allocated, inline storage → **Enables zero-copy**
///
/// ## Memory Layout (Inline Storage)
///
/// ```text
/// [count: u16][_padding: [u8; 6]][elements: [T; N]]
/// ```
///
/// All data is stored inline (no heap allocation), enabling direct memory mapping
/// for zero-copy serialization/deserialization with zerocopy.
///
/// **Note**: AsBytes/FromBytes cannot be derived generically - manual impls required
/// for each concrete instantiation. See bottom of file for trait implementations.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct FixedVec<T, const N: usize>
where
    T: Copy,
{
    /// Number of valid elements (0 to N)
    count: u16,

    /// Padding to ensure proper alignment for elements array
    _padding: [u8; 6],

    /// Fixed-size array holding elements (unused slots are zeroed)
    elements: [T; N],
}

impl<T, const N: usize> FixedVec<T, N>
where
    T: Copy + Default,
{
    /// Create new empty FixedVec
    pub fn new() -> Self {
        Self {
            count: 0,
            _padding: [0; 6],
            elements: [T::default(); N],
        }
    }

    /// Get the underlying array (including unused slots)
    pub fn as_array(&self) -> &[T; N] {
        &self.elements
    }

    /// Get mutable access to the underlying array (for zero-copy construction)
    pub fn as_array_mut(&mut self) -> &mut [T; N] {
        &mut self.elements
    }

    /// Set the element count (for direct array manipulation)
    ///
    /// # Safety
    ///
    /// Caller must ensure that elements[0..count] are properly initialized
    pub fn set_count(&mut self, count: usize) {
        debug_assert!(count <= N, "Count {} exceeds capacity {}", count, N);
        self.count = count.min(N) as u16;
    }

    /// Push element if capacity allows
    pub fn try_push(&mut self, element: T) -> Result<(), FixedVecError> {
        if self.count as usize >= N {
            return Err(FixedVecError::CapacityExceeded {
                max_capacity: N,
                attempted: self.count as usize + 1,
            });
        }

        self.elements[self.count as usize] = element;
        self.count += 1;
        Ok(())
    }

    /// Clear all elements (zeros the array)
    pub fn clear(&mut self) {
        self.count = 0;
        self.elements = [T::default(); N];
    }

    /// Get iterator over valid elements only
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// Maximum capacity of this FixedVec
    pub const fn capacity() -> usize {
        N
    }

    /// Current number of valid elements
    pub fn len(&self) -> usize {
        self.count as usize
    }

    /// Check if FixedVec is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Get element at index (bounds checked)
    pub fn get(&self, index: usize) -> Option<&T> {
        if index < self.count as usize {
            Some(&self.elements[index])
        } else {
            None
        }
    }

    /// Convert to slice of valid elements only
    pub fn as_slice(&self) -> &[T] {
        &self.elements[..self.count as usize]
    }

    /// Create from slice with bounds validation
    pub fn from_slice(slice: &[T]) -> Result<Self, FixedVecError> {
        if slice.len() > N {
            return Err(FixedVecError::InvalidLength {
                expected_max: N,
                got: slice.len(),
            });
        }

        let mut result = Self::new();
        result.count = slice.len() as u16;

        // Copy elements to array
        for (i, &element) in slice.iter().enumerate() {
            result.elements[i] = element;
        }

        Ok(result)
    }

    /// Convert to Vec for interoperability
    pub fn to_vec(&self) -> Vec<T> {
        self.as_slice().to_vec()
    }
}

impl<T, const N: usize> Default for FixedVec<T, N>
where
    T: Copy + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> TryFrom<&[T]> for FixedVec<T, N>
where
    T: Copy + Default,
{
    type Error = FixedVecError;

    fn try_from(slice: &[T]) -> Result<Self, Self::Error> {
        Self::from_slice(slice)
    }
}

impl<T, const N: usize> TryFrom<Vec<T>> for FixedVec<T, N>
where
    T: Copy + Default,
{
    type Error = FixedVecError;

    fn try_from(vec: Vec<T>) -> Result<Self, Self::Error> {
        Self::from_slice(&vec)
    }
}

/// Fixed-capacity UTF-8 string with zero-copy serialization
///
/// Stores UTF-8 string data in fixed-size array with length prefix.
/// Unused bytes are zeroed for deterministic serialization.
///
/// **Note**: AsBytes/FromBytes manually implemented for MAX_SYMBOL_LENGTH.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct FixedStr<const N: usize> {
    /// Length of valid UTF-8 data
    len: u16,

    /// Padding for alignment
    _padding: [u8; 6],

    /// Raw UTF-8 bytes (unused bytes are zeroed)
    data: [u8; N],
}

impl<const N: usize> FixedStr<N> {
    /// Create new empty string
    pub fn new() -> Self {
        Self {
            len: 0,
            _padding: [0; 6],
            data: [0; N],
        }
    }

    /// Create from string slice with validation
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, FixedVecError> {
        let bytes = s.as_bytes();
        if bytes.len() > N {
            return Err(FixedVecError::InvalidLength {
                expected_max: N,
                got: bytes.len(),
            });
        }

        let mut result = Self::new();
        result.len = bytes.len() as u16;
        result.data[..bytes.len()].copy_from_slice(bytes);
        Ok(result)
    }

    /// Get string slice of valid data
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.data[..self.len as usize])
    }

    /// Get length of valid string data
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Check if string is empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get maximum capacity
    pub const fn capacity() -> usize {
        N
    }
}

impl<const N: usize> Default for FixedStr<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> TryFrom<&str> for FixedStr<N> {
    type Error = FixedVecError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_str(s)
    }
}

impl<const N: usize> TryFrom<String> for FixedStr<N> {
    type Error = FixedVecError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::from_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_vec_basic_operations() {
        let mut vec: FixedVec<u32, 4> = FixedVec::new();
        assert_eq!(vec.len(), 0);
        assert!(vec.is_empty());
        assert_eq!(FixedVec::<u32, 4>::capacity(), 4);

        // Test pushing elements
        assert!(vec.try_push(10).is_ok());
        assert!(vec.try_push(20).is_ok());
        assert_eq!(vec.len(), 2);
        assert!(!vec.is_empty());

        // Test accessing elements
        assert_eq!(vec.get(0), Some(&10));
        assert_eq!(vec.get(1), Some(&20));
        assert_eq!(vec.get(2), None);

        // Test slice conversion
        assert_eq!(vec.as_slice(), &[10, 20]);
    }

    #[test]
    fn test_fixed_vec_capacity_exceeded() {
        let mut vec: FixedVec<u32, 2> = FixedVec::new();

        assert!(vec.try_push(1).is_ok());
        assert!(vec.try_push(2).is_ok());

        // Should fail on third element
        let result = vec.try_push(3);
        assert!(matches!(
            result,
            Err(FixedVecError::CapacityExceeded { .. })
        ));
    }

    #[test]
    fn test_fixed_vec_from_slice() {
        let slice = &[1u32, 2, 3];
        let vec: FixedVec<u32, 5> = FixedVec::from_slice(slice).unwrap();

        assert_eq!(vec.len(), 3);
        assert_eq!(vec.as_slice(), slice);
        assert_eq!(vec.to_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn test_fixed_vec_bijection() {
        let original = vec![10u32, 20, 30, 40];
        let fixed_vec: FixedVec<u32, 8> = FixedVec::from_slice(&original).unwrap();
        let recovered = fixed_vec.to_vec();

        // Perfect bijection
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_fixed_str_basic_operations() {
        let mut s: FixedStr<16> = FixedStr::new();
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
        assert_eq!(FixedStr::<16>::capacity(), 16);

        // Create from string
        s = FixedStr::from_str("hello").unwrap();
        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());
        assert_eq!(s.as_str().unwrap(), "hello");
    }

    #[test]
    fn test_fixed_str_capacity_exceeded() {
        let long_str = "This string is definitely longer than 8 characters";
        let result = FixedStr::<8>::from_str(long_str);
        assert!(matches!(result, Err(FixedVecError::InvalidLength { .. })));
    }

    #[test]
    fn test_zerocopy_roundtrip_fixed_vec() {
        use zerocopy::AsBytes;

        let mut vec: FixedVec<[u8; 20], MAX_POOL_ADDRESSES> = FixedVec::new();
        vec.try_push([1; 20]).unwrap();
        vec.try_push([2; 20]).unwrap();

        // Serialize with zerocopy
        let bytes = vec.as_bytes();

        // Deserialize with zerocopy (zero-copy cast)
        let deserialized: &FixedVec<[u8; 20], MAX_POOL_ADDRESSES> =
            FixedVec::ref_from(bytes).unwrap();

        assert_eq!(deserialized.len(), 2);
        assert_eq!(deserialized.get(0), Some(&[1; 20]));
        assert_eq!(deserialized.get(1), Some(&[2; 20]));
    }

    #[test]
    fn test_zerocopy_roundtrip_fixed_str() {
        use zerocopy::AsBytes;

        let s = FixedStr::<MAX_SYMBOL_LENGTH>::from_str("Hello").unwrap();

        // Serialize with zerocopy
        let bytes = s.as_bytes();

        // Deserialize with zerocopy (zero-copy cast)
        let deserialized: &FixedStr<MAX_SYMBOL_LENGTH> = FixedStr::ref_from(bytes).unwrap();

        assert_eq!(deserialized.as_str().unwrap(), "Hello");
    }
}

// ============================================================================
// Manual zerocopy trait implementations for concrete types
// ============================================================================
//
// NOTE: zerocopy cannot derive AsBytes/FromBytes generically for FixedVec<T, N>
// Must manually implement for each concrete instantiation used in messages.

/// Manual AsBytes/FromBytes for FixedVec<[u8; 20], 4> (arbitrage paths)
unsafe impl AsBytes for FixedVec<[u8; 20], MAX_POOL_ADDRESSES> {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}

unsafe impl FromBytes for FixedVec<[u8; 20], MAX_POOL_ADDRESSES> {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}

unsafe impl FromZeroes for FixedVec<[u8; 20], MAX_POOL_ADDRESSES> {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}

/// Manual AsBytes/FromBytes for FixedStr<32> (token symbols)
unsafe impl AsBytes for FixedStr<MAX_SYMBOL_LENGTH> {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}

unsafe impl FromBytes for FixedStr<MAX_SYMBOL_LENGTH> {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}

unsafe impl FromZeroes for FixedStr<MAX_SYMBOL_LENGTH> {
    fn only_derive_is_allowed_to_implement_this_trait() {}
}
