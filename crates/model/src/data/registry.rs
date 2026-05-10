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

//! Registries for custom data: JSON (de)serialization and Arrow encode/decode.
//!
//! Mirrors Python's `register_serializable_type` and `register_arrow` in `custom.py`.
//! The registry only stores type name -> callbacks for lookup; each type provides
//! its own deserialize/encode/decode via the trait or registration.

use std::{collections::HashMap, sync::Arc};

use arrow::{datatypes::Schema, record_batch::RecordBatch};
use dashmap::{DashMap, mapref::entry::Entry};
use nautilus_core::Params;
#[cfg(feature = "python")]
use pyo3::types::PyAnyMethods;

use crate::data::{CustomData, CustomDataTrait, Data, DataType};

pub type JsonDeserializer =
    Box<dyn Fn(serde_json::Value) -> Result<Arc<dyn CustomDataTrait>, anyhow::Error> + Send + Sync>;
pub type ArrowEncoder =
    Box<dyn Fn(&[Arc<dyn CustomDataTrait>]) -> Result<RecordBatch, anyhow::Error> + Send + Sync>;
pub type ArrowDecoder = Box<
    dyn Fn(&HashMap<String, String>, RecordBatch) -> Result<Vec<Data>, anyhow::Error> + Send + Sync,
>;

struct Registries {
    json: DashMap<String, JsonDeserializer>,
    arrow: DashMap<String, (Arc<Schema>, ArrowEncoder, ArrowDecoder)>,
}

fn registries() -> &'static Registries {
    static REGISTRIES: std::sync::OnceLock<Registries> = std::sync::OnceLock::new();
    REGISTRIES.get_or_init(|| Registries {
        json: DashMap::new(),
        arrow: DashMap::new(),
    })
}

/// Registers a JSON deserializer for the given custom data type name.
/// When `Data::deserialize` sees this type name, it will call this function.
///
/// # Errors
/// Returns an error if the type is already registered.
pub fn register_json_deserializer(
    type_name: &str,
    deserializer: JsonDeserializer,
) -> Result<(), anyhow::Error> {
    let reg = registries();
    match reg.json.entry(type_name.to_string()) {
        Entry::Occupied(_) => {
            anyhow::bail!("Custom data type \"{type_name}\" is already registered for JSON");
        }
        Entry::Vacant(v) => {
            v.insert(deserializer);
            Ok(())
        }
    }
}

/// Registers a JSON deserializer for the given custom data type name if not already registered.
/// If the type is already registered, returns `Ok(())` without overwriting (idempotent).
/// Use this where repeated registration can occur (e.g. module init).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
pub fn ensure_json_deserializer_registered(
    type_name: &str,
    deserializer: JsonDeserializer,
) -> Result<(), anyhow::Error> {
    let reg = registries();
    reg.json
        .entry(type_name.to_string())
        .or_insert_with(|| deserializer);
    Ok(())
}

/// Parses a "`data_type`" JSON object into `DataType` (`type_name`, metadata, identifier).
fn parse_data_type_from_value(value: &serde_json::Value) -> Option<DataType> {
    let obj = value.get("data_type")?.as_object()?;
    let type_name = obj.get("type_name")?.as_str()?;
    let metadata = obj.get("metadata").and_then(|m| {
        if m.is_null() {
            None
        } else {
            let p: Params = serde_json::from_value(m.clone()).ok()?;
            if p.is_empty() { None } else { Some(p) }
        }
    });
    let identifier = obj
        .get("identifier")
        .and_then(|v| v.as_str())
        .map(String::from);
    Some(DataType::new(type_name, metadata, identifier))
}

/// Parses the canonical `CustomData` JSON envelope `{ type, data_type, payload }` and returns
/// the payload value to pass to the registered type deserializer. Does not depend on
/// user payload field names.
fn parse_envelope_payload(value: &serde_json::Value) -> Result<serde_json::Value, anyhow::Error> {
    let payload = value
        .get("payload")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("CustomData JSON missing 'payload' field"))?;
    Ok(payload)
}

/// Looks up and runs the JSON deserializer for the given type name.
/// Returns `None` if the type is not registered.
///
/// # Errors
/// Returns an error if the deserializer fails.
pub fn deserialize_custom_from_json(
    type_name: &str,
    value: &serde_json::Value,
) -> Result<Option<Data>, anyhow::Error> {
    let reg = registries();
    let deserializer_ref = match reg.json.get(type_name) {
        Some(d) => d,
        None => return Ok(None),
    };
    let data_type = parse_data_type_from_value(value);
    let payload = parse_envelope_payload(value)?;
    let arc = deserializer_ref.value()(payload)?;
    let custom = match data_type {
        Some(dt) => CustomData::new(arc, dt),
        None => CustomData::from_arc(arc),
    };
    Ok(Some(Data::Custom(custom)))
}

