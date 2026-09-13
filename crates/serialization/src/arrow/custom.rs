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
//!   catalog) for each custom data type `T` using the `#[arrow_custom_data]` macro. When Python
//!   support is enabled, also call `nautilus_model::data::register_rust_extractor::<T>()`.
//! - **Decoder:** [`CustomDataDecoder`] provides [`ArrowSchemaProvider`] and
//!   [`DecodeDataFromRecordBatch`] for Parquet-backed custom data decoded at runtime by type name.
//!   Types must be registered via [`ensure_custom_data_registered::<T>()`] before use.

use std::sync::Arc;

use arrow::{
    array::{
        Array, FixedSizeListArray, GenericListArray, GenericListViewArray, LargeListArray,
        LargeListViewArray, ListArray, ListViewArray, OffsetSizeTrait,
    },
    datatypes::{DataType as ArrowDataType, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::data::{
    ArrowDecoder, ArrowEncoder, CustomData, CustomDataTrait, Data, DataType,
    decode_custom_from_arrow, ensure_arrow_registered, ensure_custom_data_json_registered,
    get_arrow_schema, validate_custom_arrow_schema,
};

use super::{ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch};

/// Trait for custom data types that support Arrow schema and record batch encoding.
/// Used as a type bound by the `#[arrow_custom_data]` macro; catalog encoding goes through
/// the registry, not this trait directly.
///
/// Implemented by the `#[arrow_custom_data]` macro for Rust custom data types. Python custom
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
    if let Some(schema) = get_arrow_schema(type_name) {
        assert_custom_schema(type_name, &schema);
        return;
    }

    let _ = ensure_custom_data_json_registered::<T>();

    let schema = Arc::new(T::get_schema(None));
    assert_custom_schema(type_name, &schema);

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

fn assert_custom_schema(type_name: &str, schema: &Schema) {
    validate_custom_arrow_schema(type_name, schema, true).unwrap();
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Array, ArrayData, FixedSizeListArray, Int64Array, ListArray, ListViewArray},
        buffer::{Buffer, NullBuffer},
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use rstest::rstest;

    use super::{
        assert_custom_schema, validate_required_list_child, validate_required_list_values,
    };

    #[rstest]
    fn custom_schema_allows_vec_u8_binary() {
        let schema = Schema::new(vec![Field::new("payload", DataType::Binary, false)]);

        assert_custom_schema("BinaryPayload", &schema);
    }

    #[rstest]
    #[should_panic(
        expected = "custom write schema `OpaquePayload` contains opaque byte field `item`: FixedSizeBinary(8)"
    )]
    fn custom_schema_rejects_nested_opaque_bytes() {
        let child = Field::new("item", DataType::FixedSizeBinary(8), false);
        let schema = Schema::new(vec![Field::new(
            "values",
            DataType::List(std::sync::Arc::new(child)),
            false,
        )]);

        assert_custom_schema("OpaquePayload", &schema);
    }

    #[rstest]
    fn required_list_rejects_null_child_value() {
        let item = Field::new("item", DataType::Int64, false);
        let values = Int64Array::from(vec![Some(1), None]);

        let error = validate_required_list_child("values", &item, &values, std::iter::once((0, 2)))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Error parsing `custom_data`: field 'values': required list element 1 is null"
        );
    }

    #[rstest]
    fn required_fixed_size_list_allows_null_child_under_null_outer_row() {
        let child = Arc::new(Field::new("item", DataType::Int64, false));
        let values = Int64Array::from(vec![Some(1), Some(2), None, None, Some(5), Some(6)]);
        let nulls = NullBuffer::from(vec![true, false, true]);
        let list =
            FixedSizeListArray::try_new(Arc::clone(&child), 2, Arc::new(values), Some(nulls))
                .unwrap();
        let schema = Schema::new(vec![Field::new(
            "values",
            DataType::FixedSizeList(child, 2),
            true,
        )]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(list)]).unwrap();

        validate_required_list_values(&batch).unwrap();
    }

    #[rstest]
    #[allow(
        unsafe_code,
        reason = "storage can declare a non-nullable child while slots hold nulls; arrow's safe constructors reject building such data"
    )]
    fn required_list_allows_unreferenced_null_child_after_slice() {
        let child = Arc::new(Field::new("item", DataType::Int64, false));
        let values = Int64Array::from(vec![Some(1), Some(2), None]);
        let data = unsafe {
            ArrayData::builder(DataType::List(Arc::clone(&child)))
                .len(3)
                .add_buffer(Buffer::from_slice_ref([0i32, 1, 2, 3]))
                .add_child_data(values.into_data())
                .build_unchecked()
        };
        let list = ListArray::from(data).slice(0, 2);
        let schema = Schema::new(vec![Field::new("values", DataType::List(child), false)]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(list)]).unwrap();

        validate_required_list_values(&batch).unwrap();
    }

    #[rstest]
    #[allow(
        unsafe_code,
        reason = "storage can declare a non-nullable child while slots hold nulls; arrow's safe constructors reject building such data"
    )]
    fn required_list_rejects_null_child_referenced_by_valid_row() {
        let child = Arc::new(Field::new("item", DataType::Int64, false));
        let values = Int64Array::from(vec![Some(1), None]);
        let data = unsafe {
            ArrayData::builder(DataType::List(Arc::clone(&child)))
                .len(2)
                .add_buffer(Buffer::from_slice_ref([0i32, 1, 2]))
                .add_child_data(values.into_data())
                .build_unchecked()
        };
        let list = ListArray::from(data);
        let schema = Schema::new(vec![Field::new("values", DataType::List(child), false)]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(list)]).unwrap();

        let error = validate_required_list_values(&batch).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Error parsing `custom_data`: field 'values': required list element 1 is null"
        );
    }

    #[rstest]
    fn required_list_view_rejects_referenced_null_child_value() {
        let child = Arc::new(Field::new("item", DataType::Int64, false));
        let values = Int64Array::from(vec![Some(1), None, Some(3)]);
        let data = ArrayData::builder(DataType::ListView(Arc::clone(&child)))
            .len(2)
            .add_buffer(Buffer::from_slice_ref([0i32, 1]))
            .add_buffer(Buffer::from_slice_ref([1i32, 2]))
            .add_child_data(values.into_data())
            .build()
            .unwrap();
        let list = ListViewArray::from(data);
        let schema = Schema::new(vec![Field::new("values", DataType::ListView(child), false)]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(list)]).unwrap();

        let error = validate_required_list_values(&batch).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Error parsing `custom_data`: field 'values': required list element 1 is null"
        );
    }
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
    fn get_schema(
        metadata: Option<std::collections::HashMap<String, String>>,
    ) -> arrow::datatypes::Schema {
        if let Some(metadata) = metadata
            && let Some(type_name) = metadata.get("type_name")
            && let Some(schema) = get_arrow_schema(type_name)
        {
            let schema = (*schema).clone();
            let mut fields = schema.fields().iter().cloned().collect::<Vec<_>>();
            if schema.field_with_name("data_type").is_err() {
                fields.push(Arc::new(arrow::datatypes::Field::new(
                    "data_type",
                    arrow::datatypes::DataType::Utf8,
                    false,
                )));
            }
            let mut merged_metadata = schema.metadata().clone();
            merged_metadata.extend(metadata);
            return arrow::datatypes::Schema::new_with_metadata(fields, merged_metadata);
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
    let data_type = if cols[data_type_col_idx].is_null(0) {
        None
    } else {
        let string_col =
            extract_column_string(cols, "data_type", data_type_col_idx).map_err(|e| {
                super::EncodingError::ParseError("custom_data", format!("data_type column: {e}"))
            })?;
        let first_value = string_col.value(0);
        Some(
            DataType::from_persistence_json(first_value)
                .map_err(|e| super::EncodingError::ParseError("custom_data", e.to_string()))?,
        )
    };

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

    Ok((stripped_batch, data_type))
}

impl CustomDataDecoder {
    /// Decodes a `RecordBatch` into typed [`CustomData`] values.
    ///
    /// # Errors
    ///
    /// Returns an `EncodingError` if the type is unregistered, decoding fails, or a registered
    /// decoder yields a non-custom row.
    pub fn decode_custom_batch(
        metadata: &std::collections::HashMap<String, String>,
        record_batch: &RecordBatch,
    ) -> Result<Vec<CustomData>, super::EncodingError> {
        let type_name = metadata
            .get("type_name")
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());

        let (batch_to_decode, restored_data_type) = strip_data_type_column(record_batch)?;
        validate_required_list_values(&batch_to_decode)?;

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

        data.into_iter()
            .map(|d| match d {
                Data::Custom(c) => Ok(match &restored_data_type {
                    Some(dt) => CustomData::new(Arc::clone(&c.data), dt.clone()),
                    None => c,
                }),
                _ => Err(super::EncodingError::ParseError(
                    "custom_data",
                    format!("registered decoder for '{type_name}' yielded a non-custom row"),
                )),
            })
            .collect()
    }
}

