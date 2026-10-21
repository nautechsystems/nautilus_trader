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

//! Immutable JSON parameter storage with optional shared values.
//!
//! This module provides a centralized definition of [`Params`] as a generic storage
//! solution for `serde_json::Value` data, along with Python bindings.

use std::{
    fmt::Debug,
    hash::Hash,
    ops::Deref,
    sync::{Arc, OnceLock},
};

use indexmap::{Equivalent, IndexMap};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

/// Immutable generic parameter storage.
///
/// This represents a map of string keys to JSON values, used for passing
/// adapter-specific configuration, metadata, and any generic key-value data.
///
/// `from_pydict` rejects integer values outside the signed or unsigned 64-bit range with
/// `ValueError`, including values in nested containers.
///
/// `Params` uses `IndexMap` to preserve insertion order, which is important for
/// consistent serialization and debugging.
///
/// Maps built with [`Self::from_shared`] share storage across clones and can share values
/// across different maps. [`Self::get`], [`Self::borrowed_entries`], and the typed getters borrow
/// these values directly. `len`, `is_empty`, `capacity`, and `contains_key` also avoid copying.
/// Indexing, `&Params` iteration, and access through the read-only `IndexMap` interface
/// materialize and cache owned values. Use `borrowed_entries` to iterate without copying.
/// Serialization preserves contents and insertion order, but not allocation sharing.
///
/// Build an owned map before calling [`Self::new`]. To derive different
/// parameters, use `IndexMap::clone(&params)`, edit the map, and construct a new `Params`.
/// To retain shared values, edit the map from [`Self::to_shared_map`] and call
/// [`Self::from_shared`].
///
/// The compiler rejects mutation, including nested JSON values:
///
/// ```compile_fail
/// use nautilus_core::Params;
///
/// let mut params = Params::empty();
/// params.insert("key".to_string(), serde_json::json!(1));
/// ```
///
/// ```compile_fail
/// use nautilus_core::Params;
///
/// let mut params = Params::new(
///     [("nested".to_string(), serde_json::json!({"value": 1}))].into(),
/// );
/// params["nested"]["value"] = serde_json::json!(2);
/// ```
///
/// ```compile_fail
/// use nautilus_core::Params;
/// use std::sync::Arc;
///
/// let mut params = Params::from_shared(
///     [("nested".to_string(), Arc::new(serde_json::json!({"value": 1})))].into(),
/// );
/// params["nested"]["value"] = serde_json::json!(2);
/// ```
#[derive(Clone)]
pub struct Params(Storage);

impl Params {
    /// Creates an empty `Params` map.
    #[must_use]
    pub fn empty() -> Self {
        Self::new(IndexMap::new())
    }

    /// Creates `Params` from an `IndexMap`.
    #[must_use]
    pub fn new(map: IndexMap<String, Value>) -> Self {
        Self(Storage::Owned(map))
    }

    /// Creates immutable parameters from shared JSON values without copying their contents.
    #[must_use]
    pub fn from_shared(map: IndexMap<String, Arc<Value>>) -> Self {
        Self(Storage::Shared(Arc::new(SharedStorage {
            values: map,
            owned: OnceLock::new(),
        })))
    }

    /// Copies the map for deriving new parameters, retaining existing shared value allocations.
    ///
    /// Owned values are cloned.
    #[must_use]
    pub fn to_shared_map(&self) -> IndexMap<String, Arc<Value>> {
        match &self.0 {
            Storage::Owned(values) => values
                .iter()
                .map(|(key, value)| (key.clone(), Arc::new(value.clone())))
                .collect(),
            Storage::Shared(shared) => shared.values.clone(),
        }
    }

    /// Borrows entries in insertion order without materializing shared values.
    pub fn borrowed_entries(&self) -> impl DoubleEndedIterator<Item = (&String, &Value)> {
        let (owned, shared) = match &self.0 {
            Storage::Owned(values) => (Some(values), None),
            Storage::Shared(shared) => (None, Some(&shared.values)),
        };

        owned.into_iter().flat_map(IndexMap::iter).chain(
            shared
                .into_iter()
                .flat_map(IndexMap::iter)
                .map(|(key, value)| (key, value.as_ref())),
        )
    }

