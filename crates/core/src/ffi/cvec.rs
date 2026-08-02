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

//! Utilities for transferring heap-allocated Rust `Vec<T>` values across an FFI boundary.
//!
//! The primary abstraction offered by this module is `CVec`, a C-compatible struct that stores
//! a raw pointer (`ptr`) together with the vector's logical `len` and `cap`.  By moving the
//! allocation metadata into a plain `repr(C)` type we allow the memory created by Rust to be
//! owned, inspected, and ultimately freed by foreign code (or vice-versa) without introducing
//! undefined behaviour.
//!
//! Only a very small API surface is exposed to C:
//!
//! - `cvec_new` - create an empty `CVec` sentinel that can be returned to foreign code.
//!
//! De-allocation is intentionally **not** provided via a generic helper. Instead each FFI module
//! must expose its own *type-specific* `vec_*_drop` function which reconstructs the original
//! `Vec<T>` with [`Vec::from_raw_parts`] and allows it to drop. This avoids the size-mismatch risk
//! that a one-size-fits-all `cvec_drop` had in the past.
//!
//! All other manipulation happens on the Rust side before relinquishing ownership.  This keeps the
//! rules for memory safety straightforward: foreign callers must treat the memory region pointed
//! to by `ptr` as **opaque** and interact with it solely through the functions provided here.

use std::{ffi::c_void, fmt::Display, ptr::NonNull};

use crate::ffi::abort_on_panic;

/// `CVec` is a C compatible struct that stores an opaque pointer to a block of
/// memory, its length and the capacity of the vector it was allocated from.
///
/// # Safety
///
/// Changing the values here may lead to undefined behavior when the memory is dropped.
#[repr(C)]
#[derive(Debug)]
pub struct CVec {
    /// Opaque pointer to block of memory storing elements to access the
    /// elements cast it to the underlying type.
    pub ptr: *mut c_void,
    /// The number of elements in the block.
    pub len: usize,
    /// The capacity of vector from which it was allocated.
    /// Used when deallocating the memory
    pub cap: usize,
}

impl CVec {
    /// Returns an empty [`CVec`].
    ///
    /// This is primarily useful for constructing a sentinel value that represents the
    /// absence of data when crossing the FFI boundary.
    ///
    /// Uses a dangling pointer (like `Vec::new()`) rather than null to satisfy
    /// `Vec::from_raw_parts` preconditions when the `CVec` is later dropped.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            ptr: NonNull::<u8>::dangling().as_ptr().cast::<c_void>(),
            len: 0,
            cap: 0,
        }
    }

    /// Reconstructs and consumes the Rust vector represented by this value.
    ///
    /// # Safety
    ///
    /// For non-zero capacity, `ptr`, `len`, and `cap` must describe exactly one live allocation
    /// originally created by `Vec<T>` with `len` initialized elements. The allocation must not be
    /// accessed or reconstructed again after this call.
    ///
    /// # Panics
    ///
    /// Panics if `len > cap`, `len != 0` when `cap == 0`, or a non-empty allocation has a null
    /// pointer.
    #[must_use]
    pub unsafe fn into_vec<T>(self) -> Vec<T> {
        assert!(
            self.len <= self.cap,
            "CVec::into_vec: len ({}) > cap ({})",
            self.len,
            self.cap
        );

        if self.cap == 0 {
            assert_eq!(
                self.len, 0,
                "CVec::into_vec: zero capacity with non-zero len ({})",
                self.len
            );
            return Vec::new();
        }

        assert!(
            !self.ptr.is_null(),
            "CVec::into_vec: null ptr with non-zero cap ({})",
            self.cap
        );
        debug_assert!(self.ptr.cast::<T>().is_aligned());
        debug_assert!(
            self.cap
                .checked_mul(std::mem::size_of::<T>())
                .is_some_and(|bytes| isize::try_from(bytes).is_ok())
        );

        unsafe { Vec::from_raw_parts(self.ptr.cast::<T>(), self.len, self.cap) }
    }

    /// Borrows the initialized elements represented by this value.
    ///
    /// # Safety
    ///
    /// For non-zero length, `ptr` must point to `len` initialized, properly aligned `T` values
    /// that remain valid and are not mutated for the returned slice's lifetime.
    ///
    /// # Panics
    ///
    /// Panics if `len > cap` or a non-empty slice has a null pointer.
    #[must_use]
    pub unsafe fn as_slice<T>(&self) -> &[T] {
        assert!(
            self.len <= self.cap,
            "CVec::as_slice: len ({}) > cap ({})",
            self.len,
            self.cap
        );

        if self.len == 0 {
            return &[];
        }

        assert!(
            !self.ptr.is_null(),
            "CVec::as_slice: null ptr with non-zero len ({})",
            self.len
        );
        debug_assert!(self.ptr.cast::<T>().is_aligned());
        debug_assert!(
            self.len
                .checked_mul(std::mem::size_of::<T>())
                .is_some_and(|bytes| isize::try_from(bytes).is_ok())
        );

        unsafe { std::slice::from_raw_parts(self.ptr.cast::<T>(), self.len) }
    }
}

