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
    enums::{ContingencyType, OrderSide, TimeInForce},
    identifiers::{
        ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, StrategyId, TraderId,
    },
    orders::{MarketOrder, OrderAny},
    types::Quantity,
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
    ) -> Self {
        let order_id_generator = ClientOrderIdGenerator::new(
            trader_id,
            strategy_id,
            init_order_id_count.unwrap_or(0),
            clock,
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

    pub const fn set_client_order_id_count(&mut self, count: usize) {
        self.order_id_generator.set_count(count);
    }

    pub const fn set_order_list_id_count(&mut self, count: usize) {
        self.order_list_id_generator.set_count(count);
    }

    pub fn generate_client_order_id(&mut self) -> ClientOrderId {
        self.order_id_generator.generate()
    }

    pub fn generate_order_list_id(&mut self) -> OrderListId {
        self.order_list_id_generator.generate()
    }

    pub const fn reset_factory(&mut self) {
        self.order_id_generator.reset();
        self.order_list_id_generator.reset();
    }

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
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod tests {
    use nautilus_core::time::get_atomic_clock_static;
    use nautilus_model::{
        enums::{OrderSide, TimeInForce},
        identifiers::{
            ClientOrderId, InstrumentId, OrderListId,
            stubs::{strategy_id_ema_cross, trader_id},
        },
        orders::Order,
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
}
