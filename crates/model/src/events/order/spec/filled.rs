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

use nautilus_core::{UUID4, UnixNanos};

use crate::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::OrderFilled,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    stubs::{TestDefault, test_uuid},
    types::{Currency, Money, Price, Quantity},
};

/// Test-only fluent spec for [`OrderFilled`].
///
/// All fields carry sensible defaults so callers only set what differs.
/// `build()` constructs the event through [`OrderFilled::new`] so any future invariants
/// added to the production constructor are exercised by tests built on this spec.
#[derive(Debug, Clone, bon::Builder)]
#[builder(finish_fn = into_spec)]
pub struct OrderFilledSpec {
    #[builder(default = TraderId::test_default())]
    pub trader_id: TraderId,
    #[builder(default = StrategyId::test_default())]
    pub strategy_id: StrategyId,
    #[builder(default = InstrumentId::test_default())]
    pub instrument_id: InstrumentId,
    #[builder(default = ClientOrderId::test_default())]
    pub client_order_id: ClientOrderId,
    #[builder(default = VenueOrderId::test_default())]
    pub venue_order_id: VenueOrderId,
    #[builder(default = AccountId::test_default())]
    pub account_id: AccountId,
    #[builder(default = TradeId::test_default())]
    pub trade_id: TradeId,
    #[builder(default = OrderSide::Buy)]
    pub order_side: OrderSide,
    #[builder(default = OrderType::Market)]
    pub order_type: OrderType,
    #[builder(default = Quantity::new(100_000.0, 0))]
    pub last_qty: Quantity,
    #[builder(default = Price::from("1.00000"))]
    pub last_px: Price,
    #[builder(default = Currency::USD())]
    pub currency: Currency,
    #[builder(default = LiquiditySide::Taker)]
    pub liquidity_side: LiquiditySide,
    #[builder(default = test_uuid())]
    pub event_id: UUID4,
    #[builder(default = UnixNanos::default())]
    pub ts_event: UnixNanos,
    #[builder(default = UnixNanos::default())]
    pub ts_init: UnixNanos,
    #[builder(default = false)]
    pub reconciliation: bool,
    pub position_id: Option<PositionId>,
    pub commission: Option<Money>,
}

impl<S: order_filled_spec_builder::IsComplete> OrderFilledSpecBuilder<S> {
    /// Builds the spec and constructs an [`OrderFilled`] through its production constructor.
    #[must_use]
    pub fn build(self) -> OrderFilled {
        let spec = self.into_spec();
        OrderFilled::new(
            spec.trader_id,
            spec.strategy_id,
            spec.instrument_id,
            spec.client_order_id,
            spec.venue_order_id,
            spec.account_id,
            spec.trade_id,
            spec.order_side,
            spec.order_type,
            spec.last_qty,
            spec.last_px,
            spec.currency,
            spec.liquidity_side,
            spec.event_id,
            spec.ts_event,
            spec.ts_init,
            spec.reconciliation,
            spec.position_id,
            spec.commission,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stubs::reset_test_uuid_rng;

    #[rstest]
    fn defaults_are_sensible() {
        // Pin the spec's no-arg defaults so accidental drift in any individual default surfaces here,
        // rather than as silent behavior change in downstream tests.
        let order = OrderFilledSpec::builder().build();
        assert_eq!(order.trader_id, TraderId::test_default());
        assert_eq!(order.strategy_id, StrategyId::test_default());
        assert_eq!(order.instrument_id, InstrumentId::test_default());
        assert_eq!(order.client_order_id, ClientOrderId::test_default());
        assert_eq!(order.venue_order_id, VenueOrderId::test_default());
        assert_eq!(order.account_id, AccountId::test_default());
        assert_eq!(order.trade_id, TradeId::test_default());
        assert_eq!(order.order_side, OrderSide::Buy);
        assert_eq!(order.order_type, OrderType::Market);
        assert_eq!(order.last_qty, Quantity::new(100_000.0, 0));
        assert_eq!(order.last_px, Price::from("1.00000"));
        assert_eq!(order.currency, Currency::USD());
        assert_eq!(order.liquidity_side, LiquiditySide::Taker);
        assert_eq!(order.ts_event, UnixNanos::default());
        assert_eq!(order.ts_init, UnixNanos::default());
        assert!(!order.reconciliation);
        assert_eq!(order.position_id, None);
        assert_eq!(order.commission, None);
    }

    #[rstest]
    fn overrides_apply_through_constructor() {
        let order = OrderFilledSpec::builder()
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from("50"))
            .last_px(Price::from("1.25000"))
            .commission(Money::from("0.5 USD"))
            .build();

        assert_eq!(order.order_side, OrderSide::Sell);
        assert_eq!(order.last_qty, Quantity::from("50"));
        assert_eq!(order.last_px, Price::from("1.25000"));
        assert_eq!(order.commission, Some(Money::from("0.5 USD")));
        assert_eq!(order.trader_id, TraderId::test_default());
    }

    #[rstest]
    fn event_ids_are_unique_within_a_run() {
        reset_test_uuid_rng();
        let a = OrderFilledSpec::builder().build();
        let b = OrderFilledSpec::builder().build();
        let c = OrderFilledSpec::builder().build();
        assert_ne!(a.event_id, b.event_id);
        assert_ne!(b.event_id, c.event_id);
        assert_ne!(a.event_id, c.event_id);
    }

    #[rstest]
    fn event_id_sequence_is_reproducible() {
        // Reset before each draw so the comparison is run-order independent.
        reset_test_uuid_rng();
        let first_run: Vec<_> = (0..3)
            .map(|_| OrderFilledSpec::builder().build().event_id)
            .collect();

        reset_test_uuid_rng();
        let second_run: Vec<_> = (0..3)
            .map(|_| OrderFilledSpec::builder().build().event_id)
            .collect();

        assert_eq!(first_run, second_run);
    }
}
