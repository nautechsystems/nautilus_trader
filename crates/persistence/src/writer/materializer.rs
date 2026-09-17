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

//! Arrow-only transforms for Feather stream files promoted into durable catalog files.

use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
    sync::Arc,
};

use arrow::{
    array::{Array, ArrayRef, StringArray, UInt64Array},
    compute::{SortColumn, SortOptions, concat_batches, lexsort_to_indices, take_record_batch},
    datatypes::{DataType, Field, Schema},
    ipc::reader::StreamReader,
    record_batch::RecordBatch,
};
use nautilus_model::{data::BarType, enums::AggregationSource};
use nautilus_serialization::arrow::U64ColumnRef;
use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjectPath};

use crate::writer::feather::{
    NAUTILUS_ARROW_METADATA_ID_COLUMN, NAUTILUS_ARROW_METADATA_JSON_COLUMN,
    canonical_metadata_json, staged_metadata_id,
};

/// Decoded Feather contents and the storage identity observed by the same read.
pub(crate) struct FeatherReadResult {
    pub batches: Vec<RecordBatch>,
    pub content_hash: String,
}

/// Options applied while converting streaming Feather data to durable catalog data.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StreamConversionOptions {
    /// Replaces the `ts_init` column with the `ts_event` column before writing.
    pub use_ts_event_for_ts_init: bool,
    /// Converts schema-level `bar_type` metadata from `INTERNAL` to `EXTERNAL`.
    pub convert_bar_type_to_external: bool,
}

/// Reads a Feather IPC stream file from object storage without decoding into Nautilus data values.
///
/// # Errors
///
/// Returns an error if storage access fails or the IPC stream cannot be decoded.
pub(crate) async fn read_feather_record_batches(
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
) -> anyhow::Result<Vec<RecordBatch>> {
    read_feather_record_batches_with_hash(object_store, path)
        .await
        .map(|(batches, _)| batches)
}

/// Reads a Feather IPC stream and returns its content hash.
///
/// # Errors
///
/// Returns an error if storage access fails or the IPC stream cannot be decoded.
pub(crate) async fn read_feather_record_batches_with_hash(
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
) -> anyhow::Result<(Vec<RecordBatch>, String)> {
    let result = read_feather_record_batches_with_identity(object_store, path).await?;
    Ok((result.batches, result.content_hash))
}

/// Reads a Feather IPC stream with the version metadata returned by the same object read.
///
/// # Errors
///
/// Returns an error if storage access fails or the IPC stream cannot be decoded.
pub(crate) async fn read_feather_record_batches_with_identity(
    object_store: Arc<dyn ObjectStore>,
    path: &ObjectPath,
) -> anyhow::Result<FeatherReadResult> {
    let result = object_store.get(path).await?;
    let bytes = result.bytes().await?;
    let content_hash = blake3::hash(&bytes).to_hex().to_string();

    if bytes.is_empty() {
        return Ok(FeatherReadResult {
            batches: Vec::new(),
            content_hash,
        });
    }

    let reader = StreamReader::try_new(Cursor::new(bytes.as_ref()), None)?;
    let batches = reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to read Feather IPC batch: {e}"))?;
    Ok(FeatherReadResult {
        batches,
        content_hash,
    })
}

