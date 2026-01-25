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

//! Execution algorithm infrastructure for order slicing and execution optimization.
//!
//! This module provides the [`ExecutionAlgorithm`] trait and supporting infrastructure
//! for implementing algorithms like TWAP (Time-Weighted Average Price) and VWAP
//! (Volume-Weighted Average Price) that slice large orders into smaller child orders.
//!
//! # Architecture
//!
//! Execution algorithms extend [`DataActor`] (not [`Strategy`](super::Strategy)) because:
//! - They don't own positions (the parent Strategy does).
//! - Spawned orders carry the parent Strategy's ID, not the algorithm's ID.
//! - They act as order processors/transformers, not position managers.
//!
//! # Order Flow
//!
//! 1. A Strategy submits an order with `exec_algorithm_id` set.
//! 2. The order is routed to the algorithm's `{id}.execute` endpoint.
//! 3. The algorithm receives the order via `on_order()`.
//! 4. The algorithm spawns child orders using `spawn_market()`, `spawn_limit()`, etc.
//! 5. Spawned orders are submitted through the RiskEngine.
//! 6. The algorithm receives fill events and manages remaining quantity.

pub mod config;
pub mod core;
pub mod twap;

pub use core::{ExecutionAlgorithmCore, StrategyEventHandlers};

pub use config::ExecutionAlgorithmConfig;
use nautilus_common::{
    actor::{DataActor, registry::try_get_actor_unchecked},
    enums::ComponentState,
    logging::{CMD, EVT, RECV, SEND},
    messages::execution::{CancelOrder, ModifyOrder, SubmitOrder, TradingCommand},
    msgbus::{self, MessagingSwitchboard, TypedHandler},
    timer::TimeEvent,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{OrderStatus, TimeInForce, TriggerType},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted,
        OrderTriggered, OrderUpdated, PositionChanged, PositionClosed, PositionEvent,
        PositionOpened,
    },
    identifiers::{ClientId, ExecAlgorithmId, PositionId, StrategyId},
    orders::{LimitOrder, MarketOrder, MarketToLimitOrder, Order, OrderAny, OrderList},
    types::{Price, Quantity},
};
pub use twap::{TwapAlgorithm, TwapAlgorithmConfig};
use ustr::Ustr;

/// Core trait for implementing execution algorithms in NautilusTrader.
///
/// Execution algorithms are specialized [`DataActor`]s that receive orders from strategies
/// and execute them by spawning child orders. They are used for order slicing algorithms
/// like TWAP and VWAP.
///
/// # Key Capabilities
///
/// - All [`DataActor`] capabilities (data subscriptions, event handling, timers)
/// - Order spawning (market, limit, market-to-limit)
/// - Order lifecycle management (submit, modify, cancel)
/// - Event filtering for algorithm-owned orders
///
/// # Implementation
///
/// User algorithms should implement the required methods and hold an
/// [`ExecutionAlgorithmCore`] member. The struct should `Deref` and `DerefMut`
/// to `ExecutionAlgorithmCore` (which itself derefs to `DataActorCore`).
pub trait ExecutionAlgorithm: DataActor {
    /// Provides mutable access to the internal `ExecutionAlgorithmCore`.
    fn core_mut(&mut self) -> &mut ExecutionAlgorithmCore;

    /// Returns the execution algorithm ID.
    fn id(&mut self) -> ExecAlgorithmId {
        self.core_mut().exec_algorithm_id
    }

    /// Executes a trading command.
    ///
    /// This is the main entry point for commands routed to the algorithm.
    /// Dispatches to the appropriate handler based on command type.
    ///
    /// Commands are only processed when the algorithm is in `Running` state.
    ///
    /// # Errors
    ///
    /// Returns an error if command handling fails.
    fn execute(&mut self, command: TradingCommand) -> anyhow::Result<()>
    where
        Self: 'static + std::fmt::Debug + Sized,
    {
        let core = self.core_mut();
        if core.config.log_commands {
            let id = &core.actor.actor_id;
            log::info!("{id} {RECV}{CMD} {command:?}");
        }

        if core.state() != ComponentState::Running {
            return Ok(());
        }

        match command {
            TradingCommand::SubmitOrder(cmd) => {
                self.subscribe_to_strategy_events(cmd.strategy_id);
                let order = self.core_mut().get_order(&cmd.client_order_id)?;
                self.on_order(order)
            }
            TradingCommand::SubmitOrderList(cmd) => {
                self.subscribe_to_strategy_events(cmd.strategy_id);
                self.on_order_list(cmd.order_list)
            }
            TradingCommand::CancelOrder(cmd) => self.handle_cancel_order(cmd),
            _ => {
                log::warn!("Unhandled command type: {command:?}");
                Ok(())
            }
        }
    }

    /// Called when a primary order is received for execution.
    ///
    /// Override this method to implement the algorithm's order slicing logic.
    ///
    /// # Errors
    ///
    /// Returns an error if order handling fails.
    fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()>;

    /// Called when an order list is received for execution.
    ///
    /// Override this method to handle order lists. The default implementation
    /// processes each order individually.
    ///
    /// # Errors
    ///
    /// Returns an error if order list handling fails.
    fn on_order_list(&mut self, order_list: OrderList) -> anyhow::Result<()> {
        for order in order_list.orders {
            self.on_order(order)?;
        }
        Ok(())
    }

    /// Handles a cancel order command for algorithm-managed orders.
    ///
    /// This generates an internal cancel event and publishes it. The order
    /// is canceled locally without sending a command to the execution engine.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    fn handle_cancel_order(&mut self, command: CancelOrder) -> anyhow::Result<()> {
        let (mut order, is_pending_cancel) = {
            let cache = self.core_mut().cache();

            let Some(order) = cache.order(&command.client_order_id) else {
                log::warn!(
                    "Cannot cancel order: {} not found in cache",
                    command.client_order_id
                );
                return Ok(());
            };

            let is_pending = cache.is_order_pending_cancel_local(&command.client_order_id);
            (order.clone(), is_pending)
        };

        if is_pending_cancel {
            return Ok(());
        }

        if order.is_closed() {
            log::warn!("Order already closed for {command:?}");
            return Ok(());
        }

        let event = self.generate_order_canceled(&order);

        if let Err(e) = order.apply(OrderEventAny::Canceled(event)) {
            log::warn!("InvalidStateTrigger: {e}, did not apply cancel event");
            return Ok(());
        }

