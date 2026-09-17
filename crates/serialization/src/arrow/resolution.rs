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

//! Arrow schema and codecs for [`MarketResolution`].
//!
//! Every field carries a typed column except the payout vector, whose length and currency vary
//! per leg. That column holds canonical JSON of the declared payouts, so an exact per-unit amount
//! survives a write and read cycle without a lossy decimal conversion. A resolution that carries
//! no payouts stores an empty string, because `pending` and `disputed` have none to store.
//!
//! Timestamps are stored once, in their own columns. The codecs never write a timestamp into the
//! payload column, so a file cannot disagree with itself about when a resolution was observed.

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{Array, StringBuilder, UInt32Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::Data,
    prediction::{MarketResolution, OutcomePayout, ResolutionOutcome, ResolutionSource},
    types::Money,
};

use super::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, DecodeFromRecordBatch, EncodeToRecordBatch,
    EncodingError, StringColumnRef, extract_column, extract_column_string,
};

/// The metadata key holding the outcome group identity of the encoded resolutions.
pub const KEY_GROUP_ID: &str = "group_id";

const FIELD_GROUP_ID: &str = "group_id";
const FIELD_VERSION: &str = "resolution_version";
const FIELD_VENUE: &str = "venue";
const FIELD_SOURCE_REFERENCE: &str = "source_reference";
const FIELD_SOURCE_URL: &str = "source_url";
const FIELD_OUTCOME_STATE: &str = "outcome_state";
const FIELD_OUTCOME: &str = "outcome";
const FIELD_EFFECTIVE_NS: &str = "effective_ns";
const FIELD_OBSERVED_NS: &str = "observed_ns";
const FIELD_TS_EVENT: &str = "ts_event";
const FIELD_TS_INIT: &str = "ts_init";

/// Encodes the payout vector into canonical JSON.
///
/// A void outcome stores its single per-unit amount; a pending or disputed outcome stores an empty
/// string; a payout outcome stores `[{"outcome_id": ..., "payout_per_unit": ...}, ...]` in the
/// order the venue declared.
fn encode_outcome(outcome: &ResolutionOutcome) -> String {
    match outcome {
        ResolutionOutcome::Payouts(payouts) => {
            let encoded: Vec<EncodedPayout<'_>> = payouts
                .iter()
                .map(|payout| EncodedPayout {
                    outcome_id: payout.outcome_id.as_str(),
                    payout_per_unit: payout.payout_per_unit.to_string(),
                })
                .collect();
            serde_json::to_string(&encoded)
                .expect("a payout vector of string fields serializes to JSON")
        }
        ResolutionOutcome::Void { payout_per_unit } => payout_per_unit.to_string(),
        ResolutionOutcome::Pending | ResolutionOutcome::Disputed => String::new(),
    }
}

