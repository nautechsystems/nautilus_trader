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

#[cfg(feature = "python")]
use std::collections::HashSet;
#[cfg(feature = "python")]
use std::sync::RwLock;
use std::{any::Any, fmt::Debug, sync::Arc};

use nautilus_core::UnixNanos;
#[cfg(feature = "python")]
use pyo3::{IntoPyObjectExt, prelude::*, types::PyAny};
use serde::{Serialize, Serializer};

use crate::data::{
    Data, DataType, HasTsInit,
    registry::{ensure_json_deserializer_registered, register_json_deserializer},
};

#[cfg(feature = "python")]
fn intern_type_name_static(name: String) -> &'static str {
    static INTERNER: std::sync::OnceLock<RwLock<HashSet<&'static str>>> =
        std::sync::OnceLock::new();
    let set = INTERNER.get_or_init(|| RwLock::new(HashSet::new()));

    if let Ok(guard) = set.read()
        && guard.contains(name.as_str())
    {
        return guard.get(name.as_str()).copied().unwrap();
    }

    if let Ok(mut guard) = set.write() {
        if let Some(&existing) = guard.get(name.as_str()) {
            return existing;
        }
        let leaked: &'static str = Box::leak(name.into_boxed_str());
        guard.insert(leaked);
        leaked
    } else {
        log::warn!("intern_type_name_static: RwLock poisoned, interning skipped for type name");
        Box::leak(name.into_boxed_str())
    }
}

/// Wraps a Python custom data object so it can participate in the Rust data
/// pipeline as an `Arc<dyn CustomDataTrait>`.
///
/// Holds a reference to the Python object and delegates trait methods via the
/// Python GIL. `ts_event`, `ts_init`, and `type_name` are cached at construction
/// to avoid GIL acquisition in the hot path (e.g., data sorting, message routing).
#[cfg(feature = "python")]
pub struct PythonCustomDataWrapper {
    /// The Python object implementing the custom data interface.
    py_object: Py<PyAny>,
    /// Cached `ts_event` value (extracted once at construction).
    cached_ts_event: UnixNanos,
    /// Cached `ts_init` value (extracted once at construction).
    cached_ts_init: UnixNanos,
    /// Cached type name (extracted once at construction).
    cached_type_name: String,
    /// Leaked static string for `type_name()` return (required by trait signature).
    cached_type_name_static: &'static str,
}

#[cfg(feature = "python")]
impl PythonCustomDataWrapper {
    /// Creates a new wrapper from a Python custom data object.
    ///
    /// Extracts and caches `ts_event`, `ts_init`, and the type name from the Python object.
    ///
    /// # Errors
    /// Returns an error if required attributes cannot be extracted from the Python object.
    pub fn new(_py: Python<'_>, py_object: &Bound<'_, PyAny>) -> PyResult<Self> {
        // Extract ts_event
        let ts_event: u64 = py_object.getattr("ts_event")?.extract()?;
        let ts_event = UnixNanos::from(ts_event);

        // Extract ts_init
        let ts_init: u64 = py_object.getattr("ts_init")?.extract()?;
        let ts_init = UnixNanos::from(ts_init);

        // Get type name from class
        let data_class = py_object.get_type();
        let type_name: String = if data_class.hasattr("type_name_static")? {
            data_class.call_method0("type_name_static")?.extract()?
        } else {
            data_class.getattr("__name__")?.extract()?
        };

        // Intern so we only store one static copy per distinct type name
        let type_name_static: &'static str = intern_type_name_static(type_name.clone());

        Ok(Self {
            py_object: py_object.clone().unbind(),
            cached_ts_event: ts_event,
            cached_ts_init: ts_init,
            cached_type_name: type_name,
            cached_type_name_static: type_name_static,
        })
    }

    /// Returns a reference to the underlying Python object.
    #[must_use]
    pub fn py_object(&self) -> &Py<PyAny> {
        &self.py_object
    }

    /// Returns the cached type name.
    #[must_use]
    pub fn get_type_name(&self) -> &str {
        &self.cached_type_name
    }
}

#[cfg(feature = "python")]
impl Clone for PythonCustomDataWrapper {
    fn clone(&self) -> Self {
        Python::attach(|py| Self {
            py_object: self.py_object.clone_ref(py),
            cached_ts_event: self.cached_ts_event,
            cached_ts_init: self.cached_ts_init,
            cached_type_name: self.cached_type_name.clone(),
            cached_type_name_static: self.cached_type_name_static,
        })
    }
}

