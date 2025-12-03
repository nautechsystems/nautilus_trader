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

pub mod config;
pub mod core;

pub use core::StrategyCore;

pub use config::StrategyConfig;
use nautilus_common::{
    actor::DataActor,
    messages::execution::{
        CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
    msgbus,
    timer::TimeEvent,
};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, PositionSide, TimeInForce, TriggerType},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionEvent, PositionOpened,
    },
    identifiers::{ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId},
    orders::{Order, OrderAny, OrderCore, OrderList},
    position::Position,
    types::{Price, Quantity},
};
use ustr::Ustr;

/// Core trait for implementing trading strategies in NautilusTrader.
///
/// Strategies are specialized [`DataActor`]s that combine data ingestion capabilities with
/// comprehensive order and position management functionality. By implementing this trait,
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
/// User strategies should implement the [`Strategy::core_mut`] method to provide
/// access to their internal [`StrategyCore`], which handles the integration with
/// the trading engine. All order and position management methods are provided
/// as default implementations.
pub trait Strategy: DataActor {
    /// Provides mutable access to the internal `StrategyCore`.
    ///
    /// This method must be implemented by the user's strategy struct, typically
    /// by returning a mutable reference to its `StrategyCore` member.
    fn core_mut(&mut self) -> &mut StrategyCore;

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
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let command = SubmitOrder::new(
            trader_id,
            client_id.unwrap_or_default(),
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id().unwrap_or_default(),
            order.clone(),
            order.exec_algorithm_id(),
            position_id,
            None, // params
            UUID4::new(),
            ts_init,
        )?;

        let Some(manager) = &mut core.order_manager else {
            anyhow::bail!("Strategy not registered: OrderManager missing");
        };

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
        order_list: OrderList,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();
        {
            let cache_rc = core.cache();
            if cache_rc.order_list_exists(&order_list.id) {
                anyhow::bail!("OrderList denied: duplicate {}", order_list.id);
            }

            for order in &order_list.orders {
                if order.status() != OrderStatus::Initialized {
                    anyhow::bail!(
                        "Order in list denied: invalid status for {}, expected INITIALIZED",
                        order.client_order_id()
                    );
                }
                if cache_rc.order_exists(&order.client_order_id()) {
                    anyhow::bail!(
                        "Order in list denied: duplicate {}",
                        order.client_order_id()
                    );
                }
            }
        }

        let command = SubmitOrderList::new(
            trader_id,
            client_id.unwrap_or_default(),
            strategy_id,
            order_list.instrument_id,
            order_list
                .orders
                .first()
                .map(|o| o.client_order_id())
                .unwrap_or_default(),
            order_list
                .orders
                .first()
                .map(|o| o.venue_order_id().unwrap_or_default())
                .unwrap_or_default(),
            order_list.clone(),
            None,
            position_id,
            UUID4::new(),
            ts_init,
        )?;

        let has_emulated_order = order_list.orders.iter().any(|o| {
            matches!(o.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger)
                || o.is_emulated()
        });

        let first_order = order_list.orders.first();
        let exec_algorithm_id = first_order.and_then(|o| o.exec_algorithm_id());

        let Some(manager) = &mut core.order_manager else {
            anyhow::bail!("Strategy not registered: OrderManager missing");
        };

        if has_emulated_order {
            manager.send_emulator_command(TradingCommand::SubmitOrderList(command));
        } else if let Some(algo_id) = exec_algorithm_id {
            let endpoint = format!("{algo_id}.execute");
            msgbus::send_any(endpoint.into(), &TradingCommand::SubmitOrderList(command));
        } else {
            manager.send_risk_command(TradingCommand::SubmitOrderList(command));
        }

        for order in &order_list.orders {
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
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let command = ModifyOrder::new(
            trader_id,
            client_id.unwrap_or_default(),
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id().unwrap_or_default(),
            quantity,
            price,
            trigger_price,
            UUID4::new(),
            ts_init,
        )?;

        let Some(manager) = &mut core.order_manager else {
            anyhow::bail!("Strategy not registered: OrderManager missing");
        };

        if matches!(order.emulation_trigger(), Some(trigger) if trigger != TriggerType::NoTrigger) {
            manager.send_emulator_command(TradingCommand::ModifyOrder(command));
        } else if order.exec_algorithm_id().is_some() {
            manager.send_risk_command(TradingCommand::ModifyOrder(command));
        } else {
            manager.send_exec_command(TradingCommand::ModifyOrder(command));
        }
        Ok(())
    }

