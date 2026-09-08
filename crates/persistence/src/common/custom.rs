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

//! Custom data persistence: shared conversion and orchestration.
//!
//! Centralizes the logic for appending the `data_type` column and metadata to Arrow batches
//! (Parquet/Feather), and custom-data write preparation, path construction, and decode logic
//! so the catalog delegates here instead of inlining custom-specific branching.

#![expect(
    clippy::missing_panics_doc,
    reason = "custom Arrow conversion validates non-empty inputs before internal unwraps"
)]

use std::{
    collections::{BTreeMap, HashMap},
    hash::BuildHasher,
    sync::Arc,
};

use datafusion::arrow::{
    array::{Array, StringArray},
    datatypes::{DataType as ArrowDataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, CustomData, CustomDataTrait, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
    MarkPriceUpdate, NautilusDataType, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick,
    TradeTick, close::InstrumentClose, encode_custom_to_arrow,
};
use nautilus_serialization::arrow::{
    DecodeDataFromRecordBatch, custom::CustomDataDecoder, record_batch_with_identifier_column,
};

use crate::{
    catalog::types::data_type_from_data_path_prefix, common::paths::urisafe_instrument_id,
};

/// Builds a schema that adds the `data_type` column and `type_name` metadata to a base schema.
/// Used when creating a Feather buffer for custom data (single type per writer).
#[must_use]
pub fn schema_with_data_type_column(base_schema: &Schema, type_name: &str) -> Schema {
    let mut fields: Vec<_> = base_schema.fields().iter().cloned().collect();
    fields.push(Arc::new(Field::new("data_type", ArrowDataType::Utf8, true)));
    let mut meta = base_schema.metadata().clone();
    meta.insert("type_name".to_string(), type_name.to_string());
    Schema::new_with_metadata(fields, meta)
}

/// Appends a `data_type` column (JSON string per row) and `type_name` + optional metadata to the
/// batch schema. Used by both the Parquet catalog and Feather writer for catalog-compatible output.
///
/// # Errors
///
/// Returns an error if the new `RecordBatch` cannot be created.
pub fn augment_batch_with_data_type_column<S: BuildHasher>(
    batch: &RecordBatch,
    data_type_json: &str,
    type_name: &str,
    dt_meta: Option<&HashMap<String, String, S>>,
) -> anyhow::Result<RecordBatch> {
    let num_rows = batch.num_rows();
    let data_type_array: Arc<dyn Array> = Arc::new(StringArray::from(
        (0..num_rows)
            .map(|_| Some(data_type_json))
            .collect::<Vec<_>>(),
    ));
    let schema = batch.schema();
    let mut fields: Vec<_> = schema.fields().iter().cloned().collect();
    fields.push(Arc::new(Field::new(
        "data_type",
        ArrowDataType::Utf8,
        false,
    )));
    let mut meta = schema.metadata().clone();
    meta.insert("type_name".to_string(), type_name.to_string());

    if let Some(m) = dt_meta {
        meta.extend(m.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    let new_schema = Arc::new(Schema::new_with_metadata(fields, meta));
    let mut columns = batch.columns().to_vec();
    columns.push(data_type_array);
    let new_batch = RecordBatch::try_new(new_schema, columns)
        .map_err(|e| anyhow::anyhow!("Failed to merge custom data type metadata: {e}"))?;
    Ok(new_batch)
}

/// Returns path components for custom data: `["data", "custom", type_name, identifier]`.
/// Used by the catalog to build full object-store paths via `make_object_store_path`.
#[must_use]
pub fn custom_data_path_components(type_name: &str, identifier: Option<&str>) -> Vec<String> {
    let mut components = vec![
        "data".to_string(),
        "custom".to_string(),
        type_name.to_string(),
    ];

    if let Some(id) = identifier {
        let safe = urisafe_instrument_id(id);
        if !safe.is_empty() {
            components.push(safe);
        }
    }
    components
}

/// Groups custom data by full persistence identity.
///
/// The identity includes type name, catalog identifier, and metadata so each Arrow batch has a
/// consistent schema and `data_type` column.
#[must_use]
pub fn group_custom_data_by_type<'a>(
    data: impl IntoIterator<Item = &'a CustomData>,
) -> Vec<Vec<&'a CustomData>> {
    let mut grouped: BTreeMap<(String, Option<String>, String), Vec<&'a CustomData>> =
        BTreeMap::new();

    for custom in data {
        let key = (
            custom.data_type.type_name().to_string(),
            custom.data_type.identifier().map(String::from),
            custom.data_type.metadata_str(),
        );
        grouped.entry(key).or_default().push(custom);
    }

    grouped.into_values().collect()
}

/// Prepares a batch of custom data for writing: encodes to Arrow, augments with `data_type` column,
/// and returns type identity and timestamp range so the catalog can build path and perform I/O.
///
/// # Errors
///
/// Returns an error if encoding or augmentation fails, or if the type is not registered.
pub fn prepare_custom_data_batch(
    data: &[&CustomData],
) -> anyhow::Result<(RecordBatch, String, Option<String>, UnixNanos, UnixNanos)> {
    if data.is_empty() {
        anyhow::bail!("prepare_custom_data_batch called with empty data");
    }

    let first_custom = data.first().unwrap();
    let type_name = first_custom.data.type_name();
    let identifier = first_custom.data_type.identifier().map(String::from);
    let metadata_str = first_custom.data_type.metadata_str();
    let dt_meta = first_custom.data_type.metadata_string_map();
    let data_type_json = first_custom
        .data_type
        .to_persistence_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize data_type for persistence: {e}"))?;

    for custom in data {
        anyhow::ensure!(
            custom.data.type_name() == type_name
                && custom.data_type.identifier() == identifier.as_deref()
                && custom.data_type.metadata_str() == metadata_str,
            "Cannot prepare one custom data batch from mixed DataType values",
        );
    }

    let items: Vec<Arc<dyn CustomDataTrait>> = data.iter().map(|c| Arc::clone(&c.data)).collect();
    let mut start_ts = items[0].ts_init();
    let mut end_ts = start_ts;

    for item in &items[1..] {
        let ts_init = item.ts_init();
        start_ts = start_ts.min(ts_init);
        end_ts = end_ts.max(ts_init);
    }

    let batch = encode_custom_to_arrow(type_name, &items)
        .map_err(|e| anyhow::anyhow!("Failed to encode custom data to Arrow: {e}"))?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Custom data type \"{type_name}\" is not registered for Arrow encoding; \
                 call register_custom_data_class or ensure_custom_data_registered before writing"
            )
        })?;
    let batch =
        augment_batch_with_data_type_column(&batch, &data_type_json, type_name, dt_meta.as_ref())?;
    let batch = record_batch_with_identifier_column(batch, identifier.as_deref())?;

    Ok((batch, type_name.to_string(), identifier, start_ts, end_ts))
}

