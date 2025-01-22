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
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use anyhow::Result;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    messages::data::DataResponse,
    msgbus::{
        handler::{MessageHandler, ShareableMessageHandler},
        MessageBus,
    },
};
use nautilus_core::uuid::UUID4;
use nautilus_model::{
    data::{Data, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{ContingencyType, OrderSide, OrderStatus, OrderType, TriggerType},
    events::{OrderCanceled, OrderEmulated, OrderEventAny, OrderReleased, OrderUpdated},
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId},
    orders::{LimitOrder, MarketOrder, Order, OrderAny, PassiveOrderAny},
    types::{Price, Quantity},
};
use ustr::Ustr;

use crate::{
    manager::OrderManager,
    matching_core::OrderMatchingCore,
    messages::{
        CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
    trailing::trailing_stop_calculate,
};

pub struct OrderEmulator {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    _msgbus: Rc<RefCell<MessageBus>>,
    manager: Rc<RefCell<OrderManager>>,
    state: Rc<RefCell<OrderEmulatorState>>,
}

struct OrderEmulatorExecuteHandler {
    id: Ustr,
    callback: Box<dyn Fn(&TradingCommand)>,
}

impl MessageHandler for OrderEmulatorExecuteHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&TradingCommand>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl OrderEmulator {
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
    ) -> Self {
        // TODO: Impl Actor Trait
        // self.register_base(portfolio, msgbus, cache, clock);

        let active_local = true;
        let manager = Rc::new(RefCell::new(OrderManager::new(
            clock.clone(),
            msgbus.clone(),
            cache.clone(),
            active_local,
            None,
            None,
            None,
        )));
        let state = Rc::new(RefCell::new(OrderEmulatorState::new(
            clock.clone(),
            cache.clone(),
            msgbus.clone(),
            manager.clone(),
        )));

        let handler = {
            let state = state.clone();
            ShareableMessageHandler(Rc::new(OrderEmulatorExecuteHandler {
                id: Ustr::from(&UUID4::new().to_string()),
                callback: Box::new(move |command: &TradingCommand| {
                    state.borrow_mut().execute(command.clone());
                }),
            }))
        };

        msgbus
            .borrow_mut()
            .register("OrderEmulator.execute", handler);

        Self {
            clock,
            cache,
            _msgbus: msgbus,
            manager,
            state,
        }
    }

    #[must_use]
    pub fn subscribed_quotes(&self) -> Vec<InstrumentId> {
        let mut quotes: Vec<InstrumentId> = self
            .state
            .borrow()
            .subscribed_quotes
            .iter()
            .copied()
            .collect();
        quotes.sort();
        quotes
    }

    #[must_use]
    pub fn subscribed_trades(&self) -> Vec<InstrumentId> {
        let mut trades: Vec<_> = self
            .state
            .borrow()
            .subscribed_trades
            .iter()
            .copied()
            .collect();
        trades.sort();
        trades
    }

    #[must_use]
    pub fn get_submit_order_commands(&self) -> HashMap<ClientOrderId, SubmitOrder> {
        self.manager.borrow().get_submit_order_commands()
    }

    #[must_use]
    pub fn get_matching_core(&self, instrument_id: &InstrumentId) -> Option<OrderMatchingCore> {
        self.state
            .borrow()
            .matching_cores
            .borrow()
            .get(instrument_id)
            .cloned()
    }

    // Action Implementations
    pub fn on_start(&mut self) -> Result<()> {
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
                    self.state.borrow_mut().manager_cancel_order(order.clone());
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

            let command = match SubmitOrder::new(
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
            ) {
                Ok(command) => command,
                Err(e) => {
                    log::error!("Cannot create submit order command: {}", e);
                    continue;
                }
            };

            self.state.borrow_mut().handle_submit_order(command);
        }

        Ok(())
    }

    pub const fn on_stop(&self) {}

    pub fn on_reset(&mut self) {
        self.manager.borrow_mut().reset();
        self.state.borrow_mut().matching_cores.borrow_mut().clear();
    }

    pub const fn on_dispose(&self) {}

    // --------------------------------------------------------------------------------------------

    pub fn update_order(&mut self, order: &mut OrderAny, new_quantity: Quantity) {
        log::info!(
            "Updating order {} quantity to {}",
            order.client_order_id(),
            new_quantity
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
            log::error!("Cannot apply order event: {:?}", e);
            return;
        }
        if let Err(e) = self.cache.borrow_mut().update_order(order) {
            log::error!("Cannot update order: {:?}", e);
            return;
        }

        self.manager
            .borrow()
            .send_risk_event(OrderEventAny::Updated(event));
    }

    // -----------------------------------------------------------------------------------------------

    pub fn on_order_book_deltas(&mut self, deltas: OrderBookDeltas) {
        log::debug!("Processing OrderBookDeltas:{}", deltas);

        let mut matching_core = if let Some(matching_core) = self
            .state
            .borrow()
            .matching_cores
            .borrow()
            .get(&deltas.instrument_id)
        {
            matching_core.clone()
        } else {
            log::error!(
                "Cannot handle `OrderBookDeltas`: no matching core for instrument {}",
                deltas.instrument_id
            );
            return;
        };

        let borrowed_cache = self.cache.borrow();
        let book = if let Some(book) = borrowed_cache.order_book(&deltas.instrument_id) {
            book
        } else {
            log::error!(
                "Cannot handle `OrderBookDeltas`: no book being maintained for {}",
                deltas.instrument_id
            );
            return;
        };

        let best_bid = book.best_bid_price();
        let best_ask = book.best_ask_price();

        if let Some(best_bid) = best_bid {
            matching_core.set_bid_raw(best_bid);
        }

        if let Some(best_ask) = best_ask {
            matching_core.set_ask_raw(best_ask);
        }

        drop(borrowed_cache);
        self.iterate_orders(&mut matching_core);

        self.state
            .borrow_mut()
            .matching_cores
            .borrow_mut()
            .insert(deltas.instrument_id, matching_core);
    }

    pub fn on_quote_tick(&mut self, tick: QuoteTick) {
        log::debug!("Processing QuoteTick:{}", tick);

        let mut matching_core = if let Some(matching_core) = self
            .state
            .borrow()
            .matching_cores
            .borrow()
            .get(&tick.instrument_id)
        {
            matching_core.clone()
        } else {
            log::error!(
                "Cannot handle `QuoteTick`: no matching core for instrument {}",
                tick.instrument_id
            );
            return;
        };

        matching_core.set_bid_raw(tick.bid_price);
        matching_core.set_ask_raw(tick.ask_price);

        self.iterate_orders(&mut matching_core);

        self.state
            .borrow_mut()
            .matching_cores
            .borrow_mut()
            .insert(tick.instrument_id, matching_core);
    }

    pub fn on_trade_tick(&mut self, tick: TradeTick) {
        log::debug!("Processing TradeTick:{}", tick);

        let borrowed_state = self.state.borrow();
        let mut matching_core = if let Some(matching_core) = borrowed_state
            .matching_cores
            .borrow()
            .get(&tick.instrument_id)
        {
            matching_core.clone()
        } else {
            log::error!(
                "Cannot handle `TradeTick`: no matching core for instrument {}",
                tick.instrument_id
            );
            return;
        };

        matching_core.set_last_raw(tick.price);
        if !self
            .state
            .borrow()
            .subscribed_quotes
            .contains(&tick.instrument_id)
        {
            matching_core.set_bid_raw(tick.price);
            matching_core.set_ask_raw(tick.price);
        }

        drop(borrowed_state);
        self.iterate_orders(&mut matching_core);

        self.state
            .borrow_mut()
            .matching_cores
            .borrow_mut()
            .insert(tick.instrument_id, matching_core);
    }

    pub fn on_event(&mut self, event: OrderEventAny) {
        OrderEmulatorState::on_event(
            event,
            self.manager.clone(),
            self.cache.clone(),
            self.state.borrow().matching_cores.clone(),
        );
    }

    fn iterate_orders(&mut self, matching_core: &mut OrderMatchingCore) {
        matching_core.iterate();

        let orders = matching_core.get_orders_ask().iter().cloned();
        for order in orders {
            if order.is_closed() {
                continue;
            }

            let mut order: OrderAny = order.clone().into();
            if matches!(
                order.order_type(),
                OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
            ) {
                self.state
                    .borrow_mut()
                    .update_trailing_stop_order(matching_core, &mut order);
            }
        }
    }
}

