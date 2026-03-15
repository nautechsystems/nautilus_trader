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
    array::{FixedSizeBinaryBuilder, Float64Array, StringBuilder, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{events::PositionOpened, types::fixed::PRECISION_BYTES};

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for PositionOpened {
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

impl EncodeToRecordBatch for PositionOpened {
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
        let mut last_qty_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut last_px_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut currency_builder = StringBuilder::new();
        let mut avg_px_open_builder = Float64Array::builder(data.len());
        let mut event_id_builder = StringBuilder::new();
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
            last_qty_builder
                .append_value(event.last_qty.raw.to_le_bytes())
                .unwrap();
            last_px_builder
                .append_value(event.last_px.raw.to_le_bytes())
                .unwrap();
            currency_builder.append_value(event.currency.code.as_str());
            avg_px_open_builder.append_value(event.avg_px_open);
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
                Arc::new(opening_order_id_builder.finish()),
                Arc::new(entry_builder.finish()),
                Arc::new(side_builder.finish()),
                Arc::new(signed_qty_builder.finish()),
                Arc::new(quantity_builder.finish()),
                Arc::new(last_qty_builder.finish()),
                Arc::new(last_px_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(avg_px_open_builder.finish()),
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
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, PositionSide},
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn create_test_position_opened() -> PositionOpened {
        PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 100.0,
            quantity: Quantity::from("100"),
            last_qty: Quantity::from("100"),
            last_px: Price::from("1.0500"),
            currency: Currency::USD(),
            avg_px_open: 1.0500,
            event_id: UUID4::default(),
            ts_event: UnixNanos::from(1_000_000_000),
            ts_init: UnixNanos::from(2_000_000_000),
        }
    }

    #[rstest]
    fn test_position_opened_schema_has_all_fields() {
        let schema = PositionOpened::get_schema(None);
        let fields = schema.fields();

        assert_eq!(fields.len(), 17);

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

        assert_eq!(fields[10].name(), "last_qty");
        assert_eq!(
            fields[10].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );

        assert_eq!(fields[11].name(), "last_px");
        assert_eq!(
            fields[11].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );

        assert_eq!(fields[12].name(), "currency");
        assert_eq!(fields[12].data_type(), &DataType::Utf8);

        assert_eq!(fields[13].name(), "avg_px_open");
        assert_eq!(fields[13].data_type(), &DataType::Float64);

        assert_eq!(fields[14].name(), "event_id");
        assert_eq!(fields[14].data_type(), &DataType::Utf8);

        assert_eq!(fields[15].name(), "ts_event");
        assert_eq!(fields[15].data_type(), &DataType::UInt64);

        assert_eq!(fields[16].name(), "ts_init");
        assert_eq!(fields[16].data_type(), &DataType::UInt64);
    }

    #[rstest]
    fn test_position_opened_encode_single() {
        let event = create_test_position_opened();
        let metadata = event.metadata();
        let record_batch =
            PositionOpened::encode_batch(&metadata, &[event]).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 17);
    }

    #[rstest]
    fn test_position_opened_encode_multiple() {
        let event1 = create_test_position_opened();
        let mut event2 = create_test_position_opened();
        event2.signed_qty = -100.0;
        event2.side = PositionSide::Short;
        event2.entry = OrderSide::Sell;

        let data = vec![event1, event2];
        let metadata = PositionOpened::chunk_metadata(&data);
        let record_batch =
            PositionOpened::encode_batch(&metadata, &data).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 17);
    }
}
