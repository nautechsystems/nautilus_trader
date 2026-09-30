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
//! Centralizes the logic for the custom-data columns of Arrow batches (Parquet/Feather), and
//! custom-data write preparation, path construction, and decode logic
//! so the catalog delegates here instead of inlining custom-specific branching.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use datafusion::arrow::{
    array::{Array as _, StringArray},
    compute::cast,
    datatypes::{DataType as ArrowDataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, CustomData, CustomDataTrait, Data, DataType, FundingRateUpdate, IndexPriceUpdate,
    InstrumentStatus, MarkPriceUpdate, NautilusDataType, OptionGreeks, OrderBookDelta,
    OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose, encode_custom_to_arrow,
    get_arrow_schema,
};
use nautilus_serialization::arrow::{
    DecodeDataFromRecordBatch, KEY_CUSTOM_DATA_METADATA, KEY_IDENTIFIER, KEY_TYPE_NAME,
    StringColumnRef, custom::CustomDataDecoder, json_string_field, timestamp_data_type,
};

use crate::{
    catalog::types::data_type_from_data_path_prefix, common::paths::urisafe_instrument_id,
};

/// Returns `schema` with the `type_name` metadata entry of a custom type.
#[must_use]
pub(crate) fn schema_with_type_name(base_schema: &Schema, type_name: &str) -> Schema {
    let mut metadata = base_schema.metadata().clone();
    metadata.insert(KEY_TYPE_NAME.to_string(), type_name.to_string());

    Schema::new_with_metadata(base_schema.fields().clone(), metadata)
}

/// Returns `schema` with the nullable `identifier` column appended, the column every custom
/// producer adds because a registered Arrow encoding carries no identity.
#[must_use]
pub(crate) fn schema_with_identifier_column(schema: &Schema) -> Schema {
    let mut fields = schema.fields().iter().cloned().collect::<Vec<_>>();
    fields.push(Arc::new(Field::new(
        KEY_IDENTIFIER,
        ArrowDataType::Utf8,
        true,
    )));

    Schema::new_with_metadata(fields, schema.metadata().clone())
}

fn append_identifier_column(
    batch: &RecordBatch,
    identifier: Option<&str>,
) -> anyhow::Result<RecordBatch> {
    let schema = schema_with_identifier_column(batch.schema().as_ref());
    let mut columns = batch.columns().to_vec();
    columns.push(Arc::new(StringArray::from(vec![
        identifier.map(
            ToString::to_string
        );
        batch.num_rows()
    ])));

    Ok(RecordBatch::try_new(Arc::new(schema), columns)?)
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

/// Encodes custom data rows into the catalog batch shape every producer writes.
///
/// Appends the `identifier` column and the `custom_data_metadata` column (the `DataType`
/// metadata as JSON, NULL when absent) to the registered Arrow encoding, and sets the `type_name`
/// metadata entry.
///
/// # Errors
///
/// Returns an error if the type is not registered for Arrow encoding, encoding fails, or the
/// `DataType` cannot be serialized.
pub fn encode_custom_rows(
    type_name: &str,
    items: &[Arc<dyn CustomDataTrait>],
    data_type: &DataType,
) -> anyhow::Result<RecordBatch> {
    let batch = encode_custom_to_arrow(type_name, items)
        .map_err(|e| anyhow::anyhow!("Failed to encode custom data to Arrow: {e}"))?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Custom data type \"{type_name}\" is not registered for Arrow encoding; \
                 call register_custom_data_class or ensure_custom_data_registered before writing"
            )
        })?;
    let batch = RecordBatch::try_new(
        Arc::new(schema_with_type_name(batch.schema().as_ref(), type_name)),
        batch.columns().to_vec(),
    )?;
    let batch = append_identifier_column(&batch, data_type.identifier())?;

    append_custom_data_metadata_column(&batch, data_type)
}