/// Registers Arrow schema, encoder, and decoder for the given custom data type name.
///
/// # Errors
/// Returns an error if the type is already registered for Arrow.
pub fn register_arrow(
    type_name: &str,
    schema: Arc<Schema>,
    encoder: ArrowEncoder,
    decoder: ArrowDecoder,
) -> Result<(), anyhow::Error> {
    let reg = registries();
    match reg.arrow.entry(type_name.to_string()) {
        Entry::Occupied(_) => {
            anyhow::bail!("Custom data type \"{type_name}\" is already registered for Arrow");
        }
        Entry::Vacant(v) => {
            v.insert((schema, encoder, decoder));
            Ok(())
        }
    }
}

/// Registers Arrow schema, encoder, and decoder for the given custom data type name if not already
/// registered. If the type is already registered, returns `Ok(())` without overwriting (idempotent).
/// Use this where repeated registration can occur (e.g. module init).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
pub fn ensure_arrow_registered(
    type_name: &str,
    schema: Arc<Schema>,
    encoder: ArrowEncoder,
    decoder: ArrowDecoder,
) -> Result<(), anyhow::Error> {
    let reg = registries();
    reg.arrow
        .entry(type_name.to_string())
        .or_insert_with(|| (schema, encoder, decoder));
    Ok(())
}

/// Returns the Arrow schema for the given custom type name, if registered.
#[must_use]
pub fn get_arrow_schema(type_name: &str) -> Option<Arc<Schema>> {
    let reg = registries();
    reg.arrow
        .get(type_name)
        .map(|entry| Arc::clone(&entry.value().0))
}

/// Encodes a slice of custom data trait objects to a `RecordBatch` using the registered encoder.
///
/// # Errors
/// Returns an error if the type is not registered or encoding fails.
pub fn encode_custom_to_arrow(
    type_name: &str,
    items: &[Arc<dyn CustomDataTrait>],
) -> Result<Option<RecordBatch>, anyhow::Error> {
    let reg = registries();
    let entry = match reg.arrow.get(type_name) {
        Some(e) => e,
        None => return Ok(None),
    };
    let encoder = &entry.value().1;
    encoder(items).map(Some)
}

/// Decodes a `RecordBatch` into `Vec<Data>` using the registered decoder.
///
/// # Errors
/// Returns an error if the type is not registered or decoding fails.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always use the default hasher"
)]
pub fn decode_custom_from_arrow(
    type_name: &str,
    metadata: &HashMap<String, String>,
    record_batch: RecordBatch,
) -> Result<Option<Vec<Data>>, anyhow::Error> {
    let reg = registries();
    let entry = match reg.arrow.get(type_name) {
        Some(e) => e,
        None => return Ok(None),
    };
    let decoder = &entry.value().2;
    decoder(metadata, record_batch).map(Some)
}

#[cfg(feature = "python")]
pub type PyExtractor = Box<
    dyn for<'a> Fn(&pyo3::Bound<'a, pyo3::PyAny>) -> Option<Arc<dyn CustomDataTrait>> + Send + Sync,
>;

#[cfg(feature = "python")]
fn py_extractors() -> &'static DashMap<String, PyExtractor> {
    static PY_EXTRACTORS: std::sync::OnceLock<DashMap<String, PyExtractor>> =
        std::sync::OnceLock::new();
    PY_EXTRACTORS.get_or_init(DashMap::new)
}

/// Registers a `PyExtractor` for the given custom data type name.
/// Used by `CustomData` constructor to convert Python objects to `Arc<dyn CustomDataTrait>`.
///
/// # Errors
/// Returns an error if the type is already registered.
#[cfg(feature = "python")]
pub fn register_py_extractor(type_name: &str, extractor: PyExtractor) -> Result<(), anyhow::Error> {
    let reg = py_extractors();
    match reg.entry(type_name.to_string()) {
        Entry::Occupied(_) => {
            anyhow::bail!(
                "Custom data type \"{type_name}\" is already registered for Python extraction"
            );
        }
        Entry::Vacant(v) => {
            v.insert(extractor);
            Ok(())
        }
    }
}

/// Registers a `PyExtractor` for the given custom data type name if not already registered.
/// If the type is already registered, returns `Ok(())` without overwriting (idempotent).
/// Use this where repeated registration can occur (e.g. module init).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
#[cfg(feature = "python")]
pub fn ensure_py_extractor_registered(
    type_name: &str,
    extractor: PyExtractor,
) -> Result<(), anyhow::Error> {
    let reg = py_extractors();
    reg.entry(type_name.to_string())
        .or_insert_with(|| extractor);
    Ok(())
}

