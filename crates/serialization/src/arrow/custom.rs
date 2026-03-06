// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this code except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Custom data: registration and dynamic decoding.
//!
//! - **Registration:** Call [`ensure_custom_data_registered::<T>()`] once (e.g. before using the
//!   catalog) for each custom data type `T` produced by the `#[custom_data]` macro. For Python
//!   bindings, also call [`nautilus_model::data::register_rust_extractor::<T>()`].
//! - **Decoder:** [`CustomDataDecoder`] provides [`ArrowSchemaProvider`] and
//!   [`DecodeDataFromRecordBatch`] for Parquet-backed custom data decoded at runtime by type name.
//!   Types must be registered via [`ensure_custom_data_registered::<T>()`] before use.

use std::sync::Arc;

#[cfg(feature = "python")]
use abi_stable::std_types::{RResult, RString, RVec};
use arrow::record_batch::RecordBatch;
#[cfg(feature = "python")]
use nautilus_model::data::plugin::{
    ExternalCustomDataHandle, ExternalCustomDataPlugin, PluginMetadataEntry, err_value, ok,
    record_batch_from_ipc_bytes, record_batch_to_ipc_bytes, schema_to_ipc_bytes,
};
use nautilus_model::data::{
    ArrowDecoder, ArrowEncoder, CustomData, CustomDataTrait, Data, DataType,
    decode_custom_from_arrow, ensure_arrow_registered, ensure_custom_data_json_registered,
    get_arrow_schema,
};
#[cfg(feature = "python")]
use pyo3::prelude::*;

use super::{ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch};

// ---------------------------------------------------------------------------
// CustomDataSerialize - Arrow/JSON serialization for custom data (serialization crate)
// ---------------------------------------------------------------------------

/// Trait for custom data types that support Arrow schema and record batch encoding.
/// Used as a type bound by the `#[custom_data]` macro; catalog encoding goes through
/// the registry, not this trait directly.
///
/// Implemented by the `#[custom_data]` macro for Rust custom data types. Python custom
/// types use the registry encoder registered by `register_custom_data_class` instead.
pub trait CustomDataSerialize: CustomDataTrait {
    /// Returns the Arrow schema for this custom data type.
    ///
    /// # Errors
    /// Returns an error if schema construction fails.
    fn schema(&self) -> anyhow::Result<arrow::datatypes::Schema>;

    /// Encodes a batch of custom data items to an Arrow RecordBatch.
    ///
    /// # Errors
    /// Returns an error if encoding fails (e.g. type mismatch or Arrow error).
    fn encode_record_batch(
        &self,
        items: &[Arc<dyn CustomDataTrait>],
    ) -> anyhow::Result<RecordBatch>;
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Registers a custom data type in the JSON and Arrow registries. Call once per type
/// (e.g. at catalog decode or before querying custom data).
///
/// Each distinct type `T` is registered at most once (per process). Safe to call
/// multiple times for the same `T`.
///
/// For types exposed to Python, also call [`nautilus_model::data::register_rust_extractor::<T>()`].
pub fn ensure_custom_data_registered<T>()
where
    T: CustomDataTrait
        + ArrowSchemaProvider
        + EncodeToRecordBatch
        + DecodeDataFromRecordBatch
        + Clone
        + Send
        + Sync
        + 'static,
{
    let type_name = T::type_name_static();

    // Skip if already registered
    if get_arrow_schema(type_name).is_some() {
        return;
    }

    let _ = ensure_custom_data_json_registered::<T>();

    let schema = Arc::new(T::get_schema(None));

    let encoder: ArrowEncoder = Box::new(|items: &[Arc<dyn CustomDataTrait>]| {
        let typed: Result<Vec<T>, _> = items
            .iter()
            .map(|b| {
                b.as_any()
                    .downcast_ref::<T>()
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Expected {}", T::type_name_static()))
            })
            .collect();
        let typed = typed?;
        let metadata = typed
            .first()
            .map(EncodeToRecordBatch::metadata)
            .unwrap_or_default();
        EncodeToRecordBatch::encode_batch(&metadata, &typed).map_err(|e| anyhow::anyhow!("{e}"))
    });

    let decoder: ArrowDecoder = Box::new(|metadata, batch| {
        T::decode_data_batch(metadata, batch).map_err(|e| anyhow::anyhow!("{e}"))
    });

    let _ = ensure_arrow_registered(type_name, schema, encoder, decoder);
}

#[cfg(feature = "python")]
extern "C" fn plugin_type_name_impl<T>() -> RString
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    RString::from(T::type_name_static())
}

