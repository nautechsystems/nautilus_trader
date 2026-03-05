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
    array::{StringBuilder, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::events::position::adjusted::PositionAdjusted;

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for PositionAdjusted {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("trader_id", DataType::Utf8, false),
            Field::new("strategy_id", DataType::Utf8, false),
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("position_id", DataType::Utf8, false),
            Field::new("account_id", DataType::Utf8, false),
            Field::new("adjustment_type", DataType::Utf8, false),
            Field::new("quantity_change", DataType::Utf8, true),
            Field::new("pnl_change", DataType::Utf8, true),
            Field::new("reason", DataType::Utf8, true),
            Field::new("event_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for PositionAdjusted {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut trader_id_builder = StringBuilder::new();
        let mut strategy_id_builder = StringBuilder::new();
        let mut instrument_id_builder = StringBuilder::new();
        let mut position_id_builder = StringBuilder::new();
        let mut account_id_builder = StringBuilder::new();
        let mut adjustment_type_builder = StringBuilder::new();
        let mut quantity_change_builder = StringBuilder::new();
        let mut pnl_change_builder = StringBuilder::new();
        let mut reason_builder = StringBuilder::new();
        let mut event_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for event in data {
            trader_id_builder.append_value(event.trader_id.as_str());
            strategy_id_builder.append_value(event.strategy_id.as_str());
            instrument_id_builder.append_value(event.instrument_id.to_string());
            position_id_builder.append_value(event.position_id.as_str());
            account_id_builder.append_value(event.account_id.as_str());
            adjustment_type_builder.append_value(format!("{:?}", event.adjustment_type));

            match event.quantity_change {
                Some(ref qty) => quantity_change_builder.append_value(qty.to_string()),
                None => quantity_change_builder.append_null(),
            }

            match event.pnl_change {
                Some(ref money) => pnl_change_builder.append_value(money.to_string()),
                None => pnl_change_builder.append_null(),
            }

            match event.reason {
                Some(ref r) => reason_builder.append_value(r.as_str()),
                None => reason_builder.append_null(),
            }

            event_id_builder.append_value(event.event_id.to_string());
            ts_event_builder.append_value(event.ts_event.as_u64());
            ts_init_builder.append_value(event.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(trader_id_builder.finish()),
                Arc::new(strategy_id_builder.finish()),
                Arc::new(instrument_id_builder.finish()),
                Arc::new(position_id_builder.finish()),
                Arc::new(account_id_builder.finish()),
                Arc::new(adjustment_type_builder.finish()),
                Arc::new(quantity_change_builder.finish()),
                Arc::new(pnl_change_builder.finish()),
                Arc::new(reason_builder.finish()),
                Arc::new(event_id_builder.finish()),
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

    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::PositionAdjustmentType,
        identifiers::{AccountId, InstrumentId, PositionId, StrategyId, TraderId},
        types::{Currency, Money},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use super::*;

    fn create_test_position_adjusted() -> PositionAdjusted {
        PositionAdjusted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            PositionId::from("P-001"),
            AccountId::from("BINANCE-001"),
            PositionAdjustmentType::Commission,
            Some(Decimal::from_str("-0.001").unwrap()),
            None,
            Some(Ustr::from("O-123")),
            UUID4::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    #[rstest]
    fn test_position_adjusted_schema_has_all_fields() {
        let schema = PositionAdjusted::get_schema(None);
        let fields = schema.fields();

        assert_eq!(fields.len(), 12);

        assert_eq!(fields[0].name(), "trader_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert!(!fields[0].is_nullable());

        assert_eq!(fields[1].name(), "strategy_id");
        assert_eq!(fields[2].name(), "instrument_id");
        assert_eq!(fields[3].name(), "position_id");
        assert_eq!(fields[4].name(), "account_id");

        assert_eq!(fields[5].name(), "adjustment_type");
        assert_eq!(fields[5].data_type(), &DataType::Utf8);
        assert!(!fields[5].is_nullable());

        assert_eq!(fields[6].name(), "quantity_change");
        assert_eq!(fields[6].data_type(), &DataType::Utf8);
        assert!(fields[6].is_nullable());

        assert_eq!(fields[7].name(), "pnl_change");
        assert_eq!(fields[7].data_type(), &DataType::Utf8);
        assert!(fields[7].is_nullable());

        assert_eq!(fields[8].name(), "reason");
        assert_eq!(fields[8].data_type(), &DataType::Utf8);
        assert!(fields[8].is_nullable());

        assert_eq!(fields[9].name(), "event_id");

        assert_eq!(fields[10].name(), "ts_event");
        assert_eq!(fields[10].data_type(), &DataType::UInt64);

        assert_eq!(fields[11].name(), "ts_init");
        assert_eq!(fields[11].data_type(), &DataType::UInt64);
    }

    #[rstest]
    fn test_position_adjusted_encode_single() {
        let event = create_test_position_adjusted();
        let metadata = event.metadata();
        let record_batch =
            PositionAdjusted::encode_batch(&metadata, &[event]).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 12);
    }

    #[rstest]
    fn test_position_adjusted_encode_multiple() {
        let event1 = create_test_position_adjusted();
        let event2 = PositionAdjusted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("BTCUSD-PERP.BINANCE"),
            PositionId::from("P-002"),
            AccountId::from("BINANCE-001"),
            PositionAdjustmentType::Funding,
            None,
            Some(Money::new(-5.50, Currency::USD())),
            Some(Ustr::from("funding_2024_01_15_08:00")),
            UUID4::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        let data = vec![event1, event2];
        let metadata = PositionAdjusted::chunk_metadata(&data);
        let record_batch =
            PositionAdjusted::encode_batch(&metadata, &data).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 12);
    }
}
