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

pub mod api;
pub mod config;
pub mod core;

pub use core::{StrategyCore, StrategyNative};
use std::panic::{AssertUnwindSafe, catch_unwind};

use ahash::AHashSet;
pub use api::{OrderApi, PortfolioApi};
pub use config::{ImportableStrategyConfig, StrategyConfig};
use nautilus_common::{
    actor::DataActor,
    component::Component,
    enums::ComponentState,
    logging::{CMD, EVT, RECV, SEND},
    messages::execution::{
        BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, ModifyOrder,
        QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
    msgbus::{self, MessagingSwitchboard},
    timer::TimeEvent,
};
use nautilus_core::{Params, UUID4};
use nautilus_execution::order_manager::OrderManagerAction;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, PositionSide, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSubmitted, OrderTriggered, OrderUpdated, PositionChanged, PositionClosed,
        PositionEvent, PositionOpened,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId,
        TraderId,
    },
    orders::{
        LIMIT_ORDER_TYPES, Order, OrderAny, OrderCore, OrderError, OrderList, STOP_ORDER_TYPES,
    },
    position::Position,
    types::{Price, Quantity},
};
use ustr::Ustr;

/// Describes one child update in a batch modify request.
pub type BatchModifyOrder = (
    ClientOrderId,
    Option<Quantity>,
    Option<Price>,
    Option<Price>,
);

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
/// - Event routing for orders and emulator events.
///
/// # Implementation
///
/// Use the `nautilus_strategy!` macro to generate the native runtime wiring
/// and `Strategy` implementations. Normal strategy logic should call facade
/// methods such as `strategy_id()`, `clock()`, `cache()`, `order()`, and
/// `portfolio()`. Native runtime code that needs the internal core should use
/// [`StrategyNative`]. For strategies that override additional trait methods,
/// pass them in a block:
///
/// ```ignore
/// nautilus_strategy!(MyStrategy, {
///     fn on_order_rejected(&mut self, event: OrderRejected) {
///         // custom handling
///     }
/// });
/// ```
///
/// Default methods that read or mutate native runtime state carry explicit
/// [`StrategyNative`] and [`Component`] bounds. Implementations that only need
/// behavioral callbacks do not own or implement native runtime state.
pub trait Strategy: DataActor {
    /// Returns the instrument IDs this strategy intends to claim for external order routing.
    ///
    /// Live strategy registration materializes this configuration intent as active cache claims.
    fn external_order_instrument_ids(&self) -> Option<Vec<InstrumentId>> {
        None
    }

    /// Replaces this strategy's active external order claims with `instrument_ids`.
    ///
    /// External orders, fills, and materialized reconciliation activity for matching instrument
    /// IDs are assigned to the strategy. Passing an empty vector releases every claim owned by the
    /// strategy. Existing cached orders keep their assigned strategy ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered, the cache is already borrowed, an
    /// instrument is repeated, or an instrument is claimed by another strategy.
    fn set_external_order_instrument_ids(
        &mut self,
        instrument_ids: Vec<InstrumentId>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        let strategy_id = registered_strategy_id(core)?;
        if !core.actor.is_registered() {
            anyhow::bail!("Strategy {strategy_id} is not registered with a trader");
        }
        let cache = core.cache_rc();
        cache
            .try_borrow_mut()
            .map_err(|e| anyhow::anyhow!("Cannot set external order claims: {e}"))?
            .set_external_order_claims(strategy_id, &instrument_ids)?;
        core.config.external_order_instrument_ids = Some(instrument_ids);
        Ok(())
    }

    /// Returns the runtime strategy ID, when configured or registered.
    fn strategy_id(&self) -> Option<StrategyId>
    where
        Self: StrategyNative,
    {
        StrategyNative::strategy_core(self).strategy_id()
    }

    /// Returns the user-facing order creation API.
    fn order(&self) -> OrderApi<'_>
    where
        Self: StrategyNative,
    {
        StrategyNative::strategy_core(self).order()
    }

    /// Returns the user-facing portfolio read API.
    fn portfolio(&self) -> PortfolioApi<'_>
    where
        Self: StrategyNative,
    {
        StrategyNative::strategy_core(self).portfolio_api()
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
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);

        let trader_id = registered_trader_id(core)?;
        let strategy_id = registered_strategy_id(core)?;
        let ts_init = core.clock_mut().timestamp_ns();

        if order.status() != OrderStatus::Initialized {
            anyhow::bail!(
                "Order denied: invalid status for {}, expected INITIALIZED",
                order.client_order_id()
            );
        }

        let market_exit_tag = core.market_exit_tag;
        let is_market_exit_order = order
            .tags()
            .is_some_and(|tags| tags.contains(&market_exit_tag));
        let should_deny_for_market_exit =
            core.is_exiting && !order.is_reduce_only() && !is_market_exit_order;

        if should_deny_for_market_exit {
            self.deny_order(&order, Ustr::from("MARKET_EXIT_IN_PROGRESS"));
            return Ok(());
        }

