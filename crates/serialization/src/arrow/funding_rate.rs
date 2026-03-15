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

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{StringBuilder, UInt64Array, UInt64Builder},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::FundingRateUpdate;

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for FundingRateUpdate {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("rate", DataType::Utf8, false),
            Field::new("next_funding_ns", DataType::UInt64, true),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for FundingRateUpdate {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut instrument_id_builder = StringBuilder::new();
        let mut rate_builder = StringBuilder::new();
        let mut next_funding_ns_builder = UInt64Builder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for event in data {
            instrument_id_builder.append_value(event.instrument_id.to_string());
            rate_builder.append_value(event.rate.to_string());

            match event.next_funding_ns {
                Some(ns) => next_funding_ns_builder.append_value(ns.as_u64()),
                None => next_funding_ns_builder.append_null(),
            }

            ts_event_builder.append_value(event.ts_event.as_u64());
            ts_init_builder.append_value(event.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(instrument_id_builder.finish()),
                Arc::new(rate_builder.finish()),
                Arc::new(next_funding_ns_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        HashMap::from([("instrument_id".to_string(), self.instrument_id.to_string())])
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

    fn stub_funding_rate() -> FundingRateUpdate {
        FundingRateUpdate::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Decimal::from_str("0.0001").expect("valid decimal"),
            Some(UnixNanos::from(1_000_000_000)),
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
    }

    #[rstest]
    fn test_funding_rate_schema_has_all_fields() {
        let schema = FundingRateUpdate::get_schema(None);
        let fields = schema.fields();

        assert_eq!(fields.len(), 5);

        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert!(!fields[0].is_nullable());

        assert_eq!(fields[1].name(), "rate");
        assert_eq!(fields[1].data_type(), &DataType::Utf8);
        assert!(!fields[1].is_nullable());

        assert_eq!(fields[2].name(), "next_funding_ns");
        assert_eq!(fields[2].data_type(), &DataType::UInt64);
        assert!(fields[2].is_nullable());

        assert_eq!(fields[3].name(), "ts_event");
        assert_eq!(fields[3].data_type(), &DataType::UInt64);

        assert_eq!(fields[4].name(), "ts_init");
        assert_eq!(fields[4].data_type(), &DataType::UInt64);
    }

    #[rstest]
    fn test_funding_rate_encode_single() {
        let rate = stub_funding_rate();
        let metadata = rate.metadata();
        let record_batch = FundingRateUpdate::encode_batch(&metadata, &[rate]).unwrap();

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 5);
    }

    #[rstest]
    fn test_funding_rate_encode_multiple() {
        let rate1 = stub_funding_rate();
        let rate2 = FundingRateUpdate::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Decimal::from_str("-0.0002").expect("valid decimal"),
            None,
            UnixNanos::from(3),
            UnixNanos::from(4),
        );

        let data = vec![rate1, rate2];
        let metadata = FundingRateUpdate::chunk_metadata(&data);
        let record_batch = FundingRateUpdate::encode_batch(&metadata, &data).unwrap();

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 5);
    }
}
