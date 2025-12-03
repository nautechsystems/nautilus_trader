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

//! Factories for constructing domain objects such as orders.

use indexmap::IndexMap;
use nautilus_core::{AtomicTime, UUID4};
use nautilus_model::{
    enums::{ContingencyType, OrderSide, TimeInForce, TriggerType},
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    orders::{
        LimitIfTouchedOrder, LimitOrder, MarketIfTouchedOrder, MarketOrder, OrderAny, OrderList,
        StopLimitOrder, StopMarketOrder,
    },
    types::{Price, Quantity},
};
use ustr::Ustr;

use crate::generators::{
    client_order_id::ClientOrderIdGenerator, order_list_id::OrderListIdGenerator,
};

#[repr(C)]
#[derive(Debug)]
pub struct OrderFactory {
    clock: &'static AtomicTime,
    trader_id: TraderId,
    strategy_id: StrategyId,
    order_id_generator: ClientOrderIdGenerator,
    order_list_id_generator: OrderListIdGenerator,
}

impl OrderFactory {
    /// Creates a new [`OrderFactory`] instance.
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        init_order_id_count: Option<usize>,
        init_order_list_id_count: Option<usize>,
        clock: &'static AtomicTime,
        use_uuids_for_client_order_ids: bool,
        use_hyphens_in_client_order_ids: bool,
    ) -> Self {
        let order_id_generator = ClientOrderIdGenerator::new(
            trader_id,
            strategy_id,
            init_order_id_count.unwrap_or(0),
            clock,
            use_uuids_for_client_order_ids,
            use_hyphens_in_client_order_ids,
        );

        let order_list_id_generator = OrderListIdGenerator::new(
            trader_id,
            strategy_id,
            init_order_list_id_count.unwrap_or(0),
            clock,
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
    #[allow(clippy::too_many_arguments)]
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
            self.clock.get_time_ns(),
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
    #[allow(clippy::too_many_arguments)]
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
            self.clock.get_time_ns(),
        );
        OrderAny::Limit(order)
    }

    /// Creates a new stop-market order.
    #[allow(clippy::too_many_arguments)]
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
            self.clock.get_time_ns(),
        );
        OrderAny::StopMarket(order)
    }

    /// Creates a new stop-limit order.
    #[allow(clippy::too_many_arguments)]
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
            self.clock.get_time_ns(),
        );
        OrderAny::StopLimit(order)
    }

    /// Creates a new market-if-touched order.
    #[allow(clippy::too_many_arguments)]
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
            self.clock.get_time_ns(),
        );
        OrderAny::MarketIfTouched(order)
    }

    /// Creates a new limit-if-touched order.
    #[allow(clippy::too_many_arguments)]
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
            self.clock.get_time_ns(),
        );
        OrderAny::LimitIfTouched(order)
    }

    /// Creates a bracket order list with entry order and attached stop-loss and take-profit orders.
    #[allow(clippy::too_many_arguments)]
    pub fn bracket(
        &mut self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        entry_price: Option<Price>,
        sl_trigger_price: Price,
        tp_price: Price,
        entry_trigger_price: Option<Price>,
        time_in_force: Option<TimeInForce>,
        expire_time: Option<nautilus_core::UnixNanos>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        emulation_trigger: Option<TriggerType>,
        trigger_instrument_id: Option<InstrumentId>,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>,
        tags: Option<Vec<Ustr>>,
    ) -> OrderList {
        let order_list_id = self.generate_order_list_id();
        let ts_init = self.clock.get_time_ns();

        let entry_client_order_id = self.generate_client_order_id();
        let sl_client_order_id = self.generate_client_order_id();
        let tp_client_order_id = self.generate_client_order_id();

        // Exec spawn IDs for algorithm orders
        let entry_exec_spawn_id = exec_algorithm_id.as_ref().map(|_| entry_client_order_id);
        let sl_exec_spawn_id = exec_algorithm_id.as_ref().map(|_| sl_client_order_id);
        let tp_exec_spawn_id = exec_algorithm_id.as_ref().map(|_| tp_client_order_id);

        // Entry order linkage
        let entry_contingency_type = Some(ContingencyType::Oto);
        let entry_order_list_id = Some(order_list_id);
        let entry_linked_order_ids = Some(vec![sl_client_order_id, tp_client_order_id]);
        let entry_parent_order_id = None;

        let entry_order = if let Some(trigger_price) = entry_trigger_price {
            if let Some(price) = entry_price {
                OrderAny::StopLimit(StopLimitOrder::new(
                    self.trader_id,
                    self.strategy_id,
                    instrument_id,
                    entry_client_order_id,
                    order_side,
                    quantity,
                    price,
                    trigger_price,
                    TriggerType::Default,
                    time_in_force.unwrap_or(TimeInForce::Gtc),
                    expire_time,
                    post_only.unwrap_or(false),
                    reduce_only.unwrap_or(false),
                    quote_quantity.unwrap_or(false),
                    None, // display_qty
                    emulation_trigger,
                    trigger_instrument_id,
                    entry_contingency_type,
                    entry_order_list_id,
                    entry_linked_order_ids,
                    entry_parent_order_id,
                    exec_algorithm_id,
                    exec_algorithm_params.clone(),
                    entry_exec_spawn_id,
                    tags.clone(),
                    UUID4::new(),
                    ts_init,
                ))
            } else {
                OrderAny::StopMarket(StopMarketOrder::new(
                    self.trader_id,
                    self.strategy_id,
                    instrument_id,
                    entry_client_order_id,
                    order_side,
                    quantity,
                    trigger_price,
                    TriggerType::Default,
                    time_in_force.unwrap_or(TimeInForce::Gtc),
                    expire_time,
                    reduce_only.unwrap_or(false),
                    quote_quantity.unwrap_or(false),
                    None, // display_qty
                    emulation_trigger,
                    trigger_instrument_id,
                    entry_contingency_type,
                    entry_order_list_id,
                    entry_linked_order_ids,
                    entry_parent_order_id,
                    exec_algorithm_id,
                    exec_algorithm_params.clone(),
                    entry_exec_spawn_id,
                    tags.clone(),
                    UUID4::new(),
                    ts_init,
                ))
            }
        } else if let Some(price) = entry_price {
            OrderAny::Limit(LimitOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                entry_client_order_id,
                order_side,
                quantity,
                price,
                time_in_force.unwrap_or(TimeInForce::Gtc),
                expire_time,
                post_only.unwrap_or(false),
                reduce_only.unwrap_or(false),
                quote_quantity.unwrap_or(false),
                None, // display_qty
                emulation_trigger,
                trigger_instrument_id,
                entry_contingency_type,
                entry_order_list_id,
                entry_linked_order_ids,
                entry_parent_order_id,
                exec_algorithm_id,
                exec_algorithm_params.clone(),
                entry_exec_spawn_id,
                tags.clone(),
                UUID4::new(),
                ts_init,
            ))
        } else {
            OrderAny::Market(MarketOrder::new(
                self.trader_id,
                self.strategy_id,
                instrument_id,
                entry_client_order_id,
                order_side,
                quantity,
                time_in_force.unwrap_or(TimeInForce::Gtc),
                UUID4::new(),
                ts_init,
                reduce_only.unwrap_or(false),
                quote_quantity.unwrap_or(false),
                entry_contingency_type,
                entry_order_list_id,
                entry_linked_order_ids,
                entry_parent_order_id,
                exec_algorithm_id,
                exec_algorithm_params.clone(),
                entry_exec_spawn_id,
                tags.clone(),
            ))
        };

        let sl_tp_side = match order_side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
            OrderSide::NoOrderSide => OrderSide::NoOrderSide,
        };

        // SL order linkage
        let sl_contingency_type = Some(ContingencyType::Oco);
        let sl_order_list_id = Some(order_list_id);
        let sl_linked_order_ids = Some(vec![tp_client_order_id]);
        let sl_parent_order_id = Some(entry_client_order_id);

        let sl_order = OrderAny::StopMarket(StopMarketOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            sl_client_order_id,
            sl_tp_side,
            quantity,
            sl_trigger_price,
            TriggerType::Default,
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            true, // SL/TP should only reduce positions
            quote_quantity.unwrap_or(false),
            None, // display_qty
            emulation_trigger,
            trigger_instrument_id,
            sl_contingency_type,
            sl_order_list_id,
            sl_linked_order_ids,
            sl_parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params.clone(),
            sl_exec_spawn_id,
            tags.clone(),
            UUID4::new(),
            ts_init,
        ));

        // TP order linkage
        let tp_contingency_type = Some(ContingencyType::Oco);
        let tp_order_list_id = Some(order_list_id);
        let tp_linked_order_ids = Some(vec![sl_client_order_id]);
        let tp_parent_order_id = Some(entry_client_order_id);

        let tp_order = OrderAny::Limit(LimitOrder::new(
            self.trader_id,
            self.strategy_id,
            instrument_id,
            tp_client_order_id,
            sl_tp_side,
            quantity,
            tp_price,
            time_in_force.unwrap_or(TimeInForce::Gtc),
            expire_time,
            post_only.unwrap_or(false),
            true, // SL/TP should only reduce positions
            quote_quantity.unwrap_or(false),
            None, // display_qty
            emulation_trigger,
            trigger_instrument_id,
            tp_contingency_type,
            tp_order_list_id,
            tp_linked_order_ids,
            tp_parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            tp_exec_spawn_id,
            tags,
            UUID4::new(),
            ts_init,
        ));

        OrderList::new(
            order_list_id,
            instrument_id,
            self.strategy_id,
            vec![entry_order, sl_order, tp_order],
            ts_init,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub mod tests {
    use nautilus_core::time::get_atomic_clock_static;
    use nautilus_model::{
        enums::{ContingencyType, OrderSide, TimeInForce, TriggerType},
        identifiers::{
            ClientOrderId, InstrumentId, OrderListId,
            stubs::{strategy_id_ema_cross, trader_id},
        },
        orders::Order,
        types::Price,
    };
    use rstest::{fixture, rstest};

    use crate::factories::OrderFactory;

    #[fixture]
    pub fn order_factory() -> OrderFactory {
        let trader_id = trader_id();
        let strategy_id = strategy_id_ema_cross();
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            get_atomic_clock_static(),
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
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            get_atomic_clock_static(),
            true, // use_uuids_for_client_order_ids
            true, // use_hyphens_in_client_order_ids
        )
    }

    #[fixture]
    pub fn order_factory_with_hyphens_removed() -> OrderFactory {
        let trader_id = trader_id();
        let strategy_id = strategy_id_ema_cross();
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            get_atomic_clock_static(),
            false, // use_uuids_for_client_order_ids
            false, // use_hyphens_in_client_order_ids
        )
    }

    #[fixture]
    pub fn order_factory_with_uuids_and_hyphens_removed() -> OrderFactory {
        let trader_id = trader_id();
        let strategy_id = strategy_id_ema_cross();
        OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            get_atomic_clock_static(),
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
        let bracket = order_factory.bracket(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            None,                    // market entry
            Price::from("45000.00"), // SL trigger
            Price::from("55000.00"), // TP price
            None,                    // no entry trigger
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
        );

        assert_eq!(bracket.orders.len(), 3);
        assert_eq!(bracket.instrument_id, "BTCUSDT.BINANCE".into());

        // Entry should be market order
        assert_eq!(bracket.orders[0].order_side(), OrderSide::Buy);

        // SL should be opposite side stop-market
        assert_eq!(bracket.orders[1].order_side(), OrderSide::Sell);
        assert_eq!(
            bracket.orders[1].trigger_price(),
            Some(Price::from("45000.00"))
        );

        // TP should be opposite side limit
        assert_eq!(bracket.orders[2].order_side(), OrderSide::Sell);
        assert_eq!(bracket.orders[2].price(), Some(Price::from("55000.00")));
    }

    #[rstest]
    fn test_bracket_order_with_limit_entry(mut order_factory: OrderFactory) {
        let bracket = order_factory.bracket(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Some(Price::from("49000.00")), // limit entry
            Price::from("45000.00"),       // SL trigger
            Price::from("55000.00"),       // TP price
            None,                          // no entry trigger
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
        );

        assert_eq!(bracket.orders.len(), 3);

        // Entry should be limit order at entry price
        assert_eq!(bracket.orders[0].price(), Some(Price::from("49000.00")));
    }

    #[rstest]
    fn test_bracket_order_with_stop_entry(mut order_factory: OrderFactory) {
        let bracket = order_factory.bracket(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            None,                          // no limit price (stop-market entry)
            Price::from("45000.00"),       // SL trigger
            Price::from("55000.00"),       // TP price
            Some(Price::from("51000.00")), // entry trigger (stop entry)
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
        );

        assert_eq!(bracket.orders.len(), 3);

        // Entry should be stop-market order
        assert_eq!(
            bracket.orders[0].trigger_price(),
            Some(Price::from("51000.00"))
        );
    }

    #[rstest]
    fn test_bracket_order_sell_side(mut order_factory: OrderFactory) {
        let bracket = order_factory.bracket(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Sell,
            100.into(),
            Some(Price::from("51000.00")), // limit entry
            Price::from("55000.00"),       // SL trigger (above entry for sell)
            Price::from("45000.00"),       // TP price (below entry for sell)
            None,
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
        );

        assert_eq!(bracket.orders.len(), 3);

        // Entry should be sell
        assert_eq!(bracket.orders[0].order_side(), OrderSide::Sell);

        // SL should be buy (opposite)
        assert_eq!(bracket.orders[1].order_side(), OrderSide::Buy);

        // TP should be buy (opposite)
        assert_eq!(bracket.orders[2].order_side(), OrderSide::Buy);
    }

    #[rstest]
    fn test_bracket_order_sets_contingencies(mut order_factory: OrderFactory) {
        let bracket = order_factory.bracket(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            100.into(),
            Some(Price::from("50000.00")),
            Price::from("45000.00"),
            Price::from("55000.00"),
            None,
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
        );

        let entry = &bracket.orders[0];
        let stop = &bracket.orders[1];
        let take = &bracket.orders[2];

        assert_eq!(entry.order_list_id(), Some(bracket.id));
        assert_eq!(entry.contingency_type(), Some(ContingencyType::Oto));
        assert_eq!(
            entry.linked_order_ids().unwrap(),
            &[stop.client_order_id(), take.client_order_id()]
        );

        assert_eq!(stop.order_list_id(), Some(bracket.id));
        assert_eq!(stop.contingency_type(), Some(ContingencyType::Oco));
        assert_eq!(stop.parent_order_id(), Some(entry.client_order_id()));
        assert_eq!(stop.linked_order_ids().unwrap(), &[take.client_order_id()]);

        assert_eq!(take.order_list_id(), Some(bracket.id));
        assert_eq!(take.contingency_type(), Some(ContingencyType::Oco));
        assert_eq!(take.parent_order_id(), Some(entry.client_order_id()));
        assert_eq!(take.linked_order_ids().unwrap(), &[stop.client_order_id()]);
    }
}