#[cfg(feature = "python")]
impl Debug for PythonCustomDataWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PythonCustomDataWrapper))
            .field("py_object", &self.py_object)
            .field("type_name", &self.cached_type_name)
            .field("type_name_static", &self.cached_type_name_static)
            .field("ts_event", &self.cached_ts_event)
            .field("ts_init", &self.cached_ts_init)
            .finish()
    }
}

#[cfg(feature = "python")]
impl HasTsInit for PythonCustomDataWrapper {
    fn ts_init(&self) -> UnixNanos {
        self.cached_ts_init
    }
}

#[cfg(feature = "python")]
impl CustomDataTrait for PythonCustomDataWrapper {
    fn type_name(&self) -> &'static str {
        self.cached_type_name_static
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.cached_ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Python::attach(|py| {
            let obj = self.py_object.bind(py);
            // Call to_json() on the Python object if available
            if obj.hasattr("to_json")? {
                let json_str: String = obj.call_method0("to_json")?.extract()?;
                Ok(json_str)
            } else {
                // Fallback: use Python's json module
                let json_module = py.import("json")?;
                // Try to get a dict representation
                let dict = if obj.hasattr("__dict__")? {
                    obj.getattr("__dict__")?
                } else {
                    anyhow::bail!("Python object has no to_json() method or __dict__ attribute");
                };
                let json_str: String = json_module.call_method1("dumps", (dict,))?.extract()?;
                Ok(json_str)
            }
        })
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        // Equality by Python object identity only, to avoid false equality when two
        // distinct Python objects share the same type name and timestamps.
        if let Some(other_wrapper) = other.as_any().downcast_ref::<Self>() {
            Python::attach(|py| {
                let a = self.py_object.bind(py);
                let b = other_wrapper.py_object.bind(py);
                if a.is(b) {
                    return true;
                }
                a.eq(b).unwrap_or(false)
            })
        } else {
            false
        }
    }

    fn to_pyobject(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        // Return the underlying Python object directly
        Ok(self.py_object.clone_ref(py))
    }
}

#[cfg(feature = "python")]
fn python_data_classes() -> &'static dashmap::DashMap<String, Py<PyAny>> {
    static PYTHON_DATA_CLASSES: std::sync::OnceLock<dashmap::DashMap<String, Py<PyAny>>> =
        std::sync::OnceLock::new();
    PYTHON_DATA_CLASSES.get_or_init(dashmap::DashMap::new)
}

#[cfg(feature = "python")]
pub fn register_python_data_class(type_name: &str, data_class: &Bound<'_, PyAny>) {
    python_data_classes().insert(type_name.to_string(), data_class.clone().unbind());
}

#[cfg(feature = "python")]
#[must_use]
pub fn get_python_data_class(py: Python<'_>, type_name: &str) -> Option<Py<PyAny>> {
    python_data_classes()
        .get(type_name)
        .map(|entry| entry.value().clone_ref(py))
}

/// Reconstructs a Python custom data instance from type name and JSON.
///
/// # Errors
///
/// Returns a Python error if no class is registered for `type_name` or JSON parsing fails.
#[cfg(feature = "python")]
pub fn reconstruct_python_custom_data(
    py: Python<'_>,
    type_name: &str,
    json: &str,
) -> PyResult<Py<PyAny>> {
    let data_class = get_python_data_class(py, type_name).ok_or_else(|| {
        nautilus_core::python::to_pyruntime_err(format!(
            "No registered Python class for custom data type `{type_name}`"
        ))
    })?;
    let json_module = py.import("json")?;
    let payload = json_module.call_method1("loads", (json,))?;
    data_class
        .bind(py)
        .call_method1("from_json", (payload,))
        .map(|obj| obj.unbind())
}