    /// Returns the number of parameters.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.0 {
            Storage::Owned(values) => values.len(),
            Storage::Shared(shared) => shared.values.len(),
        }
    }

    /// Returns whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the map capacity without materializing shared values.
    #[must_use]
    pub fn capacity(&self) -> usize {
        match &self.0 {
            Storage::Owned(values) => values.capacity(),
            Storage::Shared(shared) => shared.values.capacity(),
        }
    }

    /// Returns a value by key without materializing shared values.
    #[must_use]
    pub fn get<Q>(&self, key: &Q) -> Option<&Value>
    where
        Q: ?Sized + Hash + Equivalent<String>,
    {
        match &self.0 {
            Storage::Owned(values) => values.get(key),
            Storage::Shared(shared) => shared.values.get(key).map(AsRef::as_ref),
        }
    }

    /// Returns whether the map contains a key.
    #[must_use]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        Q: ?Sized + Hash + Equivalent<String>,
    {
        self.get(key).is_some()
    }

    /// Estimates retained key, value, and element storage without materializing shared values.
    ///
    /// Charges shared values in full for each entry and includes any cached owned map.
    /// Excludes the outer storage allocation, allocator overhead, and hash-table overhead.
    #[must_use]
    pub fn estimated_heap_size(&self) -> usize {
        match &self.0 {
            Storage::Owned(values) => map_heap_size(values),
            Storage::Shared(shared) => shared.values.iter().fold(
                shared.owned.get().map_or(0, map_heap_size).saturating_add(
                    shared
                        .values
                        .capacity()
                        .saturating_mul(size_of::<(usize, String, Arc<Value>)>()),
                ),
                |bytes, (key, value)| {
                    bytes
                        .saturating_add(key.capacity())
                        .saturating_add(size_of::<(usize, usize, Value)>())
                        .saturating_add(json_heap_size(value))
                },
            ),
        }
    }

    /// Extracts a `u64` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `u64`.
    #[must_use]
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(Value::as_u64)
    }

    /// Extracts an `i64` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `i64`.
    #[must_use]
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Value::as_i64)
    }

    /// Extracts a `usize` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `usize`.
    #[must_use]
    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get(key)
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
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
        self.get(key).and_then(Value::as_bool)
    }

    /// Extracts a `f64` value from the params map.
    ///
    /// Returns `None` if the key is missing or the value cannot be converted to `f64`.
    #[must_use]
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    #[cfg(feature = "python")]
    /// Converts `Params` to a Python dict.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion of any value fails.
    pub fn to_pydict(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::types::PyDict>> {
        crate::python::params::params_to_pydict(py, self)
    }
}

impl Deref for Params {
    type Target = IndexMap<String, Value>;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            Storage::Owned(values) => values,
            Storage::Shared(shared) => shared.owned.get_or_init(|| {
                shared
                    .values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.as_ref().clone()))
                    .collect()
            }),
        }
    }
}

impl<'a> IntoIterator for &'a Params {
    type Item = (&'a String, &'a Value);
    type IntoIter = indexmap::map::Iter<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.deref().iter()
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::empty()
    }
}

impl Debug for Params {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Storage::Owned(values) => f.debug_tuple("Params").field(values).finish(),
            Storage::Shared(shared) => f.debug_tuple("Params").field(&shared.values).finish(),
        }
    }
}

impl PartialEq for Params {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .borrowed_entries()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl Eq for Params {}

impl Serialize for Params {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self.borrowed_entries() {
            map.serialize_entry(key, value)?;
        }

        map.end()
    }
}

impl<'de> Deserialize<'de> for Params {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        IndexMap::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Clone)]
enum Storage {
    Owned(IndexMap<String, Value>),
    Shared(Arc<SharedStorage>),
}

struct SharedStorage {
    values: IndexMap<String, Arc<Value>>,
    owned: OnceLock<IndexMap<String, Value>>,
}

fn map_heap_size(values: &IndexMap<String, Value>) -> usize {
    values.iter().fold(
        values
            .capacity()
            .saturating_mul(size_of::<(usize, String, Value)>()),
        |bytes, (key, value)| {
            bytes
                .saturating_add(key.capacity())
                .saturating_add(json_heap_size(value))
        },
    )
}

