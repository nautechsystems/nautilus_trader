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

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use ahash::{AHashMap, AHashSet};
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV, SEND},
    messages::{
        data::{
            DataCommand, SubscribeCommand, SubscribeQuotes, SubscribeTrades, UnsubscribeCommand,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
        execution::{
            BatchModifyOrders, CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder,
            SubmitOrderList, TradingCommand,
        },
    },
    msgbus::{
        self, MessagingSwitchboard, TypedHandler, TypedIntoHandler,
        switchboard::{
            get_event_orders_topic, get_order_cancels_topic, get_quotes_topic, get_trades_topic,
        },
    },
};
use nautilus_core::{UUID4, WeakCell};
use nautilus_model::{
    data::{OrderBookDeltas, QuoteTick, TradeTick},
    enums::{ContingencyType, OrderSide, OrderSideSpecified, OrderStatus, OrderType, TriggerType},
    events::{OrderCanceled, OrderEmulated, OrderEventAny, OrderReleased, OrderUpdated},
    identifiers::{ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId},
    instruments::Instrument,
    orders::{LimitOrder, MarketOrder, Order, OrderAny},
    types::{Price, Quantity},
};
use ustr::Ustr;

use super::handlers::OrderEmulatorOnEventHandler;
use crate::{
    matching_core::{MatchAction, OrderMatchingCore, RestingOrder},
    order_manager::{OrderManagerAction, manager::OrderManager},
    trailing::trailing_stop_calculate,
};

pub struct OrderEmulator {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    manager: OrderManager,
    matching_cores: AHashMap<InstrumentId, OrderMatchingCore>,
    subscribed_quotes: AHashSet<InstrumentId>,
    subscribed_trades: AHashSet<InstrumentId>,
    subscribed_strategies: AHashSet<StrategyId>,
    monitored_positions: AHashSet<PositionId>,
    quote_tick_handler: Option<TypedHandler<QuoteTick>>,
    trade_tick_handler: Option<TypedHandler<TradeTick>>,
    quote_handlers: AHashMap<InstrumentId, TypedHandler<QuoteTick>>,
    trade_handlers: AHashMap<InstrumentId, TypedHandler<TradeTick>>,
    on_event_handler: Option<TypedHandler<OrderEventAny>>,
}

impl Debug for OrderEmulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderEmulator))
            .field("cores", &self.matching_cores.len())
            .field("subscribed_quotes", &self.subscribed_quotes.len())
            .field("subscribed_trades", &self.subscribed_trades.len())
            .field("subscribed_strategies", &self.subscribed_strategies.len())
            .finish()
    }
}

impl OrderEmulator {
    pub fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        let active_local = true;
        let manager = OrderManager::new(clock.clone(), cache.clone(), active_local);

