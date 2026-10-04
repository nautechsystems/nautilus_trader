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

//! Factory for generating order and account events.

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    enums::{AccountType, LiquiditySide},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEventAny, OrderExpired, OrderFilled, OrderModifyRejected, OrderRejected,
        OrderSubmitted, OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, PositionId, TradeId, TraderId, VenueOrderId},
    orders::{Order, OrderAny},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};

/// Factory for generating order and account events.
///
/// This struct holds the identity information needed to construct events and provides
/// methods to generate all order event types. It is `Clone` and `Send`, allowing it
/// to be used in async contexts.
#[derive(Debug, Clone)]
pub struct OrderEventFactory {
    trader_id: TraderId,
    account_id: AccountId,
    account_type: AccountType,
    base_currency: Option<Currency>,
}

impl OrderEventFactory {
    /// Creates a new [`OrderEventFactory`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Option<Currency>,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            account_type,
            base_currency,
        }
    }

    /// Returns the trader ID.
    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    /// Returns the account ID.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Sets the account ID.
    pub const fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = account_id;
    }

    /// Generates an account state event.
    #[must_use]
    pub fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        info: Option<Params>,
    ) -> AccountState {
        AccountState::new(
            self.account_id,
            self.account_type,
            balances,
            margins,
            reported,
            UUID4::new(),
            ts_event,
            ts_init,
            self.base_currency,
        )
        .with_info(info)
    }

    /// Generates an order denied event.
    ///
    /// The event timestamp `ts_event` is the same as the initialized timestamp `ts_init`.
    #[must_use]
    pub fn generate_order_denied(
        &self,
        order: &OrderAny,
        reason: &str,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderDenied::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_init,
            ts_init,
        );
        OrderEventAny::Denied(event)
    }

    /// Generates an order submitted event.
    ///
    /// The event timestamp `ts_event` is the same as the initialized timestamp `ts_init`.
    #[must_use]
    pub fn generate_order_submitted(&self, order: &OrderAny, ts_init: UnixNanos) -> OrderEventAny {
        let event = OrderSubmitted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            UUID4::new(),
            ts_init,
            ts_init,
        );
        OrderEventAny::Submitted(event)
    }

    /// Generates an order rejected event.
    #[must_use]
    pub fn generate_order_rejected(
        &self,
        order: &OrderAny,
        reason: &str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        due_post_only: bool,
    ) -> OrderEventAny {
        let event = OrderRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            reason.into(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            due_post_only,
        );
        OrderEventAny::Rejected(event)
    }

    /// Generates an order accepted event.
    #[must_use]
    pub fn generate_order_accepted(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderAccepted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
        );
        OrderEventAny::Accepted(event)
    }

    /// Generates an order modify rejected event.
    #[must_use]
    pub fn generate_order_modify_rejected(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderModifyRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::ModifyRejected(event)
    }

    /// Generates an order cancel rejected event.
    #[must_use]
    pub fn generate_order_cancel_rejected(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderCancelRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::CancelRejected(event)
    }

    /// Generates an order updated event.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn generate_order_updated(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        protection_price: Option<Price>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderUpdated::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            quantity,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            Some(venue_order_id),
            Some(self.account_id),
            price,
            trigger_price,
            protection_price,
            false, // is_quote_quantity
        );
        OrderEventAny::Updated(event)
    }

    /// Generates an order canceled event.
    #[must_use]
    pub fn generate_order_canceled(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderCanceled::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
            None,
        );
        OrderEventAny::Canceled(event)
    }

    /// Generates an order triggered event.
    #[must_use]
    pub fn generate_order_triggered(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderTriggered::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::Triggered(event)
    }

    /// Generates an order expired event.
    #[must_use]
    pub fn generate_order_expired(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderExpired::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::Expired(event)
    }

    /// Generates an order filled event.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn generate_order_filled(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        venue_position_id: Option<PositionId>,
        trade_id: TradeId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Option<Money>,
        liquidity_side: LiquiditySide,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> OrderEventAny {
        let event = OrderFilled::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            self.account_id,
            trade_id,
            order.order_side(),
            order.order_type(),
            last_qty,
            last_px,
            quote_currency,
            liquidity_side,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            venue_position_id,
            commission,
            None,
        );
        OrderEventAny::Filled(event)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{ClientOrderId, InstrumentId},
        orders::OrderTestBuilder,
        types::Quantity,
    };
    use rstest::{fixture, rstest};

    use super::*;

    const TRADER_ID: &str = "TESTER-001";
    const ACCOUNT_ID: &str = "SIM-002";

    #[fixture]
    fn factory() -> OrderEventFactory {
        OrderEventFactory::new(
            TraderId::from(TRADER_ID),
            AccountId::from(ACCOUNT_ID),
            AccountType::Margin,
            Some(Currency::USD()),
        )
    }

    #[fixture]
    fn order() -> OrderAny {
        OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("ETHUSDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-19700101-000000-001-001-1"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("3.000"))
            .price(Price::from("2500.00"))
            .build()
    }

    #[rstest]
    fn test_identity_accessors_and_account_id_override(mut factory: OrderEventFactory) {
        assert_eq!(factory.trader_id(), TraderId::from(TRADER_ID));
        assert_eq!(factory.account_id(), AccountId::from(ACCOUNT_ID));

        factory.set_account_id(AccountId::from("SIM-999"));

        assert_eq!(factory.account_id(), AccountId::from("SIM-999"));
        assert_eq!(factory.trader_id(), TraderId::from(TRADER_ID));
    }

    #[rstest]
    fn test_generate_account_state_carries_factory_identity(factory: OrderEventFactory) {
        let balance = AccountBalance::new(
            Money::new(1_500.0, Currency::USD()),
            Money::new(500.0, Currency::USD()),
            Money::new(1_000.0, Currency::USD()),
        );

        let margin = MarginBalance::new(
            Money::new(10.0, Currency::USD()),
            Money::new(20.0, Currency::USD()),
            Some(InstrumentId::from("ETHUSDT.BINANCE")),
        );

        let event = factory.generate_account_state(
            vec![balance],
            vec![margin],
            true,
            UnixNanos::from(3),
            UnixNanos::from(5),
            None,
        );

        assert_eq!(event.account_id, AccountId::from(ACCOUNT_ID));
        assert_eq!(event.account_type, AccountType::Margin);
        assert_eq!(event.base_currency, Some(Currency::USD()));
        assert_eq!(event.balances, vec![balance]);
        assert_eq!(event.margins, vec![margin]);
        assert!(event.is_reported);
        assert_eq!(event.ts_event, UnixNanos::from(3));
        assert_eq!(event.ts_init, UnixNanos::from(5));
        assert_eq!(event.info, None);
    }

    #[rstest]
    fn test_generate_account_state_attaches_info(factory: OrderEventFactory) {
        let mut info = Params::new();
        info.insert("source".into(), "unit-test".into());

        let event = factory.generate_account_state(
            Vec::new(),
            Vec::new(),
            false,
            UnixNanos::from(3),
            UnixNanos::from(5),
            Some(info.clone()),
        );

        assert!(!event.is_reported);
        assert_eq!(event.info, Some(info));
    }

    #[rstest]
    fn test_generate_order_denied_uses_ts_init_for_both_timestamps(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let OrderEventAny::Denied(event) =
            factory.generate_order_denied(&order, "Risk limit exceeded", UnixNanos::from(11))
        else {
            panic!("expected a denied event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.strategy_id, order.strategy_id());
        assert_eq!(event.instrument_id, order.instrument_id());
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.reason.as_str(), "Risk limit exceeded");
        assert_eq!(event.ts_event, UnixNanos::from(11));
        assert_eq!(event.ts_init, UnixNanos::from(11));
    }

    #[rstest]
    fn test_generate_order_submitted_uses_ts_init_for_both_timestamps(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let OrderEventAny::Submitted(event) =
            factory.generate_order_submitted(&order, UnixNanos::from(13))
        else {
            panic!("expected a submitted event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.strategy_id, order.strategy_id());
        assert_eq!(event.instrument_id, order.instrument_id());
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.account_id, AccountId::from(ACCOUNT_ID));
        assert_eq!(event.ts_event, UnixNanos::from(13));
        assert_eq!(event.ts_init, UnixNanos::from(13));
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn test_generate_order_rejected_carries_reason_and_post_only_flag(
        factory: OrderEventFactory,
        order: OrderAny,
        #[case] due_post_only: bool,
    ) {
        let OrderEventAny::Rejected(event) = factory.generate_order_rejected(
            &order,
            "Insufficient margin",
            UnixNanos::from(17),
            UnixNanos::from(19),
            due_post_only,
        ) else {
            panic!("expected a rejected event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.strategy_id, order.strategy_id());
        assert_eq!(event.instrument_id, order.instrument_id());
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.account_id, AccountId::from(ACCOUNT_ID));
        assert_eq!(event.reason.as_str(), "Insufficient margin");
        assert_eq!(event.ts_event, UnixNanos::from(17));
        assert_eq!(event.ts_init, UnixNanos::from(19));
        assert!(!event.reconciliation);
        assert_eq!(event.due_post_only, due_post_only);
    }

    #[rstest]
    fn test_generate_order_accepted_carries_venue_order_id(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let OrderEventAny::Accepted(event) = factory.generate_order_accepted(
            &order,
            VenueOrderId::from("V-1"),
            UnixNanos::from(23),
            UnixNanos::from(29),
        ) else {
            panic!("expected an accepted event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, VenueOrderId::from("V-1"));
        assert_eq!(event.account_id, AccountId::from(ACCOUNT_ID));
        assert_eq!(event.ts_event, UnixNanos::from(23));
        assert_eq!(event.ts_init, UnixNanos::from(29));
        assert!(!event.reconciliation);
    }

    #[rstest]
    fn test_generate_order_modify_rejected_carries_optional_venue_order_id(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let OrderEventAny::ModifyRejected(event) = factory.generate_order_modify_rejected(
            &order,
            Some(VenueOrderId::from("V-2")),
            "Unknown order",
            UnixNanos::from(31),
            UnixNanos::from(37),
        ) else {
            panic!("expected a modify rejected event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, Some(VenueOrderId::from("V-2")));
        assert_eq!(event.account_id, Some(AccountId::from(ACCOUNT_ID)));
        assert_eq!(event.reason.as_str(), "Unknown order");
        assert_eq!(event.ts_event, UnixNanos::from(31));
        assert_eq!(event.ts_init, UnixNanos::from(37));
    }

    #[rstest]
    fn test_generate_order_cancel_rejected_allows_absent_venue_order_id(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let OrderEventAny::CancelRejected(event) = factory.generate_order_cancel_rejected(
            &order,
            None,
            "Already closed",
            UnixNanos::from(41),
            UnixNanos::from(43),
        ) else {
            panic!("expected a cancel rejected event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, None);
        assert_eq!(event.account_id, Some(AccountId::from(ACCOUNT_ID)));
        assert_eq!(event.reason.as_str(), "Already closed");
        assert_eq!(event.ts_event, UnixNanos::from(41));
        assert_eq!(event.ts_init, UnixNanos::from(43));
    }

    #[rstest]
    fn test_generate_order_updated_carries_every_revised_field(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let OrderEventAny::Updated(event) = factory.generate_order_updated(
            &order,
            VenueOrderId::from("V-3"),
            Quantity::from("2.000"),
            Some(Price::from("2400.00")),
            Some(Price::from("2450.00")),
            Some(Price::from("2350.00")),
            UnixNanos::from(47),
            UnixNanos::from(53),
        ) else {
            panic!("expected an updated event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, Some(VenueOrderId::from("V-3")));
        assert_eq!(event.account_id, Some(AccountId::from(ACCOUNT_ID)));
        assert_eq!(event.quantity, Quantity::from("2.000"));
        assert_eq!(event.price, Some(Price::from("2400.00")));
        assert_eq!(event.trigger_price, Some(Price::from("2450.00")));
        assert_eq!(event.protection_price, Some(Price::from("2350.00")));
        assert!(!event.is_quote_quantity);
        assert_eq!(event.ts_event, UnixNanos::from(47));
        assert_eq!(event.ts_init, UnixNanos::from(53));
    }

    #[rstest]
    fn test_generate_order_canceled_carries_identity(factory: OrderEventFactory, order: OrderAny) {
        let OrderEventAny::Canceled(event) = factory.generate_order_canceled(
            &order,
            Some(VenueOrderId::from("V-4")),
            UnixNanos::from(59),
            UnixNanos::from(61),
        ) else {
            panic!("expected a canceled event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, Some(VenueOrderId::from("V-4")));
        assert_eq!(event.account_id, Some(AccountId::from(ACCOUNT_ID)));
        assert_eq!(event.ts_event, UnixNanos::from(59));
        assert_eq!(event.ts_init, UnixNanos::from(61));
    }

    #[rstest]
    fn test_generate_order_triggered_carries_identity(factory: OrderEventFactory, order: OrderAny) {
        let OrderEventAny::Triggered(event) = factory.generate_order_triggered(
            &order,
            Some(VenueOrderId::from("V-5")),
            UnixNanos::from(67),
            UnixNanos::from(71),
        ) else {
            panic!("expected a triggered event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, Some(VenueOrderId::from("V-5")));
        assert_eq!(event.account_id, Some(AccountId::from(ACCOUNT_ID)));
        assert_eq!(event.ts_event, UnixNanos::from(67));
        assert_eq!(event.ts_init, UnixNanos::from(71));
    }

    #[rstest]
    fn test_generate_order_expired_carries_identity(factory: OrderEventFactory, order: OrderAny) {
        let OrderEventAny::Expired(event) = factory.generate_order_expired(
            &order,
            Some(VenueOrderId::from("V-6")),
            UnixNanos::from(73),
            UnixNanos::from(79),
        ) else {
            panic!("expected an expired event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, Some(VenueOrderId::from("V-6")));
        assert_eq!(event.account_id, Some(AccountId::from(ACCOUNT_ID)));
        assert_eq!(event.ts_event, UnixNanos::from(73));
        assert_eq!(event.ts_init, UnixNanos::from(79));
    }

    #[rstest]
    fn test_generate_order_filled_takes_side_and_type_from_the_order(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let commission = Money::new(1.25, Currency::USD());

        let OrderEventAny::Filled(event) = factory.generate_order_filled(
            &order,
            VenueOrderId::from("V-7"),
            Some(PositionId::from("P-1")),
            TradeId::from("T-1"),
            Quantity::from("1.500"),
            Price::from("2499.50"),
            Currency::USDT(),
            Some(commission),
            LiquiditySide::Maker,
            UnixNanos::from(83),
            UnixNanos::from(89),
        ) else {
            panic!("expected a filled event");
        };

        assert_eq!(event.trader_id, TraderId::from(TRADER_ID));
        assert_eq!(event.strategy_id, order.strategy_id());
        assert_eq!(event.instrument_id, order.instrument_id());
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.venue_order_id, VenueOrderId::from("V-7"));
        assert_eq!(event.account_id, AccountId::from(ACCOUNT_ID));
        assert_eq!(event.trade_id, TradeId::from("T-1"));
        assert_eq!(event.position_id, Some(PositionId::from("P-1")));
        assert_eq!(event.order_side, OrderSide::Sell);
        assert_eq!(event.order_type, OrderType::Limit);
        assert_eq!(event.last_qty, Quantity::from("1.500"));
        assert_eq!(event.last_px, Price::from("2499.50"));
        assert_eq!(event.currency, Currency::USDT());
        assert_eq!(event.liquidity_side, LiquiditySide::Maker);
        assert_eq!(event.commission, Some(commission));
        assert_eq!(event.ts_event, UnixNanos::from(83));
        assert_eq!(event.ts_init, UnixNanos::from(89));
    }

    #[rstest]
    fn test_generate_order_filled_allows_absent_position_and_commission(
        factory: OrderEventFactory,
        order: OrderAny,
    ) {
        let OrderEventAny::Filled(event) = factory.generate_order_filled(
            &order,
            VenueOrderId::from("V-8"),
            None,
            TradeId::from("T-2"),
            Quantity::from("3.000"),
            Price::from("2501.00"),
            Currency::USDT(),
            None,
            LiquiditySide::Taker,
            UnixNanos::from(97),
            UnixNanos::from(101),
        ) else {
            panic!("expected a filled event");
        };

        assert_eq!(event.position_id, None);
        assert_eq!(event.commission, None);
        assert_eq!(event.liquidity_side, LiquiditySide::Taker);
    }

    #[rstest]
    fn test_generated_events_carry_distinct_event_ids(factory: OrderEventFactory, order: OrderAny) {
        let (OrderEventAny::Submitted(first), OrderEventAny::Submitted(second)) = (
            factory.generate_order_submitted(&order, UnixNanos::from(1)),
            factory.generate_order_submitted(&order, UnixNanos::from(1)),
        ) else {
            panic!("expected submitted events");
        };

        assert_ne!(first.event_id, second.event_id);
    }

    #[rstest]
    fn test_factory_clone_preserves_identity(factory: OrderEventFactory) {
        let mut clone = factory.clone();
        clone.set_account_id(AccountId::from("SIM-003"));

        assert_eq!(factory.account_id(), AccountId::from(ACCOUNT_ID));
        assert_eq!(clone.account_id(), AccountId::from("SIM-003"));
        assert_eq!(clone.trader_id(), factory.trader_id());
    }
}