/// Consumes and leaks the Vec, returning a mutable pointer to the contents as
/// a [`CVec`]. The memory has been leaked and now exists for the lifetime of the
/// program unless dropped manually.
/// Note: drop the memory by reconstructing the vec using `from_raw_parts` method
/// as shown in the test below.
impl<T> From<Vec<T>> for CVec {
    fn from(mut data: Vec<T>) -> Self {
        if data.is_empty() {
            Self::empty()
        } else {
            let len = data.len();
            let cap = data.capacity();
            let ptr = data.as_mut_ptr();
            #[allow(
                clippy::mem_forget,
                reason = "intentional ownership transfer to C; matching CVec::drop reclaims via Vec::from_raw_parts"
            )]
            std::mem::forget(data);
            Self {
                ptr: ptr.cast::<std::ffi::c_void>(),
                len,
                cap,
            }
        }
    }
}

impl Display for CVec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CVec {{ ptr: {:?}, len: {}, cap: {} }}",
            self.ptr, self.len, self.cap,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

/// Construct a new *empty* [`CVec`] value for use as initialiser/sentinel in foreign code.
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub extern "C" fn cvec_new() -> CVec {
    abort_on_panic(CVec::empty)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use rstest::*;

    use super::CVec;

    /// Access values from a vector converted into a [`CVec`].
    #[rstest]
    #[allow(unused_assignments)]
    fn access_values_test() {
        let test_data = vec![1_u64, 2, 3];
        let mut vec_len = 0;
        let mut vec_cap = 0;
        let cvec: CVec = {
            let data = test_data.clone();
            vec_len = data.len();
            vec_cap = data.capacity();
            data.into()
        };

        assert_eq!(cvec.len, vec_len);
        assert_eq!(cvec.cap, vec_cap);

        let data = unsafe { cvec.into_vec::<u64>() };
        assert_eq!(data, test_data);
    }

    /// An empty vector gets converted to a dangling (non-null) pointer in a [`CVec`].
    #[rstest]
    fn empty_vec_should_give_dangling_ptr() {
        let data: Vec<u64> = vec![];
        let cvec: CVec = data.into();
        assert!(!cvec.ptr.is_null());
        assert_eq!(cvec.len, 0);
        assert_eq!(cvec.cap, 0);
    }

    #[repr(align(64))]
    struct Aligned;

    #[rstest]
    #[case(CVec::empty())]
    #[case(Vec::<u64>::new().into())]
    fn empty_into_vec_does_not_inspect_pointer(#[case] cvec: CVec) {
        let values = unsafe { cvec.into_vec::<u64>() };
        assert!(values.is_empty());
    }

    #[rstest]
    fn aligned_empty_into_vec_does_not_reconstruct_pointer() {
        let values = unsafe { CVec::empty().into_vec::<Aligned>() };
        assert!(values.is_empty());
    }

    #[rstest]
    fn aligned_empty_as_slice_does_not_inspect_pointer() {
        let cvec = CVec::empty();
        let values = unsafe { cvec.as_slice::<Aligned>() };
        assert!(values.is_empty());
    }

    #[rstest]
    fn non_empty_into_vec_round_trips_and_drops_once() {
        struct DropCounter(Arc<AtomicUsize>);

        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let drops = Arc::new(AtomicUsize::new(0));
        let cvec: CVec = vec![DropCounter(Arc::clone(&drops))].into();
        let values = unsafe { cvec.into_vec::<DropCounter>() };

        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(values);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[rstest]
    fn as_slice_borrows_without_consuming_caller_storage() {
        let values = vec![1_u64, 2, 3];
        let cvec = CVec {
            ptr: values.as_ptr().cast_mut().cast(),
            len: values.len(),
            cap: values.capacity(),
        };

        let borrowed = unsafe { cvec.as_slice::<u64>() };

        assert_eq!(borrowed, values);
        assert_eq!(values, [1, 2, 3]);
    }
}
