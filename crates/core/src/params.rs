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

//! Generic JSON parameter storage with shared values.
//!
//! This module provides a centralized definition of [`Params`] as a generic storage
//! solution for `serde_json::Value` data, along with Python bindings.

use std::{
    fmt::Debug,
    hash::Hash,
    ops::{Deref, DerefMut, Index, IndexMut},
    sync::{Arc, OnceLock},
};

use indexmap::{Equivalent, IndexMap};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// Generic parameter storage.
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
/// Values are shared across clones. Borrow them through `get` or the typed getters,
/// and use `get_mut` for copy-on-write access. The dereferenced `IndexMap` and its
/// iterators retain their owned `Value` API, materializing values when accessed.
/// Serialization preserves the values, but does not preserve sharing.
#[derive(Default)]
pub struct Params {
    // Both maps may be cached, but mutation must invalidate the other representation
    shared: OnceLock<IndexMap<String, Arc<Value>>>,
    owned: OnceLock<IndexMap<String, Value>>,
}

impl Params {
    /// Creates an empty `Params` map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates `Params` from an `IndexMap`.
    #[must_use]
    pub fn from_index_map(map: IndexMap<String, Value>) -> Self {
        Self {
            owned: map.into(),
            ..Self::default()
        }
    }

    /// Returns the number of parameters without materializing values.
    #[must_use]
    pub fn len(&self) -> usize {
        match self.shared.get() {
            Some(values) => values.len(),
            None => self.owned.get().map_or(0, IndexMap::len),
        }
    }

    /// Returns whether the map is empty without materializing values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns whether the map contains a key without materializing values.
    #[must_use]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        Q: ?Sized + Hash + Equivalent<String>,
    {
        self.get(key).is_some()
    }

    /// Estimates retained heap storage in bytes without materializing values.
    ///
    /// Counts both cached maps and charges shared allocations in full for each entry.
    /// Excludes allocator and hash-table overhead.
    #[must_use]
    pub fn estimated_heap_size(&self) -> usize {
        let owned = self.owned.get().map_or(0, |values| {
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
        });

        self.shared.get().map_or(owned, |values| {
            values.iter().fold(
                owned.saturating_add(values.capacity().saturating_mul(size_of::<(
                    usize,
                    String,
                    Arc<Value>,
                )>())),
                |bytes, (key, value)| {
                    bytes
                        .saturating_add(key.capacity())
                        .saturating_add(size_of::<(usize, usize, Value)>())
                        .saturating_add(json_heap_size(value))
                },
            )
        })
    }

    /// Inserts a JSON value, returning the previous value if present.
    pub fn insert(&mut self, key: String, value: Value) -> Option<Value> {
        self.insert_shared(key, Arc::new(value))
            .map(Arc::unwrap_or_clone)
    }

    /// Inserts a shared JSON value without copying its contents.
    pub fn insert_shared(&mut self, key: String, value: Arc<Value>) -> Option<Arc<Value>> {
        self.shared_values_mut().insert(key, value)
    }

    /// Returns a borrowed JSON value.
    #[must_use]
    pub fn get<Q>(&self, key: &Q) -> Option<&Value>
    where
        Q: ?Sized + Hash + Equivalent<String>,
    {
        match self.shared.get() {
            Some(values) => values.get(key).map(AsRef::as_ref),
            None => self.owned.get()?.get(key),
        }
    }

    /// Returns a shared JSON handle, which can be cloned without copying the value.
    #[must_use]
    pub fn get_shared(&self, key: &str) -> Option<&Arc<Value>> {
        self.shared_values().get(key)
    }

    /// Returns a mutable JSON value, copying it first if other handles share it.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut Value>
    where
        Q: ?Sized + Hash + Equivalent<String>,
    {
        self.shared_values_mut().get_mut(key).map(Arc::make_mut)
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

    pub(crate) fn shared_values(&self) -> &IndexMap<String, Arc<Value>> {
        self.shared.get_or_init(|| {
            self.owned
                .get()
                .into_iter()
                .flatten()
                .map(|(key, value)| (key.clone(), Arc::new(value.clone())))
                .collect()
        })
    }

    fn shared_values_mut(&mut self) -> &mut IndexMap<String, Arc<Value>> {
        if self.shared.get().is_none() {
            self.shared = self
                .owned
                .take()
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, Arc::new(value)))
                .collect::<IndexMap<_, _>>()
                .into();
        }