fn validate_required_list_values(batch: &RecordBatch) -> Result<(), super::EncodingError> {
    let schema = batch.schema();
    for (field, column) in schema.fields().iter().zip(batch.columns()) {
        match field.data_type() {
            ArrowDataType::List(child) => {
                let list = downcast_list::<ListArray>(field.name(), column.as_ref(), "list")?;
                validate_required_list_child(
                    field.name(),
                    child,
                    list.values().as_ref(),
                    list_ranges(list),
                )?;
            }
            ArrowDataType::LargeList(child) => {
                let list =
                    downcast_list::<LargeListArray>(field.name(), column.as_ref(), "large-list")?;
                validate_required_list_child(
                    field.name(),
                    child,
                    list.values().as_ref(),
                    list_ranges(list),
                )?;
            }
            ArrowDataType::ListView(child) => {
                let list =
                    downcast_list::<ListViewArray>(field.name(), column.as_ref(), "list-view")?;
                validate_required_list_child(
                    field.name(),
                    child,
                    list.values().as_ref(),
                    list_view_ranges(list),
                )?;
            }
            ArrowDataType::LargeListView(child) => {
                let list = downcast_list::<LargeListViewArray>(
                    field.name(),
                    column.as_ref(),
                    "large-list-view",
                )?;
                validate_required_list_child(
                    field.name(),
                    child,
                    list.values().as_ref(),
                    list_view_ranges(list),
                )?;
            }
            ArrowDataType::FixedSizeList(child, _) => {
                let list = downcast_list::<FixedSizeListArray>(
                    field.name(),
                    column.as_ref(),
                    "fixed-size-list",
                )?;
                validate_required_list_child(
                    field.name(),
                    child,
                    list.values().as_ref(),
                    fixed_size_list_ranges(list),
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn downcast_list<'a, T: Array + 'static>(
    field_name: &str,
    column: &'a dyn Array,
    kind: &str,
) -> Result<&'a T, super::EncodingError> {
    column.as_any().downcast_ref::<T>().ok_or_else(|| {
        super::EncodingError::ParseError(
            "custom_data",
            format!("field '{field_name}' is not a {kind} array"),
        )
    })
}

fn validate_required_list_child(
    field_name: &str,
    child: &arrow::datatypes::Field,
    values: &dyn Array,
    ranges: impl Iterator<Item = (usize, usize)>,
) -> Result<(), super::EncodingError> {
    if child.is_nullable() || values.null_count() == 0 {
        return Ok(());
    }

    for (start, end) in ranges {
        if let Some(index) = (start..end).find(|index| values.is_null(*index)) {
            return Err(super::EncodingError::ParseError(
                "custom_data",
                format!("field '{field_name}': required list element {index} is null"),
            ));
        }
    }
    Ok(())
}

fn list_ranges<O: OffsetSizeTrait>(
    list: &GenericListArray<O>,
) -> impl Iterator<Item = (usize, usize)> + '_ {
    let offsets = list.value_offsets();
    (0..list.len())
        .filter(|row| list.is_valid(*row))
        .map(move |row| (offsets[row].as_usize(), offsets[row + 1].as_usize()))
}

fn list_view_ranges<O: OffsetSizeTrait>(
    list: &GenericListViewArray<O>,
) -> impl Iterator<Item = (usize, usize)> + '_ {
    let offsets = list.value_offsets();
    let sizes = list.value_sizes();
    (0..list.len())
        .filter(|row| list.is_valid(*row))
        .map(move |row| {
            let start = offsets[row].as_usize();
            (start, start + sizes[row].as_usize())
        })
}

fn fixed_size_list_ranges(list: &FixedSizeListArray) -> impl Iterator<Item = (usize, usize)> + '_ {
    let length =
        usize::try_from(list.value_length()).expect("fixed-size list length is non-negative");
    (0..list.len())
        .filter(|row| list.is_valid(*row))
        .map(move |row| (row * length, (row + 1) * length))
}

impl DecodeDataFromRecordBatch for CustomDataDecoder {
    fn decode_data_batch(
        metadata: &std::collections::HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, super::EncodingError> {
        Ok(Self::decode_custom_batch(metadata, &record_batch)?
            .into_iter()
            .map(Data::Custom)
            .collect())
    }
}