/// Restores the original Arrow schema metadata embedded in a staged Feather batch.
///
/// A single staging batch can contain rows from several input schemas. The metadata columns
/// identify contiguous schema runs, so restoring them can produce more than one record batch.
///
/// # Errors
///
/// Returns an error if the staging metadata columns are incomplete, malformed, or inconsistent.
pub(crate) fn restore_staged_record_batches(
    batch: RecordBatch,
) -> anyhow::Result<Vec<RecordBatch>> {
    let schema = batch.schema();
    let Ok(id_index) = schema.index_of(NAUTILUS_ARROW_METADATA_ID_COLUMN) else {
        anyhow::ensure!(
            schema
                .index_of(NAUTILUS_ARROW_METADATA_JSON_COLUMN)
                .is_err(),
            "Feather batch has staged metadata JSON without a metadata ID"
        );
        return Ok(vec![batch]);
    };
    let json_index = schema
        .index_of(NAUTILUS_ARROW_METADATA_JSON_COLUMN)
        .map_err(|_| anyhow::anyhow!("Feather batch has a metadata ID without metadata JSON"))?;
    let ids = batch
        .column(id_index)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow::anyhow!("Feather metadata ID column is not UTF-8"))?;
    let metadata_json = batch
        .column(json_index)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow::anyhow!("Feather metadata JSON column is not UTF-8"))?;
    let mut restored = Vec::new();
    let mut run_start = 0;

    while run_start < batch.num_rows() {
        anyhow::ensure!(
            !ids.is_null(run_start) && !metadata_json.is_null(run_start),
            "Feather row is missing staged Arrow metadata"
        );
        let id = ids.value(run_start);
        let json = metadata_json.value(run_start);
        let mut run_end = run_start + 1;
        while run_end < batch.num_rows()
            && !ids.is_null(run_end)
            && !metadata_json.is_null(run_end)
            && ids.value(run_end) == id
            && metadata_json.value(run_end) == json
        {
            run_end += 1;
        }

        let (metadata, field_metadata) = staged_arrow_metadata(json)?;
        // Current staged files hash the canonical schema metadata; files staged before
        // that change hash the raw staged JSON string instead.
        anyhow::ensure!(
            id == staged_metadata_id(&canonical_metadata_json(&metadata)?)
                || id == staged_metadata_id(json),
            "Feather staged metadata hash does not match its JSON"
        );
        let slice = batch.slice(run_start, run_end - run_start);
        let fields = slice
            .schema()
            .fields()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != id_index && *index != json_index)
            .map(|(_, field)| {
                field_metadata.get(field.name()).map_or_else(
                    || field.clone(),
                    |metadata| Arc::new(field.as_ref().clone().with_metadata(metadata.clone())),
                )
            })
            .collect::<Vec<_>>();
        let columns = slice
            .columns()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != id_index && *index != json_index)
            .map(|(_, column)| column.clone())
            .collect::<Vec<_>>();
        restored.push(RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(fields, metadata)),
            columns,
        )?);
        run_start = run_end;
    }
    Ok(restored)
}

type StagedFieldMetadata = HashMap<String, HashMap<String, String>>;

fn staged_arrow_metadata(
    metadata_json: &str,
) -> anyhow::Result<(HashMap<String, String>, StagedFieldMetadata)> {
    let value = serde_json::from_str::<serde_json::Value>(metadata_json)?;
    if value.get("format_version").is_none() {
        return Ok((serde_json::from_value(value)?, HashMap::new()));
    }
    anyhow::ensure!(
        value
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            == Some(1),
        "Unsupported staged Arrow metadata format version"
    );
    let schema_metadata = serde_json::from_value(
        value
            .get("schema_metadata")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Staged Arrow metadata has no schema_metadata"))?,
    )?;
    let field_metadata = serde_json::from_value(
        value
            .get("field_metadata")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Staged Arrow metadata has no field_metadata"))?,
    )?;
    Ok((schema_metadata, field_metadata))
}

/// Applies table-level stream conversion transforms without decoding rows into Nautilus data values.
///
/// # Errors
///
/// Returns an error if required timestamp columns are missing or transformed batches are invalid.
pub(crate) fn apply_stream_conversion_transforms(
    batches: &[RecordBatch],
    options: StreamConversionOptions,
) -> anyhow::Result<Vec<RecordBatch>> {
    batches
        .iter()
        .map(|batch| apply_stream_conversion_transform(batch, options))
        .collect()
}

/// Applies stream conversion transforms to one Arrow record batch.
///
/// # Errors
///
/// Returns an error if required timestamp columns are missing or the transformed batch is invalid.
pub(crate) fn apply_stream_conversion_transform(
    batch: &RecordBatch,
    options: StreamConversionOptions,
) -> anyhow::Result<RecordBatch> {
    let schema = batch.schema();
    let mut columns = batch.columns().to_vec();

    if options.use_ts_event_for_ts_init {
        let ts_event_idx = schema
            .index_of("ts_event")
            .map_err(|_| anyhow::anyhow!("ts_event column not found"))?;
        let ts_init_idx = schema
            .index_of("ts_init")
            .map_err(|_| anyhow::anyhow!("ts_init column not found"))?;
        columns[ts_init_idx] = columns[ts_event_idx].clone();
    }

    if options.convert_bar_type_to_external
        && let Ok(identifier_index) = schema.index_of("identifier")
    {
        let identifiers = columns[identifier_index]
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot convert bar type identifiers to EXTERNAL: identifier column is {}, \
                     expected Utf8",
                    columns[identifier_index].data_type(),
                )
            })?;
        let converted = identifiers
            .iter()
            .map(|identifier| identifier.map(external_bar_type))
            .collect::<Vec<_>>();
        columns[identifier_index] = Arc::new(StringArray::from(converted)) as ArrayRef;
    }

    let schema = if options.convert_bar_type_to_external {
        schema_with_external_bar_type(schema.as_ref())
    } else {
        schema.as_ref().clone()
    };

    RecordBatch::try_new(Arc::new(schema), columns)
        .map_err(|e| anyhow::anyhow!("Failed to build transformed stream batch: {e}"))
}

