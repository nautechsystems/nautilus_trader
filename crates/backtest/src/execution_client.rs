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

//! Provides a `BacktestExecutionClient` implementation for backtesting.

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use async_trait::async_trait;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::Clock,
    factories::OrderEventFactory,
    messages::execution::{
        BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, ModifyOrder,
        QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
    msgbus::{self, MessagingSwitchboard},
};
use nautilus_core::{Params, UnixNanos, WeakCell};
use nautilus_execution::client::core::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, ClientOrderId, TraderId, Venue},
    orders::OrderAny,
    types::{AccountBalance, MarginBalance},
};

use crate::exchange::SimulatedExchange;

/// Execution client implementation for backtesting trading operations.
///
/// The `BacktestExecutionClient` provides an execution client interface for
/// backtesting environments, handling order management and trade execution
/// through simulated exchanges. It processes trading commands and coordinates
/// with the simulation infrastructure to provide realistic execution behavior.
#[derive(Clone)]
pub struct BacktestExecutionClient {
    core: ExecutionClientCore,
    factory: OrderEventFactory,
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    exchange: WeakCell<SimulatedExchange>,
    /// Buffered order events for deferred processing.
    ///
    /// Events like `OrderSubmitted` cannot be sent synchronously through
    /// the msgbus during `submit_order` because the exec engine holds a
    /// borrow via its `execute` handler. Instead, events are buffered here
    /// and drained by the engine after the execute borrow is released.
    queued_events: Rc<RefCell<Vec<OrderEventAny>>>,
    routing: bool,
    _frozen_account: bool,
}

impl Debug for BacktestExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BacktestExecutionClient))
            .field("client_id", &self.core.client_id)
            .field("routing", &self.routing)
            .finish_non_exhaustive()
    }
}

