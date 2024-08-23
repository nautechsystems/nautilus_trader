// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! An `OrderMatchingEngine` for use in research, backtesting and sandbox environments.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{any::Any, cell::RefCell, collections::HashMap, rc::Rc};

use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{nanos::UnixNanos, time::AtomicTime, uuid::UUID4};
use nautilus_execution::matching_core::OrderMatchingCore;
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        delta::OrderBookDelta,
    },
    enums::{
        AccountType, BookType, ContingencyType, LiquiditySide, MarketStatus, OmsType, OrderSide,
        OrderStatus, OrderType,
    },
    events::order::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny, OrderExpired,
        OrderFilled, OrderModifyRejected, OrderRejected, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{any::InstrumentAny, EXPIRING_INSTRUMENT_TYPES},
    orderbook::book::OrderBook,
    orders::{
        any::{OrderAny, PassiveOrderAny, StopOrderAny},
        trailing_stop_limit::TrailingStopLimitOrder,
        trailing_stop_market::TrailingStopMarketOrder,
    },
    position::Position,
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};
use ustr::Ustr;

use crate::models::fill::FillModel;

/// Configuration for [`OrderMatchingEngine`] instances.
#[derive(Debug, Clone)]
pub struct OrderMatchingEngineConfig {
    pub bar_execution: bool,
    pub reject_stop_orders: bool,
    pub support_gtd_orders: bool,
    pub support_contingent_orders: bool,
    pub use_position_ids: bool,
    pub use_random_ids: bool,
    pub use_reduce_only: bool,
}