fn external_bar_type(value: &str) -> String {
    let Ok(bar_type) = value.parse::<BarType>() else {
        return value.to_string();
    };

    if bar_type.standard().is_externally_aggregated() {
        return value.to_string();
    }
    let standard = bar_type.standard();
    BarType::new(
        standard.instrument_id(),
        standard.spec(),
        AggregationSource::External,
    )
    .to_string()
}

/// Applies stream conversion transforms, concatenates batches, and sorts by `ts_init`.
///
/// This mirrors the existing stream-to-catalog behavior while keeping the conversion on Arrow
/// batches instead of decoding rows into Nautilus data values.
///
/// # Errors
///
/// Returns an error if required timestamp columns are missing, transformed batches are invalid,
/// or sorting fails.
pub(crate) fn coalesce_stream_conversion_batches(
    batches: &[RecordBatch],
    options: StreamConversionOptions,
) -> anyhow::Result<Option<RecordBatch>> {
    let has_staged_metadata = batches.iter().any(|batch| {
        batch
            .schema()
            .index_of(NAUTILUS_ARROW_METADATA_ID_COLUMN)
            .is_ok()
    });
    let mut restored = Vec::new();
    for batch in batches {
        restored.extend(restore_staged_record_batches(batch.clone())?);
    }
    let mut batches = apply_stream_conversion_transforms(&restored, options)?;
    if has_staged_metadata {
        batches = batches
            .iter()
            .map(stage_restored_metadata)
            .collect::<anyhow::Result<Vec<_>>>()?;
    }

    if batches.is_empty() {
        return Ok(None);
    }

    let schema = batches[0].schema();
    let mut batch = concat_batches(&schema, batches.iter())
        .map_err(|e| anyhow::anyhow!("Failed to concatenate stream batches: {e}"))?;

    if batch.num_rows() == 0 {
        return Ok(None);
    }

    if !is_record_batch_monotonic_by_ts_init(&batch)? {
        let original_row_index =
            Arc::new(UInt64Array::from_iter_values(0..batch.num_rows() as u64));
        let indices = lexsort_to_indices(
            &[
                SortColumn {
                    values: batch.column(batch.schema().index_of("ts_init")?).clone(),
                    options: Some(SortOptions {
                        descending: false,
                        nulls_first: false,
                    }),
                },
                SortColumn {
                    values: original_row_index,
                    options: Some(SortOptions {
                        descending: false,
                        nulls_first: false,
                    }),
                },
            ],
            None,
        )
        .map_err(|e| anyhow::anyhow!("Failed to sort stream conversion batch: {e}"))?;
        batch = take_record_batch(&batch, &indices)
            .map_err(|e| anyhow::anyhow!("Failed to reorder stream conversion batch: {e}"))?;
    }

    Ok(Some(batch))
}

fn stage_restored_metadata(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    let schema_metadata = batch
        .schema()
        .metadata()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let field_metadata = batch
        .schema()
        .fields()
        .iter()
        .filter(|field| !field.metadata().is_empty())
        .map(|field| {
            (
                field.name().clone(),
                field
                    .metadata()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let metadata_json = serde_json::to_string(&serde_json::json!({
        "format_version": 1,
        "schema_metadata": schema_metadata,
        "field_metadata": field_metadata,
    }))?;
    let metadata_id = staged_metadata_id(&canonical_metadata_json(batch.schema().metadata())?);
    let mut fields = batch
        .schema()
        .fields()
        .iter()
        .map(|field| {
            Arc::new(Field::new(
                field.name().clone(),
                field.data_type().clone(),
                field.is_nullable(),
            ))
        })
        .collect::<Vec<_>>();
    fields.push(Arc::new(Field::new(
        NAUTILUS_ARROW_METADATA_ID_COLUMN,
        DataType::Utf8,
        false,
    )));
    fields.push(Arc::new(Field::new(
        NAUTILUS_ARROW_METADATA_JSON_COLUMN,
        DataType::Utf8,
        false,
    )));
    let mut columns = batch.columns().to_vec();
    columns.push(Arc::new(StringArray::from(vec![
        metadata_id;
        batch.num_rows()
    ])));
    columns.push(Arc::new(StringArray::from(vec![
        metadata_json;
        batch.num_rows()
    ])));
    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(fields)),
        columns,
    )?)
}

