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
use nautilus_model::events::{
    OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated, OrderExpired,
    OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate,
    OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered, OrderUpdated,
};

use super::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_INSTRUMENT_ID,
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

const ORDER_INITIALIZED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("order_side", false),
    JsonFieldSpec::utf8("order_type", false),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("time_in_force", false),
    JsonFieldSpec::boolean("post_only", false),
    JsonFieldSpec::boolean("reduce_only", false),
    JsonFieldSpec::boolean("quote_quantity", false),
    JsonFieldSpec::boolean("reconciliation", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::utf8("price", true),
    JsonFieldSpec::utf8("trigger_price", true),
    JsonFieldSpec::utf8("trigger_type", true),
    JsonFieldSpec::utf8("limit_offset", true),
    JsonFieldSpec::utf8("trailing_offset", true),
    JsonFieldSpec::utf8("trailing_offset_type", true),
    JsonFieldSpec::u64("expire_time", true),
    JsonFieldSpec::utf8("display_qty", true),
    JsonFieldSpec::utf8("emulation_trigger", true),
    JsonFieldSpec::utf8("trigger_instrument_id", true),
    JsonFieldSpec::utf8("contingency_type", true),
    JsonFieldSpec::utf8("order_list_id", true),
    JsonFieldSpec::utf8_json("linked_order_ids", true),
    JsonFieldSpec::utf8("parent_order_id", true),
    JsonFieldSpec::utf8("exec_algorithm_id", true),
    JsonFieldSpec::utf8_json("exec_algorithm_params", true),
    JsonFieldSpec::utf8("exec_spawn_id", true),
    JsonFieldSpec::utf8_json("tags", true),
];

const ORDER_DENIED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("reason", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

const ORDER_EMULATED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

const ORDER_SUBMITTED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

const ORDER_ACCEPTED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("venue_order_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
];

const ORDER_REJECTED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("reason", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::u64("due_post_only", false),
];

const ORDER_PENDING_CANCEL_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::utf8("venue_order_id", true),
];

const ORDER_CANCELED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::utf8("venue_order_id", true),
    JsonFieldSpec::utf8("account_id", true),
];

const ORDER_CANCEL_REJECTED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("reason", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::utf8("venue_order_id", true),
    JsonFieldSpec::utf8("account_id", true),
];

const ORDER_EXPIRED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::utf8("venue_order_id", true),
    JsonFieldSpec::utf8("account_id", true),
];

const ORDER_TRIGGERED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::utf8("venue_order_id", true),
    JsonFieldSpec::utf8("account_id", true),
];

const ORDER_PENDING_UPDATE_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::utf8("venue_order_id", true),
];

const ORDER_RELEASED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("released_price", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

const ORDER_MODIFY_REJECTED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("reason", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
    JsonFieldSpec::utf8("venue_order_id", true),
    JsonFieldSpec::utf8("account_id", true),
];

const ORDER_UPDATED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("venue_order_id", true),
    JsonFieldSpec::utf8("account_id", true),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("price", true),
    JsonFieldSpec::utf8("trigger_price", true),
    JsonFieldSpec::utf8("protection_price", true),
    JsonFieldSpec::boolean("is_quote_quantity", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("reconciliation", false),
];

const ORDER_FILLED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("venue_order_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("trade_id", false),
    JsonFieldSpec::utf8("order_side", false),
    JsonFieldSpec::utf8("order_type", false),
    JsonFieldSpec::utf8("last_qty", false),
    JsonFieldSpec::utf8("last_px", false),
    JsonFieldSpec::utf8("currency", false),
    JsonFieldSpec::utf8("liquidity_side", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::boolean("reconciliation", false),
    JsonFieldSpec::utf8("position_id", true),
    JsonFieldSpec::utf8("commission", true),
];

fn instrument_metadata(type_name: &'static str, instrument_id: &str) -> HashMap<String, String> {
    let mut metadata = metadata_for_type(type_name);
    metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string());
    metadata
}

macro_rules! impl_order_event_arrow {
    ($type:ty, $type_name:expr, $fields:expr) => {
        impl ArrowSchemaProvider for $type {
            fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
                schema_for_type($type_name, metadata, $fields)
            }
        }

        impl EncodeToRecordBatch for $type {
            fn encode_batch(
                metadata: &HashMap<String, String>,
                data: &[Self],
            ) -> Result<RecordBatch, ArrowError> {
                encode_batch($type_name, metadata, data, $fields)
            }

            fn metadata(&self) -> HashMap<String, String> {
                instrument_metadata($type_name, &self.instrument_id.to_string())
            }
        }

        impl DecodeTypedFromRecordBatch for $type {
            fn decode_typed_batch(
                metadata: &HashMap<String, String>,
                record_batch: RecordBatch,
            ) -> Result<Vec<Self>, EncodingError> {
                decode_batch(metadata, &record_batch, $fields, Some($type_name))
            }
        }
    };
}