#[cfg(feature = "python")]
extern "C" fn plugin_schema_ipc_impl<T>() -> RResult<RVec<u8>, RString>
where
    T: CustomDataTrait + ArrowSchemaProvider + Clone + Send + Sync + 'static,
{
    match schema_to_ipc_bytes(&T::get_schema(None)) {
        Ok(bytes) => ok(RVec::from(bytes)),
        Err(e) => err_value(e),
    }
}

#[cfg(feature = "python")]
extern "C" fn plugin_from_json_impl<T>(json: RString) -> RResult<ExternalCustomDataHandle, RString>
where
    T: CustomDataTrait + Clone + Send + Sync + 'static,
{
    let value = match serde_json::from_str::<serde_json::Value>(&json.into_string()) {
        Ok(value) => value,
        Err(e) => return err_value(e),
    };
    let arc = match T::from_json(value) {
        Ok(arc) => arc,
        Err(e) => return err_value(e),
    };
    match arc.as_any().downcast_ref::<T>() {
        Some(value) => ok(ExternalCustomDataHandle::new(value.clone())),
        None => err_value(format!(
            "from_json downcast failed for {}",
            T::type_name_static()
        )),
    }
}

#[cfg(feature = "python")]
extern "C" fn plugin_encode_batch_impl<T>(
    handles: RVec<ExternalCustomDataHandle>,
) -> RResult<RVec<u8>, RString>
where
    T: CustomDataTrait + ArrowSchemaProvider + EncodeToRecordBatch + Clone + Send + Sync + 'static,
{
    let typed: anyhow::Result<Vec<T>> = handles
        .iter()
        .map(ExternalCustomDataHandle::try_clone_as::<T>)
        .collect();
    let typed: Vec<T> = match typed {
        Ok(typed) => typed,
        Err(e) => return err_value(e),
    };
    let metadata = typed
        .first()
        .map(EncodeToRecordBatch::metadata)
        .unwrap_or_default();
    let batch = match EncodeToRecordBatch::encode_batch(&metadata, &typed) {
        Ok(batch) => batch,
        Err(e) => return err_value(e),
    };
    match record_batch_to_ipc_bytes(&batch) {
        Ok(bytes) => ok(RVec::from(bytes)),
        Err(e) => err_value(e),
    }
}

#[cfg(feature = "python")]
extern "C" fn plugin_decode_batch_impl<T>(
    metadata: RVec<PluginMetadataEntry>,
    bytes: RVec<u8>,
) -> RResult<RVec<ExternalCustomDataHandle>, RString>
where
    T: CustomDataTrait + DecodeDataFromRecordBatch + Clone + Send + Sync + 'static,
{
    let metadata: std::collections::HashMap<String, String> = metadata
        .into_iter()
        .map(|entry| (entry.key.into_string(), entry.value.into_string()))
        .collect();
    let batch = match record_batch_from_ipc_bytes(bytes.as_slice()) {
        Ok(batch) => batch,
        Err(e) => return err_value(e),
    };
    let decoded = match T::decode_data_batch(&metadata, batch) {
        Ok(items) => items,
        Err(e) => return err_value(e),
    };
    let mut handles = Vec::with_capacity(decoded.len());
    for item in decoded {
        match item {
            Data::Custom(custom) => match custom.data.as_any().downcast_ref::<T>() {
                Some(value) => handles.push(ExternalCustomDataHandle::new(value.clone())),
                None => {
                    return err_value(format!(
                        "Decoded custom data was not {}",
                        T::type_name_static()
                    ));
                }
            },
            _ => return err_value("Expected Data::Custom variant"),
        }
    }
    ok(RVec::from(handles))
}

#[cfg(feature = "python")]
pub fn external_custom_data_plugin<T>() -> ExternalCustomDataPlugin
where
    T: CustomDataTrait
        + ArrowSchemaProvider
        + EncodeToRecordBatch
        + DecodeDataFromRecordBatch
        + Clone
        + Send
        + Sync
        + 'static,
{
    ExternalCustomDataPlugin {
        abi_version: ExternalCustomDataPlugin::ABI_VERSION,
        type_name: plugin_type_name_impl::<T>,
        schema_ipc: plugin_schema_ipc_impl::<T>,
        from_json: plugin_from_json_impl::<T>,
        encode_batch: plugin_encode_batch_impl::<T>,
        decode_batch: plugin_decode_batch_impl::<T>,
    }
}

