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
    any::Any,
    cell::RefCell,
    cmp::min,
    collections::HashMap,
    ops::{Add, Sub},
    rc::Rc,
};

use chrono::TimeDelta;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    msgbus::{self},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick, order::BookOrder},
    enums::{
        AccountType, AggregationSource, AggressorSide, BarAggregation, BookType, ContingencyType,
        LiquiditySide, MarketStatus, MarketStatusAction, OmsType, OrderSide, OrderSideSpecified,
        OrderStatus, OrderType, PriceType, TimeInForce,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny, OrderExpired,
        OrderFilled, OrderModifyRejected, OrderRejected, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{EXPIRING_INSTRUMENT_TYPES, Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny, PassiveOrderAny, StopOrderAny},
    position::Position,
    types::{Currency, Money, Price, Quantity, fixed::FIXED_PRECISION},
};
use ustr::Ustr;

use crate::{
    matching_core::OrderMatchingCore,
    matching_engine::{config::OrderMatchingEngineConfig, ids_generator::IdsGenerator},
    messages::{BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder},
    models::{
        fee::{FeeModel, FeeModelAny},
        fill::FillModel,
    },
    trailing::trailing_stop_calculate,
};

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
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    book: OrderBook,
    pub core: OrderMatchingCore,
    fill_model: FillModel,
    fee_model: FeeModelAny,
    target_bid: Option<Price>,
    target_ask: Option<Price>,
    target_last: Option<Price>,
    last_bar_bid: Option<Bar>,
    last_bar_ask: Option<Bar>,
    execution_bar_types: HashMap<InstrumentId, BarType>,
    execution_bar_deltas: HashMap<BarType, TimeDelta>,
    account_ids: HashMap<TraderId, AccountId>,
    cached_filled_qty: HashMap<ClientOrderId, Quantity>,
    ids_generator: IdsGenerator,
}

