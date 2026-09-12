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
//! 5. Spawned orders are submitted through the `RiskEngine`.
//! 6. The algorithm receives fill events and manages remaining quantity.

use std::fmt::Display;

pub mod config;
pub mod core;
pub mod twap;

pub use core::{ExecutionAlgorithmCore, ExecutionAlgorithmNative, StrategyEventHandlers};

pub use config::{ExecutionAlgorithmConfig, ImportableExecutionAlgorithmConfig};
use nautilus_common::{
    actor::{DataActor, DataActorNative, registry::try_get_actor_unchecked},
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
        OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSubmitted, OrderTriggered, OrderUpdated, PositionChanged, PositionClosed,
        PositionEvent, PositionOpened,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, PositionId, StrategyId, TraderId,
    },
    orders::{LimitOrder, MarketOrder, MarketToLimitOrder, Order, OrderAny, OrderError, OrderList},
    types::{Price, Quantity, quantity::QuantityRaw},
};
pub use twap::{TwapAlgorithm, TwapAlgorithmConfig};
use ustr::Ustr;

use crate::algorithm::core::SpawnReduction;

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
/// When a reduced spawned order terminates with unfilled quantity, its reduction
/// is restored in primary quantity units while the primary remains locally
/// mutable. Submission handoff permanently ends restoration and late-fill
/// re-deduction. A caller-held primary value remains reduced and must be
/// discarded or refreshed from the cache before reuse.
///
/// # Implementation
///
/// Use the `nautilus_execution_algorithm!` macro to generate the native runtime
/// wiring and `ExecutionAlgorithm` implementation, including the required
/// `on_order()` method. Normal execution algorithm logic should call facade
/// methods such as `submit_order()`, `spawn_market()`, and
/// `unsubscribe_all_strategy_events()`. Native runtime code that needs the
/// internal core should use [`ExecutionAlgorithmNative`].
pub trait ExecutionAlgorithm: DataActor {
    /// Returns the execution algorithm ID.
    fn id(&self) -> ExecAlgorithmId
    where
        Self: ExecutionAlgorithmNative,
    {
        ExecutionAlgorithmNative::exec_algorithm_core(self).exec_algorithm_id
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
        Self: ExecutionAlgorithmNative,
        Self: 'static + std::fmt::Debug + Sized,
    {
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        if core.config.log_commands {
            let id = &core.actor.actor_id;
            log::info!("{id} {RECV}{CMD} {command}");
        }

        if DataActorNative::core(core).state() != ComponentState::Running {
            return Ok(());
        }

        match command {
            TradingCommand::SubmitOrder(cmd) => {
                self.subscribe_to_strategy_events(cmd.strategy_id);
                let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
                core.remember_submit_params(cmd.client_order_id, cmd.params.clone());
                let order = core.get_order(&cmd.client_order_id)?;
                self.on_order(order)
            }
            TradingCommand::SubmitOrderList(cmd) => {
                self.subscribe_to_strategy_events(cmd.strategy_id);
                let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
                for client_order_id in &cmd.order_list.client_order_ids {
                    core.remember_submit_params(*client_order_id, cmd.params.clone());
                }
                let orders = core.get_orders_for_list(&cmd.order_list)?;
                self.on_order_list(cmd.order_list, orders)
            }
            TradingCommand::ModifyOrder(cmd) => self.handle_modify_order(cmd),
            TradingCommand::CancelOrder(cmd) => self.handle_cancel_order(cmd),
            _ => {
                log::warn!("Unhandled command type: {command}");
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
    fn on_order_list(
        &mut self,
        _order_list: OrderList,
        orders: Vec<OrderAny>,
    ) -> anyhow::Result<()> {
        for order in orders {
            self.on_order(order)?;
        }
        Ok(())
    }

    /// Denies an order by applying and publishing an `OrderDenied` event.
    ///
    /// An order absent from the cache is added first, with its `OrderInitialized` event published
    /// before the denial. A closed cached order is left unchanged. Use an `OrderDeniedReason`
    /// string for the standardized reason.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The algorithm is not registered with a trader.
    /// - The order cannot be added to the cache.
    /// - The denial cannot be applied, including an invalid order state transition.
    ///
    /// No event is published when the denial cannot be applied.
    fn deny_order(&mut self, order: &OrderAny, reason: Ustr) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        registered_trader_id(core)?;
        let ts_now = core.clock_mut().timestamp_ns();
        let event = OrderEventAny::Denied(OrderDenied::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
        ));

        let publish_initialized = {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();

            if cache
                .order(&order.client_order_id())
                .is_some_and(|cached_order| cached_order.is_closed())
            {
                return Ok(());
            }

            let publish_initialized = if cache.order_exists(&order.client_order_id()) {
                false
            } else {
                cache.add_order(order.clone(), None, None, false)?;
                true
            };

            cache.update_order(&event)?;
            publish_initialized
        };

        if publish_initialized {
            publish_order_initialized(order);
        }
        publish_order_event(&event);

        // A denied order never executes, so its stored submit params are dropped here
        // rather than waiting for an execution completion that will never arrive.
        ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .remove_submit_params(&order.client_order_id());

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
    fn handle_cancel_order(&mut self, command: CancelOrder) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
        let (order, is_pending_cancel) = {
            let cache = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_ref();

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
            log::warn!("Order already closed for {command}");
            return Ok(());
        }

        let event = OrderEventAny::Canceled(self.generate_order_canceled(&order));

        let order = {
            let cache_rc = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_rc();
            let mut cache = cache_rc.borrow_mut();
            match cache.update_order(&event) {
                Ok(order) => order,
                Err(e)
                    if matches!(
                        e.downcast_ref::<OrderError>(),
                        Some(OrderError::InvalidStateTransition)
                    ) =>
                {
                    log::warn!("InvalidStateTrigger: {e}, did not apply cancel event");
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        };

        let topic = format!("events.order.{}", order.strategy_id());
        msgbus::publish_order_event(topic.into(), &event);
        msgbus::publish_order_event(
            msgbus::switchboard::get_order_canceled_topic(order.instrument_id()),
            &event,
        );

        Ok(())
    }

    /// Handles a modify order command for algorithm-managed orders.
    ///
    /// Active-local orders are left unchanged because the algorithm owns their execution state.
    ///
    /// # Errors
    ///
    /// Returns an error if command handling fails.
    fn handle_modify_order(&mut self, command: ModifyOrder) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
        let (is_closed, is_active_local) = {
            let cache = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_ref();

            let Some(order) = cache.order(&command.client_order_id) else {
                log::warn!(
                    "Cannot modify order: {} not found in cache",
                    command.client_order_id
                );
                return Ok(());
            };

            (order.is_closed(), order.is_active_local())
        };

        if is_closed {
            log::warn!("Order already closed for {command}");
            return Ok(());
        }

        if is_active_local {
            log::warn!(
                "Cannot modify {}: order is being executed by this algorithm",
                command.client_order_id
            );
            return Ok(());
        }

        // A venue-active order is routed to the execution path, not here
        log::warn!(
            "Cannot modify {}: order is not active-local",
            command.client_order_id
        );
        Ok(())
    }

    /// Generates an `OrderCanceled` event for an order.
    fn generate_order_canceled(&mut self, order: &OrderAny) -> OrderCanceled
    where
        Self: ExecutionAlgorithmNative,
    {
        let ts_now = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .clock_mut()
            .timestamp_ns();

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
            None,
        )
    }

    /// Generates an `OrderPendingUpdate` event for an order.
    fn generate_order_pending_update(&mut self, order: &OrderAny) -> OrderPendingUpdate
    where
        Self: ExecutionAlgorithmNative,
    {
        let ts_now = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
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
            false, // reconciliation
            order.venue_order_id(),
        )
    }

    /// Generates an `OrderPendingCancel` event for an order.
    fn generate_order_pending_cancel(&mut self, order: &OrderAny) -> OrderPendingCancel
    where
        Self: ExecutionAlgorithmNative,
    {
        let ts_now = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
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
            false, // reconciliation
            order.venue_order_id(),
        )
    }