    /// Cancels an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or order cancellation fails.
    fn cancel_order(&mut self, order: OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();

        let command = CancelOrder::new(
            trader_id,
            client_id.unwrap_or_default(),
            strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id().unwrap_or_default(),
            UUID4::new(),
            ts_init,
        )?;

        let Some(manager) = &mut core.order_manager else {
            anyhow::bail!("Strategy not registered: OrderManager missing");
        };

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
        let core = self.core_mut();

        let trader_id = core.trader_id().expect("Trader ID not set");
        let strategy_id = StrategyId::from(core.actor_id().inner().as_str());
        let ts_init = core.clock().timestamp_ns();
        let cache = core.cache();

        let open_orders =
            cache.orders_open(None, Some(&instrument_id), Some(&strategy_id), order_side);

        let emulated_orders =
            cache.orders_emulated(None, Some(&instrument_id), Some(&strategy_id), order_side);

        let exec_algorithm_ids = cache.exec_algorithm_ids();
        let mut algo_orders = Vec::new();

        for algo_id in &exec_algorithm_ids {
            let orders = cache.orders_for_exec_algorithm(
                algo_id,
                None,
                Some(&instrument_id),
                Some(&strategy_id),
                order_side,
            );
            algo_orders.extend(orders.iter().map(|o| (*o).clone()));
        }

        let open_count = open_orders.len();
        let emulated_count = emulated_orders.len();
        let algo_count = algo_orders.len();

        drop(cache);

        if open_count == 0 && emulated_count == 0 && algo_count == 0 {
            let side_str = order_side.map(|s| format!(" {s}")).unwrap_or_default();
            log::info!("No {instrument_id} open or emulated{side_str} orders to cancel");
            return Ok(());
        }

        let Some(manager) = &mut core.order_manager else {
            anyhow::bail!("Strategy not registered: OrderManager missing");
        };

        if open_count > 0 {
            let command = CancelAllOrders::new(
                trader_id,
                client_id.unwrap_or_default(),
                strategy_id,
                instrument_id,
                order_side.unwrap_or(OrderSide::NoOrderSide),
                UUID4::new(),
                ts_init,
            )?;
            manager.send_exec_command(TradingCommand::CancelAllOrders(command));
        }

        if emulated_count > 0 {
            let command = CancelAllOrders::new(
                trader_id,
                client_id.unwrap_or_default(),
                strategy_id,
                instrument_id,
                order_side.unwrap_or(OrderSide::NoOrderSide),
                UUID4::new(),
                ts_init,
            )?;
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
        let Some(order_factory) = &mut core.order_factory else {
            anyhow::bail!("Strategy not registered: OrderFactory missing");
        };

        if position.is_closed() {
            log::warn!("Cannot close position (already closed): {}", position.id);
            return Ok(());
        }

        let closing_side = OrderCore::closing_side(position.side);

        let order = order_factory.market(
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
    #[allow(clippy::too_many_arguments)]
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
            position_side,
        );

        if positions_open.is_empty() {
            let side_str = position_side.map(|s| format!(" {s}")).unwrap_or_default();
            log::info!("No {instrument_id} open{side_str} positions to close");
            return Ok(());
        }

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
            let Some(order_factory) = &mut core.order_factory else {
                anyhow::bail!("Strategy not registered: OrderFactory missing");
            };

            let closing_side = OrderCore::closing_side(pos_side);
            let order = order_factory.market(
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

    /// Handles an order event, dispatching to the appropriate handler and routing to the order manager.
    fn handle_order_event(&mut self, event: OrderEventAny) {
        let client_order_id = event.client_order_id();
        let is_terminal = matches!(
            &event,
            OrderEventAny::Filled(_)
                | OrderEventAny::Canceled(_)
                | OrderEventAny::Rejected(_)
                | OrderEventAny::Expired(_)
                | OrderEventAny::Denied(_)
        );

        match &event {
            OrderEventAny::Initialized(e) => self.on_order_initialized(e.clone()),
            OrderEventAny::Denied(e) => self.on_order_denied(*e),
            OrderEventAny::Emulated(e) => self.on_order_emulated(*e),
            OrderEventAny::Released(e) => self.on_order_released(*e),
            OrderEventAny::Submitted(e) => self.on_order_submitted(*e),
            OrderEventAny::Rejected(e) => self.on_order_rejected(*e),
            OrderEventAny::Accepted(e) => self.on_order_accepted(*e),
            OrderEventAny::Canceled(e) => self.on_order_canceled(*e),
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

        if is_terminal {
            self.cancel_gtd_expiry(&client_order_id);
        }

        let core = self.core_mut();
        if let Some(manager) = &mut core.order_manager {
            manager.handle_event(event);
        }
    }

    /// Handles a position event, dispatching to the appropriate handler.
    fn handle_position_event(&mut self, event: PositionEvent) {
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
    /// Routes GTD expiry timer events to the expiry handler.
    ///
    /// # Errors
    ///
    /// Returns an error if time event handling fails.
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name.starts_with("GTD-EXPIRY:") {
            self.expire_gtd_order(event.clone());
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

    /// Called when an order is canceled.
    ///
    /// Override this method to implement custom logic when an order is canceled.
    #[allow(unused_variables)]
    fn on_order_canceled(&mut self, event: OrderCanceled) {}

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

        let open_orders = cache.orders_open(None, None, Some(&strategy_id), None);

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
    };
    use nautilus_model::{
        enums::{OrderSide, PositionSide},
        events::OrderRejected,
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
        types::Currency,
    };
    use nautilus_portfolio::portfolio::Portfolio;
    use rstest::rstest;

    use super::*;

    #[derive(Debug)]
    struct TestStrategy {
        core: StrategyCore,
        on_order_rejected_called: bool,
        on_position_opened_called: bool,
    }

    impl TestStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
                on_order_rejected_called: false,
                on_position_opened_called: false,
            }
        }
    }

    impl Deref for TestStrategy {
        type Target = DataActorCore;
        fn deref(&self) -> &Self::Target {
            &self.core.actor
        }
    }

    impl DerefMut for TestStrategy {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.core.actor
        }
    }

    impl DataActor for TestStrategy {}

    impl Strategy for TestStrategy {
        fn core_mut(&mut self) -> &mut StrategyCore {
            &mut self.core
        }

        fn on_order_rejected(&mut self, _event: OrderRejected) {
            self.on_order_rejected_called = true;
        }

        fn on_position_opened(&mut self, _event: PositionOpened) {
            self.on_position_opened_called = true;
        }
    }

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

        let event = OrderEventAny::Rejected(OrderRejected {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id: ClientOrderId::from("O-001"),
            account_id: AccountId::from("ACC-001"),
            reason: "Test rejection".into(),
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: 0,
            due_post_only: 0,
        });

        strategy.handle_order_event(event);

        assert!(strategy.on_order_rejected_called);
    }

