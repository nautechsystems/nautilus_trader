// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! A stack-allocated ASCII string type for efficient identifier storage.
//!
//! This module provides [`StackStr`], a fixed-capacity string type optimized for
//! short identifier strings. Designed for use cases where:
//!
//! - Strings are known to be short (≤36 characters).
//! - Stack allocation is preferred over heap allocation.
//! - `Copy` semantics are beneficial.
//! - C FFI compatibility is required.
//!
//! # ASCII requirement
//!
//! `StackStr` only accepts ASCII strings. This guarantees that 1 character == 1 byte,
//! ensuring the buffer always holds exactly the capacity in characters. This aligns
//! with identifier conventions which are inherently ASCII.
//!
//! | Property              | ASCII    | UTF-8               |
//! |-----------------------|----------|---------------------|
//! | Bytes per char        | Always 1 | 1-4                 |
//! | 36 bytes holds        | 36 chars | 9-36 chars          |
//! | Slice at any byte     | Safe     | May split codepoint |
//! | `len()` == char count | Yes      | No                  |

// Required for C FFI pointer handling and unchecked UTF-8/CStr conversions
#![allow(unsafe_code)]

use std::{
    borrow::Borrow,
    cmp::Ordering,
    ffi::{CStr, c_char},
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::Deref,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::correctness::FAILED;

/// Maximum capacity in characters for a [`StackStr`].
pub const STACKSTR_CAPACITY: usize = 36;

/// Fixed buffer size including null terminator (capacity + 1).
const STACKSTR_BUFFER_SIZE: usize = STACKSTR_CAPACITY + 1;

/// A stack-allocated ASCII string with a maximum capacity of 36 characters.
///
/// Optimized for short identifier strings with:
/// - Stack allocation (no heap).
/// - `Copy` semantics.
/// - O(1) length access.
/// - C FFI compatibility (null-terminated).
///
/// ASCII is required to guarantee 1 character == 1 byte, ensuring the buffer
/// always holds exactly the capacity in characters. This aligns with identifier
/// conventions which are inherently ASCII.
///
/// # Memory Layout
///
/// The `value` field is placed first so the struct pointer equals the string
/// pointer, making C FFI more natural: `(char*)&stack_str` works directly.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct StackStr {
    /// ASCII data with null terminator for C FFI.
    value: [u8; 37], // STACKSTR_CAPACITY + 1
    /// Length of the string in bytes (0-36).
    len: u8,
}

impl StackStr {
    /// Maximum length in characters.
    pub const MAX_LEN: usize = STACKSTR_CAPACITY;

    /// Creates a new [`StackStr`] from a string slice.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `s` is empty or contains only whitespace.
    /// - `s` contains non-ASCII characters or interior NUL bytes.
    /// - `s` exceeds 36 characters.
    #[must_use]
    pub fn new(s: &str) -> Self {
        Self::new_checked(s).expect(FAILED)
    }

    /// Creates a new [`StackStr`] with validation, returning an error on failure.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `s` is empty or contains only whitespace.
    /// - `s` contains non-ASCII characters or interior NUL bytes.
    /// - `s` exceeds 36 characters.
    pub fn new_checked(s: &str) -> anyhow::Result<Self> {
        if s.is_empty() {
            anyhow::bail!("String is empty");
        }

        if s.len() > STACKSTR_CAPACITY {
            anyhow::bail!(
                "String exceeds maximum length of {} characters, was {}",
                STACKSTR_CAPACITY,
                s.len()
            );
        }

        if !s.is_ascii() {
            anyhow::bail!("String contains non-ASCII character");
        }

        let bytes = s.as_bytes();
        if bytes.contains(&0) {
            anyhow::bail!("String contains interior NUL byte");
        }

        if bytes.iter().all(|b| b.is_ascii_whitespace()) {
            anyhow::bail!("String contains only whitespace");
        }

        let mut value = [0u8; STACKSTR_BUFFER_SIZE];
        value[..s.len()].copy_from_slice(bytes);
        // Null terminator is already set (array initialized to 0)

        Ok(Self {
            value,
            len: s.len() as u8,
        })
    }