/// Tries to extract `Arc<dyn CustomDataTrait>` from a Python object using the registered extractor.
/// Returns None if no extractor is registered or extraction fails.
#[cfg(feature = "python")]
#[must_use]
pub fn try_extract_from_py(
    type_name: &str,
    obj: &pyo3::Bound<'_, pyo3::PyAny>,
) -> Option<Arc<dyn CustomDataTrait>> {
    let reg = py_extractors();
    let entry = reg.get(type_name)?;
    let extractor = entry.value();
    extractor(obj)
}

#[cfg(feature = "python")]
type RustExtractorFactory = Box<dyn Fn() -> PyExtractor + Send + Sync>;

#[cfg(feature = "python")]
fn rust_extractor_factories() -> &'static DashMap<String, RustExtractorFactory> {
    static RUST_EXTRACTOR_FACTORIES: std::sync::OnceLock<DashMap<String, RustExtractorFactory>> =
        std::sync::OnceLock::new();
    RUST_EXTRACTOR_FACTORIES.get_or_init(DashMap::new)
}

/// Registers a factory that produces a `PyExtractor` for the given type name.
/// Crates (e.g. persistence) call this at load time for each Rust custom data type.
/// When `register_custom_data_class(cls)` is called with that type's class, the factory is invoked
/// and the extractor is registered in the main `PyExtractor` registry.
///
/// # Errors
/// Returns an error if the type name is already registered.
#[cfg(feature = "python")]
pub fn register_rust_extractor_factory(
    type_name: &str,
    factory: RustExtractorFactory,
) -> Result<(), anyhow::Error> {
    let reg = rust_extractor_factories();
    match reg.entry(type_name.to_string()) {
        Entry::Occupied(_) => {
            anyhow::bail!("Rust extractor factory for \"{type_name}\" is already registered");
        }
        Entry::Vacant(v) => {
            v.insert(factory);
            Ok(())
        }
    }
}

/// Registers a factory that produces a `PyExtractor` for the given type name if not already
/// registered. If the type is already registered, returns `Ok(())` without overwriting (idempotent).
/// Use this where repeated registration can occur (e.g. module load).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
#[cfg(feature = "python")]
pub fn ensure_rust_extractor_factory_registered(
    type_name: &str,
    factory: RustExtractorFactory,
) -> Result<(), anyhow::Error> {
    let reg = rust_extractor_factories();
    reg.entry(type_name.to_string()).or_insert_with(|| factory);
    Ok(())
}

/// Registers a Rust custom data type for Python extraction. Call once per type at module load
/// (e.g. in the persistence PyO3 module). Uses [`register_rust_extractor_factory`] with a
/// factory that builds the extractor for `T`.
///
/// # Errors
/// Returns an error if the type name is already registered.
#[cfg(feature = "python")]
pub fn register_rust_extractor<T>() -> Result<(), anyhow::Error>
where
    T: CustomDataTrait + for<'a, 'py> pyo3::FromPyObject<'a, 'py> + Send + Sync + 'static,
{
    let type_name = T::type_name_static();
    let factory: RustExtractorFactory = Box::new(|| {
        Box::new(|obj: &pyo3::Bound<'_, pyo3::PyAny>| {
            obj.extract::<T>()
                .ok()
                .map(|x| Arc::new(x) as Arc<dyn CustomDataTrait>)
        })
    });
    register_rust_extractor_factory(type_name, factory)
}

/// Registers a Rust custom data type for Python extraction if not already registered.
/// If the type is already registered, returns `Ok(())` without overwriting (idempotent).
/// Use this where repeated registration can occur (e.g. module load).
///
/// # Errors
/// Does not return an error (idempotent insert into `DashMap`).
#[cfg(feature = "python")]
pub fn ensure_rust_extractor_registered<T>() -> Result<(), anyhow::Error>
where
    T: CustomDataTrait + for<'a, 'py> pyo3::FromPyObject<'a, 'py> + Send + Sync + 'static,
{
    let type_name = T::type_name_static();
    let factory: RustExtractorFactory = Box::new(|| {
        Box::new(|obj: &pyo3::Bound<'_, pyo3::PyAny>| {
            obj.extract::<T>()
                .ok()
                .map(|x| Arc::new(x) as Arc<dyn CustomDataTrait>)
        })
    });
    ensure_rust_extractor_factory_registered(type_name, factory)
}

