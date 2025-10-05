// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::Display;

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    enums::{
        ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, OrderListId, PositionId, VenueOrderId},
    types::{Price, Quantity},
};

/// Represents an order status at a point in time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderStatusReport {
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The client order ID.
    pub client_order_id: Option<ClientOrderId>,
    /// The venue assigned order ID.
    pub venue_order_id: VenueOrderId,
    /// The order side.
    pub order_side: OrderSide,
    /// The order type.
    pub order_type: OrderType,
    /// The order time in force.
    pub time_in_force: TimeInForce,
    /// The order status.
    pub order_status: OrderStatus,
    /// The order quantity.
    pub quantity: Quantity,
    /// The order total filled quantity.
    pub filled_qty: Quantity,
    /// The unique identifier for the event.
    pub report_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the order was accepted.
    pub ts_accepted: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the last event occurred.
    pub ts_last: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
    /// The order list ID associated with the order.
    pub order_list_id: Option<OrderListId>,
    /// The position ID associated with the order (assigned by the venue).
    pub venue_position_id: Option<PositionId>,
    /// The reported linked client order IDs related to contingency orders.
    pub linked_order_ids: Option<Vec<ClientOrderId>>,
    /// The parent order ID for contingent child orders, if available.
    pub parent_order_id: Option<ClientOrderId>,
    /// The orders contingency type.
    pub contingency_type: ContingencyType,
    /// The order expiration (UNIX epoch nanoseconds), zero for no expiration.
    pub expire_time: Option<UnixNanos>,
    /// The order price (LIMIT).
    pub price: Option<Price>,
    /// The order trigger price (STOP).
    pub trigger_price: Option<Price>,
    /// The trigger type for the order.
    pub trigger_type: Option<TriggerType>,
    /// The trailing offset for the orders limit price.
    pub limit_offset: Option<Decimal>,
    /// The trailing offset for the orders trigger price (STOP).
    pub trailing_offset: Option<Decimal>,
    /// The trailing offset type.
    pub trailing_offset_type: TrailingOffsetType,
    /// The order average fill price.
    pub avg_px: Option<f64>,
    /// The quantity of the `LIMIT` order to display on the public book (iceberg).
    pub display_qty: Option<Quantity>,
    /// If the order will only provide liquidity (make a market).
    pub post_only: bool,
    /// If the order carries the 'reduce-only' execution instruction.
    pub reduce_only: bool,
    /// The reason for order cancellation.
    pub cancel_reason: Option<String>,
    /// UNIX timestamp (nanoseconds) when the order was triggered.
    pub ts_triggered: Option<UnixNanos>,
}

