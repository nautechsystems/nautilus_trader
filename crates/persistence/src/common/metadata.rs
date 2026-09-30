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

//! Shared catalog metadata conversion.

use std::{collections::HashMap, sync::Arc};

use arrow::{datatypes::Schema, record_batch::RecordBatch};
use nautilus_core::Params;
use nautilus_model::data::{NautilusDataType, NautilusRecordType};
pub(crate) use nautilus_serialization::arrow::stored_metadata::{
    metadata_hash, restored_metadata, stored_metadata,
};
#[cfg(test)]
use nautilus_serialization::arrow::stored_metadata::{
    restore_derivable_metadata_key, strip_derivable_metadata_keys,
};
use nautilus_serialization::arrow::{
    KEY_ACCOUNT_ID, KEY_BAR_TYPE, KEY_IDENTIFIER, KEY_INSTRUMENT_ID, StringColumnRef, U64ColumnRef,
};

use crate::catalog::types::CatalogDataType;

/// Returns the metadata key that the `identifier` column restores for `data_type`, if any.
///
/// Bars restore `bar_type`, account states and execution mass status restore `account_id`, custom
/// data restores nothing, and every other type restores `instrument_id`.
#[must_use]
pub(crate) fn derivable_metadata_key(data_type: &CatalogDataType) -> Option<&'static str> {
    match data_type {
        CatalogDataType::Data(NautilusDataType::Custom { .. }) => None,
        CatalogDataType::Data(NautilusDataType::Bar) => Some(KEY_BAR_TYPE),
        CatalogDataType::Record(
            NautilusRecordType::AccountState | NautilusRecordType::ExecutionMassStatus,
        ) => Some(KEY_ACCOUNT_ID),
        _ => Some(KEY_INSTRUMENT_ID),
    }
}

/// Returns the type name that `data_type`'s storage location implies: the built-in type name or
/// the custom type name.
///
/// Instruments return `None`: a staged instrument file's path does not name its class, so the
/// class stays in the stored metadata as `type_name`.
#[must_use]
pub(crate) fn location_type_name(data_type: &CatalogDataType) -> Option<String> {
    match data_type {
        CatalogDataType::Data(NautilusDataType::Custom { type_name }) => Some(type_name.clone()),
        CatalogDataType::Data(NautilusDataType::Instrument) | CatalogDataType::Instrument(_) => {
            None
        }
        CatalogDataType::Data(data_type) => Some(data_type.to_string()),
        CatalogDataType::Record(record_type) => Some(record_type.to_string()),
    }
}

/// Returns the first non-null `identifier` value of `batch`, whatever its string encoding.
pub(crate) fn batch_identifier(batch: &RecordBatch) -> Option<String> {
    let column = batch.column_by_name(KEY_IDENTIFIER)?;
    let values = StringColumnRef::try_from_array(column.as_ref())?;

    (0..values.len())
        .find(|row| !values.is_null(*row))
        .map(|row| values.value(row).to_string())
}

fn batch_with_metadata(
    batch: &RecordBatch,
    metadata: HashMap<String, String>,
) -> anyhow::Result<RecordBatch> {
    let schema = Schema::new_with_metadata(batch.schema().fields().clone(), metadata);

    Ok(RecordBatch::try_new(
        Arc::new(schema),
        batch.columns().to_vec(),
    )?)
}

/// Replaces the schema metadata of each batch with its stored map, for file formats that keep
/// schema metadata in the file (Parquet).
///
/// A batch without an identifier keeps its metadata, since nothing then restores what is dropped.
///
/// # Errors
///
/// Returns an error if a batch cannot be rebuilt.
pub(crate) fn batches_with_stored_metadata(
    batches: &[RecordBatch],
    data_type: &CatalogDataType,
) -> anyhow::Result<Vec<RecordBatch>> {
    batches
        .iter()
        .map(|batch| {
            let Some(identifier) = batch_identifier(batch) else {
                return Ok(batch.clone());
            };
            let stored =
                stored_catalog_metadata(batch.schema().metadata(), Some(&identifier), data_type);

            batch_with_metadata(batch, stored)
        })
        .collect()
}

/// Returns the slim map to store for `data_type`, dropping the entries its identifier restores.
///
/// Custom data restores nothing: its container defines the identifier separately from its fields,
/// so every metadata entry is kept even when it equals the identifier.
pub(crate) fn stored_catalog_metadata(
    metadata: &HashMap<String, String>,
    identifier: Option<&str>,
    data_type: &CatalogDataType,
) -> HashMap<String, String> {
    stored_metadata(
        metadata,
        identifier.filter(|_| derivable_metadata_key(data_type).is_some()),
        location_type_name(data_type).as_deref(),
    )
}

