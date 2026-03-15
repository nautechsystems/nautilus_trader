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
    array::{
        FixedSizeBinaryBuilder, Float64Array, Float64Builder, StringBuilder, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{events::PositionChanged, types::fixed::PRECISION_BYTES};

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for PositionChanged {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("trader_id", DataType::Utf8, false),
            Field::new("strategy_id", DataType::Utf8, false),
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("position_id", DataType::Utf8, false),
            Field::new("account_id", DataType::Utf8, false),
            Field::new("opening_order_id", DataType::Utf8, false),
            Field::new("entry", DataType::Utf8, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("signed_qty", DataType::Float64, false),
            Field::new(
                "quantity",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new(
                "peak_quantity",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new(
                "last_qty",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new(
                "last_px",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new("currency", DataType::Utf8, false),
            Field::new("avg_px_open", DataType::Float64, false),
            Field::new("avg_px_close", DataType::Float64, true),
            Field::new("realized_return", DataType::Float64, false),
            Field::new("realized_pnl", DataType::Utf8, true),
            Field::new("unrealized_pnl", DataType::Utf8, false),
            Field::new("event_id", DataType::Utf8, false),
            Field::new("ts_opened", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for PositionChanged {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut trader_id_builder = StringBuilder::new();
        let mut strategy_id_builder = StringBuilder::new();
        let mut instrument_id_builder = StringBuilder::new();
        let mut position_id_builder = StringBuilder::new();
        let mut account_id_builder = StringBuilder::new();
        let mut opening_order_id_builder = StringBuilder::new();
        let mut entry_builder = StringBuilder::new();
        let mut side_builder = StringBuilder::new();
        let mut signed_qty_builder = Float64Array::builder(data.len());
        let mut quantity_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut peak_quantity_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut last_qty_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut last_px_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut currency_builder = StringBuilder::new();
        let mut avg_px_open_builder = Float64Array::builder(data.len());
        let mut avg_px_close_builder = Float64Builder::with_capacity(data.len());
        let mut realized_return_builder = Float64Array::builder(data.len());
        let mut realized_pnl_builder = StringBuilder::new();
        let mut unrealized_pnl_builder = StringBuilder::new();
        let mut event_id_builder = StringBuilder::new();
        let mut ts_opened_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for event in data {
            trader_id_builder.append_value(event.trader_id.as_str());
            strategy_id_builder.append_value(event.strategy_id.as_str());
            instrument_id_builder.append_value(event.instrument_id.to_string());
            position_id_builder.append_value(event.position_id.as_str());
            account_id_builder.append_value(event.account_id.as_str());
            opening_order_id_builder.append_value(event.opening_order_id.as_str());
            entry_builder.append_value(format!("{:?}", event.entry));
            side_builder.append_value(format!("{:?}", event.side));
            signed_qty_builder.append_value(event.signed_qty);
            quantity_builder
                .append_value(event.quantity.raw.to_le_bytes())
                .unwrap();
            peak_quantity_builder
                .append_value(event.peak_quantity.raw.to_le_bytes())
                .unwrap();
            last_qty_builder
                .append_value(event.last_qty.raw.to_le_bytes())
                .unwrap();
            last_px_builder
                .append_value(event.last_px.raw.to_le_bytes())
                .unwrap();
            currency_builder.append_value(event.currency.code.as_str());
            avg_px_open_builder.append_value(event.avg_px_open);

            match event.avg_px_close {
                Some(v) => avg_px_close_builder.append_value(v),
                None => avg_px_close_builder.append_null(),
            }

            realized_return_builder.append_value(event.realized_return);

            match event.realized_pnl {
                Some(ref money) => realized_pnl_builder.append_value(money.to_string()),
                None => realized_pnl_builder.append_null(),
            }

            unrealized_pnl_builder.append_value(event.unrealized_pnl.to_string());
            event_id_builder.append_value(event.event_id.to_string());
            ts_opened_builder.append_value(event.ts_opened.as_u64());
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
                Arc::new(opening_order_id_builder.finish()),
                Arc::new(entry_builder.finish()),
                Arc::new(side_builder.finish()),
                Arc::new(signed_qty_builder.finish()),
                Arc::new(quantity_builder.finish()),
                Arc::new(peak_quantity_builder.finish()),
                Arc::new(last_qty_builder.finish()),
                Arc::new(last_px_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(avg_px_open_builder.finish()),
                Arc::new(avg_px_close_builder.finish()),
                Arc::new(realized_return_builder.finish()),
                Arc::new(realized_pnl_builder.finish()),
                Arc::new(unrealized_pnl_builder.finish()),
                Arc::new(event_id_builder.finish()),
                Arc::new(ts_opened_builder.finish()),
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
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, PositionSide},
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn create_test_position_changed() -> PositionChanged {
        PositionChanged {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 150.0,
            quantity: Quantity::from("150"),
            peak_quantity: Quantity::from("150"),
            last_qty: Quantity::from("50"),
            last_px: Price::from("1.0550"),
            currency: Currency::USD(),
            avg_px_open: 1.0525,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            unrealized_pnl: Money::new(75.0, Currency::USD()),
            event_id: UUID4::default(),
            ts_opened: UnixNanos::from(1_000_000_000),
            ts_event: UnixNanos::from(1_500_000_000),
            ts_init: UnixNanos::from(2_500_000_000),
        }
    }

    #[rstest]
    fn test_position_changed_schema_has_all_fields() {
        let schema = PositionChanged::get_schema(None);
        let fields = schema.fields();

        assert_eq!(fields.len(), 23);

        assert_eq!(fields[0].name(), "trader_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert!(!fields[0].is_nullable());

        assert_eq!(fields[1].name(), "strategy_id");
        assert_eq!(fields[2].name(), "instrument_id");
        assert_eq!(fields[3].name(), "position_id");
        assert_eq!(fields[4].name(), "account_id");
        assert_eq!(fields[5].name(), "opening_order_id");
        assert_eq!(fields[6].name(), "entry");
        assert_eq!(fields[7].name(), "side");

        assert_eq!(fields[8].name(), "signed_qty");
        assert_eq!(fields[8].data_type(), &DataType::Float64);

        assert_eq!(fields[9].name(), "quantity");
        assert_eq!(
            fields[9].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );

        assert_eq!(fields[10].name(), "peak_quantity");
        assert_eq!(
            fields[10].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );

        assert_eq!(fields[11].name(), "last_qty");
        assert_eq!(fields[12].name(), "last_px");

        assert_eq!(fields[13].name(), "currency");
        assert_eq!(fields[14].name(), "avg_px_open");
        assert_eq!(fields[14].data_type(), &DataType::Float64);
        assert!(!fields[14].is_nullable());

        assert_eq!(fields[15].name(), "avg_px_close");
        assert_eq!(fields[15].data_type(), &DataType::Float64);
        assert!(fields[15].is_nullable());

        assert_eq!(fields[16].name(), "realized_return");
        assert_eq!(fields[16].data_type(), &DataType::Float64);

        assert_eq!(fields[17].name(), "realized_pnl");
        assert_eq!(fields[17].data_type(), &DataType::Utf8);
        assert!(fields[17].is_nullable());

        assert_eq!(fields[18].name(), "unrealized_pnl");
        assert_eq!(fields[18].data_type(), &DataType::Utf8);
        assert!(!fields[18].is_nullable());

        assert_eq!(fields[19].name(), "event_id");

        assert_eq!(fields[20].name(), "ts_opened");
        assert_eq!(fields[20].data_type(), &DataType::UInt64);

        assert_eq!(fields[21].name(), "ts_event");
        assert_eq!(fields[21].data_type(), &DataType::UInt64);

        assert_eq!(fields[22].name(), "ts_init");
        assert_eq!(fields[22].data_type(), &DataType::UInt64);
    }

    #[rstest]
    fn test_position_changed_encode_single() {
        let event = create_test_position_changed();
        let metadata = event.metadata();
        let record_batch =
            PositionChanged::encode_batch(&metadata, &[event]).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 23);
    }

    #[rstest]
    fn test_position_changed_encode_multiple() {
        let event1 = create_test_position_changed();
        let mut event2 = create_test_position_changed();
        event2.avg_px_close = Some(1.0575);
        event2.realized_return = 0.0048;
        event2.realized_pnl = Some(Money::new(25.0, Currency::USD()));

        let data = vec![event1, event2];
        let metadata = PositionChanged::chunk_metadata(&data);
        let record_batch =
            PositionChanged::encode_batch(&metadata, &data).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 23);
    }
}