impl_order_event_arrow!(
    OrderInitialized,
    "OrderInitialized",
    ORDER_INITIALIZED_FIELDS
);
impl_order_event_arrow!(OrderDenied, "OrderDenied", ORDER_DENIED_FIELDS);
impl_order_event_arrow!(OrderEmulated, "OrderEmulated", ORDER_EMULATED_FIELDS);
impl_order_event_arrow!(OrderSubmitted, "OrderSubmitted", ORDER_SUBMITTED_FIELDS);
impl_order_event_arrow!(OrderAccepted, "OrderAccepted", ORDER_ACCEPTED_FIELDS);
impl_order_event_arrow!(OrderRejected, "OrderRejected", ORDER_REJECTED_FIELDS);
impl_order_event_arrow!(
    OrderPendingCancel,
    "OrderPendingCancel",
    ORDER_PENDING_CANCEL_FIELDS
);
impl_order_event_arrow!(OrderCanceled, "OrderCanceled", ORDER_CANCELED_FIELDS);
impl_order_event_arrow!(
    OrderCancelRejected,
    "OrderCancelRejected",
    ORDER_CANCEL_REJECTED_FIELDS
);
impl_order_event_arrow!(OrderExpired, "OrderExpired", ORDER_EXPIRED_FIELDS);
impl_order_event_arrow!(OrderTriggered, "OrderTriggered", ORDER_TRIGGERED_FIELDS);
impl_order_event_arrow!(
    OrderPendingUpdate,
    "OrderPendingUpdate",
    ORDER_PENDING_UPDATE_FIELDS
);
impl_order_event_arrow!(OrderReleased, "OrderReleased", ORDER_RELEASED_FIELDS);
impl_order_event_arrow!(
    OrderModifyRejected,
    "OrderModifyRejected",
    ORDER_MODIFY_REJECTED_FIELDS
);
impl_order_event_arrow!(OrderUpdated, "OrderUpdated", ORDER_UPDATED_FIELDS);
impl_order_event_arrow!(OrderFilled, "OrderFilled", ORDER_FILLED_FIELDS);

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::events::order::stubs::{
        order_accepted, order_cancel_rejected, order_denied_max_submitted_rate, order_emulated,
        order_expired, order_filled, order_initialized_buy_limit, order_modify_rejected,
        order_pending_cancel, order_pending_update, order_rejected_insufficient_margin,
        order_released, order_submitted, order_triggered, order_updated,
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_order_initialized_round_trip(order_initialized_buy_limit: OrderInitialized) {
        let event = OrderInitialized {
            limit_offset: Some(Decimal::from_str("0.123456789123456789").unwrap()),
            trailing_offset: Some(Decimal::from_str("0.987654321987654321").unwrap()),
            ..order_initialized_buy_limit
        };
        let metadata = event.metadata();
        let batch =
            OrderInitialized::encode_batch(&metadata, std::slice::from_ref(&event)).unwrap();
        let decoded =
            OrderInitialized::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![event]);
    }

    #[rstest]
    fn test_order_filled_round_trip(order_filled: OrderFilled) {
        let event = order_filled;
        let metadata = event.metadata();
        let batch = OrderFilled::encode_batch(&metadata, &[event]).unwrap();
        let decoded = OrderFilled::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![event]);
    }

    fn roundtrip<T>(event: T)
    where
        T: ArrowSchemaProvider
            + EncodeToRecordBatch
            + DecodeTypedFromRecordBatch
            + Clone
            + PartialEq
            + std::fmt::Debug,
    {
        let metadata = event.metadata();
        let batch = T::encode_batch(&metadata, std::slice::from_ref(&event)).unwrap();
        let decoded = T::decode_typed_batch(batch.schema().metadata(), batch).unwrap();
        assert_eq!(decoded, vec![event]);
    }

    #[rstest]
    fn test_order_denied_round_trip(order_denied_max_submitted_rate: OrderDenied) {
        roundtrip(order_denied_max_submitted_rate);
    }

    #[rstest]
    fn test_order_submitted_round_trip(order_submitted: OrderSubmitted) {
        roundtrip(order_submitted);
    }

    #[rstest]
    fn test_order_accepted_round_trip(order_accepted: OrderAccepted) {
        roundtrip(order_accepted);
    }

    #[rstest]
    fn test_order_rejected_round_trip(order_rejected_insufficient_margin: OrderRejected) {
        roundtrip(order_rejected_insufficient_margin);
    }

    #[rstest]
    fn test_order_canceled_round_trip() {
        use nautilus_model::events::OrderCanceled;
        roundtrip(OrderCanceled::default());
    }

    #[rstest]
    fn test_order_updated_round_trip(order_updated: OrderUpdated) {
        roundtrip(order_updated);
    }

    #[rstest]
    fn test_order_triggered_round_trip(order_triggered: OrderTriggered) {
        roundtrip(order_triggered);
    }

    #[rstest]
    fn test_order_expired_round_trip(order_expired: OrderExpired) {
        roundtrip(order_expired);
    }

    #[rstest]
    fn test_order_pending_update_round_trip(order_pending_update: OrderPendingUpdate) {
        roundtrip(order_pending_update);
    }

    #[rstest]
    fn test_order_pending_cancel_round_trip(order_pending_cancel: OrderPendingCancel) {
        roundtrip(order_pending_cancel);
    }

    #[rstest]
    fn test_order_cancel_rejected_round_trip(order_cancel_rejected: OrderCancelRejected) {
        roundtrip(order_cancel_rejected);
    }

    #[rstest]
    fn test_order_modify_rejected_round_trip(order_modify_rejected: OrderModifyRejected) {
        roundtrip(order_modify_rejected);
    }

    #[rstest]
    fn test_order_emulated_round_trip(order_emulated: OrderEmulated) {
        roundtrip(order_emulated);
    }

    #[rstest]
    fn test_order_released_round_trip(order_released: OrderReleased) {
        roundtrip(order_released);
    }
}