/// Returns the full metadata decoders expect from a file's stored schema metadata.
pub(crate) fn metadata_from_stored(
    stored: HashMap<String, String>,
    identifier: Option<&str>,
    data_type: &CatalogDataType,
) -> HashMap<String, String> {
    restored_metadata(
        stored,
        derivable_metadata_key(data_type),
        identifier,
        location_type_name(data_type).as_deref(),
    )
}

/// Restores the full schema metadata of batches read from a file that stores the slim map.
///
/// # Errors
///
/// Returns an error if a batch cannot be rebuilt.
pub(crate) fn batches_with_restored_metadata(
    batches: Vec<RecordBatch>,
    data_type: &CatalogDataType,
) -> anyhow::Result<Vec<RecordBatch>> {
    batches
        .into_iter()
        .map(|batch| {
            let identifier = batch_identifier(&batch);
            let metadata = metadata_from_stored(
                batch.schema().metadata().clone(),
                identifier.as_deref(),
                data_type,
            );

            batch_with_metadata(&batch, metadata)
        })
        .collect()
}

/// Converts string Arrow schema metadata into generic catalog params.
#[must_use]
pub(crate) fn arrow_metadata_to_params(metadata: &HashMap<String, String>) -> Params {
    let mut params = Params::new();
    let mut entries = metadata.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(key, _)| *key);

    for (key, value) in entries {
        params.insert(key.clone(), serde_json::Value::String(value.clone()));
    }

    params
}

