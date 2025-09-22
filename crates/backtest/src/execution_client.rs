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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

//! Provides a `BacktestExecutionClient` implementation for backtesting.

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
        SubmitOrder, SubmitOrderList, TradingCommand,
    },
};
use nautilus_core::{SharedCell, UnixNanos, WeakCell};
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, TraderId, Venue},
    orders::Order,
    types::{AccountBalance, MarginBalance},
};

use crate::exchange::SimulatedExchange;

/// Execution client implementation for backtesting trading operations.
///
/// The `BacktestExecutionClient` provides an execution client interface for
/// backtesting environments, handling order management and trade execution
/// through simulated exchanges. It processes trading commands and coordinates
/// with the simulation infrastructure to provide realistic execution behavior.
pub struct BacktestExecutionClient {
    core: ExecutionClientCore,
    exchange: WeakCell<SimulatedExchange>,
    clock: Rc<RefCell<dyn Clock>>,
    is_connected: bool,
    routing: bool,
    frozen_account: bool,
}

impl Debug for BacktestExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BacktestExecutionClient))
            .field("client_id", &self.core.client_id)
            .field("routing", &self.routing)
            .finish()
    }
}

impl BacktestExecutionClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        account_id: AccountId,
        exchange: Rc<RefCell<SimulatedExchange>>,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        routing: Option<bool>,
        frozen_account: Option<bool>,
    ) -> Self {
        let routing = routing.unwrap_or(false);
        let frozen_account = frozen_account.unwrap_or(false);
        let exchange_shared: SharedCell<SimulatedExchange> = SharedCell::from(exchange.clone());
        let exchange_id = exchange_shared.borrow().id;
        let core_client = ExecutionClientCore::new(
            trader_id,
            ClientId::from(exchange_id.as_str()),
            Venue::from(exchange_id.as_str()),
            exchange.borrow().oms_type,
            account_id,
            exchange.borrow().account_type,
            exchange.borrow().base_currency,
            clock.clone(),
            cache,
        );

        if !frozen_account {
            // TODO Register calculated account
        }

        Self {
            exchange: exchange_shared.downgrade(),
            clock,
            core: core_client,
            is_connected: false,
            routing,
            frozen_account,
        }
    }
}

impl ExecutionClient for BacktestExecutionClient {
    fn is_connected(&self) -> bool {
        self.is_connected
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
        self.core.get_account()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.core
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.is_connected = true;
        log::info!("Backtest execution client started");
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.is_connected = false;
        log::info!("Backtest execution client stopped");
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        self.core.generate_order_submitted(
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.client_order_id,
            self.clock.borrow().timestamp_ns(),
        );

        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::SubmitOrder(cmd.clone()));
        } else {
            log::error!("submit_order: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        for order in &cmd.order_list.orders {
            self.core.generate_order_submitted(
                cmd.strategy_id,
                order.instrument_id(),
                order.client_order_id(),
                self.clock.borrow().timestamp_ns(),
            );
        }

        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::SubmitOrderList(cmd.clone()));
        } else {
            log::error!("submit_order_list: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::ModifyOrder(cmd.clone()));
        } else {
            log::error!("modify_order: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::CancelOrder(cmd.clone()));
        } else {
            log::error!("cancel_order: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::CancelAllOrders(cmd.clone()));
        } else {
            log::error!("cancel_all_orders: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::BatchCancelOrders(cmd.clone()));
        } else {
            log::error!("batch_cancel_orders: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn query_account(&self, cmd: &QueryAccount) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::QueryAccount(cmd.clone()));
        } else {
            log::error!("query_account: SimulatedExchange has been dropped");
        }
        Ok(())
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        if let Some(exchange) = self.exchange.upgrade() {
            exchange
                .borrow_mut()
                .send(TradingCommand::QueryOrder(cmd.clone()));
        } else {
            log::error!("query_order: SimulatedExchange has been dropped");
        }
        Ok(())
    }
}