    /// Creates a [`StackStr`] from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `bytes` is empty or contains only whitespace.
    /// - `bytes` contains non-ASCII characters or interior NUL bytes.
    /// - `bytes` exceeds 36 bytes (excluding trailing null terminator).
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        // Strip trailing null terminator if present
        let bytes = if bytes.last() == Some(&0) {
            &bytes[..bytes.len() - 1]
        } else {
            bytes
        };

        let s = std::str::from_utf8(bytes).map_err(|e| anyhow::anyhow!("Invalid UTF-8: {e}"))?;

        Self::new_checked(s)
    }

    /// Creates a [`StackStr`] from a C string pointer.
    ///
    /// For untrusted input from C code, use [`from_c_ptr_checked`](Self::from_c_ptr_checked)
    /// to avoid panics crossing FFI boundaries.
    ///
    /// # Safety
    ///
    /// - `ptr` must be a valid pointer to a null-terminated C string.
    /// - The string must contain only valid ASCII (no interior NUL bytes).
    /// - The string must not exceed 36 characters.
    ///
    /// Violating these requirements causes a panic. If this function is called
    /// from C code, such a panic is undefined behavior.
    #[must_use]
    pub unsafe fn from_c_ptr(ptr: *const c_char) -> Self {
        // SAFETY: Caller guarantees ptr is valid and null-terminated
        let cstr = unsafe { CStr::from_ptr(ptr) };
        let s = cstr.to_str().expect("Invalid UTF-8 in C string");
        Self::new(s)
    }

    /// Creates a [`StackStr`] from a C string pointer with validation.
    ///
    /// Returns `None` if the string is invalid. This is safe to call from C code
    /// as it never panics on invalid input.
    ///
    /// # Safety
    ///
    /// - `ptr` must be a valid pointer to a null-terminated C string.
    #[must_use]
    pub unsafe fn from_c_ptr_checked(ptr: *const c_char) -> Option<Self> {
        // SAFETY: Caller guarantees ptr is valid and null-terminated
        let cstr = unsafe { CStr::from_ptr(ptr) };
        let s = cstr.to_str().ok()?;
        Self::new_checked(s).ok()
    }

    /// Returns the string as a `&str`.
    ///
    /// This is an O(1) operation.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        debug_assert!(
            self.len as usize <= STACKSTR_CAPACITY,
            "StackStr len {} exceeds capacity {}",
            self.len,
            STACKSTR_CAPACITY
        );
        // SAFETY: We guarantee only valid ASCII is stored via check_valid_string_ascii
        // on construction. ASCII is always valid UTF-8.
        unsafe { std::str::from_utf8_unchecked(&self.value[..self.len as usize]) }
    }

    /// Returns the length in bytes (equal to character count for ASCII).
    ///
    /// This is an O(1) operation.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Returns `true` if the string is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a pointer to the null-terminated C string.
    #[inline]
    #[must_use]
    pub const fn as_ptr(&self) -> *const c_char {
        self.value.as_ptr().cast::<c_char>()
    }

    /// Returns the value as a C string slice.
    #[inline]
    #[must_use]
    pub fn as_cstr(&self) -> &CStr {
        debug_assert!(
            self.len as usize <= STACKSTR_CAPACITY,
            "StackStr len {} exceeds capacity {}",
            self.len,
            STACKSTR_CAPACITY
        );
        debug_assert!(
            self.value[self.len as usize] == 0,
            "StackStr missing null terminator at position {}",
            self.len
        );
        // SAFETY: We guarantee the string is null-terminated (buffer initialized to 0,
        // and we only write up to len bytes leaving the null terminator intact),
        // and no interior NUL bytes (rejected during construction).
        unsafe { CStr::from_bytes_with_nul_unchecked(&self.value[..=self.len as usize]) }
    }
}

impl PartialEq for StackStr {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && self.value[..self.len as usize] == other.value[..other.len as usize]
    }
}

impl Eq for StackStr {}

impl Hash for StackStr {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Only hash actual content, not padding
        self.value[..self.len as usize].hash(state);
    }
}

impl Ord for StackStr {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl PartialOrd for StackStr {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Display for StackStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Debug for StackStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl Serialize for StackStr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StackStr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        Self::new_checked(s).map_err(serde::de::Error::custom)
    }
}

