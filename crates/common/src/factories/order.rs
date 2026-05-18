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

//! Factory for constructing order objects.

use std::{cell::RefCell, rc::Rc};

use indexmap::IndexMap;
use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{check_equal, check_slice_not_empty},
};
use nautilus_model::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    orders::{
        LimitIfTouchedOrder, LimitOrder, MarketIfTouchedOrder, MarketOrder, Order, OrderAny,
        OrderList, StopLimitOrder, StopMarketOrder, TrailingStopLimitOrder,
        TrailingStopMarketOrder,
    },
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    clock::Clock,
    generators::{client_order_id::ClientOrderIdGenerator, order_list_id::OrderListIdGenerator},
};

#[derive(Debug)]
pub struct OrderFactory {
    clock: Rc<RefCell<dyn Clock>>,
    trader_id: TraderId,
    strategy_id: StrategyId,
    order_id_generator: ClientOrderIdGenerator,
    order_list_id_generator: OrderListIdGenerator,
}

#[bon::bon]
impl OrderFactory {
    /// Creates a new [`OrderFactory`] instance.
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        init_order_id_count: Option<usize>,
        init_order_list_id_count: Option<usize>,
        clock: Rc<RefCell<dyn Clock>>,
        use_uuids_for_client_order_ids: bool,
        use_hyphens_in_client_order_ids: bool,
    ) -> Self {
        let order_id_generator = ClientOrderIdGenerator::new(
            trader_id,
            strategy_id,
            init_order_id_count.unwrap_or(0),
            clock.clone(),
            use_uuids_for_client_order_ids,
            use_hyphens_in_client_order_ids,
        );

        let order_list_id_generator = OrderListIdGenerator::new(
            trader_id,
            strategy_id,
            init_order_list_id_count.unwrap_or(0),
            clock.clone(),
        );

        Self {
            clock,
            trader_id,
            strategy_id,
            order_id_generator,
            order_list_id_generator,
        }
    }

    /// Sets the client order ID generator count.
    pub const fn set_client_order_id_count(&mut self, count: usize) {
        self.order_id_generator.set_count(count);
    }

    /// Sets the order list ID generator count.
    pub const fn set_order_list_id_count(&mut self, count: usize) {
        self.order_list_id_generator.set_count(count);
    }

    /// Generates a new client order ID.
    pub fn generate_client_order_id(&mut self) -> ClientOrderId {
        self.order_id_generator.generate()
    }

    /// Generates a new order list ID.
    pub fn generate_order_list_id(&mut self) -> OrderListId {
        self.order_list_id_generator.generate()
    }

    /// Resets the factory by resetting all ID generators.
    pub const fn reset_factory(&mut self) {
        self.order_id_generator.reset();
        self.order_list_id_generator.reset();
    }

    /// Creates a new market order.
    #[expect(clippy::too_many_arguments)]
    pub fn market(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let exec_spawn_id: Option<ClientOrderId> = if exec_algorithm_id.is_none() {
            None
        } else {
            Some(client_order_id)
        };
        let order = MarketOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            time_in_force.unwrap_or(TimeInForce::Gtc),
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            reduce_only.unwrap_or(false),
            quote_quantity.unwrap_or(false),
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        );
        OrderAny::Market(order)
    }

    /// Creates a new limit order.
    #[expect(clippy::too_many_arguments)]
    pub fn limit(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<nautilus_core::UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let exec_spawn_id: Option<ClientOrderId> = if exec_algorithm_id.is_none() {
            None
        } else {
            Some(client_order_id)
        };
        let order = LimitOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            post_only.unwrap_or(false),
            reduce_only.unwrap_or(false),
            quote_quantity.unwrap_or(false),
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        );
        OrderAny::Limit(order)
    }

    /// Creates a new stop-market order.
    #[expect(clippy::too_many_arguments)]
    pub fn stop_market(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<nautilus_core::UnixNanos>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let exec_spawn_id: Option<ClientOrderId> = if exec_algorithm_id.is_none() {
            None
        } else {
            Some(client_order_id)
        };
        let order = StopMarketOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::Default),
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            reduce_only.unwrap_or(false),
            quote_quantity.unwrap_or(false),
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        );
        OrderAny::StopMarket(order)
    }

    /// Creates a new stop-limit order.
    #[expect(clippy::too_many_arguments)]
    pub fn stop_limit(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<nautilus_core::UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let exec_spawn_id: Option<ClientOrderId> = if exec_algorithm_id.is_none() {
            None
        } else {
            Some(client_order_id)
        };
        let order = StopLimitOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::Default),
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            post_only.unwrap_or(false),
            reduce_only.unwrap_or(false),
            quote_quantity.unwrap_or(false),
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        );
        OrderAny::StopLimit(order)
    }

    /// Creates a new market-if-touched order.
    #[expect(clippy::too_many_arguments)]
    pub fn market_if_touched(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<nautilus_core::UnixNanos>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let exec_spawn_id: Option<ClientOrderId> = if exec_algorithm_id.is_none() {
            None
        } else {
            Some(client_order_id)
        };
        let order = MarketIfTouchedOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::Default),
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            reduce_only.unwrap_or(false),
            quote_quantity.unwrap_or(false),
            emulation_trigger,
            trigger_instrument_id,
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        );
        OrderAny::MarketIfTouched(order)
    }

    /// Creates a new limit-if-touched order.
    #[expect(clippy::too_many_arguments)]
    pub fn limit_if_touched(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<nautilus_core::UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let exec_spawn_id: Option<ClientOrderId> = if exec_algorithm_id.is_none() {
            None
        } else {
            Some(client_order_id)
        };
        let order = LimitIfTouchedOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::Default),
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            post_only.unwrap_or(false),
            reduce_only.unwrap_or(false),
            quote_quantity.unwrap_or(false),
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        );
        OrderAny::LimitIfTouched(order)
    }

    /// Creates a new trailing-stop-market order.
    ///
    /// # Panics
    ///
    /// If neither `trigger_price` nor `activation_price` is provided.
    #[expect(clippy::too_many_arguments)]
    pub fn trailing_stop_market(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trailing_offset: Decimal,
        trailing_offset_type: Option<TrailingOffsetType>,
        activation_price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<nautilus_core::UnixNanos>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
        client_order_id: Option<ClientOrderId>,
    ) -> OrderAny {
        let client_order_id = client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let exec_spawn_id: Option<ClientOrderId> = if exec_algorithm_id.is_none() {
            None
        } else {
            Some(client_order_id)
        };

        // Trailing stops need an initial trigger level: prefer explicit trigger_price,
        // fall back to activation_price which serves as the initial trigger on OKX
        let trigger_price = trigger_price
            .or(activation_price)
            .expect("TrailingStopMarket requires either trigger_price or activation_price");

        let order = TrailingStopMarketOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            quantity,
            trigger_price,
            trigger_type.unwrap_or(TriggerType::Default),
            trailing_offset,
            trailing_offset_type.unwrap_or(TrailingOffsetType::Price),
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            reduce_only.unwrap_or(false),
            quote_quantity.unwrap_or(false),
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        );

        let mut order = OrderAny::TrailingStopMarket(order);

        if let (Some(activation_price), OrderAny::TrailingStopMarket(tsm)) =
            (activation_price, &mut order)
        {
            tsm.activation_price = Some(activation_price);
        }

        order
    }

    /// Creates a new [`OrderList`] from the given orders, generating a fresh
    /// order list ID and propagating it back to each order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `orders` is empty.
    /// - Any order has a different `instrument_id` than the first.
    /// - Any order has a different `strategy_id` than the factory.
    pub fn create_list(&mut self, orders: &mut [OrderAny], ts_init: UnixNanos) -> OrderList {
        check_slice_not_empty(orders, stringify!(orders)).unwrap();
        let instrument_id = orders[0].instrument_id();
        for order in orders.iter().skip(1) {
            check_equal(
                &order.instrument_id(),
                &instrument_id,
                "instrument_id",
                "first order instrument_id",
            )
            .unwrap();
            check_equal(
                &order.strategy_id(),
                &self.strategy_id,
                "strategy_id",
                "factory strategy_id",
            )
            .unwrap();
        }
        let order_list_id = self.generate_order_list_id();
        let order_ids: Vec<ClientOrderId> = orders.iter().map(OrderAny::client_order_id).collect();

        // Propagate list ID back to each order
        for order in orders.iter_mut() {
            order.set_order_list_id(order_list_id);
        }

        OrderList::new(
            order_list_id,
            instrument_id,
            self.strategy_id,
            order_ids,
            ts_init,
        )
    }

    /// Creates a bracket order with an entry order and attached take-profit and stop-loss legs.
    ///
    /// Defaults:
    /// - `contingency_type`: `Ouo` for the TP/SL legs.
    /// - `entry_order_type`: `Market`; `tp_order_type`: `Limit`; `sl_order_type`: `StopMarket`.
    /// - `entry_tags`: `["ENTRY"]`; `tp_tags`: `["TAKE_PROFIT"]`; `sl_tags`: `["STOP_LOSS"]`.
    /// - `tp_post_only`: `true` for `Limit` and `LimitIfTouched`; `entry_post_only`: `false`.
    /// - TP and SL legs are always `reduce_only = true`; the entry is `reduce_only = false`.
    /// - TP and SL legs do not inherit `expire_time` from the entry.
    ///
    /// # Panics
    ///
    /// Panics if `entry_order_type`, `tp_order_type`, or `sl_order_type` is not one of the
    /// supported variants, or if a required price/trigger field is missing for the chosen type.
    #[expect(clippy::too_many_lines)]
    #[builder]
    pub fn bracket(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        #[builder(default = false)] quote_quantity: bool,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        #[builder(default = ContingencyType::Ouo)] contingency_type: ContingencyType,
        // Entry order
        #[builder(default = OrderType::Market)] entry_order_type: OrderType,
        entry_price: Option<Price>,
        entry_trigger_price: Option<Price>,
        expire_time: Option<nautilus_core::UnixNanos>,
        #[builder(default = TimeInForce::Gtc)] time_in_force: TimeInForce,
        #[builder(default = false)] entry_post_only: bool,
        entry_exec_algorithm_id: Option<ExecAlgorithmId>,
        entry_exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        #[builder(default = vec![Ustr::from("ENTRY")])] entry_tags: Vec<Ustr>,
        entry_client_order_id: Option<ClientOrderId>,
        // Take-profit order
        #[builder(default = OrderType::Limit)] tp_order_type: OrderType,
        tp_price: Option<Price>,
        tp_trigger_price: Option<Price>,
        #[builder(default = TriggerType::Default)] tp_trigger_type: TriggerType,
        tp_activation_price: Option<Price>,
        tp_trailing_offset: Option<Decimal>,
        #[builder(default = TrailingOffsetType::Price)] tp_trailing_offset_type: TrailingOffsetType,
        tp_limit_offset: Option<Decimal>,
        #[builder(default = TimeInForce::Gtc)] tp_time_in_force: TimeInForce,
        #[builder(default = true)] tp_post_only: bool,
        tp_exec_algorithm_id: Option<ExecAlgorithmId>,
        tp_exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        #[builder(default = vec![Ustr::from("TAKE_PROFIT")])] tp_tags: Vec<Ustr>,
        tp_client_order_id: Option<ClientOrderId>,
        // Stop-loss order
        #[builder(default = OrderType::StopMarket)] sl_order_type: OrderType,
        sl_trigger_price: Option<Price>,
        #[builder(default = TriggerType::Default)] sl_trigger_type: TriggerType,
        sl_activation_price: Option<Price>,
        sl_trailing_offset: Option<Decimal>,
        #[builder(default = TrailingOffsetType::Price)] sl_trailing_offset_type: TrailingOffsetType,
        #[builder(default = TimeInForce::Gtc)] sl_time_in_force: TimeInForce,
        sl_exec_algorithm_id: Option<ExecAlgorithmId>,
        sl_exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        #[builder(default = vec![Ustr::from("STOP_LOSS")])] sl_tags: Vec<Ustr>,
        sl_client_order_id: Option<ClientOrderId>,
    ) -> Vec<OrderAny> {
        let order_list_id = self.generate_order_list_id();
        let ts_init = self.clock.borrow().timestamp_ns();

        let entry_client_order_id =
            entry_client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let sl_client_order_id =
            sl_client_order_id.unwrap_or_else(|| self.generate_client_order_id());
        let tp_client_order_id =
            tp_client_order_id.unwrap_or_else(|| self.generate_client_order_id());

        let entry_exec_spawn_id = entry_exec_algorithm_id
            .as_ref()
            .map(|_| entry_client_order_id);
        let tp_exec_spawn_id = tp_exec_algorithm_id.as_ref().map(|_| tp_client_order_id);
        let sl_exec_spawn_id = sl_exec_algorithm_id.as_ref().map(|_| sl_client_order_id);

        let entry_tags = Some(entry_tags);
        let tp_tags = Some(tp_tags);
        let sl_tags = Some(sl_tags);

        let entry_contingency_type = Some(ContingencyType::Oto);
        let entry_order_list_id = Some(order_list_id);
        let entry_linked_order_ids = Some(vec![sl_client_order_id, tp_client_order_id]);
        let entry_parent_order_id: Option<ClientOrderId> = None;

        let entry_order = match entry_order_type {
            OrderType::Market => OrderAny::Market(MarketOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                entry_client_order_id,
                order_side,
                quantity,
                time_in_force,
                UUID4::new(),
                ts_init,
                false, // reduce_only
                quote_quantity,
                entry_contingency_type,
                entry_order_list_id,
                entry_linked_order_ids,
                entry_parent_order_id,
                entry_exec_algorithm_id,
                entry_exec_algorithm_params,
                entry_exec_spawn_id,
                entry_tags,
            )),
            OrderType::Limit => OrderAny::Limit(LimitOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                entry_client_order_id,
                order_side,
                quantity,
                entry_price.expect("`entry_price` is required for a LIMIT entry"),
                time_in_force,
                expire_time,
                entry_post_only,
                false, // reduce_only
                quote_quantity,
                None, // display_qty
                emulation_trigger,
                trigger_instrument_id,
                entry_contingency_type,
                entry_order_list_id,
                entry_linked_order_ids,
                entry_parent_order_id,
                entry_exec_algorithm_id,
                entry_exec_algorithm_params,
                entry_exec_spawn_id,
                entry_tags,
                UUID4::new(),
                ts_init,
            )),
            OrderType::MarketIfTouched => OrderAny::MarketIfTouched(MarketIfTouchedOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                entry_client_order_id,
                order_side,
                quantity,
                entry_trigger_price
                    .expect("`entry_trigger_price` is required for a MARKET_IF_TOUCHED entry"),
                TriggerType::Default,
                time_in_force,
                expire_time,
                false, // reduce_only
                quote_quantity,
                emulation_trigger,
                trigger_instrument_id,
                entry_contingency_type,
                entry_order_list_id,
                entry_linked_order_ids,
                entry_parent_order_id,
                entry_exec_algorithm_id,
                entry_exec_algorithm_params,
                entry_exec_spawn_id,
                entry_tags,
                UUID4::new(),
                ts_init,
            )),
            OrderType::LimitIfTouched => OrderAny::LimitIfTouched(LimitIfTouchedOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                entry_client_order_id,
                order_side,
                quantity,
                entry_price.expect("`entry_price` is required for a LIMIT_IF_TOUCHED entry"),
                entry_trigger_price
                    .expect("`entry_trigger_price` is required for a LIMIT_IF_TOUCHED entry"),
                TriggerType::Default,
                time_in_force,
                expire_time,
                entry_post_only,
                false, // reduce_only
                quote_quantity,
                None, // display_qty
                emulation_trigger,
                trigger_instrument_id,
                entry_contingency_type,
                entry_order_list_id,
                entry_linked_order_ids,
                entry_parent_order_id,
                entry_exec_algorithm_id,
                entry_exec_algorithm_params,
                entry_exec_spawn_id,
                entry_tags,
                UUID4::new(),
                ts_init,
            )),
            OrderType::StopLimit => OrderAny::StopLimit(StopLimitOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                entry_client_order_id,
                order_side,
                quantity,
                entry_price.expect("`entry_price` is required for a STOP_LIMIT entry"),
                entry_trigger_price
                    .expect("`entry_trigger_price` is required for a STOP_LIMIT entry"),
                TriggerType::Default,
                time_in_force,
                expire_time,
                entry_post_only,
                false, // reduce_only
                quote_quantity,
                None, // display_qty
                emulation_trigger,
                trigger_instrument_id,
                entry_contingency_type,
                entry_order_list_id,
                entry_linked_order_ids,
                entry_parent_order_id,
                entry_exec_algorithm_id,
                entry_exec_algorithm_params,
                entry_exec_spawn_id,
                entry_tags,
                UUID4::new(),
                ts_init,
            )),
            other => panic!("invalid `entry_order_type`, was {other}"),
        };

        let sl_tp_side = match order_side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
            OrderSide::NoOrderSide => OrderSide::NoOrderSide,
        };

        let tp_contingency_type = Some(contingency_type);
        let tp_order_list_id = Some(order_list_id);
        let tp_linked_order_ids = Some(vec![sl_client_order_id]);
        let tp_parent_order_id = Some(entry_client_order_id);

        let tp_order = match tp_order_type {
            OrderType::Limit => OrderAny::Limit(LimitOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                tp_client_order_id,
                sl_tp_side,
                quantity,
                tp_price.expect("`tp_price` is required for a LIMIT take-profit"),
                tp_time_in_force,
                None, // expire_time
                tp_post_only,
                true, // reduce_only
                quote_quantity,
                None, // display_qty
                emulation_trigger,
                trigger_instrument_id,
                tp_contingency_type,
                tp_order_list_id,
                tp_linked_order_ids,
                tp_parent_order_id,
                tp_exec_algorithm_id,
                tp_exec_algorithm_params,
                tp_exec_spawn_id,
                tp_tags,
                UUID4::new(),
                ts_init,
            )),
            OrderType::LimitIfTouched => OrderAny::LimitIfTouched(LimitIfTouchedOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                tp_client_order_id,
                sl_tp_side,
                quantity,
                tp_price.expect("`tp_price` is required for a LIMIT_IF_TOUCHED take-profit"),
                tp_trigger_price
                    .expect("`tp_trigger_price` is required for a LIMIT_IF_TOUCHED take-profit"),
                tp_trigger_type,
                tp_time_in_force,
                None, // expire_time
                tp_post_only,
                true, // reduce_only
                quote_quantity,
                None, // display_qty
                emulation_trigger,
                trigger_instrument_id,
                tp_contingency_type,
                tp_order_list_id,
                tp_linked_order_ids,
                tp_parent_order_id,
                tp_exec_algorithm_id,
                tp_exec_algorithm_params,
                tp_exec_spawn_id,
                tp_tags,
                UUID4::new(),
                ts_init,
            )),
            OrderType::MarketIfTouched => OrderAny::MarketIfTouched(MarketIfTouchedOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                tp_client_order_id,
                sl_tp_side,
                quantity,
                tp_trigger_price
                    .expect("`tp_trigger_price` is required for a MARKET_IF_TOUCHED take-profit"),
                tp_trigger_type,
                tp_time_in_force,
                None, // expire_time
                true, // reduce_only
                quote_quantity,
                emulation_trigger,
                trigger_instrument_id,
                tp_contingency_type,
                tp_order_list_id,
                tp_linked_order_ids,
                tp_parent_order_id,
                tp_exec_algorithm_id,
                tp_exec_algorithm_params,
                tp_exec_spawn_id,
                tp_tags,
                UUID4::new(),
                ts_init,
            )),
            OrderType::TrailingStopMarket => {
                let tp_trailing_offset = tp_trailing_offset.expect(
                    "`tp_trailing_offset` is required for a TRAILING_STOP_MARKET take-profit",
                );
                let trigger_price = tp_trigger_price.or(tp_activation_price).expect(
                    "TRAILING_STOP_MARKET take-profit requires `tp_trigger_price` or `tp_activation_price`",
                );
                let mut order = TrailingStopMarketOrder::new(
                    self.trader_id,
                    self.strategy_id,
                    instrument_id,
                    tp_client_order_id,
                    sl_tp_side,
                    quantity,
                    trigger_price,
                    tp_trigger_type,
                    tp_trailing_offset,
                    tp_trailing_offset_type,
                    tp_time_in_force,
                    None, // expire_time
                    true, // reduce_only
                    quote_quantity,
                    None, // display_qty
                    emulation_trigger,
                    trigger_instrument_id,
                    tp_contingency_type,
                    tp_order_list_id,
                    tp_linked_order_ids,
                    tp_parent_order_id,
                    tp_exec_algorithm_id,
                    tp_exec_algorithm_params,
                    tp_exec_spawn_id,
                    tp_tags,
                    UUID4::new(),
                    ts_init,
                );
                order.activation_price = tp_activation_price;
                OrderAny::TrailingStopMarket(order)
            }
            OrderType::TrailingStopLimit => {
                let tp_trailing_offset = tp_trailing_offset.expect(
                    "`tp_trailing_offset` is required for a TRAILING_STOP_LIMIT take-profit",
                );
                let tp_limit_offset = tp_limit_offset
                    .expect("`tp_limit_offset` is required for a TRAILING_STOP_LIMIT take-profit");
                let trigger_price = tp_trigger_price.or(tp_activation_price).expect(
                    "TRAILING_STOP_LIMIT take-profit requires `tp_trigger_price` or `tp_activation_price`",
                );
                let price =
                    tp_price.expect("`tp_price` is required for a TRAILING_STOP_LIMIT take-profit");
                let mut order = TrailingStopLimitOrder::new(
                    self.trader_id,
                    self.strategy_id,
                    instrument_id,
                    tp_client_order_id,
                    sl_tp_side,
                    quantity,
                    price,
                    trigger_price,
                    tp_trigger_type,
                    tp_limit_offset,
                    tp_trailing_offset,
                    tp_trailing_offset_type,
                    tp_time_in_force,
                    None,  // expire_time
                    false, // post_only (TRAILING_STOP_LIMIT TP must not be post-only)
                    true,  // reduce_only
                    quote_quantity,
                    None, // display_qty
                    emulation_trigger,
                    trigger_instrument_id,
                    tp_contingency_type,
                    tp_order_list_id,
                    tp_linked_order_ids,
                    tp_parent_order_id,
                    tp_exec_algorithm_id,
                    tp_exec_algorithm_params,
                    tp_exec_spawn_id,
                    tp_tags,
                    UUID4::new(),
                    ts_init,
                );
                order.activation_price = tp_activation_price;
                OrderAny::TrailingStopLimit(order)
            }
            other => panic!("invalid `tp_order_type`, was {other}"),
        };

        let sl_contingency_type = Some(contingency_type);
        let sl_order_list_id = Some(order_list_id);
        let sl_linked_order_ids = Some(vec![tp_client_order_id]);
        let sl_parent_order_id = Some(entry_client_order_id);

        let sl_order = match sl_order_type {
            OrderType::StopMarket => OrderAny::StopMarket(StopMarketOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                sl_client_order_id,
                sl_tp_side,
                quantity,
                sl_trigger_price
                    .expect("`sl_trigger_price` is required for a STOP_MARKET stop-loss"),
                sl_trigger_type,
                sl_time_in_force,
                None, // expire_time
                true, // reduce_only
                quote_quantity,
                None, // display_qty
                emulation_trigger,
                trigger_instrument_id,
                sl_contingency_type,
                sl_order_list_id,
                sl_linked_order_ids,
                sl_parent_order_id,
                sl_exec_algorithm_id,
                sl_exec_algorithm_params,
                sl_exec_spawn_id,
                sl_tags,
                UUID4::new(),
                ts_init,
            )),
            OrderType::TrailingStopMarket => {
                let sl_trailing_offset = sl_trailing_offset.expect(
                    "`sl_trailing_offset` is required for a TRAILING_STOP_MARKET stop-loss",
                );
                let trigger_price = sl_trigger_price.or(sl_activation_price).expect(
                    "TRAILING_STOP_MARKET stop-loss requires `sl_trigger_price` or `sl_activation_price`",
                );
                let mut order = TrailingStopMarketOrder::new(
                    self.trader_id,
                    self.strategy_id,
                    instrument_id,
                    sl_client_order_id,
                    sl_tp_side,
                    quantity,
                    trigger_price,
                    sl_trigger_type,
                    sl_trailing_offset,
                    sl_trailing_offset_type,
                    sl_time_in_force,
                    None, // expire_time
                    true, // reduce_only
                    quote_quantity,
                    None, // display_qty
                    emulation_trigger,
                    trigger_instrument_id,
                    sl_contingency_type,
                    sl_order_list_id,
                    sl_linked_order_ids,
                    sl_parent_order_id,
                    sl_exec_algorithm_id,
                    sl_exec_algorithm_params,
                    sl_exec_spawn_id,
                    sl_tags,
                    UUID4::new(),
                    ts_init,
                );
                order.activation_price = sl_activation_price;
                OrderAny::TrailingStopMarket(order)
            }
            other => panic!("invalid `sl_order_type`, was {other}"),
        };

        vec![entry_order, sl_order, tp_order]
    }
}

