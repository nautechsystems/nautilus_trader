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

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal_macros::dec;

use super::{
    any::OrderAny, limit::LimitOrder, limit_if_touched::LimitIfTouchedOrder, market::MarketOrder,
    market_if_touched::MarketIfTouchedOrder, market_to_limit::MarketToLimitOrder,
    stop_limit::StopLimitOrder, stop_market::StopMarketOrder,
    trailing_stop_limit::TrailingStopLimitOrder, trailing_stop_market::TrailingStopMarketOrder,
};
use crate::{
    enums::{LiquiditySide, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    events::{
        OrderEventAny,
        order::spec::{OrderAcceptedSpec, OrderCanceledSpec, OrderFilledSpec, OrderSubmittedSpec},
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderTestBuilder},
    stubs::TestDefault,
    types::{Money, Price, Quantity},
};

impl TestDefault for LimitOrder {
    /// Creates a new test default [`LimitOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl TestDefault for LimitIfTouchedOrder {
    /// Creates a new test default [`LimitIfTouchedOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl TestDefault for MarketOrder {
    /// Creates a new test default [`MarketOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            TimeInForce::Day,
            UUID4::default(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }
}

impl TestDefault for MarketIfTouchedOrder {
    /// Creates a new test default [`MarketIfTouchedOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl TestDefault for MarketToLimitOrder {
    /// Creates a new test default [`MarketToLimitOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl TestDefault for StopLimitOrder {
    /// Creates a new test default [`StopLimitOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl TestDefault for StopMarketOrder {
    /// Creates a new test default [`StopMarketOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl TestDefault for TrailingStopLimitOrder {
    /// Creates a new test default [`TrailingStopLimitOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            Price::from("1.00000"),
            Price::from("1.00000"),
            TriggerType::BidAsk,
            dec!(0.001),
            dec!(0.001),
            TrailingOffsetType::Price,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

impl TestDefault for TrailingStopMarketOrder {
    /// Creates a new test default [`TrailingStopMarketOrder`] instance.
    fn test_default() -> Self {
        Self::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            InstrumentId::test_default(),
            ClientOrderId::test_default(),
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            Price::from("1.00000"),
            TriggerType::BidAsk,
            dec!(0.001),
            TrailingOffsetType::Price,
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::default(),
        )
    }
}

#[derive(Debug)]
pub struct TestOrderEventStubs;

impl TestOrderEventStubs {
    #[must_use]
    pub fn submitted(order: &OrderAny, account_id: AccountId) -> OrderEventAny {
        let event = OrderSubmittedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(account_id)
            .build();
        OrderEventAny::Submitted(event)
    }

    #[must_use]
    pub fn accepted(
        order: &OrderAny,
        account_id: AccountId,
        venue_order_id: VenueOrderId,
    ) -> OrderEventAny {
        let event = OrderAcceptedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .build();
        OrderEventAny::Accepted(event)
    }

    #[must_use]
    pub fn canceled(
        order: &OrderAny,
        account_id: AccountId,
        venue_order_id: Option<VenueOrderId>,
    ) -> OrderEventAny {
        let event = OrderCanceledSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(account_id)
            .maybe_venue_order_id(venue_order_id)
            .build();
        OrderEventAny::Canceled(event)
    }

    /// # Panics
    ///
    /// Panics if parsing the fallback price string fails or unwrapping default values fails.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn filled(
        order: &OrderAny,
        instrument: &InstrumentAny,
        trade_id: Option<TradeId>,
        position_id: Option<PositionId>,
        last_px: Option<Price>,
        last_qty: Option<Quantity>,
        liquidity_side: Option<LiquiditySide>,
        commission: Option<Money>,
        ts_filled_ns: Option<UnixNanos>,
        account_id: Option<AccountId>,
    ) -> OrderEventAny {
        let mut builder = OrderFilledTestBuilder::new(order, instrument);

        if let Some(trade_id) = trade_id {
            builder.trade_id(trade_id);
        }

        if let Some(position_id) = position_id {
            builder.position_id(position_id);
        }

        if let Some(last_px) = last_px {
            builder.last_px(last_px);
        }

        if let Some(last_qty) = last_qty {
            builder.last_qty(last_qty);
        }

        if let Some(liquidity_side) = liquidity_side {
            builder.liquidity_side(liquidity_side);
        }

        if let Some(commission) = commission {
            builder.commission(commission);
        }

        if let Some(ts_event) = ts_filled_ns {
            builder.ts_event(ts_event);
        }

        if let Some(account_id) = account_id {
            builder.account_id(account_id);
        }

        builder.build()
    }
}