        {
            let cache_rc = self.core_mut().cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&order)?;
        }

        let topic = format!("events.order.{}", order.strategy_id());
        msgbus::publish_order_event(topic.into(), &OrderEventAny::Canceled(event));

        Ok(())
    }

    /// Generates an OrderCanceled event for an order.
    fn generate_order_canceled(&mut self, order: &OrderAny) -> OrderCanceled {
        let ts_now = self.core_mut().clock().timestamp_ns();

        OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_now,
            ts_now,
            false, // reconciliation
            order.venue_order_id(),
            order.account_id(),
        )
    }

    /// Generates an OrderPendingUpdate event for an order.
    fn generate_order_pending_update(&mut self, order: &OrderAny) -> OrderPendingUpdate {
        let ts_now = self.core_mut().clock().timestamp_ns();

        OrderPendingUpdate::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order
                .account_id()
                .expect("Order must have account_id for pending update"),
            UUID4::new(),
            ts_now,
            ts_now,
            false, // reconciliation
            order.venue_order_id(),
        )
    }

    /// Generates an OrderPendingCancel event for an order.
    fn generate_order_pending_cancel(&mut self, order: &OrderAny) -> OrderPendingCancel {
        let ts_now = self.core_mut().clock().timestamp_ns();

        OrderPendingCancel::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order
                .account_id()
                .expect("Order must have account_id for pending cancel"),
            UUID4::new(),
            ts_now,
            ts_now,
            false, // reconciliation
            order.venue_order_id(),
        )
    }

    /// Spawns a market order from a primary order.
    ///
    /// Creates a new market order with:
    /// - A unique client order ID: `{primary_id}-E{sequence}`.
    /// - The primary order's trader ID, strategy ID, and instrument ID.
    /// - The algorithm's exec_algorithm_id.
    /// - exec_spawn_id set to the primary order's client order ID.
    ///
    /// If `reduce_primary` is true, the primary order's quantity will be reduced
    /// by the spawned quantity. If the spawned order is subsequently denied or
    /// rejected (before acceptance), the deducted quantity is automatically
    /// restored to the primary order.
    fn spawn_market(
        &mut self,
        primary: &mut OrderAny,
        quantity: Quantity,
        time_in_force: TimeInForce,
        reduce_only: bool,
        tags: Option<Vec<Ustr>>,
        reduce_primary: bool,
    ) -> MarketOrder {
        // Generate spawn ID first so we can track the reduction
        let core = self.core_mut();
        let client_order_id = core.spawn_client_order_id(&primary.client_order_id());
        let ts_init = core.clock().timestamp_ns();
        let exec_algorithm_id = core.exec_algorithm_id;

        if reduce_primary {
            self.reduce_primary_order(primary, quantity);
            self.core_mut()
                .track_pending_spawn_reduction(client_order_id, quantity);
        }

        MarketOrder::new(
            primary.trader_id(),
            primary.strategy_id(),
            primary.instrument_id(),
            client_order_id,
            primary.order_side(),
            quantity,
            time_in_force,
            UUID4::new(),
            ts_init,
            reduce_only,
            false, // quote_quantity
            primary.contingency_type(),
            primary.order_list_id(),
            primary.linked_order_ids().map(|ids| ids.to_vec()),
            primary.parent_order_id(),
            Some(exec_algorithm_id),
            primary.exec_algorithm_params().cloned(),
            Some(primary.client_order_id()),
            tags.or_else(|| primary.tags().map(|t| t.to_vec())),
        )
    }

    /// Spawns a limit order from a primary order.
    ///
    /// Creates a new limit order with:
    /// - A unique client order ID: `{primary_id}-E{sequence}`
    /// - The primary order's trader ID, strategy ID, and instrument ID
    /// - The algorithm's exec_algorithm_id
    /// - exec_spawn_id set to the primary order's client order ID
    ///
    /// If `reduce_primary` is true, the primary order's quantity will be reduced
    /// by the spawned quantity. If the spawned order is subsequently denied or
    /// rejected (before acceptance), the deducted quantity is automatically
    /// restored to the primary order.
    #[allow(clippy::too_many_arguments)]
    fn spawn_limit(
        &mut self,
        primary: &mut OrderAny,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        reduce_only: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        tags: Option<Vec<Ustr>>,
        reduce_primary: bool,
    ) -> LimitOrder {
        // Generate spawn ID first so we can track the reduction
        let core = self.core_mut();
        let client_order_id = core.spawn_client_order_id(&primary.client_order_id());
        let ts_init = core.clock().timestamp_ns();
        let exec_algorithm_id = core.exec_algorithm_id;

        if reduce_primary {
            self.reduce_primary_order(primary, quantity);
            self.core_mut()
                .track_pending_spawn_reduction(client_order_id, quantity);
        }

        LimitOrder::new(
            primary.trader_id(),
            primary.strategy_id(),
            primary.instrument_id(),
            client_order_id,
            primary.order_side(),
            quantity,
            price,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            false, // quote_quantity
            display_qty,
            emulation_trigger,
            None, // trigger_instrument_id
            primary.contingency_type(),
            primary.order_list_id(),
            primary.linked_order_ids().map(|ids| ids.to_vec()),
            primary.parent_order_id(),
            Some(exec_algorithm_id),
            primary.exec_algorithm_params().cloned(),
            Some(primary.client_order_id()),
            tags.or_else(|| primary.tags().map(|t| t.to_vec())),
            UUID4::new(),
            ts_init,
        )
    }

    /// Spawns a market-to-limit order from a primary order.
    ///
    /// Creates a new market-to-limit order with:
    /// - A unique client order ID: `{primary_id}-E{sequence}`
    /// - The primary order's trader ID, strategy ID, and instrument ID
    /// - The algorithm's exec_algorithm_id
    /// - exec_spawn_id set to the primary order's client order ID
    ///
    /// If `reduce_primary` is true, the primary order's quantity will be reduced
    /// by the spawned quantity. If the spawned order is subsequently denied or
    /// rejected (before acceptance), the deducted quantity is automatically
    /// restored to the primary order.
    #[allow(clippy::too_many_arguments)]
    fn spawn_market_to_limit(
        &mut self,
        primary: &mut OrderAny,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        reduce_only: bool,
        display_qty: Option<Quantity>,
        emulation_trigger: Option<TriggerType>,
        tags: Option<Vec<Ustr>>,
        reduce_primary: bool,
    ) -> MarketToLimitOrder {
        // Generate spawn ID first so we can track the reduction
        let core = self.core_mut();
        let client_order_id = core.spawn_client_order_id(&primary.client_order_id());
        let ts_init = core.clock().timestamp_ns();
        let exec_algorithm_id = core.exec_algorithm_id;

        if reduce_primary {
            self.reduce_primary_order(primary, quantity);
            self.core_mut()
                .track_pending_spawn_reduction(client_order_id, quantity);
        }

        let mut order = MarketToLimitOrder::new(
            primary.trader_id(),
            primary.strategy_id(),
            primary.instrument_id(),
            client_order_id,
            primary.order_side(),
            quantity,
            time_in_force,
            expire_time,
            false, // post_only
            reduce_only,
            false, // quote_quantity
            display_qty,
            primary.contingency_type(),
            primary.order_list_id(),
            primary.linked_order_ids().map(|ids| ids.to_vec()),
            primary.parent_order_id(),
            Some(exec_algorithm_id),
            primary.exec_algorithm_params().cloned(),
            Some(primary.client_order_id()),
            tags.or_else(|| primary.tags().map(|t| t.to_vec())),
            UUID4::new(),
            ts_init,
        );

        if emulation_trigger.is_some() {
            order.set_emulation_trigger(emulation_trigger);
        }

        order
    }

    /// Reduces the primary order's quantity by the spawn quantity.
    ///
    /// Generates an `OrderUpdated` event and applies it to the primary order,
    /// then updates the order in the cache.
    ///
    /// # Panics
    ///
    /// Panics if `spawn_qty` exceeds the primary order's `leaves_qty`.
    fn reduce_primary_order(&mut self, primary: &mut OrderAny, spawn_qty: Quantity) {
        let leaves_qty = primary.leaves_qty();
        assert!(
            leaves_qty >= spawn_qty,
            "Spawn quantity {spawn_qty} exceeds primary leaves_qty {leaves_qty}"
        );

        let primary_qty = primary.quantity();
        let new_qty = Quantity::from_raw(primary_qty.raw - spawn_qty.raw, primary_qty.precision);

        let core = self.core_mut();
        let ts_now = core.clock().timestamp_ns();

        let updated = OrderUpdated::new(
            primary.trader_id(),
            primary.strategy_id(),
            primary.instrument_id(),
            primary.client_order_id(),
            new_qty,
            UUID4::new(),
            ts_now,
            ts_now,
            false, // reconciliation
            primary.venue_order_id(),
            primary.account_id(),
            None, // price
            None, // trigger_price
            None, // protection_price
        );

        primary
            .apply(OrderEventAny::Updated(updated))
            .expect("Failed to apply OrderUpdated");

        let cache_rc = core.cache_rc();
        let mut cache = cache_rc.borrow_mut();
        cache
            .update_order(primary)
            .expect("Failed to update order in cache");
    }

    /// Restores the primary order quantity after a spawned order is denied or rejected.
    ///
    /// This is called when a spawned order fails before acceptance. The quantity
    /// that was deducted from the primary order is restored (up to the spawned
    /// order's leaves_qty to handle partial fills).
    fn restore_primary_order_quantity(&mut self, order: &OrderAny) {
        let Some(exec_spawn_id) = order.exec_spawn_id() else {
            return;
        };

        let reduction_qty = {
            let core = self.core_mut();
            core.take_pending_spawn_reduction(&order.client_order_id())
        };

        let Some(reduction_qty) = reduction_qty else {
            return;
        };

        let primary = {
            let cache = self.core_mut().cache();
            cache.order(&exec_spawn_id).cloned()
        };

        let Some(mut primary) = primary else {
            log::warn!(
                "Cannot restore primary order quantity: primary order {exec_spawn_id} not found",
            );
            return;
        };

        // Cap restore amount by leaves_qty to handle partial fills before rejection
        let restore_raw = std::cmp::min(reduction_qty.raw, order.leaves_qty().raw);
        if restore_raw == 0 {
            return;
        }

        let restored_qty = Quantity::from_raw(
            primary.quantity().raw + restore_raw,
            primary.quantity().precision,
        );

        let core = self.core_mut();
        let ts_now = core.clock().timestamp_ns();

        let updated = OrderUpdated::new(
            primary.trader_id(),
            primary.strategy_id(),
            primary.instrument_id(),
            primary.client_order_id(),
            restored_qty,
            UUID4::new(),
            ts_now,
            ts_now,
            false, // reconciliation
            primary.venue_order_id(),
            primary.account_id(),
            None, // price
            None, // trigger_price
            None, // protection_price
        );

        if let Err(e) = primary.apply(OrderEventAny::Updated(updated)) {
            log::warn!("Failed to apply OrderUpdated for quantity restoration: {e}");
            return;
        }

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            if let Err(e) = cache.update_order(&primary) {
                log::warn!("Failed to update primary order in cache: {e}");
                return;
            }
        }

        log::info!(
            "Restored primary order {} quantity to {} after spawned order {} was denied/rejected",
            primary.client_order_id(),
            restored_qty,
            order.client_order_id()
        );
    }

    /// Submits an order to the execution engine via the risk engine.
    ///
    /// # Errors
    ///
    /// Returns an error if order submission fails.
    fn submit_order(
        &mut self,
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let ts_init = core.clock().timestamp_ns();

        // For spawned orders, use the parent's strategy ID
        let strategy_id = order.strategy_id();

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
            None, // params
            UUID4::new(),
            ts_init,
        );

        if core.config.log_commands {
            let id = &core.actor.actor_id;
            log::info!("{id} {SEND}{CMD} {command:?}");
        }

        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_execute(),
            TradingCommand::SubmitOrder(command),
        );

        Ok(())
    }

    /// Modifies an order.
    ///
    /// # Errors
    ///
    /// Returns an error if order modification fails.
    fn modify_order(
        &mut self,
        order: &mut OrderAny,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        let qty_changing = quantity.is_some_and(|q| q != order.quantity());
        let price_changing = price.is_some() && price != order.price();
        let trigger_changing = trigger_price.is_some() && trigger_price != order.trigger_price();

        if !qty_changing && !price_changing && !trigger_changing {
            log::error!(
                "Cannot create command ModifyOrder: \
                quantity, price and trigger were either None \
                or the same as existing values."
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

        let core = self.core_mut();
        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = order.strategy_id();

        if !order.is_active_local() {
            let event = self.generate_order_pending_update(order);
            if let Err(e) = order.apply(OrderEventAny::PendingUpdate(event)) {
                log::warn!("InvalidStateTrigger: {e}, did not apply pending update event");
                return Ok(());
            }

            {
                let cache_rc = self.core_mut().cache_rc();
                let mut cache = cache_rc.borrow_mut();
                cache.update_order(order).ok();
            }

            let topic = format!("events.order.{strategy_id}");
            msgbus::publish_order_event(topic.into(), &OrderEventAny::PendingUpdate(event));
        }

        let ts_init = self.core_mut().clock().timestamp_ns();
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
            None, // params
        );

        if self.core_mut().config.log_commands {
            let id = &self.core_mut().actor.actor_id;
            log::info!("{id} {SEND}{CMD} {command:?}");
        }

        let has_emulation_trigger = order
            .emulation_trigger()
            .is_some_and(|t| t != TriggerType::NoTrigger);

        if order.is_emulated() || has_emulation_trigger {
            msgbus::send_trading_command(
                MessagingSwitchboard::order_emulator_execute(),
                TradingCommand::ModifyOrder(command),
            );
        } else {
            msgbus::send_trading_command(
                MessagingSwitchboard::risk_engine_execute(),
                TradingCommand::ModifyOrder(command),
            );
        }

        Ok(())
    }

    /// Modifies an INITIALIZED or RELEASED order in place without sending a command.
    ///
    /// This is useful for adjusting order parameters before submission. The order
    /// is updated locally by applying an `OrderUpdated` event and updating the cache.
    ///
    /// At least one parameter must differ from the current order values.
    ///
    /// # Errors
    ///
    /// Returns an error if the order status is not INITIALIZED or RELEASED,
    /// or if no parameters would change.
    fn modify_order_in_place(
        &mut self,
        order: &mut OrderAny,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> anyhow::Result<()> {
        // Validate order status
        let status = order.status();
        if status != OrderStatus::Initialized && status != OrderStatus::Released {
            anyhow::bail!(
                "Cannot modify order in place: status is {status:?}, expected INITIALIZED or RELEASED"
            );
        }

        // Validate order type compatibility
        if price.is_some() && order.price().is_none() {
            anyhow::bail!(
                "Cannot modify order in place: {} orders do not have a LIMIT price",
                order.order_type()
            );
        }

        if trigger_price.is_some() && order.trigger_price().is_none() {
            anyhow::bail!(
                "Cannot modify order in place: {} orders do not have a STOP trigger price",
                order.order_type()
            );
        }

        // Check if any value would actually change
        let qty_changing = quantity.is_some_and(|q| q != order.quantity());
        let price_changing = price.is_some() && price != order.price();
        let trigger_changing = trigger_price.is_some() && trigger_price != order.trigger_price();

        if !qty_changing && !price_changing && !trigger_changing {
            anyhow::bail!("Cannot modify order in place: no parameters differ from current values");
        }

        let core = self.core_mut();
        let ts_now = core.clock().timestamp_ns();

        let updated = OrderUpdated::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            quantity.unwrap_or_else(|| order.quantity()),
            UUID4::new(),
            ts_now,
            ts_now,
            false, // reconciliation
            order.venue_order_id(),
            order.account_id(),
            price,
            trigger_price,
            None, // protection_price
        );

        order
            .apply(OrderEventAny::Updated(updated))
            .map_err(|e| anyhow::anyhow!("Failed to apply OrderUpdated: {e}"))?;

        let cache_rc = core.cache_rc();
        let mut cache = cache_rc.borrow_mut();
        cache.update_order(order)?;

        Ok(())
    }

    /// Cancels an order.
    ///
    /// # Errors
    ///
    /// Returns an error if order cancellation fails.
    fn cancel_order(
        &mut self,
        order: &mut OrderAny,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        if order.is_closed() || order.is_pending_cancel() {
            log::warn!(
                "Cannot cancel order: state is {:?}, {order:?}",
                order.status()
            );
            return Ok(());
        }

        let core = self.core_mut();
        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = order.strategy_id();

        if !order.is_active_local() {
            let event = self.generate_order_pending_cancel(order);
            if let Err(e) = order.apply(OrderEventAny::PendingCancel(event)) {
                log::warn!("InvalidStateTrigger: {e}, did not apply pending cancel event");
                return Ok(());
            }

            {
                let cache_rc = self.core_mut().cache_rc();
                let mut cache = cache_rc.borrow_mut();
                cache.update_order(order).ok();
            }

            let topic = format!("events.order.{strategy_id}");
            msgbus::publish_order_event(topic.into(), &OrderEventAny::PendingCancel(event));
        }

        let ts_init = self.core_mut().clock().timestamp_ns();
        let command = CancelOrder::new(
            trader_id,
            client_id,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id(),
            UUID4::new(),
            ts_init,
            None, // params
        );

        if self.core_mut().config.log_commands {
            let id = &self.core_mut().actor.actor_id;
            log::info!("{id} {SEND}{CMD} {command:?}");
        }

        let has_emulation_trigger = order
            .emulation_trigger()
            .is_some_and(|t| t != TriggerType::NoTrigger);

        if order.is_emulated() || order.status() == OrderStatus::Released || has_emulation_trigger {
            msgbus::send_trading_command(
                MessagingSwitchboard::order_emulator_execute(),
                TradingCommand::CancelOrder(command),
            );
        } else {
            msgbus::send_trading_command(
                MessagingSwitchboard::exec_engine_execute(),
                TradingCommand::CancelOrder(command),
            );
        }

        Ok(())
    }

    /// Subscribes to events from a strategy.
    ///
    /// This is called automatically when the first order is received from a strategy.
    fn subscribe_to_strategy_events(&mut self, strategy_id: StrategyId)
    where
        Self: 'static + std::fmt::Debug + Sized,
    {
        let core = self.core_mut();
        if core.is_strategy_subscribed(&strategy_id) {
            return;
        }

        let actor_id = core.actor.actor_id.inner();

        let order_topic = format!("events.order.{strategy_id}");
        let order_actor_id = actor_id;
        let order_handler = TypedHandler::from(move |event: &OrderEventAny| {
            if let Some(mut algo) = try_get_actor_unchecked::<Self>(&order_actor_id) {
                algo.handle_order_event(event.clone());
            } else {
                log::error!(
                    "ExecutionAlgorithm {order_actor_id} not found for order event handling"
                );
            }
        });
        msgbus::subscribe_order_events(order_topic.clone().into(), order_handler.clone(), None);

        let position_topic = format!("events.position.{strategy_id}");
        let position_handler = TypedHandler::from(move |event: &PositionEvent| {
            if let Some(mut algo) = try_get_actor_unchecked::<Self>(&actor_id) {
                algo.handle_position_event(event.clone());
            } else {
                log::error!("ExecutionAlgorithm {actor_id} not found for position event handling");
            }
        });
        msgbus::subscribe_position_events(
            position_topic.clone().into(),
            position_handler.clone(),
            None,
        );

        let handlers = StrategyEventHandlers {
            order_topic,
            order_handler,
            position_topic,
            position_handler,
        };
        core.store_strategy_event_handlers(strategy_id, handlers);

        core.add_subscribed_strategy(strategy_id);
        log::info!("Subscribed to events for strategy {strategy_id}");
    }

    /// Unsubscribes from all strategy event handlers.
    ///
    /// This should be called before reset to properly clean up msgbus subscriptions.
    fn unsubscribe_all_strategy_events(&mut self) {
        let handlers = self.core_mut().take_strategy_event_handlers();
        for (strategy_id, h) in handlers {
            msgbus::unsubscribe_order_events(h.order_topic.into(), &h.order_handler);
            msgbus::unsubscribe_position_events(h.position_topic.into(), &h.position_handler);
            log::info!("Unsubscribed from events for strategy {strategy_id}");
        }
        self.core_mut().clear_subscribed_strategies();
    }

    /// Handles an order event, filtering for algorithm-owned orders.
    fn handle_order_event(&mut self, event: OrderEventAny) {
        if self.core_mut().state() != ComponentState::Running {
            return;
        }

        let order = {
            let cache = self.core_mut().cache();
            cache.order(&event.client_order_id()).cloned()
        };

        let Some(order) = order else {
            return;
        };

        let Some(order_algo_id) = order.exec_algorithm_id() else {
            return;
        };

        if order_algo_id != self.id() {
            return;
        }

        {
            let core = self.core_mut();
            if core.config.log_events {
                let id = &core.actor.actor_id;
                log::info!("{id} {RECV}{EVT} {event}");
            }
        }

        match &event {
            OrderEventAny::Initialized(e) => self.on_order_initialized(e.clone()),
            OrderEventAny::Denied(e) => {
                self.restore_primary_order_quantity(&order);
                self.on_order_denied(*e);
            }
            OrderEventAny::Emulated(e) => self.on_order_emulated(*e),
            OrderEventAny::Released(e) => self.on_order_released(*e),
            OrderEventAny::Submitted(e) => self.on_order_submitted(*e),
            OrderEventAny::Rejected(e) => {
                self.restore_primary_order_quantity(&order);
                self.on_order_rejected(*e);
            }
            OrderEventAny::Accepted(e) => {
                // Commit reduction - order accepted by venue
                self.core_mut()
                    .take_pending_spawn_reduction(&order.client_order_id());
                self.on_order_accepted(*e);
            }
            OrderEventAny::Canceled(e) => {
                self.core_mut()
                    .take_pending_spawn_reduction(&order.client_order_id());
                self.on_algo_order_canceled(*e);
            }
            OrderEventAny::Expired(e) => {
                self.core_mut()
                    .take_pending_spawn_reduction(&order.client_order_id());
                self.on_order_expired(*e);
            }
            OrderEventAny::Triggered(e) => self.on_order_triggered(*e),
            OrderEventAny::PendingUpdate(e) => self.on_order_pending_update(*e),
            OrderEventAny::PendingCancel(e) => self.on_order_pending_cancel(*e),
            OrderEventAny::ModifyRejected(e) => self.on_order_modify_rejected(*e),
            OrderEventAny::CancelRejected(e) => self.on_order_cancel_rejected(*e),
            OrderEventAny::Updated(e) => self.on_order_updated(*e),
            OrderEventAny::Filled(e) => self.on_algo_order_filled(*e),
        }

        self.on_order_event(event);
    }

    /// Handles a position event.
    fn handle_position_event(&mut self, event: PositionEvent) {
        if self.core_mut().state() != ComponentState::Running {
            return;
        }

        {
            let core = self.core_mut();
            if core.config.log_events {
                let id = &core.actor.actor_id;
                log::info!("{id} {RECV}{EVT} {event:?}");
            }
        }

        match &event {
            PositionEvent::PositionOpened(e) => self.on_position_opened(e.clone()),
            PositionEvent::PositionChanged(e) => self.on_position_changed(e.clone()),
            PositionEvent::PositionClosed(e) => self.on_position_closed(e.clone()),
            PositionEvent::PositionAdjusted(_) => {}
        }

        self.on_position_event(event);
    }

    /// Called when the algorithm is started.
    ///
    /// Override this method to implement custom initialization logic.
    ///
    /// # Errors
    ///
    /// Returns an error if start fails.
    fn on_start(&mut self) -> anyhow::Result<()> {
        let id = self.id();
        log::info!("Starting {id}");
        Ok(())
    }

    /// Called when the algorithm is stopped.
    ///
    /// # Errors
    ///
    /// Returns an error if stop fails.
    fn on_stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when the algorithm is reset.
    ///
    /// # Errors
    ///
    /// Returns an error if reset fails.
    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.unsubscribe_all_strategy_events();
        self.core_mut().reset();
        Ok(())
    }

    /// Called when a time event is received.
    ///
    /// Override this method for timer-based algorithms like TWAP.
    ///
    /// # Errors
    ///
    /// Returns an error if time event handling fails.
    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when an order is initialized.
    #[allow(unused_variables)]
    fn on_order_initialized(&mut self, event: OrderInitialized) {}

    /// Called when an order is denied.
    #[allow(unused_variables)]
    fn on_order_denied(&mut self, event: OrderDenied) {}

    /// Called when an order is emulated.
    #[allow(unused_variables)]
    fn on_order_emulated(&mut self, event: OrderEmulated) {}

    /// Called when an order is released from emulation.
    #[allow(unused_variables)]
    fn on_order_released(&mut self, event: OrderReleased) {}

    /// Called when an order is submitted.
    #[allow(unused_variables)]
    fn on_order_submitted(&mut self, event: OrderSubmitted) {}

    /// Called when an order is rejected.
    #[allow(unused_variables)]
    fn on_order_rejected(&mut self, event: OrderRejected) {}

    /// Called when an order is accepted.
    #[allow(unused_variables)]
    fn on_order_accepted(&mut self, event: OrderAccepted) {}

    /// Called when an order is canceled.
    #[allow(unused_variables)]
    fn on_algo_order_canceled(&mut self, event: OrderCanceled) {}

    /// Called when an order expires.
    #[allow(unused_variables)]
    fn on_order_expired(&mut self, event: OrderExpired) {}

    /// Called when an order is triggered.
    #[allow(unused_variables)]
    fn on_order_triggered(&mut self, event: OrderTriggered) {}

    /// Called when an order modification is pending.
    #[allow(unused_variables)]
    fn on_order_pending_update(&mut self, event: OrderPendingUpdate) {}

    /// Called when an order cancellation is pending.
    #[allow(unused_variables)]
    fn on_order_pending_cancel(&mut self, event: OrderPendingCancel) {}

    /// Called when an order modification is rejected.
    #[allow(unused_variables)]
    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {}

    /// Called when an order cancellation is rejected.
    #[allow(unused_variables)]
    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {}

    /// Called when an order is updated.
    #[allow(unused_variables)]
    fn on_order_updated(&mut self, event: OrderUpdated) {}

    /// Called when an order is filled.
    #[allow(unused_variables)]
    fn on_algo_order_filled(&mut self, event: OrderFilled) {}

    /// Called for any order event (after specific handler).
    #[allow(unused_variables)]
    fn on_order_event(&mut self, event: OrderEventAny) {}

    /// Called when a position is opened.
    #[allow(unused_variables)]
    fn on_position_opened(&mut self, event: PositionOpened) {}

    /// Called when a position is changed.
    #[allow(unused_variables)]
    fn on_position_changed(&mut self, event: PositionChanged) {}

    /// Called when a position is closed.
    #[allow(unused_variables)]
    fn on_position_closed(&mut self, event: PositionClosed) {}

    /// Called for any position event (after specific handler).
    #[allow(unused_variables)]
    fn on_position_event(&mut self, event: PositionEvent) {}
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        ops::{Deref, DerefMut},
        rc::Rc,
    };

    use nautilus_common::{
        actor::{DataActor, DataActorCore},
        cache::Cache,
        clock::TestClock,
        component::Component,
        enums::ComponentTrigger,
    };
    use nautilus_model::{
        enums::OrderSide,
        events::{OrderAccepted, OrderCanceled, OrderDenied, OrderRejected},
        identifiers::{
            AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, StrategyId, TraderId,
            VenueOrderId,
        },
        orders::{LimitOrder, MarketOrder, OrderAny, stubs::TestOrderStubs},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[derive(Debug)]
    struct TestAlgorithm {
        core: ExecutionAlgorithmCore,
        on_order_called: bool,
        last_order_client_id: Option<ClientOrderId>,
    }

    impl TestAlgorithm {
        fn new(config: ExecutionAlgorithmConfig) -> Self {
            Self {
                core: ExecutionAlgorithmCore::new(config),
                on_order_called: false,
                last_order_client_id: None,
            }
        }
    }

    impl Deref for TestAlgorithm {
        type Target = DataActorCore;
        fn deref(&self) -> &Self::Target {
            &self.core.actor
        }
    }

    impl DerefMut for TestAlgorithm {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.core.actor
        }
    }

    impl DataActor for TestAlgorithm {}

    impl ExecutionAlgorithm for TestAlgorithm {
        fn core_mut(&mut self) -> &mut ExecutionAlgorithmCore {
            &mut self.core
        }

        fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
            self.on_order_called = true;
            self.last_order_client_id = Some(order.client_order_id());
            Ok(())
        }
    }

    fn create_test_algorithm() -> TestAlgorithm {
        // Use unique ID to avoid thread-local registry/msgbus conflicts in parallel tests
        let unique_id = format!("TEST-{}", UUID4::new());
        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::new(&unique_id)),
            ..Default::default()
        };
        TestAlgorithm::new(config)
    }

    fn register_algorithm(algo: &mut TestAlgorithm) {
        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        algo.core.register(trader_id, clock, cache).unwrap();

        // Transition to Running state for tests
        algo.transition_state(ComponentTrigger::Initialize).unwrap();
        algo.transition_state(ComponentTrigger::Start).unwrap();
        algo.transition_state(ComponentTrigger::StartCompleted)
            .unwrap();
    }

    #[rstest]
    fn test_algorithm_creation() {
        let algo = create_test_algorithm();
        assert!(algo.core.exec_algorithm_id.inner().starts_with("TEST-"));
        assert!(!algo.on_order_called);
        assert!(algo.last_order_client_id.is_none());
    }

    #[rstest]
    fn test_algorithm_registration() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        assert!(algo.core.trader_id().is_some());
        assert_eq!(algo.core.trader_id(), Some(TraderId::from("TRADER-001")));
    }

    #[rstest]
    fn test_algorithm_id() {
        let mut algo = create_test_algorithm();
        assert!(algo.id().inner().starts_with("TEST-"));
    }

    #[rstest]
    fn test_algorithm_spawn_market_creates_valid_order() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false, // reduce_only
            false, // quote_quantity
            None,  // contingency_type
            None,  // order_list_id
            None,  // linked_order_ids
            None,  // parent_order_id
            None,  // exec_algorithm_id
            None,  // exec_algorithm_params
            None,  // exec_spawn_id
            None,  // tags
        ));

        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Ioc,
            false,
            None,  // tags
            false, // reduce_primary
        );

        assert_eq!(spawned.client_order_id.as_str(), "O-001-E1");
        assert_eq!(spawned.instrument_id, instrument_id);
        assert_eq!(spawned.order_side(), OrderSide::Buy);
        assert_eq!(spawned.quantity, Quantity::from("0.5"));
        assert_eq!(spawned.time_in_force, TimeInForce::Ioc);
        assert_eq!(spawned.exec_algorithm_id, Some(algo.id()));
        assert_eq!(spawned.exec_spawn_id, Some(ClientOrderId::from("O-001")));
    }

    #[rstest]
    fn test_algorithm_spawn_increments_sequence() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
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

        let spawned1 = algo.spawn_market(
            &mut primary,
            Quantity::from("0.25"),
            TimeInForce::Ioc,
            false,
            None,
            false,
        );
        let spawned2 = algo.spawn_market(
            &mut primary,
            Quantity::from("0.25"),
            TimeInForce::Ioc,
            false,
            None,
            false,
        );
        let spawned3 = algo.spawn_market(
            &mut primary,
            Quantity::from("0.25"),
            TimeInForce::Ioc,
            false,
            None,
            false,
        );

        assert_eq!(spawned1.client_order_id.as_str(), "O-001-E1");
        assert_eq!(spawned2.client_order_id.as_str(), "O-001-E2");
        assert_eq!(spawned3.client_order_id.as_str(), "O-001-E3");
    }

    #[rstest]
    fn test_algorithm_default_handlers_do_not_panic() {
        let mut algo = create_test_algorithm();

        algo.on_order_initialized(Default::default());
        algo.on_order_denied(Default::default());
        algo.on_order_emulated(Default::default());
        algo.on_order_released(Default::default());
        algo.on_order_submitted(Default::default());
        algo.on_order_rejected(Default::default());
        algo.on_order_accepted(Default::default());
        algo.on_algo_order_canceled(Default::default());
        algo.on_order_expired(Default::default());
        algo.on_order_triggered(Default::default());
        algo.on_order_pending_update(Default::default());
        algo.on_order_pending_cancel(Default::default());
        algo.on_order_modify_rejected(Default::default());
        algo.on_order_cancel_rejected(Default::default());
        algo.on_order_updated(Default::default());
        algo.on_algo_order_filled(Default::default());
    }

    #[rstest]
    fn test_strategy_subscription_tracking() {
        let mut algo = create_test_algorithm();
        let strategy_id = StrategyId::from("STRAT-001");

        assert!(!algo.core.is_strategy_subscribed(&strategy_id));

        algo.subscribe_to_strategy_events(strategy_id);
        assert!(algo.core.is_strategy_subscribed(&strategy_id));

        // Second call should be idempotent
        algo.subscribe_to_strategy_events(strategy_id);
        assert!(algo.core.is_strategy_subscribed(&strategy_id));
    }

    #[rstest]
    fn test_algorithm_reset() {
        let mut algo = create_test_algorithm();
        let strategy_id = StrategyId::from("STRAT-001");
        let primary_id = ClientOrderId::new("O-001");

        let _ = algo.core.spawn_client_order_id(&primary_id);
        algo.core.add_subscribed_strategy(strategy_id);

        assert!(algo.core.spawn_sequence(&primary_id).is_some());
        assert!(algo.core.is_strategy_subscribed(&strategy_id));

        ExecutionAlgorithm::on_reset(&mut algo).unwrap();

        assert!(algo.core.spawn_sequence(&primary_id).is_none());
        assert!(!algo.core.is_strategy_subscribed(&strategy_id));
    }

    #[rstest]
    fn test_algorithm_spawn_limit_creates_valid_order() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
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

        let price = Price::from("50000.0");
        let spawned = algo.spawn_limit(
            &mut primary,
            Quantity::from("0.5"),
            price,
            TimeInForce::Gtc,
            None,  // expire_time
            false, // post_only
            false, // reduce_only
            None,  // display_qty
            None,  // emulation_trigger
            None,  // tags
            false, // reduce_primary
        );

        assert_eq!(spawned.client_order_id.as_str(), "O-001-E1");
        assert_eq!(spawned.instrument_id, instrument_id);
        assert_eq!(spawned.order_side(), OrderSide::Buy);
        assert_eq!(spawned.quantity, Quantity::from("0.5"));
        assert_eq!(spawned.price, price);
        assert_eq!(spawned.time_in_force, TimeInForce::Gtc);
        assert_eq!(spawned.exec_algorithm_id, Some(algo.id()));
        assert_eq!(spawned.exec_spawn_id, Some(ClientOrderId::from("O-001")));
    }

    #[rstest]
    fn test_algorithm_spawn_market_to_limit_creates_valid_order() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
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

        let spawned = algo.spawn_market_to_limit(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Gtc,
            None,  // expire_time
            false, // reduce_only
            None,  // display_qty
            None,  // emulation_trigger
            None,  // tags
            false, // reduce_primary
        );

        assert_eq!(spawned.client_order_id.as_str(), "O-001-E1");
        assert_eq!(spawned.instrument_id, instrument_id);
        assert_eq!(spawned.order_side(), OrderSide::Buy);
        assert_eq!(spawned.quantity, Quantity::from("0.5"));
        assert_eq!(spawned.time_in_force, TimeInForce::Gtc);
        assert_eq!(spawned.exec_algorithm_id, Some(algo.id()));
        assert_eq!(spawned.exec_spawn_id, Some(ClientOrderId::from("O-001")));
    }

    #[rstest]
    fn test_algorithm_spawn_market_with_tags() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
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

        let tags = vec![ustr::Ustr::from("TAG1"), ustr::Ustr::from("TAG2")];
        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Ioc,
            false,
            Some(tags.clone()),
            false,
        );

        assert_eq!(spawned.tags, Some(tags));
    }

    #[rstest]
    fn test_algorithm_reduce_primary_order() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
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

        // Make accepted so OrderUpdated can be applied
        let mut primary = TestOrderStubs::make_accepted_order(&order);

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let spawn_qty = Quantity::from("0.3");
        algo.reduce_primary_order(&mut primary, spawn_qty);

        assert_eq!(primary.quantity(), Quantity::from("0.7"));
    }

    #[rstest]
    fn test_algorithm_spawn_market_with_reduce_primary() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
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

        // Make accepted so OrderUpdated can be applied
        let mut primary = TestOrderStubs::make_accepted_order(&order);

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.4"),
            TimeInForce::Ioc,
            false,
            None,
            true, // reduce_primary = true
        );

        assert_eq!(spawned.quantity, Quantity::from("0.4"));
        assert_eq!(primary.quantity(), Quantity::from("0.6"));
    }

    #[rstest]
    fn test_algorithm_generate_order_canceled() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
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

        let event = algo.generate_order_canceled(&order);

        assert_eq!(event.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(event.strategy_id, StrategyId::from("STRAT-001"));
        assert_eq!(event.instrument_id, InstrumentId::from("BTC/USDT.BINANCE"));
        assert_eq!(event.client_order_id, ClientOrderId::from("O-001"));
    }

    #[rstest]
    fn test_algorithm_modify_order_in_place_updates_quantity() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let mut order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("50000.0"),
            TimeInForce::Gtc,
            None,  // expire_time
            false, // post_only
            false, // reduce_only
            false, // quote_quantity
            None,  // display_qty
            None,  // emulation_trigger
            None,  // trigger_instrument_id
            None,  // contingency_type
            None,  // order_list_id
            None,  // linked_order_ids
            None,  // parent_order_id
            None,  // exec_algorithm_id
            None,  // exec_algorithm_params
            None,  // exec_spawn_id
            None,  // tags
            UUID4::new(),
            0.into(),
        ));

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order.clone(), None, None, false).unwrap();
        }

        let new_qty = Quantity::from("0.5");
        algo.modify_order_in_place(&mut order, Some(new_qty), None, None)
            .unwrap();

        assert_eq!(order.quantity(), new_qty);
    }

    #[rstest]
    fn test_algorithm_modify_order_in_place_rejects_no_changes() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let mut order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-001"),
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
            0.into(),
        ));

        // Try to modify with same quantity - should fail
        let result =
            algo.modify_order_in_place(&mut order, Some(Quantity::from("1.0")), None, None);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no parameters differ")
        );
    }

    #[rstest]
    fn test_spawned_order_denied_restores_primary_quantity() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(exec_algorithm_id),
            None,
            None,
            None,
        ));

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&primary).unwrap();
        }

        assert_eq!(primary.quantity(), Quantity::from("0.5"));

        let mut spawned_order = OrderAny::Market(spawned);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(spawned_order.clone(), None, None, false)
                .unwrap();
        }

        let denied = OrderDenied::new(
            spawned_order.trader_id(),
            spawned_order.strategy_id(),
            spawned_order.instrument_id(),
            spawned_order.client_order_id(),
            "TEST_DENIAL".into(),
            UUID4::new(),
            0.into(),
            0.into(),
        );

        spawned_order.apply(OrderEventAny::Denied(denied)).unwrap();
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&spawned_order).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Denied(denied));

        let restored_primary = {
            let cache = algo.core.cache();
            cache.order(&ClientOrderId::from("O-001")).cloned().unwrap()
        };
        assert_eq!(restored_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_spawned_order_rejected_restores_primary_quantity() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(exec_algorithm_id),
            None,
            None,
            None,
        ));

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&primary).unwrap();
        }

        assert_eq!(primary.quantity(), Quantity::from("0.5"));

        let mut spawned_order = OrderAny::Market(spawned);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(spawned_order.clone(), None, None, false)
                .unwrap();
        }

        let rejected = OrderRejected::new(
            spawned_order.trader_id(),
            spawned_order.strategy_id(),
            spawned_order.instrument_id(),
            spawned_order.client_order_id(),
            AccountId::from("BINANCE-001"),
            "TEST_REJECTION".into(),
            UUID4::new(),
            0.into(),
            0.into(),
            false,
            false,
        );

        spawned_order
            .apply(OrderEventAny::Rejected(rejected))
            .unwrap();
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&spawned_order).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Rejected(rejected));

        let restored_primary = {
            let cache = algo.core.cache();
            cache.order(&ClientOrderId::from("O-001")).cloned().unwrap()
        };
        assert_eq!(restored_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_spawned_order_with_reduce_primary_false_does_not_restore() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(exec_algorithm_id),
            None,
            None,
            None,
        ));

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Fok,
            false,
            None,
            false,
        );

        assert_eq!(primary.quantity(), Quantity::from("1.0"));

        let mut spawned_order = OrderAny::Market(spawned);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(spawned_order.clone(), None, None, false)
                .unwrap();
        }

        let denied = OrderDenied::new(
            spawned_order.trader_id(),
            spawned_order.strategy_id(),
            spawned_order.instrument_id(),
            spawned_order.client_order_id(),
            "TEST_DENIAL".into(),
            UUID4::new(),
            0.into(),
            0.into(),
        );

        spawned_order.apply(OrderEventAny::Denied(denied)).unwrap();
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&spawned_order).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Denied(denied));

        let final_primary = {
            let cache = algo.core.cache();
            cache.order(&ClientOrderId::from("O-001")).cloned().unwrap()
        };
        assert_eq!(final_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_multiple_spawns_with_one_denied_restores_correctly() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(exec_algorithm_id),
            None,
            None,
            None,
        ));

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let spawned1 = algo.spawn_market(
            &mut primary,
            Quantity::from("0.3"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&primary).unwrap();
        }

        let spawned2 = algo.spawn_market(
            &mut primary,
            Quantity::from("0.4"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&primary).unwrap();
        }

        assert_eq!(primary.quantity(), Quantity::from("0.3"));

        let spawned_order1 = OrderAny::Market(spawned1);
        let mut spawned_order2 = OrderAny::Market(spawned2);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(spawned_order1, None, None, false).unwrap();
            cache
                .add_order(spawned_order2.clone(), None, None, false)
                .unwrap();
        }

        let denied = OrderDenied::new(
            spawned_order2.trader_id(),
            spawned_order2.strategy_id(),
            spawned_order2.instrument_id(),
            spawned_order2.client_order_id(),
            "TEST_DENIAL".into(),
            UUID4::new(),
            0.into(),
            0.into(),
        );

        spawned_order2.apply(OrderEventAny::Denied(denied)).unwrap();
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&spawned_order2).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Denied(denied));

        let restored_primary = {
            let cache = algo.core.cache();
            cache.order(&ClientOrderId::from("O-001")).cloned().unwrap()
        };
        assert_eq!(restored_primary.quantity(), Quantity::from("0.7"));
    }

    #[rstest]
    fn test_spawned_order_accepted_prevents_restoration() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(exec_algorithm_id),
            None,
            None,
            None,
        ));

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&primary).unwrap();
        }

        assert_eq!(primary.quantity(), Quantity::from("0.5"));

        let mut spawned_order = OrderAny::Market(spawned);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(spawned_order.clone(), None, None, false)
                .unwrap();
        }

        let accepted = OrderAccepted::new(
            spawned_order.trader_id(),
            spawned_order.strategy_id(),
            spawned_order.instrument_id(),
            spawned_order.client_order_id(),
            VenueOrderId::from("V-123"),
            AccountId::from("BINANCE-001"),
            UUID4::new(),
            0.into(),
            0.into(),
            false,
        );

        spawned_order
            .apply(OrderEventAny::Accepted(accepted))
            .unwrap();
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&spawned_order).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Accepted(accepted));

        let primary_after_accept = {
            let cache = algo.core.cache();
            cache.order(&ClientOrderId::from("O-001")).cloned().unwrap()
        };
        assert_eq!(primary_after_accept.quantity(), Quantity::from("0.5"));

        // Cancel after acceptance - no restoration should occur
        let canceled = OrderCanceled::new(
            spawned_order.trader_id(),
            spawned_order.strategy_id(),
            spawned_order.instrument_id(),
            spawned_order.client_order_id(),
            UUID4::new(),
            0.into(),
            0.into(),
            false,
            Some(VenueOrderId::from("V-123")),
            Some(AccountId::from("BINANCE-001")),
        );

        spawned_order
            .apply(OrderEventAny::Canceled(canceled))
            .unwrap();
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&spawned_order).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Canceled(canceled));

        let final_primary = {
            let cache = algo.core.cache();
            cache.order(&ClientOrderId::from("O-001")).cloned().unwrap()
        };
        assert_eq!(final_primary.quantity(), Quantity::from("0.5"));
    }

    #[rstest]
    #[should_panic(expected = "exceeds primary leaves_qty")]
    fn test_spawn_quantity_exceeds_leaves_qty_panics() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            false,
            None,
            None,
            None,
            None,
            Some(exec_algorithm_id),
            None,
            None,
            None,
        ));

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let _ = algo.spawn_market(
            &mut primary,
            Quantity::from("0.8"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&primary).unwrap();
        }

        assert_eq!(primary.quantity(), Quantity::from("0.2"));
        assert_eq!(primary.leaves_qty(), Quantity::from("0.2"));

        // Should panic - spawning 0.5 when only 0.2 leaves_qty remains
        let _ = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );
    }
}