        Self {
            clock,
            cache,
            manager,
            matching_cores: AHashMap::new(),
            subscribed_quotes: AHashSet::new(),
            subscribed_trades: AHashSet::new(),
            subscribed_strategies: AHashSet::new(),
            monitored_positions: AHashSet::new(),
            quote_tick_handler: None,
            trade_tick_handler: None,
            quote_handlers: AHashMap::new(),
            trade_handlers: AHashMap::new(),
            on_event_handler: None,
        }
    }

    pub fn register_msgbus_handlers(emulator: &Rc<RefCell<Self>>) {
        let weak = WeakCell::from(Rc::downgrade(emulator));

        let execute_weak = weak.clone();
        let execute_handler = TypedIntoHandler::from(move |cmd: TradingCommand| {
            if let Some(emulator) = execute_weak.upgrade() {
                emulator.borrow_mut().execute(cmd);
            }
        });
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::order_emulator_execute(),
            execute_handler,
        );

        let quote_weak = weak.clone();
        let quote_handler = TypedHandler::from(move |quote: &QuoteTick| {
            if let Some(emulator) = quote_weak.upgrade() {
                emulator.borrow_mut().on_quote_tick(*quote);
            }
        });

        let trade_weak = weak.clone();
        let trade_handler = TypedHandler::from(move |trade: &TradeTick| {
            if let Some(emulator) = trade_weak.upgrade() {
                emulator.borrow_mut().on_trade_tick(*trade);
            }
        });

        let on_event_handler = TypedHandler::new(OrderEmulatorOnEventHandler::new(
            Ustr::from(UUID4::new().as_str()),
            weak,
        ));

        let mut emulator = emulator.borrow_mut();
        emulator.quote_tick_handler = Some(quote_handler);
        emulator.trade_tick_handler = Some(trade_handler);
        emulator.on_event_handler = Some(on_event_handler);
    }

    pub fn set_on_event_handler(&mut self, handler: TypedHandler<OrderEventAny>) {
        self.on_event_handler = Some(handler);
    }

    /// Caches a submit order command for emulation tracking.
    pub fn cache_submit_order_command(&mut self, command: SubmitOrder) {
        self.manager.cache_submit_order_command(command);
    }

    /// Subscribes to quote data for the given instrument.
    fn subscribe_quotes_for_instrument(&mut self, instrument_id: InstrumentId) -> bool {
        if self.quote_handlers.contains_key(&instrument_id) {
            log::warn!("OrderEmulator attempted duplicate quote subscription for {instrument_id}");
            return true;
        }

        let Some(handler) = self.quote_tick_handler.clone() else {
            log::warn!("Cannot subscribe to quotes: msgbus handlers not registered");
            return false;
        };

        let topic = get_quotes_topic(instrument_id);
        self.quote_handlers.insert(instrument_id, handler.clone());
        msgbus::subscribe_quotes(topic.into(), handler, None);
        self.send_data_command(DataCommand::Subscribe(SubscribeCommand::Quotes(
            SubscribeQuotes::new(
                instrument_id,
                None,
                Some(instrument_id.venue),
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                None,
                None,
            ),
        )));

        true
    }

    /// Subscribes to trade data for the given instrument.
    fn subscribe_trades_for_instrument(&mut self, instrument_id: InstrumentId) -> bool {
        if self.trade_handlers.contains_key(&instrument_id) {
            log::warn!("OrderEmulator attempted duplicate trade subscription for {instrument_id}");
            return true;
        }

        let Some(handler) = self.trade_tick_handler.clone() else {
            log::warn!("Cannot subscribe to trades: msgbus handlers not registered");
            return false;
        };

        let topic = get_trades_topic(instrument_id);
        self.trade_handlers.insert(instrument_id, handler.clone());
        msgbus::subscribe_trades(topic.into(), handler, None);
        self.send_data_command(DataCommand::Subscribe(SubscribeCommand::Trades(
            SubscribeTrades::new(
                instrument_id,
                None,
                Some(instrument_id.venue),
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                None,
                None,
            ),
        )));

        true
    }

    #[must_use]
    pub fn subscribed_quotes(&self) -> Vec<InstrumentId> {
        let mut quotes: Vec<InstrumentId> = self.subscribed_quotes.iter().copied().collect();
        quotes.sort();
        quotes
    }

    #[must_use]
    pub fn subscribed_trades(&self) -> Vec<InstrumentId> {
        let mut trades: Vec<_> = self.subscribed_trades.iter().copied().collect();
        trades.sort();
        trades
    }

    #[must_use]
    pub fn subscribed_strategy_count(&self) -> usize {
        self.subscribed_strategies.len()
    }

    #[must_use]
    pub fn monitored_position_count(&self) -> usize {
        self.monitored_positions.len()
    }

    #[must_use]
    pub fn get_submit_order_commands(&self) -> AHashMap<ClientOrderId, SubmitOrder> {
        self.manager.get_submit_order_commands()
    }

    #[must_use]
    pub fn get_matching_core(&self, instrument_id: &InstrumentId) -> Option<OrderMatchingCore> {
        self.matching_cores.get(instrument_id).cloned()
    }

    pub fn start(&mut self) {
        if let Err(e) = self.on_start() {
            log::error!("{e}");
        }

        log::info!("Started");
    }

    pub fn stop(&self) {
        self.on_stop();

        log::info!("Stopped");
    }

    pub fn reset(&mut self) {
        self.on_reset();

        log::info!("Reset");
    }

    pub fn dispose(&mut self) {
        self.on_dispose();

        log::info!("Disposed");
    }

    /// Reactivates emulated orders from cache on start.
    ///
    /// # Errors
    ///
    /// Returns an error if processing fails.
    ///
    pub fn on_start(&mut self) -> anyhow::Result<()> {
        let emulated_orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_emulated(None, None, None, None, None)
            .into_iter()
            .map(|o| o.clone())
            .collect();

        if emulated_orders.is_empty() {
            log::debug!("No emulated orders to reactivate");
            return Ok(());
        }

        for order in emulated_orders {
            if !matches!(
                order.status(),
                OrderStatus::Initialized | OrderStatus::Emulated
            ) {
                continue; // No longer emulated
            }

            if let Some(parent_order_id) = &order.parent_order_id() {
                let parent_order = if let Some(order) = self.cache.borrow().order(parent_order_id) {
                    order.clone()
                } else {
                    log::error!("Cannot handle order: parent {parent_order_id} not found");
                    continue;
                };

                let is_position_closed = parent_order
                    .position_id()
                    .is_some_and(|id| self.cache.borrow().is_position_closed(&id));
                if parent_order.is_closed() && is_position_closed {
                    let actions = self.manager.cancel_order(&order);
                    self.dispatch_manager_actions(actions);
                    continue; // Parent already closed
                }

                if parent_order.contingency_type() == Some(ContingencyType::Oto)
                    && (parent_order.is_active_local()
                        || parent_order.filled_qty() == Quantity::zero(0))
                {
                    continue; // Process contingency order later once parent triggered
                }
            }

            let position_id = self
                .cache
                .borrow()
                .position_id(&order.client_order_id())
                .copied();
            let client_id = self
                .cache
                .borrow()
                .client_id(&order.client_order_id())
                .copied();

            let command = SubmitOrder::new(
                order.trader_id(),
                client_id,
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                order.init_event().clone(),
                order.exec_algorithm_id(),
                position_id,
                None, // params
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                None, // correlation_id
            );

            self.manager.cache_submit_order_command(command.clone());
            self.handle_submit_order(&command);
        }

        Ok(())
    }

    pub fn on_event(&mut self, event: &OrderEventAny) {
        log::info!("{RECV}{EVT} {event}");

        let actions = self.manager.handle_event(event);
        self.dispatch_manager_actions(actions);

        if let Some(order) = self.cache.borrow().order(&event.client_order_id())
            && order.is_closed()
            && let Some(matching_core) = self.matching_cores.get_mut(&order.instrument_id())
            && let Err(e) = matching_core.delete_order(event.client_order_id())
        {
            log::debug!("Error deleting order: {e}");
        }
        // else: Order not in cache yet
    }

    pub const fn on_stop(&self) {}

    pub fn on_reset(&mut self) {
        self.manager.reset();
        self.matching_cores.clear();
        self.unsubscribe_all_market_data();
        self.unsubscribe_strategy_order_events();
        self.monitored_positions.clear();
    }

    pub fn on_dispose(&mut self) {
        self.on_reset();
    }

    fn unsubscribe_all_market_data(&mut self) {
        let quote_instrument_ids: Vec<_> = self.subscribed_quotes.drain().collect();
        for instrument_id in quote_instrument_ids {
            self.unsubscribe_quotes_for_instrument(instrument_id);
        }

        let trade_instrument_ids: Vec<_> = self.subscribed_trades.drain().collect();
        for instrument_id in trade_instrument_ids {
            self.unsubscribe_trades_for_instrument(instrument_id);
        }
    }

    fn unsubscribe_quotes_for_instrument(&mut self, instrument_id: InstrumentId) {
        if let Some(handler) = self.quote_handlers.remove(&instrument_id) {
            let topic = get_quotes_topic(instrument_id);
            msgbus::unsubscribe_quotes(topic.into(), &handler);
        }

        self.send_data_command(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
            UnsubscribeQuotes::new(
                instrument_id,
                None,
                Some(instrument_id.venue),
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                None,
                None,
            ),
        )));
    }

    fn unsubscribe_trades_for_instrument(&mut self, instrument_id: InstrumentId) {
        if let Some(handler) = self.trade_handlers.remove(&instrument_id) {
            let topic = get_trades_topic(instrument_id);
            msgbus::unsubscribe_trades(topic.into(), &handler);
        }

        self.send_data_command(DataCommand::Unsubscribe(UnsubscribeCommand::Trades(
            UnsubscribeTrades::new(
                instrument_id,
                None,
                Some(instrument_id.venue),
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                None,
                None,
            ),
        )));
    }

    fn unsubscribe_strategy_order_events(&mut self) {
        let strategy_ids: Vec<_> = self.subscribed_strategies.drain().collect();
        let Some(handler) = &self.on_event_handler else {
            return;
        };

        for strategy_id in strategy_ids {
            msgbus::unsubscribe_order_events(format!("events.order.{strategy_id}").into(), handler);
        }
    }

    pub fn execute(&mut self, command: TradingCommand) {
        log::info!("{RECV}{CMD} {command}");

        match command {
            TradingCommand::SubmitOrder(command) => self.handle_submit_order(&command),
            TradingCommand::SubmitOrderList(ref command) => self.handle_submit_order_list(command),
            TradingCommand::ModifyOrder(ref command) => self.handle_modify_order(command),
            TradingCommand::ModifyOrders(ref command) => self.handle_batch_modify_orders(command),
            TradingCommand::CancelOrder(command) => self.handle_cancel_order(command),
            TradingCommand::CancelAllOrders(ref command) => self.handle_cancel_all_orders(command),
            _ => log::error!("Cannot handle command: unrecognized {command:?}"),
        }
    }

    fn dispatch_manager_actions(&mut self, actions: Vec<OrderManagerAction>) {
        for action in actions {
            self.dispatch_manager_action(action);
        }
    }

    fn dispatch_manager_action(&mut self, action: OrderManagerAction) {
        match action {
            OrderManagerAction::PublishInitialized(event) => publish_order_event(&event),
            OrderManagerAction::SubmitToEmulator(command) => self.handle_submit_order(&command),
            OrderManagerAction::SubmitToRisk(command) => {
                self.send_risk_command(TradingCommand::SubmitOrder(command));
            }
            OrderManagerAction::SubmitToAlgorithm {
                command,
                exec_algorithm_id,
            } => self.send_algo_command(command, exec_algorithm_id),
            OrderManagerAction::CancelLocal(order) => self.cancel_order(&order),
            OrderManagerAction::ModifyLocalQuantity {
                mut order,
                quantity,
            } => {
                self.update_order(&mut order, quantity);
            }
        }
    }

    fn create_matching_core(
        &mut self,
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> OrderMatchingCore {
        let matching_core = OrderMatchingCore::new(instrument_id, price_increment);
        self.matching_cores
            .insert(instrument_id, matching_core.clone());
        log::info!("Creating matching core for {instrument_id:?}");
        matching_core
    }

    /// # Panics
    ///
    /// Panics if the emulation trigger type is `NoTrigger` or if order not in cache.
    pub fn handle_submit_order(&mut self, command: &SubmitOrder) {
        let client_order_id = command.client_order_id;

        let mut order = self
            .cache
            .borrow()
            .order(&client_order_id)
            .map(|o| o.clone())
            .expect("order must exist in cache");

        let emulation_trigger = order.emulation_trigger();

        if !self
            .manager
            .get_submit_order_commands()
            .contains_key(&client_order_id)
        {
            self.manager.cache_submit_order_command(command.clone());
        }

        assert_ne!(
            emulation_trigger,
            Some(TriggerType::NoTrigger),
            "order.emulation_trigger must not be TriggerType::NoTrigger"
        );

        if !matches!(
            emulation_trigger,
            Some(TriggerType::Default | TriggerType::BidAsk | TriggerType::LastPrice)
        ) {
            log::error!("Cannot emulate order: `TriggerType` {emulation_trigger:?} not supported");
            let actions = self.manager.cancel_order(&order);
            self.dispatch_manager_actions(actions);
            return;
        }
        let strategy_id = command.strategy_id;
        let position_id = command.position_id;

        // Get or create matching core
        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());

        let matching_core = self.matching_cores.get(&trigger_instrument_id).cloned();

        let mut matching_core = if let Some(core) = matching_core {
            core
        } else {
            // Handle synthetic instruments
            let (instrument_id, price_increment) = if trigger_instrument_id.is_synthetic() {
                let synthetic = self
                    .cache
                    .borrow()
                    .synthetic(&trigger_instrument_id)
                    .cloned();

                if let Some(synthetic) = synthetic {
                    (synthetic.id, synthetic.price_increment)
                } else {
                    log::error!(
                        "Cannot emulate order: no synthetic instrument {trigger_instrument_id} for trigger"
                    );
                    let actions = self.manager.cancel_order(&order);
                    self.dispatch_manager_actions(actions);
                    return;
                }
            } else {
                let instrument = self
                    .cache
                    .borrow()
                    .instrument(&trigger_instrument_id)
                    .cloned();

                if let Some(instrument) = instrument {
                    (instrument.id(), instrument.price_increment())
                } else {
                    log::error!(
                        "Cannot emulate order: no instrument {trigger_instrument_id} for trigger"
                    );
                    let actions = self.manager.cancel_order(&order);
                    self.dispatch_manager_actions(actions);
                    return;
                }
            };

            self.create_matching_core(instrument_id, price_increment)
        };

        // Update trailing stop
        if matches!(
            order.order_type(),
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
        ) {
            self.update_trailing_stop_order(&mut order);
            if order.trigger_price().is_none() {
                log::error!(
                    "Cannot handle trailing stop order with no trigger_price and no market updates"
                );

                let actions = self.manager.cancel_order(&order);
                self.dispatch_manager_actions(actions);
                return;
            }
        }

        // Check if immediately marketable
        let match_info = RestingOrder::new(
            order.client_order_id(),
            order.order_side().as_specified(),
            order.order_type(),
            order.trigger_price(),
            order.price(),
            true, // is_activated
        );
        matching_core.match_order(&match_info);

        // Handle data subscriptions
        match emulation_trigger.unwrap() {
            TriggerType::Default | TriggerType::BidAsk => {
                if !self.subscribed_quotes.contains(&trigger_instrument_id)
                    && self.subscribe_quotes_for_instrument(trigger_instrument_id)
                {
                    self.subscribed_quotes.insert(trigger_instrument_id);
                }
            }
            TriggerType::LastPrice => {
                if !self.subscribed_trades.contains(&trigger_instrument_id)
                    && self.subscribe_trades_for_instrument(trigger_instrument_id)
                {
                    self.subscribed_trades.insert(trigger_instrument_id);
                }
            }
            _ => {
                log::error!("Invalid TriggerType: {emulation_trigger:?}");
                return;
            }
        }

        // Check if order was already released
        if !self
            .manager
            .get_submit_order_commands()
            .contains_key(&order.client_order_id())
        {
            return; // Already released
        }

        // Hold in matching core
        matching_core.add_order(match_info);

        // Generate emulated event if needed
        if order.status() == OrderStatus::Initialized {
            let event = OrderEmulated::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                self.clock.borrow().timestamp_ns(),
            );

            let event = OrderEventAny::Emulated(event);

            order = match self.cache.borrow_mut().update_order(&event) {
                Ok(order) => order,
                Err(e) => {
                    log::error!("Cannot apply order event: {e:?}");
                    return;
                }
            };

            self.send_risk_event(event.clone());

            msgbus::publish_order_event(
                format!("events.order.{}", order.strategy_id()).into(),
                &event,
            );
        }

        self.check_monitoring(strategy_id, position_id);

        // Since we are cloning the matching core, we need to insert it back into the original hashmap
        self.matching_cores
            .insert(trigger_instrument_id, matching_core);

        log::info!("Emulating {order}");
    }

    fn handle_submit_order_list(&mut self, command: &SubmitOrderList) {
        self.check_monitoring(command.strategy_id, command.position_id);

        let orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_for_ids(&command.order_list.client_order_ids, &command);

        for order in &orders {
            if let Some(parent_order_id) = order.parent_order_id() {
                let cache = self.cache.borrow();
                let parent_order = if let Some(parent_order) = cache.order(&parent_order_id) {
                    parent_order
                } else {
                    log::error!("Parent order for {} not found", order.client_order_id());
                    continue;
                };

                if parent_order.contingency_type() == Some(ContingencyType::Oto) {
                    continue; // Process contingency order later once parent triggered
                }
            }

            match self.manager.create_new_submit_order(
                order,
                command.position_id,
                command.client_id,
                command.correlation_id,
            ) {
                Ok(actions) => self.dispatch_manager_actions(actions),
                Err(e) => log::error!("Error creating new submit order: {e}"),
            }
        }
    }

    fn handle_modify_order(&mut self, command: &ModifyOrder) {
        if let Some(order) = self.cache.borrow().order(&command.client_order_id) {
            let price = match command.price {
                Some(price) => Some(price),
                None => order.price(),
            };

            let trigger_price = match command.trigger_price {
                Some(trigger_price) => Some(trigger_price),
                None => order.trigger_price(),
            };

            let ts_now = self.clock.borrow().timestamp_ns();
            let event = OrderUpdated::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                command.quantity.unwrap_or(order.quantity()),
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                order.venue_order_id(),
                order.account_id(),
                price,
                trigger_price,
                None,
                order.is_quote_quantity(),
            );

            self.send_exec_event(OrderEventAny::Updated(event));

            let trigger_instrument_id = order
                .trigger_instrument_id()
                .unwrap_or_else(|| order.instrument_id());

            if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
                let match_info = RestingOrder::new(
                    order.client_order_id(),
                    order.order_side().as_specified(),
                    order.order_type(),
                    trigger_price,
                    price,
                    true, // is_activated
                );
                matching_core.match_order(&match_info);
            } else {
                log::error!(
                    "Cannot handle `ModifyOrder`: no matching core for trigger instrument {trigger_instrument_id}"
                );
            }
        } else {
            log::error!("Cannot modify order: {} not found", command.client_order_id);
        }
    }

    fn handle_batch_modify_orders(&mut self, command: &BatchModifyOrders) {
        for modify in &command.modifies {
            self.handle_modify_order(modify);
        }
    }

    pub fn handle_cancel_order(&mut self, command: CancelOrder) {
        let order = if let Some(order) = self.cache.borrow().order(&command.client_order_id) {
            order.clone()
        } else {
            log::error!("Cannot cancel order: {} not found", command.client_order_id);
            return;
        };

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());

        let matching_core = if let Some(core) = self.matching_cores.get(&trigger_instrument_id) {
            core
        } else {
            let actions = self.manager.cancel_order(&order);
            self.dispatch_manager_actions(actions);
            return;
        };

        if !matching_core.order_exists(order.client_order_id())
            && order.is_open()
            && !order.is_pending_cancel()
        {
            // Order not held in the emulator
            self.send_exec_command(TradingCommand::CancelOrder(command));
        } else {
            let actions = self.manager.cancel_order(&order);
            self.dispatch_manager_actions(actions);
        }
    }

    fn handle_cancel_all_orders(&mut self, command: &CancelAllOrders) {
        let instrument_id = command.instrument_id;
        let Some(matching_core) = self.matching_cores.get(&instrument_id) else {
            return; // No orders to cancel
        };

        // Borrow the iterator and collect just the IDs (8 bytes each) instead
        // of full RestingOrder snapshots (72 bytes each). The borrow on
        // matching_core ends here so the manager mutation can proceed.
        let ids_to_cancel: Vec<ClientOrderId> = match command.order_side {
            OrderSide::NoOrderSide => matching_core
                .iter_orders()
                .map(|o| o.client_order_id)
                .collect(),
            OrderSide::Buy => matching_core
                .iter_bid_orders()
                .map(|o| o.client_order_id)
                .collect(),
            OrderSide::Sell => matching_core
                .iter_ask_orders()
                .map(|o| o.client_order_id)
                .collect(),
        };

        for id in ids_to_cancel {
            let order = self.cache.borrow().order(&id).map(|o| o.clone());
            if let Some(order) = order {
                let actions = self.manager.cancel_order(&order);
                self.dispatch_manager_actions(actions);
            }
        }
    }

    pub fn update_order(&mut self, order: &mut OrderAny, new_quantity: Quantity) {
        log::info!(
            "Updating order {} quantity to {new_quantity}",
            order.client_order_id(),
        );

        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderUpdated::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            new_quantity,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            None,
            order.account_id(),
            None,
            None,
            None,
            order.is_quote_quantity(),
        );

        let event = OrderEventAny::Updated(event);

        *order = match self.cache.borrow_mut().update_order(&event) {
            Ok(order) => order,
            Err(e) => {
                log::error!("Cannot apply order event: {e:?}");
                return;
            }
        };

        self.send_risk_event(event);
    }

    pub fn on_order_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        log::debug!("Processing {deltas:?}");

        let instrument_id = &deltas.instrument_id;
        if let Some(matching_core) = self.matching_cores.get_mut(instrument_id) {
            if let Some(book) = self.cache.borrow().order_book(instrument_id) {
                let best_bid = book.best_bid_price();
                let best_ask = book.best_ask_price();

                if let Some(best_bid) = best_bid {
                    matching_core.set_bid_raw(best_bid);
                }

                if let Some(best_ask) = best_ask {
                    matching_core.set_ask_raw(best_ask);
                }
            } else {
                log::error!(
                    "Cannot handle `OrderBookDeltas`: no book being maintained for {}",
                    deltas.instrument_id
                );
            }

            self.iterate_orders(instrument_id);
        } else {
            log::error!(
                "Cannot handle `OrderBookDeltas`: no matching core for instrument {}",
                deltas.instrument_id
            );
        }
    }

    pub fn on_quote_tick(&mut self, quote: QuoteTick) {
        log::debug!("Processing {quote}:?");

        let instrument_id = &quote.instrument_id;
        if let Some(matching_core) = self.matching_cores.get_mut(instrument_id) {
            matching_core.set_bid_raw(quote.bid_price);
            matching_core.set_ask_raw(quote.ask_price);

            self.iterate_orders(instrument_id);
        } else {
            log::error!(
                "Cannot handle `QuoteTick`: no matching core for instrument {}",
                quote.instrument_id
            );
        }
    }

    pub fn on_trade_tick(&mut self, trade: TradeTick) {
        log::debug!("Processing {trade:?}");

        let instrument_id = &trade.instrument_id;
        if let Some(matching_core) = self.matching_cores.get_mut(instrument_id) {
            matching_core.set_last_raw(trade.price);

            if !self.subscribed_quotes.contains(instrument_id) {
                matching_core.set_bid_raw(trade.price);
                matching_core.set_ask_raw(trade.price);
            }

            self.iterate_orders(instrument_id);
        } else {
            log::error!(
                "Cannot handle `TradeTick`: no matching core for instrument {}",
                trade.instrument_id
            );
        }
    }

    fn iterate_orders(&mut self, instrument_id: &InstrumentId) {
        // Process bid actions before ask actions so cross-side
        // contingencies (OCO/OUO) mutate state between sides
        let bid_actions = if let Some(matching_core) = self.matching_cores.get_mut(instrument_id) {
            matching_core.iterate_bids()
        } else {
            log::error!("Cannot iterate orders: no matching core for instrument {instrument_id}");
            return;
        };

        for action in bid_actions {
            match action {
                MatchAction::FillLimit(id) => self.fill_limit_order(id),
                MatchAction::TriggerStop(id) => self.trigger_stop_order(id),
            }
        }

        let ask_actions = if let Some(matching_core) = self.matching_cores.get_mut(instrument_id) {
            matching_core.iterate_asks()
        } else {
            return;
        };

        for action in ask_actions {
            match action {
                MatchAction::FillLimit(id) => self.fill_limit_order(id),
                MatchAction::TriggerStop(id) => self.trigger_stop_order(id),
            }
        }

        // Re-snapshot orders after actions to avoid stale trailing stop updates
        let orders = if let Some(matching_core) = self.matching_cores.get(instrument_id) {
            matching_core.get_orders()
        } else {
            return;
        };

        for match_info in orders {
            if !matches!(
                match_info.order_type,
                OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
            ) {
                continue;
            }

            let mut order = match self
                .cache
                .borrow()
                .order(&match_info.client_order_id)
                .map(|o| o.clone())
            {
                Some(order) => order,
                None => continue,
            };

            if order.is_closed() {
                continue;
            }

            self.update_trailing_stop_order(&mut order);
        }
    }

    pub fn cancel_order(&mut self, order: &OrderAny) {
        log::info!("Canceling order {}", order.client_order_id());

        let mut order = order.clone();
        order.set_emulation_trigger(Some(TriggerType::NoTrigger));

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());

        if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id)
            && let Err(e) = matching_core.delete_order(order.client_order_id())
        {
            log::debug!("Cannot delete order: {e:?}");
        }

        self.manager
            .pop_submit_order_command(order.client_order_id());

        self.cache
            .borrow_mut()
            .update_order_pending_cancel_local(&order);

        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            order.venue_order_id(),
            order.account_id(),
        );

        let event = OrderEventAny::Canceled(event);
        if let Err(e) = self.cache.borrow_mut().update_order(&event) {
            log::error!("Failed to apply order event: {e}");
            return;
        }

        self.send_portfolio_order_event(event.clone());
        publish_order_event(&event);
    }

    fn check_monitoring(&mut self, strategy_id: StrategyId, position_id: Option<PositionId>) {
        if !self.subscribed_strategies.contains(&strategy_id) {
            // Subscribe to strategy order events
            if let Some(handler) = &self.on_event_handler {
                msgbus::subscribe_order_events(
                    format!("events.order.{strategy_id}").into(),
                    handler.clone(),
                    None,
                );
                self.subscribed_strategies.insert(strategy_id);
                log::info!("Subscribed to strategy {strategy_id} order events");
            }
        }

        if let Some(position_id) = position_id
            && !self.monitored_positions.contains(&position_id)
        {
            self.monitored_positions.insert(position_id);
        }
    }

    /// Validates market data availability for order release.
    ///
    /// Returns `Some(released_price)` if market data is available, `None` otherwise.
    /// Logs appropriate warnings when market data is not yet available.
    ///
    /// Does NOT pop the submit order command - caller must do that and handle missing command
    /// according to their contract (panic for market orders, return for limit orders).
    fn validate_release(
        &self,
        order: &OrderAny,
        matching_core: &OrderMatchingCore,
        trigger_instrument_id: InstrumentId,
    ) -> Option<Price> {
        let released_price = match order.order_side_specified() {
            OrderSideSpecified::Buy => matching_core.ask,
            OrderSideSpecified::Sell => matching_core.bid,
        };

        if released_price.is_none() {
            log::warn!(
                "Cannot release order {} yet: no market data available for {trigger_instrument_id}, will retry on next update",
                order.client_order_id(),
            );
            return None;
        }

        Some(released_price.unwrap())
    }

    /// # Panics
    ///
    /// Panics if the order type is invalid for a stop order.
    pub fn trigger_stop_order(&mut self, client_order_id: ClientOrderId) {
        let order = match self
            .cache
            .borrow()
            .order(&client_order_id)
            .map(|o| o.clone())
        {
            Some(order) => order,
            None => {
                log::error!(
                    "Cannot trigger stop order: order {client_order_id} not found in cache"
                );
                return;
            }
        };

        match order.order_type() {
            OrderType::StopLimit | OrderType::LimitIfTouched | OrderType::TrailingStopLimit => {
                self.fill_limit_order(client_order_id);
            }
            OrderType::Market
            | OrderType::MarketIfTouched
            | OrderType::StopMarket
            | OrderType::TrailingStopMarket => self.fill_market_order(client_order_id),
            _ => panic!("invalid `OrderType`, was {}", order.order_type()),
        }
    }

    /// # Panics
    ///
    /// Panics if a limit order has no price.
    pub fn fill_limit_order(&mut self, client_order_id: ClientOrderId) {
        let order = match self
            .cache
            .borrow()
            .order(&client_order_id)
            .map(|o| o.clone())
        {
            Some(order) => order,
            None => {
                log::error!("Cannot fill limit order: order {client_order_id} not found in cache");
                return;
            }
        };

        if matches!(order.order_type(), OrderType::Limit) {
            self.fill_market_order(client_order_id);
            return;
        }

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());

        let matching_core = match self.matching_cores.get(&trigger_instrument_id) {
            Some(core) => core,
            None => {
                log::error!(
                    "Cannot fill limit order: no matching core for instrument {trigger_instrument_id}"
                );
                return; // Order stays queued for retry
            }
        };

        let released_price =
            match self.validate_release(&order, matching_core, trigger_instrument_id) {
                Some(price) => price,
                None => return, // Order stays queued for retry
            };

        let command = match self
            .manager
            .pop_submit_order_command(order.client_order_id())
        {
            Some(command) => command,
            None => return, // Order already released
        };

        if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
            if let Err(e) = matching_core.delete_order(client_order_id) {
                log::debug!("Error deleting order: {e:?}");
            }

            let emulation_trigger = TriggerType::NoTrigger;

            // Transform order
            let mut transformed = if let Ok(transformed) = LimitOrder::new_checked(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                order.order_side(),
                order.quantity(),
                order.price().unwrap(),
                order.time_in_force(),
                order.expire_time(),
                order.is_post_only(),
                order.is_reduce_only(),
                order.is_quote_quantity(),
                order.display_qty(),
                Some(emulation_trigger),
                Some(trigger_instrument_id),
                order.contingency_type(),
                order.order_list_id(),
                order.linked_order_ids().map(Vec::from),
                order.parent_order_id(),
                order.exec_algorithm_id(),
                order.exec_algorithm_params().cloned(),
                order.exec_spawn_id(),
                order.tags().map(Vec::from),
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
            ) {
                transformed
            } else {
                log::error!("Cannot create limit order");
                return;
            };
            transformed.liquidity_side = order.liquidity_side();

            // TODO: fix
            // let triggered_price = order.trigger_price();
            // if triggered_price.is_some() {
            //     transformed.trigger_price() = (triggered_price.unwrap());
            // }

            let original_events = order.events();

            for event in original_events {
                transformed.events.insert(0, event.clone());
            }

            let add_result = {
                let mut cache = self.cache.borrow_mut();
                cache.add_order(
                    OrderAny::Limit(transformed.clone()),
                    command.position_id,
                    command.client_id,
                    true,
                )
            };

            if let Err(e) = add_result {
                log::error!("Failed to add order: {e}");
            } else {
                msgbus::publish_order_event(
                    format!("events.order.{}", order.strategy_id()).into(),
                    transformed.last_event(),
                );
            }

            let event = OrderReleased::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                released_price,
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                self.clock.borrow().timestamp_ns(),
            );

            let event = OrderEventAny::Released(event);

            let transformed = match self.cache.borrow_mut().update_order(&event) {
                Ok(order) => order,
                Err(e) => {
                    log::error!("Failed to apply order event: {e}");
                    return;
                }
            };

            self.send_risk_event(event.clone());

            log::info!("Releasing order {}", order.client_order_id());

            // Publish event
            msgbus::publish_order_event(
                format!("events.order.{}", transformed.strategy_id()).into(),
                &event,
            );

            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                self.send_algo_command(command, exec_algorithm_id);
            } else {
                self.send_exec_command(TradingCommand::SubmitOrder(command));
            }
        }
    }

    /// # Panics
    ///
    /// Panics if a market order command is missing.
    pub fn fill_market_order(&mut self, client_order_id: ClientOrderId) {
        let mut order = match self
            .cache
            .borrow()
            .order(&client_order_id)
            .map(|o| o.clone())
        {
            Some(order) => order,
            None => {
                log::error!("Cannot fill market order: order {client_order_id} not found in cache");
                return;
            }
        };

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());

        let matching_core = match self.matching_cores.get(&trigger_instrument_id) {
            Some(core) => core,
            None => {
                log::error!(
                    "Cannot fill market order: no matching core for instrument {trigger_instrument_id}"
                );
                return; // Order stays queued for retry
            }
        };

        let released_price =
            match self.validate_release(&order, matching_core, trigger_instrument_id) {
                Some(price) => price,
                None => return, // Order stays queued for retry
            };

        let command = self
            .manager
            .pop_submit_order_command(order.client_order_id())
            .expect("invalid operation `fill_market_order` with no command");

        if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
            if let Err(e) = matching_core.delete_order(client_order_id) {
                log::debug!("Cannot delete order: {e:?}");
            }

            order.set_emulation_trigger(Some(TriggerType::NoTrigger));

            // Transform order
            let mut transformed = MarketOrder::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                order.order_side(),
                order.quantity(),
                order.time_in_force(),
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                order.is_reduce_only(),
                order.is_quote_quantity(),
                order.contingency_type(),
                order.order_list_id(),
                order.linked_order_ids().map(Vec::from),
                order.parent_order_id(),
                order.exec_algorithm_id(),
                order.exec_algorithm_params().cloned(),
                order.exec_spawn_id(),
                order.tags().map(Vec::from),
            );

            let original_events = order.events();

            for event in original_events {
                transformed.events.insert(0, event.clone());
            }

            let add_result = {
                let mut cache = self.cache.borrow_mut();
                cache.add_order(
                    OrderAny::Market(transformed.clone()),
                    command.position_id,
                    command.client_id,
                    true,
                )
            };

            if let Err(e) = add_result {
                log::error!("Failed to add order: {e}");
            } else {
                msgbus::publish_order_event(
                    format!("events.order.{}", order.strategy_id()).into(),
                    transformed.last_event(),
                );
            }

            let ts_now = self.clock.borrow().timestamp_ns();
            let event = OrderReleased::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                released_price,
                UUID4::new(),
                ts_now,
                ts_now,
            );

            let event = OrderEventAny::Released(event);

            if let Err(e) = self.cache.borrow_mut().update_order(&event) {
                log::error!("Failed to apply order event: {e}");
                return;
            }
            self.send_risk_event(event.clone());

            log::info!("Releasing order {}", order.client_order_id());

            // Publish event
            msgbus::publish_order_event(
                format!("events.order.{}", order.strategy_id()).into(),
                &event,
            );

            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                self.send_algo_command(command, exec_algorithm_id);
            } else {
                self.send_exec_command(TradingCommand::SubmitOrder(command));
            }
        }
    }

    fn update_trailing_stop_order(&mut self, order: &mut OrderAny) {
        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());
        let Some(matching_core) = self.matching_cores.get(&trigger_instrument_id) else {
            log::error!(
                "Cannot update trailing-stop order: no matching core for instrument {trigger_instrument_id}"
            );
            return;
        };

        let mut bid = matching_core.bid;
        let mut ask = matching_core.ask;
        let mut last = matching_core.last;
        let price_increment = matching_core.price_increment;
        let instrument_id = matching_core.instrument_id;

        if bid.is_none() || ask.is_none() || last.is_none() {
            if let Some(q) = self.cache.borrow().quote(&instrument_id) {
                bid.get_or_insert(q.bid_price);
                ask.get_or_insert(q.ask_price);
            }

            if let Some(t) = self.cache.borrow().trade(&instrument_id) {
                last.get_or_insert(t.price);
            }
        }

        let (new_trigger_px, new_limit_px) = match trailing_stop_calculate(
            price_increment,
            order.trigger_price(),
            order.activation_price(),
            order,
            bid,
            ask,
            last,
        ) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("Cannot calculate trailing-stop update: {e}");
                return;
            }
        };

        if new_trigger_px.is_none() && new_limit_px.is_none() {
            return;
        }

        let ts_now = self.clock.borrow().timestamp_ns();
        let update = OrderUpdated::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.quantity(),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            order.venue_order_id(),
            order.account_id(),
            new_limit_px,
            new_trigger_px,
            None,
            order.is_quote_quantity(),
        );
        let wrapped = OrderEventAny::Updated(update);

        *order = match self.cache.borrow_mut().update_order(&wrapped) {
            Ok(order) => order,
            Err(e) => {
                log::error!("Failed to apply order event: {e}");
                return;
            }
        };

        if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
            if let Err(e) = matching_core.delete_order(order.client_order_id()) {
                log::debug!("Cannot update trailing-stop match info: {e:?}");
            }

            matching_core.add_order(RestingOrder::new(
                order.client_order_id(),
                order.order_side().as_specified(),
                order.order_type(),
                order.trigger_price(),
                order.price(),
                true,
            ));
        }

        self.send_risk_event(wrapped);
    }

    fn send_algo_command(&self, command: SubmitOrder, exec_algorithm_id: ExecAlgorithmId) {
        let id = command.strategy_id;
        log::info!("{id} {CMD}{SEND} {command}");

        let endpoint = format!("{exec_algorithm_id}.execute");
        msgbus::send_any(endpoint.into(), &TradingCommand::SubmitOrder(command));
    }

    fn send_risk_command(&self, command: TradingCommand) {
        log_cmd_send(&command);
        let endpoint = MessagingSwitchboard::risk_engine_queue_execute();
        msgbus::send_trading_command(endpoint, command);
    }

    fn send_exec_command(&self, command: TradingCommand) {
        log_cmd_send(&command);
        let endpoint = MessagingSwitchboard::exec_engine_queue_execute();
        msgbus::send_trading_command(endpoint, command);
    }

    fn send_risk_event(&self, event: OrderEventAny) {
        log_evt_send(&event);
        let endpoint = MessagingSwitchboard::risk_engine_process();
        msgbus::send_order_event(endpoint, event);
    }

    fn send_exec_event(&self, event: OrderEventAny) {
        log_evt_send(&event);
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
    }

    fn send_portfolio_order_event(&self, event: OrderEventAny) {
        log_evt_send(&event);
        let endpoint = MessagingSwitchboard::portfolio_update_order();
        msgbus::send_order_event(endpoint, event);
    }

    fn send_data_command(&self, command: DataCommand) {
        log::info!("{CMD}{SEND} {command:?}");
        let endpoint = MessagingSwitchboard::data_engine_queue_execute();
        msgbus::send_data_command(endpoint, command);
    }
}

