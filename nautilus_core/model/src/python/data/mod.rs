// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod bar;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod order;
pub mod quote;
pub mod trade;

#[cfg(feature = "ffi")]
use nautilus_core::ffi::cvec::CVec;
use pyo3::{prelude::*, types::PyCapsule};

use crate::data::Data;

/// Creates a Python `PyCapsule` object containing a Rust `Data` instance.
///
/// This function takes ownership of the `Data` instance and encapsulates it within
/// a `PyCapsule` object, allowing the Rust data to be passed into the Python runtime.
///
/// # Panics
///
/// This function will panic if the `PyCapsule` creation fails, which may occur if
/// there are issues with memory allocation or if the `Data` instance cannot be
/// properly encapsulated.
///
/// # Safety
///
/// This function is safe as long as the `Data` instance does not violate Rust's
/// safety guarantees (e.g., no invalid memory access). However, users of the
/// `PyCapsule` in Python must ensure they understand how to extract and use the
/// encapsulated `Data` safely, especially when converting the capsule back to a
/// Rust data structure.
#[must_use]
pub fn data_to_pycapsule(py: Python, data: Data) -> PyObject {
    let capsule = PyCapsule::new(py, data, None).expect("Error creating `PyCapsule`");
    capsule.into_py(py)
}

/// Drops a `PyCapsule` containing a `CVec` structure.
///
/// This function safely extracts and drops the `CVec` instance encapsulated within
/// a `PyCapsule` object. It is intended for cleaning up after the `Data` instances
/// have been transferred into Python and are no longer needed.
///
/// # Panics
///
/// Panics if the capsule cannot be downcast to a `PyCapsule`, indicating a type mismatch
/// or improper capsule handling.
///
/// # Safety
///
/// This function is unsafe as it involves raw pointer dereferencing and manual memory
/// management. The caller must ensure the `PyCapsule` contains a valid `CVec` pointer.
/// Incorrect usage can lead to memory corruption or undefined behavior.
#[pyfunction]
#[cfg(feature = "ffi")]
pub fn drop_cvec_pycapsule(capsule: &PyAny) {
    let capsule: &PyCapsule = capsule
        .downcast()
        .expect("Error on downcast to `&PyCapsule`");
    let cvec: &CVec = unsafe { &*(capsule.pointer() as *const CVec) };
    let data: Vec<Data> =
        unsafe { Vec::from_raw_parts(cvec.ptr.cast::<Data>(), cvec.len, cvec.cap) };
    drop(data);
}

#[pyfunction]
#[cfg(not(feature = "ffi"))]
pub fn drop_cvec_pycapsule(_capsule: &PyAny) {
    panic!("`ffi` feature is not enabled");
}
