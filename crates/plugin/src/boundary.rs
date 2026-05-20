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

//! Primitive `#[repr(C)]` types used at the plug-in boundary.
//!
//! Only types in this module (and other `#[repr(C)]` types built from them) may
//! cross between an independently compiled plug-in cdylib and the host. Standard
//! library types like `String`, `Vec`, and `Box<dyn Trait>` rely on Rust's
//! unstable ABI and must never appear in a function signature exposed across
//! the boundary.

#![allow(unsafe_code)]

use core::{marker::PhantomData, ptr, slice};

/// A borrowed UTF-8 string with a lifetime tied to the producer's storage.
///
/// Use this for `'static` strings baked into a plug-in's manifest (type names,
/// version strings). The host reads through the pointer while the producing
/// library is loaded; in v1 that is the process lifetime, since plug-ins are
/// not unloaded.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BorrowedStr<'a> {
    pub ptr: *const u8,
    pub len: usize,
    _phantom: PhantomData<&'a [u8]>,
}

/// SAFETY: `BorrowedStr` is just a pointer + length; sending it across threads
/// is sound as long as the underlying storage outlives the use. In v1 the
/// storage is process-lifetime static memory in the producing library.
unsafe impl Send for BorrowedStr<'_> {}
/// SAFETY: see `Send` impl.
unsafe impl Sync for BorrowedStr<'_> {}

impl<'a> BorrowedStr<'a> {
    /// Returns an empty borrowed string.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ptr: ptr::null(),
            len: 0,
            _phantom: PhantomData,
        }
    }

    /// Wraps a Rust string slice as a borrowed boundary string.
    #[must_use]
    pub const fn from_str(s: &'a str) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
            _phantom: PhantomData,
        }
    }

    /// Converts the borrowed string back to a `&str`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the producing storage is still live and the
    /// bytes are valid UTF-8.
    #[must_use]
    pub unsafe fn as_str(&self) -> &'a str {
        if self.ptr.is_null() || self.len == 0 {
            return "";
        }
        // SAFETY: caller upholds the lifetime and UTF-8 contract.
        let bytes = unsafe { slice::from_raw_parts(self.ptr, self.len) };
        // SAFETY: producer commits to valid UTF-8.
        unsafe { core::str::from_utf8_unchecked(bytes) }
    }
}

impl core::fmt::Debug for BorrowedStr<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // SAFETY: Debug is best-effort; if the producer has dropped storage
        // this would be UB. The plug-in contract pins manifest strings to
        // process lifetime so reads here are sound.
        let s = unsafe { self.as_str() };
        write!(f, "BorrowedStr({s:?})")
    }
}

/// A borrowed slice of `T` with a lifetime tied to the producer's storage.
///
/// Used in the manifest to enumerate per-trait registration entries without
/// crossing the boundary with `Vec`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Slice<'a, T> {
    pub ptr: *const T,
    pub len: usize,
    _phantom: PhantomData<&'a [T]>,
}

/// SAFETY: see [`BorrowedStr`].
unsafe impl<T: Sync> Send for Slice<'_, T> {}
/// SAFETY: see [`BorrowedStr`].
unsafe impl<T: Sync> Sync for Slice<'_, T> {}

impl<'a, T> Slice<'a, T> {
    /// Returns an empty slice.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ptr: ptr::null(),
            len: 0,
            _phantom: PhantomData,
        }
    }

    /// Wraps a Rust slice as a boundary slice.
    #[must_use]
    pub const fn from_slice(s: &'a [T]) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
            _phantom: PhantomData,
        }
    }

    /// Borrows the slice as a `&[T]`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the producing storage is still live.
    #[must_use]
    pub unsafe fn as_slice(&self) -> &'a [T] {
        if self.ptr.is_null() || self.len == 0 {
            return &[];
        }
        // SAFETY: caller upholds the lifetime.
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// Coarse-grained error categories for [`PluginError`].
///
/// Encoded as `u32` for stable wire representation.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginErrorCode {
    Ok = 0,
    Generic = 1,
    Panic = 2,
    InvalidArgument = 3,
    NotImplemented = 4,
    AbiMismatch = 5,
    SerializationFailed = 6,
}