impl From<&str> for StackStr {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl AsRef<str> for StackStr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for StackStr {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Default for StackStr {
    /// Creates an empty [`StackStr`] with length 0.
    ///
    /// Note: While [`StackStr::new`] rejects empty strings, `default()` creates
    /// an empty placeholder. Use [`is_empty`](StackStr::is_empty) to check for this state.
    fn default() -> Self {
        Self {
            value: [0u8; STACKSTR_BUFFER_SIZE],
            len: 0,
        }
    }
}

impl Deref for StackStr {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl PartialEq<&str> for StackStr {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<str> for StackStr {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl TryFrom<&[u8]> for StackStr {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hasher};

    use ahash::AHashMap;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_valid() {
        let s = StackStr::new("hello");
        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());
    }

    #[rstest]
    fn test_max_length() {
        let input = "x".repeat(36);
        let s = StackStr::new(&input);
        assert_eq!(s.len(), 36);
        assert_eq!(s.as_str(), input);
    }

    #[rstest]
    #[should_panic]
    fn test_exceeds_max_length() {
        let input = "x".repeat(37);
        let _ = StackStr::new(&input);
    }

    #[rstest]
    #[should_panic]
    fn test_empty_string() {
        let _ = StackStr::new("");
    }

    #[rstest]
    #[should_panic]
    fn test_whitespace_only() {
        let _ = StackStr::new("   ");
    }

    #[rstest]
    #[should_panic]
    fn test_non_ascii() {
        let _ = StackStr::new("hello\u{1F600}"); // emoji
    }

    #[rstest]
    #[should_panic]
    fn test_interior_nul_byte() {
        let _ = StackStr::new("abc\0def");
    }

    #[rstest]
    fn test_interior_nul_byte_checked() {
        let result = StackStr::new_checked("abc\0def");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NUL"));
    }

    #[rstest]
    fn test_from_c_ptr_checked_valid() {
        let cstring = std::ffi::CString::new("hello").unwrap();
        let s = unsafe { StackStr::from_c_ptr_checked(cstring.as_ptr()) };
        assert!(s.is_some());
        assert_eq!(s.unwrap().as_str(), "hello");
    }

    #[rstest]
    fn test_from_c_ptr_checked_too_long() {
        let long = "x".repeat(37);
        let cstring = std::ffi::CString::new(long).unwrap();
        let s = unsafe { StackStr::from_c_ptr_checked(cstring.as_ptr()) };
        assert!(s.is_none());
    }