    #[rstest]
    fn test_handle_position_event_dispatches_to_handler() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let event = PositionEvent::PositionOpened(PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            position_id: Default::default(),
            account_id: AccountId::from("ACC-001"),
            opening_order_id: ClientOrderId::from("O-001"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Default::default(),
            last_qty: Default::default(),
            last_px: Default::default(),
            currency: Currency::from("USD"),
            avg_px_open: 0.0,
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
        });

        strategy.handle_position_event(event);

        assert!(strategy.on_position_opened_called);
    }

    #[rstest]
    fn test_strategy_default_handlers_do_not_panic() {
        let mut strategy = create_test_strategy();

        strategy.on_order_initialized(Default::default());
        strategy.on_order_denied(Default::default());
        strategy.on_order_emulated(Default::default());
        strategy.on_order_released(Default::default());
        strategy.on_order_submitted(Default::default());
        strategy.on_order_rejected(Default::default());
        strategy.on_order_canceled(Default::default());
        strategy.on_order_expired(Default::default());
        strategy.on_order_triggered(Default::default());
        strategy.on_order_pending_update(Default::default());
        strategy.on_order_pending_cancel(Default::default());
        strategy.on_order_modify_rejected(Default::default());
        strategy.on_order_cancel_rejected(Default::default());
        strategy.on_order_updated(Default::default());
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
    fn test_handle_order_event_cancels_gtd_timer_on_filled() {
        use nautilus_model::events::OrderFilled;

        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        use nautilus_model::enums::{LiquiditySide, OrderType};

        let event = OrderEventAny::Filled(OrderFilled {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            venue_order_id: Default::default(),
            account_id: AccountId::from("ACC-001"),
            trade_id: Default::default(),
            position_id: Default::default(),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            last_qty: Default::default(),
            last_px: Default::default(),
            currency: Currency::from("USD"),
            liquidity_side: LiquiditySide::Taker,
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: false,
            commission: None,
        });
        strategy.handle_order_event(event);

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_handle_order_event_cancels_gtd_timer_on_canceled() {
        use nautilus_model::events::OrderCanceled;

        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        let event = OrderEventAny::Canceled(OrderCanceled {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            venue_order_id: Default::default(),
            account_id: Some(AccountId::from("ACC-001")),
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: 0,
        });
        strategy.handle_order_event(event);

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_handle_order_event_cancels_gtd_timer_on_rejected() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        let event = OrderEventAny::Rejected(OrderRejected {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            account_id: AccountId::from("ACC-001"),
            reason: "Test rejection".into(),
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: 0,
            due_post_only: 0,
        });
        strategy.handle_order_event(event);

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
    }

    #[rstest]
    fn test_handle_order_event_cancels_gtd_timer_on_expired() {
        let mut strategy = create_test_strategy();
        register_strategy(&mut strategy);

        let client_order_id = ClientOrderId::from("O-001");
        strategy
            .core
            .gtd_timers
            .insert(client_order_id, Ustr::from("GTD-EXPIRY:O-001"));

        let event = OrderEventAny::Expired(OrderExpired {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("TEST-001"),
            instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
            client_order_id,
            venue_order_id: Default::default(),
            account_id: Some(AccountId::from("ACC-001")),
            event_id: Default::default(),
            ts_event: Default::default(),
            ts_init: Default::default(),
            reconciliation: 0,
        });
        strategy.handle_order_event(event);

        assert!(!strategy.has_gtd_expiry_timer(&client_order_id));
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
}
