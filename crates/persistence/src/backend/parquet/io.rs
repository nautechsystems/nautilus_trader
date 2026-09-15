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

#![expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "Parquet I/O functions forward Arrow/object-store errors and use validated schema paths"
)]

use std::{collections::HashMap, sync::Arc};

use ahash::AHashMap;
use arrow::{
    array::{
        Array, ArrayRef, BinaryArray, Decimal128Array, FixedSizeListArray, ListArray,
        StringBuilder, StructArray, UInt32Array, UInt64Array,
    },
    buffer::{OffsetBuffer, ScalarBuffer},
    compute::cast,
    datatypes::{DataType, Field, Fields, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::{data::NautilusRecordType, instruments::InstrumentAny};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, EncodeToRecordBatch, KEY_IDENTIFIER, KEY_PRICE_PRECISION,
    KEY_SIZE_PRECISION, instrument::decode_instrument_any_batch, is_nautilus_legacy_schema,
    is_nautilus_timestamp_schema, normalize_legacy_fixed_columns,
    normalized_legacy_data_type as normalize_legacy_arrow_data_type, normalized_timestamp_type,
};
use object_store::{
    ObjectStore, ObjectStoreExt, PutMode, PutOptions, buffered::BufReader, path::Path as ObjectPath,
};
use parquet::{
    arrow::{
        ArrowSchemaConverter, ArrowWriter, ParquetRecordBatchStreamBuilder,
        arrow_reader::ParquetRecordBatchReaderBuilder,
    },
    basic::{Compression, ZstdLevel},
    file::{
        metadata::{KeyValue, SortingColumn},
        properties::WriterProperties,
        reader::{FileReader, SerializedFileReader},
        statistics::Statistics,
    },
    schema::types::ColumnPath,
};
use url::Url;

use crate::common::arrow::catalog_record_schema;

const DEPTH10_LEN: usize = 10;

pub(crate) fn is_remote_uri_scheme(scheme: &str) -> bool {
    matches!(
        scheme,
        "s3" | "gs" | "gcs" | "az" | "abfs" | "http" | "https"
    )
}

pub(crate) fn remote_store_root_url(uri: &str) -> anyhow::Result<Url> {
    let mut url = Url::parse(uri)?;
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn remote_full_uri(uri: &str, object_path: &str) -> anyhow::Result<String> {
    let root = remote_store_root_url(uri)?;
    let root = root.as_str().trim_end_matches('/');
    let object_path = object_path.trim_start_matches('/');

    if object_path.is_empty() {
        Ok(root.to_string())
    } else {
        Ok(format!("{root}/{object_path}"))
    }
}

/// Normalizes supported legacy Parquet physical encodings for explicit migration.
///
/// Older Python/v1 catalog writers can emit low-cardinality strings as Arrow dictionary
/// arrays and instrument `info` values as the JSON bytes `null` rather than Arrow nulls.
pub(crate) fn normalize_legacy_parquet_columns(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    if let Some(schema) = normalize_legacy_record_schema(batch.schema_ref()) {
        let batch = normalize_legacy_info_column(batch)?;
        let batch = normalize_legacy_fixed_columns(&batch)?;
        return Ok(nautilus_serialization::arrow::record_batch_with_timestamps(
            Arc::new(schema),
            batch.columns().to_vec(),
        )?);
    }

    if is_legacy_instrument_schema(batch.schema_ref()) {
        let metadata = batch.schema().metadata().clone();
        if batch.num_rows() == 0 {
            return Ok(RecordBatch::new_empty(Arc::new(InstrumentAny::get_schema(
                Some(metadata),
            ))));
        }
        let batch = normalize_legacy_info_column(batch)?;
        let instruments = decode_instrument_any_batch(&metadata, &batch)?;
        return Ok(InstrumentAny::encode_batch(&metadata, &instruments)?);
    }

    let normalize_legacy = is_nautilus_legacy_schema(batch.schema_ref());
    let batch = if normalize_legacy {
        normalize_dictionary_string_columns(batch)?
    } else {
        batch.clone()
    };
    let batch = normalize_legacy_fixed_columns(&batch)?;
    let batch = normalize_legacy_info_column(&batch)?;
    normalize_legacy_depth_columns(&batch)
}

/// Normalizes the Arrow schema changes made by [`normalize_legacy_parquet_columns`].
#[must_use]
pub(crate) fn normalize_legacy_parquet_schema(schema: &Schema) -> Schema {
    if let Some(schema) = normalize_legacy_record_schema(schema) {
        return schema;
    }

    if is_legacy_instrument_schema(schema) {
        return InstrumentAny::get_schema(Some(schema.metadata().clone()));
    }

    let normalize_fixed = is_nautilus_legacy_schema(schema);
    let normalize_timestamps = is_nautilus_timestamp_schema(schema);
    let fields = schema
        .fields()
        .iter()
        .map(|field| {
            let data_type =
                normalized_legacy_data_type(field.name(), field.data_type(), normalize_fixed);
            let data_type = if schema.metadata().contains_key("type_name")
                && matches!(field.name().as_str(), "ts_event" | "ts_init")
                && data_type == DataType::UInt64
            {
                nautilus_serialization::arrow::timestamp_data_type()
            } else if normalize_timestamps {
                normalized_timestamp_type(&data_type)
            } else {
                data_type
            };
            let nullable = field.is_nullable()
                || (normalize_fixed
                    && matches!(field.data_type(), DataType::FixedSizeBinary(8 | 16)))
                || (field.name() == "info" && field.data_type() == &DataType::Binary);
            Arc::new(
                field
                    .as_ref()
                    .clone()
                    .with_data_type(data_type)
                    .with_nullable(nullable),
            )
        })
        .collect::<Vec<_>>();
    normalize_legacy_depth_schema(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

fn normalize_legacy_record_schema(schema: &Schema) -> Option<Schema> {
    let record_type = schema
        .metadata()
        .get("type")?
        .parse::<NautilusRecordType>()
        .ok()?;
    let current = catalog_record_schema(&record_type).ok()?;
    let fields = schema
        .fields()
        .iter()
        .map(|field| {
            let Ok(expected) = current.field_with_name(field.name()) else {
                return field.clone();
            };

            if field.data_type() == expected.data_type()
                || (expected.data_type() == &nautilus_serialization::arrow::timestamp_data_type()
                    && (field.data_type() == &DataType::UInt64
                        || normalized_timestamp_type(field.data_type()) == *expected.data_type()))
                || (field.name() == "info" && field.data_type() == &DataType::Binary)
            {
                Arc::new(expected.clone())
            } else {
                field.clone()
            }
        })
        .collect::<Vec<_>>();
    Some(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

fn is_legacy_instrument_schema(schema: &Schema) -> bool {
    schema.metadata().contains_key("class")
        && schema
            .field_with_name("ts_init")
            .is_ok_and(|field| field.data_type() == &DataType::UInt64)
}

/// Casts dictionary-encoded string columns from legacy Parquet files to plain UTF-8 columns.
///
/// Older Python/v1 catalog writers can emit low-cardinality strings as Arrow dictionary
/// arrays. Rust decoders generally expect concrete `Utf8` columns, so Parquet reads normalize
/// this physical encoding before typed decoding.
fn normalize_dictionary_string_columns(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    let schema = batch.schema();
    let mut changed = false;
    let mut fields = Vec::with_capacity(schema.fields().len());
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(batch.num_columns());

    for (field, column) in schema.fields().iter().zip(batch.columns()) {
        let data_type = normalized_dictionary_data_type(field.data_type());

        changed |= &data_type != field.data_type();
        fields.push(Arc::new(
            field.as_ref().clone().with_data_type(data_type.clone()),
        ));

        if column.data_type() == &data_type {
            columns.push(column.clone());
        } else {
            columns.push(cast(column.as_ref(), &data_type)?);
        }
    }

    if !changed {
        return Ok(batch.clone());
    }

    let schema = Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn normalized_legacy_data_type(
    name: &str,
    data_type: &DataType,
    normalize_fixed: bool,
) -> DataType {
    if normalize_fixed
        && let DataType::FixedSizeList(item, length) = data_type
        && matches!(item.data_type(), DataType::FixedSizeBinary(8 | 16))
    {
        return DataType::FixedSizeList(
            Arc::new(
                item.as_ref()
                    .clone()
                    .with_data_type(normalize_legacy_arrow_data_type(name, item.data_type()))
                    .with_nullable(true),
            ),
            *length,
        );
    }

    if normalize_fixed {
        let normalized = normalize_legacy_arrow_data_type(name, data_type);
        if &normalized != data_type {
            return normalized;
        }
    }

    match data_type {
        DataType::Dictionary(_, value_type)
            if normalize_fixed && matches!(value_type.as_ref(), DataType::Utf8) =>
        {
            DataType::Utf8
        }
        DataType::Binary if name == "info" => DataType::Utf8,
        _ => data_type.clone(),
    }
}

fn normalized_dictionary_data_type(data_type: &DataType) -> DataType {
    match data_type {
        DataType::Dictionary(_, value_type) if matches!(value_type.as_ref(), DataType::Utf8) => {
            DataType::Utf8
        }
        _ => data_type.clone(),
    }
}

fn normalize_legacy_info_column(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    let Some(info_index) = batch.schema().index_of("info").ok() else {
        return Ok(batch.clone());
    };

    let column = batch.column(info_index);
    if column.data_type() != &DataType::Binary {
        return Ok(batch.clone());
    }

    let info = column
        .as_any()
        .downcast_ref::<BinaryArray>()
        .expect("Binary column should downcast to BinaryArray");

    let mut builder = StringBuilder::new();

    for row in 0..info.len() {
        if info.is_null(row) || info.value(row) == b"null" {
            builder.append_null();
        } else {
            builder.append_value(std::str::from_utf8(info.value(row))?);
        }
    }

    let mut fields = batch.schema().fields().iter().cloned().collect::<Vec<_>>();
    fields[info_index] = Arc::new(
        fields[info_index]
            .as_ref()
            .clone()
            .with_data_type(DataType::Utf8)
            .with_nullable(true),
    );
    let mut columns = batch.columns().to_vec();
    columns[info_index] = Arc::new(builder.finish());
    let schema = Arc::new(Schema::new_with_metadata(
        fields,
        batch.schema().metadata().clone(),
    ));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn normalize_legacy_depth_schema(schema: Schema) -> Schema {
    if !has_legacy_depth_columns(&schema) {
        return schema;
    }

    let mut fields = vec![depth_side_field("bids"), depth_side_field("asks")];
    fields.extend(
        schema
            .fields()
            .iter()
            .filter(|field| !is_legacy_depth_column(field.name()))
            .cloned(),
    );
    Schema::new_with_metadata(fields, schema.metadata().clone())
}

fn normalize_legacy_depth_columns(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    if !has_legacy_depth_columns(batch.schema().as_ref()) {
        return Ok(batch.clone());
    }

    let flat = batch.schema().index_of("bid_price_0").is_ok();
    let side_values = |side: &str, value: &str| {
        let name = format!("{side}_{value}");
        if flat {
            match value {
                "price" | "size" => decimal_depth_list(batch, &name),
                "count" => u32_depth_list(batch, &name),
                "order_id" => u64_depth_list(batch, &name),
                _ => unreachable!("depth field inventory is fixed"),
            }
            .and_then(|list| depth_list_values(&list))
        } else {
            if batch.column_by_name(&name).is_none() && matches!(value, "count" | "order_id") {
                let width = legacy_fixed_list_width(batch, side)?;
                let len = batch.num_rows().checked_mul(width).ok_or_else(|| {
                    anyhow::anyhow!("Legacy depth column '{name}' length overflow")
                })?;
                return match value {
                    "count" => Ok(Arc::new(UInt32Array::from(vec![0; len])) as ArrayRef),
                    "order_id" => Ok(Arc::new(UInt64Array::from(vec![0; len])) as ArrayRef),
                    _ => unreachable!("missing legacy depth defaults are fixed"),
                };
            }
            depth_list_values(
                batch
                    .column_by_name(&name)
                    .ok_or_else(|| anyhow::anyhow!("Missing legacy depth column '{name}'"))?,
            )
        }
    };
    let mut columns = vec![
        depth_side_array(
            &side_values("bid", "price")?,
            &side_values("bid", "size")?,
            &side_values("bid", "count")?,
            &side_values("bid", "order_id")?,
            batch.num_rows(),
        )?,
        depth_side_array(
            &side_values("ask", "price")?,
            &side_values("ask", "size")?,
            &side_values("ask", "count")?,
            &side_values("ask", "order_id")?,
            batch.num_rows(),
        )?,
    ];
    columns.extend(
        batch
            .schema()
            .fields()
            .iter()
            .zip(batch.columns())
            .filter(|(field, _)| !is_legacy_depth_column(field.name()))
            .map(|(_, column)| column.clone()),
    );
    let schema = Arc::new(normalize_legacy_depth_schema(
        batch.schema().as_ref().clone(),
    ));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn legacy_fixed_list_width(batch: &RecordBatch, side: &str) -> anyhow::Result<usize> {
    let schema = batch.schema();
    ["price", "size", "count", "order_id"]
        .iter()
        .filter_map(|value| schema.field_with_name(&format!("{side}_{value}")).ok())
        .find_map(|field| match field.data_type() {
            DataType::FixedSizeList(_, width) => usize::try_from(*width).ok(),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("Missing legacy depth FixedSizeList width for '{side}'"))
}

fn has_legacy_depth_columns(schema: &Schema) -> bool {
    if schema.index_of("bids").is_ok() {
        return false;
    }

    let has_fixed_lists = ["bid_price", "ask_price", "bid_size", "ask_size"]
        .iter()
        .all(|name| {
            schema
                .field_with_name(name)
                .is_ok_and(|field| matches!(field.data_type(), DataType::FixedSizeList(_, _)))
        });
    let has_flat_levels = ["bid_price_0", "ask_price_0", "bid_size_0", "ask_size_0"]
        .iter()
        .all(|name| schema.index_of(name).is_ok());

    has_fixed_lists || has_flat_levels
}

fn depth_level_fields() -> Fields {
    vec![
        Field::new("price", DataType::Decimal128(38, 16), false),
        Field::new("size", DataType::Decimal128(38, 16), false),
        Field::new("count", DataType::UInt32, false),
        Field::new("order_id", DataType::UInt64, false),
    ]
    .into()
}

fn depth_side_field(name: &str) -> Arc<Field> {
    let fields = depth_level_fields();
    Arc::new(Field::new(
        name,
        DataType::List(Arc::new(Field::new(
            "item",
            DataType::Struct(fields),
            false,
        ))),
        false,
    ))
}

fn decimal_depth_list(batch: &RecordBatch, prefix: &str) -> anyhow::Result<ArrayRef> {
    let mut values = Vec::with_capacity(batch.num_rows() * DEPTH10_LEN);
    let arrays = (0..DEPTH10_LEN)
        .map(|level| {
            let name = format!("{prefix}_{level}");
            batch
                .column_by_name(&name)
                .and_then(|column| column.as_any().downcast_ref::<Decimal128Array>())
                .ok_or_else(|| anyhow::anyhow!("Legacy depth column '{name}' must be Decimal128"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    for row in 0..batch.num_rows() {
        for array in &arrays {
            values.push((!array.is_null(row)).then(|| array.value(row)));
        }
    }
    let values = Decimal128Array::from(values).with_precision_and_scale(38, 16)?;
    Ok(depth_list_array(Arc::new(values), true))
}

fn u64_depth_list(batch: &RecordBatch, prefix: &str) -> anyhow::Result<ArrayRef> {
    let arrays = (0..DEPTH10_LEN)
        .map(|level| {
            let name = format!("{prefix}_{level}");
            batch
                .column_by_name(&name)
                .map(|column| {
                    column
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .ok_or_else(|| {
                            anyhow::anyhow!("Legacy depth column '{name}' must be UInt64")
                        })
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut values = Vec::with_capacity(batch.num_rows() * DEPTH10_LEN);
    for row in 0..batch.num_rows() {
        for array in &arrays {
            values.push(array.map_or(0, |array| array.value(row)));
        }
    }
    Ok(depth_list_array(Arc::new(UInt64Array::from(values)), false))
}

fn u32_depth_list(batch: &RecordBatch, prefix: &str) -> anyhow::Result<ArrayRef> {
    let arrays = (0..DEPTH10_LEN)
        .map(|level| {
            let name = format!("{prefix}_{level}");
            batch
                .column_by_name(&name)
                .and_then(|column| column.as_any().downcast_ref::<UInt32Array>())
                .ok_or_else(|| anyhow::anyhow!("Legacy depth column '{name}' must be UInt32"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut values = Vec::with_capacity(batch.num_rows() * DEPTH10_LEN);
    for row in 0..batch.num_rows() {
        for array in &arrays {
            values.push(array.value(row));
        }
    }
    Ok(depth_list_array(Arc::new(UInt32Array::from(values)), false))
}

fn depth_list_array(values: ArrayRef, values_nullable: bool) -> ArrayRef {
    Arc::new(FixedSizeListArray::new(
        Arc::new(Field::new(
            "item",
            values.data_type().clone(),
            values_nullable,
        )),
        i32::try_from(DEPTH10_LEN).expect("depth-10 length fits i32"),
        values,
        None,
    ))
}

fn depth_list_values(list: &ArrayRef) -> anyhow::Result<ArrayRef> {
    list.as_any()
        .downcast_ref::<FixedSizeListArray>()
        .map(|list| list.values().clone())
        .ok_or_else(|| anyhow::anyhow!("Legacy depth column must be FixedSizeList"))
}

fn depth_side_array(
    prices: &ArrayRef,
    sizes: &ArrayRef,
    counts: &ArrayRef,
    order_ids: &ArrayRef,
    rows: usize,
) -> anyhow::Result<ArrayRef> {
    let prices = prices
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth prices must be Decimal128"))?;
    let sizes = sizes
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth sizes must be Decimal128"))?;
    let counts = counts
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth counts must be UInt32"))?;
    let order_ids = order_ids
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or_else(|| anyhow::anyhow!("Legacy depth order IDs must be UInt64"))?;
    let expected_len = prices.len();
    anyhow::ensure!(
        sizes.len() == expected_len
            && counts.len() == expected_len
            && order_ids.len() == expected_len,
        "Legacy depth columns must contain the same number of values"
    );
    let width = if rows == 0 {
        0
    } else {
        anyhow::ensure!(
            expected_len.is_multiple_of(rows),
            "Legacy depth column length {expected_len} is not divisible by row count {rows}"
        );
        expected_len / rows
    };

    let mut open_prices = Vec::with_capacity(expected_len);
    let mut open_sizes = Vec::with_capacity(expected_len);
    let mut open_counts = Vec::with_capacity(expected_len);
    let mut open_order_ids = Vec::with_capacity(expected_len);
    let mut offsets = Vec::with_capacity(rows + 1);
    offsets.push(0);

    for row in 0..rows {
        for level in 0..width {
            let index = row * width + level;
            if prices.is_null(index) || sizes.is_null(index) {
                continue;
            }
            open_prices.push(prices.value(index));
            open_sizes.push(sizes.value(index));
            open_counts.push(counts.value(index));
            open_order_ids.push(order_ids.value(index));
        }
        offsets.push(i32::try_from(open_prices.len())?);
    }

    let fields = depth_level_fields();
    let values = StructArray::try_new(
        fields.clone(),
        vec![
            Arc::new(Decimal128Array::from(open_prices).with_precision_and_scale(38, 16)?),
            Arc::new(Decimal128Array::from(open_sizes).with_precision_and_scale(38, 16)?),
            Arc::new(UInt32Array::from(open_counts)),
            Arc::new(UInt64Array::from(open_order_ids)),
        ],
        None,
    )?;
    Ok(Arc::new(ListArray::try_new(
        Arc::new(Field::new("item", DataType::Struct(fields), false)),
        OffsetBuffer::new(ScalarBuffer::from(offsets)),
        Arc::new(values),
        None,
    )?))
}

fn is_legacy_depth_column(name: &str) -> bool {
    const LIST_COLUMNS: &[&str] = &[
        "bid_price",
        "ask_price",
        "bid_size",
        "ask_size",
        "bid_order_id",
        "ask_order_id",
        "bid_count",
        "ask_count",
    ];
    LIST_COLUMNS.contains(&name)
        || [
            "bid_price_",
            "ask_price_",
            "bid_size_",
            "ask_size_",
            "bid_order_id_",
            "ask_order_id_",
            "bid_count_",
            "ask_count_",
        ]
        .iter()
        .any(|prefix| {
            name.strip_prefix(prefix).is_some_and(|level| {
                level
                    .parse::<usize>()
                    .is_ok_and(|level| level < DEPTH10_LEN)
            })
        })
}

pub(crate) struct ObjectStoreLocation {
    pub object_store: Arc<dyn ObjectStore>,
    pub base_path: String,
    pub original_uri: String,
    store_root_url: Option<Url>,
}

impl ObjectStoreLocation {
    pub(crate) fn store_root_url(&self) -> Option<&Url> {
        self.store_root_url.as_ref()
    }
}

/// Writes a `RecordBatch` to a Parquet file using object store, with optional compression.
///
/// # Errors
///
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batch_to_parquet(
    batch: RecordBatch,
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    write_batches_to_parquet(
        &[batch],
        path,
        storage_options,
        compression,
        max_row_group_size,
    )
    .await
}

/// Writes multiple `RecordBatch` items to a Parquet file using object store, with optional compression, row group sizing, and storage options.
///
/// # Errors
///
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batches_to_parquet(
    batches: &[RecordBatch],
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
) -> anyhow::Result<()> {
    let (object_store, base_path, _) = create_object_store_from_path(path, storage_options)?;
    let object_path = if base_path.is_empty() {
        ObjectPath::from(path)
    } else {
        ObjectPath::from(format!("{base_path}/{path}"))
    };

    write_batches_to_object_store(
        batches,
        object_store,
        &object_path,
        compression,
        max_row_group_size,
        None,
    )
    .await
}

/// Reads only the Arrow schema (including key/value metadata) of a Parquet object.
///
/// Avoids decoding any record batches; use when only schema metadata is needed.
///
/// # Errors
///
/// Returns an error if the object cannot be fetched or its footer cannot be parsed.
pub async fn read_parquet_schema_from_object_store(
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
) -> anyhow::Result<Arc<arrow::datatypes::Schema>> {
    let object = object_store.head(path).await?;
    if object.size == 0 {
        return Ok(Arc::new(arrow::datatypes::Schema::new(Vec::<
            arrow::datatypes::Field,
        >::new())));
    }
    let reader = BufReader::new(object_store, &object);
    let builder = ParquetRecordBatchStreamBuilder::new(reader).await?;
    Ok(builder.schema().clone())
}

/// Reads a Parquet file from an object store and returns all record batches plus
/// the Arrow schema from the builder. The builder's schema includes metadata restored
/// from the file's `ARROW:schema` `key_value_metadata`; use it for decoding instead of
/// each batch's schema (which has metadata stripped).
///
/// # Errors
///
/// Returns an error if the path cannot be read or Parquet parsing fails.
pub async fn read_parquet_from_object_store(
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
) -> anyhow::Result<(Vec<RecordBatch>, Arc<arrow::datatypes::Schema>)> {
    let result: object_store::GetResult = object_store.get(path).await?;
    let data = result.bytes().await?;
    if data.is_empty() {
        return Ok((
            Vec::new(),
            Arc::new(arrow::datatypes::Schema::new(
                Vec::<arrow::datatypes::Field>::new(),
            )),
        ));
    }
    let builder = ParquetRecordBatchReaderBuilder::try_new(data)?;
    let schema = builder.schema().clone();
    let reader = builder.build()?;
    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch?);
    }
    Ok((batches, schema))
}

/// Writes multiple `RecordBatch` items to an object store URI, with optional compression,
/// row group sizing, and `key_value_metadata` (e.g. for instrument "class" so it survives roundtrip).
///
/// # Errors
///
/// Returns an error if writing to Parquet fails or any I/O operation fails.
pub async fn write_batches_to_object_store(
    batches: &[RecordBatch],
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
    key_value_metadata: Option<Vec<KeyValue>>,
) -> anyhow::Result<()> {
    write_batches_to_object_store_with_mode(
        batches,
        object_store,
        path,
        compression,
        max_row_group_size,
        key_value_metadata,
        PutMode::Overwrite,
    )
    .await
}

pub(crate) async fn write_batches_to_object_store_create(
    batches: &[RecordBatch],
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
    key_value_metadata: Option<Vec<KeyValue>>,
) -> anyhow::Result<()> {
    write_batches_to_object_store_with_mode(
        batches,
        object_store,
        path,
        compression,
        max_row_group_size,
        key_value_metadata,
        PutMode::Create,
    )
    .await
}

async fn write_batches_to_object_store_with_mode(
    batches: &[RecordBatch],
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
    key_value_metadata: Option<Vec<KeyValue>>,
    put_mode: PutMode,
) -> anyhow::Result<()> {
    // Create a temporary buffer to write the parquet data
    let mut buffer = Vec::new();

    let schema = batches[0].schema();
    let sorting_columns = parquet_sorting_columns(schema.as_ref())?;
    let mut props_builder = WriterProperties::builder()
        .set_compression(compression.unwrap_or(Compression::ZSTD(ZstdLevel::default())))
        .set_max_row_group_row_count(Some(
            max_row_group_size.unwrap_or(super::DEFAULT_ROW_GROUP_SIZE),
        ))
        .set_sorting_columns(sorting_columns);

    if schema.index_of(KEY_IDENTIFIER).is_ok() {
        props_builder =
            props_builder.set_column_bloom_filter_enabled(ColumnPath::from(KEY_IDENTIFIER), true);
    }

    if let Some(kv) = key_value_metadata {
        props_builder = props_builder.set_key_value_metadata(Some(kv));
    }
    let writer_props = props_builder.build();

    let mut writer = ArrowWriter::try_new(&mut buffer, schema, Some(writer_props))?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.close()?;

    // Upload the buffer to object store
    object_store
        .put_opts(
            path,
            buffer.into(),
            PutOptions {
                mode: put_mode,
                ..Default::default()
            },
        )
        .await?;

    Ok(())
}

fn parquet_sorting_columns(schema: &Schema) -> anyhow::Result<Option<Vec<SortingColumn>>> {
    if schema.index_of("ts_init").is_err() {
        return Ok(None);
    }
    let parquet_schema = ArrowSchemaConverter::new().convert(schema)?;
    let mut names = Vec::with_capacity(2);
    names.push("ts_init");
    if schema.index_of(KEY_IDENTIFIER).is_ok() {
        names.push(KEY_IDENTIFIER);
    }
    let columns = names
        .into_iter()
        .map(|name| {
            let index = parquet_schema
                .columns()
                .iter()
                .position(|column| {
                    column
                        .path()
                        .parts()
                        .first()
                        .is_some_and(|part| part == name)
                })
                .ok_or_else(|| anyhow::anyhow!("Parquet schema is missing sort column {name}"))?;
            Ok(SortingColumn {
                column_idx: i32::try_from(index)?,
                descending: false,
                nulls_first: false,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Some(columns))
}

/// Deduplicates a slice of `RecordBatch` items, removing rows that are identical across all columns.
///
/// Rows are compared by encoding each row to a canonical byte sequence using Arrow's row format.
/// Only the first occurrence of each unique row is retained; the relative order of unique rows
/// is preserved.
///
/// # Errors
///
/// Returns an error if the row converter cannot be constructed or if the `take` kernel fails.
fn deduplicate_record_batches(batches: &[RecordBatch]) -> anyhow::Result<Vec<RecordBatch>> {
    if batches.is_empty() {
        return Ok(Vec::new());
    }

    let schema = batches[0].schema();

    let fields: Vec<arrow::row::SortField> = schema
        .fields()
        .iter()
        .map(|f| arrow::row::SortField::new(f.data_type().clone()))
        .collect();

    let converter = arrow::row::RowConverter::new(fields)?;
    let mut seen: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    let mut result: Vec<RecordBatch> = Vec::new();

    for batch in batches {
        let rows = converter.convert_columns(batch.columns())?;
        let mut indices: Vec<u32> = Vec::new();

        for (i, row) in rows.iter().enumerate() {
            if seen.insert(row.as_ref().to_vec()) {
                indices.push(u32::try_from(i)?);
            }
        }

        if !indices.is_empty() {
            let index_array = arrow::array::UInt32Array::from(indices);
            let deduped_columns: Vec<arrow::array::ArrayRef> = batch
                .columns()
                .iter()
                .map(|col| arrow::compute::take(col.as_ref(), &index_array, None))
                .collect::<Result<_, _>>()?;
            result.push(RecordBatch::try_new(schema.clone(), deduped_columns)?);
        }
    }

    Ok(result)
}

/// Combines multiple Parquet files using object store with storage options
///
/// # Errors
///
/// Returns an error if file reading or writing fails.
pub async fn combine_parquet_files(
    file_paths: Vec<&str>,
    new_file_path: &str,
    storage_options: Option<AHashMap<String, String>>,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
    deduplicate: Option<bool>,
) -> anyhow::Result<()> {
    if file_paths.len() <= 1 {
        return Ok(());
    }

    // Create object store from the first file path (assuming all files are in the same store)
    let (object_store, base_path, _) =
        create_object_store_from_path(file_paths[0], storage_options)?;

    // Convert string paths to ObjectPath
    let object_paths: Vec<ObjectPath> = file_paths
        .iter()
        .map(|path| {
            if base_path.is_empty() {
                ObjectPath::from(*path)
            } else {
                ObjectPath::from(format!("{base_path}/{path}"))
            }
        })
        .collect();

    let new_object_path = if base_path.is_empty() {
        ObjectPath::from(new_file_path)
    } else {
        ObjectPath::from(format!("{base_path}/{new_file_path}"))
    };

    combine_parquet_files_from_object_store(
        object_store,
        object_paths,
        &new_object_path,
        compression,
        max_row_group_size,
        deduplicate,
    )
    .await
}

/// Combines multiple Parquet files from object store
///
/// # Errors
///
/// Returns an error if file reading or writing fails.
pub async fn combine_parquet_files_from_object_store(
    object_store: Arc<dyn ObjectStore>,
    file_paths: Vec<ObjectPath>,
    new_file_path: &ObjectPath,
    compression: Option<parquet::basic::Compression>,
    max_row_group_size: Option<usize>,
    deduplicate: Option<bool>,
) -> anyhow::Result<()> {
    if file_paths.len() <= 1 {
        return Ok(());
    }

    let mut all_batches: Vec<RecordBatch> = Vec::new();
    let mut schema_with_metadata: Option<Arc<arrow::datatypes::Schema>> = None;
    let mut schema_source: Option<&ObjectPath> = None;
    let mut field_metadata_sources = HashMap::new();

    // Read all files from object store
    for path in &file_paths {
        let result: object_store::GetResult = object_store.get(path).await?;
        let data = result.bytes().await?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(data)?;

        let candidate_schema = builder.schema().clone();
        schema_with_metadata = Some(
            if let (Some(schema), Some(source)) = (&schema_with_metadata, schema_source) {
                let reconciled = reconcile_consolidation_schema_with_sources(
                    schema,
                    source,
                    &field_metadata_sources,
                    &candidate_schema,
                    path,
                )?;

                if reconciled.schema_source == ConsolidationSchemaSource::Candidate {
                    schema_source = Some(path);
                }

                for key in reconciled.candidate_field_metadata {
                    field_metadata_sources.insert(key, path.clone());
                }

                reconciled.schema
            } else {
                schema_source = Some(path);
                field_metadata_sources
                    .extend(field_metadata_keys(&candidate_schema).map(|key| (key, path.clone())));
                candidate_schema
            },
        );

        let mut reader = builder.build()?;

        for batch in reader.by_ref() {
            all_batches.push(batch?);
        }
    }

    // Re-apply the preserved schema metadata to all collected batches so that
    // write_batches_to_object_store (which uses batches[0].schema()) can encode
    // the correct Arrow schema metadata into the combined output file.
    if let Some(schema) = &schema_with_metadata {
        all_batches = all_batches
            .into_iter()
            .map(|b| RecordBatch::try_new(schema.clone(), b.columns().to_vec()))
            .collect::<Result<Vec<_>, _>>()?;
    }

    // Deduplicate rows if requested
    let batches_to_write = if deduplicate.unwrap_or(false) {
        deduplicate_record_batches(&all_batches)?
    } else {
        all_batches
    };

    // Write combined batches to new location
    write_batches_to_object_store(
        &batches_to_write,
        object_store.clone(),
        new_file_path,
        compression,
        max_row_group_size,
        None,
    )
    .await?;

    // Remove the merged files
    for path in &file_paths {
        if path != new_file_path {
            object_store.delete(path).await?;
        }
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConsolidationSchemaSource {
    Current,
    Candidate,
}

#[derive(Debug)]
struct ReconciledConsolidationSchema {
    schema: Arc<Schema>,
    schema_source: ConsolidationSchemaSource,
    candidate_field_metadata: Vec<(String, String)>,
}

#[cfg(test)]
fn reconcile_consolidation_schema(
    current: &Arc<Schema>,
    current_path: &ObjectPath,
    candidate: &Arc<Schema>,
    candidate_path: &ObjectPath,
) -> anyhow::Result<ReconciledConsolidationSchema> {
    let field_metadata_sources = field_metadata_keys(current)
        .map(|key| (key, current_path.clone()))
        .collect();
    reconcile_consolidation_schema_with_sources(
        current,
        current_path,
        &field_metadata_sources,
        candidate,
        candidate_path,
    )
}

fn reconcile_consolidation_schema_with_sources(
    current: &Arc<Schema>,
    current_path: &ObjectPath,
    current_field_metadata_sources: &HashMap<(String, String), ObjectPath>,
    candidate: &Arc<Schema>,
    candidate_path: &ObjectPath,
) -> anyhow::Result<ReconciledConsolidationSchema> {
    anyhow::ensure!(
        current.fields().len() == candidate.fields().len(),
        "Cannot consolidate Parquet files {current_path} and {candidate_path}: field schemas differ"
    );
    let mut candidate_field_metadata = Vec::new();
    let fields = current
        .fields()
        .iter()
        .zip(candidate.fields())
        .map(|(current, candidate)| {
            anyhow::ensure!(
                current.name() == candidate.name()
                    && current.data_type() == candidate.data_type()
                    && current.is_nullable() == candidate.is_nullable(),
                "Cannot consolidate Parquet files {current_path} and {candidate_path}: field schemas differ"
            );
            let mut metadata = current.metadata().clone();
            for (key, value) in candidate.metadata() {
                if let Some(current_value) = metadata.get(key) {
                    let source = current_field_metadata_sources
                        .get(&(current.name().clone(), key.clone()))
                        .unwrap_or(current_path);
                    anyhow::ensure!(
                        current_value == value,
                        "Cannot consolidate Parquet files {source} and {candidate_path}: field '{}' metadata differs",
                        current.name(),
                    );
                } else {
                    metadata.insert(key.clone(), value.clone());
                    candidate_field_metadata.push((current.name().clone(), key.clone()));
                }
            }
            Ok(Arc::new(current.as_ref().clone().with_metadata(metadata)))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let schema_with_fields =
        |metadata| Arc::new(Schema::new_with_metadata(fields.clone(), metadata));

    if current.metadata() == candidate.metadata() {
        return Ok(ReconciledConsolidationSchema {
            schema: schema_with_fields(current.metadata().clone()),
            schema_source: ConsolidationSchemaSource::Current,
            candidate_field_metadata,
        });
    }

    let without_precision = |schema: &Schema| {
        let mut metadata = schema.metadata().clone();
        metadata.remove(KEY_PRICE_PRECISION);
        metadata.remove(KEY_SIZE_PRECISION);
        metadata
    };
    let current_metadata = current.metadata();
    let candidate_metadata = candidate.metadata();
    let current_fallback = current_metadata
        .get(KEY_PRICE_PRECISION)
        .map(String::as_str)
        == Some("0")
        && current_metadata.get(KEY_SIZE_PRECISION).map(String::as_str) == Some("0");
    let current_has_precision = current_metadata.contains_key(KEY_PRICE_PRECISION)
        && current_metadata.contains_key(KEY_SIZE_PRECISION);
    let candidate_fallback = candidate_metadata
        .get(KEY_PRICE_PRECISION)
        .map(String::as_str)
        == Some("0")
        && candidate_metadata
            .get(KEY_SIZE_PRECISION)
            .map(String::as_str)
            == Some("0");
    let candidate_has_precision = candidate_metadata.contains_key(KEY_PRICE_PRECISION)
        && candidate_metadata.contains_key(KEY_SIZE_PRECISION);

    if without_precision(current) == without_precision(candidate) {
        match (current_fallback, candidate_fallback) {
            (true, false) if candidate_has_precision => {
                return Ok(ReconciledConsolidationSchema {
                    schema: schema_with_fields(candidate.metadata().clone()),
                    schema_source: ConsolidationSchemaSource::Candidate,
                    candidate_field_metadata,
                });
            }
            (false, true) if current_has_precision => {
                return Ok(ReconciledConsolidationSchema {
                    schema: schema_with_fields(current.metadata().clone()),
                    schema_source: ConsolidationSchemaSource::Current,
                    candidate_field_metadata,
                });
            }
            _ => {}
        }
    }

    anyhow::bail!(
        "Cannot consolidate Parquet files {current_path} and {candidate_path}: schema metadata differs: {current_metadata:?} versus {candidate_metadata:?}"
    )
}

fn field_metadata_keys(schema: &Schema) -> impl Iterator<Item = (String, String)> + '_ {
    schema.fields().iter().flat_map(|field| {
        field
            .metadata()
            .keys()
            .map(|key| (field.name().clone(), key.clone()))
    })
}

/// Extracts the minimum and maximum i64 values for the specified `column_name` from a Parquet file's metadata using object store with storage options.
///
/// # Errors
///
/// Returns an error if the file cannot be read, metadata parsing fails, or the column is missing or has no statistics.
pub async fn min_max_from_parquet_metadata(
    file_path: &str,
    storage_options: Option<AHashMap<String, String>>,
    column_name: &str,
) -> anyhow::Result<(u64, u64)> {
    let (object_store, base_path, _) = create_object_store_from_path(file_path, storage_options)?;
    let object_path = if base_path.is_empty() {
        ObjectPath::from(file_path)
    } else {
        ObjectPath::from(format!("{base_path}/{file_path}"))
    };

    min_max_from_parquet_metadata_object_store(object_store, &object_path, column_name).await
}

/// Extracts the minimum and maximum i64 values for the specified `column_name` from a Parquet file's metadata in object store.
///
/// # Errors
///
/// Returns an error if the file cannot be read, metadata parsing fails, or the column is missing or has no statistics.
pub async fn min_max_from_parquet_metadata_object_store(
    object_store: Arc<dyn ObjectStore>,
    file_path: &ObjectPath,
    column_name: &str,
) -> anyhow::Result<(u64, u64)> {
    // Download the parquet file from object store
    let result: object_store::GetResult = object_store.get(file_path).await?;
    let data = result.bytes().await?;
    let reader = SerializedFileReader::new(data)?;

    let metadata = reader.metadata();
    let mut overall_min_value: Option<i64> = None;
    let mut overall_max_value: Option<i64> = None;

    // Iterate through all row groups
    for i in 0..metadata.num_row_groups() {
        let row_group = metadata.row_group(i);

        // Iterate through all columns in this row group
        for j in 0..row_group.num_columns() {
            let col_metadata = row_group.column(j);

            if col_metadata.column_path().string() == column_name {
                if let Some(stats) = col_metadata.statistics() {
                    // Check if we have Int64 statistics
                    if let Statistics::Int64(int64_stats) = stats {
                        // Extract min value if available
                        if let Some(&min_value) = int64_stats.min_opt()
                            && (overall_min_value.is_none()
                                || min_value < overall_min_value.unwrap())
                        {
                            overall_min_value = Some(min_value);
                        }

                        // Extract max value if available
                        if let Some(&max_value) = int64_stats.max_opt()
                            && (overall_max_value.is_none()
                                || max_value > overall_max_value.unwrap())
                        {
                            overall_max_value = Some(max_value);
                        }
                    } else {
                        anyhow::bail!("Warning: Column name '{column_name}' is not of type i64.");
                    }
                } else {
                    anyhow::bail!(
                        "Warning: Statistics not available for column '{column_name}' in row group {i}."
                    );
                }
            }
        }
    }

    // Return the min/max pair if both are available
    if let (Some(min), Some(max)) = (overall_min_value, overall_max_value) {
        Ok((u64::try_from(min)?, u64::try_from(max)?))
    } else {
        anyhow::bail!(
            "Column '{column_name}' not found or has no Int64 statistics in any row group."
        )
    }
}

/// Creates an object store from a URI string with optional storage options.
///
/// Supports multiple cloud storage providers:
/// - AWS S3: `s3://bucket/path`
/// - Google Cloud Storage: `gs://bucket/path` or `gcs://bucket/path`
/// - Azure Blob Storage: `az://account/container/path` or `abfs://container@account.dfs.core.windows.net/path`
/// - HTTP/WebDAV: `http://` or `https://`
/// - Local files: `file://path` or plain paths
///
/// # Parameters
///
/// - `path`: The URI string for the storage location.
/// - `storage_options`: Optional `HashMap` containing storage-specific configuration options:
///   - For S3: `endpoint_url`, region, `access_key_id`, `secret_access_key`, `session_token`, etc.
///   - For GCS: `service_account_path`, `service_account_key`, `project_id`, etc.
///   - For Azure: `account_name`, `account_key`, `sas_token`, etc.
///
/// Returns a tuple of (`ObjectStore`, `base_path`, `normalized_uri`)
pub fn create_object_store_from_path(
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let location = create_object_store_location_from_path(path, storage_options)?;
    Ok((
        location.object_store,
        location.base_path,
        location.original_uri,
    ))
}

// `storage_options` is only consumed by the cloud-feature arms,
// so keep the allow scoped to the no-cloud build.
#[cfg_attr(
    not(feature = "cloud"),
    allow(unused_variables, clippy::needless_pass_by_value)
)]
pub(crate) fn create_object_store_location_from_path(
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<ObjectStoreLocation> {
    let uri = normalize_path_to_uri(path)?;

    let (object_store, base_path, original_uri) = match uri.as_str() {
        #[cfg(feature = "cloud")]
        s if s.starts_with("s3://") => create_s3_store(&uri, storage_options),
        #[cfg(feature = "cloud")]
        s if s.starts_with("gs://") || s.starts_with("gcs://") => {
            create_gcs_store(&uri, storage_options)
        }
        #[cfg(feature = "cloud")]
        s if s.starts_with("az://") => create_azure_store(&uri, storage_options),
        #[cfg(feature = "cloud")]
        s if s.starts_with("abfs://") => create_abfs_store(&uri, storage_options),
        #[cfg(feature = "cloud")]
        s if s.starts_with("http://") || s.starts_with("https://") => {
            create_http_store(&uri, storage_options)
        }
        #[cfg(not(feature = "cloud"))]
        s if s.starts_with("s3://")
            || s.starts_with("gs://")
            || s.starts_with("gcs://")
            || s.starts_with("az://")
            || s.starts_with("abfs://")
            || s.starts_with("http://")
            || s.starts_with("https://") =>
        {
            anyhow::bail!("Cloud storage support requires the 'cloud' feature: {uri}")
        }
        s if s.starts_with("file://") => create_local_store(&uri, true),
        _ => create_local_store(&uri, false), // Fallback: assume local path
    }?;

    let store_root_url = Url::parse(&original_uri)
        .ok()
        .filter(|url| is_remote_uri_scheme(url.scheme()))
        .map(|_| remote_store_root_url(&original_uri))
        .transpose()?;
    Ok(ObjectStoreLocation {
        object_store,
        base_path,
        original_uri,
        store_root_url,
    })
}

/// Normalizes a path to URI format for consistent object store usage.
///
/// If the path is already a URI (contains "://"), returns it as-is.
/// Otherwise, converts local paths to file:// URIs with proper cross-platform handling.
///
/// Supported URI schemes:
/// - `s3://` for AWS S3
/// - `gs://` or `gcs://` for Google Cloud Storage
/// - `az://` or `abfs://` for Azure Blob Storage
/// - `http://` or `https://` for HTTP/WebDAV
/// - `file://` for local files
///
/// # Cross-platform Path Handling
///
/// - Unix absolute paths: `/path/to/file` → `file:///path/to/file`
/// - Windows drive paths: `C:\path\to\file` → `file:///C:/path/to/file`
/// - Windows UNC paths: `\\server\share\file` → `file://server/share/file`
/// - Relative paths: converted to absolute using current directory
///
/// # Errors
///
/// Returns an error if the path is relative and the current working directory cannot be
/// resolved.
pub fn normalize_path_to_uri(path: &str) -> anyhow::Result<String> {
    if path.contains("://") {
        // Already a URI - return as-is
        Ok(path.to_string())
    } else if is_absolute_path(path) {
        Ok(path_to_file_uri(path))
    } else {
        // Relative path - make it absolute first
        let cwd = std::env::current_dir().map_err(|e| {
            anyhow::anyhow!("Failed to resolve current directory for relative path '{path}': {e}")
        })?;
        let absolute_path = cwd.join(path);
        Ok(path_to_file_uri(&absolute_path.to_string_lossy()))
    }
}

/// Checks if a path is absolute on the current platform.
#[must_use]
fn is_absolute_path(path: &str) -> bool {
    if path.starts_with('/') {
        // Unix absolute path
        true
    } else if path.len() >= 3
        && path.chars().nth(1) == Some(':')
        && path.chars().nth(2) == Some('\\')
    {
        // Windows drive path like C:\
        true
    } else if path.len() >= 3
        && path.chars().nth(1) == Some(':')
        && path.chars().nth(2) == Some('/')
    {
        // Windows drive path with forward slashes like C:/
        true
    } else if path.starts_with("\\\\") {
        // Windows UNC path
        true
    } else {
        false
    }
}

/// Converts an absolute path to a file:// URI with proper platform handling.
#[must_use]
fn path_to_file_uri(path: &str) -> String {
    if path.starts_with('/') {
        // Unix absolute path
        format!("file://{path}")
    } else if path.len() >= 3 && path.chars().nth(1) == Some(':') {
        // Windows drive path - normalize separators and add proper prefix
        let normalized = path.replace('\\', "/");
        format!("file:///{normalized}")
    } else if let Some(without_prefix) = path.strip_prefix("\\\\") {
        // Windows UNC path \\server\share -> file://server/share
        let normalized = without_prefix.replace('\\', "/");
        format!("file://{normalized}")
    } else {
        // Fallback - treat as relative to root
        format!("file://{path}")
    }
}

/// Converts a file:// URI to a native path for the current platform.
/// On Windows, "file:///C:/x/y" becomes "C:\x\y" so LocalFileSystem and std::fs work correctly.
#[cfg(windows)]
pub(crate) fn file_uri_to_native_path(uri: &str) -> String {
    let without_scheme = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("file:"))
        .unwrap_or(uri);
    // Strip leading slash so "/C:/x/y" -> "C:/x/y", then use native separators
    let without_leading = without_scheme.trim_start_matches('/');
    without_leading.replace('/', "\\")
}

/// Converts a file:// URI to a path string for Unix (no-op; `object_store` accepts slash paths).
#[cfg(not(windows))]
pub(crate) fn file_uri_to_native_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

/// Appends an encoded object-store path to the local storage URI.
/// Preserve the encoded names used by the native object-store backend.
pub(crate) fn append_path_to_file_uri(base_uri: &str, path: &str) -> String {
    if let Ok(mut url) = Url::parse(base_uri) {
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty();
            segments.extend(
                path.trim_end_matches('/')
                    .split('/')
                    .filter(|segment| !segment.is_empty()),
            );
        }
        return url.to_string();
    }

    format!(
        "{}/{}",
        base_uri.trim_end_matches('/'),
        path.trim_end_matches('/')
    )
}

/// Decodes a percent-encoded `object_store` path segment back to its logical form.
///
/// `object_store` lists path segments in URL-encoded form, so a non-ASCII instrument
/// directory reads back with each non-ASCII byte as a `%XX` sequence. Decoding recovers the
/// original id for matching against `urisafe_instrument_id`. Returns the input unchanged when
/// it is not valid percent-encoded UTF-8.
pub(crate) fn decode_object_store_segment(segment: &str) -> String {
    object_store::path::Path::from_url_path(segment)
        .map_or_else(|_| segment.to_string(), String::from)
}

fn create_local_store(
    uri: &str,
    is_file_uri: bool,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let path = if is_file_uri {
        file_uri_to_native_path(uri)
    } else {
        uri.to_string()
    };

    let local_store = object_store::local::LocalFileSystem::new_with_prefix(&path)?;
    Ok((Arc::new(local_store), String::new(), uri.to_string()))
}

/// Helper function to create S3 object store with options.
#[cfg(feature = "cloud")]
fn create_s3_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid S3 URI: missing bucket")?;

    let mut builder = object_store::aws::AmazonS3Builder::new().with_bucket_name(&bucket);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, value) in options {
            match key.as_str() {
                // Accept legacy storage-option aliases alongside native names.
                "endpoint_url" | "endpoint" => {
                    builder = builder.with_endpoint(&value);
                }
                "region" => {
                    builder = builder.with_region(&value);
                }
                "access_key_id" | "key" => {
                    builder = builder.with_access_key_id(&value);
                }
                "secret_access_key" | "secret" => {
                    builder = builder.with_secret_access_key(&value);
                }
                "session_token" | "token" => {
                    builder = builder.with_token(&value);
                }
                "allow_http" => {
                    let allow_http = value.to_lowercase() == "true";
                    builder = builder.with_allow_http(allow_http);
                }
                _ => {
                    // Ignore unknown options for forward compatibility
                    log::warn!("Unknown S3 storage option: {key}");
                }
            }
        }
    }

    let s3_store = builder.build()?;
    Ok((Arc::new(s3_store), path, uri.to_string()))
}

/// Helper function to create GCS object store with options.
#[cfg(feature = "cloud")]
fn create_gcs_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let bucket = extract_host(&url, "Invalid GCS URI: missing bucket")?;

    let mut builder = object_store::gcp::GoogleCloudStorageBuilder::new().with_bucket_name(&bucket);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, value) in options {
            match key.as_str() {
                "service_account_path" | "credential_path" => {
                    builder = builder.with_service_account_path(&value);
                }
                "service_account_key" => {
                    builder = builder.with_service_account_key(&value);
                }
                "project_id" => {
                    // Note: GoogleCloudStorageBuilder doesn't have with_project_id method
                    // This would need to be handled via environment variables or service account
                    log::warn!(
                        "project_id should be set via service account or environment variables"
                    );
                }
                "application_credentials" => {
                    // Set GOOGLE_APPLICATION_CREDENTIALS env var required by Google auth libraries.
                    // SAFETY: std::env::set_var is marked unsafe because it mutates global state and
                    // can break signal-safe code. We only call it during configuration before any
                    // multi-threaded work starts, so it is considered safe in this context.
                    unsafe {
                        std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", &value);
                    }
                }
                _ => {
                    // Ignore unknown options for forward compatibility
                    log::warn!("Unknown GCS storage option: {key}");
                }
            }
        }
    }

    let gcs_store = builder.build()?;
    Ok((Arc::new(gcs_store), path, uri.to_string()))
}

/// Helper function to create Azure object store with options.
#[cfg(feature = "cloud")]
fn create_azure_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, _) = parse_url_and_path(uri)?;
    let container = extract_host(&url, "Invalid Azure URI: missing container")?;

    let path = url.path().trim_start_matches('/').to_string();

    let mut builder =
        object_store::azure::MicrosoftAzureBuilder::new().with_container_name(container);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        builder = apply_azure_storage_options(builder, options, "Azure");
    }

    let azure_store = builder.build()?;
    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Helper function to create Azure object store from abfs:// URI with options.
#[cfg(feature = "cloud")]
fn create_abfs_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (url, path) = parse_url_and_path(uri)?;
    let host = extract_host(&url, "Invalid ABFS URI: missing host")?;

    // Extract account from host (account.dfs.core.windows.net)
    let account = host
        .split('.')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid ABFS URI: cannot extract account from host"))?;

    // Extract container from username part
    let container = url
        .username()
        .split('@')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid ABFS URI: missing container"))?;

    let mut builder = object_store::azure::MicrosoftAzureBuilder::new()
        .with_account(account)
        .with_container_name(container);

    // Apply storage options if provided (same as Azure store)
    if let Some(options) = storage_options {
        builder = apply_azure_storage_options(builder, options, "ABFS");
    }

    let azure_store = builder.build()?;
    Ok((Arc::new(azure_store), path, uri.to_string()))
}

/// Applies shared Azure storage options to the builder; `store_label` names the URI
/// scheme ("Azure" or "ABFS") in unknown-option warnings.
#[cfg(feature = "cloud")]
fn apply_azure_storage_options(
    mut builder: object_store::azure::MicrosoftAzureBuilder,
    options: AHashMap<String, String>,
    store_label: &str,
) -> object_store::azure::MicrosoftAzureBuilder {
    for (key, value) in options {
        match key.as_str() {
            "account_name" => {
                builder = builder.with_account(&value);
            }
            "account_key" => {
                builder = builder.with_access_key(&value);
            }
            "sas_token" => {
                // Parse SAS token as query string parameters
                let query_pairs: Vec<(String, String)> = value
                    .split('&')
                    .filter_map(|pair| {
                        let mut parts = pair.split('=');
                        match (parts.next(), parts.next()) {
                            (Some(key), Some(val)) => Some((key.to_string(), val.to_string())),
                            _ => None,
                        }
                    })
                    .collect();
                builder = builder.with_sas_authorization(query_pairs);
            }
            "client_id" => {
                builder = builder.with_client_id(&value);
            }
            "client_secret" => {
                builder = builder.with_client_secret(&value);
            }
            "tenant_id" => {
                builder = builder.with_tenant_id(&value);
            }
            _ => {
                // Ignore unknown options for forward compatibility
                log::warn!("Unknown {store_label} storage option: {key}");
            }
        }
    }

    builder
}

/// Helper function to create HTTP object store with options.
#[cfg(feature = "cloud")]
fn create_http_store(
    uri: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<(Arc<dyn ObjectStore>, String, String)> {
    let (_, path) = parse_url_and_path(uri)?;
    let base_url = remote_store_root_url(uri)?
        .as_str()
        .trim_end_matches('/')
        .to_string();

    let builder = object_store::http::HttpBuilder::new().with_url(base_url);

    // Apply storage options if provided
    if let Some(options) = storage_options {
        for (key, _value) in options {
            // HTTP builder has limited configuration options
            // Most HTTP-specific options would be handled via client options
            // Ignore unknown options for forward compatibility
            log::warn!("Unknown HTTP storage option: {key}");
        }
    }

    let http_store = builder.build()?;
    Ok((Arc::new(http_store), path, uri.to_string()))
}

/// Helper function to parse URL and extract path component.
#[cfg(feature = "cloud")]
fn parse_url_and_path(uri: &str) -> anyhow::Result<(url::Url, String)> {
    let url = url::Url::parse(uri)?;
    let path = url.path().trim_start_matches('/').to_string();
    Ok((url, path))
}

/// Helper function to extract host from URL with error handling.
#[cfg(feature = "cloud")]
fn extract_host(url: &url::Url, error_msg: &str) -> anyhow::Result<String> {
    url.host_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("{error_msg}"))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    #[cfg(feature = "cloud")]
    use ahash::AHashMap;
    use arrow::{
        array::{
            ArrayRef, FixedSizeBinaryArray, ListArray, StringArray, StringDictionaryBuilder,
            UInt8Array, UInt32Array, UInt64Array,
        },
        datatypes::{DataType, Field, Int8Type, Schema},
    };
    use nautilus_serialization::arrow::json_string_field;
    use parquet::file::{properties::ReaderProperties, serialized_reader::ReadOptionsBuilder};
    use rstest::rstest;

    use super::*;

    fn consolidation_depth_schema(
        price_precision: &str,
        size_precision: &str,
        instrument_id: &str,
    ) -> Arc<Schema> {
        Arc::new(Schema::new_with_metadata(
            vec![Field::new("bids", DataType::Utf8, false)],
            HashMap::from([
                (KEY_PRICE_PRECISION.to_string(), price_precision.to_string()),
                (KEY_SIZE_PRECISION.to_string(), size_precision.to_string()),
                (KEY_IDENTIFIER.to_string(), instrument_id.to_string()),
            ]),
        ))
    }

    #[rstest]
    fn consolidation_schema_prefers_populated_depth_precision_in_either_order() {
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let populated = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let fallback_path = ObjectPath::from("empty.parquet");
        let populated_path = ObjectPath::from("populated.parquet");

        let fallback_first =
            reconcile_consolidation_schema(&fallback, &fallback_path, &populated, &populated_path)
                .unwrap();
        let populated_first =
            reconcile_consolidation_schema(&populated, &populated_path, &fallback, &fallback_path)
                .unwrap();

        assert_eq!(
            fallback_first.schema_source,
            ConsolidationSchemaSource::Candidate,
        );
        assert_eq!(
            populated_first.schema_source,
            ConsolidationSchemaSource::Current,
        );

        for schema in [fallback_first.schema, populated_first.schema] {
            assert_eq!(schema.metadata()[KEY_PRICE_PRECISION], "2");
            assert_eq!(schema.metadata()[KEY_SIZE_PRECISION], "3");
        }
    }

    #[rstest]
    fn consolidation_schema_rejects_other_metadata_mismatches() {
        let current = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let candidate = consolidation_depth_schema("2", "3", "BTCUSDT.BINANCE");
        let current_path = ObjectPath::from("eth.parquet");
        let candidate_path = ObjectPath::from("btc.parquet");

        let error =
            reconcile_consolidation_schema(&current, &current_path, &candidate, &candidate_path)
                .unwrap_err();

        assert!(error.to_string().contains("eth.parquet and btc.parquet"));
        assert!(error.to_string().contains("ETHUSDT.BINANCE"));
        assert!(error.to_string().contains("BTCUSDT.BINANCE"));
    }

    #[rstest]
    fn consolidation_schema_rejects_two_populated_precisions() {
        let current = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let candidate = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let current_path = ObjectPath::from("precision-2.parquet");
        let candidate_path = ObjectPath::from("precision-4.parquet");

        let error =
            reconcile_consolidation_schema(&current, &current_path, &candidate, &candidate_path)
                .unwrap_err();

        assert!(error.to_string().contains("precision-2.parquet"));
        assert!(error.to_string().contains("precision-4.parquet"));
    }

    #[rstest]
    fn consolidation_schema_keeps_fallback_for_all_empty_files() {
        let first = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let second = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");

        let reconciled = reconcile_consolidation_schema(
            &first,
            &ObjectPath::from("first-empty.parquet"),
            &second,
            &ObjectPath::from("second-empty.parquet"),
        )
        .unwrap();

        assert_eq!(reconciled.schema.metadata()[KEY_PRICE_PRECISION], "0");
        assert_eq!(reconciled.schema.metadata()[KEY_SIZE_PRECISION], "0");
    }

    #[rstest]
    fn consolidation_schema_rejects_missing_precision_against_fallback() {
        let missing = Arc::new(Schema::new_with_metadata(
            vec![Field::new("bids", DataType::Utf8, false)],
            HashMap::from([(KEY_IDENTIFIER.to_string(), "ETHUSDT.BINANCE".to_string())]),
        ));
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");

        let error = reconcile_consolidation_schema(
            &missing,
            &ObjectPath::from("missing.parquet"),
            &fallback,
            &ObjectPath::from("fallback.parquet"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("schema metadata differs"));
    }

    #[rstest]
    fn normalize_legacy_info_schema_makes_binary_info_nullable() {
        let schema = Schema::new(vec![Field::new("info", DataType::Binary, false)]);

        let normalized = normalize_legacy_parquet_schema(&schema);

        let info = normalized.field_with_name("info").unwrap();
        assert_eq!(info.data_type(), &DataType::Utf8);
        assert!(info.is_nullable());
    }

    #[rstest]
    fn consolidation_schema_merges_json_field_annotation_in_either_order() {
        let bare = Arc::new(Schema::new(vec![Field::new("info", DataType::Utf8, true)]));
        let annotated = Arc::new(Schema::new(vec![json_string_field("info", true)]));
        let bare_path = ObjectPath::from("bare.parquet");
        let annotated_path = ObjectPath::from("annotated.parquet");

        let bare_first =
            reconcile_consolidation_schema(&bare, &bare_path, &annotated, &annotated_path).unwrap();
        let annotated_first =
            reconcile_consolidation_schema(&annotated, &annotated_path, &bare, &bare_path).unwrap();

        assert_eq!(bare_first.schema, annotated_first.schema);
        assert_eq!(
            bare_first.schema.field_with_name("info").unwrap(),
            &json_string_field("info", true),
        );
    }

    #[rstest]
    fn consolidation_field_metadata_conflict_names_the_winning_file() {
        let bare = Arc::new(Schema::new(vec![Field::new("info", DataType::Utf8, true)]));
        let annotated = Arc::new(Schema::new(vec![json_string_field("info", true)]));
        let conflicting = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true).with_metadata(HashMap::from([(
                "ARROW:extension:name".to_string(),
                "other.extension".to_string(),
            )])),
        ]));
        let bare_path = ObjectPath::from("bare.parquet");
        let annotated_path = ObjectPath::from("annotated.parquet");
        let conflicting_path = ObjectPath::from("conflicting.parquet");
        let reconciled =
            reconcile_consolidation_schema(&bare, &bare_path, &annotated, &annotated_path).unwrap();

        assert_eq!(reconciled.candidate_field_metadata.len(), 2);
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("info".to_string(), "ARROW:extension:name".to_string())),
        );
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("info".to_string(), "ARROW:extension:metadata".to_string())),
        );
        let field_metadata_sources = reconciled
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, annotated_path.clone()))
            .collect();
        let error = reconcile_consolidation_schema_with_sources(
            &reconciled.schema,
            &bare_path,
            &field_metadata_sources,
            &conflicting,
            &conflicting_path,
        )
        .unwrap_err();
        assert!(error.to_string().contains("annotated.parquet"));
        assert!(error.to_string().contains("conflicting.parquet"));
        assert!(error.to_string().contains("field 'info' metadata differs"));
    }

    #[rstest]
    fn consolidation_conflict_names_the_last_winning_schema() {
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let precision_2 = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let precision_4 = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let fallback_path = ObjectPath::from("fallback.parquet");
        let precision_2_path = ObjectPath::from("precision-2.parquet");
        let precision_4_path = ObjectPath::from("precision-4.parquet");
        let reconciled = reconcile_consolidation_schema(
            &fallback,
            &fallback_path,
            &precision_2,
            &precision_2_path,
        )
        .unwrap();

        let error = reconcile_consolidation_schema(
            &reconciled.schema,
            &precision_2_path,
            &precision_4,
            &precision_4_path,
        )
        .unwrap_err();

        assert!(error.to_string().contains("precision-2.parquet"));
        assert!(error.to_string().contains("precision-4.parquet"));
    }

    #[rstest]
    fn consolidation_field_metadata_does_not_replace_precision_source() {
        let precision_2 = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let fallback = Arc::new(Schema::new_with_metadata(
            vec![json_string_field("bids", false)],
            consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE")
                .metadata()
                .clone(),
        ));
        let precision_4 = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let precision_2_path = ObjectPath::from("precision-2.parquet");
        let fallback_path = ObjectPath::from("fallback-annotated.parquet");
        let precision_4_path = ObjectPath::from("precision-4.parquet");
        let reconciled = reconcile_consolidation_schema(
            &precision_2,
            &precision_2_path,
            &fallback,
            &fallback_path,
        )
        .unwrap();

        assert_eq!(reconciled.schema_source, ConsolidationSchemaSource::Current,);
        assert_eq!(reconciled.candidate_field_metadata.len(), 2);
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("bids".to_string(), "ARROW:extension:name".to_string())),
        );
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("bids".to_string(), "ARROW:extension:metadata".to_string())),
        );
        let field_metadata_sources = reconciled
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, fallback_path.clone()))
            .collect();
        let error = reconcile_consolidation_schema_with_sources(
            &reconciled.schema,
            &precision_2_path,
            &field_metadata_sources,
            &precision_4,
            &precision_4_path,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("precision-2.parquet"));
        assert!(message.contains("precision-4.parquet"));
        assert!(!message.contains("fallback-annotated.parquet"));
    }

    #[rstest]
    fn consolidation_field_metadata_tracks_each_origin() {
        let bare = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true),
            Field::new("balances", DataType::Utf8, true),
        ]));
        let info = Arc::new(Schema::new(vec![
            json_string_field("info", true),
            Field::new("balances", DataType::Utf8, true),
        ]));
        let balances = Arc::new(Schema::new(vec![
            json_string_field("info", true),
            json_string_field("balances", true),
        ]));
        let conflicting = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true).with_metadata(HashMap::from([(
                "ARROW:extension:name".to_string(),
                "other.extension".to_string(),
            )])),
            json_string_field("balances", true),
        ]));
        let bare_path = ObjectPath::from("bare.parquet");
        let info_path = ObjectPath::from("info.parquet");
        let balances_path = ObjectPath::from("balances.parquet");
        let conflicting_path = ObjectPath::from("conflicting.parquet");
        let with_info =
            reconcile_consolidation_schema(&bare, &bare_path, &info, &info_path).unwrap();
        let mut field_metadata_sources = with_info
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, info_path.clone()))
            .collect::<HashMap<_, _>>();
        let with_balances = reconcile_consolidation_schema_with_sources(
            &with_info.schema,
            &bare_path,
            &field_metadata_sources,
            &balances,
            &balances_path,
        )
        .unwrap();

        for key in with_balances.candidate_field_metadata {
            field_metadata_sources.insert(key, balances_path.clone());
        }

        let error = reconcile_consolidation_schema_with_sources(
            &with_balances.schema,
            &bare_path,
            &field_metadata_sources,
            &conflicting,
            &conflicting_path,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("info.parquet"));
        assert!(message.contains("conflicting.parquet"));
        assert!(!message.contains("balances.parquet"));
    }

    #[rstest]
    fn normalize_dictionary_string_columns_casts_string_dictionaries_to_utf8() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("AUD/USD.SIM").unwrap();
        builder.append("EUR/USD.SIM").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;
        let schema = Arc::new(Schema::new(vec![
            Field::new("instrument_id", dictionary.data_type().clone(), false),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                dictionary,
                Arc::new(UInt64Array::from(vec![1_u64, 2])) as ArrayRef,
            ],
        )
        .unwrap();

        let normalized = normalize_dictionary_string_columns(&batch).unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name("instrument_id")
                .unwrap()
                .data_type(),
            &DataType::Utf8,
        );
        let values = normalized
            .column_by_name("instrument_id")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .iter()
            .collect::<Vec<_>>();
        assert_eq!(values, vec![Some("AUD/USD.SIM"), Some("EUR/USD.SIM")]);
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_preserves_unrecognized_dictionary() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("alpha").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;
        let schema = Arc::new(Schema::new(vec![Field::new(
            "label",
            dictionary.data_type().clone(),
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![dictionary]).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_open_custom_columns_preserves_dictionary_with_type_metadata() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("alpha").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;
        let decimal = Arc::new(
            Decimal128Array::from(vec![Some(123_i128)])
                .with_precision_and_scale(38, 16)
                .unwrap(),
        ) as ArrayRef;
        let schema = Arc::new(Schema::new_with_metadata(
            vec![
                Field::new("label", dictionary.data_type().clone(), false),
                Field::new("price", decimal.data_type().clone(), false),
                Field::new("ts_recv", DataType::UInt64, false),
            ],
            HashMap::from([("type_name".to_string(), "CustomData".to_string())]),
        ));
        let batch = RecordBatch::try_new(
            schema,
            vec![dictionary, decimal, Arc::new(UInt64Array::from(vec![7]))],
        )
        .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_legacy_parquet_schema_preserves_unrecognized_fixed_binary() {
        let schema = Schema::new(vec![Field::new(
            "price",
            DataType::FixedSizeBinary(8),
            false,
        )]);

        let normalized = normalize_legacy_parquet_schema(&schema);

        assert_eq!(normalized, schema);
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_converts_binary_info_null_to_arrow_null() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Binary, true),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(BinaryArray::from_vec(vec![b"null".as_slice()])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1_u64])) as ArrayRef,
            ],
        )
        .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();
        let info = normalized
            .column_by_name("info")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name("info")
                .unwrap()
                .data_type(),
            &DataType::Utf8,
        );
        assert!(info.is_null(0));
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_preserves_quote_price_columns() {
        let decimal = DataType::Decimal128(38, 16);
        let schema = Arc::new(Schema::new(vec![
            Field::new("bid_price", decimal.clone(), false),
            Field::new("ask_price", decimal.clone(), false),
            Field::new("bid_size", decimal.clone(), false),
            Field::new("ask_size", decimal, false),
        ]));
        let values = || {
            Arc::new(
                Decimal128Array::from(vec![1_i128])
                    .with_precision_and_scale(38, 16)
                    .unwrap(),
            ) as ArrayRef
        };
        let batch =
            RecordBatch::try_new(schema.clone(), vec![values(), values(), values(), values()])
                .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized.schema(), schema);
        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_legacy_depth_flat_columns_builds_structured_sides() {
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for level in 0..DEPTH10_LEN {
                for (name, value) in [("price", 11_i128), ("size", 22_i128)] {
                    fields.push(Field::new(
                        format!("{side}_{name}_{level}"),
                        DataType::Decimal128(38, 16),
                        true,
                    ));
                    let value = (level == 0).then_some(value);
                    columns.push(Arc::new(
                        Decimal128Array::from(vec![value])
                            .with_precision_and_scale(38, 16)
                            .unwrap(),
                    ) as ArrayRef);
                }
                fields.push(Field::new(
                    format!("{side}_count_{level}"),
                    DataType::UInt32,
                    false,
                ));
                columns.push(Arc::new(UInt32Array::from(vec![33])) as ArrayRef);
                fields.push(Field::new(
                    format!("{side}_order_id_{level}"),
                    DataType::UInt64,
                    false,
                ));
                columns.push(Arc::new(UInt64Array::from(vec![44])) as ArrayRef);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(&normalized, 1, 11, 22, 33, 44);
    }

    #[rstest]
    fn normalize_legacy_depth_fixed_lists_builds_structured_sides() {
        let decimal_values = |value| {
            Arc::new(
                Decimal128Array::from(
                    (0..DEPTH10_LEN)
                        .map(|level| (level == 0).then_some(value))
                        .collect::<Vec<_>>(),
                )
                .with_precision_and_scale(38, 16)
                .unwrap(),
            ) as ArrayRef
        };
        let counts = Arc::new(UInt32Array::from(vec![33; DEPTH10_LEN])) as ArrayRef;
        let order_ids = Arc::new(UInt64Array::from(vec![44; DEPTH10_LEN])) as ArrayRef;
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for (name, column) in [
                ("price", depth_list_array(decimal_values(11), true)),
                ("size", depth_list_array(decimal_values(22), true)),
                ("count", depth_list_array(counts.clone(), false)),
                ("order_id", depth_list_array(order_ids.clone(), false)),
            ] {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(&normalized, 1, 11, 22, 33, 44);
    }

    #[rstest]
    fn normalize_legacy_depth_missing_counts_and_order_ids_uses_list_width() {
        const WIDTH: i32 = 3;
        let decimal_values = |value| {
            Arc::new(
                Decimal128Array::from(vec![value; WIDTH as usize])
                    .with_precision_and_scale(38, 16)
                    .unwrap(),
            ) as ArrayRef
        };
        let list = |values: ArrayRef| {
            Arc::new(FixedSizeListArray::new(
                Arc::new(Field::new("item", values.data_type().clone(), false)),
                WIDTH,
                values,
                None,
            )) as ArrayRef
        };
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for (name, column) in [
                ("price", list(decimal_values(11))),
                ("size", list(decimal_values(22))),
            ] {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(&normalized, WIDTH as usize, 11, 22, 0, 0);
    }

    #[rstest]
    #[case::with_order_ids(true, 44)]
    #[case::without_order_ids(false, 0)]
    fn normalize_legacy_depth_fixed_binary_lists_matches_schema(
        #[case] include_order_ids: bool,
        #[case] expected_order_id: u64,
    ) {
        let fixed_values = |value: [u8; 8]| {
            let values = (0..DEPTH10_LEN)
                .map(|level| (level == 0).then_some(value))
                .collect::<Vec<_>>();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    values
                        .iter()
                        .map(Option::as_ref)
                        .map(|value| value.map(<[u8; 8]>::as_slice)),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };
        let counts = Arc::new(UInt32Array::from(vec![33; DEPTH10_LEN])) as ArrayRef;
        let order_ids = Arc::new(UInt64Array::from(vec![44; DEPTH10_LEN])) as ArrayRef;
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            let mut side_columns = vec![
                (
                    "price",
                    depth_list_array(fixed_values(11_i64.to_le_bytes()), true),
                ),
                (
                    "size",
                    depth_list_array(fixed_values(22_u64.to_le_bytes()), true),
                ),
                ("count", depth_list_array(counts.clone(), false)),
            ];

            if include_order_ids {
                side_columns.push(("order_id", depth_list_array(order_ids.clone(), false)));
            }

            for (name, column) in side_columns {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        assert!(is_nautilus_legacy_schema(batch.schema_ref()));
        let normalized_schema = normalize_legacy_parquet_schema(batch.schema_ref());
        let normalized_batch = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized_schema,
            normalized_batch.schema().as_ref().clone()
        );
        assert_normalized_depth(
            &normalized_batch,
            1,
            110_000_000,
            220_000_000,
            33,
            expected_order_id,
        );
    }

    #[rstest]
    fn normalize_legacy_depth_flat_fixed_columns_preserves_order_ids() {
        let fixed_price = || {
            let bytes = 11_i64.to_le_bytes();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(bytes.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };
        let fixed_size = || {
            let bytes = 22_u64.to_le_bytes();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(bytes.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for level in 0..DEPTH10_LEN {
                fields.push(Field::new(
                    format!("{side}_price_{level}"),
                    DataType::FixedSizeBinary(8),
                    false,
                ));
                columns.push(fixed_price());
                fields.push(Field::new(
                    format!("{side}_size_{level}"),
                    DataType::FixedSizeBinary(8),
                    false,
                ));
                columns.push(fixed_size());
                fields.push(Field::new(
                    format!("{side}_count_{level}"),
                    DataType::UInt32,
                    false,
                ));
                columns.push(Arc::new(UInt32Array::from(vec![33])) as ArrayRef);
                fields.push(Field::new(
                    format!("{side}_order_id_{level}"),
                    DataType::UInt64,
                    false,
                ));
                columns.push(Arc::new(UInt64Array::from(vec![44])) as ArrayRef);
            }
        }

        for (field, column) in [
            (
                Field::new("flags", DataType::UInt8, false),
                Arc::new(UInt8Array::from(vec![0])) as ArrayRef,
            ),
            (
                Field::new("sequence", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![1])) as ArrayRef,
            ),
            (
                Field::new("ts_event", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![2])) as ArrayRef,
            ),
            (
                Field::new("ts_init", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![3])) as ArrayRef,
            ),
        ] {
            fields.push(field);
            columns.push(column);
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();
        assert!(is_nautilus_legacy_schema(batch.schema_ref()));

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        for side in ["bids", "asks"] {
            let list = normalized
                .column_by_name(side)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let levels = list.value(0);
            let levels = levels.as_any().downcast_ref::<StructArray>().unwrap();
            let order_ids = levels
                .column_by_name("order_id")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            assert_eq!(order_ids.values(), &[44; DEPTH10_LEN]);
        }
    }

    #[rstest]
    fn normalize_legacy_depth_fixture_matches_open_shape() {
        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus")
            .join(precision_dir)
            .join("depths.parquet");
        let file = std::fs::File::open(path).unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
        let metadata = builder
            .metadata()
            .file_metadata()
            .key_value_metadata()
            .unwrap();

        for (key, value) in [
            ("instrument_id", "AAPL.XNAS"),
            ("price_precision", "4"),
            ("size_precision", "1"),
        ] {
            assert_eq!(
                metadata
                    .iter()
                    .find(|entry| entry.key == key)
                    .and_then(|entry| entry.value.as_deref()),
                Some(value),
            );
        }
        let batch = builder.build().unwrap().next().unwrap().unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(
            &normalized,
            DEPTH10_LEN,
            12_345_000_000_000_000,
            25_000_000_000_000_000,
            3,
            0,
        );
        assert_eq!(normalized.num_columns(), 6);
        assert_eq!(
            normalized
                .column_by_name("flags")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt8Array>()
                .unwrap()
                .value(0),
            32
        );
        assert_eq!(
            normalized
                .column_by_name("sequence")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .value(0),
            7
        );

        for name in ["ts_event", "ts_init"] {
            assert_eq!(
                normalized
                    .schema()
                    .field_with_name(name)
                    .unwrap()
                    .data_type(),
                &DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, Some("UTC".into())),
            );
        }
    }

    #[rstest]
    #[case("quotes.parquet", "bid_price", None)]
    #[case("trades.parquet", "price", Some("aggressor_side"))]
    #[case("bars.parquet", "open", None)]
    #[case("deltas.parquet", "price", Some("action"))]
    fn legacy_market_fixture_matches_open_types(
        #[case] file_name: &str,
        #[case] fixed_field: &str,
        #[case] enum_field: Option<&str>,
    ) {
        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus")
            .join(precision_dir)
            .join(file_name);
        let file = std::fs::File::open(path).unwrap();
        let batch = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name(fixed_field)
                .unwrap()
                .data_type(),
            &DataType::Decimal128(38, 16),
        );
        assert_eq!(
            normalized
                .schema()
                .field_with_name("ts_init")
                .unwrap()
                .data_type(),
            &DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, Some("UTC".into())),
        );

        if let Some(enum_field) = enum_field {
            assert!(matches!(
                normalized
                    .schema()
                    .field_with_name(enum_field)
                    .unwrap()
                    .data_type(),
                DataType::Dictionary(_, value) if value.as_ref() == &DataType::Utf8
            ));
        }
    }

    #[rstest]
    #[case("depths.parquet")]
    #[case("quotes.parquet")]
    #[case("trades.parquet")]
    #[case("bars.parquet")]
    #[case("deltas.parquet")]
    #[case("dictionary-trade")]
    fn legacy_fixture_schema_normalization_matches_batch(#[case] file_name: &str) {
        let (schema, batch) = if file_name == "dictionary-trade" {
            let dictionary = |value: &str| {
                let mut builder = StringDictionaryBuilder::<Int8Type>::new();
                builder.append(value).unwrap();
                Arc::new(builder.finish()) as ArrayRef
            };
            let price = 11_i64.to_le_bytes();
            let size = 22_u64.to_le_bytes();
            let trade_ids = dictionary("trade-1");
            let identifiers = dictionary("AAPL.XNAS");
            let schema = Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("price", DataType::FixedSizeBinary(8), false),
                    Field::new("size", DataType::FixedSizeBinary(8), false),
                    Field::new("aggressor_side", DataType::UInt8, false),
                    Field::new("trade_id", trade_ids.data_type().clone(), false),
                    Field::new("ts_event", DataType::UInt64, false),
                    Field::new("ts_init", DataType::UInt64, false),
                    Field::new(KEY_IDENTIFIER, identifiers.data_type().clone(), false),
                ],
                HashMap::from([("type".to_string(), "TradeTick".to_string())]),
            ));
            let batch = RecordBatch::try_new(
                Arc::clone(&schema),
                vec![
                    Arc::new(
                        FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                            [Some(price.as_slice())].into_iter(),
                            8,
                        )
                        .unwrap(),
                    ),
                    Arc::new(
                        FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                            [Some(size.as_slice())].into_iter(),
                            8,
                        )
                        .unwrap(),
                    ),
                    Arc::new(UInt8Array::from(vec![1])),
                    trade_ids,
                    Arc::new(UInt64Array::from(vec![1])),
                    Arc::new(UInt64Array::from(vec![2])),
                    identifiers,
                ],
            )
            .unwrap();
            (schema, batch)
        } else {
            let precision_dir = if cfg!(feature = "high-precision") {
                "128-bit"
            } else {
                "64-bit"
            };
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test_data/nautilus")
                .join(precision_dir)
                .join(file_name);
            let file = std::fs::File::open(path).unwrap();
            let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
            let schema = builder.schema().clone();
            let batch = builder.build().unwrap().next().unwrap().unwrap();
            let batch =
                RecordBatch::try_new(Arc::clone(&schema), batch.columns().to_vec()).unwrap();
            (schema, batch)
        };
        let normalized_schema = normalize_legacy_parquet_schema(schema.as_ref());
        let normalized_batch = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized_schema,
            normalized_batch.schema().as_ref().clone()
        );

        if file_name == "dictionary-trade" {
            assert_eq!(
                normalized_batch
                    .schema()
                    .field_with_name("trade_id")
                    .unwrap()
                    .data_type(),
                &DataType::Utf8,
            );
        }
    }

    fn assert_normalized_depth(
        batch: &RecordBatch,
        level_count: usize,
        price: i128,
        size: i128,
        count: u32,
        order_id: u64,
    ) {
        let schema = batch.schema();
        assert_eq!(schema.field(0).name(), "bids");
        assert_eq!(schema.field(1).name(), "asks");

        for side in ["bids", "asks"] {
            let list = batch
                .column_by_name(side)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let levels = list.value(0);
            let levels = levels.as_any().downcast_ref::<StructArray>().unwrap();
            let prices = levels
                .column_by_name("price")
                .unwrap()
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let sizes = levels
                .column_by_name("size")
                .unwrap()
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let counts = levels
                .column_by_name("count")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt32Array>()
                .unwrap();
            let order_ids = levels
                .column_by_name("order_id")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();

            assert_eq!(levels.len(), level_count);
            assert_eq!(prices.value(0), price);
            assert_eq!(sizes.value(0), size);
            assert_eq!(counts.value(0), count);
            assert_eq!(order_ids.value(0), order_id);
        }
    }

    #[tokio::test]
    async fn default_writer_sets_zstd_sorting_and_identifier_bloom_filter() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.parquet");
        let object_store = Arc::new(
            object_store::local::LocalFileSystem::new_with_prefix(directory.path()).unwrap(),
        );
        let object_path = ObjectPath::from("layout.parquet");
        let schema = Arc::new(Schema::new(vec![
            Field::new("identifier", DataType::Utf8, false),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["AUD/USD.SIM", "AUD/USD.SIM"])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1_u64, 2])) as ArrayRef,
            ],
        )
        .unwrap();

        write_batches_to_object_store(&[batch], object_store, &object_path, None, None, None)
            .await
            .unwrap();

        let read_options = ReadOptionsBuilder::new()
            .with_reader_properties(
                ReaderProperties::builder()
                    .set_read_bloom_filter(true)
                    .build(),
            )
            .build();
        let reader = SerializedFileReader::new_with_options(
            std::fs::File::open(path).unwrap(),
            read_options,
        )
        .unwrap();
        let row_group = reader.metadata().row_group(0);
        let sorting = row_group.sorting_columns().unwrap();

        assert_eq!(crate::backend::parquet::DEFAULT_ROW_GROUP_SIZE, 131_072);
        assert_eq!(
            sorting,
            &vec![
                SortingColumn {
                    column_idx: 1,
                    descending: false,
                    nulls_first: false,
                },
                SortingColumn {
                    column_idx: 0,
                    descending: false,
                    nulls_first: false,
                },
            ],
        );
        assert!(
            row_group
                .columns()
                .iter()
                .all(|column| column.compression() == Compression::ZSTD(ZstdLevel::default())),
        );
        assert!(
            reader
                .get_row_group(0)
                .unwrap()
                .get_column_bloom_filter(0)
                .is_some(),
        );
    }

    #[rstest]
    fn test_create_object_store_from_path_local() {
        // Create a temporary directory for testing
        let temp_dir = std::env::temp_dir().join("nautilus_test");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = create_object_store_from_path(temp_dir.to_str().unwrap(), None);
        if let Err(e) = &result {
            println!("Error: {e:?}");
        }
        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "");
        // The URI should be normalized to file:// format
        assert_eq!(uri, format!("file://{}", temp_dir.to_str().unwrap()));

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_s3() {
        let mut options = AHashMap::new();
        options.insert(
            "endpoint_url".to_string(),
            "https://test.endpoint.com".to_string(),
        );
        options.insert("region".to_string(), "us-west-2".to_string());
        options.insert("access_key_id".to_string(), "test_key".to_string());
        options.insert("secret_access_key".to_string(), "test_secret".to_string());

        let result = create_object_store_from_path("s3://test-bucket/path", Some(options));
        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "path");
        assert_eq!(uri, "s3://test-bucket/path");
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_azure() {
        let mut options = AHashMap::new();
        options.insert("account_name".to_string(), "testaccount".to_string());
        // Use a valid base64 encoded key for testing
        options.insert("account_key".to_string(), "dGVzdGtleQ==".to_string()); // "testkey" in base64

        let result = create_object_store_from_path("az://container/path", Some(options));
        if let Err(e) = &result {
            println!("Azure Error: {e:?}");
        }
        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "path");
        assert_eq!(uri, "az://container/path");
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_gcs() {
        // Test GCS without service account (will use default credentials or fail gracefully)
        let mut options = AHashMap::new();
        options.insert("project_id".to_string(), "test-project".to_string());

        let result = create_object_store_from_path("gs://test-bucket/path", Some(options));
        // GCS might fail due to missing credentials, but we're testing the path parsing
        // The function should at least parse the URI correctly before failing on auth
        match result {
            Ok((_, base_path, uri)) => {
                assert_eq!(base_path, "path");
                assert_eq!(uri, "gs://test-bucket/path");
            }
            Err(e) => {
                // Expected to fail due to missing credentials, but should contain bucket info
                let error_msg = format!("{e:?}");
                assert!(error_msg.contains("test-bucket") || error_msg.contains("credential"));
            }
        }
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_create_object_store_from_path_empty_options() {
        let result = create_object_store_from_path("s3://test-bucket/path", None);
        assert!(result.is_ok());
        let (_, base_path, uri) = result.unwrap();
        assert_eq!(base_path, "path");
        assert_eq!(uri, "s3://test-bucket/path");
    }

    #[rstest]
    #[cfg(feature = "cloud")]
    fn test_remote_store_root_url_preserves_authority() {
        let https_root = remote_store_root_url("https://example.com:9000/base/path").unwrap();
        assert_eq!(
            https_root.as_str().trim_end_matches('/'),
            "https://example.com:9000"
        );

        let abfs_root =
            remote_store_root_url("abfs://container@account.dfs.core.windows.net/base/path")
                .unwrap();
        assert_eq!(
            abfs_root.as_str().trim_end_matches('/'),
            "abfs://container@account.dfs.core.windows.net"
        );

        let full_uri = remote_full_uri(
            "https://example.com:9000/base/path",
            "base/path/data/%5E/file.parquet",
        )
        .unwrap();
        assert_eq!(
            full_uri,
            "https://example.com:9000/base/path/data/%5E/file.parquet"
        );

        let location = create_object_store_location_from_path("s3://test-bucket/path", None)
            .expect("S3 location should be created");
        assert_eq!(location.base_path, "path");
        assert_eq!(
            remote_store_root_url(&location.original_uri)
                .expect("S3 should be remote")
                .as_str()
                .trim_end_matches('/'),
            "s3://test-bucket"
        );
    }

    #[rstest]
    fn test_normalize_path_to_uri() {
        // Unix absolute paths
        assert_eq!(
            normalize_path_to_uri("/tmp/test").unwrap(),
            "file:///tmp/test"
        );

        // Windows drive paths
        assert_eq!(
            normalize_path_to_uri("C:\\tmp\\test").unwrap(),
            "file:///C:/tmp/test"
        );
        assert_eq!(
            normalize_path_to_uri("C:/tmp/test").unwrap(),
            "file:///C:/tmp/test"
        );
        assert_eq!(
            normalize_path_to_uri("D:\\data\\file.txt").unwrap(),
            "file:///D:/data/file.txt"
        );

        // Windows UNC paths
        assert_eq!(
            normalize_path_to_uri("\\\\server\\share\\file").unwrap(),
            "file://server/share/file"
        );

        // Already URIs - should remain unchanged
        assert_eq!(
            normalize_path_to_uri("s3://bucket/path").unwrap(),
            "s3://bucket/path"
        );
        assert_eq!(
            normalize_path_to_uri("file:///tmp/test").unwrap(),
            "file:///tmp/test"
        );
        assert_eq!(
            normalize_path_to_uri("https://example.com/path").unwrap(),
            "https://example.com/path"
        );
    }

    #[rstest]
    fn test_is_absolute_path() {
        // Unix absolute paths
        assert!(is_absolute_path("/tmp/test"));
        assert!(is_absolute_path("/"));

        // Windows drive paths
        assert!(is_absolute_path("C:\\tmp\\test"));
        assert!(is_absolute_path("C:/tmp/test"));
        assert!(is_absolute_path("D:\\"));
        assert!(is_absolute_path("Z:/"));

        // Windows UNC paths
        assert!(is_absolute_path("\\\\server\\share"));
        assert!(is_absolute_path("\\\\localhost\\c$"));

        // Relative paths
        assert!(!is_absolute_path("tmp/test"));
        assert!(!is_absolute_path("./test"));
        assert!(!is_absolute_path("../test"));
        assert!(!is_absolute_path("test.txt"));

        // Edge cases
        assert!(!is_absolute_path(""));
        assert!(!is_absolute_path("C"));
        assert!(!is_absolute_path("C:"));
        assert!(!is_absolute_path("\\"));
    }

    #[rstest]
    fn test_path_to_file_uri() {
        // Unix absolute paths
        assert_eq!(path_to_file_uri("/tmp/test"), "file:///tmp/test");
        assert_eq!(path_to_file_uri("/"), "file:///");

        // Windows drive paths
        assert_eq!(path_to_file_uri("C:\\tmp\\test"), "file:///C:/tmp/test");
        assert_eq!(path_to_file_uri("C:/tmp/test"), "file:///C:/tmp/test");
        assert_eq!(path_to_file_uri("D:\\"), "file:///D:/");

        // Windows UNC paths
        assert_eq!(
            path_to_file_uri("\\\\server\\share\\file"),
            "file://server/share/file"
        );
        assert_eq!(
            path_to_file_uri("\\\\localhost\\c$\\test"),
            "file://localhost/c$/test"
        );
    }
}