/// An owned byte buffer crossing the plug-in boundary.
///
/// Allocated by the producing side and freed by the producer's `drop_fn` so
/// allocator mismatches between host and plug-in stay impossible. v1 uses
/// this only for runtime-constructed error messages; data payloads cross via
/// other paths (Arrow IPC for batches, JSON via `OwnedBytes` for single items).
#[repr(C)]
pub struct OwnedBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
    pub drop_fn: Option<unsafe extern "C" fn(ptr: *mut u8, len: usize, cap: usize)>,
}

/// SAFETY: a heap pointer freed only by its producer's `drop_fn`; safe to
/// transfer ownership across threads.
unsafe impl Send for OwnedBytes {}

impl OwnedBytes {
    /// Constructs an empty `OwnedBytes` with no drop function.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
            drop_fn: None,
        }
    }

    /// Returns whether the buffer is empty (no allocation).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0 || self.ptr.is_null()
    }

    /// Constructs an `OwnedBytes` from a Rust `Vec<u8>` using the producer's
    /// allocator and stamps the matching producer-side free as `drop_fn`.
    ///
    /// Consumers release the buffer by dropping the `OwnedBytes` (which
    /// invokes the embedded `drop_fn`) or by calling that `drop_fn`
    /// explicitly. Do not call [`drop_owned_bytes`] on a value received
    /// across the plug-in boundary: that would free with the *consumer's*
    /// allocator, which may not match the producer's. [`drop_owned_bytes`]
    /// is only the default function installed here for the producer; each
    /// side sees its own copy linked against its own allocator.
    #[must_use]
    pub fn from_vec(v: Vec<u8>) -> Self {
        let mut v = core::mem::ManuallyDrop::new(v);
        let ptr = v.as_mut_ptr();
        let len = v.len();
        let cap = v.capacity();
        Self {
            ptr,
            len,
            cap,
            drop_fn: Some(drop_owned_bytes),
        }
    }

    /// Borrows the buffer as a byte slice.
    ///
    /// # Safety
    ///
    /// The buffer must still be live (i.e. its `drop_fn` not yet called).
    #[must_use]
    pub unsafe fn as_bytes(&self) -> &[u8] {
        if self.is_empty() {
            return &[];
        }
        // SAFETY: caller upholds liveness.
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for OwnedBytes {
    fn drop(&mut self) {
        if let Some(f) = self.drop_fn.take()
            && !self.ptr.is_null()
        {
            // SAFETY: ptr/len/cap originate from `from_vec` or from a
            // matching producer; drop_fn is the matching free.
            unsafe { f(self.ptr, self.len, self.cap) };
            self.ptr = ptr::null_mut();
            self.len = 0;
            self.cap = 0;
        }
    }
}

/// Default `drop_fn` used by [`OwnedBytes::from_vec`]. Plug-ins that build
/// `OwnedBytes` via `from_vec` get matching free behaviour automatically.
///
/// # Safety
///
/// The caller must pass `ptr`, `len`, and `cap` originally returned by a
/// `Vec<u8>` that was leaked via `from_vec`.
pub unsafe extern "C" fn drop_owned_bytes(ptr: *mut u8, len: usize, cap: usize) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: pointer originates from `Vec::into_raw_parts`-style leak.
    unsafe {
        let _ = Vec::from_raw_parts(ptr, len, cap);
    }
}

/// Generic plug-in error returned across the boundary.
///
/// `message` is owned by the producer; the consumer drops it via its
/// `OwnedBytes` `drop_fn` once it has been logged or wrapped.
#[repr(C)]
pub struct PluginError {
    pub code: PluginErrorCode,
    pub message: OwnedBytes,
}

impl PluginError {
    /// Constructs an error with a `Generic` code and a message string.
    #[must_use]
    pub fn generic(message: impl AsRef<str>) -> Self {
        Self {
            code: PluginErrorCode::Generic,
            message: OwnedBytes::from_vec(message.as_ref().as_bytes().to_vec()),
        }
    }

    /// Constructs an error with the given code and message string.
    #[must_use]
    pub fn new(code: PluginErrorCode, message: impl AsRef<str>) -> Self {
        Self {
            code,
            message: OwnedBytes::from_vec(message.as_ref().as_bytes().to_vec()),
        }
    }

    /// Constructs a panic error with the given message.
    #[must_use]
    pub fn panic(message: impl AsRef<str>) -> Self {
        Self::new(PluginErrorCode::Panic, message)
    }

    /// Returns the message as a `String` (lossy if non-UTF8).
    #[must_use]
    pub fn message_string(&self) -> String {
        // SAFETY: message is live until self is dropped.
        let bytes = unsafe { self.message.as_bytes() };
        String::from_utf8_lossy(bytes).into_owned()
    }
}

