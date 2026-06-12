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

//! Strategy cdylib used by live plug-in execution-boundary tests.

#![allow(
    clippy::multiple_unsafe_ops_per_block,
    reason = "each host-command block derefs the host vtable and performs the FFI call as one boundary crossing"
)]

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, PositionSide, TimeInForce},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId,
    },
    orderbook::OrderBook,
    orders::{MarketOrder, OrderAny},
    types::Quantity,
};
use nautilus_plugin::{
    prelude::*,
    surfaces::commands::{
        CancelAllOrdersCommand, CancelAllOrdersHandle, CancelOrderCommand, CancelOrderHandle,
        CancelOrdersCommand, CancelOrdersHandle, CloseAllPositionsCommand, CloseAllPositionsHandle,
        ClosePositionCommand, ClosePositionHandle, ModifyOrderCommand, ModifyOrderHandle,
        QueryAccountCommand, QueryAccountHandle, QueryOrderCommand, QueryOrderHandle,
        SubmitOrderCommand, SubmitOrderHandle, SubmitOrderListCommand, SubmitOrderListHandle,
    },
};

#[derive(Clone, Copy)]
enum ExecAction {
    SubmitOrder,
    SubmitOrderList,
    CancelOrder,
    CancelOrders,
    CancelAllOrders,
    ModifyOrder,
    ClosePosition,
    CloseAllPositions,
    QueryAccount,
    QueryOrder,
}

pub struct ExecTestStrategy {
    host: *const HostVTable,
    ctx: *const HostContext,
    action: ExecAction,
    strategy_id: StrategyId,
    client_order_id: ClientOrderId,
    secondary_client_order_id: ClientOrderId,
    client_id: Option<ClientId>,
    account_id: AccountId,
    position_id: PositionId,
    expected_instrument_id: InstrumentId,
    callback_path: Option<std::path::PathBuf>,
}

// SAFETY: the host owns both pointers and keeps them live for the strategy
// lifetime; the live engine drives the strategy from one thread.
unsafe impl Send for ExecTestStrategy {}

impl PluginStrategy for ExecTestStrategy {
    const TYPE_NAME: &'static str = "ExecTestStrategy";

