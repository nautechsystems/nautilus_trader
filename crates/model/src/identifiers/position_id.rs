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

//! Represents a valid position ID.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use nautilus_core::correctness::{
    CorrectnessResult, CorrectnessResultExt, FAILED, check_valid_string_utf8,
};
use ustr::Ustr;

/// Represents a valid position ID.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct PositionId(Ustr);

impl PositionId {
    /// Creates a new [`PositionId`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `value` is not a valid string.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked<T: AsRef<str>>(value: T) -> CorrectnessResult<Self> {
        let value = value.as_ref();
        check_valid_string_utf8(value, stringify!(value))?;
        Ok(Self(Ustr::from(value)))
    }

    /// Creates a new [`PositionId`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not a valid string.
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self::new_checked(value).expect_display(FAILED)
    }

    /// Sets the inner identifier value.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(crate) fn set_inner(&mut self, value: &str) {
        self.0 = Ustr::from(value);
    }

    /// Returns the inner identifier value.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        self.0
    }

    /// Returns the inner identifier value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Checks if the position ID is virtual.
    ///
    /// Returns `true` if the position ID starts with "P-", otherwise `false`.
    #[must_use]
    pub fn is_virtual(&self) -> bool {
        self.0.starts_with("P-")
    }
}

impl Debug for PositionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\"", self.0)
    }
}
impl Display for PositionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::PositionId;
    use crate::identifiers::stubs::*;

    #[rstest]
    fn test_string_reprs(position_id_test: PositionId) {
        assert_eq!(position_id_test.as_str(), "P-123456789");
        assert_eq!(format!("{position_id_test}"), "P-123456789");
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: invalid string for 'value', was empty")]
    fn test_new_with_empty_string_panics_with_display_format() {
        let _ = PositionId::new("");
    }

    #[rstest]
    fn test_deserialize_json_with_unicode_escapes() {
        let id: PositionId = serde_json::from_str(r#""P-\u9f99\u867e-1""#).unwrap();
        assert_eq!(id.as_str(), "P-\u{9f99}\u{867e}-1");
    }

    #[rstest]
    fn test_serialization_roundtrip_non_ascii() {
        let id = PositionId::new("P-\u{9f99}\u{867e}-1");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"P-\u{9f99}\u{867e}-1\"");

        let deserialized: PositionId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[rstest]
    fn test_deserialize_rejects_empty_string() {
        let result: Result<PositionId, _> = serde_json::from_str(r#""""#);
        assert!(result.is_err());
    }
}