/// Calls the registered factory for the given type name and returns the extractor, if any.
#[cfg(feature = "python")]
#[must_use]
pub fn get_rust_extractor(type_name: &str) -> Option<PyExtractor> {
    let reg = rust_extractor_factories();
    let factory_ref = reg.get(type_name)?;
    Some(factory_ref.value()())
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::data::{CustomData, custom::register_custom_data_json};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestRegCustomData {
        ts_init: UnixNanos,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct StrictRegCustomData {
        ts_init: UnixNanos,
    }

    impl crate::data::HasTsInit for TestRegCustomData {
        fn ts_init(&self) -> UnixNanos {
            self.ts_init
        }
    }

    impl crate::data::custom::CustomDataTrait for TestRegCustomData {
        fn type_name(&self) -> &'static str {
            "TestRegCustomData"
        }
        fn type_name_static() -> &'static str {
            "TestRegCustomData"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn ts_event(&self) -> nautilus_core::UnixNanos {
            self.ts_init
        }
        fn to_json(&self) -> anyhow::Result<String> {
            Ok(serde_json::to_string(self)?)
        }
        fn clone_arc(&self) -> Arc<dyn crate::data::CustomDataTrait> {
            Arc::new(self.clone())
        }
        fn eq_arc(&self, other: &dyn crate::data::CustomDataTrait) -> bool {
            other.as_any().downcast_ref::<Self>() == Some(self)
        }
        fn from_json(
            value: serde_json::Value,
        ) -> anyhow::Result<Arc<dyn crate::data::CustomDataTrait>> {
            let t: Self = serde_json::from_value(value)?;
            Ok(Arc::new(t))
        }
    }

    impl crate::data::HasTsInit for StrictRegCustomData {
        fn ts_init(&self) -> UnixNanos {
            self.ts_init
        }
    }

    impl crate::data::custom::CustomDataTrait for StrictRegCustomData {
        fn type_name(&self) -> &'static str {
            "StrictRegCustomData"
        }
        fn type_name_static() -> &'static str {
            "StrictRegCustomData"
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn ts_event(&self) -> nautilus_core::UnixNanos {
            self.ts_init
        }
        fn to_json(&self) -> anyhow::Result<String> {
            Ok(serde_json::to_string(self)?)
        }
        fn clone_arc(&self) -> Arc<dyn crate::data::CustomDataTrait> {
            Arc::new(self.clone())
        }
        fn eq_arc(&self, other: &dyn crate::data::CustomDataTrait) -> bool {
            other.as_any().downcast_ref::<Self>() == Some(self)
        }
        fn from_json(
            value: serde_json::Value,
        ) -> anyhow::Result<Arc<dyn crate::data::CustomDataTrait>> {
            let t: Self = serde_json::from_value(value)?;
            Ok(Arc::new(t))
        }
    }

    #[rstest]
    fn json_registry_roundtrip() {
        let _ = register_custom_data_json::<TestRegCustomData>();

        let data = Data::Custom(CustomData::from_arc(Arc::new(TestRegCustomData {
            ts_init: UnixNanos::from(100),
        })));

        let json = serde_json::to_string(&data).unwrap();
        let back: Data = serde_json::from_str(&json).unwrap();

        match (&data, &back) {
            (Data::Custom(a), Data::Custom(b)) => {
                assert_eq!(a.data.type_name(), b.data.type_name());
                assert_eq!(a.data.ts_init(), b.data.ts_init());
            }
            _ => panic!("expected Custom variant"),
        }
    }

    #[rstest]
    fn json_registry_roundtrip_with_deny_unknown_fields() {
        let _ = register_custom_data_json::<StrictRegCustomData>();

        let data = Data::Custom(CustomData::from_arc(Arc::new(StrictRegCustomData {
            ts_init: UnixNanos::from(200),
        })));

        let json = serde_json::to_string(&data).unwrap();
        let back: Data = serde_json::from_str(&json).unwrap();

        match (&data, &back) {
            (Data::Custom(a), Data::Custom(b)) => {
                assert_eq!(a.data.type_name(), b.data.type_name());
                assert_eq!(a.data.ts_init(), b.data.ts_init());
            }
            _ => panic!("expected Custom variant"),
        }
    }

    #[rstest]
    fn ensure_json_deserializer_registered_is_idempotent() {
        let deserializer: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r1 = ensure_json_deserializer_registered("IdempotentTestJson", deserializer);
        assert!(r1.is_ok(), "first registration should succeed");
        let deserializer2: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r2 = ensure_json_deserializer_registered("IdempotentTestJson", deserializer2);
        assert!(
            r2.is_ok(),
            "second registration with same type_name should succeed (idempotent)"
        );
    }

    #[rstest]
    fn register_json_deserializer_fails_on_duplicate() {
        let deserializer: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r1 = register_json_deserializer("StrictDuplicateTestJson", deserializer);
        assert!(r1.is_ok());
        let deserializer2: JsonDeserializer = Box::new(|value| {
            let t: TestRegCustomData = serde_json::from_value(value)?;
            Ok(Arc::new(t) as Arc<dyn crate::data::CustomDataTrait>)
        });
        let r2 = register_json_deserializer("StrictDuplicateTestJson", deserializer2);
        assert!(r2.is_err());
        let err_msg = r2.unwrap_err().to_string();
        assert!(
            err_msg.contains("already registered"),
            "expected 'already registered' in error, found: {err_msg}"
        );
    }
}
