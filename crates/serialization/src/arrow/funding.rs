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

use std::collections::HashMap;

use arrow::{datatypes::Schema, error::ArrowError, record_batch::RecordBatch};
use nautilus_model::data::FundingRateUpdate;

use super::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_INSTRUMENT_ID,
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

const FUNDING_RATE_UPDATE_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("rate", false),
    JsonFieldSpec::u64("interval", true),
    JsonFieldSpec::u64("next_funding_ns", true),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

impl ArrowSchemaProvider for FundingRateUpdate {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        schema_for_type("FundingRateUpdate", metadata, FUNDING_RATE_UPDATE_FIELDS)
    }
}

impl EncodeToRecordBatch for FundingRateUpdate {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        encode_batch(
            "FundingRateUpdate",
            metadata,
            data,
            FUNDING_RATE_UPDATE_FIELDS,
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = metadata_for_type("FundingRateUpdate");
        metadata.insert(
            KEY_INSTRUMENT_ID.to_string(),
            self.instrument_id.to_string(),
        );
        metadata
    }
}

impl DecodeTypedFromRecordBatch for FundingRateUpdate {
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        decode_batch(
            metadata,
            &record_batch,
            FUNDING_RATE_UPDATE_FIELDS,
            Some("FundingRateUpdate"),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::UnixNanos;
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_funding_rate_update_round_trip_preserves_decimal_precision() {
        let update = FundingRateUpdate::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Decimal::from_str("0.000123456789123456789").unwrap(),
            Some(480),
            Some(UnixNanos::from(9_000_000_000)),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );
        let metadata = update.metadata();
        let batch = FundingRateUpdate::encode_batch(&metadata, &[update]).unwrap();
        let decoded =
            FundingRateUpdate::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![update]);
    }

    #[rstest]
    fn test_funding_rate_update_round_trip_null_optionals() {
        let update = FundingRateUpdate::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Decimal::from_str("0.0001").unwrap(),
            None,
            None,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );
        let metadata = update.metadata();
        let batch = FundingRateUpdate::encode_batch(&metadata, &[update]).unwrap();
        let decoded =
            FundingRateUpdate::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![update]);
        assert!(decoded[0].interval.is_none());
        assert!(decoded[0].next_funding_ns.is_none());
    }
}