/// Replaces the `data_type` JSON column of a batch written before `custom_data_metadata` existed
/// with the `custom_data_metadata` column, so migrated files match a fresh write.
///
/// A batch that already has `custom_data_metadata`, or no `data_type` column, is returned
/// unchanged.
///
/// # Errors
///
/// Returns an error if the stored `DataType` cannot be parsed or the batch cannot be rebuilt.
pub(crate) fn upgrade_legacy_custom_batch(batch: RecordBatch) -> anyhow::Result<RecordBatch> {
    const LEGACY_COLUMN: &str = "data_type";

    let schema = batch.schema();
    let Ok(legacy_index) = schema.index_of(LEGACY_COLUMN) else {
        return Ok(batch);
    };

    if schema.index_of(KEY_CUSTOM_DATA_METADATA).is_ok() {
        return Ok(batch);
    }
    let data_type = if batch.num_rows() == 0 || batch.column(legacy_index).is_null(0) {
        None
    } else {
        let json = StringColumnRef::try_from_array(batch.column(legacy_index).as_ref())
            .ok_or_else(|| anyhow::anyhow!("data_type column must be a string column"))?
            .value(0)
            .to_string();

        Some(DataType::from_persistence_json(&json)?)
    };
    let mut fields = schema.fields().iter().cloned().collect::<Vec<_>>();
    let mut columns = batch.columns().to_vec();
    fields.remove(legacy_index);
    columns.remove(legacy_index);
    let stripped = RecordBatch::try_new(
        Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone())),
        columns,
    )?;

    match data_type {
        Some(data_type) => append_custom_data_metadata_column(&stripped, &data_type),
        None => append_custom_data_metadata_column(&stripped, &DataType::new("", None, None)),
    }
}

/// Returns `schema` with the nullable `custom_data_metadata` JSON column appended.
#[must_use]
pub fn schema_with_custom_data_metadata_column(schema: &Schema) -> Schema {
    let mut fields: Vec<_> = schema.fields().iter().cloned().collect();
    fields.push(Arc::new(json_string_field(KEY_CUSTOM_DATA_METADATA, true)));

    Schema::new_with_metadata(fields, schema.metadata().clone())
}

fn append_custom_data_metadata_column(
    batch: &RecordBatch,
    data_type: &DataType,
) -> anyhow::Result<RecordBatch> {
    let metadata_json = data_type
        .metadata()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| anyhow::anyhow!("Failed to serialize DataType metadata: {e}"))?;

    let schema = schema_with_custom_data_metadata_column(batch.schema().as_ref());
    let mut columns = batch.columns().to_vec();
    columns.push(Arc::new(StringArray::from(vec![
        metadata_json.as_deref();
        batch.num_rows()
    ])));

    RecordBatch::try_new(Arc::new(schema), columns)
        .map_err(|e| anyhow::anyhow!("Failed to append custom data metadata column: {e}"))
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
/// Returns an error if encoding or augmentation fails, if the type is not registered, or if the
/// registered Arrow schema omits `ts_init` or carries timestamps the catalog cannot query.
pub fn prepare_custom_data_batch(
    data: &[&CustomData],
) -> anyhow::Result<(RecordBatch, String, Option<String>, UnixNanos, UnixNanos)> {
    let Some(first_custom) = data.first() else {
        anyhow::bail!("prepare_custom_data_batch called with empty data");
    };

    let type_name = first_custom.data.type_name();
    let identifier = first_custom.data_type.identifier().map(String::from);
    let metadata_str = first_custom.data_type.metadata_str();

    let mut start_ts = first_custom.data.ts_init();
    let mut end_ts = start_ts;

    for custom in data {
        anyhow::ensure!(
            custom.data.type_name() == type_name
                && custom.data_type.identifier() == identifier.as_deref()
                && custom.data_type.metadata_str() == metadata_str,
            "Cannot prepare one custom data batch from mixed DataType values",
        );

        let ts_init = custom.data.ts_init();
        start_ts = start_ts.min(ts_init);
        end_ts = end_ts.max(ts_init);
    }

    let items: Vec<Arc<dyn CustomDataTrait>> = data.iter().map(|c| Arc::clone(&c.data)).collect();

    if let Some(schema) = get_arrow_schema(type_name) {
        validate_custom_catalog_schema(type_name, &schema)?;
    }

    let batch = encode_custom_rows(type_name, &items, &first_custom.data_type)?;

    Ok((batch, type_name.to_string(), identifier, start_ts, end_ts))
}

