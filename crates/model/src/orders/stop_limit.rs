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

use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
};

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos, correctness::FAILED};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Order, OrderAny, OrderCore, OrderError, check_display_qty, check_time_in_force};
use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::{OrderEventAny, OrderInitialized, OrderUpdated},
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity, quantity::check_positive_quantity},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct StopLimitOrder {
    pub price: Price,
    pub trigger_price: Price,
    pub trigger_type: TriggerType,
    pub expire_time: Option<UnixNanos>,
    pub is_post_only: bool,
    pub display_qty: Option<Quantity>,
    pub trigger_instrument_id: Option<InstrumentId>,
    pub is_triggered: bool,
    pub ts_triggered: Option<UnixNanos>,
    core: OrderCore,
}

impl StopLimitOrder {
    /// Creates a new [`StopLimitOrder`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `quantity` is not positive.
    /// - The `display_qty` (when provided) exceeds `quantity`.
    /// - The `time_in_force` is `GTD` **and** `expire_time` is `None` or zero.
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<Ustr>>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_positive_quantity(quantity, stringify!(quantity))?;
        check_display_qty(display_qty, quantity)?;
        check_time_in_force(time_in_force, expire_time)?;

        let init_order = OrderInitialized::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            OrderType::StopLimit,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            quote_quantity,
            false,
            init_id,
            ts_init,
            ts_init,
            Some(price),
            Some(trigger_price),
            Some(trigger_type),
            None,
            None,
            None,
            expire_time,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        );

        Ok(Self {
            core: OrderCore::new(init_order),
            price,
            trigger_price,
            trigger_type,
            expire_time,
            is_post_only: post_only,
            display_qty,
            trigger_instrument_id,
            is_triggered: false,
            ts_triggered: None,
        })
    }

    /// Creates a new [`StopLimitOrder`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any order validation fails (see [`StopLimitOrder::new_checked`]).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        contingency_type: Option<ContingencyType>,
        order_list_id: Option<OrderListId>,
        linked_order_ids: Option<Vec<ClientOrderId>>,
        parent_order_id: Option<ClientOrderId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        exec_spawn_id: Option<ClientOrderId>,
        tags: Option<Vec<Ustr>>,
        init_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            init_id,
            ts_init,
        )
        .expect(FAILED)
    }
}

impl Deref for StopLimitOrder {
    type Target = OrderCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for StopLimitOrder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl PartialEq for StopLimitOrder {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id == other.client_order_id
    }
}

impl Order for StopLimitOrder {
    fn into_any(self) -> OrderAny {
        OrderAny::StopLimit(self)
    }

    fn status(&self) -> OrderStatus {
        self.status
    }

    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    fn strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    fn symbol(&self) -> Symbol {
        self.instrument_id.symbol
    }

    fn venue(&self) -> Venue {
        self.instrument_id.venue
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        self.venue_order_id
    }

    fn position_id(&self) -> Option<PositionId> {
        self.position_id
    }

    fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    fn last_trade_id(&self) -> Option<TradeId> {
        self.last_trade_id
    }

    fn order_side(&self) -> OrderSide {
        self.side
    }

    fn order_type(&self) -> OrderType {
        self.order_type
    }

    fn quantity(&self) -> Quantity {
        self.quantity
    }

    fn time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    fn expire_time(&self) -> Option<UnixNanos> {
        self.expire_time
    }

    fn price(&self) -> Option<Price> {
        Some(self.price)
    }

    fn trigger_price(&self) -> Option<Price> {
        Some(self.trigger_price)
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        Some(self.trigger_type)
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        self.liquidity_side
    }

    fn is_post_only(&self) -> bool {
        self.is_post_only
    }

    fn is_reduce_only(&self) -> bool {
        self.is_reduce_only
    }

    fn is_quote_quantity(&self) -> bool {
        self.is_quote_quantity
    }

    fn has_price(&self) -> bool {
        true
    }

    fn display_qty(&self) -> Option<Quantity> {
        self.display_qty
    }

