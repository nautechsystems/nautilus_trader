// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    ffi::c_char,
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use anyhow::Result;
use nautilus_core::{correctness::check_valid_string, string::cstr_to_str};
use ustr::Ustr;

/// Represents a valid execution algorithm ID.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct ExecAlgorithmId {
    /// The execution algorithm ID value.
    pub value: Ustr,
}

impl ExecAlgorithmId {
    pub fn new(s: &str) -> Result<Self> {
        check_valid_string(s, "`ExecAlgorithmId` value")?;

        Ok(Self {
            value: Ustr::from(s),
        })
    }
}

impl Debug for ExecAlgorithmId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Display for ExecAlgorithmId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<&str> for ExecAlgorithmId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
/// Returns a Nautilus identifier from a C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn exec_algorithm_id_new(ptr: *const c_char) -> ExecAlgorithmId {
    ExecAlgorithmId::from(cstr_to_str(ptr))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn exec_algorithm_id_hash(id: &ExecAlgorithmId) -> u64 {
    id.value.precomputed_hash()
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use super::ExecAlgorithmId;

    #[fixture]
    pub fn exec_algorithm_id() -> ExecAlgorithmId {
        ExecAlgorithmId::from("001")
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, ExecAlgorithmId};

    #[rstest]
    fn test_string_reprs(exec_algorithm_id: ExecAlgorithmId) {
        assert_eq!(exec_algorithm_id.to_string(), "001");
        assert_eq!(format!("{exec_algorithm_id}"), "001");
    }
}
