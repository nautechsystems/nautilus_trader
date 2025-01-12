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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use anyhow::Result;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    msgbus::MessageBus,
};
use nautilus_core::uuid::UUID4;
use nautilus_model::{
    data::{OrderBookDeltas, QuoteTick, TradeTick},
    enums::{ContingencyType, OrderSide, OrderStatus, OrderType, TriggerType},
    events::{OrderCanceled, OrderEmulated, OrderEventAny, OrderUpdated},
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId},
    orders::{OrderAny, PassiveOrderAny},
    types::{Price, Quantity},
};

use crate::{
    manager::OrderManager,
    matching_core::OrderMatchingCore,
    messages::{
        CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
};

pub struct OrderEmulator {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    manager: OrderManager,
    matching_cores: HashMap<InstrumentId, OrderMatchingCore>,
    subscribed_quotes: HashSet<InstrumentId>,
    subscribed_trades: HashSet<InstrumentId>,
    subscribed_strategies: HashSet<StrategyId>,
    monitored_positions: HashSet<PositionId>,
}

impl OrderEmulator {
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        manager: OrderManager,
        debug: bool,
    ) -> Self {
        // todo: at last
        Self {
            clock,
            cache,
            msgbus,
            manager,
            matching_cores: HashMap::new(),
            subscribed_quotes: HashSet::new(),
            subscribed_trades: HashSet::new(),
            subscribed_strategies: HashSet::new(),
            monitored_positions: HashSet::new(),
        }
    }

    #[must_use]
    pub fn subscribed_quotes(&self) -> Vec<InstrumentId> {
        let mut quotes: Vec<_> = self.subscribed_quotes.iter().copied().collect();
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
    pub fn get_matching_core(&self, instrument_id: &InstrumentId) -> Option<&OrderMatchingCore> {
        self.matching_cores.get(instrument_id)
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
            log::info!("No emulated orders to reactivate");
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
                let cache = self.cache.borrow();
                let parent_order = cache.order(parent_order_id).ok_or_else(|| {
                    anyhow::anyhow!("Cannot handle order: parent {parent_order_id} not found")
                })?;

                let position_id = parent_order.position_id();

                if parent_order.is_closed()
                    && (position_id.is_none()
                        || self
                            .cache
                            .borrow()
                            .is_position_closed(&position_id.unwrap()))
                {
                    self.manager.cancel_order(order.clone());
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

            // fix unwraps
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

            // todo: complete this
            self.handle_submit_order(command);
        }

        Ok(())
    }

    pub fn on_event(&mut self, event: OrderEventAny) {
        log::info!("{RECV}{EVT} {event}");

        self.manager.handle_event(event.clone());

        let order = match self.cache.borrow().order(&event.client_order_id()) {
            Some(order) => order.clone(),
            None => return, // Order not in cache yet
        };

        if order.is_closed() {
            if let Some(matching_core) = self.matching_cores.get_mut(&order.instrument_id()) {
                if let Err(e) = matching_core.delete_order(&PassiveOrderAny::from(order)) {
                    log::error!("Error deleting order: {}", e);
                }
            }
        }
    }

    pub const fn on_stop(&self) {}

    pub fn on_reset(&mut self) {
        self.manager.reset();
        self.matching_cores.clear();
    }

    pub const fn on_dispose(&self) {}

    // --------------------------------------------------------------------------------------------

    pub fn execute(&mut self, command: TradingCommand) {
        log::info!("{RECV}{CMD} {command}");

        match command {
            TradingCommand::SubmitOrder(command) => self.handle_submit_order(command),
            TradingCommand::SubmitOrderList(command) => {
                self.handle_submit_order_list(command).unwrap();
            }
            TradingCommand::ModifyOrder(command) => self.handle_modify_order(command).unwrap(),
            TradingCommand::CancelOrder(command) => self.handle_cancel_order(command).unwrap(),
            TradingCommand::CancelAllOrders(command) => {
                self.handle_cancel_all_orders(command).unwrap();
            }
            _ => {
                log::error!("Cannot handle command: unrecognized {:?}", command);
            }
        }
    }

    // keep it in Outer
    pub fn create_matching_core(
        &mut self,
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> OrderMatchingCore {
        // let matching_core = OrderMatchingCore::new(
        //     instrument_id,
        //     price_increment,
        //     s,
        //     fill_market_order,
        //     fill_limit_order,
        // );
        // self.matching_cores.insert(instrument_id, matching_core);
        log::info!("Creating matching core for {:?}", instrument_id);
        // matching_core
        todo!()
    }

    fn handle_submit_order(&mut self, command: SubmitOrder) {
        let mut order = command.order.clone();
        let emulation_trigger = order.emulation_trigger();

        // Condition.not_equal(emulation_trigger, TriggerType.NO_TRIGGER, "command.order.emulation_trigger", "TriggerType.NO_TRIGGER")
        // Condition.not_in(command.order.client_order_id, self._manager.get_submit_order_commands(), "command.order.client_order_id", "manager.submit_order_commands")

        if !matches!(
            emulation_trigger,
            Some(TriggerType::Default | TriggerType::BidAsk | TriggerType::LastPrice)
        ) {
            log::error!(
                "Cannot emulate order: `TriggerType` {:?} not supported",
                emulation_trigger
            );
            self.manager.cancel_order(order.clone());
            return;
        }
        // todo: fix unwrap
        self.check_monitoring(command.strategy_id, command.position_id.unwrap());

        // Get or create matching core
        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());

        let mut matching_core = if let Some(core) = self.matching_cores.get(&trigger_instrument_id)
        {
            core.clone()
        } else {
            // Handle synthetic instruments
            if trigger_instrument_id.is_synthetic() {
                let synthetic = self
                    .cache
                    .borrow()
                    .synthetic(&trigger_instrument_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Cannot emulate order: no synthetic instrument {} for trigger",
                            trigger_instrument_id
                        )
                    })
                    .unwrap()
                    .clone();
                self.create_matching_core(synthetic.id, synthetic.price_increment)
            } else {
                let instrument = self
                    .cache
                    .borrow()
                    .instrument(&trigger_instrument_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Cannot emulate order: no instrument {} for trigger",
                            trigger_instrument_id
                        )
                    })
                    .unwrap()
                    .clone();
                self.create_matching_core(instrument.id(), instrument.price_increment())
            }
        };

        // Handle trailing stop orders
        if matches!(
            order.order_type(),
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
        ) {
            // todo: fix
            self.update_trailing_stop_order(
                &mut matching_core,
                &PassiveOrderAny::from(order.clone()),
            );
            if order.trigger_price().is_none() {
                log::error!(
                    "Cannot handle trailing stop order with no trigger_price and no market updates"
                );
                self.manager.cancel_order(order.clone());
                return;
            }
        }

        // Cache command
        self.manager.cache_submit_order_command(command);

        // Check if immediately marketable
        // matching_core.match_order(&PassiveOrderAny::from(order), true);
        matching_core.match_order(&PassiveOrderAny::from(order.clone()), true);

        // Handle data subscriptions
        match emulation_trigger.unwrap() {
            TriggerType::Default | TriggerType::BidAsk => {
                if !self.subscribed_quotes.contains(&trigger_instrument_id) {
                    if !trigger_instrument_id.is_synthetic() {
                        // todo: fix after completing Actor Trait
                        // self.subscribe_order_book_deltas(&trigger_instrument_id);
                    }
                    // todo: fix
                    // self.subscribe_quote_ticks(&trigger_instrument_id)?;
                    self.subscribed_quotes.insert(trigger_instrument_id);
                }
            }
            TriggerType::LastPrice => {
                if !self.subscribed_trades.contains(&trigger_instrument_id) {
                    // todo: fix
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
            .get_submit_order_commands()
            .contains_key(&order.client_order_id())
        {
            return; // Already released
        }

        // Hold in matching core
        matching_core
            .add_order(PassiveOrderAny::from(order.clone()))
            .unwrap();

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

            order.apply(OrderEventAny::Emulated(event)).unwrap();
            self.cache.borrow_mut().update_order(&order).unwrap();
            self.manager.send_risk_event(OrderEventAny::Emulated(event));

            // todo: check diff of passing event directly vs wrapping in OrderEventAny??
            self.msgbus.borrow().publish(
                &format!("events.order.{}", order.strategy_id()).into(),
                &OrderEventAny::Emulated(event),
            );
        }

        // Since we are cloning the matching core, we need to insert it back into the map
        self.matching_cores
            .insert(trigger_instrument_id, matching_core);

        log::info!("Emulating {}", order);
    }

    fn handle_submit_order_list(&mut self, command: SubmitOrderList) -> Result<()> {
        // todo: fix unwrap
        self.check_monitoring(command.strategy_id, command.position_id.unwrap());

        for order in &command.order_list.orders {
            if let Some(parent_order_id) = order.parent_order_id() {
                let cache = self.cache.borrow();
                let parent_order = cache.order(&parent_order_id).ok_or_else(|| {
                    anyhow::anyhow!("Parent order for {} not found", order.client_order_id())
                })?;

                if parent_order.contingency_type() == Some(ContingencyType::Oto) {
                    continue; // Process contingency order later once parent triggered
                }
            }

            self.manager.create_new_submit_order(
                order.clone(),
                command.position_id,
                Some(command.client_id),
            )?;
        }

        Ok(())
    }

    fn handle_modify_order(&mut self, command: ModifyOrder) -> Result<()> {
        let order = self
            .cache
            .borrow()
            .order(&command.client_order_id)
            .ok_or_else(|| {
                log::error!("Cannot modify order: {} not found", command.client_order_id);
            })
            .unwrap()
            .clone();

        // Determine price values
        let price = match command.price {
            Some(price) => Some(price),
            None => {
                if order.has_price() {
                    order.price()
                } else {
                    None
                }
            }
        };

        let trigger_price = match command.trigger_price {
            Some(trigger_price) => Some(trigger_price),
            None => {
                if order.price().is_some() {
                    order.trigger_price()
                } else {
                    None
                }
            }
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

        let matching_core = self
            .matching_cores
            .get_mut(&trigger_instrument_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot handle ModifyOrder: no matching core for trigger instrument {}",
                    trigger_instrument_id
                )
            })?;

        matching_core.match_order(&PassiveOrderAny::from(order), false);

        // todo: complete this + fix clones
        // match order.order_side() {
        //     OrderSide::Buy => matching_core.sort_bid_orders(),
        //     OrderSide::Sell => matching_core.sort_ask_orders(),
        //     _ => return Err(anyhow::anyhow!("Invalid OrderSide")),
        // }

        Ok(())
    }

    fn handle_cancel_order(&mut self, command: CancelOrder) -> Result<()> {
        let order = self
            .cache
            .borrow()
            .order(&command.client_order_id)
            .ok_or_else(|| {
                anyhow::anyhow!("Cannot cancel order: {} not found", command.client_order_id)
            })?
            .clone();

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or_else(|| order.instrument_id());

        let matching_core = if let Some(core) = self.matching_cores.get(&trigger_instrument_id) {
            core
        } else {
            self.manager.cancel_order(order);
            return Ok(());
        };

        if !matching_core.order_exists(order.client_order_id())
            && order.is_open()
            && !order.is_pending_cancel()
        {
            // Order not held in the emulator
            self.manager
                .send_exec_command(TradingCommand::CancelOrder(command));
        } else {
            self.manager.cancel_order(order);
        }

        Ok(())
    }

    fn handle_cancel_all_orders(&mut self, command: CancelAllOrders) -> Result<()> {
        let matching_core = match self.matching_cores.get(&command.instrument_id) {
            Some(core) => core,
            None => return Ok(()), // No orders to cancel
        };

        let orders = match command.order_side {
            // fix
            // OrderSide::NoOrderSide => matching_core.get_orders(),
            OrderSide::Buy => matching_core.get_orders_bid(),
            OrderSide::Sell => matching_core.get_orders_ask(),
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid OrderSide: {:?}",
                    command.order_side
                ))
            }
        };

        for order in orders {
            // todo: fix
            self.manager.cancel_order(order.clone().into());
        }

        Ok(())
    }

    fn check_monitoring(&mut self, strategy_id: StrategyId, position_id: PositionId) {
        // todo: fix: add handler
        if !self.subscribed_strategies.contains(&strategy_id) {
            // Subscribe to all strategy events
            // self.msgbus.borrow().subscribe(
            //     format!("events.order.{}", strategy_id),
            //     self.on_event,
            //     None,
            // );
            // self.msgbus.borrow().subscribe(
            //     format!("events.position.{}", strategy_id),
            //     self.on_event,
            //     None,
            // );
            self.subscribed_strategies.insert(strategy_id);
            log::info!(
                "Subscribed to strategy {} order and position events",
                strategy_id
            );
        }

        if !self.monitored_positions.contains(&position_id) {
            self.monitored_positions.insert(position_id);
        }
    }

    fn cancel_order(&mut self, order: &OrderAny) {
        log::info!("Canceling order {}", order.client_order_id());

        let mut order = order.clone();
        order.set_emulation_trigger(Some(TriggerType::NoTrigger));

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());
        let matching_core = self.matching_cores.get_mut(&trigger_instrument_id);
        if let Some(matching_core) = matching_core {
            matching_core
                .delete_order(&PassiveOrderAny::from(order.clone()))
                .unwrap();
        }

        self.cache
            .borrow_mut()
            .update_order_pending_cancel_local(&order);

        // Generate event
        let ts_now = self.clock.borrow().timestamp_ns();
        let event: OrderCanceled = OrderCanceled::new(
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

    fn update_order(&mut self, order: &mut OrderAny, new_quantity: Quantity) {
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

        order.apply(OrderEventAny::Updated(event)).unwrap();
        self.cache.borrow_mut().update_order(order).unwrap();

        self.manager.send_risk_event(OrderEventAny::Updated(event));
    }

    // -----------------------------------------------------------------------------------------------

    fn trigger_stop_order(&mut self, order: &OrderAny) {
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

    fn fill_market_order(&mut self, order: &OrderAny) {
        // Fetch command
        let command = match self
            .manager
            .pop_submit_order_command(order.client_order_id())
        {
            Some(command) => command,
            None => panic!("invalid operation `_fill_market_order` with no command"),
        };

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());
        if let Some(matching_core) = self.matching_cores.get_mut(&trigger_instrument_id) {
            matching_core
                .delete_order(&PassiveOrderAny::from(order.clone()))
                .unwrap();
        }

        let emulation_trigger = TriggerType::NoTrigger;

        // TODO
        // cdef MarketOrder transformed = MarketOrder.transform(order, self.clock.timestamp_ns())
    }

    fn fill_limit_order(&mut self, order: &OrderAny) {
        if matches!(order.order_type(), OrderType::Limit) {
            self.fill_market_order(order);
            return;
        }

        // Fetch command
        let command = match self
            .manager
            .pop_submit_order_command(order.client_order_id())
        {
            Some(command) => command,
            None => return, // Order already released
        };

        let trigger_instrument_id = order
            .trigger_instrument_id()
            .unwrap_or(order.instrument_id());
        let matching_core = self.matching_cores.get_mut(&trigger_instrument_id);
        if let Some(matching_core) = matching_core {
            matching_core
                .delete_order(&PassiveOrderAny::from(order.clone()))
                .unwrap();
        }

        let emulation_trigger = TriggerType::NoTrigger;

        // TODO
        // cdef MarketOrder transformed = MarketOrder.transform(order, self.clock.timestamp_ns())
    }

    pub fn on_order_book_deltas(&mut self, deltas: OrderBookDeltas) {
        log::debug!("Processing OrderBookDeltas:{}", deltas);

        //     let matching_core = match self.matching_cores.get_mut(&deltas.instrument_id) {
        //         Some(matching_core) => matching_core,
        //         None => {
        //             log::error!(
        //                 "Cannot handle `OrderBookDeltas`: no matching core for instrument {}",
        //                 deltas.instrument_id
        //             );
        //             return;
        //         }
        //     };

        //     let borrowed_cache = self.cache.borrow();
        //     let book = match borrowed_cache.order_book(&deltas.instrument_id) {
        //         Some(book) => book,
        //         None => {
        //             log::error!(
        //                 "Cannot handle `OrderBookDeltas`: no book being maintained for {}",
        //                 deltas.instrument_id
        //             );
        //             return;
        //         }
        //     };

        //     let best_bid = book.best_bid_price();
        //     let best_ask = book.best_ask_price();

        //     if let Some(best_bid) = best_bid {
        //         matching_core.set_bid_raw(best_bid);
        //     }

        //     if let Some(best_ask) = best_ask {
        //         matching_core.set_ask_raw(best_ask);
        //     }

        //     self.iterate_orders(matching_core).unwrap();
    }

    fn on_quote_tick(&mut self, tick: QuoteTick) {
        //     log::debug!("Processing QuoteTick:{}", tick);

        //     let matching_core = match self.matching_cores.get_mut(&tick.instrument_id) {
        //         Some(matching_core) => matching_core,
        //         None => {
        //             log::error!(
        //                 "Cannot handle `QuoteTick`: no matching core for instrument {}",
        //                 tick.instrument_id
        //             );
        //             return;
        //         }
        //     };

        //     matching_core.set_bid_raw(tick.bid_price);
        //     matching_core.set_ask_raw(tick.ask_price);

        //     self.iterate_orders(matching_core).unwrap();
    }

    fn on_trade_tick(&mut self, tick: TradeTick) {
        //     log::debug!("Processing TradeTick:{}", tick);

        //     let matching_core = match self.matching_cores.get_mut(&tick.instrument_id) {
        //         Some(matching_core) => matching_core,
        //         None => {
        //             log::error!(
        //                 "Cannot handle `TradeTick`: no matching core for instrument {}",
        //                 tick.instrument_id
        //             );
        //             return;
        //         }
        //     };

        //     matching_core.set_last_raw(tick.price);
        //     if !self.subscribed_quotes.contains(&tick.instrument_id) {
        //         matching_core.set_bid_raw(tick.price);
        //         matching_core.set_ask_raw(tick.price);
        //     }

        //     self.iterate_orders(matching_core).unwrap();
    }

    fn iterate_orders(&mut self, matching_core: &mut OrderMatchingCore) -> Result<()> {
        matching_core.iterate();

        let orders = matching_core.get_orders_ask();
        // for order in orders {
        //     if order.is_closed() {
        //         continue;
        //     }

        //     // Manage trailing stop
        //     if order.order_type() == OrderType::TrailingStopMarket
        //         || order.order_type() == OrderType::TrailingStopLimit
        //     {
        //         self.update_trailing_stop_order(matching_core, order);
        //     }
        // }

        Ok(())
    }

    fn update_trailing_stop_order(
        &self,
        matching_core: &mut OrderMatchingCore,
        order: &PassiveOrderAny,
    ) {
        // let mut bid = None;
        // let mut ask = None;
        // let mut last = None;

        // if matching_core.is_bid_initialized {
        //     bid = matching_core.bid;
        // }
        // if matching_core.is_ask_initialized {
        //     ask = matching_core.ask;
        // }
        // if matching_core.is_last_initialized {
        //     last = matching_core.last;
        // }

        // TODO
        // let output = Trail::calculate(
        //     price_increment: matching_core.price_increment,
        //     order: order,
        //     bid: bid,
        //     ask: ask,
        //     last: last,
        // );
    }
}