        self.owned = OnceLock::new();
        self.shared.get_mut().unwrap()
    }
}

impl Clone for Params {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared_values().clone().into(),
            ..Self::default()
        }
    }
}

impl Debug for Params {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(stringify!(Params))
            .field(self.shared_values())
            .finish()
    }
}

impl PartialEq for Params {
    fn eq(&self, other: &Self) -> bool {
        self.shared_values() == other.shared_values()
    }
}

impl Eq for Params {}

impl Deref for Params {
    type Target = IndexMap<String, Value>;

    fn deref(&self) -> &Self::Target {
        self.owned.get_or_init(|| {
            self.shared_values()
                .iter()
                .map(|(key, value)| (key.clone(), value.as_ref().clone()))
                .collect()
        })
    }
}

impl DerefMut for Params {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.owned.get().is_none() {
            self.owned = self
                .shared
                .take()
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, Arc::unwrap_or_clone(value)))
                .collect::<IndexMap<_, _>>()
                .into();
        }

        self.shared = OnceLock::new();
        self.owned.get_mut().unwrap()
    }
}

impl<I> Index<I> for Params
where
    IndexMap<String, Arc<Value>>: Index<I, Output = Arc<Value>>,
{
    type Output = Value;

    fn index(&self, index: I) -> &Self::Output {
        &self.shared_values()[index]
    }
}

impl<I> IndexMut<I> for Params
where
    IndexMap<String, Arc<Value>>: IndexMut<I, Output = Arc<Value>>,
{
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        Arc::make_mut(&mut self.shared_values_mut()[index])
    }
}

impl<'a> IntoIterator for &'a Params {
    type Item = (&'a String, &'a Value);
    type IntoIter = indexmap::map::Iter<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.deref().iter()
    }
}

impl Serialize for Params {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.shared.get() {
            Some(values) => values.serialize(serializer),
            None => self.deref().serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Params {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self {
            shared: IndexMap::deserialize(deserializer)?.into(),
            ..Self::default()
        })
    }
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
    use std::sync::Arc;

    use indexmap::IndexMap;
    use rstest::*;
    use serde_json::{Value, json};

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
    fn test_params_queries_do_not_materialize_values(#[values(false, true)] shared: bool) {
        let mut params = Params::new();
        assert_eq!(params.len(), 0);
        assert!(params.is_empty());
        assert!(!params.contains_key("missing"));
        assert_eq!(params.estimated_heap_size(), 0);
        assert!(params.shared.get().is_none());
        assert!(params.owned.get().is_none());

        params.insert(
            "payload".to_string(),
            json!({"markets": [{"id": "market"}]}),
        );

        if !shared {
            let _: &mut IndexMap<String, Value> = &mut params;
        }

        assert_eq!(params.len(), 1);
        assert!(!params.is_empty());
        assert!(params.contains_key::<str>("payload"));
        assert!(params.contains_key(&"payload".to_string()));
        assert!(!params.contains_key("missing"));
        assert_eq!(params.shared.get().is_some(), shared);
        assert_eq!(params.owned.get().is_some(), !shared);
    }

    #[rstest]
    fn test_params_heap_size_includes_shared_and_cached_storage() {
        let mut key = String::with_capacity(41);
        key.push_str("key");
        let mut value = String::with_capacity(83);
        value.push_str("payload");
        let contents = key.capacity() + value.capacity();
        let mut params = Params::new();
        params.insert(key, Value::String(value));
        let shared = params.shared.get().unwrap();
        let shared_bytes = shared.capacity() * size_of::<(usize, String, Arc<Value>)>()
            + size_of::<(usize, usize, Value)>()
            + contents;

        assert_eq!(params.estimated_heap_size(), shared_bytes);
        assert!(params.owned.get().is_none());

        let owned: &IndexMap<String, Value> = &params;
        let (key, Value::String(value)) = owned.first().unwrap() else {
            panic!("Expected a string value");
        };
        let owned_bytes = owned.capacity() * size_of::<(usize, String, Value)>()
            + key.capacity()
            + value.capacity();
        assert_eq!(params.estimated_heap_size(), shared_bytes + owned_bytes);
    }