/// Fluent test builder for a [`crate::events::OrderFilled`] event derived from an order and instrument.
#[derive(Debug)]
pub struct OrderFilledTestBuilder<'a> {
    order: &'a OrderAny,
    instrument: &'a InstrumentAny,
    trade_id: Option<TradeId>,
    position_id: Option<PositionId>,
    last_px: Option<Price>,
    last_qty: Option<Quantity>,
    liquidity_side: Option<LiquiditySide>,
    commission: Option<Money>,
    ts_event: Option<UnixNanos>,
    account_id: Option<AccountId>,
    without_position_id: bool,
    without_commission: bool,
}

impl<'a> OrderFilledTestBuilder<'a> {
    /// Creates an order-derived [`crate::events::OrderFilled`] test builder.
    #[must_use]
    pub fn new(order: &'a OrderAny, instrument: &'a InstrumentAny) -> Self {
        Self {
            order,
            instrument,
            trade_id: None,
            position_id: None,
            last_px: None,
            last_qty: None,
            liquidity_side: None,
            commission: None,
            ts_event: None,
            account_id: None,
            without_position_id: false,
            without_commission: false,
        }
    }

    /// Sets the trade ID.
    pub fn trade_id(&mut self, trade_id: TradeId) -> &mut Self {
        self.trade_id = Some(trade_id);
        self
    }

    /// Sets the position ID.
    pub fn position_id(&mut self, position_id: PositionId) -> &mut Self {
        self.position_id = Some(position_id);
        self.without_position_id = false;
        self
    }

    /// Omits the position ID.
    pub fn without_position_id(&mut self) -> &mut Self {
        self.without_position_id = true;
        self
    }

    /// Sets the fill price.
    pub fn last_px(&mut self, last_px: Price) -> &mut Self {
        self.last_px = Some(last_px);
        self
    }

    /// Sets the fill quantity.
    pub fn last_qty(&mut self, last_qty: Quantity) -> &mut Self {
        self.last_qty = Some(last_qty);
        self
    }

    /// Sets the liquidity side.
    pub fn liquidity_side(&mut self, liquidity_side: LiquiditySide) -> &mut Self {
        self.liquidity_side = Some(liquidity_side);
        self
    }

    /// Sets the commission.
    pub fn commission(&mut self, commission: Money) -> &mut Self {
        self.commission = Some(commission);
        self.without_commission = false;
        self
    }

    /// Omits the commission.
    pub fn without_commission(&mut self) -> &mut Self {
        self.without_commission = true;
        self
    }

    /// Sets the event timestamp.
    pub fn ts_event(&mut self, ts_event: UnixNanos) -> &mut Self {
        self.ts_event = Some(ts_event);
        self
    }

    /// Sets the account ID.
    pub fn account_id(&mut self, account_id: AccountId) -> &mut Self {
        self.account_id = Some(account_id);
        self
    }

    /// Builds the [`OrderEventAny::Filled`] event.
    #[must_use]
    pub fn build(&self) -> OrderEventAny {
        let venue_order_id = self
            .order
            .venue_order_id()
            .unwrap_or_else(VenueOrderId::test_default);
        let account_id = self
            .account_id
            .or(self.order.account_id())
            .unwrap_or_else(AccountId::test_default);
        let trade_id = self.trade_id.unwrap_or_else(|| {
            TradeId::new(
                self.order
                    .client_order_id()
                    .as_str()
                    .replace('O', "E")
                    .as_str(),
            )
        });
        let position_id = (!self.without_position_id).then(|| {
            self.position_id
                .or(self.order.position_id())
                .unwrap_or(PositionId::new("1"))
        });
        let commission =
            (!self.without_commission).then(|| self.commission.unwrap_or(Money::from("2 USD")));
        let event = OrderFilledSpec::builder()
            .trader_id(self.order.trader_id())
            .strategy_id(self.order.strategy_id())
            .instrument_id(self.instrument.id())
            .client_order_id(self.order.client_order_id())
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .trade_id(trade_id)
            .order_side(self.order.order_side())
            .order_type(self.order.order_type())
            .last_qty(self.last_qty.unwrap_or(self.order.quantity()))
            .last_px(self.last_px.unwrap_or(Price::from("1.0")))
            .currency(self.instrument.quote_currency())
            .liquidity_side(self.liquidity_side.unwrap_or(LiquiditySide::Maker))
            .ts_event(self.ts_event.unwrap_or_default())
            .maybe_position_id(position_id)
            .maybe_commission(commission)
            .build();
        OrderEventAny::Filled(event)
    }
}

