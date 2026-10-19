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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

pub mod accepted;
pub mod accepted_batch;
pub mod any;
pub mod cancel_rejected;
pub mod canceled;
pub mod canceled_batch;
pub mod denied;
pub mod denied_reason;
pub mod emulated;
pub mod expired;
pub mod fill_voided;
pub mod filled;
pub mod initialized;
pub mod modify_rejected;
pub mod pending_cancel;
pub mod pending_update;
pub mod rejected;
pub mod released;
pub mod snapshot;
pub mod submitted;
pub mod submitted_batch;
pub mod triggered;
pub mod updated;

#[cfg(any(test, feature = "test-support"))]
pub mod spec;
#[cfg(any(test, feature = "test-support"))]
pub mod stubs;

/// Represents a type of [`OrderEvent`].
#[derive(Debug, PartialEq, Eq)]
pub enum OrderEventType {
    Initialized,
    Denied,
    Emulated,
    Released,
    Submitted,
    Accepted,
    Rejected,
    Canceled,
    Expired,
    Triggered,
    PendingUpdate,
    PendingCancel,
    ModifyRejected,
    CancelRejected,
    Updated,
    PartiallyFilled,
    Filled,
    FillVoided,
}

pub trait OrderEvent: 'static + Send {
    fn id(&self) -> UUID4;
    fn type_name(&self) -> &'static str;
    fn order_type(&self) -> Option<OrderType>;
    fn order_side(&self) -> Option<OrderSide>;
    fn trader_id(&self) -> TraderId;
    fn strategy_id(&self) -> StrategyId;
    fn instrument_id(&self) -> InstrumentId;
    fn trade_id(&self) -> Option<TradeId>;
    fn currency(&self) -> Option<Currency>;
    fn client_order_id(&self) -> ClientOrderId;
    fn reason(&self) -> Option<Ustr>;
    fn quantity(&self) -> Option<Quantity>;
    fn time_in_force(&self) -> Option<TimeInForce>;
    fn liquidity_side(&self) -> Option<LiquiditySide>;
    fn post_only(&self) -> Option<bool>;
    fn reduce_only(&self) -> Option<bool>;
    fn quote_quantity(&self) -> Option<bool>;
    fn reconciliation(&self) -> bool;
    fn price(&self) -> Option<Price>;
    fn last_px(&self) -> Option<Price>;
    fn last_qty(&self) -> Option<Quantity>;
    fn activation_price(&self) -> Option<Price>;
    fn trigger_price(&self) -> Option<Price>;
    fn trigger_type(&self) -> Option<TriggerType>;
    fn limit_offset(&self) -> Option<Decimal>;
    fn trailing_offset(&self) -> Option<Decimal>;
    fn trailing_offset_type(&self) -> Option<TrailingOffsetType>;
    fn expire_time(&self) -> Option<UnixNanos>;
    fn display_qty(&self) -> Option<Quantity>;
    fn emulation_trigger(&self) -> Option<TriggerType>;
    fn trigger_instrument_id(&self) -> Option<InstrumentId>;
    fn contingency_type(&self) -> Option<ContingencyType>;
    fn order_list_id(&self) -> Option<OrderListId>;
    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>>;
    fn parent_order_id(&self) -> Option<ClientOrderId>;
    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId>;
    fn exec_algorithm_params(&self) -> Option<IndexMap<Ustr, Ustr>> {
        None
    }
    fn exec_spawn_id(&self) -> Option<ClientOrderId>;
    fn tags(&self) -> Option<Vec<Ustr>> {
        None
    }
    fn venue_order_id(&self) -> Option<VenueOrderId>;
    fn account_id(&self) -> Option<AccountId>;
    fn position_id(&self) -> Option<PositionId>;
    fn commission(&self) -> Option<Money>;
    fn ts_event(&self) -> UnixNanos;
    fn ts_init(&self) -> UnixNanos;
    fn causation_id(&self) -> Option<UUID4> {
        None
    }
    fn released_price(&self) -> Option<Price> {
        None
    }
    fn protection_price(&self) -> Option<Price> {
        None
    }
    fn due_post_only(&self) -> bool {
        false
    }
    fn correction_id(&self) -> Option<Ustr> {
        None
    }
    fn is_reopened(&self) -> bool {
        false
    }
    fn info(&self) -> Option<IndexMap<Ustr, Ustr>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::events::order::{any::OrderEventAny, spec::*};

    /// Returns the sorted names of every `Option` accessor that yields `Some` for `event`.
    fn optional_fields_present(event: &dyn OrderEvent) -> Vec<&'static str> {
        let mut present: Vec<&'static str> = [
            ("activation_price", event.activation_price().is_some()),
            ("account_id", event.account_id().is_some()),
            ("causation_id", event.causation_id().is_some()),
            ("commission", event.commission().is_some()),
            ("contingency_type", event.contingency_type().is_some()),
            ("correction_id", event.correction_id().is_some()),
            ("currency", event.currency().is_some()),
            ("display_qty", event.display_qty().is_some()),
            ("emulation_trigger", event.emulation_trigger().is_some()),
            ("exec_algorithm_id", event.exec_algorithm_id().is_some()),
            (
                "exec_algorithm_params",
                event.exec_algorithm_params().is_some(),
            ),
            ("exec_spawn_id", event.exec_spawn_id().is_some()),
            ("expire_time", event.expire_time().is_some()),
            ("info", event.info().is_some()),
            ("last_px", event.last_px().is_some()),
            ("last_qty", event.last_qty().is_some()),
            ("limit_offset", event.limit_offset().is_some()),
            ("linked_order_ids", event.linked_order_ids().is_some()),
            ("liquidity_side", event.liquidity_side().is_some()),
            ("order_list_id", event.order_list_id().is_some()),
            ("order_side", event.order_side().is_some()),
            ("order_type", event.order_type().is_some()),
            ("parent_order_id", event.parent_order_id().is_some()),
            ("position_id", event.position_id().is_some()),
            ("post_only", event.post_only().is_some()),
            ("price", event.price().is_some()),
            ("protection_price", event.protection_price().is_some()),
            ("quantity", event.quantity().is_some()),
            ("quote_quantity", event.quote_quantity().is_some()),
            ("reason", event.reason().is_some()),
            ("reduce_only", event.reduce_only().is_some()),
            ("released_price", event.released_price().is_some()),
            ("tags", event.tags().is_some()),
            ("time_in_force", event.time_in_force().is_some()),
            ("trade_id", event.trade_id().is_some()),
            ("trailing_offset", event.trailing_offset().is_some()),
            (
                "trailing_offset_type",
                event.trailing_offset_type().is_some(),
            ),
            (
                "trigger_instrument_id",
                event.trigger_instrument_id().is_some(),
            ),
            ("trigger_price", event.trigger_price().is_some()),
            ("trigger_type", event.trigger_type().is_some()),
            ("venue_order_id", event.venue_order_id().is_some()),
        ]
        .into_iter()
        .filter(|(_, present)| *present)
        .map(|(name, _)| name)
        .collect();
        present.sort_unstable();
        present
    }

    fn params() -> IndexMap<Ustr, Ustr> {
        IndexMap::from([(Ustr::from("k"), Ustr::from("v"))])
    }

    fn fully_populated_events() -> Vec<(OrderEventAny, Vec<&'static str>)> {
        let venue_order_id = VenueOrderId::from("V-1");
        let account_id = AccountId::from("SIM-001");
        let reason = Ustr::from("REASON");
        let px = Price::from("1.00");

        vec![
            (
                OrderEventAny::Initialized(
                    OrderInitializedSpec::builder()
                        .price(px)
                        .activation_price(px)
                        .trigger_price(px)
                        .trigger_type(TriggerType::LastPrice)
                        .limit_offset(dec!(0.5))
                        .trailing_offset(dec!(0.5))
                        .trailing_offset_type(TrailingOffsetType::Price)
                        .expire_time(UnixNanos::from(9))
                        .display_qty(Quantity::from(1))
                        .emulation_trigger(TriggerType::BidAsk)
                        .trigger_instrument_id(InstrumentId::from("AUD/USD.SIM"))
                        .contingency_type(ContingencyType::Oto)
                        .order_list_id(OrderListId::from("OL-1"))
                        .linked_order_ids(vec![ClientOrderId::from("O-2")])
                        .parent_order_id(ClientOrderId::from("O-3"))
                        .exec_algorithm_id(ExecAlgorithmId::from("TWAP"))
                        .exec_algorithm_params(params())
                        .exec_spawn_id(ClientOrderId::from("O-4"))
                        .tags(vec![Ustr::from("TAG")])
                        .build(),
                ),
                vec![
                    "activation_price",
                    "contingency_type",
                    "display_qty",
                    "emulation_trigger",
                    "exec_algorithm_id",
                    "exec_algorithm_params",
                    "exec_spawn_id",
                    "expire_time",
                    "limit_offset",
                    "linked_order_ids",
                    "order_list_id",
                    "order_side",
                    "order_type",
                    "parent_order_id",
                    "post_only",
                    "price",
                    "quantity",
                    "quote_quantity",
                    "reduce_only",
                    "tags",
                    "time_in_force",
                    "trailing_offset",
                    "trailing_offset_type",
                    "trigger_instrument_id",
                    "trigger_price",
                    "trigger_type",
                ],
            ),
            (
                OrderEventAny::Denied(OrderDeniedSpec::builder().build()),
                vec!["reason"],
            ),
            (
                OrderEventAny::Emulated(OrderEmulatedSpec::builder().build()),
                vec![],
            ),
            (
                OrderEventAny::Released(OrderReleasedSpec::builder().build()),
                vec!["released_price"],
            ),
            (
                OrderEventAny::Submitted(OrderSubmittedSpec::builder().build()),
                vec!["account_id"],
            ),
            (
                OrderEventAny::Accepted(OrderAcceptedSpec::builder().build()),
                vec!["account_id", "venue_order_id"],
            ),
            (
                OrderEventAny::Rejected(OrderRejectedSpec::builder().build()),
                vec!["account_id", "reason"],
            ),
            (
                OrderEventAny::Canceled(
                    OrderCanceledSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .reason(reason)
                        .build(),
                ),
                vec!["account_id", "reason", "venue_order_id"],
            ),
            (
                OrderEventAny::Expired(
                    OrderExpiredSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .build(),
                ),
                vec!["account_id", "venue_order_id"],
            ),
            (
                OrderEventAny::Triggered(
                    OrderTriggeredSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .build(),
                ),
                vec!["account_id", "venue_order_id"],
            ),
            (
                OrderEventAny::PendingUpdate(
                    OrderPendingUpdateSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .build(),
                ),
                vec!["account_id", "venue_order_id"],
            ),
            (
                OrderEventAny::PendingCancel(
                    OrderPendingCancelSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .build(),
                ),
                vec!["account_id", "venue_order_id"],
            ),
            (
                OrderEventAny::ModifyRejected(
                    OrderModifyRejectedSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .build(),
                ),
                vec!["account_id", "reason", "venue_order_id"],
            ),
            (
                OrderEventAny::CancelRejected(
                    OrderCancelRejectedSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .build(),
                ),
                vec!["account_id", "reason", "venue_order_id"],
            ),
            (
                OrderEventAny::Updated(
                    OrderUpdatedSpec::builder()
                        .venue_order_id(venue_order_id)
                        .account_id(account_id)
                        .price(px)
                        .trigger_price(px)
                        .protection_price(px)
                        .build(),
                ),
                vec![
                    "account_id",
                    "price",
                    "protection_price",
                    "quantity",
                    "quote_quantity",
                    "trigger_price",
                    "venue_order_id",
                ],
            ),
            (
                OrderEventAny::Filled(
                    OrderFilledSpec::builder()
                        .position_id(PositionId::from("P-1"))
                        .commission(Money::from("1.00 USD"))
                        .info(params())
                        .build(),
                ),
                vec![
                    "account_id",
                    "commission",
                    "currency",
                    "info",
                    "last_px",
                    "last_qty",
                    "liquidity_side",
                    "order_side",
                    "order_type",
                    "position_id",
                    "quantity",
                    "trade_id",
                    "venue_order_id",
                ],
            ),
            (
                OrderEventAny::FillVoided(
                    OrderFillVoidedSpec::builder()
                        .commission_voided(Money::from("1.00 USD"))
                        .position_id(PositionId::from("P-1"))
                        .reason(reason)
                        .info(params())
                        .build(),
                ),
                vec![
                    "account_id",
                    "commission",
                    "correction_id",
                    "currency",
                    "info",
                    "last_px",
                    "last_qty",
                    "liquidity_side",
                    "order_side",
                    "order_type",
                    "position_id",
                    "quantity",
                    "reason",
                    "trade_id",
                    "venue_order_id",
                ],
            ),
        ]
    }

    #[rstest]
    fn test_optional_field_surface_matches_event_payload() {
        for (event, expected) in fully_populated_events() {
            let event_type = event.event_type();
            let boxed = event.into_boxed();

            assert_eq!(
                optional_fields_present(boxed.as_ref()),
                expected,
                "optional accessor surface changed for {event_type:?}"
            );
        }
    }

    #[rstest]
    fn test_initialized_accessors_map_distinct_fields() {
        let event = OrderInitializedSpec::builder()
            .quantity(Quantity::from(100))
            .price(Price::from("1.00"))
            .activation_price(Price::from("2.00"))
            .trigger_price(Price::from("3.00"))
            .trigger_type(TriggerType::LastPrice)
            .emulation_trigger(TriggerType::BidAsk)
            .limit_offset(dec!(0.5))
            .trailing_offset(dec!(1.5))
            .display_qty(Quantity::from(10))
            .parent_order_id(ClientOrderId::from("O-PARENT"))
            .exec_spawn_id(ClientOrderId::from("O-SPAWN"))
            .ts_event(UnixNanos::from(7))
            .ts_init(UnixNanos::from(8))
            .build();

        assert_eq!(event.price(), Some(Price::from("1.00")));
        assert_eq!(event.activation_price(), Some(Price::from("2.00")));
        assert_eq!(event.trigger_price(), Some(Price::from("3.00")));
        assert_eq!(event.trigger_type(), Some(TriggerType::LastPrice));
        assert_eq!(event.emulation_trigger(), Some(TriggerType::BidAsk));
        assert_eq!(event.limit_offset(), Some(dec!(0.5)));
        assert_eq!(event.trailing_offset(), Some(dec!(1.5)));
        assert_eq!(event.quantity(), Some(Quantity::from(100)));
        assert_eq!(event.display_qty(), Some(Quantity::from(10)));
        assert_eq!(
            event.parent_order_id(),
            Some(ClientOrderId::from("O-PARENT"))
        );
        assert_eq!(event.exec_spawn_id(), Some(ClientOrderId::from("O-SPAWN")));
        assert_eq!(event.ts_event(), UnixNanos::from(7));
        assert_eq!(event.ts_init(), UnixNanos::from(8));
    }

    #[rstest]
    fn test_filled_accessors_map_distinct_fields() {
        let event = OrderFilledSpec::builder()
            .last_qty(Quantity::from(25))
            .last_px(Price::from("4.00"))
            .commission(Money::from("2.00 USD"))
            .ts_event(UnixNanos::from(7))
            .ts_init(UnixNanos::from(8))
            .build();

        assert_eq!(event.last_px(), Some(Price::from("4.00")));
        assert_eq!(event.price(), None);
        assert_eq!(event.last_qty(), Some(Quantity::from(25)));
        assert_eq!(event.quantity(), Some(Quantity::from(25)));
        assert_eq!(event.commission(), Some(Money::from("2.00 USD")));
        assert_eq!(event.ts_event(), UnixNanos::from(7));
        assert_eq!(event.ts_init(), UnixNanos::from(8));
    }

    #[rstest]
    fn test_updated_accessors_map_distinct_prices() {
        let event = OrderUpdatedSpec::builder()
            .quantity(Quantity::from(50))
            .price(Price::from("1.00"))
            .trigger_price(Price::from("2.00"))
            .protection_price(Price::from("3.00"))
            .ts_event(UnixNanos::from(7))
            .ts_init(UnixNanos::from(8))
            .build();

        assert_eq!(event.price(), Some(Price::from("1.00")));
        assert_eq!(event.trigger_price(), Some(Price::from("2.00")));
        assert_eq!(event.protection_price(), Some(Price::from("3.00")));
        assert_eq!(event.quantity(), Some(Quantity::from(50)));
        assert_eq!(event.ts_event(), UnixNanos::from(7));
        assert_eq!(event.ts_init(), UnixNanos::from(8));
    }

    #[rstest]
    fn test_type_name_matches_variant() {
        let expected = [
            "OrderInitialized",
            "OrderDenied",
            "OrderEmulated",
            "OrderReleased",
            "OrderSubmitted",
            "OrderAccepted",
            "OrderRejected",
            "OrderCanceled",
            "OrderExpired",
            "OrderTriggered",
            "OrderPendingUpdate",
            "OrderPendingCancel",
            "OrderModifyRejected",
            "OrderCancelRejected",
            "OrderUpdated",
            "OrderFilled",
            "OrderFillVoided",
        ];

        let names: Vec<&'static str> = fully_populated_events()
            .into_iter()
            .map(|(event, _)| event.into_boxed().type_name())
            .collect();

        assert_eq!(names, expected);
    }
}
