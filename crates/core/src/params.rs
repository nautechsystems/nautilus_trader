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

//! Generic parameter storage using `IndexMap<String, Value>`.
//!
//! This module provides a centralized definition of [`Params`] as a generic storage
//! solution for `serde_json::Value` data, along with Python bindings.

use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Newtype wrapper for generic parameter storage.
///
/// This represents a map of string keys to JSON values, used for passing
/// adapter-specific configuration, metadata, and any generic key-value data.
///
/// `Params` uses `IndexMap` to preserve insertion order, which is important for
/// consistent serialization and debugging.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Params(IndexMap<String, Value>);

impl Params {
    /// Creates an empty `Params` map.
    #[must_use]
    pub fn new() -> Self {
        Self(IndexMap::new())
    }

    /// Creates `Params` from an `IndexMap`.
    #[must_use]
    pub fn from_index_map(map: IndexMap<String, Value>) -> Self {
        Self(map)
    }

    /// Extracts a `u64` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `u64`.
    #[must_use]
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(|v| v.as_u64())
    }

    /// Extracts an `i64` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `i64`.
    #[must_use]
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_i64())
    }

    /// Extracts a `usize` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `usize`.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "usize is 64-bit on all supported targets"
    )]
    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
    }

    /// Extracts a string value from the params map.
    ///
    /// Returns `None` if the key is missing or the value is not a string.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_str())
    }

    /// Extracts a boolean value from the params map.
    ///
    /// Returns `None` if the key is missing or the value is not a boolean.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| v.as_bool())
    }

    /// Extracts a `f64` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `f64`.
    #[must_use]
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.as_f64())
    }

    #[cfg(feature = "python")]
    /// Converts `Params` to a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if conversion of any value fails.
    pub fn to_pydict(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::types::PyDict>> {
        crate::python::params::params_to_pydict(py, self)
    }
}

impl Deref for Params {
    type Target = IndexMap<String, Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Params {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> IntoIterator for &'a Params {
    type Item = (&'a String, &'a Value);
    type IntoIter = indexmap::map::Iter<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(feature = "python")]
/// Converts a Python dict to `Params`.
///
/// This is a convenience function that wraps `pydict_to_params`.
///
/// # Errors
///
/// Returns a `PyErr` if:
/// - the dict cannot be serialized to JSON
/// - the JSON is not a valid object
pub fn from_pydict(
    py: pyo3::Python<'_>,
    dict: pyo3::Py<pyo3::types::PyDict>,
) -> pyo3::PyResult<Option<Params>> {
    crate::python::params::pydict_to_params(py, dict)
}

#[cfg(test)]
mod tests {
    use rstest::*;
    use serde_json::json;

    use super::Params;

    fn create_test_params() -> Params {
        let mut params = Params::new();
        params.insert("u64_val".to_string(), json!(42u64));
        params.insert("i64_val".to_string(), json!(-100i64));
        params.insert("usize_val".to_string(), json!(5u64));
        params.insert("str_val".to_string(), json!("hello"));
        params.insert("bool_val".to_string(), json!(true));
        params.insert("f64_val".to_string(), json!(2.5));
        params
    }

    #[rstest]
    fn test_params_option_get_u64() {
        let params = Some(create_test_params());
        assert_eq!(params.as_ref().and_then(|p| p.get_u64("u64_val")), Some(42));
        assert_eq!(params.as_ref().and_then(|p| p.get_u64("missing")), None);
        assert_eq!(params.as_ref().and_then(|p| p.get_u64("str_val")), None);
    }

    #[rstest]
    fn test_params_option_get_i64() {
        let params = Some(create_test_params());
        assert_eq!(
            params.as_ref().and_then(|p| p.get_i64("i64_val")),
            Some(-100)
        );
        assert_eq!(params.as_ref().and_then(|p| p.get_i64("missing")), None);
    }

    #[rstest]
    fn test_params_option_get_usize() {
        let params = Some(create_test_params());
        assert_eq!(
            params.as_ref().and_then(|p| p.get_usize("usize_val")),
            Some(5)
        );
        assert_eq!(params.as_ref().and_then(|p| p.get_usize("missing")), None);
    }

    #[rstest]
    fn test_params_option_get_str() {
        let params = Some(create_test_params());
        assert_eq!(
            params.as_ref().and_then(|p| p.get_str("str_val")),
            Some("hello")
        );
        assert_eq!(params.as_ref().and_then(|p| p.get_str("missing")), None);
        assert_eq!(params.as_ref().and_then(|p| p.get_str("u64_val")), None);
    }

    #[rstest]
    fn test_params_option_get_bool() {
        let params = Some(create_test_params());
        assert_eq!(
            params.as_ref().and_then(|p| p.get_bool("bool_val")),
            Some(true)
        );
        assert_eq!(params.as_ref().and_then(|p| p.get_bool("missing")), None);
    }

    #[rstest]
    fn test_params_option_get_f64() {
        let params = Some(create_test_params());
        assert_eq!(
            params.as_ref().and_then(|p| p.get_f64("f64_val")),
            Some(2.5)
        );
        assert_eq!(params.as_ref().and_then(|p| p.get_f64("missing")), None);
    }

    #[rstest]
    fn test_params_option_none() {
        let params: Option<Params> = None;
        assert_eq!(params.as_ref().and_then(|p| p.get_u64("any")), None);
        assert_eq!(params.as_ref().and_then(|p| p.get_str("any")), None);
    }

    #[rstest]
    fn test_params_ref_get_u64() {
        let params = create_test_params();
        assert_eq!(params.get_u64("u64_val"), Some(42));
        assert_eq!(params.get_u64("missing"), None);
    }

    #[rstest]
    fn test_params_ref_get_usize() {
        let params = create_test_params();
        assert_eq!(params.get_usize("usize_val"), Some(5));
        assert_eq!(params.get_usize("missing"), None);
    }

    #[rstest]
    fn test_params_ref_get_str() {
        let params = create_test_params();
        assert_eq!(params.get_str("str_val"), Some("hello"));
        assert_eq!(params.get_str("missing"), None);
    }

    #[rstest]
    fn test_submit_tries_pattern() {
        let mut params = Params::new();
        params.insert("submit_tries".to_string(), json!(3u64));
        let cmd_params = Some(params);

        let submit_tries = cmd_params
            .as_ref()
            .and_then(|p| p.get_usize("submit_tries"))
            .filter(|&n| n > 0);

        assert_eq!(submit_tries, Some(3));
    }

    #[rstest]
    fn test_submit_tries_pattern_zero_filtered() {
        let mut params = Params::new();
        params.insert("submit_tries".to_string(), json!(0u64));
        let cmd_params = Some(params);

        let submit_tries = cmd_params
            .as_ref()
            .and_then(|p| p.get_usize("submit_tries"))
            .filter(|&n| n > 0);

        assert_eq!(submit_tries, None);
    }

    #[rstest]
    fn test_submit_tries_pattern_missing() {
        let cmd_params: Option<Params> = None;

        let submit_tries = cmd_params
            .as_ref()
            .and_then(|p| p.get_usize("submit_tries"))
            .filter(|&n| n > 0);

        assert_eq!(submit_tries, None);
    }
}