fn publish_order_event(event: &OrderEventAny) {
    msgbus::publish_order_event(get_event_orders_topic(event.strategy_id()), event);

    if let OrderEventAny::Canceled(_) = event {
        msgbus::publish_order_event(get_order_cancels_topic(event.instrument_id()), event);
    }
}

fn log_cmd_send(command: &TradingCommand) {
    if let Some(id) = command.strategy_id() {
        log::info!("{id} {CMD}{SEND} {command}");
    } else {
        log::info!("{CMD}{SEND} {command}");
    }
}

fn log_evt_send(event: &OrderEventAny) {
    let id = event.strategy_id();
    log::info!("{id} {EVT}{SEND} {event}");
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        messages::data::{DataCommand, SubscribeCommand, UnsubscribeCommand},
        msgbus::{
            MessagingSwitchboard,
            stubs::{
                TypedIntoMessageSavingHandler, get_any_saving_handler,
                get_typed_into_message_saving_handler,
            },
        },
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        data::{QuoteTick, TradeTick},
        enums::{AggressorSide, OrderSide, OrderType, TriggerType},
        identifiers::{ClientOrderId, OrderListId, StrategyId, TradeId, TraderId},
        instruments::{
            CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt,
        },
        orders::{OrderList, OrderTestBuilder},
        types::{Price, Quantity},
    };
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::*;

    #[fixture]
    fn instrument() -> CryptoPerpetual {
        crypto_perpetual_ethusdt()
    }

    #[expect(clippy::type_complexity)]
    fn create_emulator() -> (
        Rc<RefCell<dyn Clock>>,
        Rc<RefCell<Cache>>,
        Rc<RefCell<OrderEmulator>>,
    ) {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let emulator = Rc::new(RefCell::new(OrderEmulator::new(
            clock.clone(),
            cache.clone(),
        )));

        OrderEmulator::register_msgbus_handlers(&emulator);

        (clock, cache, emulator)
    }

    fn create_stop_market_order(instrument: &CryptoPerpetual, trigger: TriggerType) -> OrderAny {
        OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .trigger_price(Price::from("5100.00"))
            .quantity(Quantity::from(1))
            .emulation_trigger(trigger)
            .build()
    }

    fn create_stop_limit_order(instrument: &CryptoPerpetual, trigger: TriggerType) -> OrderAny {
        OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .price(Price::from("5100.00"))
            .trigger_price(Price::from("5100.00"))
            .quantity(Quantity::from(1))
            .emulation_trigger(trigger)
            .build()
    }

    fn create_list_stop_market_order(
        instrument: &CryptoPerpetual,
        client_order_id: &str,
        order_list_id: OrderListId,
    ) -> OrderAny {
        OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(client_order_id))
            .order_list_id(order_list_id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("5100.00"))
            .quantity(Quantity::from(1))
            .emulation_trigger(TriggerType::BidAsk)
            .build()
    }

    fn create_submit_order(instrument: &CryptoPerpetual, order: &OrderAny) -> SubmitOrder {
        SubmitOrder::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("STRATEGY-001"),
            instrument.id(),
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            None,
            UUID4::new(),
            0.into(),
            None, // correlation_id
        )
    }

    fn create_quote_tick(instrument: &CryptoPerpetual, bid: &str, ask: &str) -> QuoteTick {
        QuoteTick::new(
            instrument.id(),
            Price::from(bid),
            Price::from(ask),
            Quantity::from(10),
            Quantity::from(10),
            0.into(),
            0.into(),
        )
    }

    fn create_trade_tick(instrument: &CryptoPerpetual, price: &str) -> TradeTick {
        TradeTick::new(
            instrument.id(),
            Price::from(price),
            Quantity::from(1),
            AggressorSide::Buyer,
            TradeId::from("T-001"),
            0.into(),
            0.into(),
        )
    }

    fn add_instrument_to_cache(cache: &Rc<RefCell<Cache>>, instrument: &CryptoPerpetual) {
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument.clone()))
            .unwrap();
    }

    fn register_risk_event_handler(id: &str) -> TypedIntoMessageSavingHandler<OrderEventAny> {
        let (handler, saving_handler) =
            get_typed_into_message_saving_handler::<OrderEventAny>(Some(Ustr::from(id)));
        msgbus::register_order_event_endpoint(MessagingSwitchboard::risk_engine_process(), handler);
        saving_handler
    }

    fn register_portfolio_event_handler(id: &str) -> TypedIntoMessageSavingHandler<OrderEventAny> {
        let (handler, saving_handler) =
            get_typed_into_message_saving_handler::<OrderEventAny>(Some(Ustr::from(id)));
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::portfolio_update_order(),
            handler,
        );
        saving_handler
    }

    fn register_data_command_handler(id: &str) -> TypedIntoMessageSavingHandler<DataCommand> {
        let (handler, saving_handler) =
            get_typed_into_message_saving_handler::<DataCommand>(Some(Ustr::from(id)));
        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_queue_execute(),
            handler,
        );
        saving_handler
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

    fn subscribe_order_cancel_topic(
        instrument_id: InstrumentId,
    ) -> (TypedHandler<OrderEventAny>, Rc<RefCell<Vec<OrderEventAny>>>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let handler = TypedHandler::from({
            let events = events.clone();
            move |event: &OrderEventAny| {
                events.borrow_mut().push(event.clone());
            }
        });
        msgbus::subscribe_order_events(
            get_order_cancels_topic(instrument_id).into(),
            handler.clone(),
            None,
        );
        (handler, events)
    }

    #[rstest]
    fn test_dispatch_manager_publish_initialized_publishes_order_event(
        instrument: CryptoPerpetual,
    ) {
        let (_clock, _cache, emulator) = create_emulator();
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let strategy_id = order.strategy_id();
        let client_order_id = order.client_order_id();
        let event = OrderEventAny::Initialized(order.init_event().clone());
        let (order_handler, order_events) = subscribe_order_topic(strategy_id);

        emulator
            .borrow_mut()
            .dispatch_manager_action(OrderManagerAction::PublishInitialized(event));
        msgbus::unsubscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            &order_handler,
        );
        let order_events = order_events.borrow();

        assert_eq!(order_events.len(), 1);
        assert!(matches!(
            &order_events[0],
            OrderEventAny::Initialized(event) if event.client_order_id == client_order_id
        ));
    }

    #[rstest]
    fn test_dispatch_manager_submit_to_emulator_applies_emulated_event(
        instrument: CryptoPerpetual,
    ) {
        let (_clock, cache, emulator) = create_emulator();
        let risk_events = register_risk_event_handler("RiskEngine.process.dispatch_emulated");
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());

        emulator
            .borrow_mut()
            .dispatch_manager_action(OrderManagerAction::SubmitToEmulator(command));
        let cache = cache.borrow();
        let cached_order = cache.order(&client_order_id).unwrap();
        let risk_events = risk_events.get_messages();

        assert_eq!(cached_order.status(), OrderStatus::Emulated);
        assert_eq!(risk_events.len(), 1);
        assert!(matches!(risk_events[0], OrderEventAny::Emulated(_)));
    }

    #[rstest]
    fn test_registered_execute_endpoint_routes_submit_order(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let risk_events = register_risk_event_handler("RiskEngine.process.endpoint_emulated");
        let data_commands = register_data_command_handler("DataEngine.queue_execute.endpoint");
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());

        msgbus::send_trading_command(
            MessagingSwitchboard::order_emulator_execute(),
            TradingCommand::SubmitOrder(command),
        );
        let cache = cache.borrow();
        let cached_order = cache.order(&client_order_id).unwrap();
        let risk_events = risk_events.get_messages();
        let data_commands = data_commands.get_messages();

        assert_eq!(cached_order.status(), OrderStatus::Emulated);
        assert_eq!(risk_events.len(), 1);
        assert!(matches!(risk_events[0], OrderEventAny::Emulated(_)));
        assert_eq!(data_commands.len(), 1);
        assert!(matches!(
            &data_commands[0],
            DataCommand::Subscribe(SubscribeCommand::Quotes(command))
                if command.instrument_id == instrument.id()
        ));
    }

    #[rstest]
    fn test_dispatch_manager_submit_to_risk_uses_risk_queue(instrument: CryptoPerpetual) {
        let (_clock, _cache, emulator) = create_emulator();
        let (handler, messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
            get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            handler,
        );
        let order = create_stop_market_order(&instrument, TriggerType::NoTrigger);
        let command = create_submit_order(&instrument, &order);
        let client_order_id = command.client_order_id;

        emulator
            .borrow_mut()
            .dispatch_manager_action(OrderManagerAction::SubmitToRisk(command));

        let messages = messages.get_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages.first(),
            Some(TradingCommand::SubmitOrder(command))
                if command.client_order_id == client_order_id
        ));
    }

    #[rstest]
    fn test_dispatch_manager_submit_to_algorithm_uses_dynamic_endpoint(
        instrument: CryptoPerpetual,
    ) {
        let (_clock, _cache, emulator) = create_emulator();
        let exec_algorithm_id = ExecAlgorithmId::from("ALG-001");
        let endpoint = format!("{exec_algorithm_id}.execute");
        let (handler, messages) =
            get_any_saving_handler::<TradingCommand>(Some(Ustr::from("ALG-001.execute")));
        msgbus::register_any(endpoint.into(), handler);
        let order = create_stop_market_order(&instrument, TriggerType::NoTrigger);
        let command = create_submit_order(&instrument, &order);
        let client_order_id = command.client_order_id;

        emulator
            .borrow_mut()
            .dispatch_manager_action(OrderManagerAction::SubmitToAlgorithm {
                command,
                exec_algorithm_id,
            });

        let messages = messages.get_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages.first(),
            Some(TradingCommand::SubmitOrder(command))
                if command.client_order_id == client_order_id
        ));
    }

    #[rstest]
    fn test_dispatch_manager_cancel_local_applies_and_publishes_event(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let portfolio_events =
            register_portfolio_event_handler("Portfolio.update_order.dispatch_canceled");
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let command = create_submit_order(&instrument, &order);
        let (order_handler, order_events) = subscribe_order_topic(strategy_id);
        let (cancel_handler, cancel_events) = subscribe_order_cancel_topic(instrument_id);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        emulator.borrow_mut().cache_submit_order_command(command);

        emulator
            .borrow_mut()
            .dispatch_manager_action(OrderManagerAction::CancelLocal(order));
        msgbus::unsubscribe_order_events(
            get_event_orders_topic(strategy_id).into(),
            &order_handler,
        );
        msgbus::unsubscribe_order_events(
            get_order_cancels_topic(instrument_id).into(),
            &cancel_handler,
        );
        let cached_order = cache
            .borrow()
            .order(&client_order_id)
            .map(|order| order.clone())
            .unwrap();
        let portfolio_events = portfolio_events.get_messages();
        let order_events = order_events.borrow();
        let cancel_events = cancel_events.borrow();
        let commands = emulator.borrow().get_submit_order_commands();

        assert_eq!(cached_order.status(), OrderStatus::Canceled);
        assert_eq!(portfolio_events.len(), 1);
        assert_eq!(order_events.len(), 1);
        assert_eq!(cancel_events.len(), 1);
        assert!(matches!(
            &portfolio_events[0],
            OrderEventAny::Canceled(event) if event.client_order_id == client_order_id
        ));
        assert_eq!(order_events[0].client_order_id(), client_order_id);
        assert_eq!(cancel_events[0].client_order_id(), client_order_id);
        assert!(!commands.contains_key(&client_order_id));
    }

    #[rstest]
    fn test_dispatch_manager_modify_local_quantity_updates_order(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let risk_events = register_risk_event_handler("RiskEngine.process.dispatch_updated");
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let quantity = Quantity::from(2);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .dispatch_manager_action(OrderManagerAction::ModifyLocalQuantity { order, quantity });
        let cache = cache.borrow();
        let cached_order = cache.order(&client_order_id).unwrap();
        let risk_events = risk_events.get_messages();

        assert_eq!(cached_order.quantity(), quantity);
        assert_eq!(risk_events.len(), 1);
        assert!(matches!(
            &risk_events[0],
            OrderEventAny::Updated(event) if event.client_order_id == client_order_id
        ));
    }

    #[rstest]
    fn test_subscribed_quotes_initially_empty() {
        let (_clock, _cache, emulator) = create_emulator();

        assert!(emulator.borrow().subscribed_quotes().is_empty());
    }

    #[rstest]
    fn test_subscribed_trades_initially_empty() {
        let (_clock, _cache, emulator) = create_emulator();

        assert!(emulator.borrow().subscribed_trades().is_empty());
    }

    #[rstest]
    fn test_get_submit_order_commands_initially_empty() {
        let (_clock, _cache, emulator) = create_emulator();

        assert!(emulator.borrow().get_submit_order_commands().is_empty());
    }

    #[rstest]
    fn test_get_matching_core_returns_none_when_not_created(instrument: CryptoPerpetual) {
        let (_clock, _cache, emulator) = create_emulator();

        assert!(
            emulator
                .borrow()
                .get_matching_core(&instrument.id())
                .is_none()
        );
    }

    #[rstest]
    fn test_create_matching_core(instrument: CryptoPerpetual) {
        let (_clock, _cache, emulator) = create_emulator();

        emulator
            .borrow_mut()
            .create_matching_core(instrument.id(), instrument.price_increment);

        assert!(
            emulator
                .borrow()
                .get_matching_core(&instrument.id())
                .is_some()
        );
    }

    #[rstest]
    fn test_on_quote_tick_no_matching_core_does_not_panic(instrument: CryptoPerpetual) {
        let (_clock, _cache, emulator) = create_emulator();
        let quote = create_quote_tick(&instrument, "5060.00", "5070.00");

        emulator.borrow_mut().on_quote_tick(quote);
    }

    #[rstest]
    fn test_on_trade_tick_no_matching_core_does_not_panic(instrument: CryptoPerpetual) {
        let (_clock, _cache, emulator) = create_emulator();
        let trade = create_trade_tick(&instrument, "5065.00");

        emulator.borrow_mut().on_trade_tick(trade);
    }

    #[rstest]
    fn test_submit_order_bid_ask_trigger_creates_matching_core(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        assert!(
            emulator
                .borrow()
                .get_matching_core(&instrument.id())
                .is_some()
        );
    }

    #[rstest]
    fn test_submit_order_bid_ask_trigger_tracks_quote_subscription(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let data_commands = register_data_command_handler("DataEngine.queue_execute.quotes");
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        assert_eq!(emulator.borrow().subscribed_quotes(), vec![instrument.id()]);
        assert!(emulator.borrow().subscribed_trades().is_empty());

        let commands = data_commands.get_messages();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            DataCommand::Subscribe(SubscribeCommand::Quotes(command))
                if command.instrument_id == instrument.id()
        ));

        let quote = create_quote_tick(&instrument, "5060.00", "5070.00");
        msgbus::publish_quote(get_quotes_topic(instrument.id()), &quote);
        let core = emulator
            .borrow()
            .get_matching_core(&instrument.id())
            .unwrap();
        assert_eq!(core.bid, Some(Price::from("5060.00")));
        assert_eq!(core.ask, Some(Price::from("5070.00")));
    }

    #[rstest]
    fn test_submit_order_last_price_trigger_tracks_trade_subscription(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let data_commands = register_data_command_handler("DataEngine.queue_execute.trades");
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::LastPrice);
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        assert!(emulator.borrow().subscribed_quotes().is_empty());
        assert_eq!(emulator.borrow().subscribed_trades(), vec![instrument.id()]);

        let commands = data_commands.get_messages();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            DataCommand::Subscribe(SubscribeCommand::Trades(command))
                if command.instrument_id == instrument.id()
        ));

        let trade = create_trade_tick(&instrument, "5065.00");
        msgbus::publish_trade(get_trades_topic(instrument.id()), &trade);
        let core = emulator
            .borrow()
            .get_matching_core(&instrument.id())
            .unwrap();
        assert_eq!(core.last, Some(Price::from("5065.00")));
    }

    #[rstest]
    fn test_reset_unsubscribes_market_data_and_clears_state(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let data_commands = register_data_command_handler("DataEngine.queue_execute.reset");
        add_instrument_to_cache(&cache, &instrument);
        let quote_order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let quote_command = create_submit_order(&instrument, &quote_order);
        let trade_order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-RESET-TRADE"))
            .side(OrderSide::Buy)
            .trigger_price(Price::from("5100.00"))
            .quantity(Quantity::from(1))
            .emulation_trigger(TriggerType::LastPrice)
            .build();
        let trade_command = create_submit_order(&instrument, &trade_order);
        cache
            .borrow_mut()
            .add_order(quote_order, None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(trade_order, None, None, false)
            .unwrap();
        emulator
            .borrow_mut()
            .cache_submit_order_command(quote_command.clone());
        emulator.borrow_mut().handle_submit_order(&quote_command);
        emulator
            .borrow_mut()
            .cache_submit_order_command(trade_command.clone());
        emulator.borrow_mut().handle_submit_order(&trade_command);
        data_commands.clear();

        emulator.borrow_mut().reset();
        let commands = data_commands.get_messages();
        let emulator_ref = emulator.borrow();

        assert!(emulator_ref.subscribed_quotes.is_empty());
        assert!(emulator_ref.subscribed_trades.is_empty());
        assert!(emulator_ref.subscribed_strategies.is_empty());
        assert!(emulator_ref.monitored_positions.is_empty());
        assert!(emulator_ref.quote_handlers.is_empty());
        assert!(emulator_ref.trade_handlers.is_empty());
        assert!(emulator_ref.get_submit_order_commands().is_empty());
        assert!(emulator_ref.get_matching_core(&instrument.id()).is_none());
        assert!(commands.iter().any(|command| matches!(
            command,
            DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command))
                if command.instrument_id == instrument.id()
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            DataCommand::Unsubscribe(UnsubscribeCommand::Trades(command))
                if command.instrument_id == instrument.id()
        )));

        drop(emulator_ref);
        emulator
            .borrow_mut()
            .create_matching_core(instrument.id(), instrument.price_increment);
        let quote = create_quote_tick(&instrument, "5060.00", "5070.00");
        let trade = create_trade_tick(&instrument, "5065.00");
        msgbus::publish_quote(get_quotes_topic(instrument.id()), &quote);
        msgbus::publish_trade(get_trades_topic(instrument.id()), &trade);
        let core = emulator
            .borrow()
            .get_matching_core(&instrument.id())
            .unwrap();
        assert_eq!(core.bid, None);
        assert_eq!(core.ask, None);
        assert_eq!(core.last, None);
    }

    #[rstest]
    fn test_submit_order_list_handles_reentrant_order_events(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let data_commands = register_data_command_handler("DataEngine.queue_execute.list");
        add_instrument_to_cache(&cache, &instrument);
        let order_list_id = OrderListId::from("OL-EMULATOR-001");
        let first_order = create_list_stop_market_order(&instrument, "O-LIST-001", order_list_id);
        let second_order = create_list_stop_market_order(&instrument, "O-LIST-002", order_list_id);
        let orders = vec![first_order.clone(), second_order.clone()];
        let order_list = OrderList::from_orders(&orders, 0.into());
        let order_inits = orders
            .iter()
            .map(|order| order.init_event().clone())
            .collect();
        cache
            .borrow_mut()
            .add_order(first_order.clone(), None, None, false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(second_order.clone(), None, None, false)
            .unwrap();
        let command = SubmitOrderList::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("STRATEGY-001"),
            order_list,
            order_inits,
            None,
            None,
            None,
            UUID4::new(),
            0.into(),
            None,
        );

        emulator
            .borrow_mut()
            .execute(TradingCommand::SubmitOrderList(command));

        let commands = data_commands.get_messages();
        let cache = cache.borrow();
        let first_status = cache
            .order(&first_order.client_order_id())
            .unwrap()
            .status();
        let second_status = cache
            .order(&second_order.client_order_id())
            .unwrap()
            .status();
        drop(cache);
        let emulator = emulator.borrow();

        assert_eq!(first_status, OrderStatus::Emulated);
        assert_eq!(second_status, OrderStatus::Emulated);
        assert!(emulator.get_matching_core(&instrument.id()).is_some());
        assert_eq!(emulator.subscribed_quotes(), vec![instrument.id()]);
        assert!(commands.iter().any(|command| matches!(
            command,
            DataCommand::Subscribe(SubscribeCommand::Quotes(command))
                if command.instrument_id == instrument.id()
        )));
    }

    #[rstest]
    fn test_submit_order_caches_command(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        let commands = emulator.borrow().get_submit_order_commands();
        assert!(commands.contains_key(&client_order_id));
    }

    #[rstest]
    fn test_handle_submit_order_applies_emulated_event_to_cache(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let risk_events = register_risk_event_handler("RiskEngine.process.emulated");
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        let (order_handler, order_events) = subscribe_order_topic(strategy_id);

        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);
        msgbus::unsubscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            &order_handler,
        );
        let cache = cache.borrow();
        let cached_order = cache.order(&client_order_id).unwrap();
        let risk_events = risk_events.get_messages();
        let order_events = order_events.borrow();

        assert_eq!(cached_order.status(), OrderStatus::Emulated);
        assert_eq!(cached_order.event_count(), 2);
        assert_eq!(risk_events.len(), 1);
        assert!(matches!(risk_events[0], OrderEventAny::Emulated(_)));
        assert_eq!(order_events.len(), 1);
        assert!(matches!(order_events[0], OrderEventAny::Emulated(_)));
    }

    #[rstest]
    fn test_update_order_applies_updated_event_to_cache(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let risk_events = register_risk_event_handler("RiskEngine.process.updated");
        let mut order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .update_order(&mut order, Quantity::from(2));
        let cache = cache.borrow();
        let cached_order = cache.order(&client_order_id).unwrap();
        let risk_events = risk_events.get_messages();

        assert_eq!(order.quantity(), Quantity::from(2));
        assert_eq!(cached_order.quantity(), Quantity::from(2));
        assert_eq!(cached_order.status(), OrderStatus::Initialized);
        assert_eq!(risk_events.len(), 1);
        assert!(matches!(risk_events[0], OrderEventAny::Updated(_)));
    }

    #[rstest]
    fn test_fill_market_order_applies_released_event_to_cache(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        let risk_events = register_risk_event_handler("RiskEngine.process.released");
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);
        risk_events.clear();
        let (order_handler, order_events) = subscribe_order_topic(strategy_id);
        {
            let mut emulator = emulator.borrow_mut();
            emulator
                .matching_cores
                .get_mut(&instrument.id())
                .unwrap()
                .set_ask_raw(Price::from("5100.00"));
            emulator.fill_market_order(client_order_id);
        }
        msgbus::unsubscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            &order_handler,
        );
        let cache = cache.borrow();
        let cached_order = cache.order(&client_order_id).unwrap();
        let risk_events = risk_events.get_messages();
        let order_events = order_events.borrow();

        assert_eq!(cached_order.status(), OrderStatus::Released);
        assert_eq!(risk_events.len(), 1);
        assert!(matches!(risk_events[0], OrderEventAny::Released(_)));
        assert_eq!(order_events.len(), 2);
        assert!(matches!(order_events[0], OrderEventAny::Initialized(_)));
        assert!(matches!(order_events[1], OrderEventAny::Released(_)));
    }

    #[rstest]
    fn test_fill_limit_order_publishes_transformed_initialized_before_released(
        instrument: CryptoPerpetual,
    ) {
        let (_clock, cache, emulator) = create_emulator();
        let risk_events = register_risk_event_handler("RiskEngine.process.limit_released");
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_limit_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);
        risk_events.clear();
        let (order_handler, order_events) = subscribe_order_topic(strategy_id);
        {
            let mut emulator = emulator.borrow_mut();
            emulator
                .matching_cores
                .get_mut(&instrument.id())
                .unwrap()
                .set_ask_raw(Price::from("5100.00"));
            emulator.fill_limit_order(client_order_id);
        }
        msgbus::unsubscribe_order_events(
            format!("events.order.{strategy_id}").into(),
            &order_handler,
        );
        let cache = cache.borrow();
        let cached_order = cache.order(&client_order_id).unwrap();
        let risk_events = risk_events.get_messages();
        let order_events = order_events.borrow();

        assert_eq!(cached_order.status(), OrderStatus::Released);
        assert_eq!(risk_events.len(), 1);
        assert!(matches!(risk_events[0], OrderEventAny::Released(_)));
        assert_eq!(order_events.len(), 2);
        assert!(matches!(order_events[0], OrderEventAny::Initialized(_)));
        assert!(matches!(order_events[1], OrderEventAny::Released(_)));
    }

    #[rstest]
    fn test_quote_tick_updates_matching_core_prices(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        let quote = create_quote_tick(&instrument, "5060.00", "5070.00");
        emulator.borrow_mut().on_quote_tick(quote);

        let core = emulator
            .borrow()
            .get_matching_core(&instrument.id())
            .unwrap();
        assert_eq!(core.bid, Some(Price::from("5060.00")));
        assert_eq!(core.ask, Some(Price::from("5070.00")));
    }

    #[rstest]
    fn test_trade_tick_updates_matching_core_last_price(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::LastPrice);
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        let trade = create_trade_tick(&instrument, "5065.00");
        emulator.borrow_mut().on_trade_tick(trade);

        let core = emulator
            .borrow()
            .get_matching_core(&instrument.id())
            .unwrap();
        assert_eq!(core.last, Some(Price::from("5065.00")));
    }

    #[rstest]
    fn test_cancel_order_removes_from_matching_core(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        emulator.borrow_mut().cancel_order(&order);

        let core = emulator
            .borrow()
            .get_matching_core(&instrument.id())
            .unwrap();
        assert!(core.get_orders().is_empty());
    }

    #[rstest]
    fn test_cancel_order_removes_cached_command(instrument: CryptoPerpetual) {
        let (_clock, cache, emulator) = create_emulator();
        add_instrument_to_cache(&cache, &instrument);
        let order = create_stop_market_order(&instrument, TriggerType::BidAsk);
        let client_order_id = order.client_order_id();
        let command = create_submit_order(&instrument, &order);
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        emulator
            .borrow_mut()
            .cache_submit_order_command(command.clone());
        emulator.borrow_mut().handle_submit_order(&command);

        emulator.borrow_mut().cancel_order(&order);

        let commands = emulator.borrow().get_submit_order_commands();
        assert!(!commands.contains_key(&client_order_id));
    }
}