    /// Spawns a market order from a primary order.
    ///
    /// Creates a new market order with:
    /// - A unique client order ID: `{primary_id}-E{sequence}`.
    /// - The primary order's trader ID, strategy ID, and instrument ID.
    /// - The algorithm's `exec_algorithm_id`.
    /// - `exec_spawn_id` set to the primary order's client order ID.
    ///
    /// If `reduce_primary` is true, the primary order's quantity is reduced by
    /// the spawned quantity. Unfilled quantity is restored when the spawn is
    /// denied, rejected, canceled, expired, or refused before submission while
    /// the primary remains locally mutable. Converted quote-quantity spawns are
    /// restored proportionally in primary units. Late fills re-deduct restored
    /// quantity until primary submission is handed off.
    fn spawn_market(
        &mut self,
        primary: &mut OrderAny,
        quantity: Quantity,
        time_in_force: TimeInForce,
        reduce_only: bool,
        tags: Option<Vec<Ustr>>,
        reduce_primary: bool,
    ) -> MarketOrder
    where
        Self: ExecutionAlgorithmNative,
    {
        // Generate spawn ID first so we can track the reduction
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let client_order_id = core.spawn_client_order_id(&primary.client_order_id());
        let ts_init = core.clock_mut().timestamp_ns();
        let exec_algorithm_id = core.exec_algorithm_id;

        if reduce_primary {
            self.reduce_primary_order(primary, quantity);
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self).track_pending_spawn_reduction(
                client_order_id,
                primary.client_order_id(),
                quantity,
                primary.is_quote_quantity(),
            );
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
            primary.is_quote_quantity(),
            primary.contingency_type(),
            primary.order_list_id(),
            primary.linked_order_ids().map(<[ClientOrderId]>::to_vec),
            primary.parent_order_id(),
            Some(exec_algorithm_id),
            primary.exec_algorithm_params().cloned(),
            Some(primary.client_order_id()),
            tags.or_else(|| primary.tags().map(<[Ustr]>::to_vec)),
        )
    }

    /// Spawns a limit order from a primary order.
    ///
    /// Creates a new limit order with:
    /// - A unique client order ID: `{primary_id}-E{sequence}`
    /// - The primary order's trader ID, strategy ID, and instrument ID
    /// - The algorithm's `exec_algorithm_id`
    /// - `exec_spawn_id` set to the primary order's client order ID
    ///
    /// `submit_order` refuses the returned order when `emulation_trigger` is
    /// `Some`; use `None` for an order that the execution algorithm will submit.
    ///
    /// If `reduce_primary` is true, the primary order's quantity is reduced by
    /// the spawned quantity. Unfilled quantity is restored when the spawn is
    /// denied, rejected, canceled, expired, or refused before submission while
    /// the primary remains locally mutable. Converted quote-quantity spawns are
    /// restored proportionally in primary units. Late fills re-deduct restored
    /// quantity until primary submission is handed off.
    #[expect(clippy::too_many_arguments)]
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
    ) -> LimitOrder
    where
        Self: ExecutionAlgorithmNative,
    {
        // Generate spawn ID first so we can track the reduction
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let client_order_id = core.spawn_client_order_id(&primary.client_order_id());
        let ts_init = core.clock_mut().timestamp_ns();
        let exec_algorithm_id = core.exec_algorithm_id;

        if reduce_primary {
            self.reduce_primary_order(primary, quantity);
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self).track_pending_spawn_reduction(
                client_order_id,
                primary.client_order_id(),
                quantity,
                primary.is_quote_quantity(),
            );
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
            primary.is_quote_quantity(),
            display_qty,
            emulation_trigger,
            None, // trigger_instrument_id
            primary.contingency_type(),
            primary.order_list_id(),
            primary.linked_order_ids().map(<[ClientOrderId]>::to_vec),
            primary.parent_order_id(),
            Some(exec_algorithm_id),
            primary.exec_algorithm_params().cloned(),
            Some(primary.client_order_id()),
            tags.or_else(|| primary.tags().map(<[Ustr]>::to_vec)),
            UUID4::new(),
            ts_init,
        )
    }

    /// Spawns a market-to-limit order from a primary order.
    ///
    /// Creates a new market-to-limit order with:
    /// - A unique client order ID: `{primary_id}-E{sequence}`
    /// - The primary order's trader ID, strategy ID, and instrument ID
    /// - The algorithm's `exec_algorithm_id`
    /// - `exec_spawn_id` set to the primary order's client order ID
    ///
    /// If `reduce_primary` is true, the primary order's quantity is reduced by
    /// the spawned quantity. Unfilled quantity is restored when the spawn is
    /// denied, rejected, canceled, expired, or refused before submission while
    /// the primary remains locally mutable. Converted quote-quantity spawns are
    /// restored proportionally in primary units. Late fills re-deduct restored
    /// quantity until primary submission is handed off.
    ///
    /// `_emulation_trigger` is accepted for signature parity and is not applied:
    /// a `MARKET_TO_LIMIT` order is always initialized with no emulation trigger
    /// and cannot be emulated.
    #[expect(clippy::too_many_arguments)]
    fn spawn_market_to_limit(
        &mut self,
        primary: &mut OrderAny,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        reduce_only: bool,
        display_qty: Option<Quantity>,
        _emulation_trigger: Option<TriggerType>,
        tags: Option<Vec<Ustr>>,
        reduce_primary: bool,
    ) -> MarketToLimitOrder
    where
        Self: ExecutionAlgorithmNative,
    {
        // Generate spawn ID first so we can track the reduction
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let client_order_id = core.spawn_client_order_id(&primary.client_order_id());
        let ts_init = core.clock_mut().timestamp_ns();
        let exec_algorithm_id = core.exec_algorithm_id;

        if reduce_primary {
            self.reduce_primary_order(primary, quantity);
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self).track_pending_spawn_reduction(
                client_order_id,
                primary.client_order_id(),
                quantity,
                primary.is_quote_quantity(),
            );
        }

        MarketToLimitOrder::new(
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
            primary.is_quote_quantity(),
            display_qty,
            primary.contingency_type(),
            primary.order_list_id(),
            primary.linked_order_ids().map(<[ClientOrderId]>::to_vec),
            primary.parent_order_id(),
            Some(exec_algorithm_id),
            primary.exec_algorithm_params().cloned(),
            Some(primary.client_order_id()),
            tags.or_else(|| primary.tags().map(<[Ustr]>::to_vec)),
            UUID4::new(),
            ts_init,
        )
    }

    /// Reduces the primary order's quantity by the spawn quantity.
    ///
    /// Generates an `OrderUpdated` event and applies it to the primary order,
    /// then updates the order in the cache.
    ///
    /// # Panics
    ///
    /// Panics if `spawn_qty` exceeds the primary order's `leaves_qty`.
    fn reduce_primary_order(&mut self, primary: &mut OrderAny, spawn_qty: Quantity)
    where
        Self: ExecutionAlgorithmNative,
    {
        let leaves_qty = primary.leaves_qty();
        assert!(
            leaves_qty >= spawn_qty,
            "Spawn quantity {spawn_qty} exceeds primary leaves_qty {leaves_qty}"
        );

        let primary_qty = primary.quantity();
        let mut new_qty = primary_qty - spawn_qty;
        new_qty.precision = primary_qty.precision;

        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let ts_now = core.clock_mut().timestamp_ns();

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
            primary.is_quote_quantity(),
        );

        let event = OrderEventAny::Updated(updated);

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            *primary = cache
                .update_order(&event)
                .expect("Failed to update order in cache");
        }

        publish_order_event(&event);
    }

    /// Restores a spawn reduction while the cached primary order remains local.
    ///
    /// The quantity deducted from the cached primary order is restored up to the
    /// spawned order's unfilled proportion in primary units. Tracked fill voids
    /// return only the additional budget released by the correction. Primaries handed
    /// off for submission retain their committed quantity. Uncompensated
    /// late-fill debt on the primary is discharged before quantity is returned.
    ///
    /// `refused_before_submission` selects whether the restoration log records a
    /// refusal or an order update.
    fn restore_primary_order_quantity(&mut self, order: &OrderAny, refused_before_submission: bool)
    where
        Self: ExecutionAlgorithmNative,
    {
        let Some(exec_spawn_id) = order.exec_spawn_id() else {
            return;
        };

        let reduction = {
            let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
            core.spawn_reduction(order.client_order_id())
        };

        let Some(mut reduction) = reduction else {
            return;
        };

        let primary = {
            let cache = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_ref();
            cache
                .order(&exec_spawn_id)
                .map(|o| <OrderAny as Clone>::clone(&o))
        };

        let Some(primary) = primary else {
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .take_pending_spawn_reduction(order.client_order_id());
            log::warn!(
                "Cannot restore primary order quantity: primary order {exec_spawn_id} not found",
            );
            return;
        };

        let handed_off = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .primary_was_handed_off(exec_spawn_id);

        if !primary.is_active_local() || handed_off {
            let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
            core.take_pending_spawn_reduction(order.client_order_id());
            core.discard_spawn_fill_debt(exec_spawn_id);
            log::info!(
                "Skipped restoring primary order {exec_spawn_id} after spawned order {}: primary is no longer locally mutable",
                order.client_order_id(),
            );
            return;
        }

        let Some(unfilled_qty) =
            spawn_unfilled_quantity(order, reduction, primary.quantity().precision)
        else {
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .take_pending_spawn_reduction(order.client_order_id());
            return;
        };
        let restored_qty = reduction
            .restored_qty
            .unwrap_or_else(|| Quantity::zero(unfilled_qty.precision));
        let restore_qty = unfilled_qty.saturating_sub(restored_qty);
        reduction.restored_qty = Some(unfilled_qty);

        if restore_qty.is_zero() {
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .set_spawn_reduction(order.client_order_id(), reduction);
            return;
        }

        // Discharge uncompensated late-fill debt on this primary before
        // returning quantity to it
        let debt_qty = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .spawn_fill_debt(exec_spawn_id)
            .unwrap_or_else(|| Quantity::zero(restore_qty.precision));
        let discharge_qty = restore_qty.min(debt_qty);
        let net_restore_qty = restore_qty - discharge_qty;

        if net_restore_qty.is_zero() {
            // The whole restoration discharged debt: keep the record with the
            // gross released amount so this child's own late fills stay tracked
            let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
            core.set_spawn_reduction(order.client_order_id(), reduction);
            core.set_spawn_fill_debt(exec_spawn_id, debt_qty - discharge_qty);
            log::info!(
                "Restoration from spawned order {} fully discharged late-fill debt on primary order {exec_spawn_id}",
                order.client_order_id(),
            );
            return;
        }

        let mut restored_qty = primary.quantity() + net_restore_qty;
        restored_qty.precision = primary.quantity().precision;

        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let ts_now = core.clock_mut().timestamp_ns();

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
            primary.is_quote_quantity(),
        );

        let event = OrderEventAny::Updated(updated);

        let primary = {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            match cache.update_order(&event) {
                Ok(primary) => primary,
                Err(e) => {
                    log::warn!("Failed to update primary order in cache: {e}");
                    return;
                }
            }
        };

        // Commit the lifecycle record and debt before publishing: subscribers
        // run synchronously and may re-enter order handling. The record keeps
        // the gross released amount (including the debt-discharged portion) as
        // this child's late-fill accounting budget; only the net reaches the
        // primary.
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        core.set_spawn_reduction(order.client_order_id(), reduction);
        if !discharge_qty.is_zero() {
            core.set_spawn_fill_debt(exec_spawn_id, debt_qty - discharge_qty);
        }

        publish_order_event(&event);

        let outcome = if refused_before_submission {
            "refused before submission"
        } else {
            "updated with unfilled quantity"
        };
        log::info!(
            "Restored primary order {} quantity to {} after spawned order {} was {outcome}",
            primary.client_order_id(),
            restored_qty,
            order.client_order_id()
        );
    }

    /// Re-deducts a late spawn fill from a previously restored local primary order.
    fn rededuct_late_spawn_fill(&mut self, order: &OrderAny)
    where
        Self: ExecutionAlgorithmNative,
    {
        let spawn_id = order.client_order_id();
        let Some(exec_spawn_id) = order.exec_spawn_id() else {
            return;
        };
        let Some(reduction) =
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self).spawn_reduction(spawn_id)
        else {
            return;
        };

        let Some(restored_qty) = reduction.restored_qty else {
            return;
        };

        let primary = {
            let cache = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_ref();
            cache
                .order(&exec_spawn_id)
                .map(|o| <OrderAny as Clone>::clone(&o))
        };
        let Some(primary) = primary else {
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .take_pending_spawn_reduction(spawn_id);
            log::warn!(
                "Cannot re-deduct late fill from primary order {exec_spawn_id}: order not found",
            );
            return;
        };

        let handed_off = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .primary_was_handed_off(exec_spawn_id);

        if !primary.is_active_local() || handed_off {
            let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
            core.take_pending_spawn_reduction(spawn_id);
            core.discard_spawn_fill_debt(exec_spawn_id);
            log::info!(
                "Skipped re-deducting late fill from primary order {exec_spawn_id}: primary is no longer locally mutable",
            );
            return;
        }

        let Some(unfilled_qty) =
            spawn_unfilled_quantity(order, reduction, primary.quantity().precision)
        else {
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .take_pending_spawn_reduction(spawn_id);
            return;
        };
        let uncapped_qty = restored_qty.saturating_sub(unfilled_qty);
        let primary_qty = primary.quantity();
        // Restored quantity may already have been reused by a later spawn, so
        // cap at the primary's remaining quantity; the shortfall becomes debt
        // discharged against later spawn restorations for this primary.
        let rededuct_qty = uncapped_qty.min(primary_qty);
        let shortfall_qty = uncapped_qty - rededuct_qty;
        if rededuct_qty.is_zero() {
            if !uncapped_qty.is_zero() {
                charge_spawn_reduction(
                    ExecutionAlgorithmNative::exec_algorithm_core_mut(self),
                    spawn_id,
                    exec_spawn_id,
                    reduction,
                    unfilled_qty,
                    shortfall_qty,
                );
                log::warn!(
                    "Cannot re-deduct late fill on spawned order {spawn_id} from primary order {exec_spawn_id}: primary quantity exhausted, shortfall recorded as debt",
                );
            }
            return;
        }
        let mut new_qty = primary_qty - rededuct_qty;
        new_qty.precision = primary_qty.precision;
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let ts_now = core.clock_mut().timestamp_ns();
        let event = OrderEventAny::Updated(OrderUpdated::new(
            primary.trader_id(),
            primary.strategy_id(),
            primary.instrument_id(),
            primary.client_order_id(),
            new_qty,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            primary.venue_order_id(),
            primary.account_id(),
            None,
            None,
            None,
            primary.is_quote_quantity(),
        ));

        if let Err(e) = core.cache_rc().borrow_mut().update_order(&event) {
            log::warn!("Failed to update primary order in cache: {e}");
            return;
        }

        // Commit the lifecycle record before publishing: subscribers run
        // synchronously and may re-enter order handling.
        charge_spawn_reduction(
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self),
            spawn_id,
            exec_spawn_id,
            reduction,
            unfilled_qty,
            shortfall_qty,
        );

        if !shortfall_qty.is_zero() {
            log::warn!(
                "Late fill on spawned order {spawn_id} partially re-deducted from primary order {exec_spawn_id}: shortfall recorded as debt",
            );
        }

        publish_order_event(&event);
    }

    /// Submits an order to the execution engine via the risk engine.
    ///
    /// Orders carrying a live emulation trigger are refused before submission.
    /// For spawned orders with a pending primary reduction, refusal restores the
    /// cached primary order quantity while it remains local and publishes
    /// `OrderUpdated`.
    ///
    /// # Errors
    ///
    /// Returns an error if the order carries a live emulation trigger or submission fails.
    fn submit_order(
        &mut self,
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
        let trader_id =
            registered_trader_id(ExecutionAlgorithmNative::exec_algorithm_core_mut(self))?;

        if order.emulation_trigger().is_some() {
            let client_order_id = order.client_order_id();
            self.restore_primary_order_quantity(&order, true);
            return Err(EmulatedOrderSubmissionError { client_order_id }.into());
        }

        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let ts_init = core.clock_mut().timestamp_ns();

        // For spawned orders, use the parent's strategy ID
        let strategy_id = order.strategy_id();

        let primary_id = order
            .exec_spawn_id()
            .unwrap_or_else(|| order.client_order_id());
        let params = core.submit_params(&primary_id);

        let order_exists = {
            let cache = core.cache_ref();
            cache.order_exists(&order.client_order_id())
        };

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order.clone(), position_id, client_id, true)?;
        }

        if !order_exists {
            publish_order_initialized(&order);
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
            None, // correlation_id
        );

        if core.config.log_commands {
            let id = &core.actor.actor_id;
            log::info!("{id} {SEND}{CMD} {command}");
        }

        if order.is_primary() {
            core.mark_primary_handed_off(order.client_order_id());
        }

        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_queue_execute(),
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
    ) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
        let qty_changing = quantity.is_some_and(|q| q != order.quantity());
        let price_changing = price.is_some() && price != order.price();
        let trigger_changing = trigger_price.is_some() && trigger_price != order.trigger_price();

        if !qty_changing && !price_changing && !trigger_changing {
            log::error!(
                "Cannot create command ModifyOrder: \
                quantity, price, and trigger were either None \
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

        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let trader_id = registered_trader_id(core)?;
        let strategy_id = order.strategy_id();

        if !order.is_active_local() {
            required_account_id(order, "pending update")?;
            let event = self.generate_order_pending_update(order);
            let event = OrderEventAny::PendingUpdate(event);

            {
                let cache_rc = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_rc();
                let mut cache = cache_rc.borrow_mut();
                match cache.update_order(&event) {
                    Ok(updated) => *order = updated,
                    Err(e)
                        if matches!(
                            e.downcast_ref::<OrderError>(),
                            Some(OrderError::InvalidStateTransition)
                        ) =>
                    {
                        log::warn!("InvalidStateTrigger: {e}, did not apply pending update event");
                        return Ok(());
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
        }

        let ts_init = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .clock_mut()
            .timestamp_ns();
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
            None, // params,
            None, // correlation_id
        );

        if ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .config
            .log_commands
        {
            let id = &ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .actor
                .actor_id;
            log::info!("{id} {SEND}{CMD} {command}");
        }

        let has_emulation_trigger = order.emulation_trigger().is_some();

        if order.is_emulated() || has_emulation_trigger {
            msgbus::send_trading_command(
                MessagingSwitchboard::order_emulator_execute(),
                TradingCommand::ModifyOrder(command),
            );
        } else {
            msgbus::send_trading_command(
                MessagingSwitchboard::risk_engine_queue_execute(),
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
    ) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
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

        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let ts_now = core.clock_mut().timestamp_ns();

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
            order.is_quote_quantity(),
        );

        let event = OrderEventAny::Updated(updated);

        {
            let cache_rc = core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            *order = cache.update_order(&event)?;
        }

        publish_order_event(&event);

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
    ) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
        if order.is_closed() || order.is_pending_cancel() {
            log::warn!(
                "Cannot cancel order: state is {:?}, {order:?}",
                order.status()
            );
            return Ok(());
        }

        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
        let trader_id = registered_trader_id(core)?;
        let strategy_id = order.strategy_id();

        if !order.is_active_local() {
            required_account_id(order, "pending cancel")?;
            let event = self.generate_order_pending_cancel(order);
            let event = OrderEventAny::PendingCancel(event);

            {
                let cache_rc = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_rc();
                let mut cache = cache_rc.borrow_mut();
                match cache.update_order(&event) {
                    Ok(updated) => *order = updated,
                    Err(e)
                        if matches!(
                            e.downcast_ref::<OrderError>(),
                            Some(OrderError::InvalidStateTransition)
                        ) =>
                    {
                        log::warn!("InvalidStateTrigger: {e}, did not apply pending cancel event");
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                }
            }

            let topic = format!("events.order.{strategy_id}");
            msgbus::publish_order_event(topic.into(), &event);
            msgbus::publish_order_event(
                msgbus::switchboard::get_order_pending_cancel_topic(order.instrument_id()),
                &event,
            );
        }

        let ts_init = ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .clock_mut()
            .timestamp_ns();
        let command = CancelOrder::new(
            trader_id,
            client_id,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id(),
            UUID4::new(),
            ts_init,
            None, // params,
            None, // correlation_id
        );

        if ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
            .config
            .log_commands
        {
            let id = &ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .actor
                .actor_id;
            log::info!("{id} {SEND}{CMD} {command}");
        }

        let has_emulation_trigger = order.emulation_trigger().is_some();

        if order.is_emulated() || order.status() == OrderStatus::Released || has_emulation_trigger {
            msgbus::send_trading_command(
                MessagingSwitchboard::order_emulator_execute(),
                TradingCommand::CancelOrder(command),
            );
        } else {
            msgbus::send_trading_command(
                MessagingSwitchboard::exec_engine_queue_execute(),
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
        Self: ExecutionAlgorithmNative,
        Self: 'static + std::fmt::Debug + Sized,
    {
        let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
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
    fn unsubscribe_all_strategy_events(&mut self)
    where
        Self: ExecutionAlgorithmNative,
    {
        let handlers =
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self).take_strategy_event_handlers();

        for (strategy_id, h) in handlers {
            msgbus::unsubscribe_order_events(h.order_topic.into(), &h.order_handler);
            msgbus::unsubscribe_position_events(h.position_topic.into(), &h.position_handler);
            log::info!("Unsubscribed from events for strategy {strategy_id}");
        }
        ExecutionAlgorithmNative::exec_algorithm_core_mut(self).clear_subscribed_strategies();
    }

    /// Handles an order event, filtering for algorithm-owned orders.
    fn handle_order_event(&mut self, event: OrderEventAny)
    where
        Self: ExecutionAlgorithmNative,
    {
        if DataActorNative::core(ExecutionAlgorithmNative::exec_algorithm_core_mut(self)).state()
            != ComponentState::Running
        {
            return;
        }

        let order = {
            let cache = ExecutionAlgorithmNative::exec_algorithm_core_mut(self).cache_ref();
            cache.order(&event.client_order_id()).map(|o| o.clone())
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
            let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
            if core.config.log_events {
                let id = &core.actor.actor_id;
                log::info!("{id} {RECV}{EVT} {event}");
            }
        }

        if order.is_primary() && !order.is_active_local() {
            ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                .clear_primary_spawn_state(order.client_order_id());
        }

        match &event {
            OrderEventAny::Initialized(e) => self.on_order_initialized(e.clone()),
            OrderEventAny::Denied(e) => {
                self.restore_primary_order_quantity(&order, false);
                self.on_order_denied(*e);
            }
            OrderEventAny::Emulated(e) => self.on_order_emulated(*e),
            OrderEventAny::Released(e) => self.on_order_released(*e),
            OrderEventAny::Submitted(e) => self.on_order_submitted(*e),
            OrderEventAny::Rejected(e) => {
                self.restore_primary_order_quantity(&order, false);
                self.on_order_rejected(*e);
            }
            OrderEventAny::Accepted(e) => self.on_order_accepted(*e),
            OrderEventAny::Canceled(e) => {
                self.restore_primary_order_quantity(&order, false);
                self.on_algo_order_canceled(*e);
            }
            OrderEventAny::Expired(e) => {
                self.restore_primary_order_quantity(&order, false);
                self.on_order_expired(*e);
            }
            OrderEventAny::Triggered(e) => self.on_order_triggered(*e),
            OrderEventAny::PendingUpdate(e) => self.on_order_pending_update(*e),
            OrderEventAny::PendingCancel(e) => self.on_order_pending_cancel(*e),
            OrderEventAny::ModifyRejected(e) => self.on_order_modify_rejected(*e),
            OrderEventAny::CancelRejected(e) => self.on_order_cancel_rejected(*e),
            OrderEventAny::Updated(e) => self.on_order_updated(*e),
            OrderEventAny::Filled(e) => {
                self.rededuct_late_spawn_fill(&order);
                if order.leaves_qty().is_zero() {
                    let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
                    if core
                        .spawn_reduction(order.client_order_id())
                        .is_some_and(|reduction| reduction.restored_qty.is_none())
                    {
                        core.take_pending_spawn_reduction(order.client_order_id());
                    }
                }
                self.on_algo_order_filled(e.clone());
            }
            OrderEventAny::FillVoided(e) => {
                if ExecutionAlgorithmNative::exec_algorithm_core_mut(self)
                    .spawn_reduction(order.client_order_id())
                    .is_some_and(|reduction| reduction.restored_qty.is_some())
                {
                    self.restore_primary_order_quantity(&order, false);
                }
                self.on_order_fill_voided(e);
            }
        }

        self.on_order_event(event);
    }

    /// Handles a position event.
    fn handle_position_event(&mut self, event: PositionEvent)
    where
        Self: ExecutionAlgorithmNative,
    {
        if DataActorNative::core(ExecutionAlgorithmNative::exec_algorithm_core_mut(self)).state()
            != ComponentState::Running
        {
            return;
        }

        {
            let core = ExecutionAlgorithmNative::exec_algorithm_core_mut(self);
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
    fn on_start(&mut self) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
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

    /// Called when the algorithm is resumed.
    ///
    /// # Errors
    ///
    /// Returns an error if resume fails.
    fn on_resume(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when the algorithm is reset.
    ///
    /// # Errors
    ///
    /// Returns an error if reset fails.
    fn on_reset(&mut self) -> anyhow::Result<()>
    where
        Self: ExecutionAlgorithmNative,
    {
        self.unsubscribe_all_strategy_events();
        ExecutionAlgorithmNative::exec_algorithm_core_mut(self).reset();
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

    /// Called when an applied order fill is partly or fully voided.
    #[allow(unused_variables)]
    fn on_order_fill_voided(&mut self, event: &OrderFillVoided) {}

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

#[derive(Debug)]
pub(crate) struct EmulatedOrderSubmissionError {
    client_order_id: ClientOrderId,
}

impl Display for EmulatedOrderSubmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Execution algorithm cannot submit order {} with a live emulation trigger",
            self.client_order_id
        )
    }
}

impl std::error::Error for EmulatedOrderSubmissionError {}

fn spawn_unfilled_quantity(
    order: &OrderAny,
    reduction: SpawnReduction,
    primary_precision: u8,
) -> Option<Quantity> {
    if reduction.spawn_was_quote_quantity == order.is_quote_quantity() {
        return Some(
            order
                .quantity()
                .min(reduction.deducted_qty)
                .saturating_sub(order.filled_qty()),
        );
    }

    if !reduction.spawn_was_quote_quantity {
        log::warn!(
            "Cannot account for spawned order {} quantity: denomination changed from base to quote",
            order.client_order_id(),
        );
        return None;
    }

    let converted_total = order.quantity();
    if converted_total.is_zero() {
        log::warn!(
            "Cannot account for converted spawned order {} quantity: converted total is zero",
            order.client_order_id(),
        );
        return None;
    }

    // Voided fills release budget even when the venue does not reopen their leaves
    let child_qty = converted_total.saturating_sub(order.filled_qty());

    // Fills and leaves never exceed the order total; the clamp makes that a
    // structural bound so the quotient below always fits the raw width.
    let child_qty = child_qty.min(converted_total);
    // QuantityRaw is u64 or u128 depending on the high-precision feature, so
    // the widening is real in one build shape and an identity in the other.
    #[allow(clippy::useless_conversion)]
    let proportional_raw = QuantityRaw::try_from(muldiv_floor_u128(
        u128::from(reduction.deducted_qty.raw()),
        u128::from(child_qty.raw()),
        u128::from(converted_total.raw()),
    ))
    .expect("Quotient bounded by the deducted quantity");

    let precision_increment = Quantity::from_decimal_dp(
        rust_decimal::Decimal::new(1, u32::from(primary_precision)),
        primary_precision,
    )
    .expect("Primary quantity precision must be valid")
    .raw();
    let floored_raw = proportional_raw - proportional_raw % precision_increment;
    Some(Quantity::from_raw(floored_raw, primary_precision))
}

/// Returns `floor(a * b / c)` without intermediate overflow.
///
/// # Panics
///
/// Panics if `c` is zero, or if `b > c` and the quotient exceeds `u128`
/// (callers bound `b` by `c`, which bounds the quotient by `a`).
fn muldiv_floor_u128(a: u128, b: u128, c: u128) -> u128 {
    if let Some(product) = a.checked_mul(b) {
        return product / c;
    }

    let (hi, lo) = mul_wide_u128(a, b);
    assert!(hi < c, "muldiv_floor_u128 quotient exceeds u128");
    div_wide_u128(hi, lo, c)
}

/// Returns the 256-bit product of two `u128` values as `(high, low)` halves.
fn mul_wide_u128(a: u128, b: u128) -> (u128, u128) {
    const MASK: u128 = (1u128 << 64) - 1;
    let (a_hi, a_lo) = (a >> 64, a & MASK);
    let (b_hi, b_lo) = (b >> 64, b & MASK);

    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;

    let mid = (ll >> 64) + (lh & MASK) + (hl & MASK);
    let lo = (mid << 64) | (ll & MASK);
    let hi = hh + (lh >> 64) + (hl >> 64) + (mid >> 64);
    (hi, lo)
}

/// Divides the 256-bit value `(hi, lo)` by `c` via restoring long division,
/// truncating toward zero. Requires `hi < c` so the quotient fits `u128`.
fn div_wide_u128(hi: u128, lo: u128, c: u128) -> u128 {
    let mut rem = hi;
    let mut quotient = 0u128;

    for i in (0..u128::BITS).rev() {
        // A carry out of the shift means the true remainder is rem + 2^128,
        // which always exceeds c; wrapping_sub then yields the exact value.
        let carry = rem >> 127;
        rem = (rem << 1) | ((lo >> i) & 1);

        if carry == 1 || rem >= c {
            rem = rem.wrapping_sub(c);
            quotient |= 1 << i;
        }
    }
    quotient
}

/// Charges a late fill against a spawn reduction record, booking any shortfall
/// as debt against the primary order.
fn charge_spawn_reduction(
    core: &mut ExecutionAlgorithmCore,
    spawn_id: ClientOrderId,
    primary_id: ClientOrderId,
    mut reduction: SpawnReduction,
    unfilled_qty: Quantity,
    shortfall_qty: Quantity,
) {
    reduction.restored_qty = Some(unfilled_qty);
    core.set_spawn_reduction(spawn_id, reduction);

    if !shortfall_qty.is_zero() {
        core.add_spawn_fill_debt(primary_id, shortfall_qty);
    }
}

fn publish_order_initialized(order: &OrderAny) {
    let event = OrderEventAny::Initialized(order.init_event().clone());
    publish_order_event(&event);
}

fn publish_order_event(event: &OrderEventAny) {
    let topic = format!("events.order.{}", event.strategy_id());
    msgbus::publish_order_event(topic.into(), event);
}

fn registered_trader_id(core: &ExecutionAlgorithmCore) -> anyhow::Result<TraderId> {
    DataActorNative::core(core)
        .trader_id()
        .ok_or_else(|| anyhow::anyhow!("ExecutionAlgorithm not registered: trader_id is not set"))
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
        actor::DataActor,
        cache::Cache,
        clock::TestClock,
        component::Component,
        enums::ComponentTrigger,
        msgbus::{
            self, TypedHandler,
            stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
        },
    };
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType},
        events::{
            OrderAccepted, OrderCanceled, OrderDenied, OrderDeniedReason, OrderRejected,
            order::spec::{
                OrderAcceptedSpec, OrderCanceledSpec, OrderDeniedSpec, OrderExpiredSpec,
                OrderFillVoidedSpec, OrderFilledSpec, OrderRejectedSpec, OrderSubmittedSpec,
                OrderUpdatedSpec,
            },
        },
        identifiers::{
            AccountId, ActorId, ClientOrderId, ExecAlgorithmId, InstrumentId, StrategyId, TradeId,
            TraderId, VenueOrderId,
        },
        orders::{LimitOrder, MarketOrder, OrderAny, OrderTestBuilder, stubs::TestOrderStubs},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::nautilus_execution_algorithm;

    #[derive(Debug)]
    struct TestAlgorithm {
        core: ExecutionAlgorithmCore,
        order_client_ids: Vec<ClientOrderId>,
    }

    #[derive(Debug)]
    struct ModifyDispatchAlgorithm {
        core: ExecutionAlgorithmCore,
        modify_client_order_ids: Vec<ClientOrderId>,
    }

    #[derive(Debug)]
    struct CoreFreeExecutionAlgorithm {
        orders_seen: usize,
    }

    #[derive(Debug)]
    struct MacroTestCustomField {
        inner: ExecutionAlgorithmCore,
    }

    impl DataActor for CoreFreeExecutionAlgorithm {}

    impl ExecutionAlgorithm for CoreFreeExecutionAlgorithm {
        fn on_order(&mut self, _order: OrderAny) -> anyhow::Result<()> {
            self.orders_seen += 1;
            Ok(())
        }
    }

    impl DataActor for MacroTestCustomField {}

    nautilus_execution_algorithm!(MacroTestCustomField, inner, {
        fn on_order(&mut self, _order: OrderAny) -> anyhow::Result<()> {
            Ok(())
        }
    });

    impl TestAlgorithm {
        fn new(config: ExecutionAlgorithmConfig) -> Self {
            Self {
                core: ExecutionAlgorithmCore::new(config),
                order_client_ids: Vec::new(),
            }
        }
    }

    impl DataActor for TestAlgorithm {}

    nautilus_execution_algorithm!(TestAlgorithm, {
        fn on_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
            self.order_client_ids.push(order.client_order_id());
            Ok(())
        }
    });

    impl ModifyDispatchAlgorithm {
        fn new(config: ExecutionAlgorithmConfig) -> Self {
            Self {
                core: ExecutionAlgorithmCore::new(config),
                modify_client_order_ids: Vec::new(),
            }
        }
    }

    impl DataActor for ModifyDispatchAlgorithm {}

    nautilus_execution_algorithm!(ModifyDispatchAlgorithm, {
        fn on_order(&mut self, _order: OrderAny) -> anyhow::Result<()> {
            Ok(())
        }

        fn handle_modify_order(&mut self, command: ModifyOrder) -> anyhow::Result<()> {
            self.modify_client_order_ids.push(command.client_order_id);
            Ok(())
        }
    });

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

    fn subscribe_order_topic(
        strategy_id: StrategyId,
    ) -> (TypedHandler<OrderEventAny>, Rc<RefCell<Vec<OrderEventAny>>>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let handler = TypedHandler::from({
            let events = events.clone();
            move |event: &OrderEventAny| {
                events.borrow_mut().push(event.clone());
            }
        });
        msgbus::subscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            handler.clone(),
            None,
        );
        (handler, events)
    }

    fn setup_pending_spawn() -> (TestAlgorithm, ClientOrderId, OrderAny) {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);
        let client_order_id = ClientOrderId::from("O-001");
        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            client_order_id,
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
            Some(algo.id()),
            None,
            Some(client_order_id),
            None,
        ));
        algo.core
            .cache_rc()
            .borrow_mut()
            .add_order(primary.clone(), None, None, false)
            .unwrap();
        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.5"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );
        let spawned_order = OrderAny::Market(spawned);
        algo.core
            .cache_rc()
            .borrow_mut()
            .add_order(spawned_order.clone(), None, None, false)
            .unwrap();
        (algo, client_order_id, spawned_order)
    }

    fn setup_accepted_spawn() -> (TestAlgorithm, ClientOrderId, OrderAny) {
        let (mut algo, client_order_id, mut spawned_order) = setup_pending_spawn();
        let accepted = OrderAcceptedSpec::builder()
            .trader_id(spawned_order.trader_id())
            .strategy_id(spawned_order.strategy_id())
            .instrument_id(spawned_order.instrument_id())
            .client_order_id(spawned_order.client_order_id())
            .venue_order_id(VenueOrderId::from("V-123"))
            .account_id(AccountId::from("BINANCE-001"))
            .build();
        spawned_order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::Accepted(accepted))
            .unwrap();
        algo.handle_order_event(OrderEventAny::Accepted(accepted));
        (algo, client_order_id, spawned_order)
    }

    fn setup_accepted_quote_spawn(
        primary_qty: Quantity,
        spawn_qty: Quantity,
    ) -> (TestAlgorithm, ClientOrderId, OrderAny) {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);
        let client_order_id = ClientOrderId::from("O-QUOTE");
        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            client_order_id,
            OrderSide::Buy,
            primary_qty,
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false,
            true,
            None,
            None,
            None,
            None,
            Some(algo.id()),
            None,
            Some(client_order_id),
            None,
        ));
        algo.core
            .cache_rc()
            .borrow_mut()
            .add_order(primary.clone(), None, None, false)
            .unwrap();
        let mut spawned_order = OrderAny::Market(algo.spawn_market(
            &mut primary,
            spawn_qty,
            TimeInForce::Fok,
            false,
            None,
            true,
        ));
        algo.core
            .cache_rc()
            .borrow_mut()
            .add_order(spawned_order.clone(), None, None, false)
            .unwrap();
        accept_spawned_order(&mut algo, &mut spawned_order);
        (algo, client_order_id, spawned_order)
    }

    fn fill_spawned_order(algo: &mut TestAlgorithm, order: &mut OrderAny, quantity: Quantity) {
        let venue_order_id = order
            .venue_order_id()
            .unwrap_or_else(|| VenueOrderId::from("V-PRIMARY"));
        let filled = OrderFilledSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(venue_order_id)
            .account_id(AccountId::from("BINANCE-001"))
            .trade_id(TradeId::new(UUID4::new().to_string()))
            .order_side(order.order_side())
            .order_type(order.order_type())
            .last_qty(quantity)
            .last_px(Price::from("50000.0"))
            .currency(Currency::USD())
            .liquidity_side(LiquiditySide::Taker)
            .build();
        *order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::Filled(filled.clone()))
            .unwrap();
        algo.handle_order_event(OrderEventAny::Filled(filled));
    }

    fn submit_order_in_cache(algo: &mut TestAlgorithm, order: &mut OrderAny) {
        let submitted = OrderSubmittedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(AccountId::from("BINANCE-001"))
            .build();
        *order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::Submitted(submitted))
            .unwrap();
        algo.handle_order_event(OrderEventAny::Submitted(submitted));
    }

    fn cancel_spawned_order(algo: &mut TestAlgorithm, order: &mut OrderAny) {
        let venue_order_id = order
            .venue_order_id()
            .unwrap_or_else(|| VenueOrderId::from("V-123"));
        let canceled = OrderCanceledSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(venue_order_id)
            .account_id(AccountId::from("BINANCE-001"))
            .build();
        *order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::Canceled(canceled))
            .unwrap();
        algo.handle_order_event(OrderEventAny::Canceled(canceled));
    }

    fn convert_spawn_to_base(algo: &mut TestAlgorithm, order: &mut OrderAny, quantity: Quantity) {
        let updated = OrderUpdatedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .quantity(quantity)
            .maybe_venue_order_id(order.venue_order_id())
            .maybe_account_id(order.account_id())
            .is_quote_quantity(false)
            .build();
        *order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::Updated(updated))
            .unwrap();
        algo.handle_order_event(OrderEventAny::Updated(updated));
    }

    fn expire_spawned_order(algo: &mut TestAlgorithm, order: &mut OrderAny) {
        let expired = OrderExpiredSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from("V-123"))
            .account_id(AccountId::from("BINANCE-001"))
            .build();
        *order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::Expired(expired))
            .unwrap();
        algo.handle_order_event(OrderEventAny::Expired(expired));
    }

    fn accept_spawned_order(algo: &mut TestAlgorithm, order: &mut OrderAny) {
        let venue_order_id = VenueOrderId::from(format!("V-{}", order.client_order_id()).as_str());
        let accepted = OrderAcceptedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(venue_order_id)
            .account_id(AccountId::from("BINANCE-001"))
            .build();
        *order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::Accepted(accepted))
            .unwrap();
        algo.handle_order_event(OrderEventAny::Accepted(accepted));
    }

    fn spawn_reduced_child(
        algo: &mut TestAlgorithm,
        primary_id: ClientOrderId,
        quantity: Quantity,
    ) -> OrderAny {
        let mut primary = algo.cache().order(&primary_id).unwrap();
        let child = OrderAny::Market(algo.spawn_market(
            &mut primary,
            quantity,
            TimeInForce::Fok,
            false,
            None,
            true,
        ));
        algo.core
            .cache_rc()
            .borrow_mut()
            .add_order(child.clone(), None, None, false)
            .unwrap();
        child
    }

    #[rstest]
    fn test_algorithm_creation() {
        let algo = create_test_algorithm();
        assert!(algo.id().inner().starts_with("TEST-"));
        assert!(algo.order_client_ids.is_empty());
    }

    #[rstest]
    fn test_algorithm_registration() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        assert_eq!(algo.trader_id(), Some(TraderId::from("TRADER-001")));
    }

    #[rstest]
    fn test_algorithm_deny_order_updates_cache_and_publishes_once() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-DENY");
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-ALGO-DENY"),
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
        {
            let cache_rc = algo.core.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
        }
        let reason = OrderDeniedReason::ValidationFailed {
            detail: "invalid execution schedule".to_string(),
        }
        .to_string();
        let reason = Ustr::from(&reason);
        let (handler, events) = subscribe_order_topic(strategy_id);

        algo.deny_order(&order, reason).unwrap();
        algo.deny_order(&order, reason).unwrap();

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        let cached_order = algo.cache().order(&order.client_order_id()).unwrap();
        let events = events.borrow();

        assert_eq!(cached_order.status(), OrderStatus::Denied);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            OrderEventAny::Denied(event)
                if event.reason == reason
                    && event.strategy_id == strategy_id
                    && event.client_order_id == order.client_order_id()
        ));
    }

    #[rstest]
    fn test_algorithm_deny_order_initializes_missing_order_once() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-DENY-MISSING");
        let order = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(strategy_id)
            .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-ALGO-DENY-MISSING"))
            .quantity(Quantity::from("1.0"))
            .build();
        let reason = Ustr::from("VALIDATION_FAILED: invalid execution schedule");
        let (handler, events) = subscribe_order_topic(strategy_id);

        algo.deny_order(&order, reason).unwrap();
        algo.deny_order(&order, reason).unwrap();

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        let cached_order = algo.cache().order(&order.client_order_id()).unwrap();
        let events = events.borrow();

        assert_eq!(cached_order.status(), OrderStatus::Denied);
        assert_eq!(cached_order.event_count(), 2);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            OrderEventAny::Initialized(event)
                if event.strategy_id == strategy_id
                    && event.client_order_id == order.client_order_id()
        ));
        assert!(matches!(
            &events[1],
            OrderEventAny::Denied(event)
                if event.reason == reason
                    && event.strategy_id == strategy_id
                    && event.client_order_id == order.client_order_id()
        ));
    }

    #[rstest]
    fn test_algorithm_deny_order_does_not_publish_when_apply_fails() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-DENY-APPLY");
        let order = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(strategy_id)
            .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-ALGO-DENY-APPLY"))
            .quantity(Quantity::from("1.0"))
            .build();
        let order = TestOrderStubs::make_accepted_order(&order);
        {
            let cache_rc = algo.core.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
        }
        let (handler, events) = subscribe_order_topic(strategy_id);

        let mut params = nautilus_core::Params::new();
        params.insert(
            "route".to_string(),
            serde_json::Value::String("A".to_string()),
        );
        algo.core
            .remember_submit_params(order.client_order_id(), Some(params));

        let error = algo
            .deny_order(
                &order,
                Ustr::from("VALIDATION_FAILED: invalid execution schedule"),
            )
            .unwrap_err();

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        let cached_order = algo.cache().order(&order.client_order_id()).unwrap();

        assert!(matches!(
            error.downcast_ref::<OrderError>(),
            Some(OrderError::InvalidStateTransition)
        ));
        assert_eq!(cached_order.status(), OrderStatus::Accepted);
        assert!(events.borrow().is_empty());
        // A failed denial is not terminal, so the submit params must be retained
        assert!(algo.core.submit_params(&order.client_order_id()).is_some());
    }

    #[rstest]
    fn test_algorithm_deny_order_removes_submit_params() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-DENY-PARAMS");
        let order = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(strategy_id)
            .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-ALGO-DENY-PARAMS"))
            .quantity(Quantity::from("1.0"))
            .build();
        {
            let cache_rc = algo.core.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
        }

        let mut params = nautilus_core::Params::new();
        params.insert(
            "route".to_string(),
            serde_json::Value::String("A".to_string()),
        );
        algo.core
            .remember_submit_params(order.client_order_id(), Some(params));
        assert!(algo.core.submit_params(&order.client_order_id()).is_some());

        algo.deny_order(&order, Ustr::from("VALIDATION_FAILED: test"))
            .unwrap();

        assert!(algo.core.submit_params(&order.client_order_id()).is_none());
    }

    #[rstest]
    fn test_submit_order_errors_when_algorithm_not_registered() {
        let mut algo = create_test_algorithm();
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-UNREGISTERED-001"),
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

        let err = algo
            .submit_order(order, None, None)
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "ExecutionAlgorithm not registered: trader_id is not set"
        );
    }

    #[rstest]
    fn test_required_account_id_errors_when_missing_for_algorithm_event() {
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-NO-ACCOUNT-001"),
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

        let err = required_account_id(&order, "pending update")
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "Cannot generate pending update event for O-NO-ACCOUNT-001: account_id is not set"
        );
    }

    #[rstest]
    fn test_algorithm_id() {
        let algo = create_test_algorithm();
        assert!(algo.id().inner().starts_with("TEST-"));
    }

    #[rstest]
    fn test_execution_algorithm_behavior_does_not_require_native_core_access() {
        fn assert_execution_algorithm<T: ExecutionAlgorithm + DataActor>() {}

        assert_execution_algorithm::<CoreFreeExecutionAlgorithm>();

        let mut algorithm = CoreFreeExecutionAlgorithm { orders_seen: 0 };
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
            .quantity(Quantity::from("1.0"))
            .build();

        algorithm.on_order(order).unwrap();

        assert_eq!(algorithm.orders_seen, 1);
    }

    #[rstest]
    fn test_nautilus_execution_algorithm_macro_custom_field() {
        let exec_algorithm_id = ExecAlgorithmId::from("MACRO-001");
        let algorithm = MacroTestCustomField {
            inner: ExecutionAlgorithmCore::new(ExecutionAlgorithmConfig {
                exec_algorithm_id: Some(exec_algorithm_id),
                ..Default::default()
            }),
        };

        assert_eq!(algorithm.id(), exec_algorithm_id);
        assert_eq!(algorithm.actor_id(), ActorId::from("MACRO-001"));
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

        algo.on_order_initialized(OrderInitialized::default());
        algo.on_order_denied(OrderDenied::default());
        algo.on_order_emulated(OrderEmulated::default());
        algo.on_order_released(OrderReleased::default());
        algo.on_order_submitted(OrderSubmitted::default());
        algo.on_order_rejected(OrderRejected::default());
        algo.on_order_accepted(OrderAccepted::default());
        algo.on_algo_order_canceled(OrderCanceled::default());
        algo.on_order_expired(OrderExpired::default());
        algo.on_order_triggered(OrderTriggered::default());
        algo.on_order_pending_update(OrderPendingUpdate::default());
        algo.on_order_pending_cancel(OrderPendingCancel::default());
        algo.on_order_modify_rejected(OrderModifyRejected::default());
        algo.on_order_cancel_rejected(OrderCancelRejected::default());
        algo.on_order_updated(OrderUpdated::default());
        algo.on_algo_order_filled(OrderFilledSpec::builder().build());
        algo.on_order_fill_voided(&OrderFillVoidedSpec::builder().build());
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
    fn test_algorithm_spawn_propagates_primary_fields() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let mut params = indexmap::IndexMap::new();
        params.insert(ustr::Ustr::from("horizon_secs"), ustr::Ustr::from("30"));
        params.insert(ustr::Ustr::from("interval_secs"), ustr::Ustr::from("10"));
        let primary_tags = vec![ustr::Ustr::from("PRIMARY_TAG")];
        let linked_order_ids = vec![ClientOrderId::from("LINK-1")];
        let client_order_id = ClientOrderId::from("O-001");

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            client_order_id,
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            0.into(),
            false, // reduce_only
            true,  // quote_quantity
            None,  // contingency_type
            None,  // order_list_id
            Some(linked_order_ids.clone()),
            None, // parent_order_id
            Some(algo.id()),
            Some(params.clone()),
            Some(client_order_id),
            Some(primary_tags.clone()),
        ));

        let spawned_market = algo.spawn_market(
            &mut primary,
            Quantity::from("0.25"),
            TimeInForce::Ioc,
            false,
            None, // falls back to primary.tags
            false,
        );
        assert!(spawned_market.is_quote_quantity);
        assert_eq!(spawned_market.exec_algorithm_params, Some(params.clone()));
        assert_eq!(spawned_market.tags, Some(primary_tags.clone()));
        assert_eq!(
            spawned_market.linked_order_ids,
            Some(linked_order_ids.clone())
        );

        let spawned_limit = algo.spawn_limit(
            &mut primary,
            Quantity::from("0.25"),
            Price::from("50000.0"),
            TimeInForce::Gtc,
            None,  // expire_time
            false, // post_only
            false, // reduce_only
            None,  // display_qty
            None,  // emulation_trigger
            None,  // falls back to primary.tags
            false,
        );
        assert!(spawned_limit.is_quote_quantity);
        assert_eq!(spawned_limit.exec_algorithm_params, Some(params.clone()));
        assert_eq!(spawned_limit.tags, Some(primary_tags.clone()));
        assert_eq!(
            spawned_limit.linked_order_ids,
            Some(linked_order_ids.clone())
        );

        let spawned_mtl = algo.spawn_market_to_limit(
            &mut primary,
            Quantity::from("0.25"),
            TimeInForce::Gtc,
            None,  // expire_time
            false, // reduce_only
            None,  // display_qty
            None,  // emulation_trigger
            None,  // falls back to primary.tags
            false,
        );
        assert!(spawned_mtl.is_quote_quantity);
        assert_eq!(spawned_mtl.exec_algorithm_params, Some(params));
        assert_eq!(spawned_mtl.tags, Some(primary_tags));
        assert_eq!(spawned_mtl.linked_order_ids, Some(linked_order_ids));
    }

    #[rstest]
    fn test_muldiv_floor_u128_narrow_path_truncates() {
        assert_eq!(muldiv_floor_u128(50, 3, 5), 30);
        assert_eq!(muldiv_floor_u128(10, 2, 3), 6);
        assert_eq!(muldiv_floor_u128(u128::MAX, 7, 7), u128::MAX);
    }

    #[rstest]
    fn test_muldiv_floor_u128_wide_path_floors_exactly() {
        // a * b overflows u128; the exact quotient is a - a/c, fractionally
        // under a, so an implementation that rounds instead of flooring
        // returns a. This is the shape of the Decimal-fallback defect.
        let a = 10u128.pow(30);
        let b = 7 * 10u128.pow(30);
        let c = b + 1;
        assert!(a.checked_mul(b).is_none());
        assert_eq!(muldiv_floor_u128(a, b, c), a - 1);
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
    fn test_algorithm_reduce_primary_order_publishes_updated_event() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-REDUCE-PUBLISH");
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-ALGO-REDUCE"),
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
        let mut primary = TestOrderStubs::make_accepted_order(&order);

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }

        let (handler, events) = subscribe_order_topic(strategy_id);

        algo.reduce_primary_order(&mut primary, Quantity::from("0.3"));

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        let events = events.borrow();

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            OrderEventAny::Updated(event) if event.quantity == Quantity::from("0.7")
        ));
    }

    #[rstest]
    fn test_algorithm_submit_order_publishes_initialized_for_new_order() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-INIT-PUBLISH");
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-ALGO-INIT"),
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
        let (handler, events) = subscribe_order_topic(strategy_id);

        algo.submit_order(order.clone(), None, None).unwrap();

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        let events = events.borrow();

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            OrderEventAny::Initialized(event) if event.client_order_id == order.client_order_id()
        ));
    }

    #[rstest]
    fn test_algorithm_submit_order_does_not_republish_initialized_for_existing_order() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-INIT-EXISTING");
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-ALGO-INIT-EXISTING"),
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
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order.clone(), None, None, true).unwrap();
        }
        let (handler, events) = subscribe_order_topic(strategy_id);

        algo.submit_order(order, None, None).unwrap();

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        assert!(events.borrow().is_empty());
    }

    #[rstest]
    fn test_algorithm_submit_order_refuses_emulated_limit_spawn() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-EMULATED-LIMIT");
        let order = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(strategy_id)
            .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-ALGO-EMULATED-LIMIT"))
            .quantity(Quantity::from("1.0"))
            .build();
        let mut primary = TestOrderStubs::make_accepted_order(&order);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, false).unwrap();
        }
        let (event_handler, events) = subscribe_order_topic(strategy_id);
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let (emulator_handler, emulator_messages): (
            _,
            TypedIntoMessageSavingHandler<TradingCommand>,
        ) = get_typed_into_message_saving_handler(Some(Ustr::from("OrderEmulator.execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            emulator_handler,
        );

        let spawned = algo.spawn_limit(
            &mut primary,
            Quantity::from("0.4"),
            Price::from("50000.0"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            Some(TriggerType::BidAsk),
            None,
            true,
        );
        let spawned = OrderAny::Limit(spawned);
        let client_order_id = spawned.client_order_id();
        let result = algo.submit_order(spawned, None, None);

        msgbus::unsubscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            &event_handler,
        );
        let cache = algo.core.cache_ref();
        let error = result.unwrap_err();
        assert!(
            error
                .downcast_ref::<EmulatedOrderSubmissionError>()
                .is_some()
        );
        let error = error.to_string();
        assert!(error.contains("live emulation trigger"), "{error}");
        assert!(error.contains(client_order_id.as_str()), "{error}");
        assert!(risk_messages.get_messages().is_empty());
        assert!(emulator_messages.get_messages().is_empty());
        assert!(!cache.order_exists(&client_order_id));
        assert!(!events.borrow().iter().any(|event| matches!(
            event,
            OrderEventAny::Initialized(initialized)
                if initialized.client_order_id == client_order_id
        )));
        // The spawn already reduced the accepted primary locally (the venue
        // still works 1.0); restoration declines to mutate a non-local
        // primary, so the local deduction stands.
        assert_eq!(
            cache.order(&primary.client_order_id()).unwrap().quantity(),
            Quantity::from("0.6"),
        );
        drop(cache);
        assert!(
            algo.core
                .take_pending_spawn_reduction(client_order_id)
                .is_none()
        );
    }

    #[rstest]
    fn test_algorithm_submit_order_routes_unemulated_spawn_to_risk() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let mut primary = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-ALGO-UNEMULATED"))
            .quantity(Quantity::from("1.0"))
            .build();
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );

        let spawned = algo.spawn_limit(
            &mut primary,
            Quantity::from("0.4"),
            Price::from("50000.0"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            None,
            None,
            false,
        );
        let client_order_id = spawned.client_order_id;
        algo.submit_order(OrderAny::Limit(spawned), None, None)
            .unwrap();

        let risk_messages = risk_messages.get_messages();
        assert_eq!(risk_messages.len(), 1);
        assert!(matches!(
            risk_messages.first(),
            Some(TradingCommand::SubmitOrder(command))
                if command.client_order_id == client_order_id
        ));
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
    fn test_algorithm_forwards_captured_params_to_spawned_order() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-FWD-001");
        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-FWD-001"),
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
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(primary.clone(), None, None, true).unwrap();
        }

        let mut params = nautilus_core::Params::new();
        params.insert("is_leverage".to_string(), serde_json::Value::Bool(true));
        let command = SubmitOrder::new(
            TraderId::from("TRADER-001"),
            None,
            strategy_id,
            primary.instrument_id(),
            primary.client_order_id(),
            primary.init_event().clone(),
            primary.exec_algorithm_id(),
            None,
            Some(params),
            UUID4::new(),
            0.into(),
            None,
        );
        algo.execute(TradingCommand::SubmitOrder(command)).unwrap();

        let received = Rc::new(RefCell::new(None::<SubmitOrder>));
        let handler = msgbus::TypedIntoHandler::from({
            let captured = received.clone();
            move |cmd: TradingCommand| {
                if let TradingCommand::SubmitOrder(cmd) = cmd {
                    *captured.borrow_mut() = Some(cmd);
                }
            }
        });
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            handler,
        );

        let spawned = algo.spawn_market(
            &mut primary,
            Quantity::from("0.4"),
            TimeInForce::Ioc,
            false,
            None,
            false, // reduce_primary
        );
        algo.submit_order(OrderAny::Market(spawned), None, None)
            .unwrap();

        let captured = received.borrow();
        let cmd = captured.as_ref().expect("expected a forwarded SubmitOrder");
        assert_eq!(cmd.client_order_id, ClientOrderId::from("O-FWD-001-E1"));
        assert_eq!(
            cmd.params.as_ref().and_then(|p| p.get_bool("is_leverage")),
            Some(true),
        );
    }

    #[rstest]
    fn test_algorithm_routes_modify_and_cancel_commands_through_engine_queues() {
        let mut modify_algo = create_test_algorithm();
        let mut cancel_algo = create_test_algorithm();
        register_algorithm(&mut modify_algo);
        register_algorithm(&mut cancel_algo);

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

        let mut modify_order = TestOrderStubs::make_accepted_order(
            &OrderTestBuilder::new(OrderType::Limit)
                .strategy_id(StrategyId::from("STRAT-ALGO-ROUTING"))
                .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
                .client_order_id(ClientOrderId::from("O-ALGO-MODIFY"))
                .quantity(Quantity::from("1.0"))
                .price(Price::from("50000.0"))
                .build(),
        );
        let mut cancel_order = TestOrderStubs::make_accepted_order(
            &OrderTestBuilder::new(OrderType::Market)
                .strategy_id(StrategyId::from("STRAT-ALGO-ROUTING"))
                .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
                .client_order_id(ClientOrderId::from("O-ALGO-CANCEL"))
                .quantity(Quantity::from("1.0"))
                .build(),
        );
        {
            let cache_rc = modify_algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(modify_order.clone(), None, None, false)
                .unwrap();
        }
        {
            let cache_rc = cancel_algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(cancel_order.clone(), None, None, false)
                .unwrap();
        }

        modify_algo
            .modify_order(
                &mut modify_order,
                None,
                Some(Price::from("51000.0")),
                None,
                None,
            )
            .unwrap();
        cancel_algo.cancel_order(&mut cancel_order, None).unwrap();

        let risk_messages = risk_messages.get_messages();
        let exec_messages = exec_messages.get_messages();
        assert_eq!(risk_messages.len(), 1);
        assert!(matches!(
            risk_messages.first(),
            Some(TradingCommand::ModifyOrder(command))
                if command.client_order_id == modify_order.client_order_id()
        ));
        assert_eq!(exec_messages.len(), 1);
        assert!(matches!(
            exec_messages.first(),
            Some(TradingCommand::CancelOrder(command))
                if command.client_order_id == cancel_order.client_order_id()
        ));
    }

    #[rstest]
    fn test_algorithm_submit_order_list_captures_params_per_order() {
        use nautilus_common::messages::execution::SubmitOrderList;
        use nautilus_model::identifiers::OrderListId;

        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-LIST-001");
        let order1 = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-LIST-001"),
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
        let order2 = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            InstrumentId::from("BTC/USDT.BINANCE"),
            ClientOrderId::from("O-LIST-002"),
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
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order1.clone(), None, None, true).unwrap();
            cache.add_order(order2.clone(), None, None, true).unwrap();
        }

        let order_list = OrderList::new(
            OrderListId::from("OL-001"),
            order1.instrument_id(),
            strategy_id,
            vec![order1.client_order_id(), order2.client_order_id()],
            0.into(),
        );

        let mut params = nautilus_core::Params::new();
        params.insert("is_leverage".to_string(), serde_json::Value::Bool(true));
        let command = SubmitOrderList::new(
            TraderId::from("TRADER-001"),
            None,
            strategy_id,
            order_list,
            vec![order1.init_event().clone(), order2.init_event().clone()],
            order1.exec_algorithm_id(),
            None,
            Some(params),
            UUID4::new(),
            0.into(),
            None,
        );
        algo.execute(TradingCommand::SubmitOrderList(command))
            .unwrap();

        assert_eq!(
            algo.order_client_ids,
            [
                ClientOrderId::from("O-LIST-001"),
                ClientOrderId::from("O-LIST-002"),
            ],
        );

        for id in ["O-LIST-001", "O-LIST-002"] {
            assert_eq!(
                algo.core
                    .submit_params(&ClientOrderId::from(id))
                    .and_then(|p| p.get_bool("is_leverage")),
                Some(true),
                "expected forwarded params for {id}",
            );
        }
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
    fn test_algorithm_handle_cancel_order_publishes_instrument_canceled_topic() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-CANCEL-PUBLISH");
        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
            instrument_id,
            ClientOrderId::from("O-ALGO-CANCEL"),
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
        let order = TestOrderStubs::make_accepted_order(&order);

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(order.clone(), None, None, false).unwrap();
        }

        let received = Rc::new(RefCell::new(Vec::<OrderEventAny>::new()));
        let handler = TypedHandler::from({
            let received = received.clone();
            move |event: &OrderEventAny| {
                received.borrow_mut().push(event.clone());
            }
        });
        let topic = msgbus::switchboard::get_order_canceled_topic(instrument_id);
        msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

        let command = CancelOrder::new(
            order.trader_id(),
            None,
            strategy_id,
            instrument_id,
            order.client_order_id(),
            order.venue_order_id(),
            UUID4::new(),
            0.into(),
            None,
            None,
        );
        algo.handle_cancel_order(command).unwrap();

        msgbus::unsubscribe_order_events(topic.into(), &handler);
        let received = received.borrow();
        assert_eq!(received.len(), 1);
        assert!(matches!(received[0], OrderEventAny::Canceled(_)));
        assert_eq!(received[0].client_order_id(), order.client_order_id());
        assert_eq!(received[0].instrument_id(), instrument_id);
    }

    #[rstest]
    fn test_algorithm_execute_dispatches_modify_order_to_handler() {
        let unique_id = format!("TEST-{}", UUID4::new());
        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::new(&unique_id)),
            ..Default::default()
        };
        let mut algo = ModifyDispatchAlgorithm::new(config);
        algo.core
            .register(
                TraderId::from("TRADER-001"),
                Rc::new(RefCell::new(TestClock::new())),
                Rc::new(RefCell::new(Cache::default())),
            )
            .unwrap();
        algo.transition_state(ComponentTrigger::Initialize).unwrap();
        algo.transition_state(ComponentTrigger::Start).unwrap();
        algo.transition_state(ComponentTrigger::StartCompleted)
            .unwrap();

        let client_order_id = ClientOrderId::from("O-ALGO-DISPATCH");
        let command = ModifyOrder::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("STRAT-ALGO-DISPATCH"),
            InstrumentId::from("BTC/USDT.BINANCE"),
            client_order_id,
            None,
            Some(Quantity::from("0.5")),
            None,
            None,
            UUID4::new(),
            0.into(),
            None,
            None,
        );

        algo.execute(TradingCommand::ModifyOrder(command)).unwrap();

        assert_eq!(algo.modify_client_order_ids, vec![client_order_id]);
    }

    #[rstest]
    fn test_algorithm_handle_modify_order_refuses_active_local_order_without_events() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-MODIFY");
        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(InstrumentId::from("BTC/USDT.BINANCE"))
            .client_order_id(ClientOrderId::from("O-ALGO-MODIFY"))
            .quantity(Quantity::from("1.0"))
            .exec_algorithm_id(algo.id())
            .exec_spawn_id(ClientOrderId::from("O-ALGO-MODIFY"))
            .build();
        {
            let cache_rc = algo.core.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
        }
        let (handler, events) = subscribe_order_topic(strategy_id);
        let command = ModifyOrder::new(
            order.trader_id(),
            None,
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            None,
            Some(Quantity::from("0.5")),
            None,
            None,
            UUID4::new(),
            0.into(),
            None,
            None,
        );

        algo.execute(TradingCommand::ModifyOrder(command)).unwrap();

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        let cached_order = algo.cache().order(&order.client_order_id()).unwrap();
        assert_eq!(cached_order.status(), OrderStatus::Initialized);
        assert_eq!(cached_order.quantity(), Quantity::from("1.0"));
        assert!(events.borrow().is_empty());
    }

    #[rstest]
    fn test_algorithm_modify_order_in_place_updates_quantity() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let strategy_id = StrategyId::from("STRAT-ALGO-MODIFY-IN-PLACE");
        let mut order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TRADER-001"),
            strategy_id,
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
        let (handler, events) = subscribe_order_topic(strategy_id);

        algo.modify_order_in_place(&mut order, Some(new_qty), None, None)
            .unwrap();

        msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), &handler);
        let events = events.borrow();

        assert_eq!(order.quantity(), new_qty);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            OrderEventAny::Updated(event) if event.quantity == new_qty
        ));
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
        let client_order_id = ClientOrderId::from("O-001");

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            client_order_id,
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
            Some(client_order_id),
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

        assert_eq!(primary.quantity(), Quantity::from("0.5"));

        let spawned_order = OrderAny::Market(spawned);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(spawned_order.clone(), None, None, false)
                .unwrap();
        }

        let denied = OrderDeniedSpec::builder()
            .trader_id(spawned_order.trader_id())
            .strategy_id(spawned_order.strategy_id())
            .instrument_id(spawned_order.instrument_id())
            .client_order_id(spawned_order.client_order_id())
            .reason("TEST_DENIAL".into())
            .build();

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&OrderEventAny::Denied(denied)).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Denied(denied));

        let restored_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(restored_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_spawned_order_rejected_restores_primary_quantity() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();
        let client_order_id = ClientOrderId::from("O-001");

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            client_order_id,
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
            Some(client_order_id),
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

        assert_eq!(primary.quantity(), Quantity::from("0.5"));

        let spawned_order = OrderAny::Market(spawned);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(spawned_order.clone(), None, None, false)
                .unwrap();
        }

        let rejected = OrderRejectedSpec::builder()
            .trader_id(spawned_order.trader_id())
            .strategy_id(spawned_order.strategy_id())
            .instrument_id(spawned_order.instrument_id())
            .client_order_id(spawned_order.client_order_id())
            .account_id(AccountId::from("BINANCE-001"))
            .reason("TEST_REJECTION".into())
            .build();

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .update_order(&OrderEventAny::Rejected(rejected))
                .unwrap();
        }

        algo.handle_order_event(OrderEventAny::Rejected(rejected));

        let restored_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(restored_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_spawned_order_with_reduce_primary_false_does_not_restore() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();
        let client_order_id = ClientOrderId::from("O-001");

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            client_order_id,
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
            Some(client_order_id),
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

        let spawned_order = OrderAny::Market(spawned);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache
                .add_order(spawned_order.clone(), None, None, false)
                .unwrap();
        }

        let denied = OrderDeniedSpec::builder()
            .trader_id(spawned_order.trader_id())
            .strategy_id(spawned_order.strategy_id())
            .instrument_id(spawned_order.instrument_id())
            .client_order_id(spawned_order.client_order_id())
            .reason("TEST_DENIAL".into())
            .build();

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&OrderEventAny::Denied(denied)).unwrap();
        }

        algo.handle_order_event(OrderEventAny::Denied(denied));

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_multiple_spawns_with_one_denied_restores_correctly() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();
        let client_order_id = ClientOrderId::from("O-001");

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            client_order_id,
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
            Some(client_order_id),
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
        let spawned2 = algo.spawn_market(
            &mut primary,
            Quantity::from("0.4"),
            TimeInForce::Fok,
            false,
            None,
            true,
        );
        assert_eq!(primary.quantity(), Quantity::from("0.3"));

        let spawned_order1 = OrderAny::Market(spawned1);
        let spawned_order2 = OrderAny::Market(spawned2);
        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.add_order(spawned_order1, None, None, false).unwrap();
            cache
                .add_order(spawned_order2.clone(), None, None, false)
                .unwrap();
        }

        let denied = OrderDeniedSpec::builder()
            .trader_id(spawned_order2.trader_id())
            .strategy_id(spawned_order2.strategy_id())
            .instrument_id(spawned_order2.instrument_id())
            .client_order_id(spawned_order2.client_order_id())
            .reason("TEST_DENIAL".into())
            .build();

        {
            let cache_rc = algo.core.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            cache.update_order(&OrderEventAny::Denied(denied)).unwrap();
        }

        let (handler, events) = subscribe_order_topic(spawned_order2.strategy_id());

        algo.handle_order_event(OrderEventAny::Denied(denied));

        msgbus::unsubscribe_order_events(
            format!("events.order.{}", spawned_order2.strategy_id()).into(),
            &handler,
        );
        let events = events.borrow();

        let restored_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(restored_primary.quantity(), Quantity::from("0.7"));
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            OrderEventAny::Updated(event) if event.quantity == Quantity::from("0.7")
        ));
    }

    #[rstest]
    fn test_spawned_order_accepted_then_canceled_restores_reduction() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();

        let primary_after_accept = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(primary_after_accept.quantity(), Quantity::from("0.5"));

        // Per the maintainer's ruling, acceptance preserves the reduction until terminal outcome
        cancel_spawned_order(&mut algo, &mut spawned_order);

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_spawned_order_canceled_after_primary_submission_does_not_restore_reduction() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        let mut primary = algo.cache().order(&client_order_id).unwrap();
        submit_order_in_cache(&mut algo, &mut primary);

        cancel_spawned_order(&mut algo, &mut spawned_order);

        let mut submitted_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(submitted_primary.quantity(), Quantity::from("0.5"));
        assert!(
            algo.core
                .take_pending_spawn_reduction(spawned_order.client_order_id())
                .is_none()
        );
        fill_spawned_order(&mut algo, &mut submitted_primary, Quantity::from("0.5"));
        assert!(submitted_primary.is_closed());
        assert_eq!(submitted_primary.status(), OrderStatus::Filled);
    }

    #[rstest]
    fn test_spawned_order_canceled_during_primary_submission_handoff_does_not_restore() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        let primary = algo.cache().order(&client_order_id).unwrap();
        algo.core
            .add_spawn_fill_debt(client_order_id, Quantity::from("0.1"));
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let strategy_id = primary.strategy_id();
        let (event_handler, events) = subscribe_order_topic(strategy_id);

        algo.submit_order(primary, None, None).unwrap();
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().status(),
            OrderStatus::Initialized,
        );
        cancel_spawned_order(&mut algo, &mut spawned_order);

        msgbus::unsubscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            &event_handler,
        );
        let risk_messages = risk_messages.get_messages();
        let [TradingCommand::SubmitOrder(command)] = risk_messages.as_slice() else {
            panic!("Expected exactly one SubmitOrder command");
        };
        assert_eq!(command.client_order_id, client_order_id);
        // The command embeds the immutable OrderInitialized event, so its
        // quantity is the pre-reduction 1.0; consumers resolve the cached
        // order, which carries the reduced quantity asserted below.
        assert_eq!(command.order_init.quantity, Quantity::from("1.0"));
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.5"),
        );
        assert!(!events.borrow().iter().any(|event| matches!(
            event,
            OrderEventAny::Updated(updated) if updated.client_order_id == client_order_id
        )));
        assert!(
            algo.core
                .take_pending_spawn_reduction(spawned_order.client_order_id())
                .is_none()
        );
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
    }

    #[rstest]
    fn test_late_spawn_fill_rededucts_restored_primary_before_final_submission() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut spawned_order);
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("1.0"),
        );

        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.5"));

        let mut primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(primary.quantity(), Quantity::from("0.5"));
        submit_order_in_cache(&mut algo, &mut primary);
        assert_eq!(
            spawned_order.filled_qty() + primary.quantity(),
            Quantity::from("1.0"),
        );
    }

    #[rstest]
    fn test_late_spawn_fill_during_primary_submission_handoff_does_not_rededuct() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut spawned_order);
        let primary = algo.cache().order(&client_order_id).unwrap();
        algo.core
            .add_spawn_fill_debt(client_order_id, Quantity::from("0.1"));
        let (risk_handler, risk_messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            risk_handler,
        );
        let strategy_id = primary.strategy_id();
        let (event_handler, events) = subscribe_order_topic(strategy_id);

        algo.submit_order(primary, None, None).unwrap();
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().status(),
            OrderStatus::Initialized,
        );
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.5"));

        msgbus::unsubscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            &event_handler,
        );
        let risk_messages = risk_messages.get_messages();
        let [TradingCommand::SubmitOrder(command)] = risk_messages.as_slice() else {
            panic!("Expected exactly one SubmitOrder command");
        };
        assert_eq!(command.client_order_id, client_order_id);
        // The command embeds the immutable OrderInitialized event; here the
        // cancellation already restored the cache to 1.0, so the two agree.
        assert_eq!(command.order_init.quantity, Quantity::from("1.0"));
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("1.0"),
        );
        assert!(!events.borrow().iter().any(|event| matches!(
            event,
            OrderEventAny::Updated(updated) if updated.client_order_id == client_order_id
        )));
        assert!(
            algo.core
                .take_pending_spawn_reduction(spawned_order.client_order_id())
                .is_none()
        );
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
    }

    #[rstest]
    fn test_converted_quote_spawn_canceled_unfilled_restores_full_quote_quantity() {
        let (mut algo, client_order_id, mut spawned_order) =
            setup_accepted_quote_spawn(Quantity::from("100"), Quantity::from("50"));
        convert_spawn_to_base(&mut algo, &mut spawned_order, Quantity::from("5"));

        cancel_spawned_order(&mut algo, &mut spawned_order);

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("100"),
        );
        assert!(
            algo.cache()
                .order(&client_order_id)
                .unwrap()
                .is_quote_quantity()
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("50"),
        );
    }

    #[rstest]
    fn test_converted_quote_spawn_partial_fill_restores_proportional_quote_quantity() {
        let (mut algo, client_order_id, mut spawned_order) =
            setup_accepted_quote_spawn(Quantity::from("100"), Quantity::from("50"));
        convert_spawn_to_base(&mut algo, &mut spawned_order, Quantity::from("5"));
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("2"));

        cancel_spawned_order(&mut algo, &mut spawned_order);

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("80"),
        );
        assert!(
            algo.cache()
                .order(&client_order_id)
                .unwrap()
                .is_quote_quantity()
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("30"),
        );
    }

    #[rstest]
    fn test_converted_quote_spawn_late_fill_charges_proportional_quote_quantity() {
        let (mut algo, client_order_id, mut spawned_order) =
            setup_accepted_quote_spawn(Quantity::from("100"), Quantity::from("50"));
        convert_spawn_to_base(&mut algo, &mut spawned_order, Quantity::from("5"));
        cancel_spawned_order(&mut algo, &mut spawned_order);

        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("1"));

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("90"),
        );
        assert!(
            algo.cache()
                .order(&client_order_id)
                .unwrap()
                .is_quote_quantity()
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("40"),
        );
    }

    #[rstest]
    fn test_converted_quote_spawn_restoration_rounds_down_to_primary_precision() {
        let (mut algo, client_order_id, mut spawned_order) =
            setup_accepted_quote_spawn(Quantity::from("20.00"), Quantity::from("10.00"));
        convert_spawn_to_base(&mut algo, &mut spawned_order, Quantity::from("3.000"));
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("1.000"));

        cancel_spawned_order(&mut algo, &mut spawned_order);

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("16.66"),
        );
        assert!(
            algo.cache()
                .order(&client_order_id)
                .unwrap()
                .is_quote_quantity()
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("6.66"),
        );
    }

    #[rstest]
    fn test_converted_quote_spawn_repeated_fractional_late_fills_conserve_budget() {
        let (mut algo, client_order_id, mut spawned_order) =
            setup_accepted_quote_spawn(Quantity::from("20.00"), Quantity::from("10.00"));
        convert_spawn_to_base(&mut algo, &mut spawned_order, Quantity::from("3.000"));
        cancel_spawned_order(&mut algo, &mut spawned_order);

        for (primary_qty, restored_qty) in [
            (Quantity::from("16.66"), Quantity::from("6.66")),
            (Quantity::from("13.33"), Quantity::from("3.33")),
            (Quantity::from("10.00"), Quantity::from("0.00")),
        ] {
            fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("1.000"));
            assert_eq!(
                algo.cache().order(&client_order_id).unwrap().quantity(),
                primary_qty,
            );
            assert_eq!(
                algo.core
                    .spawn_reduction(spawned_order.client_order_id())
                    .unwrap()
                    .restored_qty
                    .unwrap(),
                restored_qty,
            );
        }
    }

    #[rstest]
    #[case::single_fill(Quantity::from("3.0"), 1)]
    #[case::split_fills(Quantity::from("0.1"), 30)]
    fn test_converted_quote_spawn_fill_partition_preserves_total(
        #[case] fill_qty: Quantity,
        #[case] fills: usize,
    ) {
        let (mut algo, primary_id, mut child) =
            setup_accepted_quote_spawn(Quantity::from("20"), Quantity::from("10"));
        convert_spawn_to_base(&mut algo, &mut child, Quantity::from("3.0"));
        cancel_spawned_order(&mut algo, &mut child);

        for _ in 0..fills {
            fill_spawned_order(&mut algo, &mut child, fill_qty);
        }

        assert_eq!(child.filled_qty(), Quantity::from("3.0"));
        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("10")
        );
        assert_eq!(
            algo.core
                .spawn_reduction(child.client_order_id())
                .unwrap()
                .restored_qty,
            Some(Quantity::from("0"))
        );
        assert!(algo.core.spawn_fill_debt(primary_id).is_none());
    }

    #[rstest]
    fn test_increased_spawn_late_fill_debits_original_restoration() {
        let (mut algo, primary_id, mut child) = setup_pending_spawn();
        algo.modify_order_in_place(&mut child, Some(Quantity::from("0.8")), None, None)
            .unwrap();
        accept_spawned_order(&mut algo, &mut child);
        cancel_spawned_order(&mut algo, &mut child);

        fill_spawned_order(&mut algo, &mut child, Quantity::from("0.1"));

        assert_eq!(child.quantity(), Quantity::from("0.8"));
        assert_eq!(child.filled_qty(), Quantity::from("0.1"));
        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("0.9")
        );
        assert_eq!(
            algo.core
                .spawn_reduction(child.client_order_id())
                .unwrap()
                .restored_qty,
            Some(Quantity::from("0.4"))
        );
        assert!(algo.core.spawn_fill_debt(primary_id).is_none());

        void_last_spawn_fill(&mut algo, &mut child, Quantity::from("0.1"));

        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("1.0")
        );
    }

    #[rstest]
    #[case::partial_fill(Quantity::from("0.1"))]
    #[case::full_fill(Quantity::from("0.5"))]
    fn test_voided_late_spawn_fill_restores_primary_quantity(#[case] fill_qty: Quantity) {
        let (mut algo, primary_id, mut child) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut child);
        fill_spawned_order(&mut algo, &mut child, fill_qty);

        void_last_spawn_fill(&mut algo, &mut child, fill_qty);

        assert_eq!(child.filled_qty(), Quantity::from("0.0"));
        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("1.0")
        );
        assert!(algo.core.spawn_fill_debt(primary_id).is_none());
        assert_eq!(
            algo.core
                .spawn_reduction(child.client_order_id())
                .unwrap()
                .restored_qty,
            Some(Quantity::from("0.5"))
        );
    }

    #[rstest]
    #[case::debt_only(
        Quantity::from("0.1"),
        Quantity::from("0.0"),
        Some(Quantity::from("0.1"))
    )]
    #[case::debt_and_quantity(Quantity::from("0.3"), Quantity::from("0.1"), None)]
    fn test_voided_late_spawn_fill_discharges_debt_before_restoring_quantity(
        #[case] voided_qty: Quantity,
        #[case] primary_qty: Quantity,
        #[case] debt_qty: Option<Quantity>,
    ) {
        let (mut algo, primary_id, mut child) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut child);
        let _second = spawn_reduced_child(&mut algo, primary_id, Quantity::from("0.8"));
        fill_spawned_order(&mut algo, &mut child, Quantity::from("0.4"));
        assert_eq!(
            algo.core.spawn_fill_debt(primary_id),
            Some(Quantity::from("0.2"))
        );

        void_last_spawn_fill(&mut algo, &mut child, voided_qty);

        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            primary_qty
        );
        assert_eq!(algo.core.spawn_fill_debt(primary_id), debt_qty);
        assert_eq!(child.filled_qty(), Quantity::from("0.4") - voided_qty);
        assert_eq!(
            algo.core
                .spawn_reduction(child.client_order_id())
                .unwrap()
                .restored_qty,
            Some(Quantity::from("0.1") + voided_qty)
        );
    }

    #[rstest]
    fn test_spawn_fill_void_before_restoration_preserves_reserved_quantity() {
        let (mut algo, primary_id, mut child) = setup_accepted_spawn();
        fill_spawned_order(&mut algo, &mut child, Quantity::from("0.1"));

        void_last_spawn_fill(&mut algo, &mut child, Quantity::from("0.1"));

        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("0.5")
        );
        assert_eq!(
            algo.core
                .spawn_reduction(child.client_order_id())
                .unwrap()
                .restored_qty,
            None
        );
        cancel_spawned_order(&mut algo, &mut child);
        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("1.0")
        );
    }

    #[rstest]
    fn test_spawn_fill_void_after_primary_handoff_preserves_submitted_quantity() {
        let (mut algo, primary_id, mut child) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut child);
        fill_spawned_order(&mut algo, &mut child, Quantity::from("0.1"));
        let primary = algo.cache().order(&primary_id).unwrap();
        let (handler, _messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            handler,
        );
        algo.submit_order(primary, None, None).unwrap();

        void_last_spawn_fill(&mut algo, &mut child, Quantity::from("0.1"));

        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("0.9")
        );
        assert_eq!(
            algo.cache().order(&primary_id).unwrap().status(),
            OrderStatus::Initialized
        );
        assert!(algo.core.primary_was_handed_off(primary_id));
        assert!(algo.core.spawn_reduction(child.client_order_id()).is_none());
        assert!(algo.core.spawn_fill_debt(primary_id).is_none());
    }

    #[rstest]
    fn test_converted_quote_spawn_fill_void_restores_cumulative_budget_once() {
        let (mut algo, primary_id, mut child) =
            setup_accepted_quote_spawn(Quantity::from("20.00"), Quantity::from("10.00"));
        convert_spawn_to_base(&mut algo, &mut child, Quantity::from("3.000"));
        cancel_spawned_order(&mut algo, &mut child);
        fill_spawned_order(&mut algo, &mut child, Quantity::from("1.000"));
        fill_spawned_order(&mut algo, &mut child, Quantity::from("1.000"));

        void_last_spawn_fill(&mut algo, &mut child, Quantity::from("0.500"));
        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("15.00")
        );
        void_last_spawn_fill(&mut algo, &mut child, Quantity::from("1.000"));
        let duplicate = child.events().into_iter().last().unwrap().clone();
        algo.handle_order_event(duplicate);

        assert_eq!(child.filled_qty(), Quantity::from("1.000"));
        assert_eq!(
            algo.cache().order(&primary_id).unwrap().quantity(),
            Quantity::from("16.66")
        );
        assert_eq!(
            algo.core
                .spawn_reduction(child.client_order_id())
                .unwrap()
                .restored_qty,
            Some(Quantity::from("6.66"))
        );
        assert!(algo.core.spawn_fill_debt(primary_id).is_none());
    }

    fn void_last_spawn_fill(algo: &mut TestAlgorithm, order: &mut OrderAny, quantity: Quantity) {
        let fill = order
            .events()
            .into_iter()
            .rev()
            .find_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill.clone()),
                _ => None,
            })
            .unwrap();
        let voided = OrderFillVoidedSpec::builder()
            .trader_id(fill.trader_id)
            .strategy_id(fill.strategy_id)
            .instrument_id(fill.instrument_id)
            .client_order_id(fill.client_order_id)
            .venue_order_id(fill.venue_order_id)
            .account_id(fill.account_id)
            .trade_id(fill.trade_id)
            .voided_qty(quantity)
            .order_side(fill.order_side)
            .order_type(fill.order_type)
            .last_px(fill.last_px)
            .currency(fill.currency)
            .liquidity_side(fill.liquidity_side)
            .build();
        *order = algo
            .core
            .cache_rc()
            .borrow_mut()
            .update_order(&OrderEventAny::FillVoided(voided.clone()))
            .unwrap();
        algo.handle_order_event(OrderEventAny::FillVoided(voided));
    }

    #[rstest]
    fn test_primary_handoff_clears_spawn_accounting_without_child_events() {
        let (mut algo, primary_id, mut child) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut child);
        let primary = algo.cache().order(&primary_id).unwrap();
        let (handler, _messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            handler,
        );

        algo.submit_order(primary, None, None).unwrap();

        assert!(algo.core.spawn_reduction(child.client_order_id()).is_none());
        assert!(algo.core.spawn_fill_debt(primary_id).is_none());
        assert!(algo.core.primary_was_handed_off(primary_id));

        let mut primary = algo.cache().order(&primary_id).unwrap();
        submit_order_in_cache(&mut algo, &mut primary);
        assert!(!algo.core.primary_was_handed_off(primary_id));
    }

    #[rstest]
    fn test_primary_cancellation_clears_spawn_accounting() {
        let (mut algo, primary_id, mut child) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut child);
        let second = spawn_reduced_child(&mut algo, primary_id, Quantity::from("0.8"));
        fill_spawned_order(&mut algo, &mut child, Quantity::from("0.4"));
        let mut primary = algo.cache().order(&primary_id).unwrap();

        cancel_spawned_order(&mut algo, &mut primary);

        assert_eq!(primary.status(), OrderStatus::Canceled);
        assert!(algo.core.spawn_reduction(child.client_order_id()).is_none());
        assert!(
            algo.core
                .spawn_reduction(second.client_order_id())
                .is_none()
        );
        assert!(algo.core.spawn_fill_debt(primary_id).is_none());
        assert!(!algo.core.primary_was_handed_off(primary_id));
    }

    #[rstest]
    fn test_converted_quote_spawn_partial_cancel_then_late_fill_charges_restored_budget() {
        let (mut algo, client_order_id, mut spawned_order) =
            setup_accepted_quote_spawn(Quantity::from("100"), Quantity::from("50"));
        convert_spawn_to_base(&mut algo, &mut spawned_order, Quantity::from("5"));
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("2"));
        cancel_spawned_order(&mut algo, &mut spawned_order);

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("80"),
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("30"),
        );

        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("1"));

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("70"),
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("20"),
        );
    }

    #[rstest]
    fn test_unmarked_submitted_primary_cancellation_discards_reduction_and_debt() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        let mut primary = algo.cache().order(&client_order_id).unwrap();
        algo.core
            .add_spawn_fill_debt(client_order_id, Quantity::from("0.1"));

        submit_order_in_cache(&mut algo, &mut primary);
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
        cancel_spawned_order(&mut algo, &mut spawned_order);

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.5"),
        );
        assert!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .is_none()
        );
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
    }

    #[rstest]
    #[case::denied(true)]
    #[case::rejected(false)]
    fn test_spawn_refusal_does_not_restore_submitted_primary(#[case] denied: bool) {
        let (mut algo, client_order_id, spawned_order) = setup_pending_spawn();
        let spawned_id = spawned_order.client_order_id();
        let mut primary = algo.cache().order(&client_order_id).unwrap();
        submit_order_in_cache(&mut algo, &mut primary);

        if denied {
            let event = OrderDeniedSpec::builder()
                .trader_id(spawned_order.trader_id())
                .strategy_id(spawned_order.strategy_id())
                .instrument_id(spawned_order.instrument_id())
                .client_order_id(spawned_id)
                .reason("TEST_DENIAL".into())
                .build();
            algo.handle_order_event(OrderEventAny::Denied(event));
        } else {
            let event = OrderRejectedSpec::builder()
                .trader_id(spawned_order.trader_id())
                .strategy_id(spawned_order.strategy_id())
                .instrument_id(spawned_order.instrument_id())
                .client_order_id(spawned_id)
                .account_id(AccountId::from("BINANCE-001"))
                .reason("TEST_REJECTION".into())
                .build();
            algo.handle_order_event(OrderEventAny::Rejected(event));
        }

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.5"),
        );
        assert!(algo.core.take_pending_spawn_reduction(spawned_id).is_none());
    }

    #[rstest]
    fn test_multiple_late_partial_fills_net_only_the_restored_quantity() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.2"));
        cancel_spawned_order(&mut algo, &mut spawned_order);

        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.1"));
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.7"),
        );

        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.2"));
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.5"),
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty,
            Some(Quantity::from("0.0")),
        );
    }

    #[rstest]
    fn test_late_spawn_fill_after_restored_quantity_reused_caps_at_primary_quantity() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut spawned_order);
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("1.0"),
        );

        // Reuse most of the restored quantity through a second spawn
        let mut primary = algo.cache().order(&client_order_id).unwrap();
        let second_order = OrderAny::Market(algo.spawn_market(
            &mut primary,
            Quantity::from("0.8"),
            TimeInForce::Fok,
            false,
            None,
            true,
        ));
        algo.core
            .cache_rc()
            .borrow_mut()
            .add_order(second_order.clone(), None, None, false)
            .unwrap();
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.2"),
        );

        // The late fill exceeds the primary's remaining quantity: the
        // re-deduction caps at zero and the shortfall becomes debt
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.5"));
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.0"),
        );
        assert_eq!(
            algo.core
                .spawn_reduction(spawned_order.client_order_id())
                .unwrap()
                .restored_qty,
            Some(Quantity::from("0.0")),
        );
        assert_eq!(
            algo.core.spawn_fill_debt(client_order_id),
            Some(Quantity::from("0.3")),
        );

        // The second spawn terminating unfilled discharges the debt before
        // returning quantity: 0.8 restores only 0.5
        let rejected = OrderRejectedSpec::builder()
            .trader_id(second_order.trader_id())
            .strategy_id(second_order.strategy_id())
            .instrument_id(second_order.instrument_id())
            .client_order_id(second_order.client_order_id())
            .account_id(AccountId::from("BINANCE-001"))
            .reason("TEST_REJECTION".into())
            .build();
        algo.handle_order_event(OrderEventAny::Rejected(rejected));

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("0.5"));
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
        assert_eq!(
            spawned_order.filled_qty() + final_primary.quantity(),
            Quantity::from("1.0"),
        );
    }

    #[rstest]
    fn test_late_fill_on_child_that_fully_discharged_debt_recreates_debt() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut spawned_order);

        let mut child_b = spawn_reduced_child(&mut algo, client_order_id, Quantity::from("0.3"));
        let mut child_c = spawn_reduced_child(&mut algo, client_order_id, Quantity::from("0.5"));
        accept_spawned_order(&mut algo, &mut child_b);
        accept_spawned_order(&mut algo, &mut child_c);
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.2"),
        );

        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.5"));
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.0"),
        );
        assert_eq!(
            algo.core.spawn_fill_debt(client_order_id),
            Some(Quantity::from("0.3")),
        );

        // B's cancellation fully discharges the debt; its record keeps the
        // gross released amount for late-fill tracking
        cancel_spawned_order(&mut algo, &mut child_b);
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.0"),
        );
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
        assert_eq!(
            algo.core
                .spawn_reduction(child_b.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("0.3"),
        );

        // B's late fill reverses the settlement for the filled amount
        fill_spawned_order(&mut algo, &mut child_b, Quantity::from("0.2"));
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.0"),
        );
        assert_eq!(
            algo.core.spawn_fill_debt(client_order_id),
            Some(Quantity::from("0.2")),
        );

        cancel_spawned_order(&mut algo, &mut child_c);
        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("0.3"));
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
        assert_eq!(
            spawned_order.filled_qty() + child_b.filled_qty() + final_primary.quantity(),
            Quantity::from("1.0"),
        );
    }

    #[rstest]
    fn test_late_fill_on_child_after_partial_debt_discharge_nets_from_primary() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        cancel_spawned_order(&mut algo, &mut spawned_order);

        let mut child_b = spawn_reduced_child(&mut algo, client_order_id, Quantity::from("0.8"));
        accept_spawned_order(&mut algo, &mut child_b);
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.5"));
        assert_eq!(
            algo.core.spawn_fill_debt(client_order_id),
            Some(Quantity::from("0.3")),
        );

        // B's cancellation discharges 0.3 of debt and restores the net 0.5;
        // its record keeps the gross 0.8
        cancel_spawned_order(&mut algo, &mut child_b);
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.5"),
        );
        assert!(algo.core.spawn_fill_debt(client_order_id).is_none());
        assert_eq!(
            algo.core
                .spawn_reduction(child_b.client_order_id())
                .unwrap()
                .restored_qty
                .unwrap(),
            Quantity::from("0.8"),
        );

        fill_spawned_order(&mut algo, &mut child_b, Quantity::from("0.3"));
        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("0.2"));
        assert_eq!(
            spawned_order.filled_qty() + child_b.filled_qty() + final_primary.quantity(),
            Quantity::from("1.0"),
        );
    }

    #[rstest]
    fn test_emulated_spawn_refusal_restores_initialized_primary() {
        let (mut algo, client_order_id, _spawned_order) = setup_pending_spawn();
        let mut primary = algo.cache().order(&client_order_id).unwrap();
        let spawned = algo.spawn_limit(
            &mut primary,
            Quantity::from("0.3"),
            Price::from("50000.0"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            None,
            Some(TriggerType::BidAsk),
            None,
            true,
        );
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.2"),
        );

        let result = algo.submit_order(OrderAny::Limit(spawned), None, None);

        assert!(result.is_err());
        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("0.5"),
        );
    }

    #[rstest]
    fn test_spawned_order_accepted_then_expired_restores_reduction() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();

        expire_spawned_order(&mut algo, &mut spawned_order);

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    fn test_partially_filled_spawned_order_canceled_restores_leaves_quantity() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.2"));

        cancel_spawned_order(&mut algo, &mut spawned_order);

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("0.8"));
    }

    #[rstest]
    fn test_partially_filled_spawned_order_expired_restores_leaves_quantity() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.2"));

        expire_spawned_order(&mut algo, &mut spawned_order);

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("0.8"));
    }

    #[rstest]
    fn test_fully_filled_spawned_order_consumes_reduction_without_restoration() {
        let (mut algo, client_order_id, mut spawned_order) = setup_accepted_spawn();
        let spawned_id = spawned_order.client_order_id();

        fill_spawned_order(&mut algo, &mut spawned_order, Quantity::from("0.5"));

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("0.5"));
        assert!(algo.core.take_pending_spawn_reduction(spawned_id).is_none());
    }

    #[rstest]
    #[case::denied(true)]
    #[case::rejected(false)]
    fn test_spawned_order_refusal_then_terminal_event_restores_only_once(#[case] denied: bool) {
        // A canceled event after a denial/rejection is not an applicable state
        // transition and the engine drops such races before publication; the
        // second event is dispatched directly to exercise the handler's own
        // idempotence (the accounted unfilled quantity is unchanged).
        let (mut algo, client_order_id, spawned_order) = setup_pending_spawn();

        if denied {
            let event = OrderDeniedSpec::builder()
                .trader_id(spawned_order.trader_id())
                .strategy_id(spawned_order.strategy_id())
                .instrument_id(spawned_order.instrument_id())
                .client_order_id(spawned_order.client_order_id())
                .reason("TEST_DENIAL".into())
                .build();
            algo.core
                .cache_rc()
                .borrow_mut()
                .update_order(&OrderEventAny::Denied(event))
                .unwrap();
            algo.handle_order_event(OrderEventAny::Denied(event));
        } else {
            let event = OrderRejectedSpec::builder()
                .trader_id(spawned_order.trader_id())
                .strategy_id(spawned_order.strategy_id())
                .instrument_id(spawned_order.instrument_id())
                .client_order_id(spawned_order.client_order_id())
                .account_id(AccountId::from("BINANCE-001"))
                .reason("TEST_REJECTION".into())
                .build();
            algo.core
                .cache_rc()
                .borrow_mut()
                .update_order(&OrderEventAny::Rejected(event))
                .unwrap();
            algo.handle_order_event(OrderEventAny::Rejected(event));
        }

        assert_eq!(
            algo.cache().order(&client_order_id).unwrap().quantity(),
            Quantity::from("1.0"),
        );

        let canceled = OrderCanceledSpec::builder()
            .trader_id(spawned_order.trader_id())
            .strategy_id(spawned_order.strategy_id())
            .instrument_id(spawned_order.instrument_id())
            .client_order_id(spawned_order.client_order_id())
            .build();
        algo.handle_order_event(OrderEventAny::Canceled(canceled));

        let final_primary = algo.cache().order(&client_order_id).unwrap();
        assert_eq!(final_primary.quantity(), Quantity::from("1.0"));
    }

    #[rstest]
    #[should_panic(expected = "exceeds primary leaves_qty")]
    fn test_spawn_quantity_exceeds_leaves_qty_panics() {
        let mut algo = create_test_algorithm();
        register_algorithm(&mut algo);

        let instrument_id = InstrumentId::from("BTC/USDT.BINANCE");
        let exec_algorithm_id = algo.id();
        let client_order_id = ClientOrderId::from("O-001");

        let mut primary = OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("STRAT-001"),
            instrument_id,
            client_order_id,
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
            Some(client_order_id),
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
