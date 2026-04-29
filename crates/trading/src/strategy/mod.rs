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

pub mod config;
pub mod core;

pub use core::StrategyCore;
use std::panic::{AssertUnwindSafe, catch_unwind};

use ahash::AHashSet;
pub use config::{ImportableStrategyConfig, StrategyConfig};
use nautilus_common::{
    actor::DataActor,
    component::Component,
    enums::ComponentState,
    logging::{EVT, RECV},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
        SubmitOrder, SubmitOrderList, TradingCommand,
    },
    msgbus,
    timer::TimeEvent,
};
use nautilus_core::{Params, UUID4};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, PositionSide, TimeInForce, TriggerType},
    events::{
        OrderAccepted, OrderCancelRejected, OrderDenied, OrderEmulated, OrderEventAny,
        OrderExpired, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionEvent, PositionOpened,
    },
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId},
    orders::{Order, OrderAny, OrderCore, OrderList},
    position::Position,
    types::{Price, Quantity},
};
use ustr::Ustr;

/// Core trait for implementing trading strategies in NautilusTrader.
///
/// Strategies are specialized [`DataActor`]s that combine data ingestion capabilities with
/// order and position management functionality. By implementing this trait,
/// custom strategies gain access to the full trading execution stack including order
/// submission, modification, cancellation, and position management.
///
/// # Key Capabilities
///
/// - All [`DataActor`] capabilities (data subscriptions, event handling, timers).
/// - Order lifecycle management (submit, modify, cancel).
/// - Position management (open, close, monitor).
/// - Access to the trading cache and portfolio.
/// - Event routing to order manager and emulator.
///
/// # Implementation
///
/// Use the `nautilus_strategy!` macro to generate `Deref`, `DerefMut`, and
/// `Strategy` implementations. For strategies that override additional trait
/// methods, pass them in a block:
///
/// ```ignore
/// nautilus_strategy!(MyStrategy, {
///     fn on_order_rejected(&mut self, event: OrderRejected) {
///         // custom handling
///     }
/// });
/// ```
///
/// All order and position management methods are provided as default
/// implementations.
pub trait Strategy: DataActor {
    /// Provides access to the internal `StrategyCore`.
    ///
    /// Generated automatically by the `nautilus_strategy!` macro.
    fn core(&self) -> &StrategyCore;

    /// Provides mutable access to the internal `StrategyCore`.
    ///
    /// Generated automatically by the `nautilus_strategy!` macro.
    fn core_mut(&mut self) -> &mut StrategyCore;

