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

use super::{Order, OrderAny, OrderCore, check_display_qty, check_time_in_force};
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
    orders::OrderError,
    types::{Currency, Money, Price, Quantity, quantity::check_positive_quantity},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct LimitIfTouchedOrder {
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

#[allow(clippy::too_many_arguments)]
impl LimitIfTouchedOrder {
    /// Creates a new [`LimitIfTouchedOrder`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The `quantity` is not positive.
    /// - The `display_qty` (when provided) exceeds `quantity`.
    /// - The `time_in_force` is GTD and the `expire_time` is `None` or zero.
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

        match order_side {
            OrderSide::Buy if trigger_price > price => {
                anyhow::bail!("BUY Limit-If-Touched must have `trigger_price` <= `price`")
            }
            OrderSide::Sell if trigger_price < price => {
                anyhow::bail!("SELL Limit-If-Touched must have `trigger_price` >= `price`")
            }
            _ => {}
        }

        let init_order = OrderInitialized::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            OrderType::LimitIfTouched,
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
            price,
            trigger_price,
            trigger_type,
            expire_time,
            is_post_only: post_only,
            display_qty,
            trigger_instrument_id,
            is_triggered: false,
            ts_triggered: None,
            core: OrderCore::new(init_order),
        })
    }

    /// Creates a new [`LimitIfTouchedOrder`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any order validation fails (see [`LimitIfTouchedOrder::new_checked`]).
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