fn json_heap_size(value: &Value) -> usize {
    match value {
        Value::String(value) => value.capacity(),
        Value::Array(values) => values.iter().fold(
            values.capacity().saturating_mul(size_of::<Value>()),
            |bytes, value| bytes.saturating_add(json_heap_size(value)),
        ),
        Value::Object(values) => values.iter().fold(0usize, |bytes, (key, value)| {
            bytes
                .saturating_add(size_of::<(String, Value)>())
                .saturating_add(key.capacity())
                .saturating_add(json_heap_size(value))
        }),
        _ => 0,
    }
}

#[cfg(feature = "python")]
/// Converts a Python dict to `Params`.
///
/// This is a convenience function that wraps `pydict_to_params`.
///
/// # Errors
///
/// Returns an error if:
/// - The dict cannot be serialized to JSON.
/// - The JSON is not a valid object.
/// - An integer value is outside the signed or unsigned 64-bit range.
pub fn from_pydict(
    py: pyo3::Python<'_>,
    dict: &pyo3::Py<pyo3::types::PyDict>,
) -> pyo3::PyResult<Option<Params>> {
    crate::python::params::pydict_to_params(py, dict)
}

#[cfg(test)]
mod tests {
    use rstest::*;
    use serde_json::json;

    use super::*;

    fn create_test_params() -> Params {
        Params::new(
            [
                ("u64_val".to_string(), json!(42u64)),
                ("i64_val".to_string(), json!(-100i64)),
                ("usize_val".to_string(), json!(5u64)),
                ("str_val".to_string(), json!("hello")),
                ("bool_val".to_string(), json!(true)),
                ("f64_val".to_string(), json!(2.5)),
            ]
            .into(),
        )
    }