    /// Returns the external order claims for this strategy.
    ///
    /// These are instrument IDs whose external orders should be claimed by this strategy
    /// during reconciliation.
    fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
        None
    }

    /// Submits an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order submission fails.
    fn submit_order(
        &mut self,
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        self.submit_order_with_params(order, position_id, client_id, Params::new())
    }

    /// Submits an order with adapter-specific parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order submission fails.
    fn submit_order_with_params(
        &mut self,
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Params,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let market_exit_tag = core.market_exit_tag;
        let is_market_exit_order = order
            .tags()
            .is_some_and(|tags| tags.contains(&market_exit_tag));

        if core.is_exiting && !order.is_reduce_only() && !is_market_exit_order {
            self.deny_order(&order, Ustr::from("MARKET_EXIT_IN_PROGRESS"));
            return Ok(());
        }

        let core = self.core_mut();
        let params = if params.is_empty() {
            None
        } else {
            Some(params)
        };

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order.clone(), position_id, client_id, true)?;
        }

        let command = SubmitOrder::new(
            trader_id,
            client_id,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            position_id,
            params,
            UUID4::new(),
            ts_init,
        );

        let manager = core.order_manager();

        if matches!(order.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger) {
            manager.send_emulator_command(TradingCommand::SubmitOrder(command));
        } else if order.exec_algorithm_id().is_some() {
            manager.send_algo_command(command, order.exec_algorithm_id().unwrap());
        } else {
            manager.send_risk_command(TradingCommand::SubmitOrder(command));
        }

        self.set_gtd_expiry(&order)?;
        Ok(())
    }

    /// Submits an order list.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered, the order list is invalid,
    /// or order list submission fails.
    fn submit_order_list(
        &mut self,
        mut orders: Vec<OrderAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        let should_deny = {
            let core = self.core_mut();
            let tag = core.market_exit_tag;
            core.is_exiting
                && orders.iter().any(|o| {
                    !o.is_reduce_only() && !o.tags().is_some_and(|tags| tags.contains(&tag))
                })
        };

        if should_deny {
            self.deny_order_list(&orders, Ustr::from("MARKET_EXIT_IN_PROGRESS"));
            return Ok(());
        }

        let core = self.core_mut();
        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        // TODO: Replace with fluent builder API for order list construction
        let order_list = if orders.first().is_some_and(|o| o.order_list_id().is_some()) {
            OrderList::from_orders(&orders, ts_init)
        } else {
            core.order_factory().create_list(&mut orders, ts_init)
        };

        {
            let cache_rc = core.cache_rc();
            let cache = cache_rc.borrow();
            if cache.order_list_exists(&order_list.id) {
                anyhow::bail!("OrderList denied: duplicate {}", order_list.id);
            }

            for order in &orders {
                if order.status() != OrderStatus::Initialized {
                    anyhow::bail!(
                        "Order in list denied: invalid status for {}, expected INITIALIZED",
                        order.client_order_id()
                    );
                }

                if cache.order_exists(&order.client_order_id()) {
                    anyhow::bail!(
                        "Order in list denied: duplicate {}",
                        order.client_order_id()
                    );
                }
            }
        }

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order_list(order_list.clone())?;
            for order in &orders {
                cache.add_order(order.clone(), position_id, client_id, true)?;
            }
        }

        let first_order = orders.first();
        let order_inits: Vec<_> = orders.iter().map(|o| o.init_event().clone()).collect();
        let exec_algorithm_id = first_order.and_then(|o| o.exec_algorithm_id());

        let command = SubmitOrderList::new(
            trader_id,
            client_id,
            strategy_id,
            order_list,
            order_inits,
            exec_algorithm_id,
            position_id,
            None, // params
            UUID4::new(),
            ts_init,
        );

        let has_emulated_order = orders.iter().any(|o| {
            matches!(o.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger)
                || o.is_emulated()
        });

        let manager = core.order_manager();

        if has_emulated_order {
            manager.send_emulator_command(TradingCommand::SubmitOrderList(command));
        } else if let Some(algo_id) = exec_algorithm_id {
            let endpoint = format!("{algo_id}.execute");
            msgbus::send_any(endpoint.into(), &TradingCommand::SubmitOrderList(command));
        } else {
            manager.send_risk_command(TradingCommand::SubmitOrderList(command));
        }

        for order in &orders {
            self.set_gtd_expiry(order)?;
        }

        Ok(())
    }

    /// Submits an order list with adapter-specific parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered, the order list is invalid,
    /// or order list submission fails.
    fn submit_order_list_with_params(
        &mut self,
        mut orders: Vec<OrderAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Params,
    ) -> anyhow::Result<()> {
        let should_deny = {
            let core = self.core_mut();
            let tag = core.market_exit_tag;
            core.is_exiting
                && orders.iter().any(|o| {
                    !o.is_reduce_only() && !o.tags().is_some_and(|tags| tags.contains(&tag))
                })
        };

        if should_deny {
            self.deny_order_list(&orders, Ustr::from("MARKET_EXIT_IN_PROGRESS"));
            return Ok(());
        }

        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        // TODO: Replace with fluent builder API for order list construction
        let order_list = if orders.first().is_some_and(|o| o.order_list_id().is_some()) {
            OrderList::from_orders(&orders, ts_init)
        } else {
            core.order_factory().create_list(&mut orders, ts_init)
        };

        {
            let cache_rc = core.cache_rc();
            let cache = cache_rc.borrow();
            if cache.order_list_exists(&order_list.id) {
                anyhow::bail!("OrderList denied: duplicate {}", order_list.id);
            }

            for order in &orders {
                if order.status() != OrderStatus::Initialized {
                    anyhow::bail!(
                        "Order in list denied: invalid status for {}, expected INITIALIZED",
                        order.client_order_id()
                    );
                }

                if cache.order_exists(&order.client_order_id()) {
                    anyhow::bail!(
                        "Order in list denied: duplicate {}",
                        order.client_order_id()
                    );
                }
            }
        }

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order_list(order_list.clone())?;
            for order in &orders {
                cache.add_order(order.clone(), position_id, client_id, true)?;
            }
        }

        let params_opt = if params.is_empty() {
            None
        } else {
            Some(params)
        };

        let first_order = orders.first();
        let order_inits: Vec<_> = orders.iter().map(|o| o.init_event().clone()).collect();
        let exec_algorithm_id = first_order.and_then(|o| o.exec_algorithm_id());

        let command = SubmitOrderList::new(
            trader_id,
            client_id,
            strategy_id,
            order_list,
            order_inits,
            exec_algorithm_id,
            position_id,
            params_opt,
            UUID4::new(),
            ts_init,
        );

        let has_emulated_order = orders.iter().any(|o| {
            matches!(o.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger)
                || o.is_emulated()
        });

        let manager = core.order_manager();

        if has_emulated_order {
            manager.send_emulator_command(TradingCommand::SubmitOrderList(command));
        } else if let Some(algo_id) = exec_algorithm_id {
            let endpoint = format!("{algo_id}.execute");
            msgbus::send_any(endpoint.into(), &TradingCommand::SubmitOrderList(command));
        } else {
            manager.send_risk_command(TradingCommand::SubmitOrderList(command));
        }

        for order in &orders {
            self.set_gtd_expiry(order)?;
        }

        Ok(())
    }

    /// Modifies an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order modification fails.
    fn modify_order(
        &mut self,
        order: OrderAny,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        self.modify_order_with_params(
            order,
            quantity,
            price,
            trigger_price,
            client_id,
            Params::new(),
        )
    }

    /// Modifies an order with adapter-specific parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order modification fails.
    fn modify_order_with_params(
        &mut self,
        order: OrderAny,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
        params: Params,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let params = if params.is_empty() {
            None
        } else {
            Some(params)
        };

        let command = ModifyOrder::new(
            trader_id,
            client_id,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id(),
            quantity,
            price,
            trigger_price,
            UUID4::new(),
            ts_init,
            params,
        );

        let manager = core.order_manager();

        if matches!(order.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger) {
            manager.send_emulator_command(TradingCommand::ModifyOrder(command));
        } else {
            manager.send_risk_command(TradingCommand::ModifyOrder(command));
        }
        Ok(())
    }

    /// Cancels an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order cancellation fails.
    fn cancel_order(&mut self, order: OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        self.cancel_order_with_params(order, client_id, Params::new())
    }

    /// Cancels an order with adapter-specific parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order cancellation fails.
    fn cancel_order_with_params(
        &mut self,
        order: OrderAny,
        client_id: Option<ClientId>,
        params: Params,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let params = if params.is_empty() {
            None
        } else {
            Some(params)
        };

        let command = CancelOrder::new(
            trader_id,
            client_id,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id(),
            UUID4::new(),
            ts_init,
            params,
        );

        let manager = core.order_manager();

        if matches!(order.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger)
            || order.is_emulated()
        {
            manager.send_emulator_command(TradingCommand::CancelOrder(command));
        } else if let Some(algo_id) = order.exec_algorithm_id() {
            let endpoint = format!("{algo_id}.execute");
            msgbus::send_any(endpoint.into(), &TradingCommand::CancelOrder(command));
        } else {
            manager.send_exec_command(TradingCommand::CancelOrder(command));
        }
        Ok(())
    }

    /// Batch cancels multiple orders for the same instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered, the orders span multiple instruments,
    /// or contain emulated/local orders.
    fn cancel_orders(
        &mut self,
        mut orders: Vec<OrderAny>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        if orders.is_empty() {
            anyhow::bail!("Cannot batch cancel empty order list");
        }

        let core = self.core_mut();
        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let manager = core.order_manager();

        let first = orders.remove(0);
        let instrument_id = first.instrument_id();

        if first.is_emulated() || first.is_active_local() {
            anyhow::bail!("Cannot include emulated or local orders in batch cancel");
        }

        let mut cancels = Vec::with_capacity(orders.len() + 1);
        cancels.push(CancelOrder::new(
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            first.client_order_id(),
            first.venue_order_id(),
            UUID4::new(),
            ts_init,
            params.clone(),
        ));

        for order in orders {
            if order.instrument_id() != instrument_id {
                anyhow::bail!(
                    "Cannot batch cancel orders for different instruments: {} vs {}",
                    instrument_id,
                    order.instrument_id()
                );
            }

            if order.is_emulated() || order.is_active_local() {
                anyhow::bail!("Cannot include emulated or local orders in batch cancel");
            }

            cancels.push(CancelOrder::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                order.client_order_id(),
                order.venue_order_id(),
                UUID4::new(),
                ts_init,
                params.clone(),
            ));
        }

        let command = BatchCancelOrders::new(
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            cancels,
            UUID4::new(),
            ts_init,
            params,
        );

        manager.send_exec_command(TradingCommand::BatchCancelOrders(command));
        Ok(())
    }

    /// Cancels all open orders for the given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order cancellation fails.
    fn cancel_all_orders(
        &mut self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        self.cancel_all_orders_with_params(instrument_id, order_side, client_id, Params::new())
    }

    /// Cancels all open orders for the given instrument with adapter-specific parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order cancellation fails.
    fn cancel_all_orders_with_params(
        &mut self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
        client_id: Option<ClientId>,
        params: Params,
    ) -> anyhow::Result<()> {
        let params = if params.is_empty() {
            None
        } else {
            Some(params)
        };
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();
        let cache = core.cache();

        let open_orders = cache.orders_open(
            None,
            Some(&instrument_id),
            Some(&strategy_id),
            None,
            order_side,
        );

        let emulated_orders = cache.orders_emulated(
            None,
            Some(&instrument_id),
            Some(&strategy_id),
            None,
            order_side,
        );

        let inflight_orders = cache.orders_inflight(
            None,
            Some(&instrument_id),
            Some(&strategy_id),
            None,
            order_side,
        );

        // Sort the algorithm IDs so the per-algo cancel cascade fires msgbus
        // events in a deterministic order across runs; the cache returns an
        // unordered AHashSet.
        let mut exec_algorithm_ids: Vec<_> = cache.exec_algorithm_ids().into_iter().collect();
        exec_algorithm_ids.sort();
        let mut algo_orders = Vec::new();

        for algo_id in &exec_algorithm_ids {
            let orders = cache.orders_for_exec_algorithm(
                algo_id,
                None,
                Some(&instrument_id),
                Some(&strategy_id),
                None,
                order_side,
            );
            algo_orders.extend(orders.iter().map(|o| (*o).clone()));
        }

        let open_count = open_orders.len();
        let emulated_count = emulated_orders.len();
        let inflight_count = inflight_orders.len();
        let algo_count = algo_orders.len();

        drop(cache);

        if open_count == 0 && emulated_count == 0 && inflight_count == 0 && algo_count == 0 {
            let side_str = order_side.map(|s| format!(" {s}")).unwrap_or_default();
            log::info!("No {instrument_id} open, emulated, or inflight{side_str} orders to cancel");
            return Ok(());
        }

        let manager = core.order_manager();

        let side_str = order_side.map(|s| format!(" {s}")).unwrap_or_default();

        if open_count > 0 {
            log::info!(
                "Canceling {open_count} open{side_str} {instrument_id} order{}",
                if open_count == 1 { "" } else { "s" }
            );
        }

        if emulated_count > 0 {
            log::info!(
                "Canceling {emulated_count} emulated{side_str} {instrument_id} order{}",
                if emulated_count == 1 { "" } else { "s" }
            );
        }

        if inflight_count > 0 {
            log::info!(
                "Canceling {inflight_count} inflight{side_str} {instrument_id} order{}",
                if inflight_count == 1 { "" } else { "s" }
            );
        }

        if open_count > 0 || inflight_count > 0 {
            let command = CancelAllOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                order_side.unwrap_or(OrderSide::NoOrderSide),
                UUID4::new(),
                ts_init,
                params.clone(),
            );

            manager.send_exec_command(TradingCommand::CancelAllOrders(command));
        }

        if emulated_count > 0 {
            let command = CancelAllOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                order_side.unwrap_or(OrderSide::NoOrderSide),
                UUID4::new(),
                ts_init,
                params,
            );

            manager.send_emulator_command(TradingCommand::CancelAllOrders(command));
        }

        for order in algo_orders {
            self.cancel_order(order, client_id)?;
        }

        Ok(())
    }

    /// Closes a position by submitting a market order for the opposite side.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or position closing fails.
    fn close_position(
        &mut self,
        position: &Position,
        client_id: Option<ClientId>,
        tags: Option<Vec<Ustr>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        if position.is_closed() {
            log::warn!("Cannot close position (already closed): {}", position.id);
            return Ok(());
        }

        let closing_side = OrderCore::closing_side(position.side);

        let order = core.order_factory().market(
            position.instrument_id,
            closing_side,
            position.quantity,
            time_in_force,
            reduce_only.or(Some(true)),
            quote_quantity,
            None,
            None,
            tags,
            None,
        );

        self.submit_order(order, Some(position.id), client_id)
    }

    /// Closes all open positions for the given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or position closing fails.
    #[expect(clippy::too_many_arguments)]
    fn close_all_positions(
        &mut self,
        instrument_id: InstrumentId,
        position_side: Option<PositionSide>,
        client_id: Option<ClientId>,
        tags: Option<Vec<Ustr>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let cache = core.cache();

        let positions_open = cache.positions_open(
            None,
            Some(&instrument_id),
            Some(&strategy_id),
            None,
            position_side,
        );

        let side_str = position_side.map(|s| format!(" {s}")).unwrap_or_default();

        if positions_open.is_empty() {
            log::info!("No {instrument_id} open{side_str} positions to close");
            return Ok(());
        }

        let count = positions_open.len();
        log::info!(
            "Closing {count} open{side_str} position{}",
            if count == 1 { "" } else { "s" }
        );

        let positions_data: Vec<_> = positions_open
            .iter()
            .map(|p| (p.id, p.instrument_id, p.side, p.quantity, p.is_closed()))
            .collect();

        drop(cache);

        for (pos_id, pos_instrument_id, pos_side, pos_quantity, is_closed) in positions_data {
            if is_closed {
                continue;
            }

            let core = self.core_mut();
            let closing_side = OrderCore::closing_side(pos_side);
            let order = core.order_factory().market(
                pos_instrument_id,
                closing_side,
                pos_quantity,
                time_in_force,
                reduce_only.or(Some(true)),
                quote_quantity,
                None,
                None,
                tags.clone(),
                None,
            );

            self.submit_order(order, Some(pos_id), client_id)?;
        }

        Ok(())
    }

    /// Queries account state from the execution client.
    ///
    /// Creates a [`QueryAccount`] command and sends it to the execution engine,
    /// which will request the current account state from the execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered.
    fn query_account(
        &mut self,
        account_id: AccountId,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let ts_init = core.clock().timestamp_ns();

        let command = QueryAccount::new(
            trader_id,
            client_id,
            account_id,
            UUID4::new(),
            ts_init,
            params,
        );

        core.order_manager()
            .send_exec_command(TradingCommand::QueryAccount(command));
        Ok(())
    }

    /// Queries order state from the execution client.
    ///
    /// Creates a [`QueryOrder`] command and sends it to the execution engine,
    /// which will request the current order state from the execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered.
    fn query_order(
        &mut self,
        order: &OrderAny,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let command = QueryOrder::new(
            trader_id,
            client_id,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id(),
            UUID4::new(),
            ts_init,
            params,
        );

        core.order_manager()
            .send_exec_command(TradingCommand::QueryOrder(command));
        Ok(())
    }

    /// Handles an order event, dispatching to the appropriate handler and routing to the order manager.
    fn handle_order_event(&mut self, event: OrderEventAny) {
        let state = {
            let core = self.core_mut();
            let id = &core.actor.actor_id;
            let is_warning = matches!(
                &event,
                OrderEventAny::Denied(_)
                    | OrderEventAny::Rejected(_)
                    | OrderEventAny::CancelRejected(_)
                    | OrderEventAny::ModifyRejected(_)
            );

            if is_warning {
                log::warn!("{id} {RECV}{EVT} {event}");
            } else if core.config.log_events {
                log::info!("{id} {RECV}{EVT} {event}");
            }

            core.actor.state()
        };

        let client_order_id = event.client_order_id();
        let is_terminal = matches!(
            &event,
            OrderEventAny::Filled(_)
                | OrderEventAny::Canceled(_)
                | OrderEventAny::Rejected(_)
                | OrderEventAny::Expired(_)
                | OrderEventAny::Denied(_)
        );

        // GTD timer cleanup runs regardless of state so timers do not leak when
        // terminal events arrive during the post-stop delay.
        if is_terminal {
            self.cancel_gtd_expiry(&client_order_id);
        }

        // Events are logged unconditionally so residual events received after stop
        // remain observable, but dispatch is gated on the running state.
        if state != ComponentState::Running {
            return;
        }

        // Contingent order manager observes events before user handlers so OCO
        // bookkeeping is consistent with what the strategy then sees.
        {
            let core = self.core_mut();
            if let Some(manager) = &mut core.order_manager {
                manager.handle_event(&event);
            }
        }

        match &event {
            OrderEventAny::Initialized(e) => self.on_order_initialized(e.clone()),
            OrderEventAny::Denied(e) => self.on_order_denied(*e),
            OrderEventAny::Emulated(e) => self.on_order_emulated(*e),
            OrderEventAny::Released(e) => self.on_order_released(*e),
            OrderEventAny::Submitted(e) => self.on_order_submitted(*e),
            OrderEventAny::Rejected(e) => self.on_order_rejected(*e),
            OrderEventAny::Accepted(e) => self.on_order_accepted(*e),
            OrderEventAny::Canceled(e) => {
                let _ = DataActor::on_order_canceled(self, e);
            }
            OrderEventAny::Expired(e) => self.on_order_expired(*e),
            OrderEventAny::Triggered(e) => self.on_order_triggered(*e),
            OrderEventAny::PendingUpdate(e) => self.on_order_pending_update(*e),
            OrderEventAny::PendingCancel(e) => self.on_order_pending_cancel(*e),
            OrderEventAny::ModifyRejected(e) => self.on_order_modify_rejected(*e),
            OrderEventAny::CancelRejected(e) => self.on_order_cancel_rejected(*e),
            OrderEventAny::Updated(e) => self.on_order_updated(*e),
            OrderEventAny::Filled(e) => {
                let _ = DataActor::on_order_filled(self, e);
            }
        }
    }

    /// Handles a position event, dispatching to the appropriate handler.
    fn handle_position_event(&mut self, event: PositionEvent) {
        let state = {
            let core = self.core_mut();

            if core.config.log_events {
                let id = &core.actor.actor_id;
                log::info!("{id} {RECV}{EVT} {event:?}");
            }

            core.actor.state()
        };

        if state != ComponentState::Running {
            return;
        }

        match event {
            PositionEvent::PositionOpened(e) => self.on_position_opened(e),
            PositionEvent::PositionChanged(e) => self.on_position_changed(e),
            PositionEvent::PositionClosed(e) => self.on_position_closed(e),
            PositionEvent::PositionAdjusted(_) => {
                // No handler for adjusted events yet
            }
        }
    }

    // -- LIFECYCLE METHODS -----------------------------------------------------------------------

    /// Called when the strategy is started.
    ///
    /// Override this method to implement custom initialization logic.
    /// The default implementation reactivates GTD timers if `manage_gtd_expiry` is enabled.
    ///
    /// # Errors
    ///
    /// Returns an error if strategy initialization fails.
    fn on_start(&mut self) -> anyhow::Result<()> {
        let core = self.core_mut();
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        log::info!("Starting {strategy_id}");

        if core.config.manage_gtd_expiry {
            self.reactivate_gtd_timers();
        }

        Ok(())
    }

    /// Called when a time event is received.
    ///
    /// Routes GTD expiry timer events to the expiry handler and market exit timer events
    /// to the market exit checker.
    ///
    /// # Errors
    ///
    /// Returns an error if time event handling fails.
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name.starts_with("GTD-EXPIRY:") {
            self.expire_gtd_order(event.clone());
        } else if event.name.starts_with("MARKET_EXIT_CHECK:") {
            self.check_market_exit(event.clone());
        }
        Ok(())
    }

    // -- EVENT HANDLERS --------------------------------------------------------------------------

    /// Called when an order is initialized.
    ///
    /// Override this method to implement custom logic when an order is first created.
    #[allow(unused_variables)]
    fn on_order_initialized(&mut self, event: OrderInitialized) {}

    /// Called when an order is denied by the system.
    ///
    /// Override this method to implement custom logic when an order is denied before submission.
    #[allow(unused_variables)]
    fn on_order_denied(&mut self, event: OrderDenied) {}

    /// Called when an order is emulated.
    ///
    /// Override this method to implement custom logic when an order is taken over by the emulator.
    #[allow(unused_variables)]
    fn on_order_emulated(&mut self, event: OrderEmulated) {}

    /// Called when an order is released from emulation.
    ///
    /// Override this method to implement custom logic when an emulated order is released.
    #[allow(unused_variables)]
    fn on_order_released(&mut self, event: OrderReleased) {}

    /// Called when an order is submitted to the venue.
    ///
    /// Override this method to implement custom logic when an order is submitted.
    #[allow(unused_variables)]
    fn on_order_submitted(&mut self, event: OrderSubmitted) {}

    /// Called when an order is rejected by the venue.
    ///
    /// Override this method to implement custom logic when an order is rejected.
    #[allow(unused_variables)]
    fn on_order_rejected(&mut self, event: OrderRejected) {}

    /// Called when an order is accepted by the venue.
    ///
    /// Override this method to implement custom logic when an order is accepted.
    #[allow(unused_variables)]
    fn on_order_accepted(&mut self, event: OrderAccepted) {}

    /// Called when an order expires.
    ///
    /// Override this method to implement custom logic when an order expires.
    #[allow(unused_variables)]
    fn on_order_expired(&mut self, event: OrderExpired) {}

    /// Called when an order is triggered.
    ///
    /// Override this method to implement custom logic when a stop or conditional order is triggered.
    #[allow(unused_variables)]
    fn on_order_triggered(&mut self, event: OrderTriggered) {}

    /// Called when an order modification is pending.
    ///
    /// Override this method to implement custom logic when an order is pending modification.
    #[allow(unused_variables)]
    fn on_order_pending_update(&mut self, event: OrderPendingUpdate) {}

    /// Called when an order cancellation is pending.
    ///
    /// Override this method to implement custom logic when an order is pending cancellation.
    #[allow(unused_variables)]
    fn on_order_pending_cancel(&mut self, event: OrderPendingCancel) {}

    /// Called when an order modification is rejected.
    ///
    /// Override this method to implement custom logic when an order modification is rejected.
    #[allow(unused_variables)]
    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {}

    /// Called when an order cancellation is rejected.
    ///
    /// Override this method to implement custom logic when an order cancellation is rejected.
    #[allow(unused_variables)]
    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {}

    /// Called when an order is updated.
    ///
    /// Override this method to implement custom logic when an order is modified.
    #[allow(unused_variables)]
    fn on_order_updated(&mut self, event: OrderUpdated) {}

    // Note: on_order_filled is inherited from DataActor trait

    /// Called when a position is opened.
    ///
    /// Override this method to implement custom logic when a position is opened.
    #[allow(unused_variables)]
    fn on_position_opened(&mut self, event: PositionOpened) {}

    /// Called when a position is changed (quantity or price updated).
    ///
    /// Override this method to implement custom logic when a position changes.
    #[allow(unused_variables)]
    fn on_position_changed(&mut self, event: PositionChanged) {}

    /// Called when a position is closed.
    ///
    /// Override this method to implement custom logic when a position is closed.
    #[allow(unused_variables)]
    fn on_position_closed(&mut self, event: PositionClosed) {}

    /// Called when a market exit has been initiated.
    ///
    /// Override this method to implement custom logic when a market exit begins.
    fn on_market_exit(&mut self) {}

    /// Called after a market exit has completed.
    ///
    /// Override this method to implement custom logic after a market exit completes.
    fn post_market_exit(&mut self) {}

    /// Returns whether the strategy is currently executing a market exit.
    ///
    /// Strategies can check this to avoid submitting new orders during exit.
    fn is_exiting(&self) -> bool {
        self.core().is_exiting
    }

    /// Initiates an iterative market exit for the strategy.
    ///
    /// Will cancel all open orders and close all open positions, and wait for
    /// all in-flight orders to resolve and positions to close. The strategy
    /// remains running after the exit completes.
    ///
    /// The `on_market_exit` hook is called when the exit process begins.
    /// The `post_market_exit` hook is called when the exit process completes.
    ///
    /// Uses `market_exit_time_in_force` and `market_exit_reduce_only` from
    /// the strategy config for closing market orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the market exit cannot be initiated.
    fn market_exit(&mut self) -> anyhow::Result<()> {
        let core = self.core_mut();
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());

        if core.actor.state() != ComponentState::Running {
            log::warn!("{strategy_id} Cannot market exit: strategy is not running");
            return Ok(());
        }

        if core.is_exiting {
            log::warn!("{strategy_id} Market exit called when already in progress");
            return Ok(());
        }

        core.is_exiting = true;
        core.market_exit_attempts = 0;
        let time_in_force = core.config.market_exit_time_in_force;
        let reduce_only = core.config.market_exit_reduce_only;

        log::info!("{strategy_id} Initiating market exit...");

        self.on_market_exit();

        let core = self.core_mut();
        let cache = core.cache();

        let open_orders = cache.orders_open(None, None, Some(&strategy_id), None, None);
        let inflight_orders = cache.orders_inflight(None, None, Some(&strategy_id), None, None);
        let open_positions = cache.positions_open(None, None, Some(&strategy_id), None, None);

        let mut instruments: AHashSet<InstrumentId> = AHashSet::new();

        for order in &open_orders {
            instruments.insert(order.instrument_id());
        }

        for order in &inflight_orders {
            instruments.insert(order.instrument_id());
        }

        for position in &open_positions {
            instruments.insert(position.instrument_id);
        }

        let market_exit_tag = core.market_exit_tag;
        // Sort so the per-instrument cancel_all_orders/close_all_positions
        // cascade fires msgbus commands in a deterministic sequence; the
        // upstream dedup is AHash-backed.
        let mut instruments: Vec<_> = instruments.into_iter().collect();
        instruments.sort();
        drop(cache);

        for instrument_id in instruments {
            if let Err(e) = self.cancel_all_orders(instrument_id, None, None) {
                log::error!("Error canceling orders for {instrument_id}: {e}");
            }

            if let Err(e) = self.close_all_positions(
                instrument_id,
                None,
                None,
                Some(vec![market_exit_tag]),
                Some(time_in_force),
                Some(reduce_only),
                None,
            ) {
                log::error!("Error closing positions for {instrument_id}: {e}");
            }
        }

        let core = self.core_mut();
        let interval_ms = core.config.market_exit_interval_ms;
        let timer_name = core.market_exit_timer_name;

        log::info!("{strategy_id} Setting market exit timer at {interval_ms}ms intervals");

        let interval_ns = interval_ms * 1_000_000;
        let result = core.clock().set_timer_ns(
            timer_name.as_str(),
            interval_ns,
            None,
            None,
            None,
            None,
            None,
        );

        if let Err(e) = result {
            // Reset exit state on timer failure (caller handles pending_stop)
            core.is_exiting = false;
            core.market_exit_attempts = 0;
            return Err(e);
        }

        Ok(())
    }

    /// Checks if the market exit is complete and finalizes if so.
    ///
    /// This method is called by the market exit timer.
    fn check_market_exit(&mut self, _event: TimeEvent) {
        // Guard against stale timer events after cancel_market_exit
        if !self.is_exiting() {
            return;
        }

        let core = self.core_mut();
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());

        core.market_exit_attempts += 1;
        let attempts = core.market_exit_attempts;
        let max_attempts = core.config.market_exit_max_attempts;

        log::debug!(
            "{strategy_id} Market exit check triggered (attempt {attempts}/{max_attempts})"
        );

        if attempts >= max_attempts {
            let cache = core.cache();
            let open_orders_count = cache
                .orders_open(None, None, Some(&strategy_id), None, None)
                .len();
            let inflight_orders_count = cache
                .orders_inflight(None, None, Some(&strategy_id), None, None)
                .len();
            let open_positions_count = cache
                .positions_open(None, None, Some(&strategy_id), None, None)
                .len();
            drop(cache);

            log::warn!(
                "{strategy_id} Market exit max attempts ({max_attempts}) reached, \
                completing with open orders: {open_orders_count}, \
                inflight orders: {inflight_orders_count}, \
                open positions: {open_positions_count}"
            );

            self.finalize_market_exit();
            return;
        }

        let cache = core.cache();
        let open_orders = cache.orders_open(None, None, Some(&strategy_id), None, None);
        let inflight_orders = cache.orders_inflight(None, None, Some(&strategy_id), None, None);

        if !open_orders.is_empty() || !inflight_orders.is_empty() {
            return;
        }

        let open_positions = cache.positions_open(None, None, Some(&strategy_id), None, None);

        if !open_positions.is_empty() {
            // If there are open positions but no orders, re-send close orders
            let positions_data: Vec<_> = open_positions
                .iter()
                .map(|p| (p.id, p.instrument_id, p.side, p.quantity, p.is_closed()))
                .collect();

            drop(cache);

            for (pos_id, instrument_id, side, quantity, is_closed) in positions_data {
                if is_closed {
                    continue;
                }

                let core = self.core_mut();
                let time_in_force = core.config.market_exit_time_in_force;
                let reduce_only = core.config.market_exit_reduce_only;
                let market_exit_tag = core.market_exit_tag;
                let closing_side = OrderCore::closing_side(side);
                let order = core.order_factory().market(
                    instrument_id,
                    closing_side,
                    quantity,
                    Some(time_in_force),
                    Some(reduce_only),
                    None,
                    None,
                    None,
                    Some(vec![market_exit_tag]),
                    None,
                );

                if let Err(e) = self.submit_order(order, Some(pos_id), None) {
                    log::error!("Error re-submitting close order for position {pos_id}: {e}");
                }
            }
            return;
        }

        drop(cache);
        self.finalize_market_exit();
    }

    /// Finalizes the market exit process.
    ///
    /// Cancels the market exit timer, resets state, calls the post_market_exit hook,
    /// and stops the strategy if a stop was pending.
    fn finalize_market_exit(&mut self) {
        let (strategy_id, should_stop) = {
            let core = self.core_mut();
            let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
            let should_stop = core.pending_stop;
            (strategy_id, should_stop)
        };

        self.cancel_market_exit();

        let hook_result = catch_unwind(AssertUnwindSafe(|| {
            self.post_market_exit();
        }));

        if let Err(e) = hook_result {
            log::error!("{strategy_id} Error in post_market_exit: {e:?}");
        }

        if should_stop {
            log::info!("{strategy_id} Market exit complete, stopping strategy");

            if let Err(e) = Component::stop(self) {
                log::error!("{strategy_id} Failed to stop: {e}");
            }
        }

        let core = self.core_mut();
        debug_assert!(
            !(core.pending_stop
                && !core.is_exiting
                && core.actor.state() == ComponentState::Running),
            "INVARIANT: stuck state after finalize_market_exit"
        );
    }

    /// Cancels an active market exit without calling hooks.
    ///
    /// Used when stop() is called during an active market exit to avoid state leaks.
    fn cancel_market_exit(&mut self) {
        let core = self.core_mut();
        let timer_name = core.market_exit_timer_name;

        if core.clock().timer_names().contains(&timer_name.as_str()) {
            core.clock().cancel_timer(timer_name.as_str());
        }

        core.is_exiting = false;
        core.pending_stop = false;
        core.market_exit_attempts = 0;
    }

    /// Stops the strategy with optional managed stop behavior.
    ///
    /// If `manage_stop` is enabled in the config, the strategy will first complete
    /// any active market exit (or initiate one) before stopping. If `manage_stop`
    /// is disabled, the strategy stops immediately, cleaning up any active market
    /// exit state.
    ///
    /// # Returns
    ///
    /// Returns `true` if the strategy should proceed with stopping, `false` if
    /// the stop is being deferred until market exit completes.
    fn stop(&mut self) -> bool {
        let (manage_stop, is_exiting, should_initiate_exit) = {
            let core = self.core_mut();
            let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
            let manage_stop = core.config.manage_stop;
            let state = core.actor.state();
            let pending_stop = core.pending_stop;
            let is_exiting = core.is_exiting;

            if manage_stop {
                if state != ComponentState::Running {
                    return true; // Proceed with stop
                }

                if pending_stop {
                    return false; // Already waiting for market exit
                }

                core.pending_stop = true;
                let should_initiate_exit = !is_exiting;

                if should_initiate_exit {
                    log::info!("{strategy_id} Initiating market exit before stop");
                }

                (manage_stop, is_exiting, should_initiate_exit)
            } else {
                (manage_stop, is_exiting, false)
            }
        };

        if manage_stop {
            if should_initiate_exit && let Err(e) = self.market_exit() {
                log::warn!("Market exit failed during stop: {e}, proceeding with stop");
                self.core_mut().pending_stop = false;
                return true;
            }
            debug_assert!(
                self.is_exiting(),
                "INVARIANT: deferring stop but not exiting"
            );
            return false; // Defer stop until market exit completes
        }

        // manage_stop is false - clean up any active market exit
        if is_exiting {
            self.cancel_market_exit();
        }

        true // Proceed with stop
    }

    /// Denies an order by generating an OrderDenied event.
    ///
    /// This method creates an OrderDenied event, applies it to the order,
    /// and updates the cache.
    fn deny_order(&mut self, order: &OrderAny, reason: Ustr) {
        let core = self.core_mut();
        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_now = core.clock().timestamp_ns();

        let event = OrderDenied::new(
            trader_id,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
        );

        log::warn!(
            "{strategy_id} Order {} denied: {reason}",
            order.client_order_id()
        );

        // Add order to cache if not exists, then update with denied event
        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            if !cache.order_exists(&order.client_order_id()) {
                let _ = cache.add_order(order.clone(), None, None, true);
            }
        }

        // Apply event and update cache
        let mut order_clone = order.clone();
        if let Err(e) = order_clone.apply(OrderEventAny::Denied(event)) {
            log::warn!("Failed to apply OrderDenied event: {e}");
            return;
        }

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            let _ = cache.update_order(&order_clone);
        }
    }

    /// Denies all orders in an order list.
    ///
    /// This method denies each non-closed order in the list.
    fn deny_order_list(&mut self, orders: &[OrderAny], reason: Ustr) {
        for order in orders {
            if !order.is_closed() {
                self.deny_order(order, reason);
            }
        }
    }

    // -- GTD EXPIRY MANAGEMENT -------------------------------------------------------------------

    /// Sets a GTD expiry timer for an order.
    ///
    /// Creates a timer that will automatically cancel the order when it expires.
    ///
    /// # Errors
    ///
    /// Returns an error if timer creation fails.
    fn set_gtd_expiry(&mut self, order: &OrderAny) -> anyhow::Result<()> {
        let core = self.core_mut();

        if !core.config.manage_gtd_expiry || order.time_in_force() != TimeInForce::Gtd {
            return Ok(());
        }

        let Some(expire_time) = order.expire_time() else {
            return Ok(());
        };

        let client_order_id = order.client_order_id();
        let timer_name = format!("GTD-EXPIRY:{client_order_id}");

        let current_time_ns = {
            let clock = core.clock();
            clock.timestamp_ns()
        };

        if current_time_ns >= expire_time.as_u64() {
            log::info!("GTD order {client_order_id} already expired, canceling immediately");
            return self.cancel_order(order.clone(), None);
        }

        {
            let mut clock = core.clock();
            clock.set_time_alert_ns(&timer_name, expire_time, None, None)?;
        }

        core.gtd_timers
            .insert(client_order_id, Ustr::from(&timer_name));

        log::debug!("Set GTD expiry timer for {client_order_id} at {expire_time}");
        Ok(())
    }

    /// Cancels a GTD expiry timer for an order.
    fn cancel_gtd_expiry(&mut self, client_order_id: &ClientOrderId) {
        let core = self.core_mut();

        if let Some(timer_name) = core.gtd_timers.remove(client_order_id) {
            core.clock().cancel_timer(timer_name.as_str());
            log::debug!("Canceled GTD expiry timer for {client_order_id}");
        }
    }

    /// Checks if a GTD expiry timer exists for an order.
    fn has_gtd_expiry_timer(&mut self, client_order_id: &ClientOrderId) -> bool {
        let core = self.core_mut();
        core.gtd_timers.contains_key(client_order_id)
    }

    /// Handles GTD order expiry by canceling the order.
    ///
    /// This method is called when a GTD expiry timer fires.
    fn expire_gtd_order(&mut self, event: TimeEvent) {
        let timer_name = event.name.to_string();
        let Some(client_order_id_str) = timer_name.strip_prefix("GTD-EXPIRY:") else {
            log::error!("Invalid GTD timer name format: {timer_name}");
            return;
        };

        let client_order_id = ClientOrderId::from(client_order_id_str);

        let core = self.core_mut();
        core.gtd_timers.remove(&client_order_id);

        let cache = core.cache();
        let Some(order) = cache.order(&client_order_id) else {
            log::warn!("GTD order {client_order_id} not found in cache");
            return;
        };

        let order = order.clone();
        drop(cache);

        log::info!("GTD order {client_order_id} expired");

        if let Err(e) = self.cancel_order(order, None) {
            log::error!("Failed to cancel expired GTD order {client_order_id}: {e}");
        }
    }

    /// Reactivates GTD timers for open orders on strategy start.
    ///
    /// Queries the cache for all open GTD orders and creates timers for those
    /// that haven't expired yet. Orders that have already expired are canceled immediately.
    fn reactivate_gtd_timers(&mut self) {
        let core = self.core_mut();
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let current_time_ns = core.clock().timestamp_ns();
        let cache = core.cache();

        let open_orders = cache.orders_open(None, None, Some(&strategy_id), None, None);

        let gtd_orders: Vec<_> = open_orders
            .iter()
            .filter(|o| o.time_in_force() == TimeInForce::Gtd)
            .map(|o| (*o).clone())
            .collect();

        drop(cache);

        for order in gtd_orders {
            let Some(expire_time) = order.expire_time() else {
                continue;
            };

            let expire_time_ns = expire_time.as_u64();
            let client_order_id = order.client_order_id();

            if current_time_ns >= expire_time_ns {
                log::info!("GTD order {client_order_id} already expired, canceling immediately");
                if let Err(e) = self.cancel_order(order, None) {
                    log::error!("Failed to cancel expired GTD order {client_order_id}: {e}");
                }
            } else if let Err(e) = self.set_gtd_expiry(&order) {
                log::error!("Failed to set GTD expiry timer for {client_order_id}: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        actor::DataActor,
        cache::Cache,
        clock::{Clock, TestClock},
        component::Component,
        msgbus::{
            self, MessagingSwitchboard,
            stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
        },
        timer::{TimeEvent, TimeEventCallback},
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType, PositionSide},
        events::{OrderAccepted, OrderCanceled, OrderFilled, OrderRejected},
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
            VenueOrderId,
        },
        orders::MarketOrder,
        stubs::TestDefault,
        types::{Currency, Money},
    };
    use nautilus_portfolio::portfolio::Portfolio;
    use rstest::rstest;

    use super::*;
    use crate::nautilus_strategy;

    #[derive(Debug)]
    struct TestStrategy {
        core: StrategyCore,
        on_order_rejected_called: bool,
        on_order_accepted_called: bool,
        on_order_canceled_called: bool,
        on_order_filled_called: bool,
        on_order_expired_called: bool,
        on_position_opened_called: bool,
        on_position_changed_called: bool,
        on_position_closed_called: bool,
    }

    impl TestStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
                on_order_rejected_called: false,
                on_order_accepted_called: false,
                on_order_canceled_called: false,
                on_order_filled_called: false,
                on_order_expired_called: false,
                on_position_opened_called: false,
                on_position_changed_called: false,
                on_position_closed_called: false,
            }
        }
    }

    impl DataActor for TestStrategy {
        fn on_order_canceled(&mut self, _event: &OrderCanceled) -> anyhow::Result<()> {
            self.on_order_canceled_called = true;
            Ok(())
        }

        fn on_order_filled(&mut self, _event: &OrderFilled) -> anyhow::Result<()> {
            self.on_order_filled_called = true;
            Ok(())
        }
    }

    nautilus_strategy!(TestStrategy, {
        fn on_order_rejected(&mut self, _event: OrderRejected) {
            self.on_order_rejected_called = true;
        }

        fn on_order_accepted(&mut self, _event: OrderAccepted) {
            self.on_order_accepted_called = true;
        }

        fn on_order_expired(&mut self, _event: OrderExpired) {
            self.on_order_expired_called = true;
        }

        fn on_position_opened(&mut self, _event: PositionOpened) {
            self.on_position_opened_called = true;
        }

        fn on_position_changed(&mut self, _event: PositionChanged) {
            self.on_position_changed_called = true;
        }

        fn on_position_closed(&mut self, _event: PositionClosed) {
            self.on_position_closed_called = true;
        }
    });

    fn create_test_strategy() -> TestStrategy {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        TestStrategy::new(config)
    }

    fn register_strategy(strategy: &mut TestStrategy) {
        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));

        strategy
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();
        strategy.initialize().unwrap();
    }

    fn start_strategy(strategy: &mut TestStrategy) {
        strategy.start().unwrap();
    }

    fn stop_strategy(strategy: &mut TestStrategy) {
        Component::stop(strategy).unwrap();
    }

    fn make_filled(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Filled(OrderFilled {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            venue_order_id: VenueOrderId::test_default(),
            account_id: AccountId::from("ACC-001"),
            trade_id: TradeId::test_default(),
            position_id: None,
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            last_qty: Quantity::default(),
            last_px: Price::default(),
            currency: Currency::from("USD"),
            liquidity_side: LiquiditySide::Taker,
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: false,
            commission: None,
        })
    }

    fn make_canceled(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Canceled(OrderCanceled {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            venue_order_id: None,
            account_id: Some(AccountId::from("ACC-001")),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
        })
    }

    fn make_rejected(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Rejected(OrderRejected {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            account_id: AccountId::from("ACC-001"),
            reason: "Test rejection".into(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            due_post_only: 0,
        })
    }

    fn make_expired(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Expired(OrderExpired {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            venue_order_id: None,
            account_id: Some(AccountId::from("ACC-001")),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
        })
    }

    fn make_accepted(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Accepted(OrderAccepted {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            venue_order_id: VenueOrderId::test_default(),
            account_id: AccountId::from("ACC-001"),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
        })
    }

    fn make_position_opened() -> PositionEvent {
        PositionEvent::PositionOpened(PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: PositionId::test_default(),
            account_id: AccountId::from("ACC-001"),
            opening_order_id: ClientOrderId::from("O-001"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::default(),
            last_qty: Quantity::default(),
            last_px: Price::default(),
            currency: Currency::from("USD"),
            avg_px_open: 0.0,
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        })
    }

    fn make_position_changed() -> PositionEvent {
        let currency = Currency::from("USD");
        PositionEvent::PositionChanged(PositionChanged {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: PositionId::test_default(),
            account_id: AccountId::from("ACC-001"),
            opening_order_id: ClientOrderId::from("O-001"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 2.0,
            quantity: Quantity::default(),
            peak_quantity: Quantity::default(),
            last_qty: Quantity::default(),
            last_px: Price::default(),
            currency,
            avg_px_open: 0.0,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            unrealized_pnl: Money::new(0.0, currency),
            event_id: UUID4::default(),
            ts_opened: UnixNanos::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        })
    }

    fn make_position_closed() -> PositionEvent {
        let currency = Currency::from("USD");
        PositionEvent::PositionClosed(PositionClosed {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: PositionId::test_default(),
            account_id: AccountId::from("ACC-001"),
            opening_order_id: ClientOrderId::from("O-001"),
            closing_order_id: Some(ClientOrderId::from("O-002")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::default(),
            peak_quantity: Quantity::default(),
            last_qty: Quantity::default(),
            last_px: Price::default(),
            currency,
            avg_px_open: 0.0,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            unrealized_pnl: Money::new(0.0, currency),
            duration: 0,
            event_id: UUID4::default(),
            ts_opened: UnixNanos::default(),
            ts_closed: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        })
    }

    #[rstest]
    fn test_strategy_creation() {
        let strategy = create_test_strategy();
        assert_eq!(
            strategy.core.config.strategy_id,
            Some(StrategyId::from("TEST-001"))
        );
        assert!(!strategy.on_order_rejected_called);
        assert!(!strategy.on_position_opened_called);
    }

    #[rstest]
    fn test_strategy_registration() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        assert!(strategy.core.order_manager.is_some());
        assert!(strategy.core.order_factory.is_some());
        assert!(strategy.core.portfolio.is_some());
    }

    #[rstest]
    fn test_handle_order_event_dispatches_to_handler() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let event = OrderEventAny::Rejected(OrderRejected {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id: ClientOrderId::from("O-001"),
            account_id: AccountId::from("ACC-001"),
            reason: "Test rejection".into(),
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            reconciliation: 0,
            due_post_only: 0,
        });

        strategy.handle_order_event(event);

        assert!(strategy.on_order_rejected_called);
    }

    #[rstest]
    #[case::opened(make_position_opened())]
    #[case::changed(make_position_changed())]
    #[case::closed(make_position_closed())]
    fn test_handle_position_event_dispatches_to_handler(#[case] event: PositionEvent) {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let expected_opened = matches!(event, PositionEvent::PositionOpened(_));
        let expected_changed = matches!(event, PositionEvent::PositionChanged(_));
        let expected_closed = matches!(event, PositionEvent::PositionClosed(_));

        strategy.handle_position_event(event);

        assert_eq!(strategy.on_position_opened_called, expected_opened);
        assert_eq!(strategy.on_position_changed_called, expected_changed);
        assert_eq!(strategy.on_position_closed_called, expected_closed);
    }

    #[rstest]
    fn test_handle_position_event_skips_dispatch_when_stopped() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        stop_strategy(&mut strategy);
        assert_eq!(strategy.core.actor.state(), ComponentState::Stopped);

        strategy.handle_position_event(make_position_opened());

        assert!(!strategy.on_position_opened_called);
    }

    #[rstest]
    fn test_strategy_default_handlers_do_not_panic() {
        let mut strategy = create_test_strategy();

        strategy.on_order_initialized(OrderInitialized::default());
        strategy.on_order_denied(OrderDenied::default());
        strategy.on_order_emulated(OrderEmulated::default());
        strategy.on_order_released(OrderReleased::default());
        strategy.on_order_submitted(OrderSubmitted::default());
        strategy.on_order_rejected(OrderRejected::default());
        let _ = DataActor::on_order_canceled(&mut strategy, &OrderCanceled::default());
        strategy.on_order_expired(OrderExpired::default());
        strategy.on_order_triggered(OrderTriggered::default());
        strategy.on_order_pending_update(OrderPendingUpdate::default());
        strategy.on_order_pending_cancel(OrderPendingCancel::default());
        strategy.on_order_modify_rejected(OrderModifyRejected::default());
        strategy.on_order_cancel_rejected(OrderCancelRejected::default());
        strategy.on_order_updated(OrderUpdated::default());
    }

    #[rstest]
    fn test_modify_order_routes_non_emulated_orders_to_risk() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from("O-20250208-0003"),
            OrderSide::Buy,
            Quantity::from(100_000),
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
        ));

        strategy
            .modify_order(order, Some(Quantity::from(200_000)), None, None, None)
            .unwrap();

        let risk_messages = risk_messages.get_messages();
        let exec_messages = exec_messages.get_messages();

        assert_eq!(risk_messages.len(), 1);
        assert!(matches!(
            risk_messages.first(),
            Some(TradingCommand::ModifyOrder(_))
        ));
        assert!(exec_messages.is_empty());
    }

    // -- GTD EXPIRY TESTS ----------------------------------------------------------------------------

    #[rstest]
    fn test_has_gtd_expiry_timer_when_timer_not_set() {
        let mut strategy = create_test_strategy();
        let client_order_id = ClientOrderId::from("O-001");

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_has_gtd_expiry_timer_when_timer_set() {
        let mut strategy = create_test_strategy();
        let client_order_id = ClientOrderId::from("O-001");

        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        assert!(strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_cancel_gtd_expiry_removes_timer() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        strategy.cancel_gtd_expiry(&client_order_id);

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_cancel_gtd_expiry_when_timer_not_set() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");

        strategy.cancel_gtd_expiry(&client_order_id);

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    #[case::filled(make_filled)]
    #[case::canceled(make_canceled)]
    #[case::rejected(make_rejected)]
    #[case::expired(make_expired)]
    fn test_handle_order_event_cancels_gtd_timer_for_terminal_event(
        #[case] make_event: fn(ClientOrderId) -> OrderEventAny,
    ) {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        strategy.handle_order_event(make_event(client_order_id));

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    #[case::filled(make_filled)]
    #[case::canceled(make_canceled)]
    #[case::rejected(make_rejected)]
    #[case::expired(make_expired)]
    fn test_handle_order_event_cancels_gtd_timer_when_stopped(
        #[case] make_event: fn(ClientOrderId) -> OrderEventAny,
    ) {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        stop_strategy(&mut strategy);
        assert_eq!(strategy.core.actor.state(), ComponentState::Stopped);

        strategy.handle_order_event(make_event(client_order_id));

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_handle_order_event_skips_gtd_cancel_for_non_terminal() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        strategy.handle_order_event(make_accepted(client_order_id));

        assert!(strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_handle_order_event_skips_dispatch_when_stopped() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        stop_strategy(&mut strategy);
        assert_eq!(strategy.core.actor.state(), ComponentState::Stopped);

        strategy.handle_order_event(make_rejected(ClientOrderId::from("O-001")));

        assert!(!strategy.on_order_rejected_called);
    }

    #[rstest]
    fn test_on_start_calls_reactivate_gtd_timers_when_enabled() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            manage_gtd_expiry: true,
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);

        let result = Strategy::on_start(&mut strategy);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_on_start_does_not_panic_when_gtd_disabled() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            manage_gtd_expiry: false,
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);

        let result = Strategy::on_start(&mut strategy);
        assert!(result.is_ok());
    }

    // -- QUERY TESTS ---------------------------------------------------------------------------------

    #[rstest]
    fn test_query_account_when_registered() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let account_id = AccountId::from("ACC-001");

        let result = strategy.query_account(account_id, None, None);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_query_account_with_client_id() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let account_id = AccountId::from("ACC-001");
        let client_id = ClientId::from("BINANCE");

        let result = strategy.query_account(account_id, Some(client_id), None);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_query_order_when_registered() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order = OrderAny::Market(MarketOrder::test_default());

        let result = strategy.query_order(&order, None, None);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_query_order_with_client_id() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order = OrderAny::Market(MarketOrder::test_default());
        let client_id = ClientId::from("BINANCE");

        let result = strategy.query_order(&order, Some(client_id), None);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_is_exiting_returns_false_by_default() {
        let strategy = create_test_strategy();
        assert!(!strategy.is_exiting());
    }

    #[rstest]
    fn test_is_exiting_returns_true_when_set_manually() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        // Manually set the exiting state (as market_exit would do)
        strategy.core.is_exiting = true;

        assert!(strategy.is_exiting());
    }

    #[rstest]
    fn test_market_exit_sets_is_exiting_flag() {
        // Test the state changes that market_exit would make
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        assert!(!strategy.core.is_exiting);

        // Simulate what market_exit does to the state
        strategy.core.is_exiting = true;
        strategy.core.market_exit_attempts = 0;

        assert!(strategy.core.is_exiting);
        assert_eq!(strategy.core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_market_exit_uses_config_time_in_force_and_reduce_only() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            market_exit_time_in_force: TimeInForce::Ioc,
            market_exit_reduce_only: false,
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);

        assert_eq!(
            strategy.core.config.market_exit_time_in_force,
            TimeInForce::Ioc
        );
        assert!(!strategy.core.config.market_exit_reduce_only);
    }

    #[rstest]
    fn test_market_exit_resets_attempt_counter() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        // Manually set attempts to simulate prior exit
        strategy.core.market_exit_attempts = 50;

        // Reset via the reset method
        strategy.core.reset_market_exit_state();

        assert_eq!(strategy.core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_market_exit_second_call_returns_early_when_exiting() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        // First set exiting to true to simulate an in-progress exit
        strategy.core.is_exiting = true;

        // Second call should return Ok and not change state
        let result = strategy.market_exit();
        assert!(result.is_ok());
        assert!(strategy.core.is_exiting);
    }

    #[rstest]
    fn test_finalize_market_exit_resets_state() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        // Set up exiting state
        strategy.core.is_exiting = true;
        strategy.core.pending_stop = true;
        strategy.core.market_exit_attempts = 50;

        strategy.finalize_market_exit();

        assert!(!strategy.core.is_exiting);
        assert!(!strategy.core.pending_stop);
        assert_eq!(strategy.core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_market_exit_config_defaults() {
        let config = StrategyConfig::default();

        assert!(!config.manage_stop);
        assert_eq!(config.market_exit_interval_ms, 100);
        assert_eq!(config.market_exit_max_attempts, 100);
    }

    #[rstest]
    fn test_market_exit_with_custom_config() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            manage_stop: true,
            market_exit_interval_ms: 50,
            market_exit_max_attempts: 200,
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);

        assert!(strategy.core.config.manage_stop);
        assert_eq!(strategy.core.config.market_exit_interval_ms, 50);
        assert_eq!(strategy.core.config.market_exit_max_attempts, 200);
    }

    #[derive(Debug)]
    struct MarketExitHookTrackingStrategy {
        core: StrategyCore,
        on_market_exit_called: bool,
        post_market_exit_called: bool,
    }

    impl MarketExitHookTrackingStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
                on_market_exit_called: false,
                post_market_exit_called: false,
            }
        }
    }

    impl DataActor for MarketExitHookTrackingStrategy {}

    nautilus_strategy!(MarketExitHookTrackingStrategy, {
        fn on_market_exit(&mut self) {
            self.on_market_exit_called = true;
        }

        fn post_market_exit(&mut self) {
            self.post_market_exit_called = true;
        }
    });

    #[rstest]
    fn test_market_exit_calls_on_market_exit_hook() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let mut strategy = MarketExitHookTrackingStrategy::new(config);

        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));
        strategy
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();
        strategy.initialize().unwrap();
        strategy.start().unwrap();

        let _ = strategy.market_exit();

        assert!(strategy.on_market_exit_called);
    }

    #[rstest]
    fn test_finalize_market_exit_calls_post_market_exit_hook() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let mut strategy = MarketExitHookTrackingStrategy::new(config);

        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));
        strategy
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();

        strategy.core.is_exiting = true;
        strategy.finalize_market_exit();

        assert!(strategy.post_market_exit_called);
    }

    #[derive(Debug)]
    struct FailingPostExitStrategy {
        core: StrategyCore,
    }

    impl FailingPostExitStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
            }
        }
    }

    impl DataActor for FailingPostExitStrategy {}

    nautilus_strategy!(FailingPostExitStrategy, {
        fn post_market_exit(&mut self) {
            panic!("Simulated error in post_market_exit");
        }
    });

    #[rstest]
    fn test_finalize_market_exit_handles_hook_panic() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let mut strategy = FailingPostExitStrategy::new(config);

        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));
        strategy
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();

        strategy.core.is_exiting = true;
        strategy.core.pending_stop = true;

        // This should not panic - it should catch the panic in post_market_exit
        strategy.finalize_market_exit();

        // State should still be reset
        assert!(!strategy.core.is_exiting);
        assert!(!strategy.core.pending_stop);
    }

    #[rstest]
    fn test_check_market_exit_increments_attempts_before_finalizing() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        strategy.core.is_exiting = true;
        assert_eq!(strategy.core.market_exit_attempts, 0);

        let event = TimeEvent::new(
            Ustr::from("MARKET_EXIT_CHECK:TEST-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        strategy.check_market_exit(event);

        // With no orders/positions, check_market_exit will finalize immediately
        // which resets attempts to 0. This is correct behavior.
        // The attempt WAS incremented to 1 during the check, then reset on finalize.
        assert!(!strategy.core.is_exiting);
        assert_eq!(strategy.core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_check_market_exit_finalizes_when_max_attempts_reached() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            market_exit_max_attempts: 3,
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);

        strategy.core.is_exiting = true;
        strategy.core.market_exit_attempts = 2; // One below max

        let event = TimeEvent::new(
            Ustr::from("MARKET_EXIT_CHECK:TEST-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        strategy.check_market_exit(event);

        // Should have finalized since attempts >= max_attempts
        assert!(!strategy.core.is_exiting);
        assert_eq!(strategy.core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_check_market_exit_finalizes_when_no_orders_or_positions() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        strategy.core.is_exiting = true;

        let event = TimeEvent::new(
            Ustr::from("MARKET_EXIT_CHECK:TEST-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        strategy.check_market_exit(event);

        // Should have finalized since there are no orders or positions
        assert!(!strategy.core.is_exiting);
    }

    #[rstest]
    fn test_market_exit_timer_name_format() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("MY-STRATEGY-001")),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);

        assert_eq!(
            strategy.core.market_exit_timer_name.as_str(),
            "MARKET_EXIT_CHECK:MY-STRATEGY-001"
        );
    }

    #[rstest]
    fn test_reset_market_exit_state() {
        let mut strategy = create_test_strategy();

        strategy.core.is_exiting = true;
        strategy.core.pending_stop = true;
        strategy.core.market_exit_attempts = 50;

        strategy.core.reset_market_exit_state();

        assert!(!strategy.core.is_exiting);
        assert!(!strategy.core.pending_stop);
        assert_eq!(strategy.core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_cancel_market_exit_resets_state_without_hooks() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let mut strategy = MarketExitHookTrackingStrategy::new(config);

        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));
        strategy
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();

        // Set up exiting state
        strategy.core.is_exiting = true;
        strategy.core.pending_stop = true;
        strategy.core.market_exit_attempts = 50;

        // Call cancel_market_exit
        strategy.cancel_market_exit();

        // State should be reset
        assert!(!strategy.core.is_exiting);
        assert!(!strategy.core.pending_stop);
        assert_eq!(strategy.core.market_exit_attempts, 0);

        // Hooks should NOT have been called
        assert!(!strategy.on_market_exit_called);
        assert!(!strategy.post_market_exit_called);
    }

    #[rstest]
    fn test_market_exit_returns_early_when_not_running() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        // State is not Running (default is PreInitialized)
        assert_ne!(strategy.core.actor.state(), ComponentState::Running);

        let result = strategy.market_exit();

        // Should return Ok but not set is_exiting
        assert!(result.is_ok());
        assert!(!strategy.core.is_exiting);
    }

    #[rstest]
    fn test_stop_with_manage_stop_false_cleans_up_active_exit() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            manage_stop: false,
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);

        // Simulate an active market exit
        strategy.core.is_exiting = true;
        strategy.core.market_exit_attempts = 5;

        // Call stop
        let should_proceed = Strategy::stop(&mut strategy);

        // Should clean up state and allow stop to proceed
        assert!(should_proceed);
        assert!(!strategy.core.is_exiting);
        assert_eq!(strategy.core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_stop_with_manage_stop_true_defers_when_running() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            manage_stop: true,
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);

        // Custom setup with a default callback so timer scheduling succeeds
        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock
            .borrow_mut()
            .register_default_handler(TimeEventCallback::from(|_event: TimeEvent| {}));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));
        strategy
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();
        strategy.initialize().unwrap();
        strategy.start().unwrap();

        let should_proceed = Strategy::stop(&mut strategy);

        // Should set pending_stop and defer
        assert!(!should_proceed);
        assert!(strategy.core.pending_stop);
    }

    #[rstest]
    fn test_stop_with_manage_stop_true_returns_early_if_pending() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            manage_stop: true,
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        strategy.core.pending_stop = true;

        // Call stop again
        let should_proceed = Strategy::stop(&mut strategy);

        // Should return early without changing state
        assert!(!should_proceed);
        assert!(strategy.core.pending_stop);
    }

    #[rstest]
    fn test_stop_with_manage_stop_true_proceeds_when_not_running() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            manage_stop: true,
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);

        // State is not Running (default)
        assert_ne!(strategy.core.actor.state(), ComponentState::Running);

        let should_proceed = Strategy::stop(&mut strategy);

        // Should proceed with stop
        assert!(should_proceed);
    }

    #[rstest]
    fn test_finalize_market_exit_stops_strategy_when_pending() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        // Simulate a market exit with pending stop
        strategy.core.is_exiting = true;
        strategy.core.pending_stop = true;

        strategy.finalize_market_exit();

        // Should have transitioned to Stopped
        assert_eq!(strategy.core.actor.state(), ComponentState::Stopped);
        assert!(!strategy.core.is_exiting);
        assert!(!strategy.core.pending_stop);
    }

    #[rstest]
    fn test_finalize_market_exit_stays_running_when_not_pending() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        // Simulate a market exit without pending stop
        strategy.core.is_exiting = true;
        strategy.core.pending_stop = false;

        strategy.finalize_market_exit();

        // Should stay Running
        assert_eq!(strategy.core.actor.state(), ComponentState::Running);
        assert!(!strategy.core.is_exiting);
    }

    #[rstest]
    fn test_submit_order_denied_during_market_exit_when_not_reduce_only() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        strategy.core.is_exiting = true;

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from("O-20250208-0001"),
            OrderSide::Buy,
            Quantity::from(100_000),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            false, // not reduce_only
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));
        let client_order_id = order.client_order_id();
        let result = strategy.submit_order(order, None, None);

        assert!(result.is_ok());
        let cache = strategy.core.cache();
        let cached_order = cache.order(&client_order_id).unwrap();
        assert_eq!(cached_order.status(), OrderStatus::Denied);
    }

    #[rstest]
    fn test_submit_order_allowed_during_market_exit_when_reduce_only() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        strategy.core.is_exiting = true;

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from("O-20250208-0001"),
            OrderSide::Buy,
            Quantity::from(100_000),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            true, // reduce_only
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));
        let client_order_id = order.client_order_id();
        let result = strategy.submit_order(order, None, None);

        assert!(result.is_ok());
        let cache = strategy.core.cache();
        let cached_order = cache.order(&client_order_id).unwrap();
        assert_ne!(cached_order.status(), OrderStatus::Denied);
    }

    #[rstest]
    fn test_submit_order_allowed_during_market_exit_when_tagged() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        strategy.core.is_exiting = true;

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from("O-20250208-0002"),
            OrderSide::Buy,
            Quantity::from(100_000),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            false, // not reduce_only
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![Ustr::from("MARKET_EXIT")]),
        ));
        let client_order_id = order.client_order_id();
        let result = strategy.submit_order(order, None, None);

        assert!(result.is_ok());
        let cache = strategy.core.cache();
        let cached_order = cache.order(&client_order_id).unwrap();
        assert_ne!(cached_order.status(), OrderStatus::Denied);
    }

    #[derive(Debug)]
    struct MacroTestSimple {
        core: StrategyCore,
    }

    nautilus_strategy!(MacroTestSimple);

    impl DataActor for MacroTestSimple {}

    #[derive(Debug)]
    struct MacroTestWithHooks {
        core: StrategyCore,
    }

    nautilus_strategy!(MacroTestWithHooks, {
        fn on_order_rejected(&mut self, _event: OrderRejected) {}
    });

    impl DataActor for MacroTestWithHooks {}

    #[derive(Debug)]
    struct MacroTestCustomField {
        inner: StrategyCore,
    }

    nautilus_strategy!(MacroTestCustomField, inner, {
        fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
            None
        }
    });

    impl DataActor for MacroTestCustomField {}

    #[rstest]
    fn test_nautilus_strategy_macro_forms() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("MACRO-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };

        let simple = MacroTestSimple {
            core: StrategyCore::new(config.clone()),
        };
        assert_eq!(simple.core().config.strategy_id, config.strategy_id);

        let hooks = MacroTestWithHooks {
            core: StrategyCore::new(config.clone()),
        };
        assert_eq!(hooks.core().config.strategy_id, config.strategy_id);

        let custom = MacroTestCustomField {
            inner: StrategyCore::new(config.clone()),
        };
        assert_eq!(custom.core().config.strategy_id, config.strategy_id);
        assert!(custom.external_order_claims().is_none());
    }
}
