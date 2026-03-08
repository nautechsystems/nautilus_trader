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
    array::{StringBuilder, UInt64Array, UInt8Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::events::order::pending_cancel::OrderPendingCancel;

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for OrderPendingCancel {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("trader_id", DataType::Utf8, false),
            Field::new("strategy_id", DataType::Utf8, false),
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("client_order_id", DataType::Utf8, false),
            Field::new("account_id", DataType::Utf8, false),
            Field::new("event_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
            Field::new("reconciliation", DataType::UInt8, false),
            Field::new("venue_order_id", DataType::Utf8, true),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for OrderPendingCancel {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut trader_id_builder = StringBuilder::new();
        let mut strategy_id_builder = StringBuilder::new();
        let mut instrument_id_builder = StringBuilder::new();
        let mut client_order_id_builder = StringBuilder::new();
        let mut account_id_builder = StringBuilder::new();
        let mut event_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());
        let mut reconciliation_builder = UInt8Array::builder(data.len());
        let mut venue_order_id_builder = StringBuilder::new();

        for event in data {
            trader_id_builder.append_value(event.trader_id.as_str());
            strategy_id_builder.append_value(event.strategy_id.as_str());
            instrument_id_builder.append_value(event.instrument_id.to_string());
            client_order_id_builder.append_value(event.client_order_id.as_str());
            account_id_builder.append_value(event.account_id.as_str());
            event_id_builder.append_value(event.event_id.to_string());
            ts_event_builder.append_value(event.ts_event.as_u64());
            ts_init_builder.append_value(event.ts_init.as_u64());
            reconciliation_builder.append_value(event.reconciliation);

            if let Some(ref id) = event.venue_order_id {
                venue_order_id_builder.append_value(id.as_str());
            } else {
                venue_order_id_builder.append_null();
            }
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(trader_id_builder.finish()),
                Arc::new(strategy_id_builder.finish()),
                Arc::new(instrument_id_builder.finish()),
                Arc::new(client_order_id_builder.finish()),
                Arc::new(account_id_builder.finish()),
                Arc::new(event_id_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
                Arc::new(reconciliation_builder.finish()),
                Arc::new(venue_order_id_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        HashMap::from([("instrument_id".to_string(), self.instrument_id.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::events::order::pending_cancel::OrderPendingCancelBuilder;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_order_pending_cancel_schema_has_all_fields() {
        let schema = OrderPendingCancel::get_schema(None);
        let fields = schema.fields();

        assert_eq!(fields.len(), 10);

        assert_eq!(fields[0].name(), "trader_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert!(!fields[0].is_nullable());

        assert_eq!(fields[1].name(), "strategy_id");
        assert_eq!(fields[2].name(), "instrument_id");
        assert_eq!(fields[3].name(), "client_order_id");

        assert_eq!(fields[4].name(), "account_id");
        assert_eq!(fields[4].data_type(), &DataType::Utf8);
        assert!(!fields[4].is_nullable());

        assert_eq!(fields[5].name(), "event_id");

        assert_eq!(fields[6].name(), "ts_event");
        assert_eq!(fields[6].data_type(), &DataType::UInt64);

        assert_eq!(fields[7].name(), "ts_init");
        assert_eq!(fields[7].data_type(), &DataType::UInt64);

        assert_eq!(fields[8].name(), "reconciliation");
        assert_eq!(fields[8].data_type(), &DataType::UInt8);
        assert!(!fields[8].is_nullable());

        assert_eq!(fields[9].name(), "venue_order_id");
        assert!(fields[9].is_nullable());
    }

    #[rstest]
    fn test_order_pending_cancel_encode_single() {
        let event = OrderPendingCancelBuilder::default().build().unwrap();
        let metadata = event.metadata();
        let record_batch =
            OrderPendingCancel::encode_batch(&metadata, &[event]).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 10);
    }

    #[rstest]
    fn test_order_pending_cancel_encode_multiple() {
        let event1 = OrderPendingCancelBuilder::default().build().unwrap();
        let event2 = OrderPendingCancelBuilder::default()
            .reconciliation(1)
            .build()
            .unwrap();

        let data = vec![event1, event2];
        let metadata = OrderPendingCancel::chunk_metadata(&data);
        let record_batch =
            OrderPendingCancel::encode_batch(&metadata, &data).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 10);
    }
}
