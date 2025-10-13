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

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Debug,
    rc::Rc,
};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    messages::execution::{
        CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
    msgbus::{self, handler::ShareableMessageHandler},
};
use nautilus_core::UUID4;
use nautilus_model::{
    data::{OrderBookDeltas, QuoteTick, TradeTick},
    enums::{ContingencyType, OrderSide, OrderSideSpecified, OrderStatus, OrderType, TriggerType},
    events::{OrderCanceled, OrderEmulated, OrderEventAny, OrderReleased, OrderUpdated},
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId},
    instruments::Instrument,
    orders::{LimitOrder, MarketOrder, Order, OrderAny, PassiveOrderAny},
    types::{Price, Quantity},
};

use crate::{
    matching_core::OrderMatchingCore, order_manager::manager::OrderManager,
    trailing::trailing_stop_calculate,
};

pub struct OrderEmulator {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    manager: OrderManager,
    matching_cores: HashMap<InstrumentId, OrderMatchingCore>,
    subscribed_quotes: HashSet<InstrumentId>,
    subscribed_trades: HashSet<InstrumentId>,
    subscribed_strategies: HashSet<StrategyId>,
    monitored_positions: HashSet<PositionId>,
    on_event_handler: Option<ShareableMessageHandler>,
}

impl Debug for OrderEmulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderEmulator))
            .field("cores", &self.matching_cores.len())
            .field("subscribed_quotes", &self.subscribed_quotes.len())
            .finish()
    }
}

impl OrderEmulator {
    pub fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        // TODO: Impl Actor Trait
        // self.register_base(portfolio, msgbus, cache, clock);

        let active_local = true;
        let manager = OrderManager::new(clock.clone(), cache.clone(), active_local);

