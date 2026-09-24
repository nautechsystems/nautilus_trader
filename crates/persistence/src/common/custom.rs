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
    TradeTick, close::InstrumentClose, encode_custom_to_arrow, get_arrow_schema,
};
use nautilus_serialization::arrow::{
    DecodeDataFromRecordBatch, custom::CustomDataDecoder, record_batch_with_identifier_column,
    timestamp_data_type,
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
    let data_type_array: Arc<dyn Array> =
        Arc::new(StringArray::from(vec![data_type_json; num_rows]));
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
    RecordBatch::try_new(new_schema, columns)
        .map_err(|e| anyhow::anyhow!("Failed to merge custom data type metadata: {e}"))
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
    let dt_meta = first_custom.data_type.metadata_string_map();
    let data_type_json = first_custom
        .data_type
        .to_persistence_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize data_type for persistence: {e}"))?;

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
        arrow::{EncodeToRecordBatch, timestamp_data_type},
        ensure_custom_data_registered,
    };
    use rstest::rstest;

    use super::{
        custom_data_path_components, decode_batch_to_data, decode_custom_batches_to_data,
        group_custom_data_by_type, prepare_custom_data_batch, validate_custom_catalog_schema,
    };
    use crate::test_data::RustTestCustomData;

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