/// Wraps the plugin for type `T` in a PyCapsule.
///
/// # Errors
///
/// Returns a Python error if capsule creation fails.
#[cfg(feature = "python")]
pub fn custom_data_plugin_capsule<T>(py: Python<'_>) -> PyResult<Py<PyAny>>
where
    T: CustomDataTrait
        + ArrowSchemaProvider
        + EncodeToRecordBatch
        + DecodeDataFromRecordBatch
        + Clone
        + Send
        + Sync
        + 'static,
{
    let plugin = external_custom_data_plugin::<T>();
    pyo3::types::PyCapsule::new_with_destructor(py, plugin, None, |_, _| {})
        .map(|capsule| capsule.into_any().unbind())
}

// ---------------------------------------------------------------------------
// Decoder (Python feature: used by catalog/session for dynamic custom data)
// ---------------------------------------------------------------------------

/// Decoder for custom data types that are identified at runtime by metadata (e.g. `type_name`).
///
/// Only Rust-registered custom types (e.g. `RustTestCustomData`, `MacroYieldCurveData`) can be
/// decoded. Unknown types return an error.
///
/// **Important:** The caller must ensure that any Rust custom data types are registered
/// via [`ensure_custom_data_registered::<T>()`] before use.
#[derive(Debug)]
pub struct CustomDataDecoder;

impl ArrowSchemaProvider for CustomDataDecoder {
    fn get_schema(
        metadata: Option<std::collections::HashMap<String, String>>,
    ) -> arrow::datatypes::Schema {
        if let Some(metadata) = metadata
            && let Some(type_name) = metadata.get("type_name")
            && let Some(schema) = get_arrow_schema(type_name)
        {
            return (*schema).clone();
        }

        // Unknown type - return minimal schema (caller should not use this for decode)
        arrow::datatypes::Schema::new(vec![arrow::datatypes::Field::new(
            "dummy",
            arrow::datatypes::DataType::Int64,
            true,
        )])
    }
}

/// Strips the data_type column from a record batch and returns the parsed DataType.
/// Returns (batch, None) if there is no data_type column.
fn strip_data_type_column(
    batch: &RecordBatch,
) -> Result<(RecordBatch, Option<DataType>), super::EncodingError> {
    use super::extract_column_string;

    let Some(data_type_col_idx) = batch
        .schema()
        .fields()
        .iter()
        .position(|f| f.name() == "data_type")
    else {
        return Ok((batch.clone(), None));
    };

    if batch.num_rows() == 0 {
        return Ok((batch.clone(), None));
    }

    let cols = batch.columns();
    let string_col = extract_column_string(cols, "data_type", data_type_col_idx).map_err(|e| {
        super::EncodingError::ParseError("custom_data", format!("data_type column: {e}"))
    })?;
    let first_value = string_col.value(0);
    let data_type = DataType::from_persistence_json(first_value)
        .map_err(|e| super::EncodingError::ParseError("custom_data", e.to_string()))?;

    let new_fields: Vec<_> = batch
        .schema()
        .fields()
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != data_type_col_idx)
        .map(|(_, f)| f.clone())
        .collect();
    let new_columns: Vec<Arc<dyn arrow::array::Array>> = batch
        .columns()
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != data_type_col_idx)
        .map(|(_, c)| Arc::clone(c))
        .collect();
    let new_schema =
        arrow::datatypes::Schema::new_with_metadata(new_fields, batch.schema().metadata().clone());
    let stripped_batch = RecordBatch::try_new(Arc::new(new_schema), new_columns)
        .map_err(|e| super::EncodingError::ParseError("custom_data", e.to_string()))?;

    Ok((stripped_batch, Some(data_type)))
}

impl DecodeDataFromRecordBatch for CustomDataDecoder {
    fn decode_data_batch(
        metadata: &std::collections::HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, super::EncodingError> {
        let type_name = metadata
            .get("type_name")
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());

        let (batch_to_decode, restored_data_type) = strip_data_type_column(&record_batch)?;

        if batch_to_decode.num_rows() == 0 {
            return Ok(Vec::new());
        }

        let data = match decode_custom_from_arrow(&type_name, metadata, batch_to_decode) {
            Ok(Some(d)) => d,
            Ok(None) => {
                return Err(super::EncodingError::ParseError(
                    "custom_data",
                    format!(
                        "unknown custom data type '{type_name}'; only Rust-registered types are supported"
                    ),
                ));
            }
            Err(e) => {
                return Err(super::EncodingError::ParseError(
                    "custom_data",
                    format!("decode_custom_from_arrow: {e}"),
                ));
            }
        };

        if let Some(dt) = restored_data_type {
            Ok(data
                .into_iter()
                .map(|d| {
                    if let Data::Custom(c) = d {
                        Data::Custom(CustomData::new(Arc::clone(&c.data), dt.clone()))
                    } else {
                        d
                    }
                })
                .collect())
        } else {
            Ok(data)
        }
    }
}