        Self {
            clock,
            cache,
            manager,
            matching_cores: HashMap::new(),
            subscribed_quotes: HashSet::new(),
            subscribed_trades: HashSet::new(),
            subscribed_strategies: HashSet::new(),
            monitored_positions: HashSet::new(),
            on_event_handler: None,
        }
    }

    pub fn set_on_event_handler(&mut self, handler: ShareableMessageHandler) {
        self.on_event_handler = Some(handler);
    }

    // TODO: WIP
    // pub fn set_submit_order_handler(&mut self, handler: SubmitOrderHandlerAny) {
    //     self.manager.set_submit_order_handler(handler);
    // }
    //
    // pub fn set_cancel_order_handler(&mut self, handler: CancelOrderHandlerAny) {
    //     self.manager.set_cancel_order_handler(handler);
    // }
    //
    // pub fn set_modify_order_handler(&mut self, handler: ModifyOrderHandlerAny) {
    //     self.manager.set_modify_order_handler(handler);
    // }

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
    pub fn get_submit_order_commands(&self) -> HashMap<ClientOrderId, SubmitOrder> {
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
            .orders_emulated(None, None, None, None)
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
                client_id.unwrap(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                order.venue_order_id().unwrap(),
                order.clone(),
                order.exec_algorithm_id(),
                position_id,
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
            )?;

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
            && let Err(e) = matching_core.delete_order(
                &PassiveOrderAny::try_from(order.clone()).expect("passive order conversion"),
            )
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
    /// Panics if the emulation trigger type is `NoTrigger`.
    pub fn handle_submit_order(&mut self, command: SubmitOrder) {
        let mut order = command.order.clone();
        let emulation_trigger = order.emulation_trigger();

        assert_ne!(
            emulation_trigger,
            Some(TriggerType::NoTrigger),
            "command.order.emulation_trigger must not be TriggerType::NoTrigger"
        );
        assert!(
            self.manager
                .get_submit_order_commands()
                .contains_key(&order.client_order_id()),
            "command.order.client_order_id must be in submit_order_commands"
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
        matching_core.match_order(
            &PassiveOrderAny::try_from(order.clone()).expect("passive order conversion"),
            true,
        );

        // Handle data subscriptions
        match emulation_trigger.unwrap() {
            TriggerType::Default | TriggerType::BidAsk => {
                if !self.subscribed_quotes.contains(&trigger_instrument_id) {
                    if !trigger_instrument_id.is_synthetic() {
                        // TODO: Impl Actor Trait
                        // self.subscribe_order_book_deltas(&trigger_instrument_id);
                    }
                    // TODO: Impl Actor Trait
                    // self.subscribe_quote_ticks(&trigger_instrument_id)?;
                    self.subscribed_quotes.insert(trigger_instrument_id);
                }
            }
            TriggerType::LastPrice => {
                if !self.subscribed_trades.contains(&trigger_instrument_id) {
                    // TODO: Impl Actor Trait
                    // self.subscribe_trade_ticks(&trigger_instrument_id)?;
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
        if let Err(e) = matching_core
            .add_order(PassiveOrderAny::try_from(order.clone()).expect("passive order conversion"))
        {
            log::error!("Cannot add order: {e:?}");
            return;
        }

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

            msgbus::publish(
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

            if let Err(e) = self.manager.create_new_submit_order(
                order,
                command.position_id,
                Some(command.client_id),
            ) {
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

            // Generate event
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
            );

            self.manager.send_exec_event(OrderEventAny::Updated(event));

            let trigger_instrument_id = order
                .trigger_instrument_id()
                .unwrap_or_else(|| order.instrument_id());

            if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
                matching_core.match_order(
                    &PassiveOrderAny::try_from(order.clone()).expect("passive order conversion"),
                    false,
                );
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
        for order in orders_to_cancel {
            self.manager.cancel_order(&OrderAny::from(order));
        }
    }

    // --------------------------------------------------------------------------------------------

    pub fn update_order(&mut self, order: &mut OrderAny, new_quantity: Quantity) {
        log::info!(
            "Updating order {} quantity to {new_quantity}",
            order.client_order_id(),
        );

        // Generate event
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

    // -----------------------------------------------------------------------------------------------

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

        for order in orders {
            if order.is_closed() {
                continue;
            }

            let mut order: OrderAny = order.clone().into();
            if matches!(
                order.order_type(),
                OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
            ) {
                self.update_trailing_stop_order(&mut order);
            }
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
            && let Err(e) = matching_core.delete_order(
                &PassiveOrderAny::try_from(order.clone()).expect("passive order conversion"),
            )
        {
            log::error!("Cannot delete order: {e:?}");
        }

        self.cache
            .borrow_mut()
            .update_order_pending_cancel_local(&order);

        // Generate event
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
            // Subscribe to all strategy events
            if let Some(handler) = &self.on_event_handler {
                msgbus::subscribe_str(format!("events.order.{strategy_id}"), handler.clone(), None);
                msgbus::subscribe_str(
                    format!("events.position.{strategy_id}"),
                    handler.clone(),
                    None,
                );
                self.subscribed_strategies.insert(strategy_id);
                log::info!("Subscribed to strategy {strategy_id} order and position events");
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
    pub fn trigger_stop_order(&mut self, order: &mut OrderAny) {
        match order.order_type() {
            OrderType::StopLimit | OrderType::LimitIfTouched | OrderType::TrailingStopLimit => {
                self.fill_limit_order(order);
            }
            OrderType::Market | OrderType::MarketIfTouched | OrderType::TrailingStopMarket => {
                self.fill_market_order(order);
            }
            _ => panic!("invalid `OrderType`, was {}", order.order_type()),
        }
    }

    /// # Panics
    ///
    /// Panics if a limit order has no price.
    pub fn fill_limit_order(&mut self, order: &mut OrderAny) {
        if matches!(order.order_type(), OrderType::Limit) {
            self.fill_market_order(order);
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
            match self.validate_release(order, matching_core, trigger_instrument_id) {
                Some(price) => price,
                None => return, // Order stays queued for retry
            };

        let mut command = match self
            .manager
            .pop_submit_order_command(order.client_order_id())
        {
            Some(command) => command,
            None => return, // Order already released
        };

        if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
            if let Err(e) = matching_core.delete_order(
                &PassiveOrderAny::try_from(order.clone()).expect("passive order conversion"),
            ) {
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
                Some(command.client_id),
                true,
            ) {
                log::error!("Failed to add order: {e}");
            }

            // Replace commands order with transformed order
            command.order = OrderAny::Limit(transformed.clone());

            msgbus::publish(
                format!("events.order.{}", order.strategy_id()).into(),
                transformed.last_event(),
            );

            // Generate event
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
            msgbus::publish(
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
    pub fn fill_market_order(&mut self, order: &mut OrderAny) {
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
            match self.validate_release(order, matching_core, trigger_instrument_id) {
                Some(price) => price,
                None => return, // Order stays queued for retry
            };

        let mut command = self
            .manager
            .pop_submit_order_command(order.client_order_id())
            .expect("invalid operation `fill_market_order` with no command");

        if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
            if let Err(e) = matching_core.delete_order(
                &PassiveOrderAny::try_from(order.clone()).expect("passive order conversion"),
            ) {
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
                Some(command.client_id),
                true,
            ) {
                log::error!("Failed to add order: {e}");
            }

            // Replace commands order with transformed order
            command.order = OrderAny::Market(transformed.clone());

            msgbus::publish(
                format!("events.order.{}", order.strategy_id()).into(),
                transformed.last_event(),
            );

            // Generate event
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
            msgbus::publish(
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