#[derive(Debug)]
pub struct TestOrderStubs;

impl TestOrderStubs {
    /// # Panics
    ///
    /// Panics if applying the accepted event via `new_order.apply(...)` fails.
    #[must_use]
    pub fn make_accepted_order(order: &OrderAny) -> OrderAny {
        let mut new_order = order.clone();
        let accepted_event = TestOrderEventStubs::accepted(
            &new_order,
            AccountId::from("SIM-001"),
            VenueOrderId::from("V-001"),
        );
        new_order.apply(accepted_event).unwrap();
        new_order
    }

    /// # Panics
    ///
    /// Panics if applying the filled event via `accepted_order.apply(...)` fails.
    #[must_use]
    pub fn make_filled_order(
        order: &OrderAny,
        instrument: &InstrumentAny,
        liquidity_side: LiquiditySide,
    ) -> OrderAny {
        let mut accepted_order = Self::make_accepted_order(order);
        let fill = OrderFilledTestBuilder::new(&accepted_order, instrument)
            .liquidity_side(liquidity_side)
            .build();
        accepted_order.apply(fill).unwrap();
        accepted_order
    }
}

#[derive(Debug)]
pub struct TestOrdersGenerator {
    order_type: OrderType,
    venue_instruments: HashMap<Venue, u32>,
    orders_per_instrument: u32,
}

impl TestOrdersGenerator {
    #[must_use]
    pub fn new(order_type: OrderType) -> Self {
        Self {
            order_type,
            venue_instruments: HashMap::new(),
            orders_per_instrument: 5,
        }
    }

    pub fn set_orders_per_instrument(&mut self, total_orders: u32) {
        self.orders_per_instrument = total_orders;
    }

    pub fn add_venue_and_total_instruments(&mut self, venue: Venue, total_instruments: u32) {
        self.venue_instruments.insert(venue, total_instruments);
    }

    fn generate_order(&self, instrument_id: InstrumentId, client_order_id_index: u32) -> OrderAny {
        let client_order_id =
            ClientOrderId::from(format!("O-{instrument_id}-{client_order_id_index}"));
        OrderTestBuilder::new(self.order_type)
            .quantity(Quantity::from("1"))
            .price(Price::from("1"))
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .build()
    }

    #[must_use]
    pub fn build(&self) -> Vec<OrderAny> {
        let mut orders = Vec::new();

        for (venue, total_instruments) in &self.venue_instruments {
            for i in 0..*total_instruments {
                let instrument_id = InstrumentId::from(format!("SYMBOL-{i}.{venue}"));
                for order_index in 0..self.orders_per_instrument {
                    let order = self.generate_order(instrument_id, order_index);
                    orders.push(order);
                }
            }
        }
        orders
    }
}

#[must_use]
pub fn create_order_list_sample(
    total_venues: u8,
    total_instruments: u32,
    orders_per_instrument: u32,
) -> Vec<OrderAny> {
    // Create Limit orders list from order generator with spec:
    // x venues * x instruments * x orders per instrument
    let mut order_generator = TestOrdersGenerator::new(OrderType::Limit);

    for i in 0..total_venues {
        let venue = Venue::from(format!("VENUE-{i}"));
        order_generator.add_venue_and_total_instruments(venue, total_instruments);
    }
    order_generator.set_orders_per_instrument(orders_per_instrument);

    order_generator.build()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::instruments::stubs::audusd_sim;

    #[rstest]
    fn preserves_legacy_fill_defaults() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .quantity(Quantity::from(1))
            .build();
        let OrderEventAny::Filled(fill) = OrderFilledTestBuilder::new(&order, &instrument).build()
        else {
            panic!("expected OrderFilled event");
        };

        assert_eq!(fill.position_id, Some(PositionId::new("1")));
        assert_eq!(fill.commission, Some(Money::from("2 USD")));
        assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
    }

    #[rstest]
    fn can_omit_position_id_and_commission() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .quantity(Quantity::from(1))
            .build();
        let OrderEventAny::Filled(fill) = OrderFilledTestBuilder::new(&order, &instrument)
            .without_position_id()
            .without_commission()
            .build()
        else {
            panic!("expected OrderFilled event");
        };

        assert_eq!(fill.position_id, None);
        assert_eq!(fill.commission, None);
    }
}