    fn limit_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        None
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        self.emulation_trigger
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        self.trigger_instrument_id
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        self.contingency_type
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        self.order_list_id
    }

    fn linked_order_ids(&self) -> Option<&[ClientOrderId]> {
        self.linked_order_ids.as_deref()
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        self.parent_order_id
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        self.exec_algorithm_id
    }

    fn exec_algorithm_params(&self) -> Option<&IndexMap<Ustr, Ustr>> {
        self.exec_algorithm_params.as_ref()
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        self.exec_spawn_id
    }

    fn tags(&self) -> Option<&[Ustr]> {
        self.tags.as_deref()
    }

    fn filled_qty(&self) -> Quantity {
        self.filled_qty
    }

    fn leaves_qty(&self) -> Quantity {
        self.leaves_qty
    }

    fn avg_px(&self) -> Option<f64> {
        self.avg_px
    }

    fn slippage(&self) -> Option<f64> {
        self.slippage
    }

    fn init_id(&self) -> UUID4 {
        self.init_id
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    fn ts_submitted(&self) -> Option<UnixNanos> {
        self.ts_submitted
    }

    fn ts_accepted(&self) -> Option<UnixNanos> {
        self.ts_accepted
    }

    fn ts_closed(&self) -> Option<UnixNanos> {
        self.ts_closed
    }

    fn ts_last(&self) -> UnixNanos {
        self.ts_last
    }

    fn events(&self) -> Vec<&OrderEventAny> {
        self.events.iter().collect()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId> {
        self.venue_order_ids.iter().collect()
    }

    fn commissions(&self) -> &IndexMap<Currency, Money> {
        &self.commissions
    }

    fn trade_ids(&self) -> Vec<&TradeId> {
        self.trade_ids.iter().collect()
    }

    fn apply(&mut self, event: OrderEventAny) -> Result<(), OrderError> {
        if let OrderEventAny::Updated(ref event) = event {
            self.update(event);
        };

        let is_order_filled = matches!(event, OrderEventAny::Filled(_));
        let is_order_triggered = matches!(event, OrderEventAny::Triggered(_));
        let ts_event = if is_order_triggered {
            Some(event.ts_event())
        } else {
            None
        };

        self.core.apply(event)?;

        if is_order_triggered {
            self.is_triggered = true;
            self.ts_triggered = ts_event;
        }

        if is_order_filled {
            self.core.set_slippage(self.price);
        };

        Ok(())
    }

    fn update(&mut self, event: &OrderUpdated) {
        self.quantity = event.quantity;

        if let Some(price) = event.price {
            self.price = price;
        }

        if let Some(trigger_price) = event.trigger_price {
            self.trigger_price = trigger_price;
        }

        self.quantity = event.quantity;
        self.leaves_qty = self.quantity.saturating_sub(self.filled_qty);
    }

    fn is_triggered(&self) -> Option<bool> {
        Some(self.is_triggered)
    }

    fn set_position_id(&mut self, position_id: Option<PositionId>) {
        self.position_id = position_id;
    }

    fn set_quantity(&mut self, quantity: Quantity) {
        self.quantity = quantity;
    }

    fn set_leaves_qty(&mut self, leaves_qty: Quantity) {
        self.leaves_qty = leaves_qty;
    }

    fn set_emulation_trigger(&mut self, emulation_trigger: Option<TriggerType>) {
        self.emulation_trigger = emulation_trigger;
    }

    fn set_is_quote_quantity(&mut self, is_quote_quantity: bool) {
        self.is_quote_quantity = is_quote_quantity;
    }

    fn set_liquidity_side(&mut self, liquidity_side: LiquiditySide) {
        self.liquidity_side = Some(liquidity_side);
    }

    fn would_reduce_only(&self, side: PositionSide, position_qty: Quantity) -> bool {
        self.core.would_reduce_only(side, position_qty)
    }

    fn previous_status(&self) -> Option<OrderStatus> {
        self.core.previous_status
    }
}

impl From<OrderInitialized> for StopLimitOrder {
    fn from(event: OrderInitialized) -> Self {
        Self::new(
            event.trader_id,
            event.strategy_id,
            event.instrument_id,
            event.client_order_id,
            event.order_side,
            event.quantity,
            event.price.expect("`price` was None for StopLimitOrder"),
            event
                .trigger_price
                .expect("`trigger_price` was None for StopLimitOrder"),
            event
                .trigger_type
                .expect("`trigger_type` was None for StopLimitOrder"),
            event.time_in_force,
            event.expire_time,
            event.post_only,
            event.reduce_only,
            event.quote_quantity,
            event.display_qty,
            event.emulation_trigger,
            event.trigger_instrument_id,
            event.contingency_type,
            event.order_list_id,
            event.linked_order_ids,
            event.parent_order_id,
            event.exec_algorithm_id,
            event.exec_algorithm_params,
            event.exec_spawn_id,
            event.tags,
            event.event_id,
            event.ts_event,
        )
    }
}

impl Display for StopLimitOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StopLimitOrder({} {} {} {} @ {}-STOP[{}] {}-LIMIT {}, status={}, client_order_id={}, venue_order_id={}, position_id={}, tags={})",
            self.side,
            self.quantity.to_formatted_string(),
            self.instrument_id,
            self.order_type,
            self.trigger_price,
            self.trigger_type,
            self.price,
            self.time_in_force,
            self.status,
            self.client_order_id,
            self.venue_order_id
                .map_or("None".to_string(), |venue_order_id| format!(
                    "{venue_order_id}"
                )),
            self.position_id
                .map_or("None".to_string(), |position_id| format!("{position_id}")),
            self.tags.clone().map_or("None".to_string(), |tags| tags
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
                .join(", ")),
        )
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::{
        enums::{OrderSide, TimeInForce, TriggerType},
        events::order::initialized::OrderInitializedBuilder,
        identifiers::InstrumentId,
        instruments::{CurrencyPair, stubs::*},
        orders::{OrderTestBuilder, stubs::TestOrderStubs},
        types::{Price, Quantity},
    };

    #[rstest]
    fn test_initialize(_audusd_sim: CurrencyPair) {
        // ---------------------------------------------------------------------
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(_audusd_sim.id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("0.68000"))
            .price(Price::from("0.68100"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        assert_eq!(order.trigger_price(), Some(Price::from("0.68000")));
        assert_eq!(order.price(), Some(Price::from("0.68100")));

        assert_eq!(order.time_in_force(), TimeInForce::Gtc);

        assert_eq!(order.is_triggered(), Some(false));
        assert_eq!(order.filled_qty(), Quantity::from(0));
        assert_eq!(order.leaves_qty(), Quantity::from(1));

        assert_eq!(order.display_qty(), None);
        assert_eq!(order.trigger_instrument_id(), None);
        assert_eq!(order.order_list_id(), None);
    }

    #[rstest]
    fn test_display(audusd_sim: CurrencyPair) {
        let order = OrderTestBuilder::new(OrderType::MarketToLimit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(1))
            .build();

        assert_eq!(
            order.to_string(),
            "MarketToLimitOrder(BUY 1 AUD/USD.SIM MARKET_TO_LIMIT GTC, status=INITIALIZED, client_order_id=O-19700101-000000-001-001-1, venue_order_id=None, position_id=None, exec_algorithm_id=None, exec_spawn_id=None, tags=None)"
        );
    }

    #[rstest]
    #[should_panic]
    fn test_display_qty_gt_quantity_err(audusd_sim: CurrencyPair) {
        OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("30300"))
            .price(Price::from("30100"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .display_qty(Quantity::from(2))
            .build();
    }

    #[rstest]
    #[should_panic]
    fn test_display_qty_negative_err(audusd_sim: CurrencyPair) {
        OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("30300"))
            .price(Price::from("30100"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .display_qty(Quantity::from("-1"))
            .build();
    }

    #[rstest]
    #[should_panic]
    fn test_gtd_without_expire_time_err(audusd_sim: CurrencyPair) {
        OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("30300"))
            .price(Price::from("30100"))
            .trigger_type(TriggerType::LastPrice)
            .time_in_force(TimeInForce::Gtd)
            .quantity(Quantity::from(1))
            .build();
    }
    #[rstest]
    fn test_stop_limit_order_update() {
        // Create and accept a basic stop limit order
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .build();

        let mut accepted_order = TestOrderStubs::make_accepted_order(&order);

        // Update with new values
        let updated_price = Price::new(105.0, 2);
        let updated_trigger_price = Price::new(90.0, 2);
        let updated_quantity = Quantity::from(5);

        let event = OrderUpdated {
            client_order_id: accepted_order.client_order_id(),
            strategy_id: accepted_order.strategy_id(),
            price: Some(updated_price),
            trigger_price: Some(updated_trigger_price),
            quantity: updated_quantity,
            ..Default::default()
        };

        accepted_order.apply(OrderEventAny::Updated(event)).unwrap();

        // Verify updates were applied correctly
        assert_eq!(accepted_order.quantity(), updated_quantity);
        assert_eq!(accepted_order.price(), Some(updated_price));
        assert_eq!(accepted_order.trigger_price(), Some(updated_trigger_price));
    }

    #[rstest]
    fn test_stop_limit_order_expire_time() {
        // Create a stop limit order with an expire time
        let expire_time = UnixNanos::from(1234567890);
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .expire_time(expire_time)
            .build();

        // Assert that the expire time is set correctly
        assert_eq!(order.expire_time(), Some(expire_time));
    }

    #[rstest]
    fn test_stop_limit_order_post_only() {
        // Create a stop limit order with post_only flag set to true
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .post_only(true)
            .build();

        // Assert that post_only is set correctly
        assert!(order.is_post_only());
    }

    #[rstest]
    fn test_stop_limit_order_reduce_only() {
        // Create a stop limit order with reduce_only flag set to true
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .reduce_only(true)
            .build();

        // Assert that reduce_only is set correctly
        assert!(order.is_reduce_only());
    }

    #[rstest]
    fn test_stop_limit_order_trigger_instrument_id() {
        // Create a stop limit order with a trigger instrument ID
        let trigger_instrument_id = InstrumentId::from("ETH-USDT.BINANCE");
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .trigger_instrument_id(trigger_instrument_id)
            .build();

        // Assert that the trigger instrument ID is set correctly
        assert_eq!(order.trigger_instrument_id(), Some(trigger_instrument_id));
    }

    #[rstest]
    fn test_stop_limit_order_would_reduce_only() {
        // Create a stop limit order with a sell side
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .build();

        // Test would_reduce_only functionality
        assert!(order.would_reduce_only(PositionSide::Long, Quantity::from(15)));
        assert!(!order.would_reduce_only(PositionSide::Short, Quantity::from(15)));
        assert!(!order.would_reduce_only(PositionSide::Long, Quantity::from(5)));
    }

    #[rstest]
    fn test_stop_limit_order_display_string() {
        // Create a stop limit order
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .client_order_id(ClientOrderId::from("ORDER-001"))
            .build();

        // Expected string representation - updated to match the actual format
        let expected = "StopLimitOrder(BUY 10 BTC-USDT.BINANCE STOP_LIMIT @ 95.00-STOP[DEFAULT] 100.00-LIMIT GTC, status=INITIALIZED, client_order_id=ORDER-001, venue_order_id=None, position_id=None, tags=None)";

        // Assert string representations match
        assert_eq!(order.to_string(), expected);
        assert_eq!(format!("{order}"), expected);
    }

    #[rstest]
    fn test_stop_limit_order_from_order_initialized() {
        // Create an OrderInitialized event with all required fields for a StopLimitOrder
        let order_initialized = OrderInitializedBuilder::default()
            .order_type(OrderType::StopLimit)
            .quantity(Quantity::from(10))
            .price(Some(Price::new(100.0, 2)))
            .trigger_price(Some(Price::new(95.0, 2)))
            .trigger_type(Some(TriggerType::Default))
            .post_only(true)
            .reduce_only(true)
            .expire_time(Some(UnixNanos::from(1234567890)))
            .display_qty(Some(Quantity::from(5)))
            .build()
            .unwrap();

        // Convert the OrderInitialized event into a StopLimitOrder
        let order: StopLimitOrder = order_initialized.clone().into();

        // Assert essential fields match the OrderInitialized fields
        assert_eq!(order.trader_id(), order_initialized.trader_id);
        assert_eq!(order.strategy_id(), order_initialized.strategy_id);
        assert_eq!(order.instrument_id(), order_initialized.instrument_id);
        assert_eq!(order.client_order_id(), order_initialized.client_order_id);
        assert_eq!(order.order_side(), order_initialized.order_side);
        assert_eq!(order.quantity(), order_initialized.quantity);

        // Assert specific fields for StopLimitOrder
        assert_eq!(order.price, order_initialized.price.unwrap());
        assert_eq!(
            order.trigger_price,
            order_initialized.trigger_price.unwrap()
        );
        assert_eq!(order.trigger_type, order_initialized.trigger_type.unwrap());
        assert_eq!(order.expire_time(), order_initialized.expire_time);
        assert_eq!(order.is_post_only(), order_initialized.post_only);
        assert_eq!(order.is_reduce_only(), order_initialized.reduce_only);
        assert_eq!(order.display_qty(), order_initialized.display_qty);

        // Verify order type
        assert_eq!(order.order_type(), OrderType::StopLimit);

        // Verify not triggered by default
        assert_eq!(order.is_triggered(), Some(false));
    }
}
