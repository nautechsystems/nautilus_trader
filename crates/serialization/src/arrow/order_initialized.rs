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
use nautilus_model::{
    events::order::initialized::OrderInitialized, types::fixed::PRECISION_BYTES,
};

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for OrderInitialized {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            // Required fields
            Field::new("trader_id", DataType::Utf8, false),
            Field::new("strategy_id", DataType::Utf8, false),
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("client_order_id", DataType::Utf8, false),
            Field::new("order_side", DataType::Utf8, false),
            Field::new("order_type", DataType::Utf8, false),
            Field::new(
                "quantity",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new("time_in_force", DataType::Utf8, false),
            Field::new("post_only", DataType::Boolean, false),
            Field::new("reduce_only", DataType::Boolean, false),
            Field::new("quote_quantity", DataType::Boolean, false),
            Field::new("reconciliation", DataType::Boolean, false),
            Field::new("event_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
            // Optional fields
            Field::new(
                "price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                true,
            ),
            Field::new(
                "trigger_price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                true,
            ),
            Field::new("trigger_type", DataType::Utf8, true),
            Field::new("limit_offset", DataType::Utf8, true),
            Field::new("trailing_offset", DataType::Utf8, true),
            Field::new("trailing_offset_type", DataType::Utf8, true),
            Field::new("expire_time", DataType::UInt64, true),
            Field::new(
                "display_qty",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                true,
            ),
            Field::new("emulation_trigger", DataType::Utf8, true),
            Field::new("trigger_instrument_id", DataType::Utf8, true),
            Field::new("contingency_type", DataType::Utf8, true),
            Field::new("order_list_id", DataType::Utf8, true),
            Field::new("linked_order_ids", DataType::Utf8, true),
            Field::new("parent_order_id", DataType::Utf8, true),
            Field::new("exec_algorithm_id", DataType::Utf8, true),
            Field::new("exec_algorithm_params", DataType::Utf8, true),
            Field::new("exec_spawn_id", DataType::Utf8, true),
            Field::new("tags", DataType::Utf8, true),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for OrderInitialized {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        // Required field builders
        let mut trader_id_builder = StringBuilder::new();
        let mut strategy_id_builder = StringBuilder::new();
        let mut instrument_id_builder = StringBuilder::new();
        let mut client_order_id_builder = StringBuilder::new();
        let mut order_side_builder = StringBuilder::new();
        let mut order_type_builder = StringBuilder::new();
        let mut quantity_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut time_in_force_builder = StringBuilder::new();
        let mut post_only_builder = BooleanArray::builder(data.len());
        let mut reduce_only_builder = BooleanArray::builder(data.len());
        let mut quote_quantity_builder = BooleanArray::builder(data.len());
        let mut reconciliation_builder = BooleanArray::builder(data.len());
        let mut event_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        // Optional field builders
        let mut price_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut trigger_price_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut trigger_type_builder = StringBuilder::new();
        let mut limit_offset_builder = StringBuilder::new();
        let mut trailing_offset_builder = StringBuilder::new();
        let mut trailing_offset_type_builder = StringBuilder::new();
        let mut expire_time_builder = UInt64Array::builder(data.len());
        let mut display_qty_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut emulation_trigger_builder = StringBuilder::new();
        let mut trigger_instrument_id_builder = StringBuilder::new();
        let mut contingency_type_builder = StringBuilder::new();
        let mut order_list_id_builder = StringBuilder::new();
        let mut linked_order_ids_builder = StringBuilder::new();
        let mut parent_order_id_builder = StringBuilder::new();
        let mut exec_algorithm_id_builder = StringBuilder::new();
        let mut exec_algorithm_params_builder = StringBuilder::new();
        let mut exec_spawn_id_builder = StringBuilder::new();
        let mut tags_builder = StringBuilder::new();

        for event in data {
            // Required fields
            trader_id_builder.append_value(event.trader_id.as_str());
            strategy_id_builder.append_value(event.strategy_id.as_str());
            instrument_id_builder.append_value(event.instrument_id.to_string());
            client_order_id_builder.append_value(event.client_order_id.as_str());
            order_side_builder.append_value(format!("{:?}", event.order_side));
            order_type_builder.append_value(format!("{:?}", event.order_type));
            quantity_builder
                .append_value(event.quantity.raw.to_le_bytes())
                .unwrap();
            time_in_force_builder.append_value(format!("{:?}", event.time_in_force));
            post_only_builder.append_value(event.post_only);
            reduce_only_builder.append_value(event.reduce_only);
            quote_quantity_builder.append_value(event.quote_quantity);
            reconciliation_builder.append_value(event.reconciliation);
            event_id_builder.append_value(event.event_id.to_string());
            ts_event_builder.append_value(event.ts_event.as_u64());
            ts_init_builder.append_value(event.ts_init.as_u64());

            // Optional: price (FixedSizeBinary)
            if let Some(ref price) = event.price {
                price_builder
                    .append_value(price.raw.to_le_bytes())
                    .unwrap();
            } else {
                price_builder.append_null();
            }

            // Optional: trigger_price (FixedSizeBinary)
            if let Some(ref trigger_price) = event.trigger_price {
                trigger_price_builder
                    .append_value(trigger_price.raw.to_le_bytes())
                    .unwrap();
            } else {
                trigger_price_builder.append_null();
            }

            // Optional: trigger_type (enum -> Utf8)
            if let Some(ref trigger_type) = event.trigger_type {
                trigger_type_builder.append_value(format!("{trigger_type:?}"));
            } else {
                trigger_type_builder.append_null();
            }

            // Optional: limit_offset (Decimal -> Utf8)
            if let Some(ref limit_offset) = event.limit_offset {
                limit_offset_builder.append_value(limit_offset.to_string());
            } else {
                limit_offset_builder.append_null();
            }

            // Optional: trailing_offset (Decimal -> Utf8)
            if let Some(ref trailing_offset) = event.trailing_offset {
                trailing_offset_builder.append_value(trailing_offset.to_string());
            } else {
                trailing_offset_builder.append_null();
            }

            // Optional: trailing_offset_type (enum -> Utf8)
            if let Some(ref trailing_offset_type) = event.trailing_offset_type {
                trailing_offset_type_builder.append_value(format!("{trailing_offset_type:?}"));
            } else {
                trailing_offset_type_builder.append_null();
            }

            // Optional: expire_time (UnixNanos -> UInt64)
            if let Some(ref expire_time) = event.expire_time {
                expire_time_builder.append_value(expire_time.as_u64());
            } else {
                expire_time_builder.append_null();
            }

            // Optional: display_qty (FixedSizeBinary)
            if let Some(ref display_qty) = event.display_qty {
                display_qty_builder
                    .append_value(display_qty.raw.to_le_bytes())
                    .unwrap();
            } else {
                display_qty_builder.append_null();
            }

            // Optional: emulation_trigger (enum -> Utf8)
            if let Some(ref emulation_trigger) = event.emulation_trigger {
                emulation_trigger_builder.append_value(format!("{emulation_trigger:?}"));
            } else {
                emulation_trigger_builder.append_null();
            }

            // Optional: trigger_instrument_id (InstrumentId -> Utf8)
            if let Some(ref trigger_instrument_id) = event.trigger_instrument_id {
                trigger_instrument_id_builder
                    .append_value(trigger_instrument_id.to_string());
            } else {
                trigger_instrument_id_builder.append_null();
            }

            // Optional: contingency_type (enum -> Utf8)
            if let Some(ref contingency_type) = event.contingency_type {
                contingency_type_builder.append_value(format!("{contingency_type:?}"));
            } else {
                contingency_type_builder.append_null();
            }

            // Optional: order_list_id (OrderListId -> Utf8)
            if let Some(ref order_list_id) = event.order_list_id {
                order_list_id_builder.append_value(order_list_id.as_str());
            } else {
                order_list_id_builder.append_null();
            }

            // Optional: linked_order_ids (Vec<ClientOrderId> -> JSON Utf8)
            if let Some(ref linked_order_ids) = event.linked_order_ids {
                let json = serde_json::to_string(linked_order_ids)
                    .map_err(|e| ArrowError::ExternalError(Box::new(e)))?;
                linked_order_ids_builder.append_value(json);
            } else {
                linked_order_ids_builder.append_null();
            }

            // Optional: parent_order_id (ClientOrderId -> Utf8)
            if let Some(ref parent_order_id) = event.parent_order_id {
                parent_order_id_builder.append_value(parent_order_id.as_str());
            } else {
                parent_order_id_builder.append_null();
            }

            // Optional: exec_algorithm_id (ExecAlgorithmId -> Utf8)
            if let Some(ref exec_algorithm_id) = event.exec_algorithm_id {
                exec_algorithm_id_builder.append_value(exec_algorithm_id.as_str());
            } else {
                exec_algorithm_id_builder.append_null();
            }

            // Optional: exec_algorithm_params (IndexMap<Ustr, Ustr> -> JSON Utf8)
            if let Some(ref exec_algorithm_params) = event.exec_algorithm_params {
                let json = serde_json::to_string(exec_algorithm_params)
                    .map_err(|e| ArrowError::ExternalError(Box::new(e)))?;
                exec_algorithm_params_builder.append_value(json);
            } else {
                exec_algorithm_params_builder.append_null();
            }

            // Optional: exec_spawn_id (ClientOrderId -> Utf8)
            if let Some(ref exec_spawn_id) = event.exec_spawn_id {
                exec_spawn_id_builder.append_value(exec_spawn_id.as_str());
            } else {
                exec_spawn_id_builder.append_null();
            }

            // Optional: tags (Vec<Ustr> -> JSON Utf8)
            if let Some(ref tags) = event.tags {
                let tag_strings: Vec<&str> = tags.iter().map(|t| t.as_str()).collect();
                let json = serde_json::to_string(&tag_strings)
                    .map_err(|e| ArrowError::ExternalError(Box::new(e)))?;
                tags_builder.append_value(json);
            } else {
                tags_builder.append_null();
            }
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                // Required fields
                Arc::new(trader_id_builder.finish()),
                Arc::new(strategy_id_builder.finish()),
                Arc::new(instrument_id_builder.finish()),
                Arc::new(client_order_id_builder.finish()),
                Arc::new(order_side_builder.finish()),
                Arc::new(order_type_builder.finish()),
                Arc::new(quantity_builder.finish()),
                Arc::new(time_in_force_builder.finish()),
                Arc::new(post_only_builder.finish()),
                Arc::new(reduce_only_builder.finish()),
                Arc::new(quote_quantity_builder.finish()),
                Arc::new(reconciliation_builder.finish()),
                Arc::new(event_id_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
                // Optional fields
                Arc::new(price_builder.finish()),
                Arc::new(trigger_price_builder.finish()),
                Arc::new(trigger_type_builder.finish()),
                Arc::new(limit_offset_builder.finish()),
                Arc::new(trailing_offset_builder.finish()),
                Arc::new(trailing_offset_type_builder.finish()),
                Arc::new(expire_time_builder.finish()),
                Arc::new(display_qty_builder.finish()),
                Arc::new(emulation_trigger_builder.finish()),
                Arc::new(trigger_instrument_id_builder.finish()),
                Arc::new(contingency_type_builder.finish()),
                Arc::new(order_list_id_builder.finish()),
                Arc::new(linked_order_ids_builder.finish()),
                Arc::new(parent_order_id_builder.finish()),
                Arc::new(exec_algorithm_id_builder.finish()),
                Arc::new(exec_algorithm_params_builder.finish()),
                Arc::new(exec_spawn_id_builder.finish()),
                Arc::new(tags_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        HashMap::from([("instrument_id".to_string(), self.instrument_id.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::events::order::initialized::OrderInitializedBuilder;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_order_initialized_schema_has_all_fields() {
        let schema = OrderInitialized::get_schema(None);
        let fields = schema.fields();

        // 15 required + 18 optional = 33 fields
        assert_eq!(fields.len(), 33);

        // Required fields
        assert_eq!(fields[0].name(), "trader_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert!(!fields[0].is_nullable());

        assert_eq!(fields[1].name(), "strategy_id");
        assert_eq!(fields[2].name(), "instrument_id");
        assert_eq!(fields[3].name(), "client_order_id");
        assert_eq!(fields[4].name(), "order_side");
        assert_eq!(fields[5].name(), "order_type");

        assert_eq!(fields[6].name(), "quantity");
        assert_eq!(
            fields[6].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );
        assert!(!fields[6].is_nullable());

        assert_eq!(fields[7].name(), "time_in_force");

        assert_eq!(fields[8].name(), "post_only");
        assert_eq!(fields[8].data_type(), &DataType::Boolean);
        assert!(!fields[8].is_nullable());

        assert_eq!(fields[9].name(), "reduce_only");
        assert_eq!(fields[9].data_type(), &DataType::Boolean);

        assert_eq!(fields[10].name(), "quote_quantity");
        assert_eq!(fields[10].data_type(), &DataType::Boolean);

        assert_eq!(fields[11].name(), "reconciliation");
        assert_eq!(fields[11].data_type(), &DataType::Boolean);
        assert!(!fields[11].is_nullable());

        assert_eq!(fields[12].name(), "event_id");

        assert_eq!(fields[13].name(), "ts_event");
        assert_eq!(fields[13].data_type(), &DataType::UInt64);

        assert_eq!(fields[14].name(), "ts_init");
        assert_eq!(fields[14].data_type(), &DataType::UInt64);

        // Optional fields
        assert_eq!(fields[15].name(), "price");
        assert_eq!(
            fields[15].data_type(),
            &DataType::FixedSizeBinary(PRECISION_BYTES)
        );
        assert!(fields[15].is_nullable());

        assert_eq!(fields[16].name(), "trigger_price");
        assert!(fields[16].is_nullable());

        assert_eq!(fields[17].name(), "trigger_type");
        assert!(fields[17].is_nullable());

        assert_eq!(fields[18].name(), "limit_offset");
        assert!(fields[18].is_nullable());

        assert_eq!(fields[19].name(), "trailing_offset");
        assert!(fields[19].is_nullable());

        assert_eq!(fields[20].name(), "trailing_offset_type");
        assert!(fields[20].is_nullable());

        assert_eq!(fields[21].name(), "expire_time");
        assert_eq!(fields[21].data_type(), &DataType::UInt64);
        assert!(fields[21].is_nullable());

        assert_eq!(fields[22].name(), "display_qty");
        assert!(fields[22].is_nullable());

        assert_eq!(fields[23].name(), "emulation_trigger");
        assert!(fields[23].is_nullable());

        assert_eq!(fields[24].name(), "trigger_instrument_id");
        assert!(fields[24].is_nullable());

        assert_eq!(fields[25].name(), "contingency_type");
        assert!(fields[25].is_nullable());

        assert_eq!(fields[26].name(), "order_list_id");
        assert!(fields[26].is_nullable());

        assert_eq!(fields[27].name(), "linked_order_ids");
        assert!(fields[27].is_nullable());

        assert_eq!(fields[28].name(), "parent_order_id");
        assert!(fields[28].is_nullable());

        assert_eq!(fields[29].name(), "exec_algorithm_id");
        assert!(fields[29].is_nullable());

        assert_eq!(fields[30].name(), "exec_algorithm_params");
        assert!(fields[30].is_nullable());

        assert_eq!(fields[31].name(), "exec_spawn_id");
        assert!(fields[31].is_nullable());

        assert_eq!(fields[32].name(), "tags");
        assert!(fields[32].is_nullable());
    }

    #[rstest]
    fn test_order_initialized_encode_single() {
        let event = OrderInitializedBuilder::default().build().unwrap();
        let metadata = event.metadata();
        let record_batch =
            OrderInitialized::encode_batch(&metadata, &[event]).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 33);
    }

    #[rstest]
    fn test_order_initialized_encode_multiple() {
        let event1 = OrderInitializedBuilder::default().build().unwrap();
        let event2 = OrderInitializedBuilder::default()
            .reconciliation(true)
            .build()
            .unwrap();

        let data = vec![event1, event2];
        let metadata = OrderInitialized::chunk_metadata(&data);
        let record_batch =
            OrderInitialized::encode_batch(&metadata, &data).expect("encode failed");

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 33);
    }
}