impl BacktestExecutionClient {
    /// Creates a new [`BacktestExecutionClient`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        account_id: AccountId,
        exchange: &Rc<RefCell<SimulatedExchange>>,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        routing: Option<bool>,
        frozen_account: Option<bool>,
    ) -> Self {
        let routing = routing.unwrap_or(false);
        let frozen_account = frozen_account.unwrap_or(false);
        let exchange_id = exchange.borrow().id;
        let account_type = exchange.borrow().account_type;
        let base_currency = exchange.borrow().base_currency;

        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::from(exchange_id.as_str()),
            Venue::from(exchange_id.as_str()),
            exchange.borrow().oms_type,
            account_id,
            account_type,
            base_currency,
            cache.clone(),
        );

        let factory = OrderEventFactory::new(trader_id, account_id, account_type, base_currency);

        Self {
            core,
            factory,
            exchange: WeakCell::from(Rc::downgrade(exchange)),
            cache,
            clock,
            queued_events: Rc::new(RefCell::new(Vec::new())),
            routing,
            _frozen_account: frozen_account,
        }
    }

    fn get_order(&self, client_order_id: ClientOrderId) -> anyhow::Result<OrderAny> {
        Ok(self.cache.borrow().try_order_owned(&client_order_id)?)
    }

    /// Drain buffered order events, sending each to the exec engine.
    pub fn drain_queued_events(&self) {
        let events: Vec<OrderEventAny> = self.queued_events.borrow_mut().drain(..).collect();
        let endpoint = MessagingSwitchboard::exec_engine_process();
        for event in events {
            msgbus::send_order_event(endpoint, event);
        }
    }

    pub(crate) fn order_event_handler(&self) -> Rc<dyn Fn(OrderEventAny)> {
        let queued_events = Rc::clone(&self.queued_events);
        Rc::new(move |event| queued_events.borrow_mut().push(event))
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BacktestExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        self.core.venue
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.cache.borrow().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        let ts_init = self.clock.borrow().timestamp_ns();
        let state = self
            .factory
            .generate_account_state(balances, margins, reported, ts_event, ts_init, info);
        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::send_account_state(endpoint, &state);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.core.set_connected();
        log::info!("Backtest execution client started");
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        log::info!("Backtest execution client stopped");
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        // Buffer the OrderSubmitted event for deferred processing to avoid
        // RefCell re-entrancy (exec_engine holds a borrow during execute)
        let order = self.get_order(cmd.client_order_id)?;
        let ts_init = self.clock.borrow().timestamp_ns();
        let event = self.factory.generate_order_submitted(&order, ts_init);
        self.queued_events.borrow_mut().push(event);

        if let Some(exchange) = self.exchange.upgrade() {
            exchange.borrow_mut().send(TradingCommand::SubmitOrder(cmd));
        } else {
            log::error!("submit_order: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let ts_init = self.clock.borrow().timestamp_ns();

        let orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_for_ids(&cmd.order_list.client_order_ids, &cmd);

        // Buffer events for deferred processing
        let mut queued = self.queued_events.borrow_mut();

        for order in &orders {
            let event = self.factory.generate_order_submitted(order, ts_init);
            queued.push(event);
        }
        drop(queued);

        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::SubmitOrderList(cmd));
        } else {
            log::error!("submit_order_list: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange.borrow_mut().send(TradingCommand::ModifyOrder(cmd));
        } else {
            log::error!("modify_order: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn batch_modify_orders(&self, cmd: BatchModifyOrders) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::ModifyOrders(cmd));
        } else {
            log::error!("batch_modify_orders: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange.borrow_mut().send(TradingCommand::CancelOrder(cmd));
        } else {
            log::error!("cancel_order: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::CancelAllOrders(cmd));
        } else {
            log::error!("cancel_all_orders: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::CancelOrders(cmd));
        } else {
            log::error!("batch_cancel_orders: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        log::warn!("Backtest execution client does not support account queries: {cmd}");
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::warn!("Backtest execution client does not support order queries: {cmd}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{clock::TestClock, messages::execution::QueryOrder};
    use nautilus_core::UUID4;
    use nautilus_execution::models::latency::{LatencyModelHandle, StaticLatencyModel};
    use nautilus_model::{
        enums::{AccountType, BookType, OmsType},
        identifiers::{InstrumentId, StrategyId},
        stubs::TestDefault,
        types::{Currency, Money},
    };
    use rstest::rstest;

    use super::*;
    use crate::config::SimulatedVenueConfig;

    fn setup_client_with_latency() -> (BacktestExecutionClient, Rc<RefCell<SimulatedExchange>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let latency_model = StaticLatencyModel::new(
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let config = SimulatedVenueConfig::builder()
            .venue(Venue::new("SIM"))
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L2_MBP)
            .starting_balances(vec![Money::new(1_000.0, Currency::USD())])
            .latency_model(LatencyModelHandle::new(latency_model))
            .build()
            .unwrap();
        let exchange = Rc::new(RefCell::new(
            SimulatedExchange::new(config, cache.clone(), clock.clone()).unwrap(),
        ));
        let client = BacktestExecutionClient::new(
            TraderId::test_default(),
            AccountId::test_default(),
            &exchange,
            cache,
            clock,
            None,
            None,
        );

        (client, exchange)
    }

    fn query_order() -> QueryOrder {
        QueryOrder::new(
            TraderId::test_default(),
            None,
            StrategyId::test_default(),
            InstrumentId::from("AUD/USD.SIM"),
            ClientOrderId::from("O-001"),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn query_account() -> QueryAccount {
        QueryAccount::new(
            TraderId::test_default(),
            None,
            AccountId::test_default(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    #[rstest]
    fn test_new_holds_weak_reference_to_source_exchange() {
        let (client, exchange) = setup_client_with_latency();

        // The client must not co-own the exchange, otherwise the exchange owning the
        // client closes an unbreakable cycle.
        assert_eq!(Rc::strong_count(&exchange), 1);

        let upgraded: Rc<RefCell<SimulatedExchange>> = client
            .exchange
            .upgrade()
            .expect("exchange outlives the client here")
            .into();

        assert!(Rc::ptr_eq(&upgraded, &exchange));
    }

    #[rstest]
    fn test_query_order_is_not_forwarded_to_exchange() {
        let (client, exchange) = setup_client_with_latency();

        // Hold an immutable exchange borrow across the call: if the client
        // forwards, send()'s `exchange.borrow_mut()` panics here. This makes the
        // test bite on a client-only revert rather than being masked by the
        // exchange-side query guard.
        let exchange_ref = exchange.borrow();
        let result = client.query_order(query_order());

        assert!(result.is_ok());
        assert_eq!(exchange_ref.max_inflight_command_ts(), None);
    }

    #[rstest]
    fn test_query_account_is_not_forwarded_to_exchange() {
        let (client, exchange) = setup_client_with_latency();

        // See test_query_order_is_not_forwarded_to_exchange: the held borrow
        // makes a forwarding attempt panic before the exchange guard can mask it.
        let exchange_ref = exchange.borrow();
        let result = client.query_account(query_account());

        assert!(result.is_ok());
        assert_eq!(exchange_ref.max_inflight_command_ts(), None);
    }
}
