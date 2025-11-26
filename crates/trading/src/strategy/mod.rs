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
};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, PositionSide, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionEvent, PositionOpened,
    },
    identifiers::{ClientId, InstrumentId, PositionId, StrategyId},
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

        if order.emulation_trigger().is_some() {
            manager.send_emulator_command(TradingCommand::SubmitOrder(command));
        } else if order.exec_algorithm_id().is_some() {
            manager.send_algo_command(command, order.exec_algorithm_id().unwrap());
        } else {
            manager.send_risk_command(TradingCommand::SubmitOrder(command));
        }
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

            for order in order_list.orders.iter() {
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

        let has_emulated_order = order_list
            .orders
            .iter()
            .any(|o| o.emulation_trigger().is_some() || o.is_emulated());

        let Some(manager) = &mut core.order_manager else {
            anyhow::bail!("Strategy not registered: OrderManager missing");
        };

        if has_emulated_order {
            manager.send_emulator_command(TradingCommand::SubmitOrderList(command));
        } else {
            manager.send_risk_command(TradingCommand::SubmitOrderList(command));
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

        if order.emulation_trigger().is_some() {
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

        if order.emulation_trigger().is_some() || order.is_emulated() {
            manager.send_emulator_command(TradingCommand::CancelOrder(command));
        } else if order.exec_algorithm_id().is_some() {
            manager.send_risk_command(TradingCommand::CancelOrder(command));
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

        let open_count = open_orders.len();
        let emulated_count = emulated_orders.len();

        drop(cache);

        if open_count == 0 && emulated_count == 0 {
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
}