impl OrderMatchingEngineConfig {
    #[must_use]
    pub const fn new(
        bar_execution: bool,
        reject_stop_orders: bool,
        support_gtd_orders: bool,
        support_contingent_orders: bool,
        use_position_ids: bool,
        use_random_ids: bool,
        use_reduce_only: bool,
    ) -> Self {
        Self {
            bar_execution,
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for OrderMatchingEngineConfig {
    fn default() -> Self {
        Self {
            bar_execution: false,
            reject_stop_orders: false,
            support_gtd_orders: false,
            support_contingent_orders: false,
            use_position_ids: false,
            use_random_ids: false,
            use_reduce_only: false,
        }
    }
}

/// An order matching engine for a single market.
pub struct OrderMatchingEngine {
    /// The venue for the matching engine.
    pub venue: Venue,
    /// The instrument for the matching engine.
    pub instrument: InstrumentAny,
    /// The instruments raw integer ID for the venue.
    pub raw_id: u32,
    /// The order book type for the matching engine.
    pub book_type: BookType,
    /// The order management system (OMS) type for the matching engine.
    pub oms_type: OmsType,
    /// The account type for the matching engine.
    pub account_type: AccountType,
    /// The market status for the matching engine.
    pub market_status: MarketStatus,
    /// The config for the matching engine.
    pub config: OrderMatchingEngineConfig,
    clock: &'static AtomicTime,
    msgbus: Rc<RefCell<MessageBus>>,
    cache: Rc<RefCell<Cache>>,
    book: OrderBook,
    core: OrderMatchingCore,
    fill_model: FillModel,
    target_bid: Option<Price>,
    target_ask: Option<Price>,
    target_last: Option<Price>,
    last_bar_bid: Option<Bar>,
    last_bar_ask: Option<Bar>,
    execution_bar_types: HashMap<InstrumentId, BarType>,
    execution_bar_deltas: HashMap<InstrumentId, u64>,
    account_ids: HashMap<TraderId, AccountId>,
    position_count: usize,
    order_count: usize,
    execution_count: usize,
}

impl OrderMatchingEngine {
    /// Creates a new [`OrderMatchingEngine`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument: InstrumentAny,
        raw_id: u32,
        fill_model: FillModel,
        book_type: BookType,
        oms_type: OmsType,
        account_type: AccountType,
        clock: &'static AtomicTime,
        msgbus: Rc<RefCell<MessageBus>>,
        cache: Rc<RefCell<Cache>>,
        config: OrderMatchingEngineConfig,
    ) -> Self {
        let book = OrderBook::new(instrument.id(), book_type);
        let core = OrderMatchingCore::new(
            instrument.id(),
            instrument.price_increment(),
            None, // TBD (will be a function on the engine)
            None, // TBD (will be a function on the engine)
            None, // TBD (will be a function on the engine)
        );
        Self {
            venue: instrument.id().venue,
            instrument,
            raw_id,
            fill_model,
            book_type,
            oms_type,
            account_type,
            clock,
            msgbus,
            cache,
            book,
            core,
            market_status: MarketStatus::Open,
            config,
            target_bid: None,
            target_ask: None,
            target_last: None,
            last_bar_bid: None,
            last_bar_ask: None,
            execution_bar_types: HashMap::new(),
            execution_bar_deltas: HashMap::new(),
            account_ids: HashMap::new(),
            position_count: 0,
            order_count: 0,
            execution_count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.book.clear(0, UnixNanos::default());
        self.execution_bar_types.clear();
        self.execution_bar_deltas.clear();
        self.account_ids.clear();
        self.core.reset();
        self.target_bid = None;
        self.target_ask = None;
        self.target_last = None;
        self.position_count = 0;
        self.order_count = 0;
        self.execution_count = 0;

        log::info!("Reset {}", self.instrument.id());
    }

    pub fn set_fill_model(&mut self, fill_model: FillModel) {
        self.fill_model = fill_model;
    }

    #[must_use]
    pub fn best_bid_price(&self) -> Option<Price> {
        self.book.best_bid_price()
    }

    #[must_use]
    pub fn best_ask_price(&self) -> Option<Price> {
        self.book.best_ask_price()
    }

    #[must_use]
    pub const fn get_book(&self) -> &OrderBook {
        &self.book
    }

    #[must_use]
    pub fn get_open_bid_orders(&self) -> &[PassiveOrderAny] {
        self.core.get_orders_bid()
    }

    #[must_use]
    pub fn get_open_ask_orders(&self) -> &[PassiveOrderAny] {
        self.core.get_orders_ask()
    }

    #[must_use]
    pub fn order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.core.order_exists(client_order_id)
    }

    // -- DATA PROCESSING -------------------------------------------------------------------------

    /// Process the venues market for the given order book delta.
    pub fn process_order_book_delta(&mut self, delta: &OrderBookDelta) {
        log::debug!("Processing {delta}");

        self.book.apply_delta(delta);
    }

    // -- TRADING COMMANDS ------------------------------------------------------------------------

    #[allow(clippy::needless_return)]
    pub fn process_order(&mut self, order: &OrderAny, account_id: AccountId) {
        // enter the scope where you will borrow a cache
        {
            let cache_borrow = self.cache.as_ref().borrow();

            if self.core.order_exists(order.client_order_id()) {
                self.generate_order_rejected(order, "Order already exists".into());
                return;
            }

            // Index identifiers
            self.account_ids.insert(order.trader_id(), account_id);

            // Check for instrument expiration or activation
            if EXPIRING_INSTRUMENT_TYPES.contains(&self.instrument.instrument_class()) {
                if let Some(activation_ns) = self.instrument.activation_ns() {
                    if self.clock.get_time_ns() < activation_ns {
                        self.generate_order_rejected(
                            order,
                            format!(
                                "Contract {} is not yet active, activation {}",
                                self.instrument.id(),
                                self.instrument.activation_ns().unwrap()
                            )
                            .into(),
                        );
                        return;
                    }
                }
                if let Some(expiration_ns) = self.instrument.expiration_ns() {
                    if self.clock.get_time_ns() >= expiration_ns {
                        self.generate_order_rejected(
                            order,
                            format!(
                                "Contract {} has expired, expiration {}",
                                self.instrument.id(),
                                self.instrument.expiration_ns().unwrap()
                            )
                            .into(),
                        );
                        return;
                    }
                }
            }

            // Contingent orders checks
            if self.config.support_contingent_orders {
                if let Some(parent_order_id) = order.parent_order_id() {
                    println!("Search for parent order {parent_order_id}");
                    let parent_order = cache_borrow.order(&parent_order_id);
                    if parent_order.is_none()
                        || parent_order.unwrap().contingency_type().unwrap() != ContingencyType::Oto
                    {
                        panic!("OTO parent not found");
                    }
                    if let Some(parent_order) = parent_order {
                        let parent_order_status = parent_order.status();
                        let order_is_open = order.is_open();
                        if parent_order.status() == OrderStatus::Rejected && order.is_open() {
                            self.generate_order_rejected(
                                order,
                                format!("Rejected OTO order from {parent_order_id}").into(),
                            );
                            return;
                        } else if parent_order.status() == OrderStatus::Accepted
                            && parent_order.status() == OrderStatus::Triggered
                        {
                            log::info!(
                                "Pending OTO order {} triggers from {}",
                                order.client_order_id(),
                                parent_order_id
                            );
                            return;
                        }
                    }
                }

                if let Some(linked_order_ids) = order.linked_order_ids() {
                    for client_order_id in linked_order_ids {
                        match cache_borrow.order(&client_order_id) {
                            Some(contingent_order)
                                if (order.contingency_type().unwrap() == ContingencyType::Oco
                                    || order.contingency_type().unwrap()
                                        == ContingencyType::Ouo)
                                    && !order.is_closed()
                                    && contingent_order.is_closed() =>
                            {
                                self.generate_order_rejected(
                                    order,
                                    format!("Contingent order {client_order_id} already closed")
                                        .into(),
                                );
                                return;
                            }
                            None => panic!("Cannot find contingent order for {client_order_id}"),
                            _ => {}
                        }
                    }
                }
            }

            // Check fo valid order quantity precision
            if order.quantity().precision != self.instrument.size_precision() {
                self.generate_order_rejected(
                    order,
                    format!(
                        "Invalid order quantity precision for order {}, was {} when {} size precision is {}",
                        order.client_order_id(),
                        order.quantity().precision,
                        self.instrument.id(),
                        self.instrument.size_precision()
                    )
                        .into(),
                );
                return;
            }

            // Check for valid order price precision
            if let Some(price) = order.price() {
                if price.precision != self.instrument.price_precision() {
                    self.generate_order_rejected(
                        order,
                        format!(
                            "Invalid order price precision for order {}, was {} when {} price precision is {}",
                            order.client_order_id(),
                            price.precision,
                            self.instrument.id(),
                            self.instrument.price_precision()
                        )
                            .into(),
                    );
                }
                return;
            }

            // Check for valid order trigger price precision
            if let Some(trigger_price) = order.trigger_price() {
                if trigger_price.precision != self.instrument.price_precision() {
                    self.generate_order_rejected(
                        order,
                        format!(
                            "Invalid order trigger price precision for order {}, was {} when {} price precision is {}",
                            order.client_order_id(),
                            trigger_price.precision,
                            self.instrument.id(),
                            self.instrument.price_precision()
                        )
                            .into(),
                    );
                    return;
                }
            }

            // Get position if exists
            let position: Option<&Position> = cache_borrow
                .position_for_order(&order.client_order_id())
                .or_else(|| {
                    if self.oms_type == OmsType::Netting {
                        let position_id = PositionId::new(
                            format!("{}-{}", order.instrument_id(), order.strategy_id()).as_str(),
                        );
                        cache_borrow.position(&position_id)
                    } else {
                        None
                    }
                });

            // Check not shorting an equity without a MARGIN account
            if order.order_side() == OrderSide::Sell
                && self.account_type != AccountType::Margin
                && matches!(self.instrument, InstrumentAny::Equity(_))
                && (position.is_none()
                    || !order.would_reduce_only(position.unwrap().side, position.unwrap().quantity))
            {
                let position_string = position.map_or("None".to_string(), |pos| pos.id.to_string());
                self.generate_order_rejected(
                    order,
                    format!(
                        "Short selling not permitted on a CASH account with position {position_string} and order {order}",
                    )
                        .into(),
                );
                return;
            }

            // Check reduce-only instruction
            if self.config.use_reduce_only
                && order.is_reduce_only()
                && !order.is_closed()
                && position.map_or(true, |pos| {
                    pos.is_closed()
                        || (order.is_buy() && pos.is_long())
                        || (order.is_sell() && pos.is_short())
                })
            {
                self.generate_order_rejected(
                    order,
                    format!(
                        "Reduce-only order {} ({}-{}) would have increased position",
                        order.client_order_id(),
                        order.order_type().to_string().to_uppercase(),
                        order.order_side().to_string().to_uppercase()
                    )
                    .into(),
                );
                return;
            }
        }

        match order.order_type() {
            OrderType::Market => self.process_market_order(order),
            OrderType::Limit => self.process_limit_order(order),
            OrderType::MarketToLimit => self.process_market_to_limit_order(order),
            OrderType::StopMarket => self.process_stop_market_order(order),
            OrderType::StopLimit => self.process_stop_limit_order(order),
            OrderType::MarketIfTouched => self.process_market_if_touched_order(order),
            OrderType::LimitIfTouched => self.process_limit_if_touched_order(order),
            OrderType::TrailingStopMarket => self.process_trailing_stop_market_order(order),
            OrderType::TrailingStopLimit => self.process_trailing_stop_limit_order(order),
        }
    }

    fn process_market_order(&mut self, order: &OrderAny) {
        // Check if market exists
        let order_side = order.order_side();
        let is_ask_initialized = self.core.is_ask_initialized;
        let is_bid_initialized = self.core.is_bid_initialized;
        if (order.order_side() == OrderSide::Buy && !self.core.is_ask_initialized)
            || (order.order_side() == OrderSide::Sell && !self.core.is_bid_initialized)
        {
            self.generate_order_rejected(
                order,
                format!("No market for {}", order.instrument_id()).into(),
            );
            return;
        }

        self.fill_market_order(order);
    }

    fn process_limit_order(&mut self, order: &OrderAny) {
        todo!("process_limit_order")
    }

    fn process_market_to_limit_order(&mut self, order: &OrderAny) {
        todo!("process_market_to_limit_order")
    }

    fn process_stop_market_order(&mut self, order: &OrderAny) {
        todo!("process_stop_market_order")
    }

    fn process_stop_limit_order(&mut self, order: &OrderAny) {
        todo!("process_stop_limit_order")
    }

    fn process_market_if_touched_order(&mut self, order: &OrderAny) {
        todo!("process_market_if_touched_order")
    }

    fn process_limit_if_touched_order(&mut self, order: &OrderAny) {
        todo!("process_limit_if_touched_order")
    }

    fn process_trailing_stop_market_order(&mut self, order: &OrderAny) {
        todo!("process_trailing_stop_market_order")
    }

    fn process_trailing_stop_limit_order(&mut self, order: &OrderAny) {
        todo!("process_trailing_stop_limit_order")
    }

    // -- ORDER PROCESSING ----------------------------------------------------

    /// Iterate the matching engine by processing the bid and ask order sides
    /// and advancing time up to the given UNIX `timestamp_ns`.
    pub fn iterate(&mut self, timestamp_ns: UnixNanos) {
        self.clock.set_time(timestamp_ns);

        self.core.bid = self.book.best_bid_price();
        self.core.ask = self.book.best_ask_price();

        let orders_bid = self.core.get_orders_bid().to_vec();
        let orders_ask = self.core.get_orders_ask().to_vec();

        self.iterate_orders(timestamp_ns, &orders_bid);
        self.iterate_orders(timestamp_ns, &orders_ask);
    }

    fn iterate_orders(&mut self, timestamp_ns: UnixNanos, orders: &[PassiveOrderAny]) {
        for order in orders {
            if order.is_closed() {
                continue;
            };

            // Check expiration
            if self.config.support_gtd_orders {
                if let Some(expire_time) = order.expire_time() {
                    if timestamp_ns >= expire_time {
                        // SAFTEY: We know this order is in the core
                        self.core.delete_order(order).unwrap();
                        self.expire_order(order);
                    }
                }
            }

            // Manage trailing stop
            if let PassiveOrderAny::Stop(o) = order {
                match o {
                    StopOrderAny::TrailingStopMarket(o) => self.update_trailing_stop_market(o),
                    StopOrderAny::TrailingStopLimit(o) => self.update_trailing_stop_limit(o),
                    _ => {}
                }
            }

            // Move market back to targets
            self.core.bid = self.target_bid;
            self.core.ask = self.target_ask;
            self.core.last = self.target_last;
        }

        // Reset any targets after iteration
        self.target_bid = None;
        self.target_ask = None;
        self.target_last = None;
    }

    fn determine_limit_price_and_volume(&self, order: &OrderAny) {
        todo!("determine_limit_price_and_volume")
    }

    fn determine_market_price_and_volume(&self, order: &OrderAny) {
        todo!("determine_market_price_and_volume")
    }

    fn fill_market_order(&mut self, order: &OrderAny) {
        todo!("fill_market_order")
    }

    fn fill_limit_order(&mut self, order: &OrderAny) {
        todo!("fill_limit_order")
    }

    fn apply_fills(
        &mut self,
        order: &OrderAny,
        fills: Vec<(Price, Quantity)>,
        liquidity_side: LiquiditySide,
        venue_position_id: Option<PositionId>,
        position: Option<Position>,
    ) {
        todo!("apply_fills")
    }

    fn fill_order(
        &mut self,
        order: &OrderAny,
        price: Price,
        quantity: Quantity,
        liquidity_side: LiquiditySide,
        venue_position_id: Option<PositionId>,
        position: Option<Position>,
    ) {
        todo!("fill_order")
    }

    fn update_trailing_stop_market(&mut self, order: &TrailingStopMarketOrder) {
        todo!()
    }

    fn update_trailing_stop_limit(&mut self, order: &TrailingStopLimitOrder) {
        todo!()
    }

    // -- IDENTIFIER GENERATORS -----------------------------------------------------

    fn generate_trade_id(&mut self) -> TradeId {
        self.execution_count += 1;
        let trade_id = if self.config.use_random_ids {
            UUID4::new().to_string()
        } else {
            format!("{}-{}-{}", self.venue, self.raw_id, self.execution_count)
        };
        TradeId::from(trade_id.as_str())
    }

    // -- EVENT HANDLING -----------------------------------------------------

    fn accept_order(&mut self, order: &OrderAny) {
        todo!("accept_order")
    }

    fn expire_order(&mut self, order: &PassiveOrderAny) {
        todo!("expire_order")
    }

    fn cancel_order(&mut self, order: &OrderAny) {
        todo!("cancel_order")
    }

    fn update_order(&mut self, order: &OrderAny) {
        todo!("update_order")
    }

    fn trigger_stop_order(&mut self, order: &OrderAny) {
        todo!("trigger_stop_order")
    }

    fn update_contingent_order(&mut self, order: &OrderAny) {
        todo!("update_contingent_order")
    }

    fn cancel_contingent_orders(&mut self, order: &OrderAny) {
        todo!("cancel_contingent_orders")
    }

    // -- EVENT GENERATORS -----------------------------------------------------

    fn generate_order_rejected(&self, order: &OrderAny, reason: Ustr) {
        let ts_now = self.clock.get_time_ns();
        let account_id = order
            .account_id()
            .unwrap_or(self.account_ids.get(&order.trader_id()).unwrap().to_owned());

        let event = OrderEventAny::Rejected(OrderRejected::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id,
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    fn generate_order_accepted(&self, order: &OrderAny, venue_order_id: VenueOrderId) {
        let ts_now = self.clock.get_time_ns();
        let account_id = order
            .account_id()
            .unwrap_or(self.account_ids.get(&order.trader_id()).unwrap().to_owned());
        let event = OrderEventAny::Accepted(OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_modify_rejected(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: Ustr,
    ) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::ModifyRejected(OrderModifyRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            Some(venue_order_id),
            Some(account_id),
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_cancel_rejected(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: Ustr,
    ) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::CancelRejected(OrderCancelRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            Some(venue_order_id),
            Some(account_id),
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    fn generate_order_updated(
        &self,
        order: &OrderAny,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
    ) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Updated(OrderUpdated::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            quantity,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            order.venue_order_id(),
            order.account_id(),
            Some(price),
            Some(trigger_price),
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    fn generate_order_canceled(&self, order: &OrderAny, venue_order_id: VenueOrderId) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Canceled(OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            Some(venue_order_id),
            order.account_id(),
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    fn generate_order_triggered(&self, order: &OrderAny) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Triggered(OrderTriggered::new(
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
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    fn generate_order_expired(&self, order: &OrderAny) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderEventAny::Expired(OrderExpired::new(
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
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_filled(
        &mut self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        venue_position_id: PositionId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
    ) {
        let ts_now = self.clock.get_time_ns();
        let account_id = order
            .account_id()
            .unwrap_or(self.account_ids.get(&order.trader_id()).unwrap().to_owned());
        let event = OrderEventAny::Filled(OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            self.generate_trade_id(),
            order.order_side(),
            order.order_type(),
            last_qty,
            last_px,
            quote_currency,
            liquidity_side,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            Some(venue_position_id),
            Some(commission),
        ));
        let msgbus = self.msgbus.as_ref().borrow();
        msgbus.send(&msgbus.switchboard.exec_engine_process, &event as &dyn Any);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, sync::LazyLock};

    use chrono::{TimeZone, Utc};
    use nautilus_common::{
        cache::Cache,
        msgbus::{
            handler::ShareableMessageHandler,
            stubs::{get_message_saving_handler, get_saved_messages},
            MessageBus,
        },
    };
    use nautilus_core::{nanos::UnixNanos, time::AtomicTime, uuid::UUID4};
    use nautilus_model::{
        enums::{AccountType, BookType, ContingencyType, OmsType, OrderSide, TimeInForce},
        events::order::{
            rejected::OrderRejectedBuilder, OrderEventAny, OrderEventType, OrderRejected,
        },
        identifiers::{AccountId, ClientOrderId, StrategyId, TraderId},
        instruments::{
            any::InstrumentAny,
            equity::Equity,
            stubs::{futures_contract_es, *},
        },
        orders::{any::OrderAny, market::MarketOrder, stubs::TestOrderStubs},
        types::{price::Price, quantity::Quantity},
    };
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use crate::{
        matching_engine::{OrderMatchingEngine, OrderMatchingEngineConfig},
        models::fill::FillModel,
    };

    static ATOMIC_TIME: LazyLock<AtomicTime> =
        LazyLock::new(|| AtomicTime::new(true, UnixNanos::default()));

    // -- FIXTURES --------------------------------------------------------------------------------

    #[fixture]
    fn msgbus() -> MessageBus {
        MessageBus::default()
    }

    #[fixture]
    fn account_id() -> AccountId {
        AccountId::from("SIM-001")
    }

    #[fixture]
    fn time() -> AtomicTime {
        AtomicTime::new(true, UnixNanos::default())
    }

    #[fixture]
    fn order_event_handler() -> ShareableMessageHandler {
        get_message_saving_handler::<OrderEventAny>(Some(Ustr::from("ExecEngine.process")))
    }

    // for valid es futures contract currently active
    #[fixture]
    fn instrument_es() -> InstrumentAny {
        let activation = UnixNanos::from(
            Utc.with_ymd_and_hms(2022, 4, 8, 0, 0, 0)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap() as u64,
        );
        let expiration = UnixNanos::from(
            Utc.with_ymd_and_hms(2100, 7, 8, 0, 0, 0)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap() as u64,
        );
        InstrumentAny::FuturesContract(futures_contract_es(Some(activation), Some(expiration)))
    }

    #[fixture]
    fn engine_config() -> OrderMatchingEngineConfig {
        OrderMatchingEngineConfig {
            bar_execution: false,
            reject_stop_orders: false,
            support_gtd_orders: false,
            support_contingent_orders: true,
            use_position_ids: false,
            use_random_ids: false,
            use_reduce_only: true,
        }
    }
    // -- HELPERS ---------------------------------------------------------------------------

    fn get_order_matching_engine(
        instrument: InstrumentAny,
        msgbus: Rc<RefCell<MessageBus>>,
        cache: Option<Rc<RefCell<Cache>>>,
        account_type: Option<AccountType>,
        config: Option<OrderMatchingEngineConfig>,
    ) -> OrderMatchingEngine {
        let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
        let config = config.unwrap_or_default();
        OrderMatchingEngine::new(
            instrument,
            1,
            FillModel::default(),
            BookType::L1_MBP,
            OmsType::Netting,
            account_type.unwrap_or(AccountType::Cash),
            &ATOMIC_TIME,
            msgbus,
            cache,
            config,
        )
    }

    fn get_order_event_handler_messages(
        event_handler: ShareableMessageHandler,
    ) -> Vec<OrderEventAny> {
        get_saved_messages::<OrderEventAny>(event_handler)
    }

    // -- TESTS -----------------------------------------------------------------------------------

    #[rstest]
    fn test_process_order_when_instrument_already_expired(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
    ) {
        let instrument = InstrumentAny::FuturesContract(futures_contract_es(None, None));

        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        // Create engine and process order
        let mut engine = get_order_matching_engine(
            instrument.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            None,
        );
        let order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
        );
        engine.process_order(&order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Contract ESZ1.GLBX has expired, expiration 1625702400000000000")
        );
    }

    #[rstest]
    fn test_process_order_when_instrument_not_active(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
    ) {
        let activation = UnixNanos::from(
            Utc.with_ymd_and_hms(2222, 4, 8, 0, 0, 0)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap() as u64,
        );
        let expiration = UnixNanos::from(
            Utc.with_ymd_and_hms(2223, 7, 8, 0, 0, 0)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap() as u64,
        );
        let instrument =
            InstrumentAny::FuturesContract(futures_contract_es(Some(activation), Some(expiration)));

        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        // Create engine and process order
        let mut engine = get_order_matching_engine(
            instrument.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            None,
        );
        let order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
        );
        engine.process_order(&order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Contract ESZ1.GLBX is not yet active, activation 7960723200000000000")
        );
    }

    #[rstest]
    fn test_process_order_when_invalid_quantity_precision(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        instrument_es: InstrumentAny,
    ) {
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        // Create engine and process order
        let mut engine = get_order_matching_engine(
            instrument_es.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            None,
        );
        let order = TestOrderStubs::market_order(
            instrument_es.id(),
            OrderSide::Buy,
            Quantity::from("1.122"), // <- wrong precision for es futures contract (which is 1)x
            None,
            None,
        );
        engine.process_order(&order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Invalid order quantity precision for order O-19700101-000000-001-001-1, was 3 when ESZ1.GLBX size precision is 0")
        );
    }

    #[rstest]
    fn test_process_order_when_invalid_price_precision(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        instrument_es: InstrumentAny,
    ) {
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        // Create engine and process order
        let mut engine = get_order_matching_engine(
            instrument_es.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            None,
        );
        let limit_order = TestOrderStubs::limit_order(
            instrument_es.id(),
            OrderSide::Sell,
            Price::from("100.12333"), // <- wrong price precision for es futures contract (which is 2)
            Quantity::from("1"),
            None,
            None,
        );

        engine.process_order(&limit_order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Invalid order price precision for order O-19700101-000000-001-001-1, was 5 when ESZ1.GLBX price precision is 2")
        );
    }

    #[rstest]
    fn test_process_order_when_invalid_trigger_price_precision(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        instrument_es: InstrumentAny,
    ) {
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        // Create engine and process order
        let mut engine = get_order_matching_engine(
            instrument_es.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            None,
        );
        let stop_order = TestOrderStubs::stop_market_order(
            instrument_es.id(),
            OrderSide::Sell,
            Price::from("100.12333"), // <- wrong trigger price precision for es futures contract (which is 2)
            Quantity::from("1"),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        engine.process_order(&stop_order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Invalid order trigger price precision for order O-19700101-000000-001-001-1, was 5 when ESZ1.GLBX price precision is 2")
        );
    }

    #[rstest]
    fn test_process_order_when_shorting_equity_without_margin_account(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        equity_aapl: Equity,
    ) {
        let instrument = InstrumentAny::Equity(equity_aapl);
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        // Create engine and process order
        let mut engine = get_order_matching_engine(
            instrument.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            None,
        );
        let order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Sell,
            Quantity::from("1"),
            None,
            None,
        );

        engine.process_order(&order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from(
                "Short selling not permitted on a CASH account with position None and order \
                MarketOrder(SELL 1 AAPL.XNAS @ MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-001-001-1, \
                 venue_order_id=None, position_id=None, exec_algorithm_id=None, \
                 exec_spawn_id=None, tags=None)")
        );
    }

    #[rstest]
    fn test_process_order_when_invalid_reduce_only(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        instrument_es: InstrumentAny,
        engine_config: OrderMatchingEngineConfig,
    ) {
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        let mut engine = get_order_matching_engine(
            instrument_es.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            Some(engine_config),
        );
        let market_order = TestOrderStubs::market_order_reduce(
            instrument_es.id(),
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
        );

        engine.process_order(&market_order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Reduce-only order O-19700101-000000-001-001-1 (MARKET-BUY) would have increased position")
        );
    }

    #[rstest]
    fn test_process_order_when_invalid_contingent_orders(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        instrument_es: InstrumentAny,
        engine_config: OrderMatchingEngineConfig,
    ) {
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut engine = get_order_matching_engine(
            instrument_es.clone(),
            Rc::new(RefCell::new(msgbus)),
            Some(cache.clone()),
            None,
            Some(engine_config),
        );

        let entry_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-1");
        let stop_loss_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-2");

        // Create entry market order
        let mut entry_order = OrderAny::Market(MarketOrder::new(
            TraderId::default(),
            StrategyId::default(),
            instrument_es.id(),
            entry_client_order_id,
            OrderSide::Buy,
            Quantity::from("1"),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            false,
            Some(ContingencyType::Oto), // <- set contingency type to OTO
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));
        // Set entry order status to Rejected with proper event
        let rejected_event = OrderRejected::default();
        entry_order
            .apply(OrderEventAny::Rejected(rejected_event))
            .unwrap();

        // Create stop loss order
        let stop_order = TestOrderStubs::stop_market_order(
            instrument_es.id(),
            OrderSide::Sell,
            Price::from("0.95"),
            Quantity::from(1),
            None,
            Some(ContingencyType::Oto),
            Some(stop_loss_client_order_id),
            None,
            Some(entry_client_order_id),
            None,
        );
        // Make it Accepted
        let accepted_stop_order = TestOrderStubs::make_accepted_order(&stop_order);

        // 1. save entry order in the cache as it will be loaded by the matching engine
        // 2. send the stop loss order which has parent of entry order
        cache
            .as_ref()
            .borrow_mut()
            .add_order(entry_order.clone(), None, None, false)
            .unwrap();
        engine.process_order(&accepted_stop_order, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from(format!("Rejected OTO order from {entry_client_order_id}").as_str())
        );
    }

    #[rstest]
    fn test_process_order_when_closed_linked_order(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        instrument_es: InstrumentAny,
        engine_config: OrderMatchingEngineConfig,
    ) {
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut engine = get_order_matching_engine(
            instrument_es.clone(),
            Rc::new(RefCell::new(msgbus)),
            Some(cache.clone()),
            None,
            Some(engine_config),
        );

        let stop_loss_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-2");
        let take_profit_client_order_id = ClientOrderId::from("O-19700101-000000-001-001-3");
        // Create two linked orders: stop loss and take profit
        let mut stop_loss_order = TestOrderStubs::stop_market_order(
            instrument_es.id(),
            OrderSide::Sell,
            Price::from("0.95"),
            Quantity::from(1),
            None,
            Some(ContingencyType::Oco),
            Some(stop_loss_client_order_id),
            None,
            None,
            Some(vec![take_profit_client_order_id]),
        );
        let take_profit_order = TestOrderStubs::market_if_touched_order(
            instrument_es.id(),
            OrderSide::Sell,
            Price::from("1.1"),
            Quantity::from(1),
            None,
            Some(ContingencyType::Oco),
            Some(take_profit_client_order_id),
            None,
            Some(vec![stop_loss_client_order_id]),
        );
        // Set stop loss order status to Rejected with proper event
        let rejected_event: OrderRejected = OrderRejectedBuilder::default()
            .client_order_id(stop_loss_client_order_id)
            .reason(Ustr::from("Rejected"))
            .build()
            .unwrap();
        stop_loss_order
            .apply(OrderEventAny::Rejected(rejected_event))
            .unwrap();

        // Make take profit order Accepted
        let accepted_take_profit = TestOrderStubs::make_accepted_order(&take_profit_order);

        // 1. save stop loss order in cache which is rejected and closed is set to true
        // 2. send the take profit order which has linked the stop loss order
        cache
            .as_ref()
            .borrow_mut()
            .add_order(stop_loss_order.clone(), None, None, false)
            .unwrap();
        let stop_loss_closed_after = stop_loss_order.is_closed();
        engine.process_order(&accepted_take_profit, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from(
                format!("Contingent order {stop_loss_client_order_id} already closed").as_str()
            )
        );
    }

    #[rstest]
    fn test_process_market_order_no_market_rejected(
        mut msgbus: MessageBus,
        order_event_handler: ShareableMessageHandler,
        account_id: AccountId,
        time: AtomicTime,
        instrument_es: InstrumentAny,
    ) {
        // Register saving message handler to exec engine endpoint
        msgbus.register(
            msgbus.switchboard.exec_engine_process,
            order_event_handler.clone(),
        );

        // Create engine and process order
        let mut engine = get_order_matching_engine(
            instrument_es.clone(),
            Rc::new(RefCell::new(msgbus)),
            None,
            None,
            None,
        );
        let market_order_buy = TestOrderStubs::market_order(
            instrument_es.id(),
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
        );
        let market_order_sell = TestOrderStubs::market_order(
            instrument_es.id(),
            OrderSide::Sell,
            Quantity::from("1"),
            None,
            None,
        );
        engine.process_order(&market_order_buy, account_id);
        engine.process_order(&market_order_sell, account_id);

        // Get messages and test
        let saved_messages = get_order_event_handler_messages(order_event_handler);
        assert_eq!(saved_messages.len(), 2);
        let first = saved_messages.first().unwrap();
        let second = saved_messages.get(1).unwrap();
        assert_eq!(first.event_type(), OrderEventType::Rejected);
        assert_eq!(second.event_type(), OrderEventType::Rejected);
        assert_eq!(
            first.message().unwrap(),
            Ustr::from("No market for ESZ1.GLBX")
        );
        assert_eq!(
            second.message().unwrap(),
            Ustr::from("No market for ESZ1.GLBX")
        );
    }
}