impl core::fmt::Debug for PluginError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct(stringify!(PluginError))
            .field("code", &self.code)
            .field("message", &self.message_string())
            .finish()
    }
}

/// A `Result`-shaped union for boundary calls.
///
/// `#[repr(C, u8)]` so the discriminant is a single byte at offset zero,
/// independent of payload alignment.
#[repr(C, u8)]
pub enum PluginResult<T> {
    Ok(T),
    Err(PluginError),
}

impl<T> PluginResult<T> {
    /// Converts to a `core::result::Result`, dropping the discriminant.
    pub fn into_result(self) -> Result<T, PluginError> {
        match self {
            Self::Ok(t) => Ok(t),
            Self::Err(e) => Err(e),
        }
    }

    /// Wraps a `Result` produced inside Rust into a boundary result.
    pub fn from_result(r: Result<T, PluginError>) -> Self {
        match r {
            Ok(t) => Self::Ok(t),
            Err(e) => Self::Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::ascii("hello")]
    #[case::empty("")]
    #[case::utf8("héllo wörld")]
    #[case::multibyte("\u{1F600}\u{1F4A9}")]
    fn borrowed_str_round_trips(#[case] s: &str) {
        let b = BorrowedStr::from_str(s);
        // SAFETY: storage lives for the duration of this test.
        let back = unsafe { b.as_str() };
        assert_eq!(back, s);
    }

    #[rstest]
    fn slice_round_trips() {
        let data: [u32; 3] = [1, 2, 3];
        let s = Slice::from_slice(&data);
        // SAFETY: storage lives for the duration of this test.
        let back = unsafe { s.as_slice() };
        assert_eq!(back, &[1u32, 2, 3]);
    }

    #[rstest]
    fn empty_slice_returns_empty() {
        let s: Slice<u8> = Slice::empty();
        // SAFETY: empty slice is always safe to view.
        let back = unsafe { s.as_slice() };
        assert!(back.is_empty());
    }

    #[rstest]
    fn owned_bytes_round_trip_and_drop() {
        let payload = b"hello world".to_vec();
        let owned = OwnedBytes::from_vec(payload.clone());
        // SAFETY: still live until owned drops.
        let view = unsafe { owned.as_bytes() }.to_vec();
        assert_eq!(view, payload);
        drop(owned);
    }

    #[rstest]
    fn owned_bytes_drop_fn_runs_exactly_once() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn counting_drop(ptr: *mut u8, len: usize, cap: usize) {
            if !ptr.is_null() {
                COUNTER.fetch_add(1, Ordering::SeqCst);
                // SAFETY: pointer originates from the boxed slice leaked below.
                unsafe {
                    let _ = Vec::from_raw_parts(ptr, len, cap);
                }
            }
        }

        COUNTER.store(0, Ordering::SeqCst);
        let mut v = core::mem::ManuallyDrop::new(vec![1u8, 2, 3, 4]);
        let ptr = v.as_mut_ptr();
        let len = v.len();
        let cap = v.capacity();
        let owned = OwnedBytes {
            ptr,
            len,
            cap,
            drop_fn: Some(counting_drop),
        };
        assert_eq!(COUNTER.load(Ordering::SeqCst), 0);
        drop(owned);
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    fn plugin_error_carries_message() {
        let err = PluginError::generic("bad input");
        assert_eq!(err.code, PluginErrorCode::Generic);
        assert_eq!(err.message_string(), "bad input");
    }

    #[rstest]
    fn plugin_result_round_trips() {
        let ok: PluginResult<u32> = PluginResult::Ok(42);
        let r = ok.into_result();
        assert_eq!(r.unwrap(), 42);

        let err: PluginResult<u32> = PluginResult::Err(PluginError::generic("nope"));
        let r = err.into_result();
        assert!(r.is_err());
    }

    #[rstest]
    fn plugin_result_from_result_round_trips() {
        let r: PluginResult<u32> = PluginResult::from_result(Ok(7));
        assert_eq!(r.into_result().unwrap(), 7);

        let r: PluginResult<u32> = PluginResult::from_result(Err(PluginError::generic("x")));
        let e = r.into_result().unwrap_err();
        assert_eq!(e.code, PluginErrorCode::Generic);
        assert_eq!(e.message_string(), "x");
    }
}
