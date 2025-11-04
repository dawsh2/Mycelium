//! Zero-copy safety types
//!
//! Provides explicit ownership semantics to prevent aliasing bugs in zero-copy operations.
//! These types make it clear when data is owned vs borrowed, and when copying is required.

use std::marker::PhantomData;
use std::ops::Deref;

/// Owned buffer with exclusive ownership
///
/// This buffer owns its data and will free the memory when dropped.
/// Use this when you need to store data beyond the current scope.
///
/// # Example
///
/// ```rust,ignore
/// let buf = OwnedBuf::from_vec(vec![1, 2, 3, 4]);
/// // buf owns the data and will free it on drop
/// ```
#[derive(Debug, Clone)]
pub struct OwnedBuf {
    data: Vec<u8>,
}

impl OwnedBuf {
    /// Create an owned buffer from a Vec
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Create an owned buffer by copying from a slice
    pub fn copy_from(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
        }
    }

    /// Create an empty owned buffer with the given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
        }
    }

    /// Get the underlying data as a slice
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Get mutable access to the underlying data
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Convert into the underlying Vec
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }

    /// Get the length of the buffer
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Deref for OwnedBuf {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl From<Vec<u8>> for OwnedBuf {
    fn from(data: Vec<u8>) -> Self {
        Self::from_vec(data)
    }
}

impl From<OwnedBuf> for Vec<u8> {
    fn from(buf: OwnedBuf) -> Self {
        buf.into_vec()
    }
}

/// Shared read-only view with bounded lifetime
///
/// This type borrows data for zero-copy reads. The lifetime ensures
/// the data remains valid for the duration of the borrow.
///
/// # Example
///
/// ```rust,ignore
/// let data = vec![1, 2, 3, 4];
/// let view = SharedView::from_slice(&data);
/// // view borrows data, cannot outlive data
/// ```
#[derive(Debug, Clone, Copy)]
pub struct SharedView<'a> {
    data: &'a [u8],
    _marker: PhantomData<&'a ()>,
}

impl<'a> SharedView<'a> {
    /// Create a shared view from a slice
    pub fn from_slice(data: &'a [u8]) -> Self {
        Self {
            data,
            _marker: PhantomData,
        }
    }

    /// Copy the data into an owned buffer
    pub fn to_owned(&self) -> OwnedBuf {
        OwnedBuf::copy_from(self.data)
    }

    /// Get the underlying data as a slice
    pub fn as_slice(&self) -> &[u8] {
        self.data
    }

    /// Get the length of the view
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the view is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl<'a> Deref for SharedView<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a> From<&'a [u8]> for SharedView<'a> {
    fn from(data: &'a [u8]) -> Self {
        Self::from_slice(data)
    }
}

impl<'a> From<&'a Vec<u8>> for SharedView<'a> {
    fn from(data: &'a Vec<u8>) -> Self {
        Self::from_slice(data)
    }
}

impl<'a> From<&'a OwnedBuf> for SharedView<'a> {
    fn from(buf: &'a OwnedBuf) -> Self {
        Self::from_slice(buf.as_slice())
    }
}

/// Copy-on-write buffer
///
/// Can be either borrowed (zero-copy) or owned (copied).
/// Automatically copies when mutation is needed.
///
/// # Example
///
/// ```rust,ignore
/// // Start with borrowed data (zero-copy)
/// let data = vec![1, 2, 3, 4];
/// let cow = Cow::Borrowed(SharedView::from_slice(&data));
///
/// // Convert to owned if needed
/// let owned = cow.into_owned();
/// ```
#[derive(Debug, Clone)]
pub enum Cow<'a> {
    /// Borrowed data (zero-copy read-only view)
    Borrowed(SharedView<'a>),
    /// Owned data (has exclusive ownership)
    Owned(OwnedBuf),
}

impl<'a> Cow<'a> {
    /// Create a borrowed Cow from a slice
    pub fn borrowed(data: &'a [u8]) -> Self {
        Self::Borrowed(SharedView::from_slice(data))
    }

    /// Create an owned Cow from a Vec
    pub fn owned(data: Vec<u8>) -> Self {
        Self::Owned(OwnedBuf::from_vec(data))
    }

    /// Convert to owned, copying if necessary
    pub fn into_owned(self) -> OwnedBuf {
        match self {
            Cow::Borrowed(view) => view.to_owned(),
            Cow::Owned(buf) => buf,
        }
    }

    /// Get a reference to the data as a slice
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Cow::Borrowed(view) => view.as_slice(),
            Cow::Owned(buf) => buf.as_slice(),
        }
    }

    /// Get the length of the buffer
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Check if this is borrowed data
    pub fn is_borrowed(&self) -> bool {
        matches!(self, Cow::Borrowed(_))
    }

    /// Check if this is owned data
    pub fn is_owned(&self) -> bool {
        matches!(self, Cow::Owned(_))
    }
}

impl<'a> Deref for Cow<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a> From<SharedView<'a>> for Cow<'a> {
    fn from(view: SharedView<'a>) -> Self {
        Cow::Borrowed(view)
    }
}

impl<'a> From<OwnedBuf> for Cow<'a> {
    fn from(buf: OwnedBuf) -> Self {
        Cow::Owned(buf)
    }
}

impl<'a> From<Vec<u8>> for Cow<'a> {
    fn from(data: Vec<u8>) -> Self {
        Cow::Owned(OwnedBuf::from_vec(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owned_buf() {
        let buf = OwnedBuf::from_vec(vec![1, 2, 3, 4]);
        assert_eq!(buf.len(), 4);
        assert_eq!(buf.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_owned_buf_copy_from() {
        let data = [1, 2, 3, 4];
        let buf = OwnedBuf::copy_from(&data);
        assert_eq!(buf.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_shared_view() {
        let data = vec![1, 2, 3, 4];
        let view = SharedView::from_slice(&data);
        assert_eq!(view.len(), 4);
        assert_eq!(view.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_shared_view_to_owned() {
        let data = vec![1, 2, 3, 4];
        let view = SharedView::from_slice(&data);
        let owned = view.to_owned();
        assert_eq!(owned.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_cow_borrowed() {
        let data = vec![1, 2, 3, 4];
        let cow = Cow::borrowed(&data);
        assert!(cow.is_borrowed());
        assert_eq!(cow.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_cow_owned() {
        let cow = Cow::owned(vec![1, 2, 3, 4]);
        assert!(cow.is_owned());
        assert_eq!(cow.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_cow_into_owned() {
        let data = vec![1, 2, 3, 4];
        let cow = Cow::borrowed(&data);
        let owned = cow.into_owned();
        assert_eq!(owned.as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_cow_already_owned() {
        let cow = Cow::owned(vec![1, 2, 3, 4]);
        let owned = cow.into_owned();
        assert_eq!(owned.as_slice(), &[1, 2, 3, 4]);
    }
}
