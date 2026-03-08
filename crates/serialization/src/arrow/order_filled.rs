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
    array::{BooleanArray, FixedSizeBinaryBuilder, StringBuilder, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{events::OrderFilled, types::fixed::PRECISION_BYTES};

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for OrderFilled {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("trader_id", DataType::Utf8, false),
            Field::new("strategy_id", DataType::Utf8, false),
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("client_order_id", DataType::Utf8, false),
            Field::new("venue_order_id", DataType::Utf8, false),
            Field::new("account_id", DataType::Utf8, false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("order_side", DataType::Utf8, false),
            Field::new("order_type", DataType::Utf8, false),
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
            Field::new("liquidity_side", DataType::Utf8, false),
            Field::new("event_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
            Field::new("reconciliation", DataType::Boolean, false),
            Field::new("position_id", DataType::Utf8, true),
            Field::new("commission", DataType::Utf8, true),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for OrderFilled {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut trader_id_builder = StringBuilder::new();
        let mut strategy_id_builder = StringBuilder::new();
        let mut instrument_id_builder = StringBuilder::new();
        let mut client_order_id_builder = StringBuilder::new();
        let mut venue_order_id_builder = StringBuilder::new();
        let mut account_id_builder = StringBuilder::new();
        let mut trade_id_builder = StringBuilder::new();
        let mut order_side_builder = StringBuilder::new();
        let mut order_type_builder = StringBuilder::new();
        let mut last_qty_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut last_px_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut currency_builder = StringBuilder::new();
        let mut liquidity_side_builder = StringBuilder::new();
        let mut event_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());
        let mut reconciliation_builder = BooleanArray::builder(data.len());
        let mut position_id_builder = StringBuilder::new();
        let mut commission_builder = StringBuilder::new();

        for fill in data {
            trader_id_builder.append_value(fill.trader_id.as_str());
            strategy_id_builder.append_value(fill.strategy_id.as_str());
            instrument_id_builder.append_value(fill.instrument_id.to_string());
            client_order_id_builder.append_value(fill.client_order_id.as_str());
            venue_order_id_builder.append_value(fill.venue_order_id.as_str());
            account_id_builder.append_value(fill.account_id.as_str());
            trade_id_builder.append_value(fill.trade_id.as_str());
            order_side_builder.append_value(format!("{:?}", fill.order_side));
            order_type_builder.append_value(format!("{:?}", fill.order_type));
            last_qty_builder
                .append_value(fill.last_qty.raw.to_le_bytes())
                .unwrap();
            last_px_builder
                .append_value(fill.last_px.raw.to_le_bytes())
                .unwrap();
            currency_builder.append_value(fill.currency.code.as_str());
            liquidity_side_builder.append_value(format!("{:?}", fill.liquidity_side));
            event_id_builder.append_value(fill.event_id.to_string());
            ts_event_builder.append_value(fill.ts_event.as_u64());
            ts_init_builder.append_value(fill.ts_init.as_u64());
            reconciliation_builder.append_value(fill.reconciliation);

            if let Some(ref id) = fill.position_id {
                position_id_builder.append_value(id.as_str());
            } else {
                position_id_builder.append_null();
            }

            if let Some(ref money) = fill.commission {
                commission_builder.append_value(money.to_string());
            } else {
                commission_builder.append_null();
            }
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(trader_id_builder.finish()),
                Arc::new(strategy_id_builder.finish()),
                Arc::new(instrument_id_builder.finish()),
                Arc::new(client_order_id_builder.finish()),
                Arc::new(venue_order_id_builder.finish()),
                Arc::new(account_id_builder.finish()),
                Arc::new(trade_id_builder.finish()),
                Arc::new(order_side_builder.finish()),
                Arc::new(order_type_builder.finish()),
                Arc::new(last_qty_builder.finish()),
                Arc::new(last_px_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(liquidity_side_builder.finish()),
                Arc::new(event_id_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
                Arc::new(reconciliation_builder.finish()),
                Arc::new(position_id_builder.finish()),
                Arc::new(commission_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        HashMap::from([("instrument_id".to_string(), self.instrument_id.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::events::order::filled::OrderFilledBuilder;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_order_filled_schema_has_all_fields() {
        let schema = OrderFilled::get_schema(None);
        let fields = schema.fields();

        assert_eq!(fields.len(), 19);

        assert_eq!(fields[0].name(), "trader_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert!(!fields[0].is_nullable());

        assert_eq!(fields[1].name(), "strategy_id");
        assert_eq!(fields[2].name(), "instrument_id");
        assert_eq!(fields[3].name(), "client_order_id");
        assert_eq!(fields[4].name(), "venue_order_id");
        assert_eq!(fields[5].name(), "account_id");
        assert_eq!(fields[6].name(), "trade_id");
        assert_eq!(fields[7].name(), "order_side");
        assert_eq!(fields[8].name(), "order_type");

        assert_eq!(fields[9].name(), "last_qty");
        assert_eq!(
            fields[9].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );

        assert_eq!(fields[10].name(), "last_px");
        assert_eq!(
            fields[10].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );

        assert_eq!(fields[11].name(), "currency");
        assert_eq!(fields[12].name(), "liquidity_side");
        assert_eq!(fields[13].name(), "event_id");

        assert_eq!(fields[14].name(), "ts_event");
        assert_eq!(fields[14].data_type(), &DataType::UInt64);

        assert_eq!(fields[15].name(), "ts_init");
        assert_eq!(fields[15].data_type(), &DataType::UInt64);

        assert_eq!(fields[16].name(), "reconciliation");
        assert_eq!(fields[16].data_type(), &DataType::Boolean);
        assert!(!fields[16].is_nullable());

        assert_eq!(fields[17].name(), "position_id");
        assert!(fields[17].is_nullable());

        assert_eq!(fields[18].name(), "commission");
        assert!(fields[18].is_nullable());
    }

    #[rstest]
    fn test_order_filled_encode_single() {
        let fill = OrderFilledBuilder::default().build().unwrap();
        let metadata = fill.metadata();
        let record_batch = OrderFilled::encode_batch(&metadata, &[fill]).unwrap();

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 19);
    }

    #[rstest]
    fn test_order_filled_encode_multiple() {
        let fill1 = OrderFilledBuilder::default().build().unwrap();
        let fill2 = OrderFilledBuilder::default()
            .reconciliation(true)
            .build()
            .unwrap();

        let data = vec![fill1, fill2];
        let metadata = OrderFilled::chunk_metadata(&data);
        let record_batch = OrderFilled::encode_batch(&metadata, &data).unwrap();

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 19);
    }
}