impl OrderStatusReport {
    /// Creates a new [`OrderStatusReport`] instance with required fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: VenueOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        time_in_force: TimeInForce,
        order_status: OrderStatus,
        quantity: Quantity,
        filled_qty: Quantity,
        ts_accepted: UnixNanos,
        ts_last: UnixNanos,
        ts_init: UnixNanos,
        report_id: Option<UUID4>,
    ) -> Self {
        Self {
            account_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            quantity,
            filled_qty,
            report_id: report_id.unwrap_or_default(),
            ts_accepted,
            ts_last,
            ts_init,
            order_list_id: None,
            venue_position_id: None,
            linked_order_ids: None,
            parent_order_id: None,
            contingency_type: ContingencyType::default(),
            expire_time: None,
            price: None,
            trigger_price: None,
            trigger_type: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::default(),
            avg_px: None,
            display_qty: None,
            post_only: false,
            reduce_only: false,
            cancel_reason: None,
            ts_triggered: None,
        }
    }

    /// Sets the client order ID.
    #[must_use]
    pub const fn with_client_order_id(mut self, client_order_id: ClientOrderId) -> Self {
        self.client_order_id = Some(client_order_id);
        self
    }

    /// Sets the order list ID.
    #[must_use]
    pub const fn with_order_list_id(mut self, order_list_id: OrderListId) -> Self {
        self.order_list_id = Some(order_list_id);
        self
    }

    /// Sets the linked client order IDs.
    #[must_use]
    pub fn with_linked_order_ids(
        mut self,
        linked_order_ids: impl IntoIterator<Item = ClientOrderId>,
    ) -> Self {
        self.linked_order_ids = Some(linked_order_ids.into_iter().collect());
        self
    }

    /// Sets the parent order ID.
    #[must_use]
    pub const fn with_parent_order_id(mut self, parent_order_id: ClientOrderId) -> Self {
        self.parent_order_id = Some(parent_order_id);
        self
    }

    /// Sets the venue position ID.
    #[must_use]
    pub const fn with_venue_position_id(mut self, venue_position_id: PositionId) -> Self {
        self.venue_position_id = Some(venue_position_id);
        self
    }

    /// Sets the price.
    #[must_use]
    pub const fn with_price(mut self, price: Price) -> Self {
        self.price = Some(price);
        self
    }

    /// Sets the average price.
    #[must_use]
    pub const fn with_avg_px(mut self, avg_px: f64) -> Self {
        self.avg_px = Some(avg_px);
        self
    }

    /// Sets the trigger price.
    #[must_use]
    pub const fn with_trigger_price(mut self, trigger_price: Price) -> Self {
        self.trigger_price = Some(trigger_price);
        self
    }

    /// Sets the trigger type.
    #[must_use]
    pub const fn with_trigger_type(mut self, trigger_type: TriggerType) -> Self {
        self.trigger_type = Some(trigger_type);
        self
    }

    /// Sets the limit offset.
    #[must_use]
    pub const fn with_limit_offset(mut self, limit_offset: Decimal) -> Self {
        self.limit_offset = Some(limit_offset);
        self
    }

    /// Sets the trailing offset.
    #[must_use]
    pub const fn with_trailing_offset(mut self, trailing_offset: Decimal) -> Self {
        self.trailing_offset = Some(trailing_offset);
        self
    }

    /// Sets the trailing offset type.
    #[must_use]
    pub const fn with_trailing_offset_type(
        mut self,
        trailing_offset_type: TrailingOffsetType,
    ) -> Self {
        self.trailing_offset_type = trailing_offset_type;
        self
    }

    /// Sets the display quantity.
    #[must_use]
    pub const fn with_display_qty(mut self, display_qty: Quantity) -> Self {
        self.display_qty = Some(display_qty);
        self
    }

    /// Sets the expire time.
    #[must_use]
    pub const fn with_expire_time(mut self, expire_time: UnixNanos) -> Self {
        self.expire_time = Some(expire_time);
        self
    }

    /// Sets `post_only` flag.
    #[must_use]
    pub const fn with_post_only(mut self, post_only: bool) -> Self {
        self.post_only = post_only;
        self
    }

    /// Sets `reduce_only` flag.
    #[must_use]
    pub const fn with_reduce_only(mut self, reduce_only: bool) -> Self {
        self.reduce_only = reduce_only;
        self
    }

    /// Sets cancel reason.
    #[must_use]
    pub fn with_cancel_reason(mut self, cancel_reason: String) -> Self {
        self.cancel_reason = Some(cancel_reason);
        self
    }

    /// Sets the triggered timestamp.
    #[must_use]
    pub const fn with_ts_triggered(mut self, ts_triggered: UnixNanos) -> Self {
        self.ts_triggered = Some(ts_triggered);
        self
    }

    /// Sets the contingency type.
    #[must_use]
    pub const fn with_contingency_type(mut self, contingency_type: ContingencyType) -> Self {
        self.contingency_type = contingency_type;
        self
    }
}

