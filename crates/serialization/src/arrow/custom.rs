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
//!   catalog) for each custom data type `T` produced by the `#[custom_data]` macro. When Python
//!   support is enabled, also call `nautilus_model::data::register_rust_extractor::<T>()`.
//! - **Decoder:** [`CustomDataDecoder`] provides [`ArrowSchemaProvider`] and
//!   [`DecodeDataFromRecordBatch`] for Parquet-backed custom data decoded at runtime by type name.
//!   Types must be registered via [`ensure_custom_data_registered::<T>()`] before use.

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::ArrayRef,
    datatypes::{DataType as ArrowDataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::data::{
    ArrowDecoder, ArrowEncoder, CustomData, CustomDataTrait, Data, DataType,
    decode_custom_from_arrow, ensure_arrow_registered, ensure_custom_data_json_registered,
    get_arrow_schema,
};

use super::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError,
    extract_column_string,
};

const KEY_TYPE_NAME: &str = "type_name";
const COLUMN_DATA_TYPE: &str = "data_type";
const FIELD_CUSTOM_DATA: &str = "custom_data";

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
    fn schema(&self) -> anyhow::Result<Schema>;

    /// Encodes a batch of custom data items to an Arrow RecordBatch.
    ///
    /// # Errors
    /// Returns an error if encoding fails (e.g. type mismatch or Arrow error).
    fn encode_record_batch(
        &self,
        items: &[Arc<dyn CustomDataTrait>],
    ) -> anyhow::Result<RecordBatch>;
}

/// Registers a custom data type in the JSON and Arrow registries. Call once per type
/// (e.g. at catalog decode or before querying custom data).
///
/// Each distinct type `T` is registered at most once (per process). Safe to call
/// multiple times for the same `T`.
///
/// When Python support is enabled, also call
/// `nautilus_model::data::register_rust_extractor::<T>()` for types exposed to Python.
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
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        if let Some(metadata) = metadata
            && let Some(type_name) = metadata.get(KEY_TYPE_NAME)
            && let Some(schema) = get_arrow_schema(type_name)
        {
            return (*schema).clone();
        }

        // Unknown type - return minimal schema (caller should not use this for decode)
        Schema::new(vec![Field::new("dummy", ArrowDataType::Int64, true)])
    }
}

impl DecodeDataFromRecordBatch for CustomDataDecoder {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let type_name = metadata
            .get(KEY_TYPE_NAME)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());

        let (batch, restored_data_type) = strip_data_type_column(&record_batch)?;

        if batch.num_rows() == 0 {
            return Ok(Vec::new());
        }

        let data = match decode_custom_from_arrow(&type_name, metadata, batch) {
            Ok(Some(data)) => data,
            Ok(None) => {
                return Err(EncodingError::ParseError(
                    FIELD_CUSTOM_DATA,
                    format!(
                        "unknown custom data type '{type_name}'; only Rust-registered types are supported"
                    ),
                ));
            }
            Err(e) => {
                return Err(EncodingError::ParseError(
                    FIELD_CUSTOM_DATA,
                    format!("decode_custom_from_arrow: {e}"),
                ));
            }
        };

        let Some(data_type) = restored_data_type else {
            return Ok(data);
        };

        Ok(data
            .into_iter()
            .map(|item| match item {
                Data::Custom(custom) => {
                    Data::Custom(CustomData::new(Arc::clone(&custom.data), data_type.clone()))
                }
                other => other,
            })
            .collect())
    }
}

// Splits the `data_type` column off a batch so the registered decoder sees the schema it
// registered. Returns the batch unchanged with `None` when the column is absent or the batch
// is empty.
fn strip_data_type_column(
    batch: &RecordBatch,
) -> Result<(RecordBatch, Option<DataType>), EncodingError> {
    let Some(column_index) = batch
        .schema()
        .fields()
        .iter()
        .position(|field| field.name() == COLUMN_DATA_TYPE)
    else {
        return Ok((batch.clone(), None));
    };

    if batch.num_rows() == 0 {
        return Ok((batch.clone(), None));
    }

    let columns = batch.columns();
    let data_type = if columns[column_index].is_null(0) {
        None
    } else {
        let values =
            extract_column_string(columns, COLUMN_DATA_TYPE, column_index).map_err(|e| {
                EncodingError::ParseError(FIELD_CUSTOM_DATA, format!("data_type column: {e}"))
            })?;

        Some(
            DataType::from_persistence_json(values.value(0))
                .map_err(|e| EncodingError::ParseError(FIELD_CUSTOM_DATA, e.to_string()))?,
        )
    };

    let schema = batch.schema();
    let fields: Vec<_> = schema
        .fields()
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != column_index)
        .map(|(_, field)| field.clone())
        .collect();
    let columns: Vec<ArrayRef> = columns
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != column_index)
        .map(|(_, column)| Arc::clone(column))
        .collect();
    let stripped = Schema::new_with_metadata(fields, schema.metadata().clone());

    RecordBatch::try_new(Arc::new(stripped), columns)
        .map(|batch| (batch, data_type))
        .map_err(|e| EncodingError::ParseError(FIELD_CUSTOM_DATA, e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use arrow::{
        array::{ArrayRef, Int64Array},
        datatypes::{DataType as ArrowDataType, Field, Schema},
    };
    use rstest::rstest;

    use super::*;
    use crate::arrow::EncodingError;

    #[rstest]
    fn test_decode_data_batch_rejects_unregistered_type() {
        let type_name = "UnregisteredCustomDataForArrowTest";
        let metadata = HashMap::from([("type_name".to_string(), type_name.to_string())]);
        let schema = Schema::new(vec![Field::new("value", ArrowDataType::Int64, false)]);
        let columns: Vec<ArrayRef> = vec![Arc::new(Int64Array::from(vec![7]))];
        let batch = RecordBatch::try_new(Arc::new(schema), columns).unwrap();

        let error = CustomDataDecoder::decode_data_batch(&metadata, batch)
            .expect_err("unregistered type must be rejected");

        let EncodingError::ParseError(field, message) = error else {
            panic!("unexpected error variant: {error:?}");
        };
        assert_eq!(field, "custom_data");
        assert_eq!(
            message,
            "unknown custom data type 'UnregisteredCustomDataForArrowTest'; only Rust-registered types are supported"
        );
    }
}