/// Decodes a `RecordBatch` to Data objects based on metadata.
///
/// Supports both standard data types and custom data types when `allow_custom_fallback`
/// is true (e.g. when decoding files under `custom/`). When false, unknown type names
/// produce an error instead of attempting custom decode.
///
/// # Errors
///
/// Returns an error if decoding fails or the type is unknown (and custom fallback not allowed).
#[expect(
    clippy::implicit_hasher,
    reason = "DecodeDataFromRecordBatch requires the standard HashMap metadata type"
)]
pub fn decode_batch_to_data(
    metadata: &HashMap<String, String>,
    batch: RecordBatch,
    allow_custom_fallback: bool,
) -> anyhow::Result<Vec<Data>> {
    let type_name = metadata
        .get("type_name")
        .cloned()
        .or_else(|| metadata.get("bar_type").map(|_| "bars".to_string()))
        .ok_or_else(|| anyhow::anyhow!("Missing type_name in metadata"))?;

    let data_type = match data_type_from_data_path_prefix(&type_name) {
        Ok(data_type) => data_type,
        Err(_) if allow_custom_fallback => {
            return Ok(CustomDataDecoder::decode_data_batch(metadata, batch)?);
        }
        Err(e) => return Err(e),
    };

    macro_rules! decode_builtin_data_batch {
        (
            ($data_type:ident, $metadata:ident, $batch:ident, $type_name:ident, $allow_custom:ident);
            (Instrument, InstrumentAny, Instrument, Instrument, $instrument_prefix:literal),
            $(($variant:ident, $type:ident, $data:ident, $batch_variant:ident, $prefix:literal)),+ $(,)?
        ) => {
            match $data_type {
                $(
                    NautilusDataType::$variant => {
                        Ok($type::decode_data_batch($metadata, $batch)?)
                    }
                )+
                NautilusDataType::Custom { .. } if $allow_custom => {
                    Ok(CustomDataDecoder::decode_data_batch($metadata, $batch)?)
                }
                NautilusDataType::Custom { .. } => anyhow::bail!(
                    "Unknown data type: {}; custom decode only allowed in custom data context",
                    $type_name,
                ),
                NautilusDataType::Instrument => {
                    anyhow::bail!("Instrument batches require instrument-specific decoding")
                }
                NautilusDataType::OrderBook => {
                    anyhow::bail!("Order book snapshots do not have a catalog batch representation")
                }
                #[cfg(feature = "defi")]
                NautilusDataType::Defi => {
                    anyhow::bail!("DeFi batches require DeFi-specific decoding")
                }
            }
        };
    }

    nautilus_model::for_each_data_type!(
        decode_builtin_data_batch,
        data_type,
        metadata,
        batch,
        type_name,
        allow_custom_fallback
    )
}

/// Decodes multiple `RecordBatches` (e.g. from custom data files) into a single `Vec<Data>`.
/// Optionally replaces `ts_init` column with `ts_event` before decoding each batch.
///
/// # Errors
///
/// Returns an error if any batch fails to decode.
pub fn decode_custom_batches_to_data(
    batches: Vec<RecordBatch>,
    use_ts_event_for_ts_init: bool,
) -> anyhow::Result<Vec<Data>> {
    if batches.is_empty() {
        return Ok(Vec::new());
    }

    let mut file_data = Vec::new();
    let schema = batches
        .first()
        .map(arrow::array::RecordBatch::schema)
        .expect("empty batches returned above");

    for mut batch in batches {
        if use_ts_event_for_ts_init {
            let column_names: Vec<String> =
                schema.fields().iter().map(|f| f.name().clone()).collect();

            if let (Some(ts_event_idx), Some(ts_init_idx)) = (
                column_names.iter().position(|n| n == "ts_event"),
                column_names.iter().position(|n| n == "ts_init"),
            ) {
                let mut new_columns = batch.columns().to_vec();
                new_columns[ts_init_idx] = new_columns[ts_event_idx].clone();
                batch = RecordBatch::try_new(schema.clone(), new_columns)
                    .map_err(|e| anyhow::anyhow!("Failed to create new batch: {e}"))?;
            }
        }
        let metadata = batch.schema().metadata().clone();
        let decoded = decode_batch_to_data(&metadata, batch, true)?;
        file_data.extend(decoded);
    }
    Ok(file_data)
}