    #[rstest]
    fn test_params_getters() {
        let params = create_test_params();

        assert_eq!(params.get_u64("u64_val"), Some(42));
        assert_eq!(params.get_i64("i64_val"), Some(-100));
        assert_eq!(params.get_usize("usize_val"), Some(5));
        assert_eq!(params.get_str("str_val"), Some("hello"));
        assert_eq!(params.get_bool("bool_val"), Some(true));
        assert_eq!(params.get_f64("f64_val"), Some(2.5));
        assert_eq!(params.get_u64("missing"), None);
        assert_eq!(params.get_i64("missing"), None);
        assert_eq!(params.get_usize("missing"), None);
        assert_eq!(params.get_str("missing"), None);
        assert_eq!(params.get_bool("missing"), None);
        assert_eq!(params.get_f64("missing"), None);
        assert_eq!(params.get_u64("str_val"), None);
        assert_eq!(params.get_str("u64_val"), None);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_params_shared_reads_and_serialization(#[case] materialize: bool) {
        let original = create_test_params();
        let shared = Params::from_shared(original.to_shared_map());
        let cloned = shared.clone();
        let retained = shared.estimated_heap_size();
        let json = serde_json::to_string(&original).unwrap();
        let msgpack = rmp_serde::to_vec_named(&original).unwrap();

        assert!(std::ptr::eq(
            shared.get("str_val").unwrap(),
            cloned.get("str_val").unwrap()
        ));
        assert_eq!(shared, original);
        assert_eq!(format!("{shared:?}"), format!("{original:?}"));
        assert_eq!(shared.len(), 6);
        assert!(!shared.is_empty());
        assert!(shared.contains_key("str_val"));
        assert!(!shared.contains_key("missing"));
        assert_eq!(shared.get_str("str_val"), Some("hello"));
        assert_eq!(shared.get_u64("u64_val"), Some(42));
        assert_eq!(shared.get_i64("i64_val"), Some(-100));
        assert_eq!(shared.get_usize("usize_val"), Some(5));
        assert_eq!(shared.get_bool("bool_val"), Some(true));
        assert_eq!(shared.get_f64("f64_val"), Some(2.5));
        assert_eq!(serde_json::to_string(&shared).unwrap(), json);
        assert_eq!(rmp_serde::to_vec_named(&shared).unwrap(), msgpack);
        assert_eq!(shared.estimated_heap_size(), retained);

        if materialize {
            let map: &IndexMap<String, Value> = &shared;
            let cloned_map: &IndexMap<String, Value> = &cloned;
            assert!(std::ptr::eq(map, cloned_map));
            assert_eq!(map, &*original);
            assert_eq!(shared["str_val"], json!("hello"));
            assert_eq!(
                (&shared).into_iter().collect::<Vec<_>>(),
                original.iter().collect::<Vec<_>>()
            );
        }

        assert_eq!(
            shared.borrowed_entries().collect::<Vec<_>>(),
            original.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            shared.borrowed_entries().rev().collect::<Vec<_>>(),
            original.iter().rev().collect::<Vec<_>>()
        );
        assert_eq!(serde_json::from_str::<Params>(&json).unwrap(), shared);
        assert_eq!(rmp_serde::from_slice::<Params>(&msgpack).unwrap(), shared);
    }

    #[rstest]
    fn test_params_derivation_preserves_original_and_shared_values() {
        let event = Arc::new(json!({"markets": ["first", "second"], "unknown": {"count": 7}}));
        let original = Params::from_shared(
            [
                ("event".to_string(), Arc::clone(&event)),
                ("closed".to_string(), Arc::new(json!(false))),
            ]
            .into(),
        );
        let mut map = original.to_shared_map();
        map.insert("closed".to_string(), Arc::new(json!(true)));
        let derived = Params::from_shared(map);
        let mut owned = IndexMap::clone(&original);
        owned["event"]["markets"][0] = json!("changed");
        let modified = Params::new(owned);

        assert_eq!(original.get_bool("closed"), Some(false));
        assert_eq!(derived.get_bool("closed"), Some(true));
        assert_eq!(original.get("event"), Some(event.as_ref()));
        assert_eq!(derived.get("event"), Some(event.as_ref()));
        assert!(std::ptr::eq(derived.get("event").unwrap(), event.as_ref()));
        assert_eq!(modified["event"]["markets"][0], json!("changed"));
        assert_eq!(original["event"]["markets"][0], json!("first"));
        assert_eq!(derived["event"]["markets"][0], json!("first"));
    }

    #[rstest]
    fn test_params_shared_storage_accounts_for_materialization() {
        let mut key = String::with_capacity(31);
        key.push_str("event");
        let mut text = String::with_capacity(137);
        text.push_str("snapshot");
        let contents = key.capacity() + text.capacity();
        let params = Params::from_shared([(key, Arc::new(Value::String(text)))].into());
        let shared_bytes = params.capacity() * size_of::<(usize, String, Arc<Value>)>()
            + size_of::<(usize, usize, Value)>()
            + contents;
        assert_eq!(params.estimated_heap_size(), shared_bytes);

        let map: &IndexMap<String, Value> = &params;

        let (key, Value::String(value)) = map.get_index(0).unwrap() else {
            unreachable!()
        };

        let owned_bytes = map.capacity() * size_of::<(usize, String, Value)>()
            + key.capacity()
            + value.capacity();
        assert_eq!(params.estimated_heap_size(), shared_bytes + owned_bytes);
    }

    #[rstest]
    fn test_params_empty_and_order_independent_equality() {
        let first = Params::from_shared(
            [
                ("z".to_string(), Arc::new(json!(null))),
                ("a".to_string(), Arc::new(json!([1, {"nested": true}]))),
            ]
            .into(),
        );

        let reverse = Params::new(
            first
                .borrowed_entries()
                .rev()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let empty = Params::from_shared(IndexMap::new());

        assert_eq!(first, reverse);
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            r#"{"z":null,"a":[1,{"nested":true}]}"#
        );
        assert_eq!(empty, Params::empty());
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.get("missing"), None);
        assert_eq!(empty.borrowed_entries().count(), 0);
        assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");
    }

    #[rstest]
    fn test_params_ref_get_usize_respects_target_width() {
        let params = Params::new(
            [
                ("u32_max".to_string(), json!(u32::MAX)),
                ("u32_overflow".to_string(), json!(4_294_967_296_u64)),
            ]
            .into(),
        );

        assert_eq!(params.get_usize("u32_max"), Some(4_294_967_295_usize));
        #[cfg(target_pointer_width = "32")]
        assert_eq!(params.get_usize("u32_overflow"), None);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(params.get_usize("u32_overflow"), Some(4_294_967_296));
    }
}