/// Converts a cloneable PyO3-backed custom data value into a Python object.
///
/// This is intended for `#[pyclass]` custom data types, where PyO3 already
/// provides `IntoPyObject` for owned values.
///
/// # Errors
///
/// Returns any conversion error reported by PyO3.
#[cfg(feature = "python")]
pub fn clone_pyclass_to_pyobject<T>(value: &T, py: Python<'_>) -> PyResult<Py<PyAny>>
where
    T: Clone,
    for<'py> T: pyo3::IntoPyObject<'py, Error = pyo3::PyErr>,
{
    value.clone().into_py_any(py)
}

/// Trait for typed custom data that can be used within the Nautilus domain model.
pub trait CustomDataTrait: HasTsInit + Send + Sync + Debug {
    /// Returns the type name for the custom data.
    fn type_name(&self) -> &'static str;

    /// Returns the data as a `dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns the event timestamp (when the data occurred).
    fn ts_event(&self) -> UnixNanos;

    /// Serializes the custom data to a JSON string.
    ///
    /// # Errors
    /// Returns an error if JSON serialization fails.
    fn to_json(&self) -> anyhow::Result<String>;

    /// Python-facing JSON serialization. Default implementation forwards to `to_json`.
    /// Override if a different behavior is needed for the Python API.
    ///
    /// # Errors
    /// Returns an error if JSON serialization fails.
    fn to_json_py(&self) -> anyhow::Result<String> {
        self.to_json()
    }

    /// Returns a cloned Arc of the custom data.
    fn clone_arc(&self) -> Arc<dyn CustomDataTrait>;

    /// Returns whether the custom data is equal to another.
    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool;

    /// Converts the custom data to a Python object.
    ///
    /// # Errors
    /// Returns an error if PyO3 conversion fails.
    #[cfg(feature = "python")]
    fn to_pyobject(&self, _py: Python<'_>) -> PyResult<Py<PyAny>> {
        Err(nautilus_core::python::to_pytype_err(format!(
            "to_pyobject not implemented for {}",
            self.type_name()
        )))
    }

    /// Returns the type name used in serialized form (e.g. in the `"type"` field).
    #[must_use]
    fn type_name_static() -> &'static str
    where
        Self: Sized,
    {
        std::any::type_name::<Self>()
    }

    /// Deserializes from a JSON value into an Arc'd trait object.
    ///
    /// # Errors
    /// Returns an error if JSON deserialization fails.
    fn from_json(_value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>>
    where
        Self: Sized,
    {
        anyhow::bail!(
            "from_json not implemented for {}",
            std::any::type_name::<Self>()
        )
    }
}

/// Registers a custom data type for JSON deserialization. When `Data::deserialize`
/// sees the type name returned by `T::type_name_static()`, it will call `T::from_json`.
///
/// # Errors
/// Returns an error if the type is already registered.
pub fn register_custom_data_json<T: CustomDataTrait + Sized>() -> anyhow::Result<()> {
    let type_name = T::type_name_static();
    register_json_deserializer(type_name, Box::new(|value| T::from_json(value)))
}

/// Registers a custom data type for JSON deserialization if not already registered.
/// Idempotent: safe to call multiple times for the same type (e.g. module init).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
pub fn ensure_custom_data_json_registered<T: CustomDataTrait + Sized>() -> anyhow::Result<()> {
    let type_name = T::type_name_static();
    ensure_json_deserializer_registered(type_name, Box::new(|value| T::from_json(value)))
}

/// A wrapper for custom data including its data type.
///
/// The `data` field holds an [`Arc`] to a [`CustomDataTrait`] implementation,
/// enabling cheap cloning when passing to Python (Arc clone is O(1)).
/// Custom data is always Rust-defined (optionally with PyO3 bindings).
#[cfg_attr(
    feature = "python",
    pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.model",
        name = "CustomData",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
#[derive(Clone, Debug)]
pub struct CustomData {
    /// The actual data object implementing [`CustomDataTrait`].
    pub data: Arc<dyn CustomDataTrait>,
    /// The data type metadata.
    pub data_type: DataType,
}

impl CustomData {
    /// Creates a new [`CustomData`] instance from an [`Arc`]'d [`CustomDataTrait`],
    /// deriving the data type from the inner type name.
    pub fn from_arc(arc: Arc<dyn CustomDataTrait>) -> Self {
        let data_type = DataType::new(arc.type_name(), None, None);
        Self {
            data: arc,
            data_type,
        }
    }

    /// Creates a new [`CustomData`] instance with explicit data type metadata.
    ///
    /// Use this when the data type must come from external metadata (e.g. Parquet),
    /// rather than being derived from the inner type name.
    pub fn new(data: Arc<dyn CustomDataTrait>, data_type: DataType) -> Self {
        Self { data, data_type }
    }
}

impl PartialEq for CustomData {
    fn eq(&self, other: &Self) -> bool {
        self.data.eq_arc(other.data.as_ref()) && self.data_type == other.data_type
    }
}

impl HasTsInit for CustomData {
    fn ts_init(&self) -> UnixNanos {
        self.data.ts_init()
    }
}

pub(crate) fn parse_custom_data_from_json_bytes(
    bytes: &[u8],
) -> Result<CustomData, serde_json::Error> {
    let data: Data = serde_json::from_slice(bytes)?;
    match data {
        Data::Custom(custom) => Ok(custom),
        _ => Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON does not represent CustomData",
        ))),
    }
}

impl CustomData {
    /// Deserializes `CustomData` from JSON bytes (full `CustomData` format with type and `data_type`).
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes are not valid JSON or do not represent `CustomData`.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        parse_custom_data_from_json_bytes(bytes)
    }
}

