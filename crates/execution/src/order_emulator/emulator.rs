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

use std::{
    cell::RefCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use nautilus_common::{
    actor::{DataActorConfig, DataActorCore},
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    messages::execution::{
        CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
    msgbus::{
        self, TypedHandler,
        switchboard::{get_quotes_topic, get_trades_topic},
    },
};
use nautilus_core::{UUID4, WeakCell};
use nautilus_model::{
    data::{OrderBookDeltas, QuoteTick, TradeTick},
    enums::{ContingencyType, OrderSide, OrderSideSpecified, OrderStatus, OrderType, TriggerType},
    events::{OrderCanceled, OrderEmulated, OrderEventAny, OrderReleased, OrderUpdated},
    identifiers::{ActorId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    instruments::Instrument,
    orders::{LimitOrder, MarketOrder, Order, OrderAny},
    types::{Price, Quantity},
};

use crate::{
    matching_core::{OrderMatchInfo, OrderMatchingCore},
    order_manager::{
        handlers::{CancelOrderHandlerAny, ModifyOrderHandlerAny, SubmitOrderHandlerAny},
        manager::OrderManager,
    },
    trailing::trailing_stop_calculate,
};

pub struct OrderEmulator {
    actor: DataActorCore,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    manager: OrderManager,
    matching_cores: AHashMap<InstrumentId, OrderMatchingCore>,
    subscribed_quotes: AHashSet<InstrumentId>,
    subscribed_trades: AHashSet<InstrumentId>,
    subscribed_strategies: AHashSet<StrategyId>,
    monitored_positions: AHashSet<PositionId>,
    on_event_handler: Option<TypedHandler<OrderEventAny>>,
    self_ref: Option<WeakCell<Self>>,
}

impl Debug for OrderEmulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderEmulator))
            .field("actor", &self.actor)
            .field("cores", &self.matching_cores.len())
            .field("subscribed_quotes", &self.subscribed_quotes.len())
            .finish()
    }
}

impl Deref for OrderEmulator {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.actor
    }
}

impl DerefMut for OrderEmulator {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.actor
    }
}

impl OrderEmulator {
    pub fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        let config = DataActorConfig {
            actor_id: Some(ActorId::from("OrderEmulator")),
            ..Default::default()
        };

        let active_local = true;
        let manager =
            OrderManager::new(clock.clone(), cache.clone(), active_local, None, None, None);