fn schema_with_external_bar_type(schema: &Schema) -> Schema {
    let mut metadata = schema.metadata().clone();

    if let Some(bar_type) = metadata.get("bar_type").cloned() {
        match bar_type.parse::<BarType>() {
            Ok(bar_type) if !bar_type.standard().is_externally_aggregated() => {
                let standard = bar_type.standard();
                let converted = BarType::new(
                    standard.instrument_id(),
                    standard.spec(),
                    AggregationSource::External,
                );
                metadata.insert("bar_type".to_string(), converted.to_string());
            }
            Ok(_) => {}
            Err(e) => log::warn!("Cannot convert bar_type '{bar_type}' to EXTERNAL: {e}"),
        }
    }

    Schema::new_with_metadata(schema.fields().clone(), metadata)
}

fn is_record_batch_monotonic_by_ts_init(batch: &RecordBatch) -> anyhow::Result<bool> {
    let ts_init = ts_init_array(batch)?;
    let values = ts_values(ts_init)?;

    if values.len() != ts_init.len() {
        anyhow::bail!("ts_init column contains null values");
    }

    Ok(values.windows(2).all(|window| window[1] >= window[0]))
}

fn ts_init_array(batch: &RecordBatch) -> anyhow::Result<&dyn Array> {
    let ts_init_idx = batch
        .schema()
        .index_of("ts_init")
        .map_err(|_| anyhow::anyhow!("ts_init column not found"))?;
    Ok(batch.column(ts_init_idx).as_ref())
}