fn decode_outcome(
    state: &str,
    payload: &str,
    row: usize,
) -> Result<ResolutionOutcome, EncodingError> {
    let parse_error =
        |detail: String| EncodingError::ParseError(FIELD_OUTCOME, format!("row {row}: {detail}"));

    match state {
        "payouts" => {
            let encoded: Vec<EncodedPayout<'_>> =
                serde_json::from_str(payload).map_err(|e| parse_error(e.to_string()))?;
            let mut payouts = Vec::with_capacity(encoded.len());
            for payout in encoded {
                payouts.push(OutcomePayout::new(
                    payout.outcome_id.into(),
                    payout.payout_per_unit.parse::<Money>().map_err(|e| {
                        parse_error(format!(
                            "invalid money amount {:?}: {e}",
                            payout.payout_per_unit
                        ))
                    })?,
                ));
            }
            Ok(ResolutionOutcome::Payouts(payouts))
        }
        "void" => Ok(ResolutionOutcome::Void {
            payout_per_unit: payload
                .parse::<Money>()
                .map_err(|e| parse_error(format!("invalid money amount {payload:?}: {e}")))?,
        }),
        "pending" => Ok(ResolutionOutcome::Pending),
        "disputed" => Ok(ResolutionOutcome::Disputed),
        other => Err(parse_error(format!(
            "unknown outcome state {other:?}, expected one of payouts, void, pending, disputed"
        ))),
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EncodedPayout<'a> {
    outcome_id: &'a str,
    payout_per_unit: String,
}

impl ArrowSchemaProvider for MarketResolution {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new(FIELD_GROUP_ID, DataType::Utf8, false),
            Field::new(FIELD_VERSION, DataType::UInt32, false),
            Field::new(FIELD_VENUE, DataType::Utf8, false),
            Field::new(FIELD_SOURCE_REFERENCE, DataType::Utf8, false),
            Field::new(FIELD_SOURCE_URL, DataType::Utf8, true),
            Field::new(FIELD_OUTCOME_STATE, DataType::Utf8, false),
            Field::new(FIELD_OUTCOME, DataType::Utf8, false),
            Field::new(FIELD_EFFECTIVE_NS, DataType::UInt64, false),
            Field::new(FIELD_OBSERVED_NS, DataType::UInt64, false),
            Field::new(FIELD_TS_EVENT, DataType::UInt64, false),
            Field::new(FIELD_TS_INIT, DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for MarketResolution {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut group_id_builder = StringBuilder::with_capacity(data.len(), data.len() * 32);
        let mut version_builder = UInt32Array::builder(data.len());
        let mut venue_builder = StringBuilder::with_capacity(data.len(), data.len() * 16);
        let mut reference_builder = StringBuilder::with_capacity(data.len(), data.len() * 32);
        let mut url_builder = StringBuilder::new();
        let mut state_builder = StringBuilder::with_capacity(data.len(), data.len() * 8);
        let mut outcome_builder = StringBuilder::with_capacity(data.len(), data.len() * 64);
        let mut effective_builder = UInt64Array::builder(data.len());
        let mut observed_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for item in data {
            group_id_builder.append_value(item.group_id.to_string());
            version_builder.append_value(item.version);
            venue_builder.append_value(item.source.venue);
            reference_builder.append_value(&item.source.reference);
            match item.source.url.as_deref() {
                Some(url) => url_builder.append_value(url),
                None => url_builder.append_null(),
            }
            state_builder.append_value(item.outcome.state());
            outcome_builder.append_value(encode_outcome(&item.outcome));
            effective_builder.append_value(item.effective_ns.as_u64());
            observed_builder.append_value(item.observed_ns.as_u64());
            ts_event_builder.append_value(item.ts_event.as_u64());
            ts_init_builder.append_value(item.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(group_id_builder.finish()),
                Arc::new(version_builder.finish()),
                Arc::new(venue_builder.finish()),
                Arc::new(reference_builder.finish()),
                Arc::new(url_builder.finish()),
                Arc::new(state_builder.finish()),
                Arc::new(outcome_builder.finish()),
                Arc::new(effective_builder.finish()),
                Arc::new(observed_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        HashMap::from([(KEY_GROUP_ID.to_string(), self.group_id.to_string())])
    }
}

impl DecodeFromRecordBatch for MarketResolution {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let cols = record_batch.columns();

        let group_id_values = extract_column_string(cols, FIELD_GROUP_ID, 0)?;
        let version_values =
            extract_column::<UInt32Array>(cols, FIELD_VERSION, 1, DataType::UInt32)?;
        let venue_values = extract_column_string(cols, FIELD_VENUE, 2)?;
        let reference_values = extract_column_string(cols, FIELD_SOURCE_REFERENCE, 3)?;
        let url_values = extract_column_string(cols, FIELD_SOURCE_URL, 4)?;
        let state_values = extract_column_string(cols, FIELD_OUTCOME_STATE, 5)?;
        let outcome_values = extract_column_string(cols, FIELD_OUTCOME, 6)?;
        let effective_values =
            extract_column::<UInt64Array>(cols, FIELD_EFFECTIVE_NS, 7, DataType::UInt64)?;
        let observed_values =
            extract_column::<UInt64Array>(cols, FIELD_OBSERVED_NS, 8, DataType::UInt64)?;
        let ts_event_values =
            extract_column::<UInt64Array>(cols, FIELD_TS_EVENT, 9, DataType::UInt64)?;
        let ts_init_values =
            extract_column::<UInt64Array>(cols, FIELD_TS_INIT, 10, DataType::UInt64)?;

        let expected_group_id = metadata.get(KEY_GROUP_ID);

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                let group_id = group_id_values.value(row);
                if let Some(expected) = expected_group_id
                    && group_id != expected
                {
                    return Err(EncodingError::ParseError(
                        FIELD_GROUP_ID,
                        format!(
                            "row {row}: group id {group_id} does not match the file's {expected}"
                        ),
                    ));
                }

                Ok(Self {
                    group_id: group_id.parse().map_err(|e| {
                        EncodingError::ParseError(FIELD_GROUP_ID, format!("row {row}: {e}"))
                    })?,
                    version: version_values.value(row),
                    source: ResolutionSource {
                        venue: venue_values.value(row).into(),
                        reference: reference_values.value(row).to_string(),
                        url: match &url_values {
                            StringColumnRef::Utf8(values) => {
                                (!values.is_null(row)).then(|| values.value(row).to_string())
                            }
                            StringColumnRef::Utf8View(values) => {
                                (!values.is_null(row)).then(|| values.value(row).to_string())
                            }
                        },
                    },
                    outcome: decode_outcome(
                        state_values.value(row),
                        outcome_values.value(row),
                        row,
                    )?,
                    effective_ns: effective_values.value(row).into(),
                    observed_ns: observed_values.value(row).into(),
                    ts_event: ts_event_values.value(row).into(),
                    ts_init: ts_init_values.value(row).into(),
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for MarketResolution {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let items: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(items.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use arrow::array::{StringArray, StringViewArray};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::HasTsInit,
        identifiers::OutcomeGroupId,
        types::{Currency, Money},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn group_id_for_venue(venue: &str) -> OutcomeGroupId {
        OutcomeGroupId::new_checked(venue, "0xCONDITION").unwrap()
    }

    fn group_id() -> OutcomeGroupId {
        group_id_for_venue("POLYMARKET")
    }

    fn usd(amount: &str) -> Money {
        Money::from_str(&format!("{amount} USD")).unwrap()
    }

    fn resolution_with(outcome: ResolutionOutcome, url: Option<&str>) -> MarketResolution {
        MarketResolution {
            group_id: group_id(),
            version: 3,
            source: ResolutionSource::new("POLYMARKET".into(), "0xUMA-REQUEST", url),
            outcome,
            effective_ns: UnixNanos::from(1_700_000_000_000_000_000u64),
            observed_ns: UnixNanos::from(1_700_000_060_000_000_000u64),
            ts_event: UnixNanos::from(1_700_000_000_000_000_000u64),
            ts_init: UnixNanos::from(1_700_000_060_000_000_001u64),
        }
    }

    fn payouts(amounts: &[(&str, &str)]) -> ResolutionOutcome {
        ResolutionOutcome::Payouts(
            amounts
                .iter()
                .map(|(outcome, amount)| OutcomePayout::new((*outcome).into(), usd(amount)))
                .collect(),
        )
    }

    fn payouts_in(currency: Currency, amounts: &[(&str, &str)]) -> ResolutionOutcome {
        ResolutionOutcome::Payouts(
            amounts
                .iter()
                .map(|(outcome, amount)| {
                    OutcomePayout::new(
                        (*outcome).into(),
                        Money::from_str(&format!("{amount} {currency}")).unwrap(),
                    )
                })
                .collect(),
        )
    }

    fn outcome_column(batch: &RecordBatch) -> String {
        batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("utf8 column")
            .value(0)
            .to_string()
    }

    #[rstest]
    #[case::payouts(payouts(&[("YES", "1.00"), ("NO", "0.00")]), None)]
    #[case::payouts_with_url(
        payouts(&[("YES", "1.00"), ("NO", "0.00")]),
        Some("https://example.invalid/outcome/1")
    )]
    #[case::fractional(payouts(&[("YES", "0.42"), ("NO", "0.58")]), None)]
    #[case::void(ResolutionOutcome::Void { payout_per_unit: usd("0.50") }, None)]
    #[case::pending(ResolutionOutcome::Pending, None)]
    #[case::disputed(ResolutionOutcome::Disputed, None)]
    fn test_resolution_roundtrip(#[case] outcome: ResolutionOutcome, #[case] url: Option<&str>) {
        let resolution = resolution_with(outcome, url);
        let metadata = resolution.metadata();
        let batch = MarketResolution::encode_batch(&metadata, std::slice::from_ref(&resolution))
            .expect("encodes");

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(
            batch.schema().metadata().get(KEY_GROUP_ID),
            metadata.get(KEY_GROUP_ID)
        );

        let decoded = MarketResolution::decode_batch(&metadata, batch).expect("decodes");
        assert_eq!(decoded, vec![resolution]);
    }

    #[rstest]
    fn test_decode_data_batch_wraps_resolutions() {
        let resolution = resolution_with(payouts(&[("YES", "1.00"), ("NO", "0.00")]), None);
        let metadata = resolution.metadata();
        let batch = MarketResolution::encode_batch(&metadata, std::slice::from_ref(&resolution))
            .expect("encodes");

        let decoded = MarketResolution::decode_data_batch(&metadata, batch).expect("decodes");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].ts_init(), resolution.ts_init);
    }

    #[rstest]
    fn test_decode_rejects_group_id_from_another_group() {
        let resolution = resolution_with(ResolutionOutcome::Pending, None);
        let batch = MarketResolution::encode_batch(
            &resolution.metadata(),
            std::slice::from_ref(&resolution),
        )
        .expect("encodes");

        // A file whose metadata claims another group must not decode into a resolution of this one.
        let other = HashMap::from([(
            KEY_GROUP_ID.to_string(),
            group_id_for_venue("KALSHI").to_string(),
        )]);
        let error = MarketResolution::decode_batch(&other, batch).expect_err("must reject");
        assert!(
            error.to_string().contains("does not match the file's"),
            "{error}"
        );
    }

    #[rstest]
    fn test_decode_rejects_unknown_outcome_state() {
        let resolution = resolution_with(ResolutionOutcome::Pending, None);
        let metadata = resolution.metadata();
        let batch = MarketResolution::encode_batch(&metadata, std::slice::from_ref(&resolution))
            .expect("encodes");
        let schema = MarketResolution::get_schema(Some(metadata.clone()));
        let columns = batch
            .columns()
            .iter()
            .enumerate()
            .map(|(index, column)| {
                if index == 5 {
                    Arc::new(StringArray::from(vec!["resolved"])) as Arc<dyn arrow::array::Array>
                } else {
                    column.clone()
                }
            })
            .collect::<Vec<_>>();
        let corrupted = RecordBatch::try_new(schema.into(), columns).expect("rebuilds");

        let error = MarketResolution::decode_batch(&metadata, corrupted).expect_err("must reject");
        assert!(
            error.to_string().contains("unknown outcome state"),
            "{error}"
        );
    }

    #[rstest]
    fn test_decode_rejects_malformed_money() {
        let resolution = resolution_with(payouts(&[("YES", "1.00")]), None);
        let metadata = resolution.metadata();
        let batch = MarketResolution::encode_batch(&metadata, std::slice::from_ref(&resolution))
            .expect("encodes");
        let schema = MarketResolution::get_schema(Some(metadata.clone()));
        let columns = batch
            .columns()
            .iter()
            .enumerate()
            .map(|(index, column)| {
                if index == 6 {
                    Arc::new(StringArray::from(vec![
                        r#"[{"outcome_id":"YES","payout_per_unit":"not money"}]"#,
                    ])) as Arc<dyn arrow::array::Array>
                } else {
                    column.clone()
                }
            })
            .collect::<Vec<_>>();
        let corrupted = RecordBatch::try_new(schema.into(), columns).expect("rebuilds");

        let error = MarketResolution::decode_batch(&metadata, corrupted).expect_err("must reject");
        assert!(
            error.to_string().contains("invalid money amount"),
            "{error}"
        );
    }

    #[rstest]
    fn test_payout_column_stores_the_declared_money_amount() {
        // The payout column carries the money amount exactly as the venue declared it, never a
        // scaled float.
        let resolution = resolution_with(payouts(&[("YES", "0.35"), ("NO", "0.65")]), None);
        let metadata = resolution.metadata();
        let batch = MarketResolution::encode_batch(&metadata, std::slice::from_ref(&resolution))
            .expect("encodes");

        assert_eq!(
            outcome_column(&batch),
            r#"[{"outcome_id":"YES","payout_per_unit":"0.35 USD"},{"outcome_id":"NO","payout_per_unit":"0.65 USD"}]"#,
        );

        let decoded = MarketResolution::decode_batch(&metadata, batch).expect("decodes");
        assert_eq!(decoded[0].outcome, resolution.outcome);
    }

    #[rstest]
    fn test_payout_column_keeps_full_currency_precision() {
        // USDC carries 8 decimal places, so a sub-cent payout must survive the round trip.
        let resolution =
            resolution_with(payouts_in(Currency::USDC(), &[("YES", "0.12345678")]), None);
        let metadata = resolution.metadata();
        let batch = MarketResolution::encode_batch(&metadata, std::slice::from_ref(&resolution))
            .expect("encodes");

        let outcome = outcome_column(&batch);
        assert!(outcome.contains("0.12345678"), "{outcome}");

        let decoded = MarketResolution::decode_batch(&metadata, batch).expect("decodes");
        let ResolutionOutcome::Payouts(payouts) = &decoded[0].outcome else {
            panic!("expected payouts");
        };
        assert_eq!(payouts[0].payout_per_unit.as_decimal(), dec!(0.12345678));
        assert_eq!(payouts[0].payout_per_unit.currency, Currency::USDC());
        assert_eq!(decoded[0].outcome, resolution.outcome);
    }

    #[rstest]
    fn test_decode_accepts_utf8_view_columns() {
        // Parquet hands string columns back as `Utf8View`, so a catalog read must decode too.
        let resolution = resolution_with(
            payouts(&[("YES", "1.00"), ("NO", "0.00")]),
            Some("https://example.invalid/outcome/1"),
        );
        let metadata = resolution.metadata();
        let batch = MarketResolution::encode_batch(&metadata, std::slice::from_ref(&resolution))
            .expect("encodes");

        let fields = batch
            .schema()
            .fields()
            .iter()
            .map(|field| {
                if matches!(field.data_type(), DataType::Utf8) {
                    Field::new(field.name(), DataType::Utf8View, field.is_nullable())
                } else {
                    field.as_ref().clone()
                }
            })
            .collect::<Vec<_>>();
        let columns = batch
            .columns()
            .iter()
            .map(|column| {
                if matches!(column.data_type(), DataType::Utf8) {
                    let values = column.as_any().downcast_ref::<StringArray>().expect("utf8");
                    Arc::new(StringViewArray::from_iter(values.iter())) as arrow::array::ArrayRef
                } else {
                    column.clone()
                }
            })
            .collect::<Vec<_>>();
        let view_batch = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(fields, metadata.clone())),
            columns,
        )
        .expect("rebuilds");

        let decoded = MarketResolution::decode_batch(&metadata, view_batch).expect("decodes");

        assert_eq!(decoded, vec![resolution]);
    }
}