        let core = StrategyNative::strategy_core_mut(self);
        let params = params.filter(|params| !params.is_empty());

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.try_borrow_mut().map_err(|_| {
                anyhow::anyhow!(
                    "Cannot submit order {}: cache is currently borrowed",
                    order.client_order_id()
                )
            })?;
            cache.add_order(order.clone(), position_id, client_id, true)?;
        }

        publish_order_initialized(&order);

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
            None, // correlation_id
        );

        if order.emulation_trigger().is_some() {
            send_emulator_command(TradingCommand::SubmitOrder(command));
        } else if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
            send_algo_command(command, exec_algorithm_id);
        } else {
            send_risk_command(TradingCommand::SubmitOrder(command));
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
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        if orders.is_empty() {
            log::error!("OrderList denied: no orders to submit");
            anyhow::bail!("OrderList denied: no orders to submit");
        }

        for order in &orders {
            if order.status() != OrderStatus::Initialized {
                anyhow::bail!(
                    "Order in list denied: invalid status for {}, expected INITIALIZED",
                    order.client_order_id()
                );
            }
        }

        let first_venue = orders[0].instrument_id().venue;
        for order in &orders {
            if order.instrument_id().venue != first_venue {
                anyhow::bail!(
                    "OrderList denied: orders must share the same venue; \
                     expected {first_venue}, found {} on {}",
                    order.instrument_id().venue,
                    order.client_order_id(),
                );
            }
        }

        let should_deny = {
            let core = StrategyNative::strategy_core_mut(self);
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

        let core = StrategyNative::strategy_core_mut(self);

        let trader_id = registered_trader_id(core)?;
        let strategy_id = registered_strategy_id(core)?;
        let ts_init = core.clock_mut().timestamp_ns();

        // TODO: Replace with fluent builder API for order list construction
        let order_list = if orders.first().is_some_and(|o| o.order_list_id().is_some()) {
            OrderList::from_orders(&orders, ts_init)
        } else {
            core.order_factory().create_list(&mut orders, ts_init)
        };

        if let Err(e) = order_list.validate() {
            log::error!("OrderList denied: {e}");
            anyhow::bail!("OrderList denied: {e}");
        }

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.try_borrow_mut().map_err(|_| {
                anyhow::anyhow!(
                    "Cannot submit order list {}: cache is currently borrowed",
                    order_list.id
                )
            })?;

            if cache.order_list_exists(&order_list.id) {
                anyhow::bail!("OrderList denied: duplicate {}", order_list.id);
            }

            for order in &orders {
                if cache.order_exists(&order.client_order_id()) {
                    anyhow::bail!(
                        "Order in list denied: duplicate {}",
                        order.client_order_id()
                    );
                }
            }

            cache.add_order_list(order_list.clone())?;
            for order in &orders {
                cache.add_order(order.clone(), position_id, client_id, true)?;
            }
        }

        for order in &orders {
            publish_order_initialized(order);
        }

        let params = params.filter(|params| !params.is_empty());

        let first_order = orders.first();
        let order_inits: Vec<_> = orders.iter().map(|o| o.init_event().clone()).collect();
        let exec_algorithm_id = first_order.and_then(Order::exec_algorithm_id);

        let command = SubmitOrderList::new(
            trader_id,
            client_id,
            strategy_id,
            order_list,
            order_inits,
            exec_algorithm_id,
            position_id,
            params,
            UUID4::new(),
            ts_init,
            None, // correlation_id
        );

        let has_emulated_order = orders
            .iter()
            .any(|o| o.emulation_trigger().is_some() || o.is_emulated());

        if has_emulated_order {
            send_emulator_command(TradingCommand::SubmitOrderList(command));
        } else if let Some(algo_id) = exec_algorithm_id {
            let endpoint = format!("{algo_id}.execute");
            msgbus::send_any(endpoint.into(), &TradingCommand::SubmitOrderList(command));
        } else {
            send_risk_command(TradingCommand::SubmitOrderList(command));
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
        client_order_id: ClientOrderId,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let (trader_id, strategy_id) = {
            let core = StrategyNative::strategy_core_mut(self);
            (registered_trader_id(core)?, registered_strategy_id(core)?)
        };

        let params = params.filter(|params| !params.is_empty());

        // TODO: Snapshot the order from the cache. See `cancel_order` for the rationale.
        let order = StrategyNative::strategy_core_mut(self)
            .cache_rc()
            .borrow()
            .try_order_owned(&client_order_id)
            .map_err(|e| anyhow::anyhow!("Cannot modify order: {e}"))?;

        let mut updating = false;

        if quantity.is_some_and(|q| q != order.quantity() || order.is_pending_update()) {
            updating = true;
        }

        if let Some(price) = price {
            if !LIMIT_ORDER_TYPES.contains(&order.order_type()) {
                anyhow::bail!("{} orders do not have a LIMIT price", order.order_type());
            }

            if Some(price) != order.price() {
                updating = true;
            }
        }

        if let Some(trigger_price) = trigger_price {
            if !STOP_ORDER_TYPES.contains(&order.order_type()) {
                anyhow::bail!(
                    "{} orders do not have a STOP trigger price",
                    order.order_type()
                );
            }

            if Some(trigger_price) != order.trigger_price() {
                updating = true;
            }
        }

        if !updating {
            log::error!(
                "Cannot create command ModifyOrder: quantity, price, and trigger were either None \
                or the same as existing values"
            );
            return Ok(());
        }

        if order.is_closed() || order.is_pending_cancel() {
            log::warn!(
                "Cannot create command ModifyOrder: state is {:?}, {order:?}",
                order.status()
            );
            return Ok(());
        }

        if !self.mark_order_pending_update(&order)? {
            return Ok(());
        }

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
            StrategyNative::strategy_core_mut(self)
                .clock_mut()
                .timestamp_ns(),
            params,
            None, // correlation_id
        );

        if order.emulation_trigger().is_some() || order.is_emulated() {
            send_emulator_command(TradingCommand::ModifyOrder(command));
        } else if let Some(algo_id) = order
            .exec_algorithm_id()
            .filter(|_| order.is_active_local())
        {
            let endpoint = format!("{algo_id}.execute");
            msgbus::send_any(endpoint.into(), &TradingCommand::ModifyOrder(command));
        } else {
            send_risk_command(TradingCommand::ModifyOrder(command));
        }
        Ok(())
    }

    /// Batch modifies multiple orders for the same instrument.
    ///
    /// Each tuple is `(client_order_id, quantity, price, trigger_price)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered, the orders span multiple instruments,
    /// contain emulated/local orders, or a child modify is invalid.
    fn modify_orders(
        &mut self,
        updates: Vec<BatchModifyOrder>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        if updates.is_empty() {
            anyhow::bail!("Cannot batch modify empty order list");
        }

        let (trader_id, strategy_id, ts_init) = {
            let core = StrategyNative::strategy_core_mut(self);
            (
                registered_trader_id(core)?,
                registered_strategy_id(core)?,
                core.clock_mut().timestamp_ns(),
            )
        };

        let orders: Vec<OrderAny> = {
            let cache_rc = StrategyNative::strategy_core_mut(self).cache_rc();
            let cache = cache_rc.borrow();
            updates
                .iter()
                .map(|(client_order_id, _, _, _)| {
                    cache
                        .try_order_owned(client_order_id)
                        .map_err(|e| anyhow::anyhow!("Cannot modify order: {e}"))
                })
                .collect::<Result<_, _>>()?
        };

        let instrument_id = orders[0].instrument_id();

        for (order, (_, quantity, price, trigger_price)) in orders.iter().zip(updates.iter()) {
            if order.instrument_id() != instrument_id {
                anyhow::bail!(
                    "Cannot batch modify orders for different instruments: {} vs {}",
                    instrument_id,
                    order.instrument_id()
                );
            }

            if order.is_emulated() || order.is_active_local() {
                anyhow::bail!("Cannot include emulated or local orders in batch modify");
            }

            let mut updating = false;

            if quantity.is_some_and(|q| q != order.quantity()) {
                updating = true;
            }

            if let Some(price) = price {
                if !LIMIT_ORDER_TYPES.contains(&order.order_type()) {
                    anyhow::bail!("{} orders do not have a LIMIT price", order.order_type());
                }

                if Some(*price) != order.price() {
                    updating = true;
                }
            }

            if let Some(trigger_price) = trigger_price {
                if !STOP_ORDER_TYPES.contains(&order.order_type()) {
                    anyhow::bail!(
                        "{} orders do not have a STOP trigger price",
                        order.order_type()
                    );
                }

                if Some(*trigger_price) != order.trigger_price() {
                    updating = true;
                }
            }

            if !updating {
                anyhow::bail!(
                    "Cannot create command BatchModifyOrders: quantity, price, and trigger were \
                    either None or the same as existing values for {}",
                    order.client_order_id()
                );
            }

            if order.is_closed() || order.is_pending_cancel() {
                anyhow::bail!(
                    "Cannot create command BatchModifyOrders: state is {:?}, {order:?}",
                    order.status()
                );
            }
        }

        let params = params.filter(|params| !params.is_empty());
        let mut modifies = Vec::with_capacity(orders.len());

        for (order, (_, quantity, price, trigger_price)) in orders.into_iter().zip(updates) {
            if !self.mark_order_pending_update(&order)? {
                continue;
            }

            modifies.push(ModifyOrder::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                order.client_order_id(),
                order.venue_order_id(),
                quantity,
                price,
                trigger_price,
                UUID4::new(),
                ts_init,
                params.clone(),
                None, // correlation_id
            ));
        }

        if modifies.is_empty() {
            log::warn!("Cannot send `BatchModifyOrders`, no valid modify commands");
            return Ok(());
        }

        let command = BatchModifyOrders::new(
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            modifies,
            UUID4::new(),
            ts_init,
            params,
            None, // correlation_id
        );

        send_risk_command(TradingCommand::ModifyOrders(command));
        Ok(())
    }

    /// Cancels an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order cancellation fails.
    fn cancel_order(
        &mut self,
        client_order_id: ClientOrderId,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let (trader_id, strategy_id, ts_init) = {
            let core = StrategyNative::strategy_core_mut(self);
            (
                registered_trader_id(core)?,
                registered_strategy_id(core)?,
                core.clock_mut().timestamp_ns(),
            )
        };

        let params = params.filter(|params| !params.is_empty());

        // TODO: Snapshot the order from the cache. Callers identify it by ID; we own the
        // snapshot so later calls (which take `&OrderAny` and may re-enter the cache)
        // run without holding a live cache borrow.
        let order = StrategyNative::strategy_core_mut(self)
            .cache_rc()
            .borrow()
            .try_order_owned(&client_order_id)
            .map_err(|e| anyhow::anyhow!("Cannot cancel order: {e}"))?;

        if !self.mark_order_pending_cancel(&order)? {
            return Ok(());
        }

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
            None, // correlation_id
        );

        if order.emulation_trigger().is_some() || order.is_emulated() {
            send_emulator_command(TradingCommand::CancelOrder(command));
        } else if let Some(algo_id) = order
            .exec_algorithm_id()
            .filter(|_| order.is_active_local())
        {
            let endpoint = format!("{algo_id}.execute");
            msgbus::send_any(endpoint.into(), &TradingCommand::CancelOrder(command));
        } else {
            send_exec_command(TradingCommand::CancelOrder(command));
        }

        if StrategyNative::strategy_core(self).config.manage_gtd_expiry
            && order.time_in_force() == TimeInForce::Gtd
            && self.has_gtd_expiry_timer(&order.client_order_id())
        {
            self.cancel_gtd_expiry(&order.client_order_id());
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
        client_order_ids: Vec<ClientOrderId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        if client_order_ids.is_empty() {
            anyhow::bail!("Cannot batch cancel empty order list");
        }

        let (trader_id, strategy_id, ts_init) = {
            let core = StrategyNative::strategy_core_mut(self);
            (
                registered_trader_id(core)?,
                registered_strategy_id(core)?,
                core.clock_mut().timestamp_ns(),
            )
        };

        // TODO: Snapshot all orders from the cache. See `cancel_order` for the rationale.
        let orders: Vec<OrderAny> = {
            let cache_rc = StrategyNative::strategy_core_mut(self).cache_rc();
            let cache = cache_rc.borrow();
            client_order_ids
                .iter()
                .map(|id| {
                    cache
                        .try_order_owned(id)
                        .map_err(|e| anyhow::anyhow!("Cannot cancel order: {e}"))
                })
                .collect::<Result<_, _>>()?
        };

        let instrument_id = orders[0].instrument_id();

        for order in &orders {
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
        }

        let mut cancels = Vec::with_capacity(orders.len());

        for order in orders {
            if !self.mark_order_pending_cancel(&order)? {
                continue;
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
                None, // correlation_id
            ));
        }

        if cancels.is_empty() {
            log::warn!("Cannot send `BatchCancelOrders`, no valid cancel commands");
            return Ok(());
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
            None, // correlation_id
        );

        send_exec_command(TradingCommand::CancelOrders(command));
        Ok(())
    }

    /// Marks an order as pending update locally before the modify command leaves the strategy.
    ///
    /// # Errors
    ///
    /// Returns an error if applying the pending update event to the cache fails.
    fn mark_order_pending_update(&mut self, order: &OrderAny) -> anyhow::Result<bool>
    where
        Self: StrategyNative,
    {
        if order.is_active_local() || order.is_pending_update() {
            return Ok(true);
        }

        let strategy_id = order.strategy_id();
        required_account_id(order, "pending update")?;
        let event = OrderEventAny::PendingUpdate(self.generate_order_pending_update(order));

        {
            let cache_rc = StrategyNative::strategy_core_mut(self).cache_rc();
            let mut cache = cache_rc.borrow_mut();
            match cache.update_order(&event) {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.downcast_ref::<OrderError>(),
                        Some(OrderError::InvalidStateTransition)
                    ) =>
                {
                    log::warn!("InvalidStateTrigger: {e}, did not apply pending update event");
                    return Ok(false);
                }
                Err(e) => return Err(e),
            }
        }

        let topic = format!("events.order.{strategy_id}");
        msgbus::publish_order_event(topic.into(), &event);
        msgbus::publish_order_event(
            msgbus::switchboard::get_order_pending_update_topic(order.instrument_id()),
            &event,
        );

        Ok(true)
    }

    /// Marks an order as pending cancel locally before the cancel command leaves the strategy.
    ///
    /// # Errors
    ///
    /// Returns an error if applying the pending cancel event to the cache fails.
    fn mark_order_pending_cancel(&mut self, order: &OrderAny) -> anyhow::Result<bool>
    where
        Self: StrategyNative,
    {
        if order.is_closed() || order.is_pending_cancel() {
            log::warn!(
                "Cannot cancel order: state is {:?}, {order:?}",
                order.status()
            );
            return Ok(false);
        }

        if order.is_active_local() {
            return Ok(true);
        }

        let strategy_id = order.strategy_id();
        required_account_id(order, "pending cancel")?;
        let event = OrderEventAny::PendingCancel(self.generate_order_pending_cancel(order));

        {
            let cache_rc = StrategyNative::strategy_core_mut(self).cache_rc();
            let mut cache = cache_rc.borrow_mut();
            match cache.update_order(&event) {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.downcast_ref::<OrderError>(),
                        Some(OrderError::InvalidStateTransition)
                    ) =>
                {
                    log::warn!("InvalidStateTrigger: {e}, did not apply pending cancel event");
                    return Ok(false);
                }
                Err(e) => return Err(e),
            }
            cache.update_order_pending_cancel_local(order);
        }

        let topic = format!("events.order.{strategy_id}");
        msgbus::publish_order_event(topic.into(), &event);
        msgbus::publish_order_event(
            msgbus::switchboard::get_order_pending_cancel_topic(order.instrument_id()),
            &event,
        );

        Ok(true)
    }

    /// Generates an `OrderPendingUpdate` event for an order.
    fn generate_order_pending_update(&mut self, order: &OrderAny) -> OrderPendingUpdate
    where
        Self: StrategyNative,
    {
        let ts_now = StrategyNative::strategy_core_mut(self)
            .clock_mut()
            .timestamp_ns();

        OrderPendingUpdate::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.account_id(),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            order.venue_order_id(),
        )
    }

    /// Generates an `OrderPendingCancel` event for an order.
    fn generate_order_pending_cancel(&mut self, order: &OrderAny) -> OrderPendingCancel
    where
        Self: StrategyNative,
    {
        let ts_now = StrategyNative::strategy_core_mut(self)
            .clock_mut()
            .timestamp_ns();

        OrderPendingCancel::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.account_id(),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            order.venue_order_id(),
        )
    }

    /// Cancels all open orders for the given instrument.
    ///
    /// When `strategy_only` is `true`, only orders associated with this strategy are canceled. When
    /// `false`, one [`CancelAllOrders`] command is sent even when the cache has no matching order.
    /// The execution engine selects the explicit, venue-routed, or default client and may cancel
    /// orders associated with other strategies for the same instrument, client, and account. It
    /// does not broadcast across execution clients.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order cancellation fails.
    fn cancel_all_orders(
        &mut self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
        client_id: Option<ClientId>,
        strategy_only: bool,
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let params = params.filter(|params| !params.is_empty());
        let core = StrategyNative::strategy_core_mut(self);

        let trader_id = registered_trader_id(core)?;
        let strategy_id = registered_strategy_id(core)?;
        let ts_init = core.clock_mut().timestamp_ns();

        if !strategy_only {
            let command_id = UUID4::new();
            let command = CancelAllOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                order_side,
                command_id,
                ts_init,
                params,
                Some(command_id),
            );

            send_exec_command(TradingCommand::CancelAllOrders(command));
            return Ok(());
        }

        let cache = core.cache_ref();

        let mut open_order_ids: Vec<ClientOrderId> = cache
            .orders_open(
                None,
                Some(&instrument_id),
                Some(&strategy_id),
                None,
                order_side,
            )
            .into_iter()
            .map(|order| order.client_order_id())
            .collect();

        let mut emulated_order_ids: Vec<ClientOrderId> = cache
            .orders_emulated(
                None,
                Some(&instrument_id),
                Some(&strategy_id),
                None,
                order_side,
            )
            .into_iter()
            .map(|order| order.client_order_id())
            .collect();

        let mut inflight_order_ids: Vec<ClientOrderId> = cache
            .orders_inflight(
                None,
                Some(&instrument_id),
                Some(&strategy_id),
                None,
                order_side,
            )
            .into_iter()
            .map(|order| order.client_order_id())
            .collect();

        // Sort the algorithm IDs so the per-algo cancel cascade fires msgbus
        // events in a deterministic order across runs; the cache returns an
        // unordered AHashSet.
        let mut exec_algorithm_ids: Vec<_> = cache.exec_algorithm_ids().into_iter().collect();
        exec_algorithm_ids.sort();
        let mut algo_order_ids: Vec<ClientOrderId> = Vec::new();

        for algo_id in &exec_algorithm_ids {
            algo_order_ids.extend(
                cache
                    .orders_for_exec_algorithm(
                        algo_id,
                        None,
                        Some(&instrument_id),
                        Some(&strategy_id),
                        None,
                        order_side,
                    )
                    .into_iter()
                    .map(|order| order.client_order_id()),
            );
        }

        let matches_client = |client_order_id: &ClientOrderId| {
            client_id.is_none_or(|client_id| {
                cache
                    .client_id(client_order_id)
                    .is_none_or(|order_client_id| *order_client_id == client_id)
            })
        };

        open_order_ids.retain(&matches_client);
        emulated_order_ids.retain(&matches_client);
        inflight_order_ids.retain(&matches_client);
        algo_order_ids.retain(&matches_client);

        let open_count = open_order_ids.len();
        let emulated_count = emulated_order_ids.len();
        let inflight_count = inflight_order_ids.len();
        let algo_count = algo_order_ids.len();

        let mut cancel_routes: Vec<_> = open_order_ids
            .iter()
            .chain(&emulated_order_ids)
            .chain(&inflight_order_ids)
            .chain(&algo_order_ids)
            .map(|client_order_id| {
                (
                    *client_order_id,
                    client_id.or_else(|| cache.client_id(client_order_id).copied()),
                )
            })
            .collect();
        cancel_routes.sort_by_key(|(client_order_id, _)| *client_order_id);
        cancel_routes.dedup_by_key(|(client_order_id, _)| *client_order_id);

        drop(cache);

        if open_count == 0 && emulated_count == 0 && inflight_count == 0 && algo_count == 0 {
            let side_str = order_side.map(|s| format!(" {s}")).unwrap_or_default();
            log::info!("No {instrument_id} open, emulated, or inflight{side_str} orders to cancel");
            return Ok(());
        }

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

        let mut first_error = None;

        for (client_order_id, client_id) in cancel_routes {
            if let Err(e) = self.cancel_order(client_order_id, client_id, params.clone()) {
                if first_error.is_none() {
                    first_error = Some(e);
                } else {
                    log::error!("Error canceling {client_order_id}: {e}");
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    /// Closes a position by submitting a market order for the opposite side.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or position closing fails.
    #[expect(clippy::too_many_arguments)]
    fn close_position(
        &mut self,
        position: &Position,
        client_id: Option<ClientId>,
        tags: Option<Vec<Ustr>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);

        if position.is_closed() {
            log::warn!("Cannot close position (already closed): {}", position.id);
            return Ok(());
        }

        let Some(closing_side) = OrderCore::closing_side(position.side) else {
            log::warn!("Cannot close flat position: {}", position.id);
            return Ok(());
        };

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

        self.submit_order(order, Some(position.id), client_id, params)
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
        params: Option<Params>,
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        let strategy_id = registered_strategy_id(core)?;
        let cache = core.cache_ref();

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
        drop(positions_open);

        drop(cache);

        for (pos_id, pos_instrument_id, pos_side, pos_quantity, is_closed) in positions_data {
            if is_closed {
                continue;
            }

            let core = StrategyNative::strategy_core_mut(self);
            let Some(closing_side) = OrderCore::closing_side(pos_side) else {
                continue;
            };
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

            self.submit_order(order, Some(pos_id), client_id, params.clone())?;
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
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);

        let trader_id = registered_trader_id(core)?;
        let ts_init = core.clock_mut().timestamp_ns();

        let command = QueryAccount::new(
            trader_id,
            client_id,
            account_id,
            UUID4::new(),
            ts_init,
            params,
            None, // correlation_id
        );

        send_exec_command(TradingCommand::QueryAccount(command));
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
    ) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);

        let trader_id = registered_trader_id(core)?;
        let strategy_id = registered_strategy_id(core)?;
        let ts_init = core.clock_mut().timestamp_ns();

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
            None, // correlation_id
        );

        send_exec_command(TradingCommand::QueryOrder(command));
        Ok(())
    }

    /// Handles an order event, dispatching to the appropriate handler.
    fn handle_order_event(&mut self, event: OrderEventAny)
    where
        Self: StrategyNative,
    {
        let state = {
            let core = StrategyNative::strategy_core_mut(self);
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
            } else if core.actor.config.log_events {
                log::info!("{id} {RECV}{EVT} {event}");
            }

            core.actor.state()
        };

        let client_order_id = event.client_order_id();
        let cached_order_is_closed = {
            let core = StrategyNative::strategy_core_mut(self);
            core.cache_ref()
                .order(&client_order_id)
                .map(|order| order.is_closed())
        };
        let is_terminal = match &event {
            OrderEventAny::FillVoided(event) => {
                cached_order_is_closed.unwrap_or(!event.is_reopened)
            }
            OrderEventAny::Filled(_) => cached_order_is_closed.unwrap_or(true),
            OrderEventAny::Canceled(_)
            | OrderEventAny::Rejected(_)
            | OrderEventAny::Expired(_)
            | OrderEventAny::Denied(_) => true,
            _ => false,
        };

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

        if matches!(&event, OrderEventAny::FillVoided(event) if event.is_reopened) {
            let order = StrategyNative::strategy_core_mut(self)
                .cache_ref()
                .order(&client_order_id)
                .map(|order| order.clone());
            if let Some(order) = order
                && order.is_open()
                && !self.has_gtd_expiry_timer(&client_order_id)
                && let Err(e) = self.set_gtd_expiry(&order)
            {
                log::error!(
                    "Failed to restore GTD expiry for reopened order {client_order_id}: {e}"
                );
            }
        }

        let manager_actions = {
            let core = StrategyNative::strategy_core_mut(self);
            if core.config.manage_contingent_orders {
                core.order_manager
                    .as_mut()
                    .map_or_else(Vec::new, |manager| manager.handle_event(&event))
            } else {
                Vec::new()
            }
        };
        self.dispatch_manager_actions(manager_actions);

        match &event {
            OrderEventAny::Initialized(e) => self.on_order_initialized(e.clone()),
            OrderEventAny::Denied(e) => self.on_order_denied(*e),
            OrderEventAny::Emulated(e) => self.on_order_emulated(*e),
            OrderEventAny::Released(e) => self.on_order_released(*e),
            OrderEventAny::Submitted(e) => self.on_order_submitted(*e),
            OrderEventAny::Rejected(e) => self.on_order_rejected(*e),
            OrderEventAny::Accepted(e) => self.on_order_accepted(*e),
            OrderEventAny::Canceled(e) => self.on_order_canceled(e),
            OrderEventAny::Expired(e) => self.on_order_expired(*e),
            OrderEventAny::Triggered(e) => self.on_order_triggered(*e),
            OrderEventAny::PendingUpdate(e) => self.on_order_pending_update(*e),
            OrderEventAny::PendingCancel(e) => self.on_order_pending_cancel(*e),
            OrderEventAny::ModifyRejected(e) => self.on_order_modify_rejected(*e),
            OrderEventAny::CancelRejected(e) => self.on_order_cancel_rejected(*e),
            OrderEventAny::Updated(e) => self.on_order_updated(*e),
            OrderEventAny::Filled(e) => self.on_order_filled(e),
            OrderEventAny::FillVoided(e) => self.on_order_fill_voided(e),
        }
        self.on_order_event(event);
    }

    fn dispatch_manager_actions(&mut self, actions: Vec<OrderManagerAction>)
    where
        Self: StrategyNative,
    {
        for action in actions {
            match action {
                OrderManagerAction::PublishInitialized(event) => {
                    let topic = msgbus::switchboard::get_event_order_topic(event.strategy_id());
                    msgbus::publish_order_event(topic, &event);
                }
                OrderManagerAction::SubmitToEmulator(command) => {
                    send_emulator_command(TradingCommand::SubmitOrder(command));
                }
                OrderManagerAction::SubmitToRisk(command) => {
                    send_risk_command(TradingCommand::SubmitOrder(command));
                }
                OrderManagerAction::SubmitToAlgorithm {
                    command,
                    exec_algorithm_id,
                } => send_algo_command(command, exec_algorithm_id),
                OrderManagerAction::CancelLocal(order) => {
                    let client_order_id = order.client_order_id();
                    if let Err(e) = self.cancel_order(client_order_id, None, None) {
                        log::error!(
                            "Failed to dispatch contingent cancel for {client_order_id}: {e}"
                        );
                    }
                }
                OrderManagerAction::ModifyLocalQuantity { order, quantity } => {
                    let client_order_id = order.client_order_id();
                    if let Err(e) =
                        self.modify_order(client_order_id, Some(quantity), None, None, None, None)
                    {
                        log::error!(
                            "Failed to dispatch contingent modify for {client_order_id}: {e}"
                        );
                    }
                }
            }
        }
    }

    /// Handles a position event, dispatching to the appropriate handler.
    fn handle_position_event(&mut self, event: PositionEvent)
    where
        Self: StrategyNative,
    {
        let state = {
            let core = StrategyNative::strategy_core_mut(self);

            if core.actor.config.log_events {
                let id = &core.actor.actor_id;
                log::info!("{id} {RECV}{EVT} {event:?}");
            }

            core.actor.state()
        };

        if state != ComponentState::Running {
            return;
        }

        match &event {
            PositionEvent::PositionOpened(e) => self.on_position_opened(e.clone()),
            PositionEvent::PositionChanged(e) => self.on_position_changed(e.clone()),
            PositionEvent::PositionClosed(e) => self.on_position_closed(e.clone()),
            PositionEvent::PositionAdjusted(_) => {
                return;
            }
        }
        self.on_position_event(event);
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
    fn on_start(&mut self) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        let strategy_id = registered_strategy_id(core)?;
        log::info!("Starting {strategy_id}");

        if core.config.manage_gtd_expiry {
            self.reactivate_gtd_timers();
        }

        Ok(())
    }

    /// Routes a time event through the framework-managed strategy handlers when called directly.
    ///
    /// Framework timer dispatch never calls this method. Implement [`DataActor::on_time_event`] for
    /// user timer callbacks. Runtime hosts call [`route_time_event`] directly so an override cannot
    /// replace framework-managed GTD expiry or market exit routing.
    ///
    /// # Errors
    ///
    /// Returns an error if time event handling fails.
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()>
    where
        Self: StrategyNative + Component,
    {
        route_time_event(self, event);
        Ok(())
    }

    // -- EVENT HANDLERS --------------------------------------------------------------------------

    /// Called when an order is initialized.
    ///
    /// Override this method to implement custom logic when an order is first created.
    #[allow(unused_variables)]
    fn on_order_initialized(&mut self, event: OrderInitialized) {}

    /// Called when any order event is received after the specific order handler runs.
    ///
    /// Override this method to implement custom logic for all order events.
    #[allow(unused_variables)]
    fn on_order_event(&mut self, event: OrderEventAny) {}

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

    /// Called when an order is canceled.
    ///
    /// Override this method to implement custom logic when an order is canceled.
    #[allow(unused_variables)]
    fn on_order_canceled(&mut self, event: &OrderCanceled) {}

    /// Called when an order is filled.
    ///
    /// Override this method to implement custom logic when an order is filled.
    #[allow(unused_variables)]
    fn on_order_filled(&mut self, event: &OrderFilled) {}

    /// Called when an applied order fill is partly or fully voided.
    #[allow(unused_variables)]
    fn on_order_fill_voided(&mut self, event: &OrderFillVoided) {}

    /// Called when a position is opened.
    ///
    /// Override this method to implement custom logic when a position is opened.
    #[allow(unused_variables)]
    fn on_position_opened(&mut self, event: PositionOpened) {}

    /// Called after a position opened, changed, or closed handler runs.
    ///
    /// Override this method to implement custom logic for all position events.
    #[allow(unused_variables)]
    fn on_position_event(&mut self, event: PositionEvent) {}

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
    fn is_exiting(&self) -> bool
    where
        Self: StrategyNative,
    {
        StrategyNative::strategy_core(self).is_exiting
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
    fn market_exit(&mut self) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        let strategy_id = registered_strategy_id(core)?;

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

        let core = StrategyNative::strategy_core_mut(self);
        let cache = core.cache_ref();

        let mut instruments: AHashSet<InstrumentId> = AHashSet::new();

        for client_order_id in
            cache.iter_client_order_ids_open(None, None, Some(&strategy_id), None)
        {
            if let Some(order) = cache.order(&client_order_id) {
                instruments.insert(order.instrument_id());
            }
        }

        for client_order_id in
            cache.iter_client_order_ids_inflight(None, None, Some(&strategy_id), None)
        {
            if let Some(order) = cache.order(&client_order_id) {
                instruments.insert(order.instrument_id());
            }
        }

        for position_id in cache.iter_position_open_ids(None, None, Some(&strategy_id), None) {
            if let Some(position) = cache.position(&position_id) {
                instruments.insert(position.instrument_id);
            }
        }

        let market_exit_tag = core.market_exit_tag;
        // Sort so the per-instrument cancel_all_orders/close_all_positions
        // cascade fires msgbus commands in a deterministic sequence; the
        // upstream dedup is AHash-backed.
        let mut instruments: Vec<_> = instruments.into_iter().collect();
        instruments.sort();
        drop(cache);

        for instrument_id in instruments {
            if let Err(e) = self.cancel_all_orders(instrument_id, None, None, true, None) {
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
                None,
            ) {
                log::error!("Error closing positions for {instrument_id}: {e}");
            }
        }

        let core = StrategyNative::strategy_core_mut(self);
        let interval_ms = core.config.market_exit_interval_ms;
        let timer_name = core.market_exit_timer_name;

        log::info!("{strategy_id} Setting market exit timer at {interval_ms}ms intervals");

        let interval_ns = interval_ms * 1_000_000;
        let result = core.clock_mut().set_timer_ns(
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
    fn check_market_exit(&mut self, _event: TimeEvent)
    where
        Self: StrategyNative + Component,
    {
        // Guard against stale timer events after cancel_market_exit
        if !self.is_exiting() {
            return;
        }

        let core = StrategyNative::strategy_core_mut(self);
        let Some(strategy_id) = core.strategy_id() else {
            log::error!("Cannot check market exit: strategy_id is not set");
            return;
        };

        core.market_exit_attempts += 1;
        let attempts = core.market_exit_attempts;
        let max_attempts = core.config.market_exit_max_attempts;

        log::debug!(
            "{strategy_id} Market exit check triggered (attempt {attempts}/{max_attempts})"
        );

        if attempts >= max_attempts {
            let cache = core.cache_ref();
            let open_orders_count =
                cache.orders_open_count(None, None, Some(&strategy_id), None, None);
            let inflight_orders_count =
                cache.orders_inflight_count(None, None, Some(&strategy_id), None, None);
            let open_positions_count =
                cache.positions_open_count(None, None, Some(&strategy_id), None, None);

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

        let cache = core.cache_ref();
        let has_open_orders = !cache
            .orders_open(None, None, Some(&strategy_id), None, None)
            .is_empty();
        let has_inflight_orders = !cache
            .orders_inflight(None, None, Some(&strategy_id), None, None)
            .is_empty();

        if has_open_orders || has_inflight_orders {
            return;
        }

        let positions_data: Vec<_> = cache
            .positions_open(None, None, Some(&strategy_id), None, None)
            .iter()
            .map(|p| (p.id, p.instrument_id, p.side, p.quantity, p.is_closed()))
            .collect();

        if !positions_data.is_empty() {
            // If there are open positions but no orders, re-send close orders
            drop(cache);

            for (pos_id, instrument_id, side, quantity, is_closed) in positions_data {
                if is_closed {
                    continue;
                }

                let core = StrategyNative::strategy_core_mut(self);
                let time_in_force = core.config.market_exit_time_in_force;
                let reduce_only = core.config.market_exit_reduce_only;
                let market_exit_tag = core.market_exit_tag;
                let Some(closing_side) = OrderCore::closing_side(side) else {
                    continue;
                };
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

                if let Err(e) = self.submit_order(order, Some(pos_id), None, None) {
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
    /// Cancels the market exit timer, resets state, calls the `post_market_exit` hook,
    /// and stops the strategy if a stop was pending.
    fn finalize_market_exit(&mut self)
    where
        Self: StrategyNative + Component,
    {
        let (actor_id, should_stop) = {
            let core = StrategyNative::strategy_core_mut(self);
            let actor_id = core.actor_id();
            let should_stop = core.pending_stop;
            (actor_id, should_stop)
        };

        self.cancel_market_exit();

        let hook_result = catch_unwind(AssertUnwindSafe(|| {
            self.post_market_exit();
        }));

        if let Err(e) = hook_result {
            log::error!("{actor_id} Error in post_market_exit: {e:?}");
        }

        if should_stop {
            log::info!("{actor_id} Market exit complete, stopping strategy");

            if let Err(e) = Component::stop(self) {
                log::error!("{actor_id} Failed to stop: {e}");
            }
        }

        let core = StrategyNative::strategy_core_mut(self);
        debug_assert!(
            !(core.pending_stop
                && !core.is_exiting
                && core.actor.state() == ComponentState::Running),
            "INVARIANT: stuck state after finalize_market_exit"
        );
    }

    /// Cancels an active market exit without calling hooks.
    ///
    /// Used when `stop()` is called during an active market exit to avoid state leaks.
    fn cancel_market_exit(&mut self)
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        let timer_name = core.market_exit_timer_name;

        if core
            .clock_mut()
            .timer_names()
            .contains(&timer_name.as_str())
        {
            core.clock_mut().cancel_timer(timer_name.as_str());
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
    fn stop(&mut self) -> bool
    where
        Self: StrategyNative,
    {
        let (manage_stop, is_exiting, should_initiate_exit) = {
            let core = StrategyNative::strategy_core_mut(self);
            let actor_id = core.actor_id();
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
                    log::info!("{actor_id} Initiating market exit before stop");
                }

                (manage_stop, is_exiting, should_initiate_exit)
            } else {
                (manage_stop, is_exiting, false)
            }
        };

        if manage_stop {
            if should_initiate_exit && let Err(e) = self.market_exit() {
                log::warn!("Market exit failed during stop: {e}, proceeding with stop");
                StrategyNative::strategy_core_mut(self).pending_stop = false;
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

    /// Denies an order by generating an `OrderDenied` event.
    ///
    /// This method creates an `OrderDenied` event, applies it to the order,
    /// and updates the cache.
    fn deny_order(&mut self, order: &OrderAny, reason: Ustr)
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        let Some(trader_id) = core.trader_id() else {
            log::error!(
                "Cannot deny order {}: trader_id is not set",
                order.client_order_id()
            );
            return;
        };
        let Some(strategy_id) = core.strategy_id() else {
            log::error!(
                "Cannot deny order {}: strategy_id is not set",
                order.client_order_id()
            );
            return;
        };
        let ts_now = core.clock_mut().timestamp_ns();

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

        let publish_initialized = {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            if cache.order_exists(&order.client_order_id()) {
                false
            } else {
                match cache.add_order(order.clone(), None, None, true) {
                    Ok(()) => true,
                    Err(e) => {
                        log::warn!("Failed to add denied order to cache: {e}");
                        false
                    }
                }
            }
        };

        if publish_initialized {
            publish_order_initialized(order);
        }

        let event = OrderEventAny::Denied(event);
        let applied = {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            if let Err(e) = cache.update_order(&event) {
                log::warn!("Failed to apply OrderDenied event: {e}");
                false
            } else {
                true
            }
        };

        if applied {
            let topic = format!("events.order.{strategy_id}");
            msgbus::publish_order_event(topic.into(), &event);
        }
    }

    /// Denies all orders in an order list.
    ///
    /// This method denies each non-closed order in the list.
    fn deny_order_list(&mut self, orders: &[OrderAny], reason: Ustr)
    where
        Self: StrategyNative,
    {
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
    fn set_gtd_expiry(&mut self, order: &OrderAny) -> anyhow::Result<()>
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);

        if !core.config.manage_gtd_expiry || order.time_in_force() != TimeInForce::Gtd {
            return Ok(());
        }

        let Some(expire_time) = order.expire_time() else {
            return Ok(());
        };

        let client_order_id = order.client_order_id();
        let timer_name = format!("GTD-EXPIRY:{client_order_id}");

        let current_time_ns = {
            let clock = core.clock_mut();
            clock.timestamp_ns()
        };

        if current_time_ns >= expire_time.as_u64() {
            log::info!("GTD order {client_order_id} already expired, canceling immediately");
            return self.cancel_order(order.client_order_id(), None, None);
        }

        {
            let mut clock = core.clock_mut();
            clock.set_time_alert_ns(&timer_name, expire_time, None, None)?;
        }

        core.gtd_timers
            .insert(client_order_id, Ustr::from(&timer_name));

        log::debug!("Set GTD expiry timer for {client_order_id} at {expire_time}");
        Ok(())
    }

    /// Cancels a GTD expiry timer for an order.
    fn cancel_gtd_expiry(&mut self, client_order_id: &ClientOrderId)
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);

        if let Some(timer_name) = core.gtd_timers.remove(client_order_id) {
            core.clock_mut().cancel_timer(timer_name.as_str());
            log::debug!("Canceled GTD expiry timer for {client_order_id}");
        }
    }

    /// Checks if a GTD expiry timer exists for an order.
    fn has_gtd_expiry_timer(&mut self, client_order_id: &ClientOrderId) -> bool
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        core.gtd_timers.contains_key(client_order_id)
    }

    /// Handles GTD order expiry by canceling the order.
    ///
    /// This method is called when a GTD expiry timer fires.
    fn expire_gtd_order(&mut self, event: TimeEvent)
    where
        Self: StrategyNative,
    {
        let timer_name = event.name;
        let Some(client_order_id) = timer_name
            .as_str()
            .strip_prefix("GTD-EXPIRY:")
            .and_then(|value| ClientOrderId::new_checked(value).ok())
        else {
            log::error!("Invalid GTD timer name format: {timer_name}");
            return;
        };

        let core = StrategyNative::strategy_core_mut(self);
        if core.gtd_timers.get(&client_order_id) != Some(&timer_name) {
            return;
        }
        core.gtd_timers.remove(&client_order_id);

        let order = core.cache_ref().order(&client_order_id).map(|o| o.clone());
        let Some(order) = order else {
            log::warn!("GTD order {client_order_id} not found in cache");
            return;
        };

        log::info!("GTD order {client_order_id} expired");

        if let Err(e) = self.cancel_order(order.client_order_id(), None, None) {
            log::error!("Failed to cancel expired GTD order {client_order_id}: {e}");
        }
    }

    /// Reactivates GTD timers for open orders on strategy start.
    ///
    /// Queries the cache for all open GTD orders and creates timers for those
    /// that haven't expired yet. Orders that have already expired are canceled immediately.
    fn reactivate_gtd_timers(&mut self)
    where
        Self: StrategyNative,
    {
        let core = StrategyNative::strategy_core_mut(self);
        let Some(strategy_id) = core.strategy_id() else {
            log::error!("Cannot reactivate GTD timers: strategy_id is not set");
            return;
        };
        let current_time_ns = core.clock_mut().timestamp_ns();

        let gtd_orders: Vec<OrderAny> = core
            .cache_ref()
            .orders_open(None, None, Some(&strategy_id), None, None)
            .into_iter()
            .filter(|o| o.time_in_force() == TimeInForce::Gtd)
            .map(|o| o.clone())
            .collect();

        for order in gtd_orders {
            let Some(expire_time) = order.expire_time() else {
                continue;
            };

            let expire_time_ns = expire_time.as_u64();
            let client_order_id = order.client_order_id();

            if current_time_ns >= expire_time_ns {
                log::info!("GTD order {client_order_id} already expired, canceling immediately");
                if let Err(e) = self.cancel_order(order.client_order_id(), None, None) {
                    log::error!("Failed to cancel expired GTD order {client_order_id}: {e}");
                }
            } else if let Err(e) = self.set_gtd_expiry(&order) {
                log::error!("Failed to set GTD expiry timer for {client_order_id}: {e}");
            }
        }
    }
}

/// Routes a time event through framework-managed strategy handlers.
///
/// Runtime hosts call this before the user-facing [`DataActor::on_time_event`] callback so custom
/// [`Strategy::on_time_event`] implementations cannot replace managed GTD expiry or market exit
/// routing.
pub fn route_time_event<T>(strategy: &mut T, event: &TimeEvent)
where
    T: Strategy + StrategyNative + Component + ?Sized,
{
    let (gtd_order_id, is_market_exit) = {
        let core = StrategyNative::strategy_core(strategy);
        let gtd_order_id = event
            .name
            .as_str()
            .strip_prefix("GTD-EXPIRY:")
            .and_then(|value| ClientOrderId::new_checked(value).ok())
            .filter(|client_order_id| core.gtd_timers.get(client_order_id) == Some(&event.name));
        let is_market_exit = event.name == core.market_exit_timer_name;
        (gtd_order_id, is_market_exit)
    };

    if gtd_order_id.is_none() && !is_market_exit {
        return;
    }

    let core = StrategyNative::strategy_core_mut(strategy);
    if core.managed_time_event_last_id == Some(event.event_id) {
        return;
    }
    core.managed_time_event_last_id = Some(event.event_id);

    if gtd_order_id.is_some() {
        strategy.expire_gtd_order(event.clone());
    } else {
        strategy.check_market_exit(event.clone());
    }
}

fn publish_order_initialized(order: &OrderAny) {
    let topic = format!("events.order.{}", order.strategy_id());
    let event = OrderEventAny::Initialized(order.init_event().clone());
    msgbus::publish_order_event(topic.into(), &event);
}

fn send_emulator_command(command: TradingCommand) {
    log_cmd_send(&command);
    let endpoint = MessagingSwitchboard::order_emulator_execute();
    msgbus::send_trading_command(endpoint, command);
}

fn send_algo_command(command: SubmitOrder, exec_algorithm_id: ExecAlgorithmId) {
    let id = command.strategy_id;
    log::info!("{id} {CMD}{SEND} {command}");

    let endpoint = format!("{exec_algorithm_id}.execute");
    msgbus::send_any(endpoint.into(), &TradingCommand::SubmitOrder(command));
}

fn send_risk_command(command: TradingCommand) {
    log_cmd_send(&command);
    let endpoint = MessagingSwitchboard::risk_engine_queue_execute();
    msgbus::send_trading_command(endpoint, command);
}

fn send_exec_command(command: TradingCommand) {
    log_cmd_send(&command);
    let endpoint = MessagingSwitchboard::exec_engine_queue_execute();
    msgbus::send_trading_command(endpoint, command);
}

fn log_cmd_send(command: &TradingCommand) {
    if let Some(id) = command.strategy_id() {
        log::info!("{id} {CMD}{SEND} {command}");
    } else {
        log::info!("{CMD}{SEND} {command}");
    }
}

fn registered_trader_id(core: &StrategyCore) -> anyhow::Result<TraderId> {
    core.trader_id()
        .ok_or_else(|| anyhow::anyhow!("Strategy not registered: trader_id is not set"))
}

fn registered_strategy_id(core: &StrategyCore) -> anyhow::Result<StrategyId> {
    core.strategy_id()
        .ok_or_else(|| anyhow::anyhow!("Strategy not registered: strategy_id is not set"))
}

fn required_account_id(order: &OrderAny, operation: &str) -> anyhow::Result<AccountId> {
    order.account_id().ok_or_else(|| {
        anyhow::anyhow!(
            "Cannot generate {operation} event for {}: account_id is not set",
            order.client_order_id()
        )
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        actor::{
            DataActor,
            registry::{deregister_actor, try_get_actor_unchecked},
        },
        cache::{Cache, ORDER_NOT_FOUND},
        clock::{Clock, TestClock},
        component::{Component, deregister_component, register_component_actor},
        enums::ComponentState,
        msgbus::{
            self, MessagingSwitchboard, TypedHandler, TypedIntoHandler,
            stubs::{
                TypedIntoMessageSavingHandler, TypedMessageSavingHandler, get_any_saving_handler,
                get_typed_into_message_saving_handler, get_typed_message_saving_handler,
            },
        },
        timer::{TimeEvent, TimeEventCallback},
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{
            ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType,
            PositionAdjustmentType, PositionSide, TriggerType,
        },
        events::{
            OrderAccepted, OrderCanceled, OrderFilled, OrderRejected, PositionAdjusted,
            order::spec::{
                OrderAcceptedSpec, OrderCanceledSpec, OrderEmulatedSpec, OrderExpiredSpec,
                OrderFillVoidedSpec, OrderFilledSpec, OrderRejectedSpec,
            },
        },
        identifiers::{
            AccountId, ActorId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
            TradeId, TraderId, VenueOrderId,
        },
        orderbook::own::OwnOrderBook,
        orders::{LimitOrder, MarketOrder, OrderTestBuilder, stubs::TestOrderEventStubs},
        stubs::TestDefault,
        types::{Currency, Money, Price},
    };
    use nautilus_portfolio::portfolio::Portfolio;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::nautilus_strategy;

    #[derive(Debug)]
    struct TestStrategy {
        core: StrategyCore,
        on_order_rejected_called: bool,
        on_order_event_called: bool,
        on_order_accepted_called: bool,
        on_order_canceled_called: bool,
        on_order_filled_called: bool,
        on_order_fill_voided_called: bool,
        on_order_expired_called: bool,
        order_event_timeline: Rc<RefCell<Vec<&'static str>>>,
        on_position_event_called: bool,
        on_position_opened_called: bool,
        on_position_changed_called: bool,
        on_position_closed_called: bool,
    }

    #[derive(Debug)]
    struct CoreFreeStrategy {
        started: bool,
    }

    #[derive(Debug)]
    struct InitializedModifyStrategy {
        core: StrategyCore,
        modified_quantity: Quantity,
    }

    #[derive(Debug)]
    struct TimerOverrideStrategy {
        core: StrategyCore,
        gtd_expiries: usize,
        market_exit_checks: usize,
    }

    impl DataActor for CoreFreeStrategy {
        fn on_start(&mut self) -> anyhow::Result<()> {
            self.started = true;
            Ok(())
        }
    }

    impl Strategy for CoreFreeStrategy {}

    impl DataActor for InitializedModifyStrategy {}

    nautilus_strategy!(InitializedModifyStrategy, {
        fn on_order_initialized(&mut self, event: OrderInitialized) {
            self.modify_order(
                event.client_order_id,
                Some(self.modified_quantity),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        }
    });

    impl DataActor for TimerOverrideStrategy {
        fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
            Strategy::on_time_event(self, event)
        }
    }

    nautilus_strategy!(TimerOverrideStrategy, {
        fn check_market_exit(&mut self, _event: TimeEvent) {
            self.market_exit_checks += 1;
        }

        fn expire_gtd_order(&mut self, _event: TimeEvent) {
            self.gtd_expiries += 1;
        }
    });

    impl TestStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
                on_order_rejected_called: false,
                on_order_event_called: false,
                on_order_accepted_called: false,
                on_order_canceled_called: false,
                on_order_filled_called: false,
                on_order_fill_voided_called: false,
                on_order_expired_called: false,
                order_event_timeline: Rc::new(RefCell::new(Vec::new())),
                on_position_event_called: false,
                on_position_opened_called: false,
                on_position_changed_called: false,
                on_position_closed_called: false,
            }
        }
    }

    impl DataActor for TestStrategy {}

    nautilus_strategy!(TestStrategy, {
        fn on_order_canceled(&mut self, _event: &OrderCanceled) {
            self.on_order_canceled_called = true;
        }

        fn on_order_filled(&mut self, _event: &OrderFilled) {
            self.on_order_filled_called = true;
            self.order_event_timeline.borrow_mut().push("specific");
        }

        fn on_order_fill_voided(&mut self, _event: &OrderFillVoided) {
            self.on_order_fill_voided_called = true;
        }

        fn on_order_rejected(&mut self, _event: OrderRejected) {
            self.on_order_rejected_called = true;
        }

        fn on_order_event(&mut self, _event: OrderEventAny) {
            self.on_order_event_called = true;
            self.order_event_timeline.borrow_mut().push("aggregate");
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

        fn on_position_event(&mut self, _event: PositionEvent) {
            self.on_position_event_called = true;
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
            clock.clone(),
            cache.clone(),
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
        OrderEventAny::Filled(
            OrderFilledSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("TEST-001"))
                .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
                .client_order_id(client_order_id)
                .venue_order_id(VenueOrderId::test_default())
                .account_id(AccountId::from("ACC-001"))
                .trade_id(TradeId::test_default())
                .last_qty(Quantity::default())
                .last_px(Price::default())
                .currency(Currency::from("USD"))
                .liquidity_side(LiquiditySide::Taker)
                .event_id(UUID4::default())
                .build(),
        )
    }

    fn make_fill_voided(client_order_id: ClientOrderId, is_reopened: bool) -> OrderEventAny {
        OrderEventAny::FillVoided(
            OrderFillVoidedSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("TEST-001"))
                .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
                .client_order_id(client_order_id)
                .venue_order_id(VenueOrderId::test_default())
                .account_id(AccountId::from("ACC-001"))
                .is_reopened(is_reopened)
                .build(),
        )
    }

    fn make_terminal_fill_voided(client_order_id: ClientOrderId) -> OrderEventAny {
        make_fill_voided(client_order_id, false)
    }

    fn make_canceled(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Canceled(
            OrderCanceledSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("TEST-001"))
                .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
                .client_order_id(client_order_id)
                .account_id(AccountId::from("ACC-001"))
                .event_id(UUID4::default())
                .build(),
        )
    }

    fn make_rejected(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Rejected(
            OrderRejectedSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("TEST-001"))
                .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
                .client_order_id(client_order_id)
                .account_id(AccountId::from("ACC-001"))
                .reason("Test rejection".into())
                .event_id(UUID4::default())
                .build(),
        )
    }

    fn make_expired(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Expired(
            OrderExpiredSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("TEST-001"))
                .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
                .client_order_id(client_order_id)
                .account_id(AccountId::from("ACC-001"))
                .event_id(UUID4::default())
                .build(),
        )
    }

    fn make_accepted(client_order_id: ClientOrderId) -> OrderEventAny {
        OrderEventAny::Accepted(
            OrderAcceptedSpec::builder()
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("TEST-001"))
                .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
                .client_order_id(client_order_id)
                .venue_order_id(VenueOrderId::test_default())
                .account_id(AccountId::from("ACC-001"))
                .event_id(UUID4::default())
                .build(),
        )
    }

    fn make_accepted_market_order(client_order_id: &str) -> OrderAny {
        let mut order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from(client_order_id),
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
        let account_id = AccountId::from("ACC-001");
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();
        order
            .apply(TestOrderEventStubs::accepted(
                &order,
                account_id,
                // Derived per order: a venue order ID has a single owning client order, so
                // batch tests holding two accepted orders cannot share the default.
                VenueOrderId::from(client_order_id),
            ))
            .unwrap();
        order
    }

    fn make_accepted_limit_order(client_order_id: &str) -> OrderAny {
        let mut order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from(client_order_id),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("50000.0"),
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
            UUID4::new(),
            UnixNanos::default(),
        ));
        let account_id = AccountId::from("ACC-001");
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();
        order
            .apply(TestOrderEventStubs::accepted(
                &order,
                account_id,
                // Derived per order, as above.
                VenueOrderId::from(client_order_id),
            ))
            .unwrap();
        order
    }

    fn make_initialized_market_order(client_order_id: &str) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from(client_order_id),
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
        ))
    }

    fn make_initialized_algorithm_order(client_order_id: &str) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            ClientOrderId::from(client_order_id),
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
            Some(ExecAlgorithmId::from("TWAP")),
            None,
            Some(ClientOrderId::from(client_order_id)),
            None,
        ))
    }

    fn add_order_to_cache(strategy: &TestStrategy, order: &OrderAny) {
        let cache_rc = strategy.core.cache_rc();
        let mut cache = cache_rc.borrow_mut();
        cache.add_order(order.clone(), None, None, true).unwrap();
    }

    fn make_submit_command(order: &OrderAny) -> SubmitOrder {
        SubmitOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None, // correlation_id
        )
    }

    fn add_order_to_cache_and_own_book(strategy: &TestStrategy, order: &OrderAny) {
        let cache_rc = strategy.core.cache_rc();
        let mut cache = cache_rc.borrow_mut();
        cache.add_order(order.clone(), None, None, true).unwrap();
        cache
            .add_own_order_book(OwnOrderBook::new(order.instrument_id()))
            .unwrap();
        cache.update_own_order_book(order);
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
            realized_pnl: None,
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
            unrealized_pnl: Money::zero(currency),
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
            unrealized_pnl: Money::zero(currency),
            duration: 0,
            event_id: UUID4::default(),
            ts_opened: UnixNanos::default(),
            ts_closed: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        })
    }

    fn make_position_adjusted() -> PositionEvent {
        PositionEvent::PositionAdjusted(PositionAdjusted {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: PositionId::test_default(),
            account_id: AccountId::from("ACC-001"),
            adjustment_type: PositionAdjustmentType::Funding,
            quantity_change: None,
            pnl_change: None,
            reason: None,
            event_id: UUID4::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        })
    }

    #[rstest]
    fn test_strategy_creation() {
        let strategy = create_test_strategy();
        assert_eq!(strategy.strategy_id(), Some(StrategyId::from("TEST-001")));
        assert!(!strategy.on_order_rejected_called);
        assert!(!strategy.on_position_opened_called);
    }

    #[rstest]
    fn test_strategy_registration() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        assert!(strategy.is_registered());
        let _ = strategy.order().generate_client_order_id();
        let _ = strategy.portfolio().is_initialized();
    }

    #[rstest]
    fn test_set_external_order_instrument_ids_replaces_claims_atomically() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        let cache = strategy.core.cache_rc();
        let strategy_id = StrategyId::from("TEST-001");
        let other_strategy_id = StrategyId::from("OTHER-001");
        let audusd = InstrumentId::from("AUDUSD.SIM");
        let eurusd = InstrumentId::from("EURUSD.SIM");
        let gbpusd = InstrumentId::from("GBPUSD.SIM");

        strategy
            .set_external_order_instrument_ids(vec![audusd, eurusd])
            .unwrap();
        cache
            .borrow_mut()
            .set_external_order_claims(other_strategy_id, &[gbpusd])
            .unwrap();

        let result = strategy.set_external_order_instrument_ids(vec![eurusd, gbpusd]);

        assert_eq!(
            result.unwrap_err().to_string(),
            "External order claim for GBPUSD.SIM already exists for OTHER-001"
        );
        assert_eq!(
            cache.borrow().external_order_claim(&audusd),
            Some(strategy_id)
        );
        assert_eq!(
            cache.borrow().external_order_claim(&eurusd),
            Some(strategy_id)
        );
        assert_eq!(
            cache.borrow().external_order_claim(&gbpusd),
            Some(other_strategy_id)
        );
        assert_eq!(
            strategy.core.config.external_order_instrument_ids,
            Some(vec![audusd, eurusd])
        );

        strategy
            .set_external_order_instrument_ids(vec![eurusd])
            .unwrap();

        assert_eq!(cache.borrow().external_order_claim(&audusd), None);
        assert_eq!(
            cache.borrow().external_order_claim(&eurusd),
            Some(strategy_id)
        );
        assert_eq!(
            cache.borrow().external_order_claim(&gbpusd),
            Some(other_strategy_id)
        );
        assert_eq!(
            strategy.core.config.external_order_instrument_ids,
            Some(vec![eurusd])
        );
    }

    #[rstest]
    fn test_set_external_order_instrument_ids_rejects_unregistered_strategy() {
        let mut strategy = create_test_strategy();

        let error = strategy
            .set_external_order_instrument_ids(vec![InstrumentId::from("AUDUSD.SIM")])
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Strategy TEST-001 is not registered with a trader"
        );
        assert!(strategy.core.config.external_order_instrument_ids.is_none());
    }

    #[rstest]
    fn test_strategy_native_methods_are_available_on_strategy_type() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        drop(strategy.order_factory());

        assert!(Rc::ptr_eq(
            &strategy.order_factory_rc(),
            strategy.core.order_factory.as_ref().unwrap()
        ));
        assert!(Rc::ptr_eq(
            &strategy.portfolio_rc(),
            strategy.core.portfolio.as_ref().unwrap()
        ));
    }

    #[rstest]
    fn test_handle_order_event_dispatches_to_handler() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let event = make_rejected(ClientOrderId::from("O-001"));

        strategy.handle_order_event(event);

        assert!(strategy.on_order_rejected_called);
        assert!(strategy.on_order_event_called);
    }

    #[rstest]
    fn test_dispatch_manager_actions_routes_every_action() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        let strategy_id = StrategyId::from("TEST-001");
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let initialized_order = make_initialized_market_order("O-MANAGER-INIT");
        let emulator_order = OrderTestBuilder::new(OrderType::StopMarket)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-MANAGER-EMULATOR"))
            .side(OrderSide::Buy)
            .trigger_price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .build();
        let risk_order = make_initialized_market_order("O-MANAGER-RISK");
        let algorithm_order = make_initialized_algorithm_order("O-MANAGER-ALGORITHM");
        let cancel_order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-MANAGER-CANCEL"))
            .side(OrderSide::Buy)
            .price(Price::from("50000.0"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        let missing_cancel_order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-MANAGER-MISSING-CANCEL"))
            .side(OrderSide::Buy)
            .price(Price::from("50000.0"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        let modify_order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-MANAGER-MODIFY"))
            .side(OrderSide::Buy)
            .price(Price::from("50000.0"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        add_order_to_cache(&strategy, &cancel_order);
        add_order_to_cache(&strategy, &modify_order);

        let (order_handler, order_events) =
            get_typed_message_saving_handler(Some(Ustr::from("manager-action-order-events")));
        let order_topic = format!("events.order.{strategy_id}");
        msgbus::subscribe_order_events(order_topic.clone().into(), order_handler.clone(), None);
        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );
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
        let (algorithm_handler, algorithm_messages) =
            get_any_saving_handler::<TradingCommand>(Some(Ustr::from("TWAP.execute")));
        msgbus::register_any("TWAP.execute".into(), algorithm_handler);

        strategy.dispatch_manager_actions(vec![
            OrderManagerAction::CancelLocal(missing_cancel_order),
            OrderManagerAction::PublishInitialized(OrderEventAny::Initialized(
                initialized_order.init_event().clone(),
            )),
            OrderManagerAction::SubmitToEmulator(make_submit_command(&emulator_order)),
            OrderManagerAction::SubmitToRisk(make_submit_command(&risk_order)),
            OrderManagerAction::SubmitToAlgorithm {
                command: make_submit_command(&algorithm_order),
                exec_algorithm_id: ExecAlgorithmId::from("TWAP"),
            },
            OrderManagerAction::CancelLocal(cancel_order.clone()),
            OrderManagerAction::ModifyLocalQuantity {
                order: modify_order.clone(),
                quantity: Quantity::from(50_000),
            },
            OrderManagerAction::ModifyLocalQuantity {
                order: modify_order.clone(),
                quantity: Quantity::from(100_000),
            },
        ]);
        msgbus::unsubscribe_order_events(order_topic.into(), &order_handler);

        let order_events = order_events.get_messages();
        assert_eq!(order_events.len(), 3);
        assert!(matches!(
            &order_events[0],
            OrderEventAny::Initialized(event)
                if event.client_order_id == initialized_order.client_order_id()
        ));
        assert!(matches!(
            &order_events[1],
            OrderEventAny::PendingCancel(event)
                if event.client_order_id == cancel_order.client_order_id()
        ));
        assert!(matches!(
            &order_events[2],
            OrderEventAny::PendingUpdate(event)
                if event.client_order_id == modify_order.client_order_id()
        ));
        assert!(matches!(
            emulator_messages.get_messages().as_slice(),
            [TradingCommand::SubmitOrder(command)]
                if command.client_order_id == emulator_order.client_order_id()
        ));
        assert!(matches!(
            risk_messages.get_messages().as_slice(),
            [
                TradingCommand::SubmitOrder(submit),
                TradingCommand::ModifyOrder(first_modify),
                TradingCommand::ModifyOrder(second_modify),
            ]

                if submit.client_order_id == risk_order.client_order_id()
                    && first_modify.client_order_id == modify_order.client_order_id()
                    && first_modify.quantity == Some(Quantity::from(50_000))
                    && second_modify.client_order_id == modify_order.client_order_id()
                    && second_modify.quantity == Some(Quantity::from(100_000))
        ));
        assert!(matches!(
            algorithm_messages.get_messages().as_slice(),
            [TradingCommand::SubmitOrder(command)]
                if command.client_order_id == algorithm_order.client_order_id()
        ));
        assert!(matches!(
            exec_messages.get_messages().as_slice(),
            [TradingCommand::CancelOrder(command)]
                if command.client_order_id == cancel_order.client_order_id()
        ));
    }

    #[rstest]
    #[case::disabled(false)]
    #[case::enabled(true)]
    fn test_contingent_manager_dispatch_precedes_user_handlers_and_is_idempotent(
        #[case] manage_contingent_orders: bool,
    ) {
        let mut strategy = TestStrategy::new(StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            manage_contingent_orders,
            ..Default::default()
        });
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        let timeline = strategy.order_event_timeline.clone();
        let endpoint_timeline = timeline.clone();
        let exec_handler = TypedIntoHandler::from(move |command: TradingCommand| {
            assert!(matches!(command, TradingCommand::CancelOrder(_)));
            endpoint_timeline.borrow_mut().push("endpoint");
        });
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let parent_id = ClientOrderId::from("O-CONTINGENT-PARENT");
        let sibling_id = ClientOrderId::from("O-CONTINGENT-SIBLING");
        let parent = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("TEST-001"))
            .instrument_id(instrument_id)
            .client_order_id(parent_id)
            .side(OrderSide::Buy)
            .price(Price::from("50000.0"))
            .quantity(Quantity::from(100_000))
            .contingency_type(ContingencyType::Oco)
            .linked_order_ids(vec![parent_id, sibling_id])
            .submit(true)
            .build();
        let sibling = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("TEST-001"))
            .instrument_id(instrument_id)
            .client_order_id(sibling_id)
            .side(OrderSide::Sell)
            .price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .submit(true)
            .build();
        add_order_to_cache(&strategy, &parent);
        add_order_to_cache(&strategy, &sibling);
        let event = OrderEventAny::Filled(
            OrderFilledSpec::builder()
                .trader_id(parent.trader_id())
                .strategy_id(parent.strategy_id())
                .instrument_id(parent.instrument_id())
                .client_order_id(parent.client_order_id())
                .venue_order_id(VenueOrderId::from("V-CONTINGENT-PARENT"))
                .account_id(AccountId::from("ACCOUNT-001"))
                .trade_id(TradeId::from("T-CONTINGENT-PARENT"))
                .order_side(parent.order_side())
                .order_type(parent.order_type())
                .last_qty(parent.quantity())
                .last_px(Price::from("50000.0"))
                .liquidity_side(LiquiditySide::Taker)
                .build(),
        );
        strategy
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&event)
            .unwrap();

        strategy.handle_order_event(event.clone());
        strategy.handle_order_event(event);

        let timeline = timeline.borrow();
        let sibling_status = strategy
            .core
            .cache_ref()
            .order(&sibling_id)
            .unwrap()
            .status();

        if manage_contingent_orders {
            assert_eq!(
                timeline.as_slice(),
                ["endpoint", "specific", "aggregate", "specific", "aggregate"]
            );
            assert_eq!(sibling_status, OrderStatus::PendingCancel);
        } else {
            assert_eq!(
                timeline.as_slice(),
                ["specific", "aggregate", "specific", "aggregate"]
            );
            assert_eq!(sibling_status, OrderStatus::Submitted);
        }
    }

    #[rstest]
    fn test_handle_order_fill_voided_dispatches_to_specific_handler() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        strategy.handle_order_event(make_fill_voided(ClientOrderId::from("O-001"), false));

        assert!(strategy.on_order_fill_voided_called);
        assert!(strategy.on_order_event_called);
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
        assert!(strategy.on_position_event_called);
    }

    #[rstest]
    fn test_handle_position_event_skips_dispatch_when_stopped() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        stop_strategy(&mut strategy);
        assert_eq!(strategy.state(), ComponentState::Stopped);

        strategy.handle_position_event(make_position_opened());

        assert!(!strategy.on_position_event_called);
        assert!(!strategy.on_position_opened_called);
    }

    #[rstest]
    fn test_handle_position_event_skips_dispatch_for_adjusted() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        strategy.handle_position_event(make_position_adjusted());

        assert!(!strategy.on_position_event_called);
        assert!(!strategy.on_position_opened_called);
        assert!(!strategy.on_position_changed_called);
        assert!(!strategy.on_position_closed_called);
    }

    #[rstest]
    fn test_strategy_default_handlers_do_not_panic() {
        let mut strategy = create_test_strategy();

        strategy.on_order_initialized(OrderInitialized::default());
        strategy.on_order_event(OrderEventAny::Accepted(OrderAccepted::default()));
        strategy.on_order_denied(OrderDenied::default());
        strategy.on_order_emulated(OrderEmulated::default());
        strategy.on_order_released(OrderReleased::default());
        strategy.on_order_submitted(OrderSubmitted::default());
        strategy.on_order_rejected(OrderRejected::default());
        strategy.on_order_canceled(&OrderCanceled::default());
        strategy.on_order_expired(OrderExpired::default());
        strategy.on_order_triggered(OrderTriggered::default());
        strategy.on_order_pending_update(OrderPendingUpdate::default());
        strategy.on_order_pending_cancel(OrderPendingCancel::default());
        strategy.on_order_modify_rejected(OrderModifyRejected::default());
        strategy.on_order_cancel_rejected(OrderCancelRejected::default());
        strategy.on_order_updated(OrderUpdated::default());
        strategy.on_order_filled(&OrderFilledSpec::builder().build());
        strategy.on_order_fill_voided(&OrderFillVoidedSpec::builder().build());
        strategy.on_position_event(make_position_opened());
    }

    #[rstest]
    fn test_submit_order_publishes_order_initialized_after_cache_insert_before_send() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order = make_initialized_market_order("O-20250208-INIT-001");
        let client_order_id = order.client_order_id();
        let cache_rc = strategy.core.cache_rc();
        let timeline = Rc::new(RefCell::new(Vec::new()));
        let event_messages = Rc::new(RefCell::new(Vec::new()));

        let event_handler = {
            let event_messages = event_messages.clone();
            let timeline = timeline.clone();
            TypedHandler::from_with_id("events.order.initialized", move |event: &OrderEventAny| {
                assert!(cache_rc.borrow().order_exists(&client_order_id));
                assert!(matches!(event, OrderEventAny::Initialized(_)));
                event_messages.borrow_mut().push(event.clone());
                timeline.borrow_mut().push("init");
            })
        };
        let risk_handler = {
            let timeline = timeline.clone();
            TypedIntoHandler::from_with_id(
                "RiskEngine.queue_execute",
                move |command: TradingCommand| {
                    assert!(matches!(command, TradingCommand::SubmitOrder(_)));
                    timeline.borrow_mut().push("command");
                },
            )
        };
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let topic = format!("events.order.{}", order.strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);

        strategy
            .submit_order(order.clone(), None, None, None)
            .unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        let event_messages = event_messages.borrow();
        assert_eq!(event_messages.len(), 1);
        assert_eq!(
            event_messages[0],
            OrderEventAny::Initialized(order.init_event().clone())
        );
        assert_eq!(timeline.borrow().as_slice(), &["init", "command"]);
    }

    #[rstest]
    fn test_submit_order_routes_emulated_order_to_order_emulator() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-20250208-EMULATED-001"))
            .side(OrderSide::Buy)
            .trigger_price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .build();
        let client_order_id = order.client_order_id();

        strategy.submit_order(order, None, None, None).unwrap();

        let emulator_messages = emulator_messages.get_messages();
        assert_eq!(emulator_messages.len(), 1);
        assert!(matches!(
            emulator_messages.first(),
            Some(TradingCommand::SubmitOrder(command))
                if command.client_order_id == client_order_id
        ));
        assert!(risk_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_submit_order_errors_when_strategy_not_registered() {
        let mut strategy = create_test_strategy();
        let order = make_initialized_market_order("O-20250208-UNREGISTERED-001");

        let err = strategy
            .submit_order(order, None, None, None)
            .unwrap_err()
            .to_string();

        assert_eq!(err, "Strategy not registered: trader_id is not set");
    }

    #[rstest]
    fn test_submit_order_uses_stored_strategy_id_when_actor_id_diverges() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        // A hyphenless actor ID has no strategy ID form, so any re-parse of it panics
        strategy.core.actor.actor_id = ActorId::from("Strategy");
        let order = make_initialized_market_order("O-20250208-DIVERGED-001");

        strategy.submit_order(order, None, None, None).unwrap();

        let risk_messages = risk_messages.get_messages();
        assert_eq!(risk_messages.len(), 1);
        let Some(TradingCommand::SubmitOrder(command)) = risk_messages.first() else {
            panic!("Expected a SubmitOrder command, was {risk_messages:?}");
        };
        assert_eq!(command.strategy_id, StrategyId::from("TEST-001"));
        assert_eq!(
            command.client_order_id,
            ClientOrderId::from("O-20250208-DIVERGED-001")
        );
    }

    #[rstest]
    fn test_required_account_id_errors_when_missing_for_strategy_event() {
        let order = make_initialized_market_order("O-20250208-NO-ACCOUNT-001");

        let err = required_account_id(&order, "pending cancel")
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "Cannot generate pending cancel event for O-20250208-NO-ACCOUNT-001: \
             account_id is not set"
        );
    }

    #[rstest]
    fn test_submit_order_rejects_non_initialized_without_events() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order = make_accepted_market_order("O-20250208-ACCEPTED-001");
        let topic = format!("events.order.{}", order.strategy_id());
        let (event_handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
            get_typed_message_saving_handler(Some(Ustr::from("events.order.invalid")));

        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        let result = strategy.submit_order(order, None, None, None);

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected INITIALIZED")
        );
        assert!(event_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_submit_order_returns_error_when_cache_already_borrowed() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order = make_initialized_market_order("O-20250208-BORROWED-001");
        let cache_rc = strategy.core.cache_rc();
        let _cache = cache_rc.borrow();

        let result = catch_unwind(AssertUnwindSafe(|| {
            strategy.submit_order(order, None, None, None)
        }));

        let err = result
            .expect("submit_order should not panic")
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "Cannot submit order O-20250208-BORROWED-001: cache is currently borrowed"
        );
    }

    #[rstest]
    fn test_submit_order_list_publishes_order_initialized_after_cache_insert_before_send() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order_list_id = OrderListId::from("OL-20250208-LIST-INIT");
        let mut orders = vec![
            make_initialized_market_order("O-20250208-LIST-INIT-001"),
            make_initialized_market_order("O-20250208-LIST-INIT-002"),
        ];

        for order in &mut orders {
            order.set_order_list_id(order_list_id);
        }

        let client_order_id1 = orders[0].client_order_id();
        let client_order_id2 = orders[1].client_order_id();
        let cache_rc = strategy.core.cache_rc();
        let timeline = Rc::new(RefCell::new(Vec::new()));
        let event_messages = Rc::new(RefCell::new(Vec::new()));

        let event_handler = {
            let event_messages = event_messages.clone();
            let timeline = timeline.clone();
            TypedHandler::from_with_id(
                "events.order.list_initialized",
                move |event: &OrderEventAny| {
                    match event {
                        OrderEventAny::Initialized(e) if e.client_order_id == client_order_id1 => {
                            let cache = cache_rc.borrow();
                            assert!(cache.order_exists(&client_order_id1));
                            assert!(cache.order_exists(&client_order_id2));
                            assert!(cache.order_list_exists(&order_list_id));
                            let order_list = cache.order_list(&order_list_id).unwrap();
                            assert_eq!(
                                order_list.client_order_ids.as_slice(),
                                &[client_order_id1, client_order_id2]
                            );
                            timeline.borrow_mut().push("init1");
                        }
                        OrderEventAny::Initialized(e) if e.client_order_id == client_order_id2 => {
                            assert!(cache_rc.borrow().order_exists(&client_order_id2));
                            timeline.borrow_mut().push("init2");
                        }
                        _ => panic!("unexpected order event {event:?}"),
                    }
                    event_messages.borrow_mut().push(event.clone());
                },
            )
        };
        let risk_handler = {
            let timeline = timeline.clone();
            TypedIntoHandler::from_with_id(
                "RiskEngine.queue_execute",
                move |command: TradingCommand| {
                    assert!(matches!(command, TradingCommand::SubmitOrderList(_)));
                    timeline.borrow_mut().push("command");
                },
            )
        };
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let topic = format!("events.order.{}", orders[0].strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);

        strategy
            .submit_order_list(orders.clone(), None, None, None)
            .unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        let event_messages = event_messages.borrow();
        assert_eq!(event_messages.len(), 2);
        assert_eq!(
            event_messages[0],
            OrderEventAny::Initialized(orders[0].init_event().clone())
        );
        assert_eq!(
            event_messages[1],
            OrderEventAny::Initialized(orders[1].init_event().clone())
        );
        assert_eq!(timeline.borrow().as_slice(), &["init1", "init2", "command"]);
    }

    #[rstest]
    fn test_submit_order_list_returns_error_when_cache_already_borrowed() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order_list_id = OrderListId::from("OL-20250208-BORROWED");
        let mut orders = vec![
            make_initialized_market_order("O-20250208-LIST-BORROWED-001"),
            make_initialized_market_order("O-20250208-LIST-BORROWED-002"),
        ];

        for order in &mut orders {
            order.set_order_list_id(order_list_id);
        }

        let cache_rc = strategy.core.cache_rc();
        let _cache = cache_rc.borrow();

        let result = catch_unwind(AssertUnwindSafe(|| {
            strategy.submit_order_list(orders, None, None, None)
        }));

        let err = result
            .expect("submit_order_list should not panic")
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "Cannot submit order list OL-20250208-BORROWED: cache is currently borrowed"
        );
    }

    #[rstest]
    fn test_submit_order_list_create_list_branch_publishes_init_after_cache_insert() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let orders = vec![
            make_initialized_market_order("O-20250208-LIST-CREATE-001"),
            make_initialized_market_order("O-20250208-LIST-CREATE-002"),
        ];

        let client_order_id1 = orders[0].client_order_id();
        let client_order_id2 = orders[1].client_order_id();
        let cache_rc = strategy.core.cache_rc();
        let timeline = Rc::new(RefCell::new(Vec::new()));
        let event_messages = Rc::new(RefCell::new(Vec::new()));

        let event_handler = {
            let event_messages = event_messages.clone();
            let timeline = timeline.clone();
            TypedHandler::from_with_id(
                "events.order.list_create_initialized",
                move |event: &OrderEventAny| {
                    match event {
                        OrderEventAny::Initialized(e) if e.client_order_id == client_order_id1 => {
                            let cache = cache_rc.borrow();
                            let cached_order1 = cache.order(&client_order_id1).unwrap();
                            let cached_order2 = cache.order(&client_order_id2).unwrap();
                            let order_list_id = cached_order1.order_list_id().unwrap();
                            assert_eq!(cached_order2.order_list_id(), Some(order_list_id));
                            assert_eq!(e.order_list_id, Some(order_list_id));
                            assert!(cache.order_list_exists(&order_list_id));
                            let order_list = cache.order_list(&order_list_id).unwrap();
                            assert_eq!(
                                order_list.client_order_ids.as_slice(),
                                &[client_order_id1, client_order_id2]
                            );
                            timeline.borrow_mut().push("init1");
                        }
                        OrderEventAny::Initialized(e) if e.client_order_id == client_order_id2 => {
                            let cache = cache_rc.borrow();
                            let cached_order = cache.order(&client_order_id2).unwrap();
                            assert_eq!(e.order_list_id, cached_order.order_list_id());
                            timeline.borrow_mut().push("init2");
                        }
                        _ => panic!("unexpected order event {event:?}"),
                    }
                    event_messages.borrow_mut().push(event.clone());
                },
            )
        };
        let risk_handler = {
            let timeline = timeline.clone();
            TypedIntoHandler::from_with_id(
                "RiskEngine.queue_execute",
                move |command: TradingCommand| {
                    let TradingCommand::SubmitOrderList(command) = command else {
                        panic!("expected SubmitOrderList command");
                    };
                    assert!(
                        command
                            .order_inits
                            .iter()
                            .all(|init| init.order_list_id == Some(command.order_list.id))
                    );
                    timeline.borrow_mut().push("command");
                },
            )
        };
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let topic = format!("events.order.{}", orders[0].strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);

        strategy
            .submit_order_list(orders, None, None, None)
            .unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        let cache = strategy.cache();
        let cached_order1 = cache.order(&client_order_id1).unwrap();
        let cached_order2 = cache.order(&client_order_id2).unwrap();
        let order_list_id = cached_order1.order_list_id().unwrap();
        assert_eq!(cached_order2.order_list_id(), Some(order_list_id));

        let event_messages = event_messages.borrow();
        assert_eq!(event_messages.len(), 2);
        let OrderEventAny::Initialized(init1) = &event_messages[0] else {
            panic!("expected first OrderInitialized event");
        };
        let OrderEventAny::Initialized(init2) = &event_messages[1] else {
            panic!("expected second OrderInitialized event");
        };
        assert_eq!(init1.order_list_id, Some(order_list_id));
        assert_eq!(init2.order_list_id, Some(order_list_id));
        assert_eq!(timeline.borrow().as_slice(), &["init1", "init2", "command"]);

        let order_list = cache.order_list(&order_list_id).unwrap();
        assert_eq!(
            order_list.client_order_ids.as_slice(),
            &[client_order_id1, client_order_id2]
        );
    }

    #[rstest]
    fn test_submit_order_list_routes_optional_params_to_risk() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let no_params_orders = vec![
            make_initialized_market_order("O-20250208-LIST-001"),
            make_initialized_market_order("O-20250208-LIST-002"),
        ];
        strategy
            .submit_order_list(no_params_orders, None, None, None)
            .unwrap();

        let mut params = Params::new();
        params.insert(
            "routing_hint".to_string(),
            Value::String("prefer_batch".to_string()),
        );
        let param_orders = vec![
            make_initialized_market_order("O-20250208-LIST-003"),
            make_initialized_market_order("O-20250208-LIST-004"),
        ];
        strategy
            .submit_order_list(param_orders, None, None, Some(params.clone()))
            .unwrap();

        let risk_messages = risk_messages.get_messages();
        assert_eq!(risk_messages.len(), 2);
        let Some(TradingCommand::SubmitOrderList(no_params_command)) = risk_messages.first() else {
            panic!("expected SubmitOrderList command");
        };
        let Some(TradingCommand::SubmitOrderList(param_command)) = risk_messages.get(1) else {
            panic!("expected SubmitOrderList command");
        };
        assert!(no_params_command.params.is_none());
        assert_eq!(param_command.params.as_ref(), Some(&params));
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
        add_order_to_cache(&strategy, &order);

        strategy
            .modify_order(
                order.client_order_id(),
                Some(Quantity::from(200_000)),
                None,
                None,
                None,
                None,
            )
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

    #[rstest]
    fn test_modify_order_routes_active_local_algorithm_order_to_algorithm() {
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
        let (algo_handler, algo_messages) =
            get_any_saving_handler::<TradingCommand>(Some(Ustr::from("TWAP.execute")));
        msgbus::register_any("TWAP.execute".into(), algo_handler);

        let order = make_initialized_algorithm_order("O-20250208-ALGO-MODIFY-001");
        add_order_to_cache(&strategy, &order);

        strategy
            .modify_order(
                order.client_order_id(),
                Some(Quantity::from(200_000)),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let algo_messages = algo_messages.get_messages();
        assert_eq!(algo_messages.len(), 1);
        assert!(matches!(
            algo_messages.first(),
            Some(TradingCommand::ModifyOrder(command))
                if command.client_order_id == order.client_order_id()
        ));
        assert!(risk_messages.get_messages().is_empty());
        assert!(exec_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_modify_order_routes_accepted_algorithm_order_to_risk() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let (algo_handler, algo_messages) =
            get_any_saving_handler::<TradingCommand>(Some(Ustr::from("TWAP.execute")));
        msgbus::register_any("TWAP.execute".into(), algo_handler);

        let mut order = make_initialized_algorithm_order("O-20250208-ALGO-MODIFY-002");
        let account_id = AccountId::from("ACC-001");
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();
        order
            .apply(TestOrderEventStubs::accepted(
                &order,
                account_id,
                VenueOrderId::from("O-20250208-ALGO-MODIFY-002"),
            ))
            .unwrap();
        add_order_to_cache(&strategy, &order);

        strategy
            .modify_order(
                order.client_order_id(),
                Some(Quantity::from(200_000)),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let risk_messages = risk_messages.get_messages();
        assert_eq!(risk_messages.len(), 1);
        assert!(matches!(
            risk_messages.first(),
            Some(TradingCommand::ModifyOrder(command))
                if command.client_order_id == order.client_order_id()
        ));
        assert!(algo_messages.get_messages().is_empty());
        assert_eq!(
            strategy
                .cache()
                .order(&order.client_order_id())
                .unwrap()
                .status(),
            OrderStatus::PendingUpdate
        );
    }

    #[rstest]
    fn test_modify_order_routes_emulated_order_to_order_emulator() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let mut order = OrderTestBuilder::new(OrderType::StopMarket)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("TEST-001"))
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-20250208-EMULATED-MODIFY-001"))
            .side(OrderSide::Buy)
            .trigger_price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .build();
        order
            .apply(OrderEventAny::Emulated(
                OrderEmulatedSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .build(),
            ))
            .unwrap();
        add_order_to_cache(&strategy, &order);

        strategy
            .modify_order(
                order.client_order_id(),
                None,
                None,
                Some(Price::from("52000.0")),
                None,
                None,
            )
            .unwrap();

        let emulator_messages = emulator_messages.get_messages();
        assert_eq!(emulator_messages.len(), 1);
        assert!(matches!(
            emulator_messages.first(),
            Some(TradingCommand::ModifyOrder(command))
                if command.client_order_id == order.client_order_id()
        ));
        assert!(risk_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_modify_order_routes_initialized_trigger_order_to_emulator_reentrantly() {
        let strategy_id = StrategyId::from("REENTRANT-001");
        let modified_quantity = Quantity::from(200_000);
        let mut strategy = InitializedModifyStrategy {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(strategy_id),
                ..Default::default()
            }),
            modified_quantity,
        };
        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            clock.clone(),
            cache.clone(),
            None,
        )));
        strategy
            .core
            .register(trader_id, clock, cache, portfolio)
            .unwrap();
        strategy.initialize().unwrap();
        strategy.start().unwrap();

        let actor_id = strategy.actor_id().inner();
        register_component_actor(strategy);
        let order_handler = TypedHandler::from(move |event: &OrderEventAny| {
            let mut strategy =
                try_get_actor_unchecked::<InitializedModifyStrategy>(&actor_id).unwrap();
            strategy.handle_order_event(event.clone());
        });
        let topic = format!("events.order.{strategy_id}");
        msgbus::subscribe_order_events(topic.clone().into(), order_handler.clone(), None);

        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let (algo_handler, algo_messages) =
            get_any_saving_handler::<TradingCommand>(Some(Ustr::from("TWAP.execute")));
        msgbus::register_any("TWAP.execute".into(), algo_handler);

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-REENTRANT-001"))
            .side(OrderSide::Buy)
            .trigger_price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .build();
        let client_order_id = order.client_order_id();

        let mut strategy = try_get_actor_unchecked::<InitializedModifyStrategy>(&actor_id).unwrap();
        strategy.submit_order(order, None, None, None).unwrap();
        drop(strategy);

        msgbus::unsubscribe_order_events(topic.into(), &order_handler);
        deregister_component(&strategy_id.inner());
        deregister_actor(&actor_id);

        let emulator_messages = emulator_messages.get_messages();
        assert_eq!(emulator_messages.len(), 2);
        assert!(matches!(
            emulator_messages.first(),
            Some(TradingCommand::ModifyOrder(command))
                if command.client_order_id == client_order_id
                    && command.quantity == Some(modified_quantity)
        ));
        assert!(
            risk_messages
                .get_messages()
                .iter()
                .all(|command| !matches!(command, TradingCommand::ModifyOrder(_)))
        );
        assert!(
            algo_messages
                .get_messages()
                .iter()
                .all(|command| !matches!(command, TradingCommand::ModifyOrder(_)))
        );
        assert!(matches!(
            emulator_messages.get(1),
            Some(TradingCommand::SubmitOrder(command))
                if command.client_order_id == client_order_id
        ));
    }

    #[rstest]
    fn test_modify_order_prefers_emulator_over_algorithm_for_emulated_algorithm_order() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let (algo_handler, algo_messages) =
            get_any_saving_handler::<TradingCommand>(Some(Ustr::from("TWAP.execute")));
        msgbus::register_any("TWAP.execute".into(), algo_handler);

        let mut order = OrderTestBuilder::new(OrderType::StopMarket)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("TEST-001"))
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-20250208-EMULATED-ALGO-MODIFY-001"))
            .side(OrderSide::Buy)
            .trigger_price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .exec_algorithm_id(ExecAlgorithmId::from("TWAP"))
            .exec_spawn_id(ClientOrderId::from("O-20250208-EMULATED-ALGO-MODIFY-001"))
            .build();
        order
            .apply(OrderEventAny::Emulated(
                OrderEmulatedSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .build(),
            ))
            .unwrap();

        // An `Emulated` order satisfies both `is_emulated` and `is_active_local`, so this
        // order matches the emulator branch and the algorithm branch at once.
        assert!(order.is_emulated());
        assert!(order.is_active_local());
        assert!(order.exec_algorithm_id().is_some());

        add_order_to_cache(&strategy, &order);

        strategy
            .modify_order(
                order.client_order_id(),
                None,
                None,
                Some(Price::from("52000.0")),
                None,
                None,
            )
            .unwrap();

        let emulator_messages = emulator_messages.get_messages();
        assert_eq!(emulator_messages.len(), 1);
        assert!(matches!(
            emulator_messages.first(),
            Some(TradingCommand::ModifyOrder(command))
                if command.client_order_id == order.client_order_id()
        ));
        assert!(algo_messages.get_messages().is_empty());
        assert!(risk_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_modify_order_marks_order_pending_update_locally_before_send() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let (event_handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
            get_typed_message_saving_handler(Some(Ustr::from("events.order.pending_update")));
        let order = make_accepted_limit_order("O-20250208-UPDATE-001");
        let topic = format!("events.order.{}", order.strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        add_order_to_cache(&strategy, &order);

        strategy
            .modify_order(
                order.client_order_id(),
                None,
                Some(Price::from("51000.0")),
                None,
                None,
                None,
            )
            .unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        let cache = strategy.cache();
        let cached_order = cache.order(&order.client_order_id()).unwrap();
        assert_eq!(cached_order.status(), OrderStatus::PendingUpdate);

        let risk_messages = risk_messages.get_messages();
        assert_eq!(risk_messages.len(), 1);
        assert!(matches!(
            risk_messages.first(),
            Some(TradingCommand::ModifyOrder(_))
        ));

        let event_messages = event_messages.get_messages();
        assert_eq!(event_messages.len(), 1);
        assert!(matches!(
            event_messages.first(),
            Some(OrderEventAny::PendingUpdate(_))
        ));
    }

    #[rstest]
    fn test_modify_orders_marks_orders_pending_update_locally_before_send() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let (event_handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
            get_typed_message_saving_handler(Some(Ustr::from("events.order.batch_pending_update")));
        let order1 = make_accepted_limit_order("O-20250208-BATCH-UPDATE-001");
        let order2 = make_accepted_limit_order("O-20250208-BATCH-UPDATE-002");
        let topic = format!("events.order.{}", order1.strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        add_order_to_cache(&strategy, &order1);
        add_order_to_cache(&strategy, &order2);

        strategy
            .modify_orders(
                vec![
                    (
                        order1.client_order_id(),
                        None,
                        Some(Price::from("51000.0")),
                        None,
                    ),
                    (
                        order2.client_order_id(),
                        Some(Quantity::from("2.0")),
                        None,
                        None,
                    ),
                ],
                None,
                None,
            )
            .unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        let cache = strategy.cache();
        let cached_order1 = cache.order(&order1.client_order_id()).unwrap();
        let cached_order2 = cache.order(&order2.client_order_id()).unwrap();
        assert_eq!(cached_order1.status(), OrderStatus::PendingUpdate);
        assert_eq!(cached_order2.status(), OrderStatus::PendingUpdate);

        let risk_messages = risk_messages.get_messages();
        assert_eq!(risk_messages.len(), 1);
        let Some(TradingCommand::ModifyOrders(command)) = risk_messages.first() else {
            panic!("expected BatchModifyOrders command");
        };
        assert_eq!(command.modifies.len(), 2);
        assert_eq!(
            command
                .modifies
                .iter()
                .map(|modify| modify.client_order_id)
                .collect::<Vec<_>>(),
            vec![order1.client_order_id(), order2.client_order_id()]
        );

        let event_messages = event_messages.get_messages();
        assert_eq!(event_messages.len(), 2);
        assert!(
            event_messages
                .iter()
                .all(|event| matches!(event, OrderEventAny::PendingUpdate(_)))
        );
    }

    #[rstest]
    fn test_cancel_order_marks_order_pending_cancel_locally_before_send() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let (event_handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
            get_typed_message_saving_handler(Some(Ustr::from("events.order.pending_cancel")));
        let order = make_accepted_market_order("O-20250208-CANCEL-001");
        let topic = format!("events.order.{}", order.strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        add_order_to_cache(&strategy, &order);

        strategy
            .cancel_order(order.client_order_id(), None, None)
            .unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        let cache = strategy.cache();
        let cached_order = cache.order(&order.client_order_id()).unwrap();
        assert_eq!(cached_order.status(), OrderStatus::PendingCancel);
        let cache = strategy.core.cache_ref();
        assert!(cache.is_order_pending_cancel_local(&order.client_order_id()));

        let exec_messages = exec_messages.get_messages();
        assert_eq!(exec_messages.len(), 1);
        assert!(matches!(
            exec_messages.first(),
            Some(TradingCommand::CancelOrder(_))
        ));

        let event_messages = event_messages.get_messages();
        assert_eq!(event_messages.len(), 1);
        assert!(matches!(
            event_messages.first(),
            Some(OrderEventAny::PendingCancel(_))
        ));
    }

    #[rstest]
    fn test_cancel_all_orders_strategy_only_sends_only_caller_strategy_cancels() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let order = make_accepted_market_order("O-20250208-CANCEL-ALL-001");
        let mut sibling_order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("SIBLING-001"))
            .instrument_id(order.instrument_id())
            .client_order_id(ClientOrderId::from("O-20250208-CANCEL-ALL-002"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let account_id = AccountId::from("ACC-001");
        sibling_order
            .apply(TestOrderEventStubs::submitted(&sibling_order, account_id))
            .unwrap();
        sibling_order
            .apply(TestOrderEventStubs::accepted(
                &sibling_order,
                account_id,
                VenueOrderId::from("2"),
            ))
            .unwrap();
        add_order_to_cache(&strategy, &order);
        add_order_to_cache(&strategy, &sibling_order);
        strategy.core.cache_rc().borrow_mut().build_index();

        strategy
            .cancel_all_orders(order.instrument_id(), None, None, true, None)
            .unwrap();

        let messages = exec_messages.get_messages();
        let cache = strategy.cache();
        let cached_order = cache.order(&order.client_order_id()).unwrap();
        let cached_sibling = cache.order(&sibling_order.client_order_id()).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages.first(),
            Some(TradingCommand::CancelOrder(command))
                if command.client_order_id == order.client_order_id()
        ));
        assert_eq!(cached_order.status(), OrderStatus::PendingCancel);
        assert_eq!(cached_sibling.status(), OrderStatus::Accepted);
    }

    #[rstest]
    fn test_cancel_all_orders_strategy_only_deduplicates_emulated_inflight_order() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );
        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let client_order_id = ClientOrderId::from("O-20250208-CANCEL-ALL-EMULATED-001");
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("TEST-001"))
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .exec_algorithm_id(ExecAlgorithmId::from("ALGO-001"))
            .exec_spawn_id(client_order_id)
            .build();
        add_order_to_cache(&strategy, &order);
        strategy.core.cache_rc().borrow_mut().build_index();

        strategy
            .cancel_all_orders(order.instrument_id(), None, None, true, None)
            .unwrap();

        let emulator_messages = emulator_messages.get_messages();
        assert_eq!(emulator_messages.len(), 1);
        assert!(matches!(
            emulator_messages.first(),
            Some(TradingCommand::CancelOrder(command))
                if command.client_order_id == order.client_order_id()
        ));
        assert!(exec_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_cancel_all_orders_without_strategy_only_sends_cancel_all_command() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

        strategy
            .cancel_all_orders(instrument_id, None, None, false, None)
            .unwrap();

        let messages = exec_messages.get_messages();
        assert_eq!(messages.len(), 1);
        let Some(TradingCommand::CancelAllOrders(command)) = messages.first() else {
            panic!("Expected a CancelAllOrders command, was {messages:?}");
        };
        assert_eq!(command.strategy_id, StrategyId::from("TEST-001"));
        assert_eq!(command.instrument_id, instrument_id);
        assert_eq!(command.client_id, None);
        assert_eq!(command.order_side, None);
        assert_eq!(command.correlation_id, Some(command.command_id));
        assert_eq!(command.causation_id, None);
    }

    #[rstest]
    fn test_cancel_all_orders_without_strategy_only_delegates_local_routing_with_params() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );
        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );
        let exec_algorithm_id = ExecAlgorithmId::from("ALGO-001");
        let algorithm_endpoint = format!("{exec_algorithm_id}.execute");
        let (algorithm_handler, algorithm_messages) =
            get_any_saving_handler::<TradingCommand>(Some(Ustr::from("ALGO-001.execute")));
        msgbus::register_any(algorithm_endpoint.into(), algorithm_handler);

        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let sibling_strategy_id = StrategyId::from("SIBLING-001");
        let selected_client = ClientId::from("CLIENT-001");
        let account_id = AccountId::from("ACC-001");

        let mut open_order = OrderTestBuilder::new(OrderType::Limit)
            .strategy_id(sibling_strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-BROAD-OPEN-001"))
            .side(OrderSide::Buy)
            .price(Price::from("49000.0"))
            .quantity(Quantity::from(100_000))
            .build();
        open_order
            .apply(TestOrderEventStubs::submitted(&open_order, account_id))
            .unwrap();
        open_order
            .apply(TestOrderEventStubs::accepted(
                &open_order,
                account_id,
                VenueOrderId::from("V-BROAD-OPEN-001"),
            ))
            .unwrap();
        let emulated_order = OrderTestBuilder::new(OrderType::StopMarket)
            .strategy_id(sibling_strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-BROAD-EMULATED-001"))
            .side(OrderSide::Buy)
            .trigger_price(Price::from("51000.0"))
            .quantity(Quantity::from(100_000))
            .emulation_trigger(TriggerType::BidAsk)
            .build();
        let algorithm_order_id = ClientOrderId::from("O-BROAD-ALGO-001");
        let algorithm_order = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(sibling_strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(algorithm_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .exec_algorithm_id(exec_algorithm_id)
            .exec_spawn_id(algorithm_order_id)
            .build();

        {
            let cache_rc = strategy.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            for order in [&open_order, &emulated_order, &algorithm_order] {
                cache
                    .add_order(order.clone(), None, Some(selected_client), true)
                    .unwrap();
            }
            cache.build_index();
        }

        let mut params = Params::new();
        params.insert(
            "routing_hint".to_string(),
            Value::String("broad_cancel".to_string()),
        );
        strategy
            .cancel_all_orders(
                instrument_id,
                Some(OrderSide::Buy),
                Some(selected_client),
                false,
                Some(params.clone()),
            )
            .unwrap();

        let exec_messages = exec_messages.get_messages();
        let Some(TradingCommand::CancelAllOrders(exec_command)) = exec_messages.first() else {
            panic!("Expected an execution CancelAllOrders command, was {exec_messages:?}");
        };

        assert_eq!(exec_messages.len(), 1);
        assert!(emulator_messages.get_messages().is_empty());
        assert!(algorithm_messages.get_messages().is_empty());
        assert_eq!(exec_command.client_id, Some(selected_client));
        assert_eq!(exec_command.strategy_id, StrategyId::from("TEST-001"));
        assert_eq!(exec_command.instrument_id, instrument_id);
        assert_eq!(exec_command.order_side, Some(OrderSide::Buy));
        assert_eq!(exec_command.params.as_ref(), Some(&params));
        assert_eq!(exec_command.correlation_id, Some(exec_command.command_id));
        assert_eq!(exec_command.causation_id, None);
    }

    #[rstest]
    fn test_cancel_all_orders_strategy_only_filters_by_client() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let selected_client = ClientId::from("CLIENT-001");
        let other_client = ClientId::from("CLIENT-002");
        let selected_order = make_accepted_market_order("O-20250208-CANCEL-CLIENT-001");
        let other_order = make_accepted_market_order("O-20250208-CANCEL-CLIENT-002");
        let cache_rc = strategy.core.cache_rc();
        {
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(selected_order.clone(), None, Some(selected_client), true)
                .unwrap();
            cache
                .add_order(other_order.clone(), None, Some(other_client), true)
                .unwrap();
            cache.build_index();
        }

        strategy
            .cancel_all_orders(
                selected_order.instrument_id(),
                None,
                Some(selected_client),
                true,
                None,
            )
            .unwrap();

        let messages = exec_messages.get_messages();
        let cache = strategy.cache();
        let cached_selected = cache.order(&selected_order.client_order_id()).unwrap();
        let cached_other = cache.order(&other_order.client_order_id()).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages.first(),
            Some(TradingCommand::CancelOrder(command))
                if command.client_id == Some(selected_client)
                    && command.client_order_id == selected_order.client_order_id()
        ));
        assert_eq!(cached_selected.status(), OrderStatus::PendingCancel);
        assert_eq!(cached_other.status(), OrderStatus::Accepted);
    }

    #[rstest]
    fn test_cancel_all_orders_strategy_only_filters_by_side_and_preserves_params() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let buy_order = make_accepted_market_order("O-20250208-CANCEL-SIDE-001");
        let mut sell_order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("TEST-001"))
            .instrument_id(buy_order.instrument_id())
            .client_order_id(ClientOrderId::from("O-20250208-CANCEL-SIDE-002"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();
        let account_id = AccountId::from("ACC-001");
        sell_order
            .apply(TestOrderEventStubs::submitted(&sell_order, account_id))
            .unwrap();
        sell_order
            .apply(TestOrderEventStubs::accepted(
                &sell_order,
                account_id,
                VenueOrderId::from("O-20250208-CANCEL-SIDE-002"),
            ))
            .unwrap();
        add_order_to_cache(&strategy, &buy_order);
        add_order_to_cache(&strategy, &sell_order);
        strategy.core.cache_rc().borrow_mut().build_index();

        let mut params = Params::new();
        params.insert(
            "routing_hint".to_string(),
            Value::String("strategy_only".to_string()),
        );
        strategy
            .cancel_all_orders(
                buy_order.instrument_id(),
                Some(OrderSide::Buy),
                None,
                true,
                Some(params.clone()),
            )
            .unwrap();

        let messages = exec_messages.get_messages();
        let Some(TradingCommand::CancelOrder(command)) = messages.first() else {
            panic!("Expected a CancelOrder command, was {messages:?}");
        };
        let cache = strategy.cache();
        let cached_buy = cache.order(&buy_order.client_order_id()).unwrap();
        let cached_sell = cache.order(&sell_order.client_order_id()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(command.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(command.client_id, None);
        assert_eq!(command.strategy_id, StrategyId::from("TEST-001"));
        assert_eq!(command.instrument_id, buy_order.instrument_id());
        assert_eq!(command.client_order_id, buy_order.client_order_id());
        assert_eq!(command.venue_order_id, buy_order.venue_order_id());
        assert_eq!(command.params.as_ref(), Some(&params));
        assert_eq!(cached_buy.status(), OrderStatus::PendingCancel);
        assert_eq!(cached_sell.status(), OrderStatus::Accepted);
    }

    #[rstest]
    fn test_cancel_all_orders_strategy_only_continues_after_error_and_returns_first_error() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let mut failing_order = make_accepted_market_order("O-20250208-CANCEL-ERROR-001");
        let OrderAny::Market(order) = &mut failing_order else {
            panic!("Expected a MarketOrder");
        };
        order.account_id = None;
        let succeeding_order = make_accepted_market_order("O-20250208-CANCEL-ERROR-002");
        add_order_to_cache(&strategy, &failing_order);
        add_order_to_cache(&strategy, &succeeding_order);
        strategy.core.cache_rc().borrow_mut().build_index();

        let error = strategy
            .cancel_all_orders(failing_order.instrument_id(), None, None, true, None)
            .unwrap_err()
            .to_string();

        let messages = exec_messages.get_messages();
        let cache = strategy.cache();
        let cached_failing = cache.order(&failing_order.client_order_id()).unwrap();
        let cached_succeeding = cache.order(&succeeding_order.client_order_id()).unwrap();
        assert_eq!(
            error,
            "Cannot generate pending cancel event for O-20250208-CANCEL-ERROR-001: \
             account_id is not set"
        );
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages.first(),
            Some(TradingCommand::CancelOrder(command))
                if command.client_order_id == succeeding_order.client_order_id()
        ));
        assert_eq!(cached_failing.status(), OrderStatus::Accepted);
        assert_eq!(cached_succeeding.status(), OrderStatus::PendingCancel);
    }

    #[rstest]
    fn test_cancel_orders_marks_orders_pending_cancel_locally_before_send() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let (event_handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
            get_typed_message_saving_handler(Some(Ustr::from("events.order.batch_pending_cancel")));
        let order1 = make_accepted_market_order("O-20250208-CANCEL-001");
        let order2 = make_accepted_market_order("O-20250208-CANCEL-002");
        let topic = format!("events.order.{}", order1.strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        add_order_to_cache(&strategy, &order1);
        add_order_to_cache(&strategy, &order2);

        strategy
            .cancel_orders(
                vec![order1.client_order_id(), order2.client_order_id()],
                None,
                None,
            )
            .unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        let cache = strategy.cache();
        let cached_order1 = cache.order(&order1.client_order_id()).unwrap();
        let cached_order2 = cache.order(&order2.client_order_id()).unwrap();
        assert_eq!(cached_order1.status(), OrderStatus::PendingCancel);
        assert_eq!(cached_order2.status(), OrderStatus::PendingCancel);
        let cache = strategy.core.cache_ref();
        assert!(cache.is_order_pending_cancel_local(&order1.client_order_id()));
        assert!(cache.is_order_pending_cancel_local(&order2.client_order_id()));

        let exec_messages = exec_messages.get_messages();
        assert_eq!(exec_messages.len(), 1);
        let Some(TradingCommand::CancelOrders(command)) = exec_messages.first() else {
            panic!("expected BatchCancelOrders command");
        };
        assert_eq!(command.cancels.len(), 2);

        let event_messages = event_messages.get_messages();
        assert_eq!(event_messages.len(), 2);
        assert!(
            event_messages
                .iter()
                .all(|event| matches!(event, OrderEventAny::PendingCancel(_)))
        );
    }

    #[rstest]
    fn test_cancel_order_updates_own_book_status_before_send() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, _exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let order = make_accepted_limit_order("O-20250208-CANCEL-OWN-BOOK-001");
        add_order_to_cache_and_own_book(&strategy, &order);

        strategy
            .cancel_order(order.client_order_id(), None, None)
            .unwrap();

        let mut accepted = AHashSet::new();
        accepted.insert(OrderStatus::Accepted);
        let mut pending_cancel = AHashSet::new();
        pending_cancel.insert(OrderStatus::PendingCancel);

        let cache = strategy.cache();
        let own_book = cache.own_order_book(&order.instrument_id()).unwrap();
        assert!(own_book.bids_as_map(Some(&accepted), None, None).is_empty());
        let pending_bids = own_book.bids_as_map(Some(&pending_cancel), None, None);
        assert_eq!(pending_bids.values().map(Vec::len).sum::<usize>(), 1);
    }

    #[rstest]
    fn test_cancel_order_returns_error_when_not_in_cache() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let missing_id = ClientOrderId::from("O-MISSING");
        let err = strategy
            .cancel_order(missing_id, None, None)
            .expect_err("expected cancel_order to fail when order is not in cache");

        assert_eq!(
            err.to_string(),
            format!("Cannot cancel order: {ORDER_NOT_FOUND}: {missing_id}")
        );
        assert!(exec_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_modify_order_returns_error_when_not_in_cache() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let missing_id = ClientOrderId::from("O-MISSING");
        let err = strategy
            .modify_order(missing_id, Some(Quantity::from(1)), None, None, None, None)
            .expect_err("expected modify_order to fail when order is not in cache");

        assert_eq!(
            err.to_string(),
            format!("Cannot modify order: {ORDER_NOT_FOUND}: {missing_id}")
        );
        assert!(risk_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_modify_orders_returns_error_when_any_id_missing() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let order = make_accepted_limit_order("O-PRESENT");
        add_order_to_cache(&strategy, &order);

        let missing_id = ClientOrderId::from("O-MISSING");
        let err = strategy
            .modify_orders(
                vec![
                    (
                        order.client_order_id(),
                        None,
                        Some(Price::from("51000.0")),
                        None,
                    ),
                    (missing_id, Some(Quantity::from("2.0")), None, None),
                ],
                None,
                None,
            )
            .expect_err("expected modify_orders to fail when any id is missing");

        assert_eq!(
            err.to_string(),
            format!("Cannot modify order: {ORDER_NOT_FOUND}: {missing_id}")
        );
        assert!(risk_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_cancel_orders_returns_error_when_any_id_missing() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let (exec_handler, exec_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("ExecEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            exec_handler,
        );

        let order = make_accepted_limit_order("O-PRESENT");
        add_order_to_cache(&strategy, &order);

        let missing_id = ClientOrderId::from("O-MISSING");
        let err = strategy
            .cancel_orders(vec![order.client_order_id(), missing_id], None, None)
            .expect_err("expected cancel_orders to fail when any id is missing");

        assert_eq!(
            err.to_string(),
            format!("Cannot cancel order: {ORDER_NOT_FOUND}: {missing_id}")
        );
        assert!(exec_messages.get_messages().is_empty());
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
    #[case::matching_order(Ustr::from("GTD-EXPIRY:O-PRESENT"))]
    #[case::empty_order_id(Ustr::from("GTD-EXPIRY:"))]
    fn test_route_time_event_ignores_unregistered_gtd_timer(#[case] timer_name: Ustr) {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let order = make_accepted_limit_order("O-PRESENT");
        let client_order_id = order.client_order_id();
        add_order_to_cache(&strategy, &order);
        let event = TimeEvent::new(
            timer_name,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        route_time_event(&mut strategy, &event);

        let cache = strategy.core.cache_ref();
        let cached_order = cache.order(&client_order_id).unwrap();
        assert_eq!(cached_order.status(), OrderStatus::Accepted);
        assert!(!strategy.core.gtd_timers.contains_key(&client_order_id));
    }

    #[rstest]
    #[case::filled(make_filled)]
    #[case::canceled(make_canceled)]
    #[case::rejected(make_rejected)]
    #[case::expired(make_expired)]
    #[case::fill_voided(make_terminal_fill_voided)]
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
    #[case::partial_fill(make_filled)]
    #[case::non_reopened_fill_void(make_terminal_fill_voided)]
    fn test_handle_order_event_keeps_gtd_timer_when_cached_order_remains_open(
        #[case] make_event: fn(ClientOrderId) -> OrderEventAny,
    ) {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        let order = make_accepted_limit_order(client_order_id.as_str());
        add_order_to_cache(&strategy, &order);
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        strategy.handle_order_event(make_event(client_order_id));

        assert!(strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    #[case::filled(make_filled)]
    #[case::canceled(make_canceled)]
    #[case::rejected(make_rejected)]
    #[case::expired(make_expired)]
    #[case::fill_voided(make_terminal_fill_voided)]
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
        assert_eq!(strategy.state(), ComponentState::Stopped);

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
    fn test_handle_reopened_fill_void_keeps_gtd_timer() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        strategy.handle_order_event(make_fill_voided(client_order_id, true));

        assert!(strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_handle_order_event_skips_dispatch_when_stopped() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        stop_strategy(&mut strategy);
        assert_eq!(strategy.state(), ComponentState::Stopped);

        strategy.handle_order_event(make_rejected(ClientOrderId::from("O-001")));

        assert!(!strategy.on_order_event_called);
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

    #[rstest]
    fn test_on_start_errors_when_strategy_id_is_not_set() {
        let mut strategy = TestStrategy::new(StrategyConfig::default());

        let err = Strategy::on_start(&mut strategy).unwrap_err().to_string();

        assert_eq!(err, "Strategy not registered: strategy_id is not set");
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
            clock.clone(),
            cache.clone(),
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
            clock.clone(),
            cache.clone(),
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
            clock.clone(),
            cache.clone(),
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
    fn test_route_time_event_is_idempotent_when_callback_forwards() {
        let mut strategy = TestStrategy::new(StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            ..Default::default()
        });
        register_strategy(&mut strategy);

        let order = make_accepted_limit_order("O-PRESENT");
        add_order_to_cache(&strategy, &order);
        strategy.core.cache_rc().borrow_mut().build_index();
        strategy.core.is_exiting = true;
        let event = TimeEvent::new(
            strategy.core.market_exit_timer_name,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        route_time_event(&mut strategy, &event);
        Strategy::on_time_event(&mut strategy, &event).unwrap();

        assert!(strategy.core.is_exiting);
        assert_eq!(strategy.core.market_exit_attempts, 1);
        assert_eq!(
            strategy.core.managed_time_event_last_id,
            Some(event.event_id)
        );
    }

    #[rstest]
    fn test_route_time_event_deduplicates_overridden_managed_handlers() {
        let mut strategy = TimerOverrideStrategy {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("TEST-001")),
                ..Default::default()
            }),
            gtd_expiries: 0,
            market_exit_checks: 0,
        };
        let market_event = TimeEvent::new(
            strategy.core.market_exit_timer_name,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        route_time_event(&mut strategy, &market_event);
        DataActor::on_time_event(&mut strategy, &market_event).unwrap();

        let client_order_id = ClientOrderId::from("O-001");
        let gtd_timer_name = Ustr::from("GTD-EXPIRY:O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, gtd_timer_name);
        let gtd_event = TimeEvent::new(
            gtd_timer_name,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        route_time_event(&mut strategy, &gtd_event);
        DataActor::on_time_event(&mut strategy, &gtd_event).unwrap();

        assert_eq!(strategy.market_exit_checks, 1);
        assert_eq!(strategy.gtd_expiries, 1);
        assert!(strategy.core.gtd_timers.contains_key(&client_order_id));
        assert_eq!(
            strategy.core.managed_time_event_last_id,
            Some(gtd_event.event_id)
        );
    }

    #[rstest]
    fn test_route_time_event_ignores_unowned_market_exit_timer() {
        let mut strategy = TestStrategy::new(StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            ..Default::default()
        });
        register_strategy(&mut strategy);

        let order = make_accepted_limit_order("O-PRESENT");
        add_order_to_cache(&strategy, &order);
        strategy.core.cache_rc().borrow_mut().build_index();
        strategy.core.is_exiting = true;
        let event = TimeEvent::new(
            Ustr::from("MARKET_EXIT_CHECK:OTHER-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        route_time_event(&mut strategy, &event);

        assert!(strategy.core.is_exiting);
        assert_eq!(strategy.core.market_exit_attempts, 0);
        assert_eq!(strategy.core.managed_time_event_last_id, None);
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
            clock.clone(),
            cache.clone(),
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
        assert!(!strategy.is_running());

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
            clock.clone(),
            cache.clone(),
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
        assert!(!strategy.is_running());

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
        assert_eq!(strategy.state(), ComponentState::Stopped);
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
        assert_eq!(strategy.state(), ComponentState::Running);
        assert!(!strategy.core.is_exiting);
    }

    #[rstest]
    fn test_submit_order_denied_during_market_exit_when_not_reduce_only() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        strategy.core.is_exiting = true;

        let (event_handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
            get_typed_message_saving_handler(Some(Ustr::from("events.order.denied")));
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
        let topic = format!("events.order.{}", order.strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        let client_order_id = order.client_order_id();
        let result = strategy.submit_order(order.clone(), None, None, None);

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        assert!(result.is_ok());
        let cache = strategy.cache();
        let cached_order = cache.order(&client_order_id).unwrap();
        assert_eq!(cached_order.status(), OrderStatus::Denied);

        let event_messages = event_messages.get_messages();
        assert_eq!(event_messages.len(), 2);
        assert_eq!(
            event_messages[0],
            OrderEventAny::Initialized(order.init_event().clone())
        );
        let OrderEventAny::Denied(denied) = &event_messages[1] else {
            panic!("expected OrderDenied event");
        };
        assert_eq!(denied.reason, Ustr::from("MARKET_EXIT_IN_PROGRESS"));
    }

    #[rstest]
    fn test_submit_order_list_denied_during_market_exit_publishes_init_then_denied_events() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        strategy.core.is_exiting = true;

        let orders = vec![
            make_initialized_market_order("O-20250208-LIST-DENY-001"),
            make_initialized_market_order("O-20250208-LIST-DENY-002"),
        ];
        let client_order_id1 = orders[0].client_order_id();
        let client_order_id2 = orders[1].client_order_id();
        let cache_rc = strategy.core.cache_rc();
        let timeline = Rc::new(RefCell::new(Vec::new()));
        let event_messages = Rc::new(RefCell::new(Vec::new()));

        let event_handler = {
            let event_messages = event_messages.clone();
            let timeline = timeline.clone();
            TypedHandler::from_with_id("events.order.list_denied", move |event: &OrderEventAny| {
                match event {
                    OrderEventAny::Initialized(e) if e.client_order_id == client_order_id1 => {
                        assert!(cache_rc.borrow().order_exists(&client_order_id1));
                        timeline.borrow_mut().push("init1");
                    }
                    OrderEventAny::Initialized(e) if e.client_order_id == client_order_id2 => {
                        assert!(cache_rc.borrow().order_exists(&client_order_id2));
                        timeline.borrow_mut().push("init2");
                    }
                    OrderEventAny::Denied(e) if e.client_order_id == client_order_id1 => {
                        assert_eq!(e.reason, Ustr::from("MARKET_EXIT_IN_PROGRESS"));
                        let cache = cache_rc.borrow();
                        let cached_order = cache.order(&client_order_id1).unwrap();
                        assert_eq!(cached_order.status(), OrderStatus::Denied);
                        timeline.borrow_mut().push("denied1");
                    }
                    OrderEventAny::Denied(e) if e.client_order_id == client_order_id2 => {
                        assert_eq!(e.reason, Ustr::from("MARKET_EXIT_IN_PROGRESS"));
                        let cache = cache_rc.borrow();
                        let cached_order = cache.order(&client_order_id2).unwrap();
                        assert_eq!(cached_order.status(), OrderStatus::Denied);
                        timeline.borrow_mut().push("denied2");
                    }
                    _ => panic!("unexpected order event {event:?}"),
                }
                event_messages.borrow_mut().push(event.clone());
            })
        };
        let risk_handler = {
            let timeline = timeline.clone();
            TypedIntoHandler::from_with_id(
                "RiskEngine.queue_execute",
                move |_command: TradingCommand| {
                    timeline.borrow_mut().push("command");
                },
            )
        };
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let topic = format!("events.order.{}", orders[0].strategy_id());
        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        let result = strategy.submit_order_list(orders.clone(), None, None, None);

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        assert!(result.is_ok());

        let cache = strategy.cache();
        let cached_order1 = cache.order(&client_order_id1).unwrap();
        let cached_order2 = cache.order(&client_order_id2).unwrap();
        assert_eq!(cached_order1.status(), OrderStatus::Denied);
        assert_eq!(cached_order2.status(), OrderStatus::Denied);

        let event_messages = event_messages.borrow();
        assert_eq!(event_messages.len(), 4);
        assert_eq!(
            event_messages[0],
            OrderEventAny::Initialized(orders[0].init_event().clone())
        );
        assert!(matches!(
            &event_messages[1],
            OrderEventAny::Denied(e)
                if e.client_order_id == client_order_id1
                    && e.reason == Ustr::from("MARKET_EXIT_IN_PROGRESS")
        ));
        assert_eq!(
            event_messages[2],
            OrderEventAny::Initialized(orders[1].init_event().clone())
        );
        assert!(matches!(
            &event_messages[3],
            OrderEventAny::Denied(e)
                if e.client_order_id == client_order_id2
                    && e.reason == Ustr::from("MARKET_EXIT_IN_PROGRESS")
        ));
        assert_eq!(
            timeline.borrow().as_slice(),
            &["init1", "denied1", "init2", "denied2"]
        );
    }

    #[rstest]
    fn test_submit_order_list_market_exit_rejects_non_initialized_without_events() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);
        strategy.core.is_exiting = true;

        let order = make_accepted_market_order("O-20250208-LIST-DENY-ACCEPTED");
        let topic = format!("events.order.{}", order.strategy_id());
        let (event_handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
            get_typed_message_saving_handler(Some(Ustr::from("events.order.list_invalid")));

        msgbus::subscribe_order_events(topic.clone().into(), event_handler.clone(), None);
        let result = strategy.submit_order_list(vec![order], None, None, None);

        msgbus::unsubscribe_order_events(topic.into(), &event_handler);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected INITIALIZED")
        );
        assert!(event_messages.get_messages().is_empty());
    }

    #[rstest]
    fn test_submit_order_list_rejects_mixed_venues_with_friendly_error() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);
        start_strategy(&mut strategy);

        let binance_order = make_initialized_market_order("O-MIXED-VENUE-001");
        let bybit_order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("TEST-001"),
            InstrumentId::from("BTCUSDT.BYBIT"),
            ClientOrderId::from("O-MIXED-VENUE-002"),
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

        let result = strategy.submit_order_list(vec![binance_order, bybit_order], None, None, None);

        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("OrderList denied: orders must share the same venue"),
            "unexpected error: {msg}",
        );
        assert!(msg.contains("BINANCE"), "expected BINANCE in error: {msg}");
        assert!(msg.contains("BYBIT"), "expected BYBIT in error: {msg}");
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
        let result = strategy.submit_order(order, None, None, None);

        assert!(result.is_ok());
        let cache = strategy.cache();
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
        let result = strategy.submit_order(order, None, None, None);

        assert!(result.is_ok());
        let cache = strategy.cache();
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
        fn external_order_instrument_ids(&self) -> Option<Vec<InstrumentId>> {
            None
        }
    });

    impl DataActor for MacroTestCustomField {}

    #[rstest]
    fn test_strategy_behavior_does_not_require_native_core_access() {
        fn assert_strategy<T: Strategy + DataActor>() {}

        assert_strategy::<CoreFreeStrategy>();

        let mut strategy = CoreFreeStrategy { started: false };
        DataActor::on_start(&mut strategy).unwrap();

        assert!(strategy.started);
    }

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
        assert_eq!(simple.strategy_id(), config.strategy_id);
        assert_eq!(simple.config().order_id_tag, config.order_id_tag);
        assert_eq!(simple.actor_id(), ActorId::from("MACRO-001"));

        let hooks = MacroTestWithHooks {
            core: StrategyCore::new(config.clone()),
        };
        assert_eq!(hooks.strategy_id(), config.strategy_id);
        assert_eq!(hooks.config().order_id_tag, config.order_id_tag);
        assert_eq!(hooks.actor_id(), ActorId::from("MACRO-001"));

        let custom = MacroTestCustomField {
            inner: StrategyCore::new(config.clone()),
        };
        assert_eq!(custom.strategy_id(), config.strategy_id);
        assert_eq!(custom.config().order_id_tag, config.order_id_tag);
        assert_eq!(custom.actor_id(), ActorId::from("MACRO-001"));
        assert!(custom.external_order_instrument_ids().is_none());
    }
}