impl Display for OrderStatusReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OrderStatusReport(\
                account_id={}, \
                instrument_id={}, \
                venue_order_id={}, \
                order_side={}, \
                order_type={}, \
                time_in_force={}, \
                order_status={}, \
                quantity={}, \
                filled_qty={}, \
                report_id={}, \
                ts_accepted={}, \
                ts_last={}, \
                ts_init={}, \
                client_order_id={:?}, \
                order_list_id={:?}, \
                venue_position_id={:?}, \
                linked_order_ids={:?}, \
                parent_order_id={:?}, \
                contingency_type={}, \
                expire_time={:?}, \
                price={:?}, \
                trigger_price={:?}, \
                trigger_type={:?}, \
                limit_offset={:?}, \
                trailing_offset={:?}, \
                trailing_offset_type={}, \
                avg_px={:?}, \
                display_qty={:?}, \
                post_only={}, \
                reduce_only={}, \
                cancel_reason={:?}, \
                ts_triggered={:?}\
            )",
            self.account_id,
            self.instrument_id,
            self.venue_order_id,
            self.order_side,
            self.order_type,
            self.time_in_force,
            self.order_status,
            self.quantity,
            self.filled_qty,
            self.report_id,
            self.ts_accepted,
            self.ts_last,
            self.ts_init,
            self.client_order_id,
            self.order_list_id,
            self.venue_position_id,
            self.linked_order_ids,
            self.parent_order_id,
            self.contingency_type,
            self.expire_time,
            self.price,
            self.trigger_price,
            self.trigger_type,
            self.limit_offset,
            self.trailing_offset,
            self.trailing_offset_type,
            self.avg_px,
            self.display_qty,
            self.post_only,
            self.reduce_only,
            self.cancel_reason,
            self.ts_triggered,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::*;
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        enums::{
            ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TrailingOffsetType,
            TriggerType,
        },
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, OrderListId, PositionId, VenueOrderId,
        },
        types::{Price, Quantity},
    };

    fn test_order_status_report() -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            Some(ClientOrderId::from("O-19700101-000000-001-001-1")),
            VenueOrderId::from("1"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("100"),
            Quantity::from("0"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        )
    }

    #[rstest]
    fn test_order_status_report_new() {
        let report = test_order_status_report();

        assert_eq!(report.account_id, AccountId::from("SIM-001"));
        assert_eq!(report.instrument_id, InstrumentId::from("AUDUSD.SIM"));
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-19700101-000000-001-001-1"))
        );
        assert_eq!(report.venue_order_id, VenueOrderId::from("1"));
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity, Quantity::from("100"));
        assert_eq!(report.filled_qty, Quantity::from("0"));
        assert_eq!(report.ts_accepted, UnixNanos::from(1_000_000_000));
        assert_eq!(report.ts_last, UnixNanos::from(2_000_000_000));
        assert_eq!(report.ts_init, UnixNanos::from(3_000_000_000));

        // Test default values
        assert_eq!(report.order_list_id, None);
        assert_eq!(report.venue_position_id, None);
        assert_eq!(report.linked_order_ids, None);
        assert_eq!(report.parent_order_id, None);
        assert_eq!(report.contingency_type, ContingencyType::default());
        assert_eq!(report.expire_time, None);
        assert_eq!(report.price, None);
        assert_eq!(report.trigger_price, None);
        assert_eq!(report.trigger_type, None);
        assert_eq!(report.limit_offset, None);
        assert_eq!(report.trailing_offset, None);
        assert_eq!(report.trailing_offset_type, TrailingOffsetType::default());
        assert_eq!(report.avg_px, None);
        assert_eq!(report.display_qty, None);
        assert!(!report.post_only);
        assert!(!report.reduce_only);
        assert_eq!(report.cancel_reason, None);
        assert_eq!(report.ts_triggered, None);
    }

    #[rstest]
    fn test_order_status_report_with_generated_report_id() {
        let report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            None,
            VenueOrderId::from("1"),
            OrderSide::Buy,
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None, // No report ID provided, should generate one
        );

        // Should have a generated UUID
        assert_ne!(
            report.report_id.to_string(),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[rstest]
    fn test_order_status_report_builder_methods() {
        let report = test_order_status_report()
            .with_client_order_id(ClientOrderId::from("O-19700101-000000-001-001-2"))
            .with_order_list_id(OrderListId::from("OL-001"))
            .with_venue_position_id(PositionId::from("P-001"))
            .with_parent_order_id(ClientOrderId::from("O-PARENT"))
            .with_price(Price::from("1.00000"))
            .with_avg_px(1.00001)
            .with_trigger_price(Price::from("0.99000"))
            .with_trigger_type(TriggerType::Default)
            .with_limit_offset(Decimal::from_f64_retain(0.0001).unwrap())
            .with_trailing_offset(Decimal::from_f64_retain(0.0002).unwrap())
            .with_trailing_offset_type(TrailingOffsetType::BasisPoints)
            .with_display_qty(Quantity::from("50"))
            .with_expire_time(UnixNanos::from(4_000_000_000))
            .with_post_only(true)
            .with_reduce_only(true)
            .with_cancel_reason("User requested".to_string())
            .with_ts_triggered(UnixNanos::from(1_500_000_000))
            .with_contingency_type(ContingencyType::Oco);

        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-19700101-000000-001-001-2"))
        );
        assert_eq!(report.order_list_id, Some(OrderListId::from("OL-001")));
        assert_eq!(report.venue_position_id, Some(PositionId::from("P-001")));
        assert_eq!(
            report.parent_order_id,
            Some(ClientOrderId::from("O-PARENT"))
        );
        assert_eq!(report.price, Some(Price::from("1.00000")));
        assert_eq!(report.avg_px, Some(1.00001));
        assert_eq!(report.trigger_price, Some(Price::from("0.99000")));
        assert_eq!(report.trigger_type, Some(TriggerType::Default));
        assert_eq!(
            report.limit_offset,
            Some(Decimal::from_f64_retain(0.0001).unwrap())
        );
        assert_eq!(
            report.trailing_offset,
            Some(Decimal::from_f64_retain(0.0002).unwrap())
        );
        assert_eq!(report.trailing_offset_type, TrailingOffsetType::BasisPoints);
        assert_eq!(report.display_qty, Some(Quantity::from("50")));
        assert_eq!(report.expire_time, Some(UnixNanos::from(4_000_000_000)));
        assert!(report.post_only);
        assert!(report.reduce_only);
        assert_eq!(report.cancel_reason, Some("User requested".to_string()));
        assert_eq!(report.ts_triggered, Some(UnixNanos::from(1_500_000_000)));
        assert_eq!(report.contingency_type, ContingencyType::Oco);
    }

    #[rstest]
    fn test_display() {
        let report = test_order_status_report();
        let display_str = format!("{report}");

        assert!(display_str.contains("OrderStatusReport"));
        assert!(display_str.contains("SIM-001"));
        assert!(display_str.contains("AUDUSD.SIM"));
        assert!(display_str.contains("BUY"));
        assert!(display_str.contains("LIMIT"));
        assert!(display_str.contains("GTC"));
        assert!(display_str.contains("ACCEPTED"));
        assert!(display_str.contains("100"));
    }

    #[rstest]
    fn test_clone_and_equality() {
        let report1 = test_order_status_report();
        let report2 = report1.clone();

        assert_eq!(report1, report2);
    }

    #[rstest]
    fn test_serialization_roundtrip() {
        let original = test_order_status_report();

        // Test JSON serialization
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: OrderStatusReport = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_order_status_report_different_order_types() {
        let market_report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            None,
            VenueOrderId::from("1"),
            OrderSide::Buy,
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        );

        let stop_report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            None,
            VenueOrderId::from("2"),
            OrderSide::Sell,
            OrderType::StopMarket,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("50"),
            Quantity::from("0"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        );

        assert_eq!(market_report.order_type, OrderType::Market);
        assert_eq!(stop_report.order_type, OrderType::StopMarket);
        assert_ne!(market_report, stop_report);
    }

    #[rstest]
    fn test_order_status_report_different_statuses() {
        let accepted_report = test_order_status_report();

        let filled_report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            Some(ClientOrderId::from("O-19700101-000000-001-001-1")),
            VenueOrderId::from("1"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("100"),
            Quantity::from("100"), // Fully filled
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        );

        assert_eq!(accepted_report.order_status, OrderStatus::Accepted);
        assert_eq!(filled_report.order_status, OrderStatus::Filled);
        assert_ne!(accepted_report, filled_report);
    }

    #[rstest]
    fn test_order_status_report_with_optional_fields() {
        let mut report = test_order_status_report();

        // Initially no optional fields set
        assert_eq!(report.price, None);
        assert_eq!(report.avg_px, None);
        assert!(!report.post_only);
        assert!(!report.reduce_only);

        // Test builder pattern with various optional fields
        report = report
            .with_price(Price::from("1.00000"))
            .with_avg_px(1.00001)
            .with_post_only(true)
            .with_reduce_only(true);

        assert_eq!(report.price, Some(Price::from("1.00000")));
        assert_eq!(report.avg_px, Some(1.00001));
        assert!(report.post_only);
        assert!(report.reduce_only);
    }

    #[rstest]
    fn test_order_status_report_partial_fill() {
        let partial_fill_report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            Some(ClientOrderId::from("O-19700101-000000-001-001-1")),
            VenueOrderId::from("1"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::PartiallyFilled,
            Quantity::from("100"),
            Quantity::from("30"), // Partially filled
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        );

        assert_eq!(partial_fill_report.quantity, Quantity::from("100"));
        assert_eq!(partial_fill_report.filled_qty, Quantity::from("30"));
        assert_eq!(
            partial_fill_report.order_status,
            OrderStatus::PartiallyFilled
        );
    }

    #[rstest]
    fn test_order_status_report_with_all_timestamp_fields() {
        let report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            None,
            VenueOrderId::from("1"),
            OrderSide::Buy,
            OrderType::StopLimit,
            TimeInForce::Gtc,
            OrderStatus::Triggered,
            Quantity::from("100"),
            Quantity::from("0"),
            UnixNanos::from(1_000_000_000), // ts_accepted
            UnixNanos::from(2_000_000_000), // ts_last
            UnixNanos::from(3_000_000_000), // ts_init
            None,
        )
        .with_ts_triggered(UnixNanos::from(1_500_000_000));

        assert_eq!(report.ts_accepted, UnixNanos::from(1_000_000_000));
        assert_eq!(report.ts_last, UnixNanos::from(2_000_000_000));
        assert_eq!(report.ts_init, UnixNanos::from(3_000_000_000));
        assert_eq!(report.ts_triggered, Some(UnixNanos::from(1_500_000_000)));
    }
}