fn ts_values(array: &dyn Array) -> anyhow::Result<Vec<u64>> {
    let values = U64ColumnRef::try_from_array(array)
        .ok_or_else(|| anyhow::anyhow!("ts_init column must be UInt64 or Int64"))?;

    (0..values.len())
        .filter(|&idx| !values.is_null(idx))
        .map(|idx| {
            values
                .value(idx)
                .ok_or_else(|| anyhow::anyhow!("ts_init value cannot be negative"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use arrow::{
        array::{ArrayRef, Int32Array},
        datatypes::{DataType, Field},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn staged_arrow_metadata_reads_current_and_legacy_formats() {
        let current = r#"{
            "format_version": 1,
            "schema_metadata": {"type_name": "Example"},
            "field_metadata": {"payload": {"ARROW:extension:name": "arrow.json"}}
        }"#;
        let legacy = r#"{"type_name":"Example"}"#;

        let (current_schema, current_fields) = staged_arrow_metadata(current).unwrap();
        let (legacy_schema, legacy_fields) = staged_arrow_metadata(legacy).unwrap();

        assert_eq!(current_schema["type_name"], "Example");
        assert_eq!(
            current_fields["payload"]["ARROW:extension:name"],
            "arrow.json"
        );
        assert_eq!(legacy_schema["type_name"], "Example");
        assert!(legacy_fields.is_empty());
    }

    #[rstest]
    fn coalesce_restores_staged_field_metadata() {
        let metadata_json = r#"{
            "format_version": 1,
            "schema_metadata": {"type_name": "Example"},
            "field_metadata": {"payload": {"ARROW:extension:name": "arrow.json"}}
        }"#;
        let metadata_id = staged_metadata_id(metadata_json);
        let schema = Arc::new(Schema::new(vec![
            Field::new("ts_init", DataType::UInt64, false),
            Field::new("payload", DataType::Utf8, false),
            Field::new(NAUTILUS_ARROW_METADATA_ID_COLUMN, DataType::Utf8, false),
            Field::new(NAUTILUS_ARROW_METADATA_JSON_COLUMN, DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![1])) as ArrayRef,
                Arc::new(StringArray::from(vec![r#"{"value":1}"#])) as ArrayRef,
                Arc::new(StringArray::from(vec![metadata_id])) as ArrayRef,
                Arc::new(StringArray::from(vec![metadata_json])) as ArrayRef,
            ],
        )
        .expect("batch");

        let coalesced =
            coalesce_stream_conversion_batches(&[batch], StreamConversionOptions::default())
                .expect("coalesce")
                .expect("restored batch");
        let restored = restore_staged_record_batches(coalesced).expect("restore metadata");

        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].num_columns(), 2);
        assert_eq!(
            restored[0].schema().metadata().get("type_name"),
            Some(&"Example".to_string()),
        );
        assert_eq!(
            restored[0]
                .schema()
                .field_with_name("payload")
                .expect("payload field")
                .metadata()
                .get("ARROW:extension:name"),
            Some(&"arrow.json".to_string()),
        );
    }

    #[rstest]
    fn coalesce_preserves_each_restored_metadata_run() {
        let first_json = r#"{
            "format_version": 1,
            "schema_metadata": {"instrument_id": "AUD/USD.SIM", "price_precision": "5"},
            "field_metadata": {}
        }"#;
        let second_json = r#"{
            "format_version": 1,
            "schema_metadata": {"instrument_id": "EUR/USD.SIM", "price_precision": "4"},
            "field_metadata": {}
        }"#;
        let schema = Arc::new(Schema::new(vec![
            Field::new("ts_init", DataType::UInt64, false),
            Field::new("identifier", DataType::Utf8, false),
            Field::new(NAUTILUS_ARROW_METADATA_ID_COLUMN, DataType::Utf8, false),
            Field::new(NAUTILUS_ARROW_METADATA_JSON_COLUMN, DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![1, 2])) as ArrayRef,
                Arc::new(StringArray::from(vec!["AUD/USD.SIM", "EUR/USD.SIM"])) as ArrayRef,
                Arc::new(StringArray::from(vec![
                    staged_metadata_id(first_json),
                    staged_metadata_id(second_json),
                ])) as ArrayRef,
                Arc::new(StringArray::from(vec![first_json, second_json])) as ArrayRef,
            ],
        )
        .expect("batch");

        let coalesced =
            coalesce_stream_conversion_batches(&[batch], StreamConversionOptions::default())
                .expect("coalesce")
                .expect("batch");
        let restored = restore_staged_record_batches(coalesced).expect("restore runs");

        assert_eq!(restored.len(), 2);
        assert_eq!(
            restored[0].schema().metadata()["instrument_id"],
            "AUD/USD.SIM"
        );
        assert_eq!(restored[0].schema().metadata()["price_precision"], "5");
        assert_eq!(
            restored[1].schema().metadata()["instrument_id"],
            "EUR/USD.SIM"
        );
        assert_eq!(restored[1].schema().metadata()["price_precision"], "4");
    }

    #[rstest]
    fn stream_conversion_replaces_ts_init_and_internal_bar_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "bar_type".to_string(),
            "AUD/USD.SIM-1-MINUTE-BID-INTERNAL".to_string(),
        );
        let schema = Arc::new(Schema::new_with_metadata(
            vec![
                Field::new("ts_event", DataType::UInt64, false),
                Field::new("ts_init", DataType::UInt64, false),
                Field::new("identifier", DataType::Utf8, false),
            ],
            metadata,
        ));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![10, 20])) as ArrayRef,
                Arc::new(UInt64Array::from(vec![1, 2])) as ArrayRef,
                Arc::new(StringArray::from(vec![
                    "AUD/USD.SIM-1-MINUTE-BID-INTERNAL",
                    "AUD/USD.SIM-1-MINUTE-BID-INTERNAL",
                ])) as ArrayRef,
            ],
        )
        .expect("batch");

        let batches = apply_stream_conversion_transforms(
            &[batch],
            StreamConversionOptions {
                use_ts_event_for_ts_init: true,
                convert_bar_type_to_external: true,
            },
        )
        .expect("transform");

        assert_eq!(
            batches[0].schema().metadata().get("bar_type"),
            Some(&"AUD/USD.SIM-1-MINUTE-BID-EXTERNAL".to_string()),
        );
        assert_eq!(min_max_ts_init(&batches).expect("min max"), (10, 20));
        assert_eq!(
            batches[0]
                .column_by_name("identifier")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            vec![
                Some("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"),
                Some("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"),
            ],
        );
    }

    #[rstest]
    fn stream_conversion_rejects_non_utf8_identifier_column() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("ts_init", DataType::UInt64, false),
            Field::new("identifier", DataType::Int32, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![1])) as ArrayRef,
                Arc::new(Int32Array::from(vec![7])) as ArrayRef,
            ],
        )
        .expect("batch");

        let error = apply_stream_conversion_transform(
            &batch,
            StreamConversionOptions {
                use_ts_event_for_ts_init: false,
                convert_bar_type_to_external: true,
            },
        )
        .expect_err("non-Utf8 identifier column");

        assert_eq!(
            error.to_string(),
            "Cannot convert bar type identifiers to EXTERNAL: identifier column is Int32, \
             expected Utf8",
        );
    }

    #[rstest]
    fn stage_restored_metadata_hashes_canonical_schema_metadata() {
        let metadata = HashMap::from([("type_name".to_string(), "Example".to_string())]);
        let schema = Arc::new(Schema::new_with_metadata(
            vec![Field::new("ts_init", DataType::UInt64, false)],
            metadata,
        ));
        let batch = RecordBatch::try_new(
            schema,
            vec![Arc::new(UInt64Array::from(vec![1])) as ArrayRef],
        )
        .expect("batch");

        let staged = stage_restored_metadata(&batch).expect("staged");
        let ids = staged
            .column_by_name(NAUTILUS_ARROW_METADATA_ID_COLUMN)
            .expect("id column")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("Utf8 id column");

        assert_eq!(
            ids.value(0),
            staged_metadata_id(r#"{"type_name":"Example"}"#)
        );
        let restored = restore_staged_record_batches(staged).expect("restore");
        assert_eq!(restored.len(), 1);
        assert_eq!(
            restored[0].schema().metadata().get("type_name"),
            Some(&"Example".to_string()),
        );
    }

    #[rstest]
    #[case(
        "AUD/USD.SIM-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
        "AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL"
    )]
    #[case(
        "X-INTERNAL.SIM-1-MINUTE-LAST-INTERNAL",
        "X-INTERNAL.SIM-1-MINUTE-LAST-EXTERNAL"
    )]
    #[case("not-a-bar-type-INTERNAL", "not-a-bar-type-INTERNAL")]
    fn stream_conversion_rebuilds_bar_type_metadata(#[case] input: &str, #[case] expected: &str) {
        let schema = Schema::new_with_metadata(
            Vec::<Field>::new(),
            HashMap::from([("bar_type".to_string(), input.to_string())]),
        );

        let converted = schema_with_external_bar_type(&schema);

        assert_eq!(
            converted.metadata().get("bar_type").map(String::as_str),
            Some(expected),
        );
    }

    #[rstest]
    fn stream_conversion_sort_preserves_same_ts_init_input_order() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("ts_init", DataType::UInt64, false),
            Field::new("value", DataType::Int32, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![20, 10, 10])) as ArrayRef,
                Arc::new(Int32Array::from(vec![3, 1, 2])) as ArrayRef,
            ],
        )
        .expect("batch");

        let sorted = coalesce_stream_conversion_batches(
            &[batch],
            StreamConversionOptions {
                use_ts_event_for_ts_init: false,
                convert_bar_type_to_external: false,
            },
        )
        .expect("conversion")
        .expect("batch");
        let values = sorted
            .column(1)
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("value column");

        assert_eq!(values.values(), &[1, 2, 3]);
    }

    fn min_max_ts_init(batches: &[RecordBatch]) -> anyhow::Result<(u64, u64)> {
        let mut min_value: Option<u64> = None;
        let mut max_value: Option<u64> = None;

        for batch in batches {
            let ts_init_idx = batch
                .schema()
                .index_of("ts_init")
                .map_err(|_| anyhow::anyhow!("ts_init column not found"))?;
            let column = batch.column(ts_init_idx);

            for value in ts_values(column.as_ref())? {
                min_value = Some(min_value.map_or(value, |current| current.min(value)));
                max_value = Some(max_value.map_or(value, |current| current.max(value)));
            }
        }

        match (min_value, max_value) {
            (Some(min_value), Some(max_value)) => Ok((min_value, max_value)),
            _ => anyhow::bail!("ts_init column has no values"),
        }
    }
}