impl Deref for LimitIfTouchedOrder {
    type Target = OrderCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for LimitIfTouchedOrder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Order for LimitIfTouchedOrder {
    fn into_any(self) -> OrderAny {
        OrderAny::LimitIfTouched(self)
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

    fn commissions(&self) -> &IndexMap<Currency, Money> {
        &self.commissions
    }

    fn events(&self) -> Vec<&OrderEventAny> {
        self.events.iter().collect()
    }

    fn venue_order_ids(&self) -> Vec<&VenueOrderId> {
        self.venue_order_ids.iter().collect()
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

impl Display for LimitIfTouchedOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LimitIfTouchedOrder({} {} {} @ {} / trigger {} ({:?}) {}, status={})",
            self.side,
            self.quantity.to_formatted_string(),
            self.instrument_id,
            self.price,
            self.trigger_price,
            self.trigger_type,
            self.time_in_force,
            self.status
        )
    }
}

impl From<OrderInitialized> for LimitIfTouchedOrder {
    fn from(event: OrderInitialized) -> Self {
        Self::new(
            event.trader_id,
            event.strategy_id,
            event.instrument_id,
            event.client_order_id,
            event.order_side,
            event.quantity,
            event
                .price // TODO: Improve this error, model order domain errors
                .expect("Error initializing order: `price` was `None` for `LimitIfTouchedOrder"),
            event
                .trigger_price // TODO: Improve this error, model order domain errors
                .expect(
                    "Error initializing order: `trigger_price` was `None` for `LimitIfTouchedOrder",
                ),
            event
                .trigger_type
                .expect("Error initializing order: `trigger_type` was `None`"),
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        enums::{TimeInForce, TriggerType},
        events::order::{filled::OrderFilledBuilder, initialized::OrderInitializedBuilder},
        identifiers::InstrumentId,
        instruments::{CurrencyPair, stubs::*},
        orders::{builder::OrderTestBuilder, stubs::TestOrderStubs},
        types::{Price, Quantity},
    };

    #[rstest]
    fn test_initialize(_audusd_sim: CurrencyPair) {
        let order = OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(_audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("0.68000"))
            .trigger_price(Price::from("0.68000"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        assert_eq!(order.trigger_price(), Some(Price::from("0.68000")));
        assert_eq!(order.price(), Some(Price::from("0.68000")));

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
        let order = OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("30200"))
            .price(Price::from("30200"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();

        assert_eq!(
            order.to_string(),
            "LimitIfTouchedOrder(BUY 1 AUD/USD.SIM @ 30200 / trigger 30200 (LastPrice) GTC, status=INITIALIZED)"
        );
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid `Quantity` for 'quantity' not positive, was 0"
    )]
    fn test_quantity_zero(audusd_sim: CurrencyPair) {
        let _ = OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("30000"))
            .trigger_price(Price::from("30200"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(0))
            .build();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `expire_time` is required for `GTD` order")]
    fn test_gtd_without_expire(audusd_sim: CurrencyPair) {
        let _ = OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("30000"))
            .trigger_price(Price::from("30200"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .time_in_force(TimeInForce::Gtd)
            .build();
    }

    #[rstest]
    #[should_panic(expected = "BUY Limit-If-Touched must have `trigger_price` <= `price`")]
    fn test_buy_trigger_gt_price(audusd_sim: CurrencyPair) {
        OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("30300")) // Invalid trigger > price
            .price(Price::from("30200"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();
    }

    #[rstest]
    #[should_panic(expected = "SELL Limit-If-Touched must have `trigger_price` >= `price`")]
    fn test_sell_trigger_lt_price(audusd_sim: CurrencyPair) {
        OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Sell)
            .trigger_price(Price::from("30100")) // Invalid trigger < price
            .price(Price::from("30200"))
            .trigger_type(TriggerType::LastPrice)
            .quantity(Quantity::from(1))
            .build();
    }

    #[rstest]
    fn test_limit_if_touched_order_update() {
        // Create and accept a basic limit-if-touched order
        let order = OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .price(Price::new(100.0, 2))
            .trigger_price(Price::new(95.0, 2))
            .trigger_type(TriggerType::Default)
            .build();

        let mut accepted_order = TestOrderStubs::make_accepted_order(&order);

        // Update with new values
        let updated_price = Price::new(105.0, 2);
        let updated_trigger_price = Price::new(97.0, 2);
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
        assert_eq!(accepted_order.price(), Some(updated_price));
        assert_eq!(accepted_order.trigger_price(), Some(updated_trigger_price));
        assert_eq!(accepted_order.quantity(), updated_quantity);
    }

    #[rstest]
    fn test_limit_if_touched_order_from_order_initialized() {
        // Create an OrderInitialized event with all required fields for a LimitIfTouchedOrder
        let order_initialized = OrderInitializedBuilder::default()
            .price(Some(Price::new(100.0, 2)))
            .trigger_price(Some(Price::new(95.0, 2)))
            .trigger_type(Some(TriggerType::Default))
            .order_type(OrderType::LimitIfTouched)
            .build()
            .unwrap();

        // Convert the OrderInitialized event into a LimitIfTouchedOrder
        let order: LimitIfTouchedOrder = order_initialized.clone().into();

        // Assert essential fields match the OrderInitialized fields
        assert_eq!(order.trader_id(), order_initialized.trader_id);
        assert_eq!(order.strategy_id(), order_initialized.strategy_id);
        assert_eq!(order.instrument_id(), order_initialized.instrument_id);
        assert_eq!(order.client_order_id(), order_initialized.client_order_id);
        assert_eq!(order.order_side(), order_initialized.order_side);
        assert_eq!(order.quantity(), order_initialized.quantity);

        // Assert specific fields for LimitIfTouchedOrder
        assert_eq!(order.price, order_initialized.price.unwrap());
        assert_eq!(
            order.trigger_price,
            order_initialized.trigger_price.unwrap()
        );
        assert_eq!(order.trigger_type, order_initialized.trigger_type.unwrap());
    }

    #[rstest]
    fn test_limit_if_touched_order_sets_slippage_when_filled() {
        // Create a limit-if-touched order
        let order = OrderTestBuilder::new(OrderType::LimitIfTouched)
            .instrument_id(InstrumentId::from("BTC-USDT.BINANCE"))
            .quantity(Quantity::from(10))
            .side(OrderSide::Buy) // Explicitly setting Buy side
            .price(Price::new(95.0, 2)) // Limit price
            .trigger_price(Price::new(90.0, 2)) // Trigger price LOWER than fill price
            .trigger_type(TriggerType::Default)
            .build();

        // Accept the order first
        let mut accepted_order = TestOrderStubs::make_accepted_order(&order);

        // Create a filled event with the correct quantity
        let fill_quantity = accepted_order.quantity(); // Use the same quantity as the order
        let fill_price = Price::new(98.50, 2); // Use a price LOWER than limit price

        let order_filled_event = OrderFilledBuilder::default()
            .client_order_id(accepted_order.client_order_id())
            .strategy_id(accepted_order.strategy_id())
            .instrument_id(accepted_order.instrument_id())
            .order_side(accepted_order.order_side())
            .last_qty(fill_quantity)
            .last_px(fill_price)
            .venue_order_id(VenueOrderId::from("TEST-001"))
            .trade_id(TradeId::from("TRADE-001"))
            .build()
            .unwrap();

        // Apply the fill event
        accepted_order
            .apply(OrderEventAny::Filled(order_filled_event))
            .unwrap();

        // The slippage calculation should be triggered by the filled event
        print!("Slippageee: {:?}", accepted_order.slippage());
        assert!(accepted_order.slippage().is_some());

        // We can also check the actual slippage value
        let expected_slippage = 98.50 - 95.0;
        let actual_slippage = accepted_order.slippage().unwrap();

        assert!(
            (actual_slippage - expected_slippage).abs() < 0.001,
            "Expected slippage around {expected_slippage}, was {actual_slippage}"
        );
    }
}