    #[rstest]
    fn test_cloned_params_share_values_until_mutated() {
        let event = Arc::new(json!({"markets": [{"id": "original"}]}));
        let mut params = Params::new();
        params.insert_shared("event".to_string(), Arc::clone(&event));
        let mut cloned = params.clone();

        assert!(Arc::ptr_eq(cloned.get_shared("event").unwrap(), &event));

        params.get_mut("event").unwrap()["markets"][0]["id"] = json!("changed");

        assert_eq!(params.get("event").unwrap()["markets"][0]["id"], "changed");
        assert_eq!(cloned.get("event").unwrap()["markets"][0]["id"], "original");
        assert_eq!(event["markets"][0]["id"], "original");

        cloned["event"]["markets"][0]["id"] = json!("clone changed");
        assert_eq!(cloned["event"]["markets"][0]["id"], "clone changed");
        assert_eq!(params["event"]["markets"][0]["id"], "changed");
        assert_eq!(event["markets"][0]["id"], "original");
    }

    #[rstest]
    fn test_shared_params_preserve_owned_map_operations() {
        let mut params = Params::new();
        params.insert("name".to_string(), json!("original"));
        let original = params.clone();
        assert_eq!(
            params
                .values()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            ["original"],
        );

        *params.entry("name".to_string()).or_insert(Value::Null) = json!("updated");
        assert_eq!(params.get::<str>("name"), Some(&json!("updated")));
        assert_eq!(original.get_str("name"), Some("original"));
        assert_eq!(
            serde_json::to_value(&params).unwrap(),
            json!({"name": "updated"}),
        );
        let updated = params.clone();
        assert!(Arc::ptr_eq(
            params.get_shared("name").unwrap(),
            updated.get_shared("name").unwrap(),
        ));

        let previous: Option<Value> = params.insert("name".to_string(), json!("inserted"));
        assert_eq!(previous, Some(json!("updated")));
        assert_eq!(updated.get_str("name"), Some("updated"));
        params.entry("count".to_string()).or_insert(json!(0));
        *params.get_mut::<str>("count").unwrap() = json!(1);

        let values: &IndexMap<String, Value> = &params;
        assert_eq!(values["name"], "inserted");
        assert_eq!(values["count"], 1);
        let entries: indexmap::map::Iter<'_, String, Value> = (&params).into_iter();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            serde_json::to_value(&params).unwrap(),
            json!({"name": "inserted", "count": 1}),
        );
    }

    #[rstest]
    fn test_shared_params_preserve_serialization_and_order(#[values(false, true)] shared: bool) {
        let original = IndexMap::from([
            ("z".to_string(), json!({"markets": [{"id": "market"}]})),
            ("a".to_string(), json!([null, true, 42, "text"])),
        ]);
        let params = Params::from_index_map(original.clone());

        if shared {
            assert!(params.get_shared("z").is_some());
        }

        let json = serde_json::to_string(&params).unwrap();
        let msgpack = rmp_serde::to_vec_named(&params).unwrap();

        assert_eq!(json, serde_json::to_string(&original).unwrap());
        assert_eq!(msgpack, rmp_serde::to_vec_named(&original).unwrap());
        assert_eq!(serde_json::from_str::<Params>(&json).unwrap(), params);
        assert_eq!(rmp_serde::from_slice::<Params>(&msgpack).unwrap(), params);
    }

    #[rstest]
    fn test_params_ref_get_usize_respects_target_width() {
        let mut params = Params::new();
        params.insert("u32_max".to_string(), json!(u32::MAX));
        params.insert("u32_overflow".to_string(), json!(4_294_967_296_u64));

        assert_eq!(params.get_usize("u32_max"), Some(4_294_967_295_usize));
        #[cfg(target_pointer_width = "32")]
        assert_eq!(params.get_usize("u32_overflow"), None);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(params.get_usize("u32_overflow"), Some(4_294_967_296));
    }
}