    #[rstest]
    fn test_equality() {
        let a = StackStr::new("test");
        let b = StackStr::new("test");
        let c = StackStr::new("other");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[rstest]
    fn test_hash_consistency() {
        use std::hash::DefaultHasher;

        let a = StackStr::new("test");
        let b = StackStr::new("test");

        let hash_a = {
            let mut h = DefaultHasher::new();
            a.hash(&mut h);
            h.finish()
        };
        let hash_b = {
            let mut h = DefaultHasher::new();
            b.hash(&mut h);
            h.finish()
        };

        assert_eq!(hash_a, hash_b);
    }

    #[rstest]
    fn test_hashmap_usage() {
        let mut map = AHashMap::new();
        map.insert(StackStr::new("key1"), 1);
        map.insert(StackStr::new("key2"), 2);

        assert_eq!(map.get(&StackStr::new("key1")), Some(&1));
        assert_eq!(map.get(&StackStr::new("key2")), Some(&2));
        assert_eq!(map.get(&StackStr::new("key3")), None);
    }

    #[rstest]
    fn test_ordering() {
        let a = StackStr::new("aaa");
        let b = StackStr::new("bbb");
        assert!(a < b);
        assert!(b > a);
    }

    #[rstest]
    fn test_c_compatibility() {
        let s = StackStr::new("test");
        let cstr = s.as_cstr();
        assert_eq!(cstr.to_str().unwrap(), "test");
    }

    #[rstest]
    fn test_as_ptr() {
        let s = StackStr::new("test");
        let ptr = s.as_ptr();
        assert!(!ptr.is_null());

        let cstr = unsafe { CStr::from_ptr(ptr) };
        assert_eq!(cstr.to_str().unwrap(), "test");
    }

    #[rstest]
    fn test_from_bytes() {
        let s = StackStr::from_bytes(b"hello").unwrap();
        assert_eq!(s.as_str(), "hello");
    }

    #[rstest]
    fn test_from_bytes_with_null() {
        let s = StackStr::from_bytes(b"hello\0").unwrap();
        assert_eq!(s.as_str(), "hello");
    }

    #[rstest]
    fn test_serde_roundtrip() {
        let original = StackStr::new("test123");
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "\"test123\"");

        let deserialized: StackStr = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_display() {
        let s = StackStr::new("hello");
        assert_eq!(format!("{s}"), "hello");
    }

    #[rstest]
    fn test_debug() {
        let s = StackStr::new("hello");
        assert_eq!(format!("{s:?}"), "\"hello\"");
    }

    #[rstest]
    fn test_from_str() {
        let s: StackStr = "hello".into();
        assert_eq!(s.as_str(), "hello");
    }

    #[rstest]
    fn test_as_ref() {
        let s = StackStr::new("hello");
        let r: &str = s.as_ref();
        assert_eq!(r, "hello");
    }

    #[rstest]
    fn test_borrow() {
        let s = StackStr::new("hello");
        let b: &str = s.borrow();
        assert_eq!(b, "hello");
    }

    #[rstest]
    fn test_default() {
        let s = StackStr::default();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[rstest]
    fn test_copy_semantics() {
        let a = StackStr::new("test");
        let b = a; // Copy, not move
        assert_eq!(a, b); // Both are still valid
    }

    #[rstest]
    #[case("BINANCE")]
    #[case("ETH-PERP")]
    #[case("O-20231215-001")]
    #[case("123456789012345678901234567890123456")] // 36 chars (max)
    fn test_valid_identifiers(#[case] s: &str) {
        let stack_str = StackStr::new(s);
        assert_eq!(stack_str.as_str(), s);
    }

    #[rstest]
    fn test_single_char() {
        let s = StackStr::new("x");
        assert_eq!(s.len(), 1);
        assert_eq!(s.as_str(), "x");
    }

    #[rstest]
    fn test_length_35() {
        let input = "x".repeat(35);
        let s = StackStr::new(&input);
        assert_eq!(s.len(), 35);
    }

    #[rstest]
    fn test_length_36_exact() {
        let input = "x".repeat(36);
        let s = StackStr::new(&input);
        assert_eq!(s.len(), 36);
        assert_eq!(s.as_str(), input);
    }

    #[rstest]
    fn test_length_37_rejected() {
        let input = "x".repeat(37);
        let result = StackStr::new_checked(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds"));
    }

    #[rstest]
    fn test_struct_size() {
        assert_eq!(std::mem::size_of::<StackStr>(), 38);
    }

    #[rstest]
    fn test_value_field_at_offset_zero() {
        let s = StackStr::new("hello");
        let struct_ptr = std::ptr::from_ref(&s).cast::<u8>();
        let first_byte = unsafe { *struct_ptr };
        assert_eq!(first_byte, b'h');
    }

    #[rstest]
    fn test_null_terminator_present() {
        let s = StackStr::new("test");
        let ptr = s.as_ptr();
        // Read byte at position 4 (after "test")
        let null_byte = unsafe { *ptr.offset(4) };
        assert_eq!(null_byte, 0);
    }

    #[rstest]
    fn test_from_bytes_empty() {
        let result = StackStr::from_bytes(b"");
        assert!(result.is_err());
    }

    #[rstest]
    fn test_from_bytes_interior_nul() {
        let result = StackStr::from_bytes(b"abc\0def");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NUL"));
    }

    #[rstest]
    fn test_from_bytes_non_ascii() {
        let result = StackStr::from_bytes(&[0x80, 0x81]); // Non-ASCII bytes
        assert!(result.is_err());
    }

    #[rstest]
    fn test_from_bytes_too_long() {
        let bytes = [b'x'; 55];
        let result = StackStr::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_from_bytes_whitespace_only() {
        let result = StackStr::from_bytes(b"   ");
        assert!(result.is_err());
    }

    #[rstest]
    fn test_hash_differs_for_different_content() {
        let a = StackStr::new("abc");
        let b = StackStr::new("xyz");

        let hash_a = {
            let mut h = DefaultHasher::new();
            a.hash(&mut h);
            h.finish()
        };
        let hash_b = {
            let mut h = DefaultHasher::new();
            b.hash(&mut h);
            h.finish()
        };

        assert_ne!(hash_a, hash_b);
    }

    #[rstest]
    fn test_hash_ignores_padding() {
        let a = StackStr::new("test");
        let b = StackStr::new("test");

        let hash_a = {
            let mut h = DefaultHasher::new();
            a.hash(&mut h);
            h.finish()
        };
        let hash_b = {
            let mut h = DefaultHasher::new();
            b.hash(&mut h);
            h.finish()
        };

        assert_eq!(hash_a, hash_b);
    }

    #[rstest]
    fn test_serde_deserialize_too_long() {
        let long = format!("\"{}\"", "x".repeat(55));
        let result: Result<StackStr, _> = serde_json::from_str(&long);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serde_deserialize_empty() {
        let result: Result<StackStr, _> = serde_json::from_str("\"\"");
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serde_deserialize_non_ascii() {
        let result: Result<StackStr, _> = serde_json::from_str("\"hello\u{1F600}\"");
        assert!(result.is_err());
    }

    #[rstest]
    #[case("!@#$%^&*()")]
    #[case("hello-world_123")]
    #[case("a.b.c.d")]
    #[case("key=value")]
    #[case("path/to/file")]
    #[case("[bracket]")]
    #[case("{curly}")]
    fn test_special_ascii_chars(#[case] s: &str) {
        let stack_str = StackStr::new(s);
        assert_eq!(stack_str.as_str(), s);
    }

    #[rstest]
    fn test_ascii_control_chars_tab() {
        // Tab is whitespace but valid ASCII
        let result = StackStr::new_checked("a\tb");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "a\tb");
    }

    #[rstest]
    fn test_ordering_same_prefix_different_length() {
        let short = StackStr::new("abc");
        let long = StackStr::new("abcd");
        assert!(short < long);
    }

    #[rstest]
    fn test_ordering_case_sensitive() {
        let upper = StackStr::new("ABC");
        let lower = StackStr::new("abc");
        // ASCII: 'A' (65) < 'a' (97)
        assert!(upper < lower);
    }

    #[rstest]
    fn test_partial_cmp_returns_some() {
        let a = StackStr::new("test");
        let b = StackStr::new("test");
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Equal));
    }

    #[rstest]
    fn test_new_checked_error_empty() {
        let err = StackStr::new_checked("").unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[rstest]
    fn test_new_checked_error_whitespace() {
        let err = StackStr::new_checked("   ").unwrap_err();
        assert!(err.to_string().contains("whitespace"));
    }

    #[rstest]
    fn test_new_checked_error_too_long() {
        let err = StackStr::new_checked(&"x".repeat(55)).unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }

    #[rstest]
    fn test_new_checked_error_non_ascii() {
        let err = StackStr::new_checked("hello\u{1F600}").unwrap_err();
        assert!(err.to_string().contains("non-ASCII"));
    }

    #[rstest]
    fn test_new_checked_error_interior_nul() {
        let err = StackStr::new_checked("abc\0def").unwrap_err();
        assert!(err.to_string().contains("NUL"));
    }

    #[rstest]
    fn test_clone_equals_original() {
        let a = StackStr::new("test");
        #[allow(clippy::clone_on_copy)]
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[rstest]
    fn test_deref() {
        let s = StackStr::new("hello");
        assert!(s.starts_with("hell"));
        assert_eq!(s.len(), 5);
    }

    #[rstest]
    fn test_partial_eq_str_literal() {
        let s = StackStr::new("hello");
        assert!(s == "hello");
        assert!(s != "world");
    }

    #[rstest]
    fn test_try_from_bytes() {
        let s: StackStr = b"hello".as_slice().try_into().unwrap();
        assert_eq!(s.as_str(), "hello");
    }
}