impl OrderMatchingEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument: InstrumentAny,
        raw_id: u32,
        fill_model: FillModel,
        fee_model: FeeModelAny,
        book_type: BookType,
        oms_type: OmsType,
        account_type: AccountType,
        clock: Rc<RefCell<dyn Clock>>,
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
        let ids_generator = IdsGenerator::new(
            instrument.id().venue,
            oms_type,
            raw_id,
            config.use_random_ids,
            config.use_position_ids,
            cache.clone(),
        );

        Self {
            venue: instrument.id().venue,
            instrument,
            raw_id,
            fill_model,
            fee_model,
            book_type,
            oms_type,
            account_type,
            clock,
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
            cached_filled_qty: HashMap::new(),
            ids_generator,
        }
    }

    pub fn reset(&mut self) {
        self.book.clear(0, UnixNanos::default());
        self.execution_bar_types.clear();
        self.execution_bar_deltas.clear();
        self.account_ids.clear();
        self.cached_filled_qty.clear();
        self.core.reset();
        self.target_bid = None;
        self.target_ask = None;
        self.target_last = None;
        self.ids_generator.reset();

        log::info!("Reset {}", self.instrument.id());
    }

    pub const fn set_fill_model(&mut self, fill_model: FillModel) {
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
            PriceType::Mark => panic!("Not implemented"),
        }
    }

    fn process_trade_ticks_from_bar(&mut self, bar: &Bar) {
        // Split the bar into 4 trades with quarter volume
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
            self.ids_generator.generate_trade_id(),
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
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

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
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

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
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

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
    pub fn process_order(&mut self, order: &mut OrderAny, account_id: AccountId) {
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
                    if self.clock.borrow().timestamp_ns() < activation_ns {
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
                    if self.clock.borrow().timestamp_ns() >= expiration_ns {
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
                        match cache_borrow.order(client_order_id) {
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
                    return;
                }
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
                && position.is_none_or(|pos| {
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
            OrderType::TrailingStopMarket => self.process_trailing_stop_order(order),
            OrderType::TrailingStopLimit => self.process_trailing_stop_order(order),
        }
    }

    pub fn process_modify(&mut self, command: &ModifyOrder, account_id: AccountId) {
        if let Some(order) = self.core.get_order(command.client_order_id) {
            self.update_order(
                &mut order.to_any(),
                command.quantity,
                command.price,
                command.trigger_price,
                None,
            );
        } else {
            self.generate_order_modify_rejected(
                command.trader_id,
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                Ustr::from(format!("Order {} not found", command.client_order_id).as_str()),
                Some(command.venue_order_id),
                Some(account_id),
            );
        }
    }

    pub fn process_cancel(&mut self, command: &CancelOrder, account_id: AccountId) {
        match self.core.get_order(command.client_order_id) {
            Some(passive_order) => {
                if passive_order.is_inflight() || passive_order.is_open() {
                    self.cancel_order(&OrderAny::from(passive_order.to_owned()), None);
                }
            }
            None => self.generate_order_cancel_rejected(
                command.trader_id,
                command.strategy_id,
                account_id,
                command.instrument_id,
                command.client_order_id,
                command.venue_order_id,
                Ustr::from(format!("Order {} not found", command.client_order_id).as_str()),
            ),
        }
    }

    pub fn process_cancel_all(&mut self, command: &CancelAllOrders, account_id: AccountId) {
        let open_orders = self
            .cache
            .borrow()
            .orders_open(None, Some(&command.instrument_id), None, None)
            .into_iter()
            .cloned()
            .collect::<Vec<OrderAny>>();
        for order in open_orders {
            if command.order_side != OrderSide::NoOrderSide
                && command.order_side != order.order_side()
            {
                continue;
            }
            if order.is_inflight() || order.is_open() {
                self.cancel_order(&order, None);
            }
        }
    }

    pub fn process_batch_cancel(&mut self, command: &BatchCancelOrders, account_id: AccountId) {
        for order in &command.cancels {
            self.process_cancel(order, account_id);
        }
    }

    fn process_market_order(&mut self, order: &mut OrderAny) {
        if order.time_in_force() == TimeInForce::AtTheOpen
            || order.time_in_force() == TimeInForce::AtTheClose
        {
            log::error!(
                "Market auction for the time in force {} is currently not supported",
                order.time_in_force()
            );
            return;
        }

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

    fn process_limit_order(&mut self, order: &mut OrderAny) {
        let limit_px = order.price().expect("Limit order must have a price");
        if order.is_post_only()
            && self
                .core
                .is_limit_matched(order.order_side_specified(), limit_px)
        {
            self.generate_order_rejected(
                order,
                format!(
                    "POST_ONLY {} {} order limit px of {} would have been a TAKER: bid={}, ask={}",
                    order.order_type(),
                    order.order_side(),
                    order.price().unwrap(),
                    self.core
                        .bid
                        .map_or_else(|| "None".to_string(), |p| p.to_string()),
                    self.core
                        .ask
                        .map_or_else(|| "None".to_string(), |p| p.to_string())
                )
                .into(),
            );
            return;
        }

        // Order is valid and accepted
        self.accept_order(order);

        // Check for immediate fill
        if self
            .core
            .is_limit_matched(order.order_side_specified(), limit_px)
        {
            // Filling as liquidity taker
            if order.liquidity_side().is_some()
                && order.liquidity_side().unwrap() == LiquiditySide::NoLiquiditySide
            {
                order.set_liquidity_side(LiquiditySide::Taker);
            }
            self.fill_limit_order(order);
        } else if matches!(order.time_in_force(), TimeInForce::Fok | TimeInForce::Ioc) {
            self.cancel_order(order, None);
        }
    }

    fn process_market_to_limit_order(&mut self, order: &mut OrderAny) {
        // Check that market exists
        if (order.order_side() == OrderSide::Buy && !self.core.is_ask_initialized)
            || (order.order_side() == OrderSide::Sell && !self.core.is_bid_initialized)
        {
            self.generate_order_rejected(
                order,
                format!("No market for {}", order.instrument_id()).into(),
            );
            return;
        }

        // Immediately fill marketable order
        self.fill_market_order(order);

        if order.is_open() {
            self.accept_order(order);
        }
    }

    fn process_stop_market_order(&mut self, order: &mut OrderAny) {
        let stop_px = order
            .trigger_price()
            .expect("Stop order must have a trigger price");
        if self
            .core
            .is_stop_matched(order.order_side_specified(), stop_px)
        {
            if self.config.reject_stop_orders {
                self.generate_order_rejected(
                    order,
                    format!(
                        "{} {} order stop px of {} was in the market: bid={}, ask={}, but rejected because of configuration",
                        order.order_type(),
                        order.order_side(),
                        order.trigger_price().unwrap(),
                        self.core
                            .bid
                            .map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core
                            .ask
                            .map_or_else(|| "None".to_string(), |p| p.to_string())
                    ).into(),
                );
                return;
            }
            self.fill_market_order(order);
            return;
        }

        // order is not matched but is valid and we accept it
        self.accept_order(order);
    }

    fn process_stop_limit_order(&mut self, order: &mut OrderAny) {
        let stop_px = order
            .trigger_price()
            .expect("Stop order must have a trigger price");
        if self
            .core
            .is_stop_matched(order.order_side_specified(), stop_px)
        {
            if self.config.reject_stop_orders {
                self.generate_order_rejected(
                    order,
                    format!(
                        "{} {} order stop px of {} was in the market: bid={}, ask={}, but rejected because of configuration",
                        order.order_type(),
                        order.order_side(),
                        order.trigger_price().unwrap(),
                        self.core
                            .bid
                            .map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core
                            .ask
                            .map_or_else(|| "None".to_string(), |p| p.to_string())
                    ).into(),
                );
                return;
            }

            self.accept_order(order);
            self.generate_order_triggered(order);

            // Check for immediate fill
            let limit_px = order.price().expect("Stop limit order must have a price");
            if self
                .core
                .is_limit_matched(order.order_side_specified(), limit_px)
            {
                order.set_liquidity_side(LiquiditySide::Taker);
                self.fill_limit_order(order);
            }
        }

        // order is not matched but is valid and we accept it
        self.accept_order(order);
    }

    fn process_market_if_touched_order(&mut self, order: &mut OrderAny) {
        if self
            .core
            .is_touch_triggered(order.order_side_specified(), order.trigger_price().unwrap())
        {
            if self.config.reject_stop_orders {
                self.generate_order_rejected(
                    order,
                    format!(
                        "{} {} order trigger px of {} was in the market: bid={}, ask={}, but rejected because of configuration",
                        order.order_type(),
                        order.order_side(),
                        order.trigger_price().unwrap(),
                        self.core
                            .bid
                            .map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core
                            .ask
                            .map_or_else(|| "None".to_string(), |p| p.to_string())
                    ).into(),
                );
                return;
            }
            self.fill_market_order(order);
            return;
        }

        // Order is valid and accepted
        self.accept_order(order);
    }

    fn process_limit_if_touched_order(&mut self, order: &mut OrderAny) {
        if self
            .core
            .is_touch_triggered(order.order_side_specified(), order.trigger_price().unwrap())
        {
            if self.config.reject_stop_orders {
                self.generate_order_rejected(
                    order,
                    format!(
                        "{} {} order trigger px of {} was in the market: bid={}, ask={}, but rejected because of configuration",
                        order.order_type(),
                        order.order_side(),
                        order.trigger_price().unwrap(),
                        self.core
                            .bid
                            .map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core
                            .ask
                            .map_or_else(|| "None".to_string(), |p| p.to_string())
                    ).into(),
                );
                return;
            }
            self.accept_order(order);
            self.generate_order_triggered(order);

            // Check if immediate marketable
            if self
                .core
                .is_limit_matched(order.order_side_specified(), order.price().unwrap())
            {
                order.set_liquidity_side(LiquiditySide::Taker);
                self.fill_limit_order(order);
            }
            return;
        }

        // Order is valid and accepted
        self.accept_order(order);
    }

    fn process_trailing_stop_order(&mut self, order: &mut OrderAny) {
        if let Some(trigger_price) = order.trigger_price() {
            if self
                .core
                .is_stop_matched(order.order_side_specified(), trigger_price)
            {
                self.generate_order_rejected(
                    order,
                    format!(
                        "{} {} order trigger px of {} was in the market: bid={}, ask={}, but rejected because of configuration",
                        order.order_type(),
                        order.order_side(),
                        trigger_price,
                        self.core
                            .bid
                            .map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core
                            .ask
                            .map_or_else(|| "None".to_string(), |p| p.to_string())
                    ).into(),
                );
                return;
            }
        }

        // Order is valid and accepted
        self.accept_order(order);
    }

    // -- ORDER PROCESSING ----------------------------------------------------

    /// Iterate the matching engine by processing the bid and ask order sides
    /// and advancing time up to the given UNIX `timestamp_ns`.
    pub fn iterate(&mut self, timestamp_ns: UnixNanos) {
        // TODO implement correct clock fixed time setting self.clock.set_time(ts_now);

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
            }

            // Check expiration
            if self.config.support_gtd_orders {
                if let Some(expire_time) = order.expire_time() {
                    if timestamp_ns >= expire_time {
                        // SAFTEY: We know this order is in the core
                        self.core.delete_order(order).unwrap();
                        self.cached_filled_qty.remove(&order.client_order_id());
                        self.expire_order(order);
                    }
                }
            }

            // Manage trailing stop
            if let PassiveOrderAny::Stop(o) = order {
                if let PassiveOrderAny::Stop(
                    StopOrderAny::TrailingStopMarket(_) | StopOrderAny::TrailingStopLimit(_),
                ) = order
                {
                    let mut order = OrderAny::from(o.to_owned());
                    self.update_trailing_stop_order(&mut order);
                }
            }

            // Move market back to targets
            if let Some(target_bid) = self.target_bid {
                self.core.bid = Some(target_bid);
                self.target_bid = None;
            }
            if let Some(target_ask) = self.target_ask {
                self.core.ask = Some(target_ask);
                self.target_ask = None;
            }
            if let Some(target_last) = self.target_last {
                self.core.last = Some(target_last);
                self.target_last = None;
            }
        }

        // Reset any targets after iteration
        self.target_bid = None;
        self.target_ask = None;
        self.target_last = None;
    }

    fn determine_limit_price_and_volume(&mut self, order: &OrderAny) -> Vec<(Price, Quantity)> {
        match order.price() {
            Some(order_price) => {
                // construct book order with price as passive with limit order price
                let book_order =
                    BookOrder::new(order.order_side(), order_price, order.quantity(), 1);

                let mut fills = self.book.simulate_fills(&book_order);

                // return immediately if no fills
                if fills.is_empty() {
                    return fills;
                }

                // check if trigger price exists
                if let Some(triggered_price) = order.trigger_price() {
                    // Filling as TAKER from trigger
                    if order
                        .liquidity_side()
                        .is_some_and(|liquidity_side| liquidity_side == LiquiditySide::Taker)
                    {
                        if order.order_side() == OrderSide::Sell && order_price > triggered_price {
                            // manually change the fills index 0
                            let first_fill = fills.first().unwrap();
                            let triggered_qty = first_fill.1;
                            fills[0] = (triggered_price, triggered_qty);
                            self.target_bid = self.core.bid;
                            self.target_ask = self.core.ask;
                            self.target_last = self.core.last;
                            self.core.set_ask_raw(order_price);
                            self.core.set_last_raw(order_price);
                        } else if order.order_side() == OrderSide::Buy
                            && order_price < triggered_price
                        {
                            // manually change the fills index 0
                            let first_fill = fills.first().unwrap();
                            let triggered_qty = first_fill.1;
                            fills[0] = (triggered_price, triggered_qty);
                            self.target_bid = self.core.bid;
                            self.target_ask = self.core.ask;
                            self.target_last = self.core.last;
                            self.core.set_bid_raw(order_price);
                            self.core.set_last_raw(order_price);
                        }
                    }
                }

                // Filling as MAKER from trigger
                if order
                    .liquidity_side()
                    .is_some_and(|liquidity_side| liquidity_side == LiquiditySide::Maker)
                {
                    match order.order_side().as_specified() {
                        OrderSideSpecified::Buy => {
                            let target_price = if order
                                .trigger_price()
                                .is_some_and(|trigger_price| order_price > trigger_price)
                            {
                                order.trigger_price().unwrap()
                            } else {
                                order_price
                            };
                            for fill in &fills {
                                let last_px = fill.0;
                                if last_px < order_price {
                                    // Marketable SELL would have filled at limit
                                    self.target_bid = self.core.bid;
                                    self.target_ask = self.core.ask;
                                    self.target_last = self.core.last;
                                    self.core.set_ask_raw(target_price);
                                    self.core.set_last_raw(target_price);
                                }
                            }
                        }
                        OrderSideSpecified::Sell => {
                            let target_price = if order
                                .trigger_price()
                                .is_some_and(|trigger_price| order_price < trigger_price)
                            {
                                order.trigger_price().unwrap()
                            } else {
                                order_price
                            };
                            for fill in &fills {
                                let last_px = fill.0;
                                if last_px > order_price {
                                    // Marketable BUY would have filled at limit
                                    self.target_bid = self.core.bid;
                                    self.target_ask = self.core.ask;
                                    self.target_last = self.core.last;
                                    self.core.set_bid_raw(target_price);
                                    self.core.set_last_raw(target_price);
                                }
                            }
                        }
                    }
                }

                fills
            }
            None => panic!("Limit order must have a price"),
        }
    }

    fn determine_market_price_and_volume(&self, order: &OrderAny) -> Vec<(Price, Quantity)> {
        // construct price
        let price = match order.order_side().as_specified() {
            OrderSideSpecified::Buy => Price::max(FIXED_PRECISION),
            OrderSideSpecified::Sell => Price::min(FIXED_PRECISION),
        };

        // Construct BookOrder from order
        let book_order = BookOrder::new(order.order_side(), price, order.quantity(), 0);
        self.book.simulate_fills(&book_order)
    }

    pub fn fill_market_order(&mut self, order: &mut OrderAny) {
        if let Some(filled_qty) = self.cached_filled_qty.get(&order.client_order_id()) {
            if filled_qty >= &order.quantity() {
                log::info!(
                    "Ignoring fill as already filled pending application of events: {:?}, {:?}, {:?}, {:?}",
                    filled_qty,
                    order.quantity(),
                    order.filled_qty(),
                    order.quantity()
                );
                return;
            }
        }

        let venue_position_id = self.ids_generator.get_position_id(order, Some(true));
        let position: Option<Position> = if let Some(venue_position_id) = venue_position_id {
            let cache = self.cache.as_ref().borrow();
            cache.position(&venue_position_id).cloned()
        } else {
            None
        };

        if self.config.use_reduce_only && order.is_reduce_only() && position.is_none() {
            log::warn!(
                "Canceling REDUCE_ONLY {} as would increase position",
                order.order_type()
            );
            self.cancel_order(order, None);
            return;
        }
        // set order side as taker
        order.set_liquidity_side(LiquiditySide::Taker);
        let fills = self.determine_market_price_and_volume(order);
        self.apply_fills(order, fills, LiquiditySide::Taker, None, position);
    }

    pub fn fill_limit_order(&mut self, order: &mut OrderAny) {
        match order.price() {
            Some(order_price) => {
                let cached_filled_qty = self.cached_filled_qty.get(&order.client_order_id());
                if cached_filled_qty.is_some() && *cached_filled_qty.unwrap() >= order.quantity() {
                    log::debug!(
                        "Ignoring fill as already filled pending pending application of events: {}, {}, {}, {}",
                        cached_filled_qty.unwrap(),
                        order.quantity(),
                        order.filled_qty(),
                        order.leaves_qty(),
                    );
                    return;
                }

                if order
                    .liquidity_side()
                    .is_some_and(|liquidity_side| liquidity_side == LiquiditySide::Maker)
                {
                    if order.order_side() == OrderSide::Buy
                        && self.core.bid.is_some_and(|bid| bid == order_price)
                        && !self.fill_model.is_limit_filled()
                    {
                        // no filled
                        return;
                    }
                    if order.order_side() == OrderSide::Sell
                        && self.core.ask.is_some_and(|ask| ask == order_price)
                        && !self.fill_model.is_limit_filled()
                    {
                        // no filled
                        return;
                    }
                }

                let venue_position_id = self.ids_generator.get_position_id(order, None);
                let position = if let Some(venue_position_id) = venue_position_id {
                    let cache = self.cache.as_ref().borrow();
                    cache.position(&venue_position_id).cloned()
                } else {
                    None
                };

                if self.config.use_reduce_only && order.is_reduce_only() && position.is_none() {
                    log::warn!(
                        "Canceling REDUCE_ONLY {} as would increase position",
                        order.order_type()
                    );
                    self.cancel_order(order, None);
                    return;
                }

                let fills = self.determine_limit_price_and_volume(order);

                self.apply_fills(
                    order,
                    fills,
                    order.liquidity_side().unwrap(),
                    venue_position_id,
                    position,
                );
            }
            None => panic!("Limit order must have a price"),
        }
    }

    fn apply_fills(
        &mut self,
        order: &mut OrderAny,
        fills: Vec<(Price, Quantity)>,
        liquidity_side: LiquiditySide,
        venue_position_id: Option<PositionId>,
        position: Option<Position>,
    ) {
        if order.time_in_force() == TimeInForce::Fok {
            let mut total_size = Quantity::zero(order.quantity().precision);
            for (fill_px, fill_qty) in &fills {
                total_size = total_size.add(*fill_qty);
            }

            if order.leaves_qty() > total_size {
                self.cancel_order(order, None);
                return;
            }
        }

        if fills.is_empty() {
            if order.status() == OrderStatus::Submitted {
                self.generate_order_rejected(
                    order,
                    format!("No market for {}", order.instrument_id()).into(),
                );
            } else {
                log::error!(
                    "Cannot fill order: no fills from book when fills were expected (check size in data)"
                );
                return;
            }
        }

        if self.oms_type == OmsType::Netting {
            let venue_position_id: Option<PositionId> = None;
        }

        let mut initial_market_to_limit_fill = false;
        for &(mut fill_px, ref fill_qty) in &fills {
            // Validate price precision
            assert!(
                (fill_px.precision == self.instrument.price_precision()),
                "Invalid price precision for fill price {} when instrument price precision is {}.\
                     Check that the data price precision matches the {} instrument",
                fill_px.precision,
                self.instrument.price_precision(),
                self.instrument.id()
            );

            // Validate quantity precision
            assert!(
                (fill_qty.precision == self.instrument.size_precision()),
                "Invalid quantity precision for fill quantity {} when instrument size precision is {}.\
                     Check that the data quantity precision matches the {} instrument",
                fill_qty.precision,
                self.instrument.size_precision(),
                self.instrument.id()
            );

            if order.filled_qty() == Quantity::zero(order.filled_qty().precision)
                && order.order_type() == OrderType::MarketToLimit
            {
                self.generate_order_updated(order, order.quantity(), Some(fill_px), None);
                initial_market_to_limit_fill = true;
            }

            if self.book_type == BookType::L1_MBP && self.fill_model.is_slipped() {
                fill_px = match order.order_side().as_specified() {
                    OrderSideSpecified::Buy => fill_px.add(self.instrument.price_increment()),
                    OrderSideSpecified::Sell => fill_px.sub(self.instrument.price_increment()),
                }
            }

            // Check reduce only order
            if self.config.use_reduce_only && order.is_reduce_only() {
                if let Some(position) = &position {
                    if *fill_qty > position.quantity {
                        if position.quantity == Quantity::zero(position.quantity.precision) {
                            // Done
                            return;
                        }

                        // Adjust fill to honor reduce only execution (fill remaining position size only)
                        let adjusted_fill_qty =
                            Quantity::from_raw(position.quantity.raw, fill_qty.precision);

                        self.generate_order_updated(order, adjusted_fill_qty, None, None);
                    }
                }
            }

            if fill_qty.is_zero() {
                if fills.len() == 1 && order.status() == OrderStatus::Submitted {
                    self.generate_order_rejected(
                        order,
                        format!("No market for {}", order.instrument_id()).into(),
                    );
                }
                return;
            }

            self.fill_order(
                order,
                fill_px,
                *fill_qty,
                liquidity_side,
                venue_position_id,
                position.clone(),
            );

            if order.order_type() == OrderType::MarketToLimit && initial_market_to_limit_fill {
                // filled initial level
                return;
            }
        }

        if order.time_in_force() == TimeInForce::Ioc && order.is_open() {
            // IOC order has filled all available size
            self.cancel_order(order, None);
            return;
        }

        if order.is_open()
            && self.book_type == BookType::L1_MBP
            && matches!(
                order.order_type(),
                OrderType::Market | OrderType::MarketIfTouched | OrderType::StopMarket
            )
        {
            // Exhausted simulated book volume (continue aggressive filling into next level)
            // This is a very basic implementation of slipping by a single tick, in the future
            // we will implement more detailed fill modeling.
            todo!("Exhausted simulated book volume")
        }
    }

    fn fill_order(
        &mut self,
        order: &mut OrderAny,
        last_px: Price,
        last_qty: Quantity,
        liquidity_side: LiquiditySide,
        venue_position_id: Option<PositionId>,
        position: Option<Position>,
    ) {
        match self.cached_filled_qty.get(&order.client_order_id()) {
            Some(filled_qty) => {
                let leaves_qty = order.quantity() - *filled_qty;
                let last_qty = min(last_qty, leaves_qty);
                let new_filled_qty = *filled_qty + last_qty;
                // update cached filled qty
                self.cached_filled_qty
                    .insert(order.client_order_id(), new_filled_qty);
            }
            None => {
                self.cached_filled_qty
                    .insert(order.client_order_id(), last_qty);
            }
        }

        // calculate commission
        let commission = self
            .fee_model
            .get_commission(order, last_qty, last_px, &self.instrument)
            .unwrap();

        let venue_order_id = self.ids_generator.get_venue_order_id(order).unwrap();
        self.generate_order_filled(
            order,
            venue_order_id,
            venue_position_id,
            last_qty,
            last_px,
            self.instrument.quote_currency(),
            commission,
            liquidity_side,
        );

        if order.is_passive() && order.is_closed() {
            // Check if order exists in OrderMatching core, and delete it if it does
            if self.core.order_exists(order.client_order_id()) {
                let _ = self
                    .core
                    .delete_order(&PassiveOrderAny::from(order.clone()));
            }
            self.cached_filled_qty.remove(&order.client_order_id());
        }

        if !self.config.support_contingent_orders {
            return;
        }

        if let Some(contingency_type) = order.contingency_type() {
            match contingency_type {
                ContingencyType::Oto => {
                    if let Some(linked_orders_ids) = order.linked_order_ids() {
                        for client_order_id in linked_orders_ids {
                            let mut child_order = match self.cache.borrow().order(client_order_id) {
                                Some(child_order) => child_order.clone(),
                                None => panic!("Order {client_order_id} not found in cache"),
                            };

                            if child_order.is_closed() || child_order.is_active_local() {
                                continue;
                            }

                            // Check if we need to index position id
                            if let (None, Some(position_id)) =
                                (child_order.position_id(), order.position_id())
                            {
                                self.cache
                                    .borrow_mut()
                                    .add_position_id(
                                        &position_id,
                                        &self.venue,
                                        client_order_id,
                                        &child_order.strategy_id(),
                                    )
                                    .unwrap();
                                log::debug!(
                                    "Added position id {position_id} to cache for order {client_order_id}"
                                );
                            }

                            if (!child_order.is_open())
                                || (matches!(child_order.status(), OrderStatus::PendingUpdate)
                                    && child_order
                                        .previous_status()
                                        .is_some_and(|s| matches!(s, OrderStatus::Submitted)))
                            {
                                let account_id = order.account_id().unwrap_or_else(|| {
                                    *self.account_ids.get(&order.trader_id()).unwrap_or_else(|| {
                                        panic!(
                                            "Account ID not found for trader {}",
                                            order.trader_id()
                                        )
                                    })
                                });
                                self.process_order(&mut child_order, account_id);
                            }
                        }
                    } else {
                        log::error!(
                            "OTO order {} does not have linked orders",
                            order.client_order_id()
                        );
                    }
                }
                ContingencyType::Oco => {
                    if let Some(linked_orders_ids) = order.linked_order_ids() {
                        for client_order_id in linked_orders_ids {
                            let child_order = match self.cache.borrow().order(client_order_id) {
                                Some(child_order) => child_order.clone(),
                                None => panic!("Order {client_order_id} not found in cache"),
                            };

                            if child_order.is_closed() || child_order.is_active_local() {
                                continue;
                            }

                            self.cancel_order(&child_order, None);
                        }
                    } else {
                        log::error!(
                            "OCO order {} does not have linked orders",
                            order.client_order_id()
                        );
                    }
                }
                ContingencyType::Ouo => {
                    if let Some(linked_orders_ids) = order.linked_order_ids() {
                        for client_order_id in linked_orders_ids {
                            let mut child_order = match self.cache.borrow().order(client_order_id) {
                                Some(child_order) => child_order.clone(),
                                None => panic!("Order {client_order_id} not found in cache"),
                            };

                            if child_order.is_active_local() {
                                continue;
                            }

                            if order.is_closed() && child_order.is_open() {
                                self.cancel_order(&child_order, None);
                            } else if !order.leaves_qty().is_zero()
                                && order.leaves_qty() != child_order.leaves_qty()
                            {
                                let price = child_order.price();
                                let trigger_price = child_order.trigger_price();
                                self.update_order(
                                    &mut child_order,
                                    Some(order.leaves_qty()),
                                    price,
                                    trigger_price,
                                    Some(false),
                                );
                            }
                        }
                    } else {
                        log::error!(
                            "OUO order {} does not have linked orders",
                            order.client_order_id()
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn update_limit_order(&mut self, order: &mut OrderAny, quantity: Quantity, price: Price) {
        if self
            .core
            .is_limit_matched(order.order_side_specified(), price)
        {
            if order.is_post_only() {
                self.generate_order_modify_rejected(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    Ustr::from(format!(
                        "POST_ONLY {} {} order with new limit px of {} would have been a TAKER: bid={}, ask={}",
                        order.order_type(),
                        order.order_side(),
                        price,
                        self.core.bid.map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core.ask.map_or_else(|| "None".to_string(), |p| p.to_string())
                    ).as_str()),
                    order.venue_order_id(),
                    order.account_id(),
                );
                return;
            }

            self.generate_order_updated(order, quantity, Some(price), None);
            order.set_liquidity_side(LiquiditySide::Taker);
            self.fill_limit_order(order);
            return;
        }
        self.generate_order_updated(order, quantity, Some(price), None);
    }

    fn update_stop_market_order(
        &mut self,
        order: &mut OrderAny,
        quantity: Quantity,
        trigger_price: Price,
    ) {
        if self
            .core
            .is_stop_matched(order.order_side_specified(), trigger_price)
        {
            self.generate_order_modify_rejected(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                Ustr::from(
                    format!(
                        "{} {} order new stop px of {} was in the market: bid={}, ask={}",
                        order.order_type(),
                        order.order_side(),
                        trigger_price,
                        self.core
                            .bid
                            .map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core
                            .ask
                            .map_or_else(|| "None".to_string(), |p| p.to_string())
                    )
                    .as_str(),
                ),
                order.venue_order_id(),
                order.account_id(),
            );
            return;
        }

        self.generate_order_updated(order, quantity, None, Some(trigger_price));
    }

    fn update_stop_limit_order(
        &mut self,
        order: &mut OrderAny,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
    ) {
        if order.is_triggered().is_some_and(|t| t) {
            // Update limit price
            if self
                .core
                .is_limit_matched(order.order_side_specified(), price)
            {
                if order.is_post_only() {
                    self.generate_order_modify_rejected(
                        order.trader_id(),
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                        Ustr::from(format!(
                            "POST_ONLY {} {} order with new limit px of {} would have been a TAKER: bid={}, ask={}",
                            order.order_type(),
                            order.order_side(),
                            price,
                            self.core.bid.map_or_else(|| "None".to_string(), |p| p.to_string()),
                            self.core.ask.map_or_else(|| "None".to_string(), |p| p.to_string())
                        ).as_str()),
                        order.venue_order_id(),
                        order.account_id(),
                    );
                    return;
                }
                self.generate_order_updated(order, quantity, Some(price), None);
                order.set_liquidity_side(LiquiditySide::Taker);
                self.fill_limit_order(order);
                return; // Filled
            }
        } else {
            // Update stop price
            if self
                .core
                .is_stop_matched(order.order_side_specified(), trigger_price)
            {
                self.generate_order_modify_rejected(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    Ustr::from(
                        format!(
                            "{} {} order new stop px of {} was in the market: bid={}, ask={}",
                            order.order_type(),
                            order.order_side(),
                            trigger_price,
                            self.core
                                .bid
                                .map_or_else(|| "None".to_string(), |p| p.to_string()),
                            self.core
                                .ask
                                .map_or_else(|| "None".to_string(), |p| p.to_string())
                        )
                        .as_str(),
                    ),
                    order.venue_order_id(),
                    order.account_id(),
                );
                return;
            }
        }

        self.generate_order_updated(order, quantity, Some(price), Some(trigger_price));
    }

    fn update_market_if_touched_order(
        &mut self,
        order: &mut OrderAny,
        quantity: Quantity,
        trigger_price: Price,
    ) {
        if self
            .core
            .is_touch_triggered(order.order_side_specified(), trigger_price)
        {
            self.generate_order_modify_rejected(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                Ustr::from(
                    format!(
                        "{} {} order new trigger px of {} was in the market: bid={}, ask={}",
                        order.order_type(),
                        order.order_side(),
                        trigger_price,
                        self.core
                            .bid
                            .map_or_else(|| "None".to_string(), |p| p.to_string()),
                        self.core
                            .ask
                            .map_or_else(|| "None".to_string(), |p| p.to_string())
                    )
                    .as_str(),
                ),
                order.venue_order_id(),
                order.account_id(),
            );
            // Cannot update order
            return;
        }

        self.generate_order_updated(order, quantity, None, Some(trigger_price));
    }

    fn update_limit_if_touched_order(
        &mut self,
        order: &mut OrderAny,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
    ) {
        if order.is_triggered().is_some_and(|t| t) {
            // Update limit price
            if self
                .core
                .is_limit_matched(order.order_side_specified(), price)
            {
                if order.is_post_only() {
                    self.generate_order_modify_rejected(
                        order.trader_id(),
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                        Ustr::from(format!(
                            "POST_ONLY {} {} order with new limit px of {} would have been a TAKER: bid={}, ask={}",
                            order.order_type(),
                            order.order_side(),
                            price,
                            self.core.bid.map_or_else(|| "None".to_string(), |p| p.to_string()),
                            self.core.ask.map_or_else(|| "None".to_string(), |p| p.to_string())
                        ).as_str()),
                        order.venue_order_id(),
                        order.account_id(),
                    );
                    // Cannot update order
                    return;
                }
                self.generate_order_updated(order, quantity, Some(price), None);
                order.set_liquidity_side(LiquiditySide::Taker);
                self.fill_limit_order(order);
                return;
            }
        } else {
            // Update trigger price
            if self
                .core
                .is_touch_triggered(order.order_side_specified(), trigger_price)
            {
                self.generate_order_modify_rejected(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    Ustr::from(
                        format!(
                            "{} {} order new trigger px of {} was in the market: bid={}, ask={}",
                            order.order_type(),
                            order.order_side(),
                            trigger_price,
                            self.core
                                .bid
                                .map_or_else(|| "None".to_string(), |p| p.to_string()),
                            self.core
                                .ask
                                .map_or_else(|| "None".to_string(), |p| p.to_string())
                        )
                        .as_str(),
                    ),
                    order.venue_order_id(),
                    order.account_id(),
                );
                return;
            }
        }

        self.generate_order_updated(order, quantity, Some(price), Some(trigger_price));
    }

    fn update_trailing_stop_order(&mut self, order: &mut OrderAny) {
        let (new_trigger_price, new_price) = trailing_stop_calculate(
            self.instrument.price_increment(),
            order,
            self.core.bid,
            self.core.ask,
            self.core.last,
        )
        .unwrap();

        if new_trigger_price.is_none() && new_price.is_none() {
            return;
        }

        self.generate_order_updated(order, order.quantity(), new_price, new_trigger_price);
    }

    // -- EVENT HANDLING -----------------------------------------------------

    fn accept_order(&mut self, order: &mut OrderAny) {
        if order.is_closed() {
            // Temporary guard to prevent invalid processing
            return;
        }
        if order.status() != OrderStatus::Accepted {
            let venue_order_id = self.ids_generator.get_venue_order_id(order).unwrap();
            self.generate_order_accepted(order, venue_order_id);

            if matches!(
                order.order_type(),
                OrderType::TrailingStopLimit | OrderType::TrailingStopMarket
            ) && order.trigger_price().is_none()
            {
                self.update_trailing_stop_order(order);
            }
        }

        let _ = self.core.add_order(order.to_owned().into());
    }

    fn expire_order(&mut self, order: &PassiveOrderAny) {
        if self.config.support_contingent_orders
            && order
                .contingency_type()
                .is_some_and(|c| c != ContingencyType::NoContingency)
        {
            self.cancel_contingent_orders(&OrderAny::from(order.clone()));
        }

        self.generate_order_expired(&order.to_any());
    }

    fn cancel_order(&mut self, order: &OrderAny, cancel_contingencies: Option<bool>) {
        let cancel_contingencies = cancel_contingencies.unwrap_or(true);
        if order.is_active_local() {
            log::error!(
                "Cannot cancel an order with {} from the matching engine",
                order.status()
            );
            return;
        }

        // Check if order exists in OrderMatching core, and delete it if it does
        if self.core.order_exists(order.client_order_id()) {
            let _ = self
                .core
                .delete_order(&PassiveOrderAny::from(order.clone()));
        }
        self.cached_filled_qty.remove(&order.client_order_id());

        let venue_order_id = self.ids_generator.get_venue_order_id(order).unwrap();
        self.generate_order_canceled(order, venue_order_id);

        if self.config.support_contingent_orders
            && order.contingency_type().is_some()
            && order.contingency_type().unwrap() != ContingencyType::NoContingency
            && cancel_contingencies
        {
            self.cancel_contingent_orders(order);
        }
    }

    fn update_order(
        &mut self,
        order: &mut OrderAny,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        update_contingencies: Option<bool>,
    ) {
        let update_contingencies = update_contingencies.unwrap_or(true);
        let quantity = quantity.unwrap_or(order.quantity());

        match order {
            OrderAny::Limit(_) | OrderAny::MarketToLimit(_) => {
                let price = price.unwrap_or(order.price().unwrap());
                self.update_limit_order(order, quantity, price);
            }
            OrderAny::StopMarket(_) => {
                let trigger_price = trigger_price.unwrap_or(order.trigger_price().unwrap());
                self.update_stop_market_order(order, quantity, trigger_price);
            }
            OrderAny::StopLimit(_) => {
                let price = price.unwrap_or(order.price().unwrap());
                let trigger_price = trigger_price.unwrap_or(order.trigger_price().unwrap());
                self.update_stop_limit_order(order, quantity, price, trigger_price);
            }
            OrderAny::MarketIfTouched(_) => {
                let trigger_price = trigger_price.unwrap_or(order.trigger_price().unwrap());
                self.update_market_if_touched_order(order, quantity, trigger_price);
            }
            OrderAny::LimitIfTouched(_) => {
                let price = price.unwrap_or(order.price().unwrap());
                let trigger_price = trigger_price.unwrap_or(order.trigger_price().unwrap());
                self.update_limit_if_touched_order(order, quantity, price, trigger_price);
            }
            OrderAny::TrailingStopMarket(_) => {
                let trigger_price = trigger_price.unwrap_or(order.trigger_price().unwrap());
                self.update_market_if_touched_order(order, quantity, trigger_price);
            }
            OrderAny::TrailingStopLimit(trailing_stop_limit_order) => {
                let price = price.unwrap_or(trailing_stop_limit_order.price().unwrap());
                let trigger_price =
                    trigger_price.unwrap_or(trailing_stop_limit_order.trigger_price().unwrap());
                self.update_limit_if_touched_order(order, quantity, price, trigger_price);
            }
            _ => {
                panic!(
                    "Unsupported order type {} for update_order",
                    order.order_type()
                );
            }
        }

        if self.config.support_contingent_orders
            && order
                .contingency_type()
                .is_some_and(|c| c != ContingencyType::NoContingency)
            && update_contingencies
        {
            self.update_contingent_order(order);
        }
    }

    pub fn trigger_stop_order(&mut self, order: &mut OrderAny) {
        todo!("trigger_stop_order")
    }

    fn update_contingent_order(&mut self, order: &OrderAny) {
        log::debug!("Updating OUO orders from {}", order.client_order_id());
        if let Some(linked_order_ids) = order.linked_order_ids() {
            for client_order_id in linked_order_ids {
                let mut child_order = match self.cache.borrow().order(client_order_id) {
                    Some(order) => order.clone(),
                    None => panic!("Order {client_order_id} not found in cache."),
                };

                if child_order.is_active_local() {
                    continue;
                }

                if order.leaves_qty().is_zero() {
                    self.cancel_order(&child_order, None);
                } else if child_order.leaves_qty() != order.leaves_qty() {
                    let price = child_order.price();
                    let trigger_price = child_order.trigger_price();
                    self.update_order(
                        &mut child_order,
                        Some(order.leaves_qty()),
                        price,
                        trigger_price,
                        Some(false),
                    );
                }
            }
        }
    }

    fn cancel_contingent_orders(&mut self, order: &OrderAny) {
        if let Some(linked_order_ids) = order.linked_order_ids() {
            for client_order_id in linked_order_ids {
                let contingent_order = match self.cache.borrow().order(client_order_id) {
                    Some(order) => order.clone(),
                    None => panic!("Cannot find contingent order for {client_order_id}"),
                };
                if contingent_order.is_active_local() {
                    // order is not on the exchange yet
                    continue;
                }
                if !contingent_order.is_closed() {
                    self.cancel_order(&contingent_order, Some(false));
                }
            }
        }
    }

    // -- EVENT GENERATORS -----------------------------------------------------

    fn generate_order_rejected(&self, order: &OrderAny, reason: Ustr) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);
    }

    fn generate_order_accepted(&self, order: &mut OrderAny, venue_order_id: VenueOrderId) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);

        // TODO remove this when execution engine msgbus handlers are correctly set
        order.apply(event).expect("Failed to apply order event");
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_modify_rejected(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: Ustr,
        venue_order_id: Option<VenueOrderId>,
        account_id: Option<AccountId>,
    ) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
            venue_order_id,
            account_id,
        ));
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);
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
        let ts_now = self.clock.borrow().timestamp_ns();
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
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);
    }

    fn generate_order_updated(
        &self,
        order: &mut OrderAny,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
            price,
            trigger_price,
        ));
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);

        // TODO remove this when execution engine msgbus handlers are correctly set
        order.apply(event).expect("Failed to apply order event");
    }

    fn generate_order_canceled(&self, order: &OrderAny, venue_order_id: VenueOrderId) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);
    }

    fn generate_order_triggered(&self, order: &OrderAny) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);
    }

    fn generate_order_expired(&self, order: &OrderAny) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_filled(
        &mut self,
        order: &mut OrderAny,
        venue_order_id: VenueOrderId,
        venue_position_id: Option<PositionId>,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
    ) {
        let ts_now = self.clock.borrow().timestamp_ns();
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
            self.ids_generator.generate_trade_id(),
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
            venue_position_id,
            Some(commission),
        ));
        msgbus::send(&Ustr::from("ExecEngine.process"), &event as &dyn Any);

        // TODO remove this when execution engine msgbus handlers are correctly set
        order.apply(event).expect("Failed to apply order event");
    }
}
