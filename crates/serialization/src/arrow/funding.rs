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
use nautilus_model::data::{Data, FundingRateUpdate};

use super::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, DecodeFromRecordBatch, EncodeToRecordBatch,
    EncodingError, KEY_INSTRUMENT_ID,
    json::{
        JsonFieldSpec, decode_batch, encode_batch_with_identifier, metadata_for_type,
        schema_for_type_with_identifier,
    },
};

const FUNDING_RATE_UPDATE_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("rate", false),
    JsonFieldSpec::u64("interval", true),
    JsonFieldSpec::timestamp("next_funding_ns", true),
    JsonFieldSpec::timestamp("ts_event", false),
    JsonFieldSpec::timestamp("ts_init", false),
];

impl ArrowSchemaProvider for FundingRateUpdate {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        schema_for_type_with_identifier("FundingRateUpdate", metadata, FUNDING_RATE_UPDATE_FIELDS)
    }
}

impl EncodeToRecordBatch for FundingRateUpdate {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        encode_batch_with_identifier(
            "FundingRateUpdate",
            metadata,
            data.iter().map(std::borrow::Borrow::borrow),
            FUNDING_RATE_UPDATE_FIELDS,
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|update| update.instrument_id),
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

impl DecodeFromRecordBatch for FundingRateUpdate {
    fn decode_batch(
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

impl DecodeDataFromRecordBatch for FundingRateUpdate {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let updates = Self::decode_batch(metadata, record_batch)?;
        Ok(updates.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use arrow::array::StringArray;
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
        let identifiers = batch
            .column_by_name("identifier")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(FundingRateUpdate::get_fields()["rate"], "Utf8");
        assert_eq!(
            batch.schema().field_with_name("rate").unwrap().data_type(),
            &arrow::datatypes::DataType::Utf8
        );
        assert_eq!(identifiers.value(0), "BTCUSDT-PERP.BINANCE");
        let decoded = FundingRateUpdate::decode_batch(batch.schema().metadata(), batch).unwrap();

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
        let decoded = FundingRateUpdate::decode_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![update]);
        assert!(decoded[0].interval.is_none());
        assert!(decoded[0].next_funding_ns.is_none());
    }
}