#[cfg(test)]
pub mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{
            ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType,
        },
        identifiers::{
            ClientOrderId, InstrumentId, OrderListId,
            stubs::{strategy_id_ema_cross, trader_id},
        },
        orders::Order,
        types::Price,
    };
    use rstest::{fixture, rstest};
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use crate::{clock::TestClock, factories::OrderFactory};

    #[fixture]
    pub fn order_factory() -> OrderFactory {
        let trader_id = trader_id();
        let strategy_id = strategy_id_ema_cross();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock,
            false, // use_uuids_for_client_order_ids
            true,  // use_hyphens_in_client_order_ids
        )
    }

    #[rstest]
    fn test_generate_client_order_id(mut order_factory: OrderFactory) {
        let client_order_id = order_factory.generate_client_order_id();
        assert_eq!(
            client_order_id,
            ClientOrderId::new("O-19700101-000000-001-001-1")
        );
    }

    #[rstest]
    fn test_generate_order_list_id(mut order_factory: OrderFactory) {
        let order_list_id = order_factory.generate_order_list_id();
        assert_eq!(
            order_list_id,
            OrderListId::new("OL-19700101-000000-001-001-1")
        );
    }

    #[rstest]
    fn test_set_client_order_id_count(mut order_factory: OrderFactory) {
        order_factory.set_client_order_id_count(10);
        let client_order_id = order_factory.generate_client_order_id();
        assert_eq!(
            client_order_id,
            ClientOrderId::new("O-19700101-000000-001-001-11")
        );
    }

    #[rstest]
    fn test_set_order_list_id_count(mut order_factory: OrderFactory) {
        order_factory.set_order_list_id_count(10);
        let order_list_id = order_factory.generate_order_list_id();
        assert_eq!(
            order_list_id,
            OrderListId::new("OL-19700101-000000-001-001-11")
        );
    }

    #[rstest]
    fn test_reset_factory(mut order_factory: OrderFactory) {
        order_factory.generate_order_list_id();
        order_factory.generate_client_order_id();
        order_factory.reset_factory();
        let client_order_id = order_factory.generate_client_order_id();
        let order_list_id = order_factory.generate_order_list_id();
        assert_eq!(
            client_order_id,
            ClientOrderId::new("O-19700101-000000-001-001-1")
        );
        assert_eq!(
            order_list_id,
            OrderListId::new("OL-19700101-000000-001-001-1")
        );
    }

    #[fixture]
    pub fn order_factory_with_uuids() -> OrderFactory {
        let trader_id = trader_id();
        let strategy_id = strategy_id_ema_cross();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock,
            true, // use_uuids_for_client_order_ids
            true, // use_hyphens_in_client_order_ids
        )
    }

    #[fixture]
    pub fn order_factory_with_hyphens_removed() -> OrderFactory {
        let trader_id = trader_id();
        let strategy_id = strategy_id_ema_cross();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock,
            false, // use_uuids_for_client_order_ids
            false, // use_hyphens_in_client_order_ids
        )
    }

    #[fixture]
    pub fn order_factory_with_uuids_and_hyphens_removed() -> OrderFactory {
        let trader_id = trader_id();
        let strategy_id = strategy_id_ema_cross();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock,
            true,  // use_uuids_for_client_order_ids
            false, // use_hyphens_in_client_order_ids
        )
    }

    #[rstest]
    fn test_generate_client_order_id_with_uuids(mut order_factory_with_uuids: OrderFactory) {
        let client_order_id = order_factory_with_uuids.generate_client_order_id();

        // UUID should be 36 characters with hyphens
        assert_eq!(client_order_id.as_str().len(), 36);
        assert!(client_order_id.as_str().contains('-'));
    }

    #[rstest]
    fn test_generate_client_order_id_with_hyphens_removed(
        mut order_factory_with_hyphens_removed: OrderFactory,
    ) {
        let client_order_id = order_factory_with_hyphens_removed.generate_client_order_id();

        assert_eq!(
            client_order_id,
            ClientOrderId::new("O197001010000000010011")
        );
        assert!(!client_order_id.as_str().contains('-'));
    }

    #[rstest]
    fn test_generate_client_order_id_with_uuids_and_hyphens_removed(
        mut order_factory_with_uuids_and_hyphens_removed: OrderFactory,
    ) {
        let client_order_id =
            order_factory_with_uuids_and_hyphens_removed.generate_client_order_id();

        // UUID without hyphens should be 32 characters
        assert_eq!(client_order_id.as_str().len(), 32);
        assert!(!client_order_id.as_str().contains('-'));
    }

    #[rstest]
    fn test_market_order(mut order_factory: OrderFactory) {
        let market_order = order_factory.market(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Some(TimeInForce::Gtc),
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
        );
        // TODO: Add additional polymorphic getters
        assert_eq!(market_order.instrument_id(), "BTCUSDT.BINANCE".into());
        assert_eq!(market_order.order_side(), OrderSide::Buy);
        assert_eq!(market_order.quantity(), 100.into());
        // assert_eq!(market_order.time_in_force(), TimeInForce::Gtc);
        // assert!(!market_order.is_reduce_only);
        // assert!(!market_order.is_quote_quantity);
        assert_eq!(market_order.exec_algorithm_id(), None);
        // assert_eq!(market_order.exec_algorithm_params(), None);
        // assert_eq!(market_order.exec_spawn_id, None);
        // assert_eq!(market_order.tags, None);
        assert_eq!(
            market_order.client_order_id(),
            ClientOrderId::new("O-19700101-000000-001-001-1")
        );
        // assert_eq!(market_order.order_list_id(), None);
    }

    #[rstest]
    fn test_limit_order(mut order_factory: OrderFactory) {
        let limit_order = order_factory.limit(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Price::from("50000.00"),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(limit_order.instrument_id(), "BTCUSDT.BINANCE".into());
        assert_eq!(limit_order.order_side(), OrderSide::Buy);
        assert_eq!(limit_order.quantity(), 100.into());
        assert_eq!(limit_order.price(), Some(Price::from("50000.00")));
        assert_eq!(
            limit_order.client_order_id(),
            ClientOrderId::new("O-19700101-000000-001-001-1")
        );
    }

    #[rstest]
    fn test_limit_order_with_post_only(mut order_factory: OrderFactory) {
        let limit_order = order_factory.limit(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Price::from("50000.00"),
            Some(TimeInForce::Gtc),
            None,
            Some(true), // post_only
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert!(limit_order.is_post_only());
    }

    #[rstest]
    fn test_limit_order_with_display_qty(mut order_factory: OrderFactory) {
        let limit_order = order_factory.limit(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Price::from("50000.00"),
            Some(TimeInForce::Gtc),
            None,
            Some(false),     // post_only
            Some(false),     // reduce_only
            Some(false),     // quote_quantity
            Some(50.into()), // display_qty
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(limit_order.display_qty(), Some(50.into()));
    }

    #[rstest]
    fn test_stop_market_order(mut order_factory: OrderFactory) {
        let stop_order = order_factory.stop_market(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Sell,
            100.into(),
            Price::from("45000.00"),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(stop_order.instrument_id(), "BTCUSDT.BINANCE".into());
        assert_eq!(stop_order.order_side(), OrderSide::Sell);
        assert_eq!(stop_order.quantity(), 100.into());
        assert_eq!(stop_order.trigger_price(), Some(Price::from("45000.00")));
        assert_eq!(stop_order.trigger_type(), Some(TriggerType::LastPrice));
    }

    #[rstest]
    fn test_stop_limit_order(mut order_factory: OrderFactory) {
        let stop_limit_order = order_factory.stop_limit(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Sell,
            100.into(),
            Price::from("45100.00"), // limit price
            Price::from("45000.00"), // trigger price
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(stop_limit_order.instrument_id(), "BTCUSDT.BINANCE".into());
        assert_eq!(stop_limit_order.order_side(), OrderSide::Sell);
        assert_eq!(stop_limit_order.quantity(), 100.into());
        assert_eq!(stop_limit_order.price(), Some(Price::from("45100.00")));
        assert_eq!(
            stop_limit_order.trigger_price(),
            Some(Price::from("45000.00"))
        );
        assert_eq!(
            stop_limit_order.trigger_type(),
            Some(TriggerType::LastPrice)
        );
    }

    #[rstest]
    fn test_market_if_touched_order(mut order_factory: OrderFactory) {
        let mit_order = order_factory.market_if_touched(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Price::from("48000.00"),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(mit_order.instrument_id(), "BTCUSDT.BINANCE".into());
        assert_eq!(mit_order.order_side(), OrderSide::Buy);
        assert_eq!(mit_order.quantity(), 100.into());
        assert_eq!(mit_order.trigger_price(), Some(Price::from("48000.00")));
        assert_eq!(mit_order.trigger_type(), Some(TriggerType::LastPrice));
    }

    #[rstest]
    fn test_limit_if_touched_order(mut order_factory: OrderFactory) {
        let lit_order = order_factory.limit_if_touched(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Price::from("48100.00"), // limit price
            Price::from("48000.00"), // trigger price
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(lit_order.instrument_id(), "BTCUSDT.BINANCE".into());
        assert_eq!(lit_order.order_side(), OrderSide::Buy);
        assert_eq!(lit_order.quantity(), 100.into());
        assert_eq!(lit_order.price(), Some(Price::from("48100.00")));
        assert_eq!(lit_order.trigger_price(), Some(Price::from("48000.00")));
        assert_eq!(lit_order.trigger_type(), Some(TriggerType::LastPrice));
    }

    #[rstest]
    fn test_bracket_order_with_market_entry(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders.len(), 3);
        assert_eq!(orders[0].instrument_id(), "BTCUSDT.BINANCE".into());

        // Entry should be market order
        assert_eq!(orders[0].order_side(), OrderSide::Buy);

        // SL should be opposite side stop-market
        assert_eq!(orders[1].order_side(), OrderSide::Sell);
        assert_eq!(orders[1].trigger_price(), Some(Price::from("45000.00")));

        // TP should be opposite side limit
        assert_eq!(orders[2].order_side(), OrderSide::Sell);
        assert_eq!(orders[2].price(), Some(Price::from("55000.00")));
    }

    #[rstest]
    fn test_bracket_order_with_limit_entry(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::Limit)
            .entry_price(Price::from("49000.00"))
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders.len(), 3);
        assert_eq!(orders[0].price(), Some(Price::from("49000.00")));
    }

    #[rstest]
    fn test_bracket_order_with_stop_limit_entry(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::StopLimit)
            .entry_price(Price::from("51500.00"))
            .entry_trigger_price(Price::from("51000.00"))
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders.len(), 3);
        assert_eq!(orders[0].trigger_price(), Some(Price::from("51000.00")));
        assert_eq!(orders[0].price(), Some(Price::from("51500.00")));
    }

    #[rstest]
    fn test_bracket_order_sell_side(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Sell)
            .quantity(100.into())
            .entry_order_type(OrderType::Limit)
            .entry_price(Price::from("51000.00"))
            .tp_price(Price::from("45000.00"))
            .sl_trigger_price(Price::from("55000.00"))
            .call();

        assert_eq!(orders.len(), 3);
        assert_eq!(orders[0].order_side(), OrderSide::Sell);
        assert_eq!(orders[1].order_side(), OrderSide::Buy);
        assert_eq!(orders[2].order_side(), OrderSide::Buy);
    }

    #[rstest]
    fn test_bracket_order_sets_contingencies(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::Limit)
            .entry_price(Price::from("50000.00"))
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        let entry = &orders[0];
        let stop = &orders[1];
        let take = &orders[2];

        let order_list_id = entry
            .order_list_id()
            .expect("Entry should have order_list_id");
        assert_eq!(entry.contingency_type(), Some(ContingencyType::Oto));
        assert_eq!(
            entry.linked_order_ids().unwrap(),
            &[stop.client_order_id(), take.client_order_id()]
        );

        assert_eq!(stop.order_list_id(), Some(order_list_id));
        assert_eq!(stop.contingency_type(), Some(ContingencyType::Ouo));
        assert_eq!(stop.parent_order_id(), Some(entry.client_order_id()));
        assert_eq!(stop.linked_order_ids().unwrap(), &[take.client_order_id()]);

        assert_eq!(take.order_list_id(), Some(order_list_id));
        assert_eq!(take.contingency_type(), Some(ContingencyType::Ouo));
        assert_eq!(take.parent_order_id(), Some(entry.client_order_id()));
        assert_eq!(take.linked_order_ids().unwrap(), &[stop.client_order_id()]);
    }

    #[rstest]
    fn test_bracket_order_default_tags(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[0].tags(), Some(&vec![Ustr::from("ENTRY")][..]));
        assert_eq!(orders[1].tags(), Some(&vec![Ustr::from("STOP_LOSS")][..]));
        assert_eq!(orders[2].tags(), Some(&vec![Ustr::from("TAKE_PROFIT")][..]));
    }

    #[rstest]
    fn test_bracket_order_custom_tags(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .entry_tags(vec![Ustr::from("ALPHA"), Ustr::from("ENTRY-V2")])
            .tp_tags(vec![Ustr::from("TP-V2")])
            .sl_tags(vec![Ustr::from("SL-V2")])
            .call();

        assert_eq!(
            orders[0].tags(),
            Some(&vec![Ustr::from("ALPHA"), Ustr::from("ENTRY-V2")][..])
        );
        assert_eq!(orders[1].tags(), Some(&vec![Ustr::from("SL-V2")][..]));
        assert_eq!(orders[2].tags(), Some(&vec![Ustr::from("TP-V2")][..]));
    }

    #[rstest]
    fn test_bracket_order_custom_contingency_type(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .contingency_type(ContingencyType::Oco)
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[1].contingency_type(), Some(ContingencyType::Oco));
        assert_eq!(orders[2].contingency_type(), Some(ContingencyType::Oco));
    }

    #[rstest]
    fn test_bracket_order_custom_client_order_ids(mut order_factory: OrderFactory) {
        let entry_id = ClientOrderId::new("CUSTOM-ENTRY");
        let tp_id = ClientOrderId::new("CUSTOM-TP");
        let sl_id = ClientOrderId::new("CUSTOM-SL");

        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .entry_client_order_id(entry_id)
            .tp_client_order_id(tp_id)
            .sl_client_order_id(sl_id)
            .call();

        assert_eq!(orders[0].client_order_id(), entry_id);
        assert_eq!(orders[1].client_order_id(), sl_id);
        assert_eq!(orders[2].client_order_id(), tp_id);
    }

    #[rstest]
    fn test_bracket_order_per_leg_order_types(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::Limit)
            .entry_price(Price::from("50000.00"))
            .tp_order_type(OrderType::MarketIfTouched)
            .tp_trigger_price(Price::from("55000.00"))
            .tp_trigger_type(TriggerType::LastPrice)
            .sl_order_type(OrderType::TrailingStopMarket)
            .sl_trigger_price(Price::from("45000.00"))
            .sl_activation_price(Price::from("44000.00"))
            .sl_trailing_offset(Decimal::new(50, 2))
            .sl_trailing_offset_type(TrailingOffsetType::BasisPoints)
            .call();

        // Entry: limit
        assert_eq!(orders[0].order_type(), OrderType::Limit);
        // SL: trailing stop market with non-default offset type
        assert_eq!(orders[1].order_type(), OrderType::TrailingStopMarket);
        assert_eq!(orders[1].trigger_price(), Some(Price::from("45000.00")));
        assert_eq!(orders[1].activation_price(), Some(Price::from("44000.00")));
        assert_eq!(orders[1].trailing_offset(), Some(Decimal::new(50, 2)));
        assert_eq!(
            orders[1].trailing_offset_type(),
            Some(TrailingOffsetType::BasisPoints)
        );
        // TP: market-if-touched with non-default trigger type
        assert_eq!(orders[2].order_type(), OrderType::MarketIfTouched);
        assert_eq!(orders[2].trigger_price(), Some(Price::from("55000.00")));
        assert_eq!(orders[2].trigger_type(), Some(TriggerType::LastPrice));
    }

    #[rstest]
    fn test_bracket_order_reduce_only_flags(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::Limit)
            .entry_price(Price::from("50000.00"))
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert!(!orders[0].is_reduce_only(), "entry must not be reduce-only");
        assert!(orders[1].is_reduce_only(), "SL must be reduce-only");
        assert!(orders[2].is_reduce_only(), "TP must be reduce-only");
    }

    #[rstest]
    fn test_bracket_order_default_post_only(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::Limit)
            .entry_price(Price::from("50000.00"))
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert!(!orders[0].is_post_only(), "entry default is not post-only");
        assert!(orders[2].is_post_only(), "Limit TP default is post-only");
    }

    #[rstest]
    fn test_bracket_order_trailing_stop_limit_tp_forces_no_post_only(
        mut order_factory: OrderFactory,
    ) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_order_type(OrderType::TrailingStopLimit)
            .tp_price(Price::from("55000.00"))
            .tp_trigger_price(Price::from("54000.00"))
            .tp_trailing_offset(Decimal::new(50, 2))
            .tp_limit_offset(Decimal::new(10, 2))
            .tp_post_only(true) // explicitly true; constructor must override to false
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[2].order_type(), OrderType::TrailingStopLimit);
        assert!(
            !orders[2].is_post_only(),
            "TRAILING_STOP_LIMIT TP must never be post-only"
        );
    }

    #[rstest]
    fn test_bracket_order_expire_time_entry_only(mut order_factory: OrderFactory) {
        let expire_time = UnixNanos::from(1_700_000_000_000_000_000_u64);
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::Limit)
            .entry_price(Price::from("50000.00"))
            .expire_time(expire_time)
            .time_in_force(TimeInForce::Gtd)
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[0].expire_time(), Some(expire_time));
        assert_eq!(orders[1].expire_time(), None);
        assert_eq!(orders[2].expire_time(), None);
    }

    #[rstest]
    fn test_bracket_order_with_market_if_touched_entry(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::MarketIfTouched)
            .entry_trigger_price(Price::from("51000.00"))
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[0].order_type(), OrderType::MarketIfTouched);
        assert_eq!(orders[0].trigger_price(), Some(Price::from("51000.00")));
    }

    #[rstest]
    fn test_bracket_order_with_limit_if_touched_entry(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::LimitIfTouched)
            .entry_price(Price::from("51500.00"))
            .entry_trigger_price(Price::from("51000.00"))
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[0].order_type(), OrderType::LimitIfTouched);
        assert_eq!(orders[0].price(), Some(Price::from("51500.00")));
        assert_eq!(orders[0].trigger_price(), Some(Price::from("51000.00")));
    }

    #[rstest]
    fn test_bracket_order_with_limit_if_touched_tp(mut order_factory: OrderFactory) {
        // BUY entry => SELL TP; SELL LimitIfTouched requires trigger_price >= price.
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_order_type(OrderType::LimitIfTouched)
            .tp_price(Price::from("54500.00"))
            .tp_trigger_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[2].order_type(), OrderType::LimitIfTouched);
        assert_eq!(orders[2].price(), Some(Price::from("54500.00")));
        assert_eq!(orders[2].trigger_price(), Some(Price::from("55000.00")));
        assert!(
            orders[2].is_post_only(),
            "LimitIfTouched TP default is post-only"
        );
    }

    #[rstest]
    fn test_bracket_order_with_trailing_stop_market_tp(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_order_type(OrderType::TrailingStopMarket)
            .tp_trigger_price(Price::from("55000.00"))
            .tp_activation_price(Price::from("54500.00"))
            .tp_trailing_offset(Decimal::new(75, 2))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[2].order_type(), OrderType::TrailingStopMarket);
        assert_eq!(orders[2].trigger_price(), Some(Price::from("55000.00")));
        assert_eq!(orders[2].activation_price(), Some(Price::from("54500.00")));
        assert_eq!(orders[2].trailing_offset(), Some(Decimal::new(75, 2)));
    }

    #[rstest]
    fn test_bracket_order_with_trailing_stop_limit_tp(mut order_factory: OrderFactory) {
        let orders = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_order_type(OrderType::TrailingStopLimit)
            .tp_price(Price::from("55000.00"))
            .tp_trigger_price(Price::from("54000.00"))
            .tp_activation_price(Price::from("53500.00"))
            .tp_trailing_offset(Decimal::new(50, 2))
            .tp_limit_offset(Decimal::new(10, 2))
            .sl_trigger_price(Price::from("45000.00"))
            .call();

        assert_eq!(orders[2].order_type(), OrderType::TrailingStopLimit);
        assert_eq!(orders[2].price(), Some(Price::from("55000.00")));
        assert_eq!(orders[2].trigger_price(), Some(Price::from("54000.00")));
        assert_eq!(orders[2].activation_price(), Some(Price::from("53500.00")));
        assert_eq!(orders[2].trailing_offset(), Some(Decimal::new(50, 2)));
        assert_eq!(orders[2].limit_offset(), Some(Decimal::new(10, 2)));
    }

    #[rstest]
    #[should_panic(expected = "`tp_price` is required for a LIMIT take-profit")]
    fn test_bracket_order_panics_on_missing_tp_price(mut order_factory: OrderFactory) {
        let _ = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .sl_trigger_price(Price::from("45000.00"))
            .call();
    }

    #[rstest]
    #[should_panic(expected = "`sl_trigger_price` is required for a STOP_MARKET stop-loss")]
    fn test_bracket_order_panics_on_missing_sl_trigger_price(mut order_factory: OrderFactory) {
        let _ = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_price(Price::from("55000.00"))
            .call();
    }

    #[rstest]
    #[should_panic(
        expected = "`tp_trailing_offset` is required for a TRAILING_STOP_MARKET take-profit"
    )]
    fn test_bracket_order_panics_on_missing_tp_trailing_offset(mut order_factory: OrderFactory) {
        let _ = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_order_type(OrderType::TrailingStopMarket)
            .tp_trigger_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();
    }

    #[rstest]
    #[should_panic(expected = "invalid `entry_order_type`")]
    fn test_bracket_order_panics_on_invalid_entry_order_type(mut order_factory: OrderFactory) {
        let _ = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .entry_order_type(OrderType::MarketToLimit)
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();
    }

    #[rstest]
    #[should_panic(expected = "invalid `tp_order_type`")]
    fn test_bracket_order_panics_on_invalid_tp_order_type(mut order_factory: OrderFactory) {
        let _ = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .tp_order_type(OrderType::StopMarket)
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();
    }

    #[rstest]
    #[should_panic(expected = "invalid `sl_order_type`")]
    fn test_bracket_order_panics_on_invalid_sl_order_type(mut order_factory: OrderFactory) {
        let _ = order_factory
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(100.into())
            .sl_order_type(OrderType::Limit)
            .tp_price(Price::from("55000.00"))
            .sl_trigger_price(Price::from("45000.00"))
            .call();
    }

    #[rstest]
    fn test_create_list_from_plain_orders(mut order_factory: OrderFactory) {
        let entry = order_factory.limit(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Price::from("50000.00"),
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
            None,
        );
        let sl = order_factory.stop_market(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Sell,
            100.into(),
            Price::from("45000.00"),
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
            None,
        );

        let mut orders = vec![entry.clone(), sl.clone()];
        let order_list = order_factory.create_list(&mut orders, UnixNanos::default());

        assert_eq!(order_list.len(), 2);
        assert_eq!(
            order_list.instrument_id,
            InstrumentId::from("BTCUSDT.BINANCE")
        );
        assert_eq!(order_list.client_order_ids[0], entry.client_order_id());
        assert_eq!(order_list.client_order_ids[1], sl.client_order_id());
        assert_eq!(
            order_list.id,
            OrderListId::new("OL-19700101-000000-001-001-1"),
        );
        assert_eq!(orders[0].order_list_id(), Some(order_list.id));
        assert_eq!(orders[1].order_list_id(), Some(order_list.id));
    }
}