struct OrderEmulatorEventHandler {
    id: Ustr,
    callback: Box<dyn Fn(&OrderEventAny)>,
}

impl MessageHandler for OrderEmulatorEventHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&OrderEventAny>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct OrderEmulatorState {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    manager: Rc<RefCell<OrderManager>>,
    matching_cores: Rc<RefCell<HashMap<InstrumentId, OrderMatchingCore>>>,
    subscribed_quotes: HashSet<InstrumentId>,
    subscribed_trades: HashSet<InstrumentId>,
    subscribed_strategies: HashSet<StrategyId>,
    monitored_positions: HashSet<PositionId>,
}

impl OrderEmulatorState {
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        manager: Rc<RefCell<OrderManager>>,
    ) -> Self {
        Self {
            manager,
            matching_cores: Rc::new(RefCell::new(HashMap::new())),
            subscribed_quotes: HashSet::new(),
            subscribed_trades: HashSet::new(),
            subscribed_strategies: HashSet::new(),
            monitored_positions: HashSet::new(),
            clock,
            cache,
            msgbus,
        }
    }

    pub fn execute(&mut self, command: TradingCommand) {
        log::info!("{RECV}{CMD} {command}");

        match command {
            TradingCommand::SubmitOrder(command) => self.handle_submit_order(command),
            TradingCommand::SubmitOrderList(command) => self.handle_submit_order_list(command),
            TradingCommand::ModifyOrder(command) => self.handle_modify_order(command),
            TradingCommand::CancelOrder(command) => self.handle_cancel_order(command),
            TradingCommand::CancelAllOrders(command) => self.handle_cancel_all_orders(command),
            _ => log::error!("Cannot handle command: unrecognized {:?}", command),
        }
    }

    fn handle_submit_order(&mut self, command: SubmitOrder) {
        let mut order = command.order.clone();
        let emulation_trigger = order.emulation_trigger();

        assert!(
            emulation_trigger != Some(TriggerType::NoTrigger),
            "command.order.emulation_trigger must not be TriggerType::NoTrigger"
        );
        assert!(
            self.manager
                .borrow()
                .get_submit_order_commands()
                .contains_key(&order.client_order_id()),
            "command.order.client_order_id must be in submit_order_commands"
        );

        if !matches!(
            emulation_trigger,
            Some(TriggerType::Default | TriggerType::BidAsk | TriggerType::LastPrice)
        ) {
            log::error!(
                "Cannot emulate order: `TriggerType` {:?} not supported",
                emulation_trigger
            );
            self.manager_cancel_order(order.clone());
            return;
        }

        self.check_monitoring(command.strategy_id, command.position_id);

        // Get or create matching core
        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());

        let matching_core = self
            .matching_cores
            .borrow()
            .get(&trigger_instrument_id)
            .cloned();

        let mut matching_core = if let Some(core) = matching_core {
            core
        } else {
            // Handle synthetic instruments
            let (instrument_id, price_increment) = match trigger_instrument_id.is_synthetic() {
                true => {
                    let synthetic = self
                        .cache
                        .borrow()
                        .synthetic(&trigger_instrument_id)
                        .cloned();
                    if let Some(synthetic) = synthetic {
                        (synthetic.id, synthetic.price_increment)
                    } else {
                        log::error!(
                            "Cannot emulate order: no synthetic instrument {} for trigger",
                            trigger_instrument_id
                        );
                        self.manager_cancel_order(order.clone());
                        return;
                    }
                }
                false => {
                    let instrument = self
                        .cache
                        .borrow()
                        .instrument(&trigger_instrument_id)
                        .cloned();
                    if let Some(instrument) = instrument {
                        (instrument.id(), instrument.price_increment())
                    } else {
                        log::error!(
                            "Cannot emulate order: no instrument {} for trigger",
                            trigger_instrument_id
                        );
                        self.manager_cancel_order(order.clone());
                        return;
                    }
                }
            };

            self.create_matching_core(instrument_id, price_increment)
        };

        // Update trailing stop
        if matches!(
            order.order_type(),
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
        ) {
            self.update_trailing_stop_order(&matching_core, &mut order);
            if order.trigger_price().is_none() {
                log::error!(
                    "Cannot handle trailing stop order with no trigger_price and no market updates"
                );

                self.manager_cancel_order(order.clone());
                return;
            }
        }

        // Cache command
        self.manager
            .borrow_mut()
            .cache_submit_order_command(command);

        // Check if immediately marketable
        matching_core.match_order(&PassiveOrderAny::from(order.clone()), true);

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
                log::error!("Invalid TriggerType: {:?}", emulation_trigger);
                return;
            }
        }

        // Check if order was already released
        if !self
            .manager
            .borrow()
            .get_submit_order_commands()
            .contains_key(&order.client_order_id())
        {
            return; // Already released
        }

        // Hold in matching core
        if let Err(e) = matching_core.add_order(PassiveOrderAny::from(order.clone())) {
            log::error!("Cannot add order: {:?}", e);
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
                log::error!("Cannot apply order event: {:?}", e);
                return;
            }

            if let Err(e) = self.cache.borrow_mut().update_order(&order) {
                log::error!("Cannot update order: {:?}", e);
                return;
            }

            self.manager
                .borrow()
                .send_risk_event(OrderEventAny::Emulated(event));

            self.msgbus.borrow().publish(
                &format!("events.order.{}", order.strategy_id()).into(),
                &OrderEventAny::Emulated(event),
            );
        }

        // Since we are cloning the matching core, we need to insert it back into the original hashmap
        self.matching_cores
            .borrow_mut()
            .insert(trigger_instrument_id, matching_core);

        log::info!("Emulating {}", order);
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

            if let Err(e) = self.manager.borrow_mut().create_new_submit_order(
                order.clone(),
                command.position_id,
                Some(command.client_id),
            ) {
                log::error!("Error creating new submit order: {}", e);
            }
        }
    }

    fn handle_modify_order(&mut self, command: ModifyOrder) {
        let cache = self.cache.borrow();
        let order = if let Some(order) = cache.order(&command.client_order_id) {
            order
        } else {
            log::error!("Cannot modify order: {} not found", command.client_order_id);
            return;
        };

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

        self.manager
            .borrow()
            .send_exec_event(OrderEventAny::Updated(event));

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());

        let borrowed_matching_cores = self.matching_cores.borrow();
        let matching_core = if let Some(core) = borrowed_matching_cores.get(&trigger_instrument_id)
        {
            core
        } else {
            log::error!(
                "Cannot handle `ModifyOrder`: no matching core for trigger instrument {}",
                trigger_instrument_id
            );
            return;
        };

        matching_core.match_order(&PassiveOrderAny::from(order.clone()), false);

        // TODO: fix
        // match order.order_side() {
        //     OrderSide::Buy => matching_core.get_orders_bid().sort(),
        //     OrderSide::Sell => matching_core.get_orders_ask().sort(),
        //     _ => return Err(anyhow::anyhow!("Invalid OrderSide")),
        // }
    }

    fn handle_cancel_order(&mut self, command: CancelOrder) {
        let order = if let Some(order) = self.cache.borrow().order(&command.client_order_id) {
            order.clone()
        } else {
            log::error!("Cannot cancel order: {} not found", command.client_order_id);
            return;
        };

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());

        let borrowed_matching_cores = self.matching_cores.borrow();
        let matching_core = if let Some(core) = borrowed_matching_cores.get(&trigger_instrument_id)
        {
            core
        } else {
            drop(borrowed_matching_cores);
            self.manager_cancel_order(order);
            return;
        };

        if !matching_core.order_exists(order.client_order_id())
            && order.is_open()
            && !order.is_pending_cancel()
        {
            // Order not held in the emulator
            self.manager
                .borrow()
                .send_exec_command(TradingCommand::CancelOrder(command));
        } else {
            drop(borrowed_matching_cores);
            self.manager_cancel_order(order);
        }
    }

    fn handle_cancel_all_orders(&mut self, command: CancelAllOrders) {
        let borrowed_matching_cores = self.matching_cores.borrow();
        let matching_core = match borrowed_matching_cores.get(&command.instrument_id) {
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

        drop(borrowed_matching_cores);

        // Process all orders in a single iteration
        for order in orders_to_cancel {
            let order: OrderAny = order.into();
            self.manager_cancel_order(order);
        }
    }

    // Cloned from manager to bypass few layers of handlers;
    fn manager_cancel_order(&mut self, order: OrderAny) {
        if self
            .cache
            .borrow()
            .is_order_pending_cancel_local(&order.client_order_id())
        {
            return;
        }

        if order.is_closed() {
            log::warn!("Cannot cancel order: already closed");
            return;
        }

        self.manager
            .borrow_mut()
            .pop_submit_order_command(order.client_order_id());

        // OrderEmulator.cancel_order
        self.cancel_order(&order);
    }

    fn cancel_order(&mut self, order: &OrderAny) {
        log::info!("Canceling order {}", order.client_order_id());

        let mut order = order.clone();
        order.set_emulation_trigger(Some(TriggerType::NoTrigger));

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());

        if let Some(matching_core) = self
            .matching_cores
            .borrow_mut()
            .get_mut(&trigger_instrument_id)
        {
            if let Err(e) = matching_core.delete_order(&PassiveOrderAny::from(order.clone())) {
                log::error!("Cannot delete order: {:?}", e);
            }
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
        self.manager
            .borrow()
            .send_exec_event(OrderEventAny::Canceled(event));
    }

    fn check_monitoring(&mut self, strategy_id: StrategyId, position_id: Option<PositionId>) {
        if !self.subscribed_strategies.contains(&strategy_id) {
            let handler = {
                let manager = self.manager.clone();
                let cache = self.cache.clone();
                let matching_cores = self.matching_cores.clone();
                ShareableMessageHandler(Rc::new(OrderEmulatorEventHandler {
                    id: Ustr::from(&UUID4::new().to_string()),
                    callback: Box::new(move |event: &OrderEventAny| {
                        Self::on_event(
                            event.clone(),
                            manager.clone(),
                            cache.clone(),
                            matching_cores.clone(),
                        );
                    }),
                }))
            };

            // Subscribe to all strategy events
            self.msgbus.borrow_mut().subscribe(
                format!("events.order.{strategy_id}"),
                handler.clone(),
                None,
            );
            self.msgbus.borrow_mut().subscribe(
                format!("events.position.{strategy_id}"),
                handler,
                None,
            );

            self.subscribed_strategies.insert(strategy_id);
            log::info!(
                "Subscribed to strategy {} order and position events",
                strategy_id
            );
        }

        if let Some(position_id) = position_id {
            if !self.monitored_positions.contains(&position_id) {
                self.monitored_positions.insert(position_id);
            }
        }
    }

    fn on_event(
        event: OrderEventAny,
        manager: Rc<RefCell<OrderManager>>,
        cache: Rc<RefCell<Cache>>,
        matching_cores: Rc<RefCell<HashMap<InstrumentId, OrderMatchingCore>>>,
    ) {
        log::info!("{RECV}{EVT} {event}");

        manager.borrow_mut().handle_event(event.clone());

        if let Some(order) = cache.borrow().order(&event.client_order_id()) {
            if order.is_closed() {
                if let Some(matching_core) =
                    matching_cores.borrow_mut().get_mut(&order.instrument_id())
                {
                    if let Err(e) =
                        matching_core.delete_order(&PassiveOrderAny::from(order.clone()))
                    {
                        log::error!("Error deleting order: {}", e);
                    }
                }
            }
        }
        // else: Order not in cache yet
    }

    fn create_matching_core(
        &mut self,
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> OrderMatchingCore {
        let matching_core =
            OrderMatchingCore::new(instrument_id, price_increment, None, None, None);
        self.matching_cores
            .borrow_mut()
            .insert(instrument_id, matching_core.clone());
        log::info!("Creating matching core for {:?}", instrument_id);
        matching_core
    }

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

    fn fill_limit_order(&mut self, order: &mut OrderAny) {
        if matches!(order.order_type(), OrderType::Limit) {
            self.fill_market_order(order);
            return;
        }

        // Fetch command
        let mut command = match self
            .manager
            .borrow_mut()
            .pop_submit_order_command(order.client_order_id())
        {
            Some(command) => command,
            None => return, // Order already released
        };

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());

        let mut matching_core = self
            .matching_cores
            .borrow()
            .get(&trigger_instrument_id)
            .cloned();
        if let Some(ref mut matching_core) = matching_core {
            if let Err(e) = matching_core.delete_order(&PassiveOrderAny::from(order.clone())) {
                log::error!("Cannot delete order: {:?}", e);
            } else {
                // Update matching cores
                self.matching_cores
                    .borrow_mut()
                    .insert(trigger_instrument_id, matching_core.clone());
            }
        }

        let emulation_trigger = TriggerType::NoTrigger;

        // Transform order
        let mut transformed = if let Ok(transformed) = LimitOrder::new(
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
            order.linked_order_ids(),
            order.parent_order_id(),
            order.exec_algorithm_id(),
            order.exec_algorithm_params(),
            order.exec_spawn_id(),
            order.tags(),
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
            log::error!("Failed to add order: {}", e);
        }

        // Replace commands order with transformed order
        command.order = OrderAny::Limit(transformed.clone());

        self.msgbus.borrow().publish(
            &format!("events.order.{}", order.strategy_id()).into(),
            transformed.last_event(),
        );

        // Determine triggered price
        // TODO: fix unwraps
        let released_price = match order.order_side() {
            OrderSide::Buy => matching_core.unwrap().ask,
            OrderSide::Sell => matching_core.unwrap().bid,
            _ => panic!("invalid `OrderSide`"),
        };

        // Generate event
        let event = OrderReleased::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            released_price.unwrap(),
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            self.clock.borrow().timestamp_ns(),
        );

        if let Err(e) = transformed.apply(OrderEventAny::Released(event)) {
            log::error!("Failed to apply order event: {}", e);
        }
        if let Err(e) = self
            .cache
            .borrow_mut()
            .update_order(&OrderAny::Limit(transformed.clone()))
        {
            log::error!("Failed to update order: {}", e);
        }

        self.manager
            .borrow()
            .send_risk_event(OrderEventAny::Released(event));

        log::info!("Releasing order {}", order.client_order_id());

        // Publish event
        self.msgbus.borrow().publish(
            &format!("events.order.{}", transformed.strategy_id()).into(),
            &OrderEventAny::Released(event),
        );

        if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
            self.manager
                .borrow()
                .send_algo_command(command, exec_algorithm_id);
        } else {
            self.manager
                .borrow()
                .send_exec_command(TradingCommand::SubmitOrder(command));
        }
    }

    fn fill_market_order(&mut self, order: &mut OrderAny) {
        // Fetch command
        let mut command = match self
            .manager
            .borrow_mut()
            .pop_submit_order_command(order.client_order_id())
        {
            Some(command) => command,
            None => panic!("invalid operation `_fill_market_order` with no command"),
        };

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());

        let mut matching_core = self
            .matching_cores
            .borrow()
            .get(&trigger_instrument_id)
            .cloned();
        if let Some(ref mut matching_core) = matching_core {
            if let Err(e) = matching_core.delete_order(&PassiveOrderAny::from(order.clone())) {
                log::error!("Cannot delete order: {:?}", e);
            } else {
                // Update matching cores
                self.matching_cores
                    .borrow_mut()
                    .insert(trigger_instrument_id, matching_core.clone());
            }
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
            order.linked_order_ids(),
            order.parent_order_id(),
            order.exec_algorithm_id(),
            order.exec_algorithm_params(),
            order.exec_spawn_id(),
            order.tags(),
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
            log::error!("Failed to add order: {}", e);
        }

        // Replace commands order with transformed order
        command.order = OrderAny::Market(transformed.clone());

        self.msgbus.borrow().publish(
            &format!("events.order.{}", order.strategy_id()).into(),
            transformed.last_event(),
        );

        // Determine triggered price
        // TODO: fix unwraps
        let released_price = match order.order_side() {
            OrderSide::Buy => matching_core.unwrap().ask,
            OrderSide::Sell => matching_core.unwrap().bid,
            _ => panic!("invalid `OrderSide`"),
        };

        // Generate event
        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderReleased::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            released_price.unwrap(),
            UUID4::new(),
            ts_now,
            ts_now,
        );

        if let Err(e) = transformed.apply(OrderEventAny::Released(event)) {
            log::error!("Failed to apply order event: {}", e);
        }

        if let Err(e) = self
            .cache
            .borrow_mut()
            .update_order(&OrderAny::Market(transformed))
        {
            log::error!("Failed to update order: {}", e);
        }
        self.manager
            .borrow()
            .send_risk_event(OrderEventAny::Released(event));

        log::info!("Releasing order {}", order.client_order_id());

        // Publish event
        self.msgbus.borrow().publish(
            &format!("events.order.{}", order.strategy_id()).into(),
            &OrderEventAny::Released(event),
        );

        if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
            self.manager
                .borrow()
                .send_algo_command(command, exec_algorithm_id);
        } else {
            self.manager
                .borrow()
                .send_exec_command(TradingCommand::SubmitOrder(command));
        }
    }

    fn update_trailing_stop_order(&self, matching_core: &OrderMatchingCore, order: &mut OrderAny) {
        let mut bid = None;
        let mut ask = None;
        let mut last = None;

        if matching_core.is_bid_initialized {
            bid = matching_core.bid;
        }
        if matching_core.is_ask_initialized {
            ask = matching_core.ask;
        }
        if matching_core.is_last_initialized {
            last = matching_core.last;
        }

        let quote_tick = self
            .cache
            .borrow()
            .quote(&matching_core.instrument_id)
            .copied();
        let trade_tick = self
            .cache
            .borrow()
            .trade(&matching_core.instrument_id)
            .copied();

        if bid.is_none() && quote_tick.is_some() {
            bid = Some(quote_tick.unwrap().bid_price);
        }
        if ask.is_none() && quote_tick.is_some() {
            ask = Some(quote_tick.unwrap().ask_price);
        }
        if last.is_none() && trade_tick.is_some() {
            last = Some(trade_tick.unwrap().price);
        }

        let (new_trigger_price, new_price) = if let Ok((new_trigger_price, new_price)) =
            trailing_stop_calculate(matching_core.price_increment, order, bid, ask, last)
        {
            (new_trigger_price, new_price)
        } else {
            log::warn!("Cannot calculate trailing stop order");
            return;
        };

        let (new_trigger_price, new_price) = match (new_trigger_price, new_price) {
            (None, None) => return, // No updates
            _ => (new_trigger_price, new_price),
        };

        // Generate event
        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderUpdated::new(
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
            new_price,
            new_trigger_price,
        );

        if let Err(e) = order.apply(OrderEventAny::Updated(event)) {
            log::error!("Failed to apply order event: {}", e);
        }
        if let Err(e) = self.cache.borrow_mut().update_order(order) {
            log::error!("Failed to update order: {}", e);
        }

        self.manager
            .borrow()
            .send_risk_event(OrderEventAny::Updated(event));
    }
}