/// Returns the inclusive `ts_init` range across Arrow record batches.
pub(crate) fn record_batch_ts_init_range(batches: &[RecordBatch]) -> anyhow::Result<(u64, u64)> {
    let mut range: Option<(u64, u64)> = None;

    for batch in batches {
        let ts_init = batch
            .column_by_name("ts_init")
            .ok_or_else(|| anyhow::anyhow!("ts_init column not found"))?;
        let ts_init = U64ColumnRef::try_from_array(ts_init.as_ref())
            .ok_or_else(|| anyhow::anyhow!("ts_init column has an unsupported type"))?;

        for row in 0..ts_init.len() {
            if ts_init.is_null(row) {
                anyhow::bail!("ts_init column contains null values");
            }

            let value = ts_init
                .value(row)
                .ok_or_else(|| anyhow::anyhow!("ts_init column contains a negative value"))?;
            range = Some(range.map_or((value, value), |(start_ts, end_ts)| {
                (start_ts.min(value), end_ts.max(value))
            }));
        }
    }

    range.ok_or_else(|| anyhow::anyhow!("Record batches contain no non-null ts_init values"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{ArrayRef, TimestampNanosecondArray, UInt64Array},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use nautilus_model::instruments::NautilusInstrumentType;
    use nautilus_serialization::arrow::KEY_TYPE_NAME;
    use rstest::rstest;

    use super::*;

    fn ts_init_batch(values: Vec<u64>) -> RecordBatch {
        let field = Field::new("ts_init", DataType::UInt64, false);
        let schema = Arc::new(Schema::new(vec![field]));
        let column: ArrayRef = Arc::new(UInt64Array::from(values));
        RecordBatch::try_new(schema, vec![column]).unwrap()
    }

    #[rstest]
    fn record_batch_ts_init_range_spans_unordered_batches() {
        let batches = [ts_init_batch(vec![5, 3]), ts_init_batch(vec![9, 1, 4])];

        let range = record_batch_ts_init_range(&batches).unwrap();

        assert_eq!(range, (1, 9));
    }

    #[rstest]
    fn record_batch_ts_init_range_rejects_batches_without_rows() {
        let error = record_batch_ts_init_range(&[ts_init_batch(Vec::new())]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Record batches contain no non-null ts_init values"
        );
    }

    fn batch(values: Vec<Option<u64>>) -> RecordBatch {
        RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "ts_init",
                DataType::UInt64,
                true,
            )])),
            vec![Arc::new(UInt64Array::from(values)) as ArrayRef],
        )
        .unwrap()
    }

    #[rstest]
    fn strip_removes_only_identifier_matching_derivable_keys() {
        let metadata = HashMap::from([
            ("instrument_id".to_string(), "AUD/USD.SIM".to_string()),
            ("price_precision".to_string(), "5".to_string()),
        ]);

        let stripped = strip_derivable_metadata_keys(&metadata, "AUD/USD.SIM");

        assert_eq!(
            stripped,
            HashMap::from([("price_precision".to_string(), "5".to_string())]),
        );
    }

    #[rstest]
    fn strip_removes_bar_type_and_its_instrument_id() {
        let bar_type = "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL";
        let metadata = HashMap::from([
            ("bar_type".to_string(), bar_type.to_string()),
            ("instrument_id".to_string(), "AUD/USD.SIM".to_string()),
            ("price_precision".to_string(), "5".to_string()),
        ]);

        let stripped = strip_derivable_metadata_keys(&metadata, bar_type);

        assert_eq!(
            stripped,
            HashMap::from([("price_precision".to_string(), "5".to_string())]),
        );
    }

    #[rstest]
    fn strip_without_derivable_keys_keeps_non_derivable_entries() {
        let metadata = HashMap::from([("type_name".to_string(), "AUD/USD.SIM".to_string())]);

        let stripped = strip_derivable_metadata_keys(&metadata, "AUD/USD.SIM");

        assert_eq!(stripped, metadata);
    }

    #[rstest]
    fn record_identifier_equal_instrument_id_round_trips_via_instrument_id_restore() {
        // The Timescale record path strips with the record identifier and
        // restores via `instrument_id`; its write path asserts the map value
        // equals the identifier, so strip and restore must be exact inverses
        // for any identifier shape, including one that parses as a bar type.
        let identifier = "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL";
        let metadata = HashMap::from([
            ("instrument_id".to_string(), identifier.to_string()),
            ("custom".to_string(), "kept".to_string()),
        ]);

        let mut stripped = strip_derivable_metadata_keys(&metadata, identifier);
        assert_eq!(
            stripped,
            HashMap::from([("custom".to_string(), "kept".to_string())]),
        );

        restore_derivable_metadata_key(&mut stripped, "instrument_id", identifier);
        assert_eq!(stripped, metadata);
    }

    #[rstest]
    fn restore_inserts_derivable_keys_from_identifier() {
        let mut quote_metadata = HashMap::from([("price_precision".to_string(), "5".to_string())]);
        let mut bar_metadata = HashMap::from([("price_precision".to_string(), "5".to_string())]);

        restore_derivable_metadata_key(&mut quote_metadata, "instrument_id", "AUD/USD.SIM");
        restore_derivable_metadata_key(
            &mut bar_metadata,
            "bar_type",
            "AUD/USD.SIM-1-DAY-LAST-EXTERNAL",
        );

        assert_eq!(
            quote_metadata,
            HashMap::from([
                ("instrument_id".to_string(), "AUD/USD.SIM".to_string()),
                ("price_precision".to_string(), "5".to_string()),
            ]),
        );
        assert_eq!(
            bar_metadata,
            HashMap::from([
                (
                    "bar_type".to_string(),
                    "AUD/USD.SIM-1-DAY-LAST-EXTERNAL".to_string(),
                ),
                ("instrument_id".to_string(), "AUD/USD.SIM".to_string()),
                ("price_precision".to_string(), "5".to_string()),
            ]),
        );
    }

    #[rstest]
    fn timestamp_range_rejects_nulls() {
        let error =
            record_batch_ts_init_range(&[batch(vec![Some(20), None, Some(10)])]).unwrap_err();

        assert_eq!(error.to_string(), "ts_init column contains null values");
    }

    #[rstest]
    fn timestamp_range_reads_utc_nanoseconds() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "ts_init",
                DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
                false,
            )])),
            vec![
                Arc::new(TimestampNanosecondArray::from(vec![20, 10]).with_timezone("UTC"))
                    as ArrayRef,
            ],
        )
        .unwrap();

        assert_eq!(record_batch_ts_init_range(&[batch]).unwrap(), (10, 20),);
    }

    fn quote_metadata() -> HashMap<String, String> {
        HashMap::from([
            (KEY_TYPE_NAME.to_string(), "QuoteTick".to_string()),
            (KEY_INSTRUMENT_ID.to_string(), "AUD/USD.SIM".to_string()),
            ("price_precision".to_string(), "5".to_string()),
            ("size_precision".to_string(), "0".to_string()),
        ])
    }

    #[rstest]
    fn stored_metadata_keeps_only_precision_for_quotes() {
        let stored = stored_metadata(&quote_metadata(), Some("AUD/USD.SIM"), Some("QuoteTick"));

        assert_eq!(
            stored,
            HashMap::from([
                ("price_precision".to_string(), "5".to_string()),
                ("size_precision".to_string(), "0".to_string()),
            ])
        );
    }

    #[rstest]
    #[case::quote(
        CatalogDataType::Data(NautilusDataType::QuoteTick),
        quote_metadata(),
        "AUD/USD.SIM",
        "QuoteTick",
        Some(KEY_INSTRUMENT_ID)
    )]
    #[case::bar(
        CatalogDataType::Data(NautilusDataType::Bar),
        HashMap::from([
            (KEY_TYPE_NAME.to_string(), "Bar".to_string()),
            (KEY_BAR_TYPE.to_string(), "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL".to_string()),
            (KEY_INSTRUMENT_ID.to_string(), "AUD/USD.SIM".to_string()),
            ("price_precision".to_string(), "5".to_string()),
        ]),
        "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL",
        "Bar",
        Some(KEY_BAR_TYPE)
    )]
    #[case::instrument(
        CatalogDataType::Instrument(NautilusInstrumentType::CurrencyPair),
        HashMap::from([
            (KEY_TYPE_NAME.to_string(), "CurrencyPair".to_string()),
            (KEY_INSTRUMENT_ID.to_string(), "AUD/USD.SIM".to_string()),
        ]),
        "AUD/USD.SIM",
        "CurrencyPair",
        Some(KEY_INSTRUMENT_ID)
    )]
    #[case::account_state(
        CatalogDataType::Record(NautilusRecordType::AccountState),
        HashMap::from([
            (KEY_TYPE_NAME.to_string(), "AccountState".to_string()),
            (KEY_ACCOUNT_ID.to_string(), "SIM-001".to_string()),
        ]),
        "SIM-001",
        "AccountState",
        Some(KEY_ACCOUNT_ID)
    )]
    fn restored_metadata_inverts_stored_metadata(
        #[case] data_type: CatalogDataType,
        #[case] metadata: HashMap<String, String>,
        #[case] identifier: &str,
        #[case] location_type_name: &str,
        #[case] expected_key: Option<&str>,
    ) {
        let key = derivable_metadata_key(&data_type);

        let stored = stored_metadata(&metadata, Some(identifier), Some(location_type_name));
        let restored = restored_metadata(
            stored.clone(),
            key,
            Some(identifier),
            Some(location_type_name),
        );

        assert_eq!(key, expected_key);
        assert!(!stored.contains_key(KEY_TYPE_NAME));
        assert!(!stored.contains_key(KEY_INSTRUMENT_ID));
        assert!(!stored.contains_key(KEY_BAR_TYPE));
        assert!(!stored.contains_key(KEY_ACCOUNT_ID));
        assert_eq!(restored, metadata);
    }

    #[rstest]
    fn custom_data_keeps_its_field_precision_entries() {
        let metadata = HashMap::from([
            (KEY_TYPE_NAME.to_string(), "NewsEventData".to_string()),
            ("mid_price_precision".to_string(), "4".to_string()),
            ("mid_price_kind".to_string(), "price".to_string()),
        ]);
        let key = derivable_metadata_key(&CatalogDataType::Data(NautilusDataType::Custom {
            type_name: "NewsEventData".to_string(),
        }));

        let stored = stored_metadata(&metadata, Some("AAPL.XNAS"), Some("NewsEventData"));
        let restored = restored_metadata(
            stored.clone(),
            key,
            Some("AAPL.XNAS"),
            Some("NewsEventData"),
        );

        assert_eq!(key, None);
        assert_eq!(
            stored,
            HashMap::from([
                ("mid_price_precision".to_string(), "4".to_string()),
                ("mid_price_kind".to_string(), "price".to_string()),
            ])
        );
        assert_eq!(restored, metadata);
    }

    #[rstest]
    fn custom_data_keeps_entries_that_equal_its_identifier() {
        let bar_type = "BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL";
        let metadata = HashMap::from([
            (KEY_TYPE_NAME.to_string(), "BinanceBar".to_string()),
            (KEY_BAR_TYPE.to_string(), bar_type.to_string()),
            (KEY_INSTRUMENT_ID.to_string(), bar_type.to_string()),
            (KEY_ACCOUNT_ID.to_string(), bar_type.to_string()),
        ]);
        let data_type = CatalogDataType::Data(NautilusDataType::Custom {
            type_name: "BinanceBar".to_string(),
        });

        let stored = stored_catalog_metadata(&metadata, Some(bar_type), &data_type);

        assert_eq!(
            stored,
            HashMap::from([
                (KEY_BAR_TYPE.to_string(), bar_type.to_string()),
                (KEY_INSTRUMENT_ID.to_string(), bar_type.to_string()),
                (KEY_ACCOUNT_ID.to_string(), bar_type.to_string()),
            ])
        );
    }

    #[rstest]
    fn metadata_hash_ignores_entry_order_and_separates_precisions() {
        let first = HashMap::from([
            ("price_precision".to_string(), "5".to_string()),
            ("size_precision".to_string(), "0".to_string()),
        ]);
        let reordered = HashMap::from([
            ("size_precision".to_string(), "0".to_string()),
            ("price_precision".to_string(), "5".to_string()),
        ]);
        let other = HashMap::from([
            ("price_precision".to_string(), "3".to_string()),
            ("size_precision".to_string(), "0".to_string()),
        ]);

        assert_eq!(
            metadata_hash(&first).unwrap(),
            metadata_hash(&reordered).unwrap()
        );
        assert_ne!(
            metadata_hash(&first).unwrap(),
            metadata_hash(&other).unwrap()
        );
    }
}