    fn new(host: *const HostVTable, ctx: *const HostContext, config_json: &str) -> Self {
        let config = serde_json::from_str::<serde_json::Value>(config_json)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default()));
        let strategy_id = config
            .get("strategy_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("PluginExecCdylib-001");
        let client_order_id = config
            .get("client_order_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("O-CDYLIB-001");
        let secondary_client_order_id = config
            .get("secondary_client_order_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("O-CDYLIB-002");
        let client_id = config
            .get("client_id")
            .and_then(serde_json::Value::as_str)
            .map(ClientId::from);
        let account_id = config
            .get("account_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("BINANCE-001");
        let position_id = config
            .get("position_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("P-CDYLIB-001");
        let expected_instrument_id = config
            .get("instrument_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("ETH-USDT.BINANCE");
        let callback_path = config
            .get("callback_path")
            .and_then(serde_json::Value::as_str)
            .map(std::path::PathBuf::from);
        let action = match config.get("action").and_then(serde_json::Value::as_str) {
            Some("submit_order_list") => ExecAction::SubmitOrderList,
            Some("cancel_order") => ExecAction::CancelOrder,
            Some("cancel_orders") => ExecAction::CancelOrders,
            Some("cancel_all_orders") => ExecAction::CancelAllOrders,
            Some("modify_order") => ExecAction::ModifyOrder,
            Some("close_position") => ExecAction::ClosePosition,
            Some("close_all_positions") => ExecAction::CloseAllPositions,
            Some("query_account") => ExecAction::QueryAccount,
            Some("query_order") => ExecAction::QueryOrder,
            _ => ExecAction::SubmitOrder,
        };

        Self {
            host,
            ctx,
            action,
            strategy_id: StrategyId::from(strategy_id),
            client_order_id: ClientOrderId::from(client_order_id),
            secondary_client_order_id: ClientOrderId::from(secondary_client_order_id),
            client_id,
            account_id: AccountId::from(account_id),
            position_id: PositionId::from(position_id),
            expected_instrument_id: InstrumentId::from(expected_instrument_id),
            callback_path,
        }
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        match self.action {
            ExecAction::SubmitOrder => self.submit_order(),
            ExecAction::SubmitOrderList => self.submit_order_list(),
            ExecAction::CancelOrder => self.cancel_order(),
            ExecAction::CancelOrders => self.cancel_orders(),
            ExecAction::CancelAllOrders => self.cancel_all_orders(),
            ExecAction::ModifyOrder => self.modify_order(),
            ExecAction::ClosePosition => self.close_position(),
            ExecAction::CloseAllPositions => self.close_all_positions(),
            ExecAction::QueryAccount => self.query_account(),
            ExecAction::QueryOrder => self.query_order(),
        }
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        if quote.instrument_id != self.expected_instrument_id {
            anyhow::bail!(
                "instrument id mismatch: expected {}, received {}",
                self.expected_instrument_id,
                quote.instrument_id
            );
        }

        if let Some(path) = &self.callback_path {
            std::fs::write(path, quote.instrument_id.to_string())?;
        }
        Ok(())
    }

    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        if book.instrument_id != self.expected_instrument_id {
            anyhow::bail!(
                "instrument id mismatch: expected {}, received {}",
                self.expected_instrument_id,
                book.instrument_id
            );
        }

        if let Some(path) = &self.callback_path {
            std::fs::write(path, book.instrument_id.to_string())?;
        }
        Ok(())
    }
}

impl ExecTestStrategy {
    fn submit_order(&mut self) -> anyhow::Result<()> {
        let order = self.market_order(self.client_order_id, OrderSide::Buy);
        let handle = SubmitOrderHandle::new(SubmitOrderCommand::new(
            order,
            Some(self.position_id),
            None,
            None,
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).submit_order)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn submit_order_list(&mut self) -> anyhow::Result<()> {
        let handle = SubmitOrderListHandle::new(SubmitOrderListCommand::new(
            vec![
                self.market_order(self.client_order_id, OrderSide::Buy),
                self.market_order(self.secondary_client_order_id, OrderSide::Sell),
            ],
            Some(self.position_id),
            self.client_id,
            Some(Self::params("cdylib-submit-order-list")),
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).submit_order_list)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn cancel_order(&mut self) -> anyhow::Result<()> {
        let handle =
            CancelOrderHandle::new(CancelOrderCommand::new(self.client_order_id, None, None));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).cancel_order)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn cancel_orders(&mut self) -> anyhow::Result<()> {
        let handle = CancelOrdersHandle::new(CancelOrdersCommand::new(
            vec![self.client_order_id, self.secondary_client_order_id],
            self.client_id,
            Some(Self::params("cdylib-cancel-orders")),
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).cancel_orders)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn cancel_all_orders(&mut self) -> anyhow::Result<()> {
        let handle = CancelAllOrdersHandle::new(CancelAllOrdersCommand::new(
            self.expected_instrument_id,
            Some(OrderSide::Buy),
            self.client_id,
            Some(Self::params("cdylib-cancel-all-orders")),
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).cancel_all_orders)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn modify_order(&mut self) -> anyhow::Result<()> {
        let handle = ModifyOrderHandle::new(ModifyOrderCommand::new(
            self.client_order_id,
            Some(Quantity::from("2.0")),
            None,
            None,
            None,
            None,
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).modify_order)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn close_position(&mut self) -> anyhow::Result<()> {
        let handle = ClosePositionHandle::new(ClosePositionCommand::new(
            self.position_id,
            None,
            Some(vec![ustr::Ustr::from("cdylib-close")]),
            Some(TimeInForce::Ioc),
            Some(true),
            None,
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).close_position)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn close_all_positions(&mut self) -> anyhow::Result<()> {
        let handle = CloseAllPositionsHandle::new(CloseAllPositionsCommand::new(
            self.expected_instrument_id,
            Some(PositionSide::Long),
            self.client_id,
            Some(vec![ustr::Ustr::from("cdylib-flatten")]),
            Some(TimeInForce::Ioc),
            Some(true),
            Some(false),
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).close_all_positions)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn query_account(&mut self) -> anyhow::Result<()> {
        let handle = QueryAccountHandle::new(QueryAccountCommand::new(
            self.account_id,
            self.client_id,
            Some(Self::params("cdylib-query-account")),
        ));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).query_account)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn query_order(&mut self) -> anyhow::Result<()> {
        let handle =
            QueryOrderHandle::new(QueryOrderCommand::new(self.client_order_id, None, None));

        // SAFETY: the host vtable and context came from `new`; the handle
        // stays live until the host call returns.
        let result = unsafe { ((*self.host).query_order)(self.ctx, &raw const handle) };
        result
            .into_result()
            .map_err(|e| anyhow::anyhow!(e.message_string()))
    }

    fn market_order(&self, client_order_id: ClientOrderId, order_side: OrderSide) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            self.strategy_id,
            self.expected_instrument_id,
            client_order_id,
            order_side,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
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
        ))
    }

    fn params(marker: &str) -> Params {
        let mut params = Params::new();
        params.insert(
            "marker".to_string(),
            serde_json::Value::String(marker.to_string()),
        );
        params
    }
}

nautilus_plugin::nautilus_plugin! {
    name: "exec-test-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
    strategies: [ExecTestStrategy],
}

#[allow(dead_code)]
fn main() {}
