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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{any::Any, cell::RefCell, collections::HashMap, rc::Rc};

use chrono::TimeDelta;
use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{nanos::UnixNanos, time::AtomicTime, uuid::UUID4};
use nautilus_execution::matching_core::OrderMatchingCore;
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        delta::OrderBookDelta,
        deltas::OrderBookDeltas,
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{
        AccountType, AggregationSource, AggressorSide, BarAggregation, BookType, ContingencyType,
        LiquiditySide, MarketStatus, MarketStatusAction, OmsType, OrderSide, OrderStatus,
        OrderType, PriceType,
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
use uuid::Uuid;

use crate::{matching_engine::config::OrderMatchingEngineConfig, models::fill::FillModel};

pub mod config;
#[cfg(test)]
mod tests;

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
    execution_bar_deltas: HashMap<BarType, TimeDelta>,
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
    pub fn get_open_orders(&self) -> Vec<PassiveOrderAny> {
        // Get orders from both open bid orders and open ask orders
        let mut orders = Vec::new();
        orders.extend_from_slice(self.core.get_orders_bid());
        orders.extend_from_slice(self.core.get_orders_ask());
        orders
    }

    #[must_use]
    pub fn order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.core.order_exists(client_order_id)
    }

    // -- DATA PROCESSING -------------------------------------------------------------------------

    /// Process the venues market for the given order book delta.
    pub fn process_order_book_delta(&mut self, delta: &OrderBookDelta) {
        log::debug!("Processing {delta}");

        if self.book_type == BookType::L2_MBP || self.book_type == BookType::L3_MBO {
            self.book.apply_delta(delta);
        }

        self.iterate(delta.ts_event);
    }

    pub fn process_order_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        log::debug!("Processing {deltas}");

        if self.book_type == BookType::L2_MBP || self.book_type == BookType::L3_MBO {
            self.book.apply_deltas(deltas);
        }

        self.iterate(deltas.ts_event);
    }

    pub fn process_quote_tick(&mut self, quote: &QuoteTick) {
        log::debug!("Processing {quote}");

        if self.book_type == BookType::L1_MBP {
            self.book.update_quote_tick(quote).unwrap();
        }

        self.iterate(quote.ts_event);
    }

    pub fn process_bar(&mut self, bar: &Bar) {
        log::debug!("Processing {bar}");

        // Check if configured for bar execution can only process an L1 book with bars
        if !self.config.bar_execution || self.book_type != BookType::L1_MBP {
            return;
        }

        let bar_type = bar.bar_type;
        // Do not process internally aggregated bars
        if bar_type.aggregation_source() == AggregationSource::Internal {
            return;
        }

        // Do not process monthly bars (no `timedelta` available)
        if bar_type.spec().aggregation == BarAggregation::Month {
            return;
        }

        let execution_bar_type =
            if let Some(execution_bar_type) = self.execution_bar_types.get(&bar.instrument_id()) {
                execution_bar_type.to_owned()
            } else {
                self.execution_bar_types
                    .insert(bar.instrument_id(), bar_type);
                self.execution_bar_deltas
                    .insert(bar_type, bar_type.spec().timedelta());
                bar_type
            };

        if execution_bar_type != bar_type {
            let mut bar_type_timedelta = self.execution_bar_deltas.get(&bar_type).copied();
            if bar_type_timedelta.is_none() {
                bar_type_timedelta = Some(bar_type.spec().timedelta());
                self.execution_bar_deltas
                    .insert(bar_type, bar_type_timedelta.unwrap());
            }
            if self.execution_bar_deltas.get(&execution_bar_type).unwrap()
                >= &bar_type_timedelta.unwrap()
            {
                self.execution_bar_types
                    .insert(bar_type.instrument_id(), bar_type);
            } else {
                return;
            }
        }

        match bar_type.spec().price_type {
            PriceType::Last | PriceType::Mid => self.process_trade_ticks_from_bar(bar),
            PriceType::Bid => {
                self.last_bar_bid = Some(bar.to_owned());
                self.process_quote_ticks_from_bar(bar);
            }
            PriceType::Ask => {
                self.last_bar_ask = Some(bar.to_owned());
                self.process_quote_ticks_from_bar(bar);
            }
        }
    }

    fn process_trade_ticks_from_bar(&mut self, bar: &Bar) {
        // Split the bar into 4 trade ticks with quarter volume
        let size = Quantity::new(bar.volume.as_f64() / 4.0, bar.volume.precision);
        let aggressor_side = if !self.core.is_last_initialized || bar.open > self.core.last.unwrap()
        {
            AggressorSide::Buyer
        } else {
            AggressorSide::Seller
        };

        // Create reusable trade tick
        let mut trade_tick = TradeTick::new(
            bar.instrument_id(),
            bar.open,
            size,
            aggressor_side,
            self.generate_trade_id(),
            bar.ts_event,
            bar.ts_event,
        );

        // Open
        // Check if not initialized, if it is, it will be updated by the close or last
        if !self.core.is_last_initialized {
            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init);
            self.core.set_last_raw(trade_tick.price);
        }

        // High
        // Check if higher than last
        if self.core.last.is_some_and(|last| bar.high > last) {
            trade_tick.price = bar.high;
            trade_tick.aggressor_side = AggressorSide::Buyer;
            trade_tick.trade_id = self.generate_trade_id();

            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init);

            self.core.set_last_raw(trade_tick.price);
        }

        // Low
        // Check if lower than last
        // Assumption: market traded down, aggressor hitting the bid(setting aggressor to seller)
        if self.core.last.is_some_and(|last| bar.low < last) {
            trade_tick.price = bar.low;
            trade_tick.aggressor_side = AggressorSide::Seller;
            trade_tick.trade_id = self.generate_trade_id();

            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init);

            self.core.set_last_raw(trade_tick.price);
        }

        // Close
        // Check if not the same as last
        // Assumption: if close price is higher then last, aggressor is buyer
        // Assumption: if close price is lower then last, aggressor is seller
        if self.core.last.is_some_and(|last| bar.close != last) {
            trade_tick.price = bar.close;
            trade_tick.aggressor_side = if bar.close > self.core.last.unwrap() {
                AggressorSide::Buyer
            } else {
                AggressorSide::Seller
            };
            trade_tick.trade_id = self.generate_trade_id();

            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init);

            self.core.set_last_raw(trade_tick.price);
        }
    }

    fn process_quote_ticks_from_bar(&mut self, bar: &Bar) {
        // Wait for next bar
        if self.last_bar_bid.is_none()
            || self.last_bar_ask.is_none()
            || self.last_bar_bid.unwrap().ts_event != self.last_bar_ask.unwrap().ts_event
        {
            return;
        }
        let bid_bar = self.last_bar_bid.unwrap();
        let ask_bar = self.last_bar_ask.unwrap();
        let bid_size = Quantity::new(bid_bar.volume.as_f64() / 4.0, bar.volume.precision);
        let ask_size = Quantity::new(ask_bar.volume.as_f64() / 4.0, bar.volume.precision);

        // Create reusable quote tick
        let mut quote_tick = QuoteTick::new(
            self.book.instrument_id,
            bid_bar.open,
            ask_bar.open,
            bid_size,
            ask_size,
            bid_bar.ts_init,
            bid_bar.ts_init,
        );

        // Open
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init);

        // High
        quote_tick.bid_price = bid_bar.high;
        quote_tick.ask_price = ask_bar.high;
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init);

        // Low
        quote_tick.bid_price = bid_bar.low;
        quote_tick.ask_price = ask_bar.low;
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init);

        // Close
        quote_tick.bid_price = bid_bar.close;
        quote_tick.ask_price = ask_bar.close;
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init);

        // Reset last bars
        self.last_bar_bid = None;
        self.last_bar_ask = None;
    }

    pub fn process_trade_tick(&mut self, trade: &TradeTick) {
        log::debug!("Processing {trade}");

        if self.book_type == BookType::L1_MBP {
            self.book.update_trade_tick(trade).unwrap();
        }
        self.core.set_last_raw(trade.price);

        self.iterate(trade.ts_event);
    }

    pub fn process_status(&mut self, action: MarketStatusAction) {
        log::debug!("Processing {action}");

        // Check if market is closed and market opens with trading or pre-open status
        if self.market_status == MarketStatus::Closed
            && (action == MarketStatusAction::Trading || action == MarketStatusAction::PreOpen)
        {
            self.market_status = MarketStatus::Open;
        }
        // Check if market is open and market pauses
        if self.market_status == MarketStatus::Open && action == MarketStatusAction::Pause {
            self.market_status = MarketStatus::Paused;
        }
        // Check if market is open and market suspends
        if self.market_status == MarketStatus::Open && action == MarketStatusAction::Suspend {
            self.market_status = MarketStatus::Suspended;
        }
        // Check if market is open and we halt or close
        if self.market_status == MarketStatus::Open
            && (action == MarketStatusAction::Halt || action == MarketStatusAction::Close)
        {
            self.market_status = MarketStatus::Closed;
        }
    }

    // -- TRADING COMMANDS ------------------------------------------------------------------------

    #[allow(clippy::needless_return)]
    pub fn process_order(&mut self, order: &OrderAny, account_id: AccountId) {
        // Enter the scope where you will borrow a cache
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
                                "Pending OTO order {} triggers from {parent_order_id}",
                                order.client_order_id(),
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

        // Check for updates in orderbook and set bid and ask in order matching core and iterate
        if self.book.has_bid() {
            self.core.set_bid_raw(self.book.best_bid_price().unwrap());
        }
        if self.book.has_ask() {
            self.core.set_ask_raw(self.book.best_ask_price().unwrap());
        }
        self.core.iterate();

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
            Uuid::new_v4().to_string()
        } else {
            format!("{}-{}-{}", self.venue, self.raw_id, self.execution_count)
        };
        TradeId::from(trade_id.as_str())
    }

    fn get_position_id(&mut self, order: &OrderAny, generate: Option<bool>) -> Option<PositionId> {
        let generate = generate.unwrap_or(true);
        if self.oms_type == OmsType::Hedging {
            {
                let cache = self.cache.as_ref().borrow();
                let position_id_result = cache.position_id(&order.client_order_id());
                if let Some(position_id) = position_id_result {
                    return Some(position_id.to_owned());
                }
            }
            if generate {
                self.generate_venue_position_id()
            } else {
                panic!("Position id should be generated. Hedging Oms type order matching engine doesnt exists in cache.")
            }
        } else {
            // Netting OMS (position id will be derived from instrument and strategy)
            let cache = self.cache.as_ref().borrow();
            let positions_open =
                cache.positions_open(None, Some(&order.instrument_id()), None, None);
            if !positions_open.is_empty() {
                Some(positions_open[0].id)
            } else {
                None
            }
        }
    }

    fn generate_venue_position_id(&mut self) -> Option<PositionId> {
        if !self.config.use_position_ids {
            return None;
        }

        self.position_count += 1;
        if self.config.use_random_ids {
            Some(PositionId::new(&Uuid::new_v4().to_string()))
        } else {
            Some(PositionId::new(
                format!("{}-{}-{}", self.venue, self.raw_id, self.position_count).as_str(),
            ))
        }
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