        Self {
            actor: DataActorCore::new(config),
            clock,
            cache,
            manager,
            matching_cores: AHashMap::new(),
            subscribed_quotes: AHashSet::new(),
            subscribed_trades: AHashSet::new(),
            subscribed_strategies: AHashSet::new(),
            monitored_positions: AHashSet::new(),
            on_event_handler: None,
            self_ref: None,
        }
    }

    /// Sets the weak self-reference for creating subscription handlers.
    pub fn set_self_ref(&mut self, self_ref: WeakCell<Self>) {
        self.self_ref = Some(self_ref);
    }

    /// Registers the emulator with the trading system.
    ///
    /// # Errors
    ///
    /// Returns an error if registration fails.
    pub fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()> {
        self.actor.register(trader_id, clock, cache)
    }

    pub fn set_on_event_handler(&mut self, handler: TypedHandler<OrderEventAny>) {
        self.on_event_handler = Some(handler);
    }

    /// Sets the handler for submit order commands.
    pub fn set_submit_order_handler(&mut self, handler: SubmitOrderHandlerAny) {
        self.manager.set_submit_order_handler(handler);
    }

    /// Sets the handler for cancel order commands.
    pub fn set_cancel_order_handler(&mut self, handler: CancelOrderHandlerAny) {
        self.manager.set_cancel_order_handler(handler);
    }

    /// Sets the handler for modify order commands.
    pub fn set_modify_order_handler(&mut self, handler: ModifyOrderHandlerAny) {
        self.manager.set_modify_order_handler(handler);
    }

    /// Caches a submit order command for emulation tracking.
    pub fn cache_submit_order_command(&mut self, command: SubmitOrder) {
        self.manager.cache_submit_order_command(command);
    }

    /// Subscribes to quote data for the given instrument.
    fn subscribe_quotes_for_instrument(&mut self, instrument_id: InstrumentId) {
        let Some(self_ref) = self.self_ref.clone() else {
            log::warn!("Cannot subscribe to quotes: self_ref not set");
            return;
        };

        let topic = get_quotes_topic(instrument_id);
        let handler = TypedHandler::from(move |quote: &QuoteTick| {
            if let Some(emulator) = self_ref.upgrade() {
                emulator.borrow_mut().on_quote_tick(*quote);
            }
        });

        self.actor
            .subscribe_quotes(topic, handler, instrument_id, None, None);
    }

    /// Subscribes to trade data for the given instrument.
    fn subscribe_trades_for_instrument(&mut self, instrument_id: InstrumentId) {
        let Some(self_ref) = self.self_ref.clone() else {
            log::warn!("Cannot subscribe to trades: self_ref not set");
            return;
        };

        let topic = get_trades_topic(instrument_id);
        let handler = TypedHandler::from(move |trade: &TradeTick| {
            if let Some(emulator) = self_ref.upgrade() {
                emulator.borrow_mut().on_trade_tick(*trade);
            }
        });

        self.actor
            .subscribe_trades(topic, handler, instrument_id, None, None);
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
    pub fn get_submit_order_commands(&self) -> AHashMap<ClientOrderId, SubmitOrder> {
        self.manager.get_submit_order_commands()
    }

    #[must_use]
    pub fn get_matching_core(&self, instrument_id: &InstrumentId) -> Option<OrderMatchingCore> {
        self.matching_cores.get(instrument_id).cloned()
    }

    /// Reactivates emulated orders from cache on start.
    ///
    /// # Errors
    ///
    /// Returns an error if no emulated orders are found or processing fails.
    ///
    /// # Panics
    ///
    /// Panics if a cached client ID cannot be unwrapped.
    pub fn on_start(&mut self) -> anyhow::Result<()> {
        let emulated_orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_emulated(None, None, None, None, None)
            .into_iter()
            .cloned()
            .collect();

        if emulated_orders.is_empty() {
            log::error!("No emulated orders to reactivate");
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
                    self.manager.cancel_order(&order);
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
            );

            self.handle_submit_order(command);
        }

        Ok(())
    }

    /// # Panics
    ///
    /// Panics if the order cannot be converted to a passive order.
    pub fn on_event(&mut self, event: OrderEventAny) {
        log::info!("{RECV}{EVT} {event}");

        self.manager.handle_event(event.clone());

        if let Some(order) = self.cache.borrow().order(&event.client_order_id())
            && order.is_closed()
            && let Some(matching_core) = self.matching_cores.get_mut(&order.instrument_id())
            && let Err(e) = matching_core.delete_order(event.client_order_id())
        {
            log::error!("Error deleting order: {e}");
        }
        // else: Order not in cache yet
    }

    pub const fn on_stop(&self) {}

    pub fn on_reset(&mut self) {
        self.manager.reset();
        self.matching_cores.clear();
    }

    pub const fn on_dispose(&self) {}

    pub fn execute(&mut self, command: TradingCommand) {
        log::info!("{RECV}{CMD} {command}");

        match command {
            TradingCommand::SubmitOrder(command) => self.handle_submit_order(command),
            TradingCommand::SubmitOrderList(command) => self.handle_submit_order_list(command),
            TradingCommand::ModifyOrder(command) => self.handle_modify_order(command),
            TradingCommand::CancelOrder(command) => self.handle_cancel_order(command),
            TradingCommand::CancelAllOrders(command) => self.handle_cancel_all_orders(command),
            _ => log::error!("Cannot handle command: unrecognized {command:?}"),
        }
    }

    fn create_matching_core(
        &mut self,
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> OrderMatchingCore {
        let matching_core =
            OrderMatchingCore::new(instrument_id, price_increment, None, None, None);
        self.matching_cores
            .insert(instrument_id, matching_core.clone());
        log::info!("Creating matching core for {instrument_id:?}");
        matching_core
    }

    /// # Panics
    ///
    /// Panics if the emulation trigger type is `NoTrigger` or if order not in cache.
    pub fn handle_submit_order(&mut self, command: SubmitOrder) {
        let client_order_id = command.client_order_id;

        let mut order = self
            .cache
            .borrow()
            .order(&client_order_id)
            .cloned()
            .expect("order must exist in cache");

        let emulation_trigger = order.emulation_trigger();

        assert_ne!(
            emulation_trigger,
            Some(TriggerType::NoTrigger),
            "order.emulation_trigger must not be TriggerType::NoTrigger"
        );
        assert!(
            self.manager
                .get_submit_order_commands()
                .contains_key(&client_order_id),
            "client_order_id must be in submit_order_commands"
        );

        if !matches!(
            emulation_trigger,
            Some(TriggerType::Default | TriggerType::BidAsk | TriggerType::LastPrice)
        ) {
            log::error!("Cannot emulate order: `TriggerType` {emulation_trigger:?} not supported");
            self.manager.cancel_order(&order);
            return;
        }

        self.check_monitoring(command.strategy_id, command.position_id);

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
                    self.manager.cancel_order(&order);
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
                    self.manager.cancel_order(&order);
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

                self.manager.cancel_order(&order);
                return;
            }
        }

        // Cache command
        self.manager.cache_submit_order_command(command);

        // Check if immediately marketable
        let match_info = OrderMatchInfo::new(
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
                if !self.subscribed_quotes.contains(&trigger_instrument_id) {
                    self.subscribe_quotes_for_instrument(trigger_instrument_id);
                    self.subscribed_quotes.insert(trigger_instrument_id);
                }
            }
            TriggerType::LastPrice => {
                if !self.subscribed_trades.contains(&trigger_instrument_id) {
                    self.subscribe_trades_for_instrument(trigger_instrument_id);
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

            if let Err(e) = order.apply(OrderEventAny::Emulated(event)) {
                log::error!("Cannot apply order event: {e:?}");
                return;
            }

            if let Err(e) = self.cache.borrow_mut().update_order(&order) {
                log::error!("Cannot update order: {e:?}");
                return;
            }

            self.manager.send_risk_event(OrderEventAny::Emulated(event));

            msgbus::publish_order_event(
                format!("events.order.{}", order.strategy_id()).into(),
                &OrderEventAny::Emulated(event),
            );
        }

        // Since we are cloning the matching core, we need to insert it back into the original hashmap
        self.matching_cores
            .insert(trigger_instrument_id, matching_core);

        log::info!("Emulating {order}");
    }

    fn handle_submit_order_list(&mut self, command: SubmitOrderList) {
        self.check_monitoring(command.strategy_id, command.position_id);

        for order in &command.order_list.orders {
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

            if let Err(e) =
                self.manager
                    .create_new_submit_order(order, command.position_id, command.client_id)
            {
                log::error!("Error creating new submit order: {e}");
            }
        }
    }

    fn handle_modify_order(&mut self, command: ModifyOrder) {
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
            );

            self.manager.send_exec_event(OrderEventAny::Updated(event));

            let trigger_instrument_id = order
                .trigger_instrument_id()
                .unwrap_or_else(|| order.instrument_id());

            if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
                let match_info = OrderMatchInfo::new(
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
            self.manager.cancel_order(&order);
            return;
        };

        if !matching_core.order_exists(order.client_order_id())
            && order.is_open()
            && !order.is_pending_cancel()
        {
            // Order not held in the emulator
            self.manager
                .send_exec_command(TradingCommand::CancelOrder(command));
        } else {
            self.manager.cancel_order(&order);
        }
    }

    fn handle_cancel_all_orders(&mut self, command: CancelAllOrders) {
        let instrument_id = command.instrument_id;
        let matching_core = match self.matching_cores.get(&instrument_id) {
            Some(core) => core,
            None => return, // No orders to cancel
        };

        let orders_to_cancel = match command.order_side {
            OrderSide::NoOrderSide => {
                // Get both bid and ask orders
                let mut all_orders = Vec::new();
                all_orders.extend(matching_core.get_orders_bid().iter().cloned());
                all_orders.extend(matching_core.get_orders_ask().iter().cloned());
                all_orders
            }
            OrderSide::Buy => matching_core.get_orders_bid().to_vec(),
            OrderSide::Sell => matching_core.get_orders_ask().to_vec(),
        };

        // Process all orders in a single iteration
        for match_info in orders_to_cancel {
            if let Some(order) = self
                .cache
                .borrow()
                .order(&match_info.client_order_id)
                .cloned()
            {
                self.manager.cancel_order(&order);
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
        );

        if let Err(e) = order.apply(OrderEventAny::Updated(event)) {
            log::error!("Cannot apply order event: {e:?}");
            return;
        }
        if let Err(e) = self.cache.borrow_mut().update_order(order) {
            log::error!("Cannot update order: {e:?}");
            return;
        }

        self.manager.send_risk_event(OrderEventAny::Updated(event));
    }

    pub fn on_order_book_deltas(&mut self, deltas: OrderBookDeltas) {
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
        let orders = if let Some(matching_core) = self.matching_cores.get_mut(instrument_id) {
            matching_core.iterate();

            matching_core.get_orders()
        } else {
            log::error!("Cannot iterate orders: no matching core for instrument {instrument_id}");
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
                .cloned()
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

    /// # Panics
    ///
    /// Panics if the order cannot be converted to a passive order.
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
            log::error!("Cannot delete order: {e:?}");
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

        self.manager.send_exec_event(OrderEventAny::Canceled(event));
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
        let order = match self.cache.borrow().order(&client_order_id).cloned() {
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
            OrderType::Market | OrderType::MarketIfTouched | OrderType::TrailingStopMarket => {
                self.fill_market_order(client_order_id);
            }
            _ => panic!("invalid `OrderType`, was {}", order.order_type()),
        }
    }

    /// # Panics
    ///
    /// Panics if a limit order has no price.
    pub fn fill_limit_order(&mut self, client_order_id: ClientOrderId) {
        let order = match self.cache.borrow().order(&client_order_id).cloned() {
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
                log::error!("Error deleting order: {e:?}");
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

            if let Err(e) = self.cache.borrow_mut().add_order(
                OrderAny::Limit(transformed.clone()),
                command.position_id,
                command.client_id,
                true,
            ) {
                log::error!("Failed to add order: {e}");
            }

            msgbus::publish_order_event(
                format!("events.order.{}", order.strategy_id()).into(),
                transformed.last_event(),
            );

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

            if let Err(e) = transformed.apply(OrderEventAny::Released(event)) {
                log::error!("Failed to apply order event: {e}");
            }

            if let Err(e) = self
                .cache
                .borrow_mut()
                .update_order(&OrderAny::Limit(transformed.clone()))
            {
                log::error!("Failed to update order: {e}");
            }

            self.manager.send_risk_event(OrderEventAny::Released(event));

            log::info!("Releasing order {}", order.client_order_id());

            // Publish event
            msgbus::publish_order_event(
                format!("events.order.{}", transformed.strategy_id()).into(),
                &OrderEventAny::Released(event),
            );

            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                self.manager.send_algo_command(command, exec_algorithm_id);
            } else {
                self.manager
                    .send_exec_command(TradingCommand::SubmitOrder(command));
            }
        }
    }

    /// # Panics
    ///
    /// Panics if a market order command is missing.
    pub fn fill_market_order(&mut self, client_order_id: ClientOrderId) {
        let mut order = match self.cache.borrow().order(&client_order_id).cloned() {
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
                log::error!("Cannot delete order: {e:?}");
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

            if let Err(e) = self.cache.borrow_mut().add_order(
                OrderAny::Market(transformed.clone()),
                command.position_id,
                command.client_id,
                true,
            ) {
                log::error!("Failed to add order: {e}");
            }

            msgbus::publish_order_event(
                format!("events.order.{}", order.strategy_id()).into(),
                transformed.last_event(),
            );

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

            if let Err(e) = transformed.apply(OrderEventAny::Released(event)) {
                log::error!("Failed to apply order event: {e}");
            }

            if let Err(e) = self
                .cache
                .borrow_mut()
                .update_order(&OrderAny::Market(transformed))
            {
                log::error!("Failed to update order: {e}");
            }
            self.manager.send_risk_event(OrderEventAny::Released(event));

            log::info!("Releasing order {}", order.client_order_id());

            // Publish event
            msgbus::publish_order_event(
                format!("events.order.{}", order.strategy_id()).into(),
                &OrderEventAny::Released(event),
            );

            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                self.manager.send_algo_command(command, exec_algorithm_id);
            } else {
                self.manager
                    .send_exec_command(TradingCommand::SubmitOrder(command));
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn update_trailing_stop_order(&mut self, order: &mut OrderAny) {
        let Some(matching_core) = self.matching_cores.get(&order.instrument_id()) else {
            log::error!(
                "Cannot update trailing-stop order: no matching core for instrument {}",
                order.instrument_id()
            );
            return;
        };

        let mut bid = matching_core.bid;
        let mut ask = matching_core.ask;
        let mut last = matching_core.last;

        if bid.is_none() || ask.is_none() || last.is_none() {
            if let Some(q) = self.cache.borrow().quote(&matching_core.instrument_id) {
                bid.get_or_insert(q.bid_price);
                ask.get_or_insert(q.ask_price);
            }
            if let Some(t) = self.cache.borrow().trade(&matching_core.instrument_id) {
                last.get_or_insert(t.price);
            }
        }

        let (new_trigger_px, new_limit_px) = match trailing_stop_calculate(
            matching_core.price_increment,
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
        );
        let wrapped = OrderEventAny::Updated(update);
        if let Err(e) = order.apply(wrapped.clone()) {
            log::error!("Failed to apply order event: {e}");
            return;
        }
        if let Err(e) = self.cache.borrow_mut().update_order(order) {
            log::error!("Failed to update order in cache: {e}");
            return;
        }
        self.manager.send_risk_event(wrapped);
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_core::{UUID4, WeakCell};
    use nautilus_model::{
        data::{QuoteTick, TradeTick},
        enums::{AggressorSide, OrderSide, OrderType, TriggerType},
        identifiers::{StrategyId, TradeId, TraderId},
        instruments::{
            CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt,
        },
        orders::OrderTestBuilder,
        types::{Price, Quantity},
    };
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn instrument() -> CryptoPerpetual {
        crypto_perpetual_ethusdt()
    }

    #[allow(clippy::type_complexity)]
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

        // Register with trader for subscription support
        emulator
            .borrow_mut()
            .register(TraderId::from("TRADER-001"), clock.clone(), cache.clone())
            .unwrap();

        // Set self-ref for subscription handlers
        let self_ref = WeakCell::from(Rc::downgrade(&emulator));
        emulator.borrow_mut().set_self_ref(self_ref);

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
            .add_instrument(InstrumentAny::CryptoPerpetual(*instrument))
            .unwrap();
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
        emulator.borrow_mut().handle_submit_order(command);

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
        emulator.borrow_mut().handle_submit_order(command);

        assert_eq!(emulator.borrow().subscribed_quotes(), vec![instrument.id()]);
        assert!(emulator.borrow().subscribed_trades().is_empty());
    }

    #[rstest]
    fn test_submit_order_last_price_trigger_tracks_trade_subscription(instrument: CryptoPerpetual) {
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
        emulator.borrow_mut().handle_submit_order(command);

        assert!(emulator.borrow().subscribed_quotes().is_empty());
        assert_eq!(emulator.borrow().subscribed_trades(), vec![instrument.id()]);
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
        emulator.borrow_mut().handle_submit_order(command);

        let commands = emulator.borrow().get_submit_order_commands();
        assert!(commands.contains_key(&client_order_id));
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
        emulator.borrow_mut().handle_submit_order(command);

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
        emulator.borrow_mut().handle_submit_order(command);

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
        emulator.borrow_mut().handle_submit_order(command);

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
        emulator.borrow_mut().handle_submit_order(command);

        emulator.borrow_mut().cancel_order(&order);

        let commands = emulator.borrow().get_submit_order_commands();
        assert!(!commands.contains_key(&client_order_id));
    }
}