#[cfg(test)]
mod migration_tests {
    use std::{collections::HashMap, sync::Arc};

    use arrow::{
        array::{
            ArrayRef, FixedSizeBinaryArray, ListArray, StringArray, StringDictionaryBuilder,
            TimestampNanosecondArray, UInt8Array, UInt32Array, UInt64Array,
        },
        datatypes::{DataType, Field, Int8Type, Schema, TimeUnit},
    };
    use nautilus_model::{
        data::QuoteTick,
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::{
        DecodeFromRecordBatch, EncodeToRecordBatch, json_string_field,
    };
    use parquet::file::{properties::ReaderProperties, serialized_reader::ReadOptionsBuilder};
    use rstest::rstest;

    use super::*;
    use crate::backend::parquet::DEFAULT_ROW_GROUP_SIZE;

    #[rstest]
    #[case::utc(Some("UTC"))]
    #[case::canonical(None)]
    fn v2_timestamp_normalization_matches_preflight_and_preserves_values(
        #[case] timezone: Option<&str>,
    ) {
        let first = QuoteTick {
            instrument_id: InstrumentId::from("AAPL.XNAS"),
            bid_price: Price::from("123.45"),
            ask_price: Price::from("123.67"),
            bid_size: Quantity::from(17),
            ask_size: Quantity::from(29),
            ts_event: 1_788_652_800_123_456_789_u64.into(),
            ts_init: 1_788_652_800_123_456_799_u64.into(),
        };
        let values = vec![
            first,
            QuoteTick {
                ts_event: 1_788_652_800_123_456_801_u64.into(),
                ts_init: 1_788_652_800_123_456_899_u64.into(),
                ..first
            },
        ];
        let metadata = QuoteTick::get_metadata(&first.instrument_id, 2, 0);
        let expected = QuoteTick::encode_batch(&metadata, &values).unwrap();
        let source_type = DataType::Timestamp(TimeUnit::Nanosecond, timezone.map(Into::into));
        let fields = expected
            .schema()
            .fields()
            .iter()
            .map(|field| {
                let field = field.as_ref().clone();
                if matches!(field.data_type(), DataType::Timestamp(_, _)) {
                    field.with_data_type(source_type.clone())
                } else {
                    field
                }
            })
            .collect::<Vec<_>>();
        let columns = expected
            .columns()
            .iter()
            .map(|column| {
                if let Some(timestamps) = column.as_any().downcast_ref::<TimestampNanosecondArray>()
                {
                    Arc::new(
                        timestamps
                            .clone()
                            .with_timezone_opt(timezone.map(Arc::<str>::from)),
                    ) as ArrayRef
                } else {
                    column.clone()
                }
            })
            .collect::<Vec<_>>();
        let source = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(fields, metadata.clone())),
            columns,
        )
        .unwrap();
        let preflight = normalize_legacy_parquet_schema(source.schema_ref());
        let normalized = normalize_legacy_parquet_columns(&source).unwrap();
        assert_eq!(&preflight, expected.schema_ref().as_ref());
        assert_eq!(normalized, expected);
        assert_eq!(
            QuoteTick::decode_batch(&metadata, normalized).unwrap(),
            values
        );
    }

    #[rstest]
    fn timestamp_normalization_preserves_custom_nulls_and_unrelated_numeric_fields() {
        let schema = Arc::new(Schema::new_with_metadata(
            vec![
                Field::new(
                    "ts_event",
                    DataType::Timestamp(TimeUnit::Nanosecond, None),
                    true,
                ),
                Field::new("ts_count", DataType::UInt64, false),
            ],
            HashMap::from([("type_name".to_string(), "TimestampSample".to_string())]),
        ));
        let timestamps =
            TimestampNanosecondArray::from(vec![Some(1_788_652_800_123_456_789), None]);
        let source = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(timestamps),
                Arc::new(UInt64Array::from(vec![17, 29])),
            ],
        )
        .unwrap();
        let normalized = normalize_legacy_parquet_columns(&source).unwrap();
        assert_eq!(
            &normalize_legacy_parquet_schema(source.schema_ref()),
            normalized.schema_ref().as_ref()
        );
        assert_eq!(
            normalized.schema().field(0).data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))
        );
        assert_eq!(
            normalized
                .column(0)
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            vec![Some(1_788_652_800_123_456_789), None]
        );
        assert_eq!(normalized.column(1), source.column(1));
    }

    fn consolidation_depth_schema(
        price_precision: &str,
        size_precision: &str,
        instrument_id: &str,
    ) -> Arc<Schema> {
        Arc::new(Schema::new_with_metadata(
            vec![Field::new("bids", DataType::Utf8, false)],
            HashMap::from([
                (KEY_PRICE_PRECISION.to_string(), price_precision.to_string()),
                (KEY_SIZE_PRECISION.to_string(), size_precision.to_string()),
                (KEY_IDENTIFIER.to_string(), instrument_id.to_string()),
            ]),
        ))
    }

    #[rstest]
    fn consolidation_schema_prefers_populated_depth_precision_in_either_order() {
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let populated = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let fallback_path = ObjectPath::from("empty.parquet");
        let populated_path = ObjectPath::from("populated.parquet");

        let fallback_first =
            reconcile_consolidation_schema(&fallback, &fallback_path, &populated, &populated_path)
                .unwrap();
        let populated_first =
            reconcile_consolidation_schema(&populated, &populated_path, &fallback, &fallback_path)
                .unwrap();

        assert_eq!(
            fallback_first.schema_source,
            ConsolidationSchemaSource::Candidate,
        );
        assert_eq!(
            populated_first.schema_source,
            ConsolidationSchemaSource::Current,
        );

        for schema in [fallback_first.schema, populated_first.schema] {
            assert_eq!(schema.metadata()[KEY_PRICE_PRECISION], "2");
            assert_eq!(schema.metadata()[KEY_SIZE_PRECISION], "3");
        }
    }

    #[rstest]
    fn consolidation_schema_rejects_other_metadata_mismatches() {
        let current = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let candidate = consolidation_depth_schema("2", "3", "BTCUSDT.BINANCE");
        let current_path = ObjectPath::from("eth.parquet");
        let candidate_path = ObjectPath::from("btc.parquet");

        let error =
            reconcile_consolidation_schema(&current, &current_path, &candidate, &candidate_path)
                .unwrap_err();

        assert!(error.to_string().contains("eth.parquet and btc.parquet"));
        assert!(error.to_string().contains("ETHUSDT.BINANCE"));
        assert!(error.to_string().contains("BTCUSDT.BINANCE"));
    }

    #[rstest]
    fn consolidation_schema_rejects_two_populated_precisions() {
        let current = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let candidate = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let current_path = ObjectPath::from("precision-2.parquet");
        let candidate_path = ObjectPath::from("precision-4.parquet");

        let error =
            reconcile_consolidation_schema(&current, &current_path, &candidate, &candidate_path)
                .unwrap_err();

        assert!(error.to_string().contains("precision-2.parquet"));
        assert!(error.to_string().contains("precision-4.parquet"));
    }

    #[rstest]
    fn consolidation_schema_keeps_fallback_for_all_empty_files() {
        let first = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let second = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");

        let reconciled = reconcile_consolidation_schema(
            &first,
            &ObjectPath::from("first-empty.parquet"),
            &second,
            &ObjectPath::from("second-empty.parquet"),
        )
        .unwrap();

        assert_eq!(reconciled.schema.metadata()[KEY_PRICE_PRECISION], "0");
        assert_eq!(reconciled.schema.metadata()[KEY_SIZE_PRECISION], "0");
    }

    #[rstest]
    fn consolidation_schema_rejects_missing_precision_against_fallback() {
        let missing = Arc::new(Schema::new_with_metadata(
            vec![Field::new("bids", DataType::Utf8, false)],
            HashMap::from([(KEY_IDENTIFIER.to_string(), "ETHUSDT.BINANCE".to_string())]),
        ));
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");

        let error = reconcile_consolidation_schema(
            &missing,
            &ObjectPath::from("missing.parquet"),
            &fallback,
            &ObjectPath::from("fallback.parquet"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("schema metadata differs"));
    }

    #[rstest]
    fn normalize_legacy_info_schema_makes_binary_info_nullable() {
        let schema = Schema::new(vec![Field::new("info", DataType::Binary, false)]);

        let normalized = normalize_legacy_parquet_schema(&schema);

        let info = normalized.field_with_name("info").unwrap();
        assert_eq!(info.data_type(), &DataType::Utf8);
        assert!(info.is_nullable());
    }

    #[rstest]
    fn consolidation_schema_merges_json_field_annotation_in_either_order() {
        let bare = Arc::new(Schema::new(vec![Field::new("info", DataType::Utf8, true)]));
        let annotated = Arc::new(Schema::new(vec![json_string_field("info", true)]));
        let bare_path = ObjectPath::from("bare.parquet");
        let annotated_path = ObjectPath::from("annotated.parquet");

        let bare_first =
            reconcile_consolidation_schema(&bare, &bare_path, &annotated, &annotated_path).unwrap();
        let annotated_first =
            reconcile_consolidation_schema(&annotated, &annotated_path, &bare, &bare_path).unwrap();

        assert_eq!(bare_first.schema, annotated_first.schema);
        assert_eq!(
            bare_first.schema.field_with_name("info").unwrap(),
            &json_string_field("info", true),
        );
    }

    #[rstest]
    fn consolidation_field_metadata_conflict_names_the_winning_file() {
        let bare = Arc::new(Schema::new(vec![Field::new("info", DataType::Utf8, true)]));
        let annotated = Arc::new(Schema::new(vec![json_string_field("info", true)]));
        let conflicting = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true).with_metadata(HashMap::from([(
                "ARROW:extension:name".to_string(),
                "other.extension".to_string(),
            )])),
        ]));
        let bare_path = ObjectPath::from("bare.parquet");
        let annotated_path = ObjectPath::from("annotated.parquet");
        let conflicting_path = ObjectPath::from("conflicting.parquet");
        let reconciled =
            reconcile_consolidation_schema(&bare, &bare_path, &annotated, &annotated_path).unwrap();

        assert_eq!(reconciled.candidate_field_metadata.len(), 2);
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("info".to_string(), "ARROW:extension:name".to_string())),
        );
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("info".to_string(), "ARROW:extension:metadata".to_string())),
        );
        let field_metadata_sources = reconciled
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, annotated_path.clone()))
            .collect();
        let error = reconcile_consolidation_schema_with_sources(
            &reconciled.schema,
            &bare_path,
            &field_metadata_sources,
            &conflicting,
            &conflicting_path,
        )
        .unwrap_err();
        assert!(error.to_string().contains("annotated.parquet"));
        assert!(error.to_string().contains("conflicting.parquet"));
        assert!(error.to_string().contains("field 'info' metadata differs"));
    }

    #[rstest]
    fn consolidation_conflict_names_the_last_winning_schema() {
        let fallback = consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE");
        let precision_2 = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let precision_4 = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let fallback_path = ObjectPath::from("fallback.parquet");
        let precision_2_path = ObjectPath::from("precision-2.parquet");
        let precision_4_path = ObjectPath::from("precision-4.parquet");
        let reconciled = reconcile_consolidation_schema(
            &fallback,
            &fallback_path,
            &precision_2,
            &precision_2_path,
        )
        .unwrap();

        let error = reconcile_consolidation_schema(
            &reconciled.schema,
            &precision_2_path,
            &precision_4,
            &precision_4_path,
        )
        .unwrap_err();

        assert!(error.to_string().contains("precision-2.parquet"));
        assert!(error.to_string().contains("precision-4.parquet"));
    }

    #[rstest]
    fn consolidation_field_metadata_does_not_replace_precision_source() {
        let precision_2 = consolidation_depth_schema("2", "3", "ETHUSDT.BINANCE");
        let fallback = Arc::new(Schema::new_with_metadata(
            vec![json_string_field("bids", false)],
            consolidation_depth_schema("0", "0", "ETHUSDT.BINANCE")
                .metadata()
                .clone(),
        ));
        let precision_4 = consolidation_depth_schema("4", "5", "ETHUSDT.BINANCE");
        let precision_2_path = ObjectPath::from("precision-2.parquet");
        let fallback_path = ObjectPath::from("fallback-annotated.parquet");
        let precision_4_path = ObjectPath::from("precision-4.parquet");
        let reconciled = reconcile_consolidation_schema(
            &precision_2,
            &precision_2_path,
            &fallback,
            &fallback_path,
        )
        .unwrap();

        assert_eq!(reconciled.schema_source, ConsolidationSchemaSource::Current,);
        assert_eq!(reconciled.candidate_field_metadata.len(), 2);
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("bids".to_string(), "ARROW:extension:name".to_string())),
        );
        assert!(
            reconciled
                .candidate_field_metadata
                .contains(&("bids".to_string(), "ARROW:extension:metadata".to_string())),
        );
        let field_metadata_sources = reconciled
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, fallback_path.clone()))
            .collect();
        let error = reconcile_consolidation_schema_with_sources(
            &reconciled.schema,
            &precision_2_path,
            &field_metadata_sources,
            &precision_4,
            &precision_4_path,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("precision-2.parquet"));
        assert!(message.contains("precision-4.parquet"));
        assert!(!message.contains("fallback-annotated.parquet"));
    }

    #[rstest]
    fn consolidation_field_metadata_tracks_each_origin() {
        let bare = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true),
            Field::new("balances", DataType::Utf8, true),
        ]));
        let info = Arc::new(Schema::new(vec![
            json_string_field("info", true),
            Field::new("balances", DataType::Utf8, true),
        ]));
        let balances = Arc::new(Schema::new(vec![
            json_string_field("info", true),
            json_string_field("balances", true),
        ]));
        let conflicting = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Utf8, true).with_metadata(HashMap::from([(
                "ARROW:extension:name".to_string(),
                "other.extension".to_string(),
            )])),
            json_string_field("balances", true),
        ]));
        let bare_path = ObjectPath::from("bare.parquet");
        let info_path = ObjectPath::from("info.parquet");
        let balances_path = ObjectPath::from("balances.parquet");
        let conflicting_path = ObjectPath::from("conflicting.parquet");
        let with_info =
            reconcile_consolidation_schema(&bare, &bare_path, &info, &info_path).unwrap();
        let mut field_metadata_sources = with_info
            .candidate_field_metadata
            .iter()
            .cloned()
            .map(|key| (key, info_path.clone()))
            .collect::<HashMap<_, _>>();
        let with_balances = reconcile_consolidation_schema_with_sources(
            &with_info.schema,
            &bare_path,
            &field_metadata_sources,
            &balances,
            &balances_path,
        )
        .unwrap();

        for key in with_balances.candidate_field_metadata {
            field_metadata_sources.insert(key, balances_path.clone());
        }

        let error = reconcile_consolidation_schema_with_sources(
            &with_balances.schema,
            &bare_path,
            &field_metadata_sources,
            &conflicting,
            &conflicting_path,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("info.parquet"));
        assert!(message.contains("conflicting.parquet"));
        assert!(!message.contains("balances.parquet"));
    }

    #[rstest]
    fn normalize_dictionary_string_columns_casts_string_dictionaries_to_utf8() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("AUD/USD.SIM").unwrap();
        builder.append("EUR/USD.SIM").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;
        let schema = Arc::new(Schema::new(vec![
            Field::new("instrument_id", dictionary.data_type().clone(), false),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                dictionary,
                Arc::new(UInt64Array::from(vec![1_u64, 2])) as ArrayRef,
            ],
        )
        .unwrap();

        let normalized = normalize_dictionary_string_columns(&batch).unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name("instrument_id")
                .unwrap()
                .data_type(),
            &DataType::Utf8,
        );
        let values = normalized
            .column_by_name("instrument_id")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .iter()
            .collect::<Vec<_>>();
        assert_eq!(values, vec![Some("AUD/USD.SIM"), Some("EUR/USD.SIM")]);
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_preserves_unrecognized_dictionary() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("alpha").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;
        let schema = Arc::new(Schema::new(vec![Field::new(
            "label",
            dictionary.data_type().clone(),
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![dictionary]).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_open_custom_columns_preserves_dictionary_with_type_metadata() {
        let mut builder = StringDictionaryBuilder::<Int8Type>::new();
        builder.append("alpha").unwrap();
        let dictionary = Arc::new(builder.finish()) as ArrayRef;
        let decimal = Arc::new(
            Decimal128Array::from(vec![Some(123_i128)])
                .with_precision_and_scale(38, 16)
                .unwrap(),
        ) as ArrayRef;
        let schema = Arc::new(Schema::new_with_metadata(
            vec![
                Field::new("label", dictionary.data_type().clone(), false),
                Field::new("price", decimal.data_type().clone(), false),
                Field::new("ts_recv", DataType::UInt64, false),
            ],
            HashMap::from([("type_name".to_string(), "CustomData".to_string())]),
        ));
        let batch = RecordBatch::try_new(
            schema,
            vec![dictionary, decimal, Arc::new(UInt64Array::from(vec![7]))],
        )
        .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_legacy_parquet_schema_preserves_unrecognized_fixed_binary() {
        let schema = Schema::new(vec![Field::new(
            "price",
            DataType::FixedSizeBinary(8),
            false,
        )]);

        let normalized = normalize_legacy_parquet_schema(&schema);

        assert_eq!(normalized, schema);
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_converts_binary_info_null_to_arrow_null() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("info", DataType::Binary, true),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(BinaryArray::from_vec(vec![b"null".as_slice()])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1_u64])) as ArrayRef,
            ],
        )
        .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();
        let info = normalized
            .column_by_name("info")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name("info")
                .unwrap()
                .data_type(),
            &DataType::Utf8,
        );
        assert!(info.is_null(0));
    }

    #[rstest]
    fn normalize_legacy_parquet_columns_preserves_quote_price_columns() {
        let decimal = DataType::Decimal128(38, 16);
        let schema = Arc::new(Schema::new(vec![
            Field::new("bid_price", decimal.clone(), false),
            Field::new("ask_price", decimal.clone(), false),
            Field::new("bid_size", decimal.clone(), false),
            Field::new("ask_size", decimal, false),
        ]));
        let values = || {
            Arc::new(
                Decimal128Array::from(vec![1_i128])
                    .with_precision_and_scale(38, 16)
                    .unwrap(),
            ) as ArrayRef
        };
        let batch =
            RecordBatch::try_new(schema.clone(), vec![values(), values(), values(), values()])
                .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(normalized.schema(), schema);
        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn normalize_legacy_depth_flat_columns_builds_structured_sides() {
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for level in 0..DEPTH10_LEN {
                for (name, value) in [("price", 11_i128), ("size", 22_i128)] {
                    fields.push(Field::new(
                        format!("{side}_{name}_{level}"),
                        DataType::Decimal128(38, 16),
                        true,
                    ));
                    let value = (level == 0).then_some(value);
                    columns.push(Arc::new(
                        Decimal128Array::from(vec![value])
                            .with_precision_and_scale(38, 16)
                            .unwrap(),
                    ) as ArrayRef);
                }
                fields.push(Field::new(
                    format!("{side}_count_{level}"),
                    DataType::UInt32,
                    false,
                ));
                columns.push(Arc::new(UInt32Array::from(vec![33])) as ArrayRef);
                fields.push(Field::new(
                    format!("{side}_order_id_{level}"),
                    DataType::UInt64,
                    false,
                ));
                columns.push(Arc::new(UInt64Array::from(vec![44])) as ArrayRef);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(&normalized, 1, 11, 22, 33, 44);
    }

    #[rstest]
    fn normalize_legacy_depth_fixed_lists_builds_structured_sides() {
        let decimal_values = |value| {
            Arc::new(
                Decimal128Array::from(
                    (0..DEPTH10_LEN)
                        .map(|level| (level == 0).then_some(value))
                        .collect::<Vec<_>>(),
                )
                .with_precision_and_scale(38, 16)
                .unwrap(),
            ) as ArrayRef
        };
        let counts = Arc::new(UInt32Array::from(vec![33; DEPTH10_LEN])) as ArrayRef;
        let order_ids = Arc::new(UInt64Array::from(vec![44; DEPTH10_LEN])) as ArrayRef;
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for (name, column) in [
                ("price", depth_list_array(decimal_values(11), true)),
                ("size", depth_list_array(decimal_values(22), true)),
                ("count", depth_list_array(counts.clone(), false)),
                ("order_id", depth_list_array(order_ids.clone(), false)),
            ] {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(&normalized, 1, 11, 22, 33, 44);
    }

    #[rstest]
    fn normalize_legacy_depth_missing_counts_and_order_ids_uses_list_width() {
        const WIDTH: i32 = 3;
        let decimal_values = |value| {
            Arc::new(
                Decimal128Array::from(vec![value; WIDTH as usize])
                    .with_precision_and_scale(38, 16)
                    .unwrap(),
            ) as ArrayRef
        };
        let list = |values: ArrayRef| {
            Arc::new(FixedSizeListArray::new(
                Arc::new(Field::new("item", values.data_type().clone(), false)),
                WIDTH,
                values,
                None,
            )) as ArrayRef
        };
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for (name, column) in [
                ("price", list(decimal_values(11))),
                ("size", list(decimal_values(22))),
            ] {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(&normalized, WIDTH as usize, 11, 22, 0, 0);
    }

    #[rstest]
    #[case::with_order_ids(true, 44)]
    #[case::without_order_ids(false, 0)]
    fn normalize_legacy_depth_fixed_binary_lists_matches_schema(
        #[case] include_order_ids: bool,
        #[case] expected_order_id: u64,
    ) {
        let fixed_values = |value: [u8; 8]| {
            let values = (0..DEPTH10_LEN)
                .map(|level| (level == 0).then_some(value))
                .collect::<Vec<_>>();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    values
                        .iter()
                        .map(Option::as_ref)
                        .map(|value| value.map(<[u8; 8]>::as_slice)),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };
        let counts = Arc::new(UInt32Array::from(vec![33; DEPTH10_LEN])) as ArrayRef;
        let order_ids = Arc::new(UInt64Array::from(vec![44; DEPTH10_LEN])) as ArrayRef;
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            let mut side_columns = vec![
                (
                    "price",
                    depth_list_array(fixed_values(11_i64.to_le_bytes()), true),
                ),
                (
                    "size",
                    depth_list_array(fixed_values(22_u64.to_le_bytes()), true),
                ),
                ("count", depth_list_array(counts.clone(), false)),
            ];

            if include_order_ids {
                side_columns.push(("order_id", depth_list_array(order_ids.clone(), false)));
            }

            for (name, column) in side_columns {
                fields.push(Field::new(
                    format!("{side}_{name}"),
                    column.data_type().clone(),
                    false,
                ));
                columns.push(column);
            }
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();

        assert!(is_nautilus_legacy_schema(batch.schema_ref()));
        let normalized_schema = normalize_legacy_parquet_schema(batch.schema_ref());
        let normalized_batch = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized_schema,
            normalized_batch.schema().as_ref().clone()
        );
        assert_normalized_depth(
            &normalized_batch,
            1,
            110_000_000,
            220_000_000,
            33,
            expected_order_id,
        );
    }

    #[rstest]
    fn normalize_legacy_depth_flat_fixed_columns_preserves_order_ids() {
        let fixed_price = || {
            let bytes = 11_i64.to_le_bytes();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(bytes.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };
        let fixed_size = || {
            let bytes = 22_u64.to_le_bytes();
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(bytes.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            ) as ArrayRef
        };
        let mut fields = Vec::new();
        let mut columns = Vec::new();

        for side in ["bid", "ask"] {
            for level in 0..DEPTH10_LEN {
                fields.push(Field::new(
                    format!("{side}_price_{level}"),
                    DataType::FixedSizeBinary(8),
                    false,
                ));
                columns.push(fixed_price());
                fields.push(Field::new(
                    format!("{side}_size_{level}"),
                    DataType::FixedSizeBinary(8),
                    false,
                ));
                columns.push(fixed_size());
                fields.push(Field::new(
                    format!("{side}_count_{level}"),
                    DataType::UInt32,
                    false,
                ));
                columns.push(Arc::new(UInt32Array::from(vec![33])) as ArrayRef);
                fields.push(Field::new(
                    format!("{side}_order_id_{level}"),
                    DataType::UInt64,
                    false,
                ));
                columns.push(Arc::new(UInt64Array::from(vec![44])) as ArrayRef);
            }
        }

        for (field, column) in [
            (
                Field::new("flags", DataType::UInt8, false),
                Arc::new(UInt8Array::from(vec![0])) as ArrayRef,
            ),
            (
                Field::new("sequence", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![1])) as ArrayRef,
            ),
            (
                Field::new("ts_event", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![2])) as ArrayRef,
            ),
            (
                Field::new("ts_init", DataType::UInt64, false),
                Arc::new(UInt64Array::from(vec![3])) as ArrayRef,
            ),
        ] {
            fields.push(field);
            columns.push(column);
        }
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();
        assert!(is_nautilus_legacy_schema(batch.schema_ref()));

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        for side in ["bids", "asks"] {
            let list = normalized
                .column_by_name(side)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let levels = list.value(0);
            let levels = levels.as_any().downcast_ref::<StructArray>().unwrap();
            let order_ids = levels
                .column_by_name("order_id")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            assert_eq!(order_ids.values(), &[44; DEPTH10_LEN]);
        }
    }

    #[rstest]
    fn normalize_legacy_depth_fixture_matches_open_shape() {
        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus")
            .join(precision_dir)
            .join("depths.parquet");
        let file = std::fs::File::open(path).unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
        let metadata = builder
            .metadata()
            .file_metadata()
            .key_value_metadata()
            .unwrap();

        for (key, value) in [
            ("instrument_id", "AAPL.XNAS"),
            ("price_precision", "4"),
            ("size_precision", "1"),
        ] {
            assert_eq!(
                metadata
                    .iter()
                    .find(|entry| entry.key == key)
                    .and_then(|entry| entry.value.as_deref()),
                Some(value),
            );
        }
        let batch = builder.build().unwrap().next().unwrap().unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_normalized_depth(
            &normalized,
            DEPTH10_LEN,
            12_345_000_000_000_000,
            25_000_000_000_000_000,
            3,
            0,
        );
        assert_eq!(normalized.num_columns(), 6);
        assert_eq!(
            normalized
                .column_by_name("flags")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt8Array>()
                .unwrap()
                .value(0),
            32
        );
        assert_eq!(
            normalized
                .column_by_name("sequence")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .value(0),
            7
        );

        for name in ["ts_event", "ts_init"] {
            assert_eq!(
                normalized
                    .schema()
                    .field_with_name(name)
                    .unwrap()
                    .data_type(),
                &DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, Some("UTC".into())),
            );
        }
    }

    #[rstest]
    #[case("quotes.parquet", "bid_price", None)]
    #[case("trades.parquet", "price", Some("aggressor_side"))]
    #[case("bars.parquet", "open", None)]
    #[case("deltas.parquet", "price", Some("action"))]
    fn legacy_market_fixture_matches_open_types(
        #[case] file_name: &str,
        #[case] fixed_field: &str,
        #[case] enum_field: Option<&str>,
    ) {
        let precision_dir = if cfg!(feature = "high-precision") {
            "128-bit"
        } else {
            "64-bit"
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data/nautilus")
            .join(precision_dir)
            .join(file_name);
        let file = std::fs::File::open(path).unwrap();
        let batch = ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap()
            .next()
            .unwrap()
            .unwrap();

        let normalized = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized
                .schema()
                .field_with_name(fixed_field)
                .unwrap()
                .data_type(),
            &DataType::Decimal128(38, 16),
        );
        assert_eq!(
            normalized
                .schema()
                .field_with_name("ts_init")
                .unwrap()
                .data_type(),
            &DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, Some("UTC".into())),
        );

        if let Some(enum_field) = enum_field {
            assert!(matches!(
                normalized
                    .schema()
                    .field_with_name(enum_field)
                    .unwrap()
                    .data_type(),
                DataType::Dictionary(_, value) if value.as_ref() == &DataType::Utf8
            ));
        }
    }

    #[rstest]
    #[case("depths.parquet")]
    #[case("quotes.parquet")]
    #[case("trades.parquet")]
    #[case("bars.parquet")]
    #[case("deltas.parquet")]
    #[case("dictionary-trade")]
    fn legacy_fixture_schema_normalization_matches_batch(#[case] file_name: &str) {
        let (schema, batch) = if file_name == "dictionary-trade" {
            let dictionary = |value: &str| {
                let mut builder = StringDictionaryBuilder::<Int8Type>::new();
                builder.append(value).unwrap();
                Arc::new(builder.finish()) as ArrayRef
            };
            let price = 11_i64.to_le_bytes();
            let size = 22_u64.to_le_bytes();
            let trade_ids = dictionary("trade-1");
            let identifiers = dictionary("AAPL.XNAS");
            let schema = Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("price", DataType::FixedSizeBinary(8), false),
                    Field::new("size", DataType::FixedSizeBinary(8), false),
                    Field::new("aggressor_side", DataType::UInt8, false),
                    Field::new("trade_id", trade_ids.data_type().clone(), false),
                    Field::new("ts_event", DataType::UInt64, false),
                    Field::new("ts_init", DataType::UInt64, false),
                    Field::new(KEY_IDENTIFIER, identifiers.data_type().clone(), false),
                ],
                HashMap::from([("type".to_string(), "TradeTick".to_string())]),
            ));
            let batch = RecordBatch::try_new(
                Arc::clone(&schema),
                vec![
                    Arc::new(
                        FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                            [Some(price.as_slice())].into_iter(),
                            8,
                        )
                        .unwrap(),
                    ),
                    Arc::new(
                        FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                            [Some(size.as_slice())].into_iter(),
                            8,
                        )
                        .unwrap(),
                    ),
                    Arc::new(UInt8Array::from(vec![1])),
                    trade_ids,
                    Arc::new(UInt64Array::from(vec![1])),
                    Arc::new(UInt64Array::from(vec![2])),
                    identifiers,
                ],
            )
            .unwrap();
            (schema, batch)
        } else {
            let precision_dir = if cfg!(feature = "high-precision") {
                "128-bit"
            } else {
                "64-bit"
            };
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test_data/nautilus")
                .join(precision_dir)
                .join(file_name);
            let file = std::fs::File::open(path).unwrap();
            let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
            let schema = builder.schema().clone();
            let batch = builder.build().unwrap().next().unwrap().unwrap();
            let batch =
                RecordBatch::try_new(Arc::clone(&schema), batch.columns().to_vec()).unwrap();
            (schema, batch)
        };
        let normalized_schema = normalize_legacy_parquet_schema(schema.as_ref());
        let normalized_batch = normalize_legacy_parquet_columns(&batch).unwrap();

        assert_eq!(
            normalized_schema,
            normalized_batch.schema().as_ref().clone()
        );

        if file_name == "dictionary-trade" {
            assert_eq!(
                normalized_batch
                    .schema()
                    .field_with_name("trade_id")
                    .unwrap()
                    .data_type(),
                &DataType::Utf8,
            );
        }
    }

    fn assert_normalized_depth(
        batch: &RecordBatch,
        level_count: usize,
        price: i128,
        size: i128,
        count: u32,
        order_id: u64,
    ) {
        let schema = batch.schema();
        assert_eq!(schema.field(0).name(), "bids");
        assert_eq!(schema.field(1).name(), "asks");

        for side in ["bids", "asks"] {
            let list = batch
                .column_by_name(side)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let levels = list.value(0);
            let levels = levels.as_any().downcast_ref::<StructArray>().unwrap();
            let prices = levels
                .column_by_name("price")
                .unwrap()
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let sizes = levels
                .column_by_name("size")
                .unwrap()
                .as_any()
                .downcast_ref::<Decimal128Array>()
                .unwrap();
            let counts = levels
                .column_by_name("count")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt32Array>()
                .unwrap();
            let order_ids = levels
                .column_by_name("order_id")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();

            assert_eq!(levels.len(), level_count);
            assert_eq!(prices.value(0), price);
            assert_eq!(sizes.value(0), size);
            assert_eq!(counts.value(0), count);
            assert_eq!(order_ids.value(0), order_id);
        }
    }

    #[tokio::test]
    async fn default_writer_sets_zstd_sorting_and_identifier_bloom_filter() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.parquet");
        let object_store = Arc::new(
            object_store::local::LocalFileSystem::new_with_prefix(directory.path()).unwrap(),
        );
        let object_path = ObjectPath::from("layout.parquet");
        let schema = Arc::new(Schema::new(vec![
            Field::new("identifier", DataType::Utf8, false),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["AUD/USD.SIM", "AUD/USD.SIM"])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1_u64, 2])) as ArrayRef,
            ],
        )
        .unwrap();

        write_batches_to_object_store(&[batch], object_store, &object_path, None, None, None)
            .await
            .unwrap();

        let read_options = ReadOptionsBuilder::new()
            .with_reader_properties(
                ReaderProperties::builder()
                    .set_read_bloom_filter(true)
                    .build(),
            )
            .build();
        let reader = SerializedFileReader::new_with_options(
            std::fs::File::open(path).unwrap(),
            read_options,
        )
        .unwrap();
        let row_group = reader.metadata().row_group(0);
        let sorting = row_group.sorting_columns().unwrap();

        assert_eq!(DEFAULT_ROW_GROUP_SIZE, 131_072);
        assert_eq!(
            sorting,
            &vec![
                SortingColumn {
                    column_idx: 1,
                    descending: false,
                    nulls_first: false,
                },
                SortingColumn {
                    column_idx: 0,
                    descending: false,
                    nulls_first: false,
                },
            ],
        );
        assert!(
            row_group
                .columns()
                .iter()
                .all(|column| column.compression() == Compression::ZSTD(ZstdLevel::default())),
        );
        assert!(
            reader
                .get_row_group(0)
                .unwrap()
                .get_column_bloom_filter(0)
                .is_some(),
        );
    }
}
