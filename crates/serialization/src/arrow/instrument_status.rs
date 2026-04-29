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
use nautilus_model::data::{Data, InstrumentStatus};

use super::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, DecodeTypedFromRecordBatch,
    EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

const INSTRUMENT_STATUS_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("action", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::utf8("reason", true),
    JsonFieldSpec::utf8("trading_event", true),
    JsonFieldSpec::boolean("is_trading", true),
    JsonFieldSpec::boolean("is_quoting", true),
    JsonFieldSpec::boolean("is_short_sell_restricted", true),
];

impl ArrowSchemaProvider for InstrumentStatus {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        schema_for_type("InstrumentStatus", metadata, INSTRUMENT_STATUS_FIELDS)
    }
}

impl EncodeToRecordBatch for InstrumentStatus {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        encode_batch("InstrumentStatus", metadata, data, INSTRUMENT_STATUS_FIELDS)
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = metadata_for_type("InstrumentStatus");
        metadata.insert(
            KEY_INSTRUMENT_ID.to_string(),
            self.instrument_id.to_string(),
        );
        metadata
    }
}

impl DecodeTypedFromRecordBatch for InstrumentStatus {
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        decode_batch(
            metadata,
            &record_batch,
            INSTRUMENT_STATUS_FIELDS,
            Some("InstrumentStatus"),
        )
    }
}

impl DecodeDataFromRecordBatch for InstrumentStatus {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let items: Vec<Self> = Self::decode_typed_batch(metadata, record_batch)?;
        Ok(items.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{enums::MarketStatusAction, identifiers::InstrumentId};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string())]);

        let status1 = InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Trading,
            1_000_000_000.into(),
            1_000_000_001.into(),
            Some(Ustr::from("Normal trading")),
            Some(Ustr::from("MARKET_OPEN")),
            Some(true),
            Some(true),
            Some(false),
        );

        let status2 = InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Halt,
            2_000_000_000.into(),
            2_000_000_001.into(),
            None,
            None,
            None,
            None,
            None,
        );

        let original = vec![status1, status2];
        let record_batch = InstrumentStatus::encode_batch(&metadata, &original).unwrap();
        let decoded: Vec<Data> =
            InstrumentStatus::decode_data_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            match dec {
                Data::InstrumentStatus(s) => assert_eq!(s, orig),
                other => panic!("expected Data::InstrumentStatus, was {other:?}"),
            }
        }
    }
}