/// Canonical JSON envelope for `CustomData`. All serialized `CustomData` uses this shape so
/// deserialization can extract the payload without depending on user payload field names.
struct CustomDataEnvelope {
    type_name: String,
    data_type: serde_json::Value,
    payload: serde_json::Value,
}

impl Serialize for CustomDataEnvelope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CustomDataEnvelope", 3)?;
        state.serialize_field("type", &self.type_name)?;
        state.serialize_field("data_type", &self.data_type)?;
        state.serialize_field("payload", &self.payload)?;
        state.end()
    }
}

impl CustomData {
    fn to_envelope_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        let json = self.data.to_json().map_err(|e| {
            serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        let payload: serde_json::Value = serde_json::from_str(&json)?;
        let metadata_value = self.data_type.metadata().map_or(
            serde_json::Value::Object(serde_json::Map::new()),
            |m| {
                serde_json::to_value(m).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
            },
        );
        let mut data_type_obj = serde_json::Map::new();
        data_type_obj.insert(
            "type_name".to_string(),
            serde_json::Value::String(self.data_type.type_name().to_string()),
        );
        data_type_obj.insert("metadata".to_string(), metadata_value);

        if let Some(id) = self.data_type.identifier() {
            data_type_obj.insert(
                "identifier".to_string(),
                serde_json::Value::String(id.to_string()),
            );
        }

        let envelope = CustomDataEnvelope {
            type_name: self.data.type_name().to_string(),
            data_type: serde_json::Value::Object(data_type_obj),
            payload,
        };
        serde_json::to_value(envelope)
    }
}

impl Serialize for CustomData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = self
            .to_envelope_json_value()
            .map_err(serde::ser::Error::custom)?;
        value.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{Params, UnixNanos};
    use rstest::rstest;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::{data::HasTsInit, identifiers::InstrumentId};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestCustomData {
        ts_init: UnixNanos,
        instrument_id: InstrumentId,
    }

    impl HasTsInit for TestCustomData {
        fn ts_init(&self) -> UnixNanos {
            self.ts_init
        }
    }

    impl CustomDataTrait for TestCustomData {
        fn type_name(&self) -> &'static str {
            "TestCustomData"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn ts_event(&self) -> UnixNanos {
            self.ts_init
        }
        fn to_json(&self) -> anyhow::Result<String> {
            Ok(serde_json::to_string(self)?)
        }
        fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
            Arc::new(self.clone())
        }
        fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
            if let Some(other) = other.as_any().downcast_ref::<Self>() {
                self == other
            } else {
                false
            }
        }

        fn type_name_static() -> &'static str {
            "TestCustomData"
        }

        fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
            let parsed: Self = serde_json::from_value(value)?;
            Ok(Arc::new(parsed))
        }
    }

    #[rstest]
    fn test_custom_data_json_roundtrip() {
        register_custom_data_json::<TestCustomData>()
            .expect("TestCustomData must register for JSON roundtrip test");

        let instrument_id = InstrumentId::from("TEST.SIM");
        let metadata = Some(
            serde_json::from_value::<Params>(json!({"key1": "value1", "key2": "value2"})).unwrap(),
        );
        let inner = TestCustomData {
            ts_init: UnixNanos::from(100),
            instrument_id,
        };
        let data_type = DataType::new("TestCustomData", metadata, Some(instrument_id.to_string()));
        let original = CustomData::new(Arc::new(inner), data_type);

        let json_bytes = serde_json::to_vec(&original).unwrap();
        let roundtripped = CustomData::from_json_bytes(&json_bytes).unwrap();

        assert_eq!(
            roundtripped.data_type.type_name(),
            original.data_type.type_name()
        );
        assert_eq!(
            roundtripped.data_type.metadata(),
            original.data_type.metadata()
        );
        assert_eq!(
            roundtripped.data_type.identifier(),
            original.data_type.identifier()
        );
        let orig_inner = original
            .data
            .as_any()
            .downcast_ref::<TestCustomData>()
            .unwrap();
        let rt_inner = roundtripped
            .data
            .as_any()
            .downcast_ref::<TestCustomData>()
            .unwrap();
        assert_eq!(orig_inner, rt_inner);
    }

    #[rstest]
    fn test_custom_data_wrapper() {
        let instrument_id = InstrumentId::from("TEST.SIM");
        let data = TestCustomData {
            ts_init: UnixNanos::from(100),
            instrument_id,
        };
        let data_type = DataType::new("TestCustomData", None, Some(instrument_id.to_string()));
        let custom_data = CustomData::new(Arc::new(data), data_type);

        assert_eq!(custom_data.data.ts_init(), UnixNanos::from(100));
        assert_eq!(Data::Custom(custom_data).instrument_id(), instrument_id);
    }
}