pub(crate) fn validate_custom_catalog_schema(
    type_name: &str,
    schema: &Schema,
) -> anyhow::Result<()> {
    if schema.field_with_name("ts_init").is_err() {
        anyhow::bail!(
            "Custom data type \"{type_name}\" is registered without an Arrow schema containing \
             ts_init, so written files cannot be queried back; define an `arrow_schema_py()` \
             class method, or apply the `@customdataclass` decorator"
        );
    }

    for name in ["ts_event", "ts_init"] {
        if let Ok(field) = schema.field_with_name(name)
            && field.data_type() != &timestamp_data_type()
        {
            anyhow::bail!(
                "Custom data type \"{type_name}\" is registered with {name} as {}, so written \
                 files cannot be queried back; declare it as timestamp(\"ns\", tz=\"UTC\"), or \
                 apply the `@customdataclass` decorator",
                field.data_type(),
            );
        }
    }

    Ok(())
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
                NautilusDataType::Custom { type_name: registered_type_name } if $allow_custom => {
                    // The decoder registry is keyed by the bare type name
                    let mut metadata = $metadata.clone();
                    metadata.insert("type_name".to_string(), registered_type_name);
                    Ok(CustomDataDecoder::decode_data_batch(&metadata, $batch)?)
                }
                NautilusDataType::Custom { .. } => anyhow::bail!(
                    "Unknown data type: {}; custom decode only allowed in custom data context",
                    $type_name,
                ),
                NautilusDataType::Instrument => {
                    anyhow::bail!("Instrument batches require instrument-specific decoding")
                }
                #[cfg(feature = "defi")]
                NautilusDataType::Defi => {
                    anyhow::bail!("DeFi batches require DeFi-specific decoding")
                }
                #[cfg(not(feature = "defi"))]
                #[allow(unreachable_patterns, reason = "DeFi variants can exist without this crate's defi feature")]
                _ => anyhow::bail!("DeFi batches require DeFi-specific decoding"),
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

/// Splits a batch into runs of consecutive rows that share one identifier and one
/// `custom_data_metadata` value, so each run decodes under its own `DataType`.
///
/// A batch without the metadata column, or with fewer than two rows, is returned whole.
///
/// # Errors
///
/// Returns an error if the identifier or metadata column cannot be read as strings.
pub(crate) fn split_batch_by_custom_data_type(
    batch: RecordBatch,
) -> anyhow::Result<Vec<RecordBatch>> {
    let schema = batch.schema();
    let Ok(metadata_index) = schema.index_of(KEY_CUSTOM_DATA_METADATA) else {
        return Ok(vec![batch]);
    };

    if batch.num_rows() < 2 {
        return Ok(vec![batch]);
    }
    let key_indices = [Some(metadata_index), schema.index_of(KEY_IDENTIFIER).ok()];
    let mut columns = Vec::new();

    for index in key_indices.into_iter().flatten() {
        let column = cast(batch.column(index), &ArrowDataType::Utf8)?;
        let column = column
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow::anyhow!("custom data key column is not a string column"))?
            .clone();
        columns.push(column);
    }
    let key = |row: usize| {
        columns
            .iter()
            .map(|column| (!column.is_null(row)).then(|| column.value(row)))
            .collect::<Vec<_>>()
    };
    let mut runs = Vec::new();
    let mut start = 0;

    for row in 1..batch.num_rows() {
        if key(row) != key(start) {
            runs.push(batch.slice(start, row - start));
            start = row;
        }
    }
    runs.push(batch.slice(start, batch.num_rows() - start));

    Ok(runs)
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
    let batches = batches
        .into_iter()
        .map(split_batch_by_custom_data_type)
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let Some(first_batch) = batches.first() else {
        return Ok(Vec::new());
    };

    let schema = first_batch.schema();

    let ts_columns = if use_ts_event_for_ts_init {
        schema
            .index_of("ts_event")
            .ok()
            .zip(schema.index_of("ts_init").ok())
    } else {
        None
    };

    let mut file_data = Vec::new();

    for mut batch in batches {
        if let Some((ts_event_idx, ts_init_idx)) = ts_columns {
            let mut new_columns = batch.columns().to_vec();
            new_columns[ts_init_idx] = new_columns[ts_event_idx].clone();
            batch = RecordBatch::try_new(schema.clone(), new_columns)
                .map_err(|e| anyhow::anyhow!("Failed to create new batch: {e}"))?;
        }

        let metadata = batch.schema().metadata().clone();
        let decoded = decode_batch_to_data(&metadata, batch, true)?;
        file_data.extend(decoded);
    }

    Ok(file_data)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use datafusion::arrow::{
        array::{Array as _, StringArray},
        datatypes::{DataType as ArrowDataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use nautilus_core::{Params, UnixNanos};
    use nautilus_model::{
        data::{CustomData, Data, DataType, QuoteTick},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use nautilus_serialization::{
        arrow::{EncodeToRecordBatch, KEY_CUSTOM_DATA_METADATA, timestamp_data_type},
        ensure_custom_data_registered,
    };
    use rstest::rstest;

    use super::{
        custom_data_path_components, decode_batch_to_data, decode_custom_batches_to_data,
        group_custom_data_by_type, prepare_custom_data_batch, split_batch_by_custom_data_type,
        validate_custom_catalog_schema,
    };
    use crate::{common::test_data::RustTestCustomData, writer::feather::FeatherWriter};

    #[rstest]
    fn test_validate_custom_catalog_schema_accepts_catalog_timestamps() {
        let schema = schema_with_timestamps(timestamp_data_type(), timestamp_data_type());

        assert!(validate_custom_catalog_schema("SensorReading", &schema).is_ok());
    }

    #[rstest]
    fn test_validate_custom_catalog_schema_rejects_empty_schema() {
        let error = validate_custom_catalog_schema("SensorReading", &Schema::empty()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Custom data type \"SensorReading\" is registered without an Arrow schema containing \
             ts_init, so written files cannot be queried back; define an `arrow_schema_py()` \
             class method, or apply the `@customdataclass` decorator"
        );
    }

    #[rstest]
    #[case("ts_event", ArrowDataType::UInt64, timestamp_data_type())]
    #[case("ts_init", timestamp_data_type(), ArrowDataType::UInt64)]
    fn test_validate_custom_catalog_schema_rejects_integer_timestamps(
        #[case] expected_name: &str,
        #[case] ts_event: ArrowDataType,
        #[case] ts_init: ArrowDataType,
    ) {
        let schema = schema_with_timestamps(ts_event, ts_init);

        let error = validate_custom_catalog_schema("SensorReading", &schema).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Custom data type \"SensorReading\" is registered with {expected_name} as UInt64, \
                 so written files cannot be queried back; declare it as \
                 timestamp(\"ns\", tz=\"UTC\"), or apply the `@customdataclass` decorator"
            )
        );
    }

    fn schema_with_timestamps(ts_event: ArrowDataType, ts_init: ArrowDataType) -> Schema {
        Schema::new(vec![
            Field::new("value", ArrowDataType::Float64, false),
            Field::new("ts_event", ts_event, false),
            Field::new("ts_init", ts_init, false),
        ])
    }

    fn test_custom(
        identifier: &str,
        ts_event: u64,
        ts_init: u64,
        metadata: Option<Params>,
    ) -> CustomData {
        CustomData::new(
            Arc::new(RustTestCustomData {
                instrument_id: InstrumentId::from("RUST.TEST"),
                value: 1.5,
                flag: true,
                ts_event: UnixNanos::from(ts_event),
                ts_init: UnixNanos::from(ts_init),
            }),
            DataType::new("RustTestCustomData", metadata, Some(identifier.to_string())),
        )
    }

    fn custom_rows(data: &[Data]) -> Vec<(String, Option<String>, u64, u64)> {
        data.iter()
            .map(|data| match data {
                Data::Custom(custom) => (
                    custom.data_type.type_name().to_string(),
                    custom.data_type.identifier().map(str::to_string),
                    custom.data.ts_event().as_u64(),
                    custom.data.ts_init().as_u64(),
                ),
                other => panic!("Expected custom data, received {other:?}"),
            })
            .collect()
    }

    #[rstest]
    #[case::without_identifier(None, &["data", "custom", "SensorReading"])]
    #[case::sanitized_identifier(
        Some("BTC/USD.BINANCE"),
        &["data", "custom", "SensorReading", "BTCUSD.BINANCE"]
    )]
    #[case::identifier_empty_after_sanitizing(Some("/"), &["data", "custom", "SensorReading"])]
    fn custom_data_path_components_sanitize_identifier(
        #[case] identifier: Option<&str>,
        #[case] expected: &[&str],
    ) {
        assert_eq!(
            custom_data_path_components("SensorReading", identifier),
            expected
        );
    }

    #[rstest]
    fn group_custom_data_by_type_groups_by_identifier_and_metadata() {
        let mut metadata = Params::new();
        metadata.insert("source".to_string(), "replay".into());
        let data = [
            test_custom("A", 1, 1, None),
            test_custom("B", 2, 2, None),
            test_custom("A", 3, 3, None),
            test_custom("A", 4, 4, Some(metadata)),
        ];

        let groups = group_custom_data_by_type(data.iter())
            .into_iter()
            .map(|group| {
                group
                    .iter()
                    .map(|custom| custom.data.ts_init().as_u64())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        assert_eq!(groups, vec![vec![1, 3], vec![4], vec![2]]);
    }

    #[rstest]
    fn prepare_custom_data_batch_rejects_empty_input() {
        let error = prepare_custom_data_batch(&[]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "prepare_custom_data_batch called with empty data"
        );
    }

    #[rstest]
    #[case::identifier(test_custom("B", 2, 2, None))]
    #[case::metadata({
        let mut metadata = Params::new();
        metadata.insert("source".to_string(), "replay".into());
        test_custom("A", 2, 2, Some(metadata))
    })]
    fn prepare_custom_data_batch_rejects_mixed_data_types(#[case] other: CustomData) {
        let first = test_custom("A", 1, 1, None);

        let error = prepare_custom_data_batch(&[&first, &other]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Cannot prepare one custom data batch from mixed DataType values"
        );
    }

    #[rstest]
    fn prepare_custom_data_batch_reports_ts_init_range_of_unordered_rows() {
        ensure_custom_data_registered::<RustTestCustomData>();
        let data = [
            test_custom("A", 21, 21, None),
            test_custom("A", 11, 11, None),
            test_custom("A", 31, 31, None),
        ];
        let refs = data.iter().collect::<Vec<_>>();

        let (_, _, _, start, end) = prepare_custom_data_batch(&refs).unwrap();

        assert_eq!((start, end), (UnixNanos::from(11), UnixNanos::from(31)));
    }

    #[rstest]
    fn custom_producers_return_identical_batches_with_typed_metadata_column() {
        ensure_custom_data_registered::<RustTestCustomData>();
        let mut metadata = Params::new();
        metadata.insert("source".to_string(), serde_json::json!("reuters"));
        metadata.insert("max_items".to_string(), serde_json::json!(10));
        let custom = test_custom("AAPL.XNAS", 5, 7, Some(metadata));

        let (prepared, _, _, _, _) = prepare_custom_data_batch(&[&custom]).unwrap();
        let staged = FeatherWriter::encode_custom_to_batch(&custom).unwrap();
        let metadata_column = prepared
            .column_by_name(KEY_CUSTOM_DATA_METADATA)
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let metadata_value: serde_json::Value =
            serde_json::from_str(metadata_column.value(0)).unwrap();

        assert_eq!(prepared, staged);
        assert_eq!(
            metadata_value,
            serde_json::json!({"source": "reuters", "max_items": 10})
        );
        assert!(metadata_value["max_items"].is_u64());
    }

    #[rstest]
    fn custom_batch_metadata_column_is_null_without_datatype_metadata() {
        ensure_custom_data_registered::<RustTestCustomData>();
        let custom = test_custom("AAPL.XNAS", 5, 7, None);

        let (batch, _, _, _, _) = prepare_custom_data_batch(&[&custom]).unwrap();

        assert_eq!(
            batch
                .column_by_name(KEY_CUSTOM_DATA_METADATA)
                .unwrap()
                .null_count(),
            1
        );
    }

    #[rstest]
    fn custom_data_type_round_trips_from_identifier_and_metadata_columns() {
        ensure_custom_data_registered::<RustTestCustomData>();
        let mut metadata = Params::new();
        metadata.insert("source".to_string(), serde_json::json!("reuters"));
        metadata.insert("max_items".to_string(), serde_json::json!(10));
        let custom = test_custom("AAPL.XNAS", 5, 7, Some(metadata));
        let (batch, _, _, _, _) = prepare_custom_data_batch(&[&custom]).unwrap();

        let decoded = decode_custom_batches_to_data(vec![batch], false).unwrap();

        let Data::Custom(decoded) = &decoded[0] else {
            panic!("expected custom data");
        };
        assert_eq!(decoded.data_type, custom.data_type);
        assert!(decoded.data_type.metadata().unwrap()["max_items"].is_u64());
    }

    #[rstest]
    fn custom_data_with_two_metadata_values_under_one_identifier_decodes_separately() {
        ensure_custom_data_registered::<RustTestCustomData>();
        let reuters = Params::from_index_map(
            [("source".to_string(), serde_json::json!("reuters"))]
                .into_iter()
                .collect(),
        );
        let bloomberg = Params::from_index_map(
            [("source".to_string(), serde_json::json!("bloomberg"))]
                .into_iter()
                .collect(),
        );
        let first = test_custom("AAPL.XNAS", 5, 7, Some(reuters));
        let second = test_custom("AAPL.XNAS", 6, 8, Some(bloomberg));
        let batches = [&first, &second]
            .into_iter()
            .map(|custom| {
                let (batch, _, _, _, _) = prepare_custom_data_batch(&[custom]).unwrap();
                batch
            })
            .collect::<Vec<_>>();

        let decoded = decode_custom_batches_to_data(batches, false).unwrap();

        let data_types = decoded
            .iter()
            .map(|data| match data {
                Data::Custom(custom) => custom.data_type.clone(),
                _ => panic!("expected custom data"),
            })
            .collect::<Vec<_>>();
        assert_eq!(data_types, vec![first.data_type, second.data_type]);
    }

    #[rstest]
    fn one_batch_holding_two_metadata_values_decodes_each_row_under_its_own_data_type() {
        ensure_custom_data_registered::<RustTestCustomData>();
        let metadata = |source: &str| {
            Params::from_index_map(
                [("source".to_string(), serde_json::json!(source))]
                    .into_iter()
                    .collect(),
            )
        };
        let rows = [
            test_custom("AAPL.XNAS", 5, 7, Some(metadata("reuters"))),
            test_custom("AAPL.XNAS", 6, 8, Some(metadata("reuters"))),
            test_custom("AAPL.XNAS", 7, 9, Some(metadata("bloomberg"))),
            test_custom("MSFT.XNAS", 8, 10, Some(metadata("bloomberg"))),
        ];
        let batches = rows
            .iter()
            .map(|custom| prepare_custom_data_batch(&[custom]).unwrap().0)
            .collect::<Vec<_>>();
        let schema = batches[0].schema();
        let merged = arrow::compute::concat_batches(&schema, &batches).unwrap();

        let runs = split_batch_by_custom_data_type(merged.clone()).unwrap();
        let decoded = decode_custom_batches_to_data(vec![merged], false).unwrap();

        assert_eq!(
            runs.iter().map(RecordBatch::num_rows).collect::<Vec<_>>(),
            vec![2, 1, 1]
        );
        assert_eq!(
            decoded
                .iter()
                .map(|data| match data {
                    Data::Custom(custom) => custom.data_type.clone(),
                    _ => panic!("expected custom data"),
                })
                .collect::<Vec<_>>(),
            rows.iter()
                .map(|custom| custom.data_type.clone())
                .collect::<Vec<_>>()
        );
    }

    #[rstest]
    fn custom_data_type_without_metadata_decodes_with_null_metadata_column() {
        ensure_custom_data_registered::<RustTestCustomData>();
        let custom = test_custom("AAPL.XNAS", 5, 7, None);
        let (batch, _, _, _, _) = prepare_custom_data_batch(&[&custom]).unwrap();

        let decoded = decode_custom_batches_to_data(vec![batch], false).unwrap();

        let Data::Custom(decoded) = &decoded[0] else {
            panic!("expected custom data");
        };
        assert_eq!(decoded.data_type, custom.data_type);
        assert_eq!(decoded.data_type.metadata(), None);
    }

    #[rstest]
    fn decode_custom_batches_to_data_returns_empty_for_no_batches() {
        let decoded = decode_custom_batches_to_data(Vec::new(), true).unwrap();

        assert_eq!(decoded, Vec::<Data>::new());
    }

    #[rstest]
    #[case::keeps_ts_init(false, [11, 21])]
    #[case::uses_ts_event(true, [10, 20])]
    fn prepared_custom_batch_decodes_back_to_custom_data(
        #[case] use_ts_event_for_ts_init: bool,
        #[case] expected_ts_init: [u64; 2],
    ) {
        ensure_custom_data_registered::<RustTestCustomData>();
        let data = [
            test_custom("A", 10, 11, None),
            test_custom("A", 20, 21, None),
        ];
        let refs = data.iter().collect::<Vec<_>>();

        let (batch, type_name, identifier, start, end) = prepare_custom_data_batch(&refs).unwrap();
        let decoded = decode_custom_batches_to_data(vec![batch], use_ts_event_for_ts_init).unwrap();

        assert_eq!(type_name, "RustTestCustomData");
        assert_eq!(identifier.as_deref(), Some("A"));
        assert_eq!((start, end), (UnixNanos::from(11), UnixNanos::from(21)));
        assert_eq!(
            custom_rows(&decoded),
            vec![
                (
                    "RustTestCustomData".to_string(),
                    Some("A".to_string()),
                    10,
                    expected_ts_init[0],
                ),
                (
                    "RustTestCustomData".to_string(),
                    Some("A".to_string()),
                    20,
                    expected_ts_init[1],
                ),
            ],
        );
    }

    fn prepared_custom_metadata(type_name: Option<&str>) -> (HashMap<String, String>, RecordBatch) {
        ensure_custom_data_registered::<RustTestCustomData>();
        let custom = test_custom("A", 10, 10, None);
        let batch = prepare_custom_data_batch(&[&custom]).unwrap().0;
        let mut metadata = batch.schema().metadata().clone();
        metadata.remove("type_name");

        if let Some(type_name) = type_name {
            metadata.insert("type_name".to_string(), type_name.to_string());
        }

        (metadata, batch)
    }

    #[rstest]
    #[case::missing_type_name(None, "Missing type_name in metadata")]
    #[case::bare_custom_type_name(
        Some("RustTestCustomData"),
        "Invalid `NautilusDataType`: 'RustTestCustomData'"
    )]
    #[case::custom_prefix(
        Some("custom/RustTestCustomData"),
        "Unknown data type: custom/RustTestCustomData; custom decode only allowed in custom data context"
    )]
    fn decode_batch_to_data_rejects_custom_types_outside_custom_context(
        #[case] type_name: Option<&str>,
        #[case] expected: &str,
    ) {
        let (metadata, batch) = prepared_custom_metadata(type_name);

        let error = decode_batch_to_data(&metadata, batch, false).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::bare_custom_type_name("RustTestCustomData")]
    #[case::custom_path_prefix("custom/RustTestCustomData")]
    #[case::custom_type_prefix("Custom:RustTestCustomData")]
    fn decode_batch_to_data_decodes_custom_types_in_custom_context(#[case] type_name: &str) {
        let (metadata, batch) = prepared_custom_metadata(Some(type_name));

        let decoded = decode_batch_to_data(&metadata, batch, true).unwrap();

        assert_eq!(
            custom_rows(&decoded),
            vec![(
                "RustTestCustomData".to_string(),
                Some("A".to_string()),
                10,
                10
            )],
        );
    }

    #[rstest]
    fn decode_batch_to_data_decodes_built_in_type_names() {
        let quote = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.00001"),
            Price::from("1.00002"),
            Quantity::from("100000"),
            Quantity::from("200000"),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let mut metadata = quote.metadata();
        let batch = QuoteTick::encode_batch(&metadata, &[quote]).unwrap();
        metadata.insert("type_name".to_string(), "quotes".to_string());

        let decoded = decode_batch_to_data(&metadata, batch, false).unwrap();

        assert_eq!(decoded, vec![Data::Quote(quote)]);
    }
}
