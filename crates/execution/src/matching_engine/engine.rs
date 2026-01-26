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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::RefCell,
    cmp::min,
    fmt::Debug,
    ops::{Add, Sub},
    rc::Rc,
};

use ahash::AHashMap;
use chrono::TimeDelta;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    messages::execution::{BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder},
    msgbus::{self, MessagingSwitchboard},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick, order::BookOrder},
    enums::{
        AccountType, AggregationSource, AggressorSide, BookAction, BookType, ContingencyType,
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
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    position::Position,
    types::{
        Currency, Money, Price, Quantity, fixed::FIXED_PRECISION, price::PriceRaw,
        quantity::QuantityRaw,
    },
};
use ustr::Ustr;

use crate::{
    matching_core::{OrderMatchInfo, OrderMatchingCore},
    matching_engine::{config::OrderMatchingEngineConfig, ids_generator::IdsGenerator},
    models::{
        fee::{FeeModel, FeeModelAny},
        fill::FillModel,
    },
    protection::protection_price_calculate,
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
    core: OrderMatchingCore,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    book: OrderBook,
    fill_model: FillModel,
    fee_model: FeeModelAny,
    target_bid: Option<Price>,
    target_ask: Option<Price>,
    target_last: Option<Price>,
    last_bar_bid: Option<Bar>,
    last_bar_ask: Option<Bar>,
    fill_at_market: bool,
    execution_bar_types: AHashMap<InstrumentId, BarType>,
    execution_bar_deltas: AHashMap<BarType, TimeDelta>,
    account_ids: AHashMap<TraderId, AccountId>,
    cached_filled_qty: AHashMap<ClientOrderId, Quantity>,
    ids_generator: IdsGenerator,
    last_trade_size: Option<Quantity>,
    bid_consumption: AHashMap<PriceRaw, (QuantityRaw, QuantityRaw)>,
    ask_consumption: AHashMap<PriceRaw, (QuantityRaw, QuantityRaw)>,
    trade_consumption: QuantityRaw,
}

impl Debug for OrderMatchingEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderMatchingEngine))
            .field("venue", &self.venue)
            .field("instrument", &self.instrument.id())
            .finish()
    }
}

impl OrderMatchingEngine {
    /// Creates a new [`OrderMatchingEngine`] instance.
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
            market_status: MarketStatus::Open,
            config,
            core,
            target_bid: None,
            target_ask: None,
            target_last: None,
            last_bar_bid: None,
            last_bar_ask: None,
            fill_at_market: true,
            execution_bar_types: AHashMap::new(),
            execution_bar_deltas: AHashMap::new(),
            account_ids: AHashMap::new(),
            cached_filled_qty: AHashMap::new(),
            ids_generator,
            last_trade_size: None,
            bid_consumption: AHashMap::new(),
            ask_consumption: AHashMap::new(),
            trade_consumption: 0,
        }
    }

    /// Resets the matching engine to its initial state.
    ///
    /// Clears the order book, execution state, cached data, and resets all
    /// internal components. This is typically used for backtesting scenarios
    /// where the engine needs to be reset between test runs.
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
        self.last_trade_size = None;
        self.bid_consumption.clear();
        self.ask_consumption.clear();
        self.trade_consumption = 0;
        self.fill_at_market = true;
        self.ids_generator.reset();

        log::info!("Reset {}", self.instrument.id());
    }

    fn apply_liquidity_consumption(
        &mut self,
        fills: Vec<(Price, Quantity)>,
        order_side: OrderSide,
        leaves_qty: Quantity,
        book_prices: Option<&[Price]>,
    ) -> Vec<(Price, Quantity)> {
        if !self.config.liquidity_consumption {
            return fills;
        }

        let consumption = match order_side {
            OrderSide::Buy => &mut self.ask_consumption,
            OrderSide::Sell => &mut self.bid_consumption,
            _ => return fills,
        };

        let mut adjusted_fills = Vec::with_capacity(fills.len());
        let mut remaining_qty = leaves_qty.raw;

        for (fill_idx, (price, qty)) in fills.into_iter().enumerate() {
            if remaining_qty == 0 {
                break;
            }

            // Use book_price for consumption tracking (original price before MAKER adjustment),
            // but use price (potentially adjusted) for the output fill.
            let book_price = book_prices
                .and_then(|bp| bp.get(fill_idx).copied())
                .unwrap_or(price);

            let book_price_raw = book_price.raw;
            let level_size = self
                .book
                .get_quantity_at_level(book_price, order_side, qty.precision);

            let (original_size, consumed) = consumption
                .entry(book_price_raw)
                .or_insert((level_size.raw, 0));

            // Reset consumption when book size changes (fresh data)
            if *original_size != level_size.raw {
                *original_size = level_size.raw;
                *consumed = 0;
            }

            let available = original_size.saturating_sub(*consumed);
            if available == 0 {
                continue;
            }

            let adjusted_qty_raw = min(min(qty.raw, available), remaining_qty);
            if adjusted_qty_raw == 0 {
                continue;
            }

            *consumed += adjusted_qty_raw;
            remaining_qty -= adjusted_qty_raw;

            let adjusted_qty = Quantity::from_raw(adjusted_qty_raw, qty.precision);
            adjusted_fills.push((price, adjusted_qty));
        }

        adjusted_fills
    }

    /// Sets the fill model for the matching engine.
    pub const fn set_fill_model(&mut self, fill_model: FillModel) {
        self.fill_model = fill_model;
    }

    #[must_use]
    /// Returns the best bid price from the order book.
    pub fn best_bid_price(&self) -> Option<Price> {
        self.book.best_bid_price()
    }

    #[must_use]
    /// Returns the best ask price from the order book.
    pub fn best_ask_price(&self) -> Option<Price> {
        self.book.best_ask_price()
    }

    #[must_use]
    /// Returns a reference to the internal order book.
    pub const fn get_book(&self) -> &OrderBook {
        &self.book
    }

    #[must_use]
    /// Returns all open bid orders managed by the matching core.
    pub const fn get_open_bid_orders(&self) -> &[OrderMatchInfo] {
        self.core.get_orders_bid()
    }

    #[must_use]
    /// Returns all open ask orders managed by the matching core.
    pub const fn get_open_ask_orders(&self) -> &[OrderMatchInfo] {
        self.core.get_orders_ask()
    }

    #[must_use]
    /// Returns all open orders from both bid and ask sides.
    pub fn get_open_orders(&self) -> Vec<OrderMatchInfo> {
        let mut orders = Vec::new();
        orders.extend_from_slice(self.core.get_orders_bid());
        orders.extend_from_slice(self.core.get_orders_ask());
        orders
    }

    #[must_use]
    /// Returns true if an order with the given client order ID exists in the matching engine.
    pub fn order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.core.order_exists(client_order_id)
    }

    #[must_use]
    pub const fn get_core(&self) -> &OrderMatchingCore {
        &self.core
    }

    pub fn get_core_mut(&mut self) -> &mut OrderMatchingCore {
        &mut self.core
    }

    pub fn set_fill_at_market(&mut self, value: bool) {
        self.fill_at_market = value;
    }

    // -- DATA PROCESSING -------------------------------------------------------------------------

    /// Process the venues market for the given order book delta.
    ///
    /// # Errors
    ///
    /// - If delta order price precision does not match the instrument (for Add/Update actions).
    /// - If delta order size precision does not match the instrument (for Add/Update actions).
    /// - If applying the delta to the book fails.
    pub fn process_order_book_delta(&mut self, delta: &OrderBookDelta) -> anyhow::Result<()> {
        log::debug!("Processing {delta}");

        // Validate precision for Add and Update actions (Delete/Clear may have NULL_ORDER)
        if matches!(delta.action, BookAction::Add | BookAction::Update) {
            let price_prec = self.instrument.price_precision();
            let size_prec = self.instrument.size_precision();
            let instrument_id = self.instrument.id();

            if delta.order.price.precision != price_prec {
                anyhow::bail!(
                    "Invalid delta order price precision {prec}, expected {price_prec} for {instrument_id}",
                    prec = delta.order.price.precision
                );
            }
            if delta.order.size.precision != size_prec {
                anyhow::bail!(
                    "Invalid delta order size precision {prec}, expected {size_prec} for {instrument_id}",
                    prec = delta.order.size.precision
                );
            }
        }

        if self.book_type == BookType::L2_MBP || self.book_type == BookType::L3_MBO {
            self.book.apply_delta(delta)?;
        }

        self.iterate(delta.ts_init, AggressorSide::NoAggressor);
        Ok(())
    }

    /// Process the venues market for the given order book deltas.
    ///
    /// # Errors
    ///
    /// - If any delta order price precision does not match the instrument (for Add/Update actions).
    /// - If any delta order size precision does not match the instrument (for Add/Update actions).
    /// - If applying the deltas to the book fails.
    pub fn process_order_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        log::debug!("Processing {deltas}");

        // Validate precision for Add and Update actions (Delete/Clear may have NULL_ORDER)
        let price_prec = self.instrument.price_precision();
        let size_prec = self.instrument.size_precision();
        let instrument_id = self.instrument.id();

        for delta in &deltas.deltas {
            if matches!(delta.action, BookAction::Add | BookAction::Update) {
                if delta.order.price.precision != price_prec {
                    anyhow::bail!(
                        "Invalid delta order price precision {prec}, expected {price_prec} for {instrument_id}",
                        prec = delta.order.price.precision
                    );
                }
                if delta.order.size.precision != size_prec {
                    anyhow::bail!(
                        "Invalid delta order size precision {prec}, expected {size_prec} for {instrument_id}",
                        prec = delta.order.size.precision
                    );
                }
            }
        }

        if self.book_type == BookType::L2_MBP || self.book_type == BookType::L3_MBO {
            self.book.apply_deltas(deltas)?;
        }

        self.iterate(deltas.ts_init, AggressorSide::NoAggressor);
        Ok(())
    }

    /// # Panics
    ///
    /// - If updating the order book with the quote tick fails.
    /// - If bid/ask price precision does not match the instrument.
    /// - If bid/ask size precision does not match the instrument.
    pub fn process_quote_tick(&mut self, quote: &QuoteTick) {
        log::debug!("Processing {quote}");

        let price_prec = self.instrument.price_precision();
        let size_prec = self.instrument.size_precision();
        let instrument_id = self.instrument.id();

        assert!(
            quote.bid_price.precision == price_prec,
            "Invalid bid_price precision {}, expected {price_prec} for {instrument_id}",
            quote.bid_price.precision
        );
        assert!(
            quote.ask_price.precision == price_prec,
            "Invalid ask_price precision {}, expected {price_prec} for {instrument_id}",
            quote.ask_price.precision
        );
        assert!(
            quote.bid_size.precision == size_prec,
            "Invalid bid_size precision {}, expected {size_prec} for {instrument_id}",
            quote.bid_size.precision
        );
        assert!(
            quote.ask_size.precision == size_prec,
            "Invalid ask_size precision {}, expected {size_prec} for {instrument_id}",
            quote.ask_size.precision
        );

        if self.book_type == BookType::L1_MBP {
            self.book.update_quote_tick(quote).unwrap();
        }

        self.iterate(quote.ts_init, AggressorSide::NoAggressor);
    }

    /// Processes a bar and simulates market dynamics by creating synthetic ticks.
    ///
    /// For L1 books with bar execution enabled, generates synthetic trade or quote
    /// ticks from bar OHLC data to drive order matching.
    ///
    /// # Panics
    ///
    /// - If the bar type configuration is missing a time delta.
    /// - If bar OHLC price precision does not match the instrument.
    /// - If bar volume precision does not match the instrument.
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

        let price_prec = self.instrument.price_precision();
        let size_prec = self.instrument.size_precision();
        let instrument_id = self.instrument.id();

        assert!(
            bar.open.precision == price_prec,
            "Invalid bar open precision {}, expected {price_prec} for {instrument_id}",
            bar.open.precision
        );
        assert!(
            bar.high.precision == price_prec,
            "Invalid bar high precision {}, expected {price_prec} for {instrument_id}",
            bar.high.precision
        );
        assert!(
            bar.low.precision == price_prec,
            "Invalid bar low precision {}, expected {price_prec} for {instrument_id}",
            bar.low.precision
        );
        assert!(
            bar.close.precision == price_prec,
            "Invalid bar close precision {}, expected {price_prec} for {instrument_id}",
            bar.close.precision
        );
        assert!(
            bar.volume.precision == size_prec,
            "Invalid bar volume precision {}, expected {size_prec} for {instrument_id}",
            bar.volume.precision
        );

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
        // Split the bar into 4 trades, adding remainder to close trade
        let quarter_raw = bar.volume.raw / 4;
        let remainder_raw = bar.volume.raw % 4;
        let size = Quantity::from_raw(quarter_raw, bar.volume.precision);
        let close_size = Quantity::from_raw(quarter_raw + remainder_raw, bar.volume.precision);

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
            bar.ts_init,
            bar.ts_init,
        );

        // Open: fill at market price (gap from previous bar)
        if !self.core.is_last_initialized {
            self.fill_at_market = true;
            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init, AggressorSide::NoAggressor);
            self.core.set_last_raw(trade_tick.price);
        } else if self.core.last.is_some_and(|last| bar.open != last) {
            // Gap between previous close and this bar's open
            self.fill_at_market = true;
            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init, AggressorSide::NoAggressor);
            self.core.set_last_raw(trade_tick.price);
        }

        // High: fill at trigger price (market moving through prices)
        if self.core.last.is_some_and(|last| bar.high > last) {
            self.fill_at_market = false;
            trade_tick.price = bar.high;
            trade_tick.aggressor_side = AggressorSide::Buyer;
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init, AggressorSide::NoAggressor);

            self.core.set_last_raw(trade_tick.price);
        }

        // Low: fill at trigger price (market moving through prices)
        if self.core.last.is_some_and(|last| bar.low < last) {
            self.fill_at_market = false;
            trade_tick.price = bar.low;
            trade_tick.aggressor_side = AggressorSide::Seller;
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init, AggressorSide::NoAggressor);

            self.core.set_last_raw(trade_tick.price);
        }

        // Close: fill at trigger price (market moving through prices)
        if self.core.last.is_some_and(|last| bar.close != last) {
            self.fill_at_market = false;
            trade_tick.price = bar.close;
            trade_tick.size = close_size;
            if bar.close > self.core.last.unwrap() {
                trade_tick.aggressor_side = AggressorSide::Buyer;
            } else {
                trade_tick.aggressor_side = AggressorSide::Seller;
            }
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

            self.book.update_trade_tick(&trade_tick).unwrap();
            self.iterate(trade_tick.ts_init, AggressorSide::NoAggressor);

            self.core.set_last_raw(trade_tick.price);
        }

        self.fill_at_market = true;
    }

    fn process_quote_ticks_from_bar(&mut self, bar: &Bar) {
        // Wait for next bar
        if self.last_bar_bid.is_none()
            || self.last_bar_ask.is_none()
            || self.last_bar_bid.unwrap().ts_init != self.last_bar_ask.unwrap().ts_init
        {
            return;
        }
        let bid_bar = self.last_bar_bid.unwrap();
        let ask_bar = self.last_bar_ask.unwrap();

        // Split bar volume into 4, adding remainder to close quote
        let bid_quarter = bid_bar.volume.raw / 4;
        let bid_remainder = bid_bar.volume.raw % 4;
        let ask_quarter = ask_bar.volume.raw / 4;
        let ask_remainder = ask_bar.volume.raw % 4;

        let bid_size = Quantity::from_raw(bid_quarter, bar.volume.precision);
        let ask_size = Quantity::from_raw(ask_quarter, bar.volume.precision);
        let bid_close_size = Quantity::from_raw(bid_quarter + bid_remainder, bar.volume.precision);
        let ask_close_size = Quantity::from_raw(ask_quarter + ask_remainder, bar.volume.precision);

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

        // Open: fill at market price (gap from previous bar)
        self.fill_at_market = true;
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init, AggressorSide::NoAggressor);

        // High: fill at trigger price (market moving through prices)
        self.fill_at_market = false;
        quote_tick.bid_price = bid_bar.high;
        quote_tick.ask_price = ask_bar.high;
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init, AggressorSide::NoAggressor);

        // Low: fill at trigger price (market moving through prices)
        self.fill_at_market = false;
        quote_tick.bid_price = bid_bar.low;
        quote_tick.ask_price = ask_bar.low;
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init, AggressorSide::NoAggressor);

        // Close: fill at trigger price (market moving through prices)
        self.fill_at_market = false;
        quote_tick.bid_price = bid_bar.close;
        quote_tick.ask_price = ask_bar.close;
        quote_tick.bid_size = bid_close_size;
        quote_tick.ask_size = ask_close_size;
        self.book.update_quote_tick(&quote_tick).unwrap();
        self.iterate(quote_tick.ts_init, AggressorSide::NoAggressor);

        self.last_bar_bid = None;
        self.last_bar_ask = None;
        self.fill_at_market = true;
    }

    /// Processes a trade tick to update the market state.
    ///
    /// For L1 books, updates the order book with the trade. When trade execution
    /// is enabled, allows resting orders to fill against the trade price.
    ///
    /// # Panics
    ///
    /// - If updating the order book with the trade tick fails.
    /// - If trade price precision does not match the instrument.
    /// - If trade size precision does not match the instrument.
    pub fn process_trade_tick(&mut self, trade: &TradeTick) {
        log::debug!("Processing {trade}");

        let price_prec = self.instrument.price_precision();
        let size_prec = self.instrument.size_precision();
        let instrument_id = self.instrument.id();

        assert!(
            trade.price.precision == price_prec,
            "Invalid trade price precision {}, expected {price_prec} for {instrument_id}",
            trade.price.precision
        );
        assert!(
            trade.size.precision == size_prec,
            "Invalid trade size precision {}, expected {size_prec} for {instrument_id}",
            trade.size.precision
        );

        if self.book_type == BookType::L1_MBP {
            self.book.update_trade_tick(trade).unwrap();
        }

        let price_raw = trade.price.raw;
        self.core.set_last_raw(trade.price);

        let mut original_bid: Option<Price> = None;
        let mut original_ask: Option<Price> = None;

        // Initialize aggressor_side to NoAggressor (will reset bid/ask from book in iterate)
        // Only use actual aggressor when trade_execution is enabled (preserves trade price override)
        let mut aggressor_side = AggressorSide::NoAggressor;

        if self.config.trade_execution {
            aggressor_side = trade.aggressor_side;

            match aggressor_side {
                AggressorSide::Buyer => {
                    if self.core.ask.is_none() || price_raw > self.core.ask.map_or(0, |p| p.raw) {
                        self.core.set_ask_raw(trade.price);
                    }
                    if self.core.bid.is_none()
                        || price_raw < self.core.bid.map_or(PriceRaw::MAX, |p| p.raw)
                    {
                        self.core.set_bid_raw(trade.price);
                    }
                }
                AggressorSide::Seller => {
                    if self.core.bid.is_none()
                        || price_raw < self.core.bid.map_or(PriceRaw::MAX, |p| p.raw)
                    {
                        self.core.set_bid_raw(trade.price);
                    }
                    if self.core.ask.is_none() || price_raw > self.core.ask.map_or(0, |p| p.raw) {
                        self.core.set_ask_raw(trade.price);
                    }
                }
                AggressorSide::NoAggressor => {
                    if self.core.bid.is_none()
                        || price_raw <= self.core.bid.map_or(PriceRaw::MAX, |p| p.raw)
                    {
                        self.core.set_bid_raw(trade.price);
                    }
                    if self.core.ask.is_none() || price_raw >= self.core.ask.map_or(0, |p| p.raw) {
                        self.core.set_ask_raw(trade.price);
                    }
                }
            }

            original_bid = self.core.bid;
            original_ask = self.core.ask;

            match aggressor_side {
                AggressorSide::Seller => {
                    if original_ask.is_some_and(|ask| price_raw < ask.raw) {
                        self.core.set_ask_raw(trade.price);
                    }
                }
                AggressorSide::Buyer => {
                    if original_bid.is_some_and(|bid| price_raw > bid.raw) {
                        self.core.set_bid_raw(trade.price);
                    }
                }
                AggressorSide::NoAggressor => {}
            }

            self.last_trade_size = Some(trade.size);
            self.trade_consumption = 0;
        }

        self.iterate(trade.ts_init, aggressor_side);

        if self.config.trade_execution {
            self.last_trade_size = None;
            self.trade_consumption = 0;

            match aggressor_side {
                AggressorSide::Seller => {
                    if let Some(ask) = original_ask
                        && price_raw < ask.raw
                    {
                        self.core.ask = Some(ask);
                    }
                }
                AggressorSide::Buyer => {
                    if let Some(bid) = original_bid
                        && price_raw > bid.raw
                    {
                        self.core.bid = Some(bid);
                    }
                }
                AggressorSide::NoAggressor => {}
            }
        }
    }

    /// Processes a market status action to update the market state.
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

    /// Processes a new order submission.
    ///
    /// Validates the order against instrument precision, expiration, and contingency
    /// rules before accepting or rejecting it.
    ///
    /// # Panics
    ///
    /// Panics if the instrument activation timestamp is missing.
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
            if self.instrument.has_expiration() {
                if let Some(activation_ns) = self.instrument.activation_ns()
                    && self.clock.borrow().timestamp_ns() < activation_ns
                {
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
                if let Some(expiration_ns) = self.instrument.expiration_ns()
                    && self.clock.borrow().timestamp_ns() >= expiration_ns
                {
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

            // Check for valid order quantity precision
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
            if let Some(price) = order.price()
                && price.precision != self.instrument.price_precision()
            {
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

            // Check for valid order trigger price precision
            if let Some(trigger_price) = order.trigger_price()
                && trigger_price.precision != self.instrument.price_precision()
            {
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
            OrderType::Market if self.config.price_protection_points.is_some() => {
                self.process_market_order_with_protection(order);
            }
            OrderType::Market => self.process_market_order(order),
            OrderType::Limit => self.process_limit_order(order),
            OrderType::MarketToLimit => self.process_market_to_limit_order(order),
            OrderType::StopMarket if self.config.price_protection_points.is_some() => {
                self.process_stop_market_order_with_protection(order);
            }
            OrderType::StopMarket => self.process_stop_market_order(order),
            OrderType::StopLimit => self.process_stop_limit_order(order),
            OrderType::MarketIfTouched => self.process_market_if_touched_order(order),
            OrderType::LimitIfTouched => self.process_limit_if_touched_order(order),
            OrderType::TrailingStopMarket => self.process_trailing_stop_order(order),
            OrderType::TrailingStopLimit => self.process_trailing_stop_order(order),
        }
    }

    /// Processes an order modify command to update quantity, price, or trigger price.
    pub fn process_modify(&mut self, command: &ModifyOrder, account_id: AccountId) {
        if !self.core.order_exists(command.client_order_id) {
            self.generate_order_modify_rejected(
                command.trader_id,
                command.strategy_id,
                command.instrument_id,
                command.client_order_id,
                Ustr::from(format!("Order {} not found", command.client_order_id).as_str()),
                command.venue_order_id,
                Some(account_id),
            );
            return;
        }

        let mut order = match self.cache.borrow().order(&command.client_order_id).cloned() {
            Some(order) => order,
            None => {
                log::error!(
                    "Cannot modify order: order {} not found in cache",
                    command.client_order_id
                );
                return;
            }
        };

        let update_success = self.update_order(
            &mut order,
            command.quantity,
            command.price,
            command.trigger_price,
            None,
        );

        // Only persist changes if update succeeded and order is still open
        if update_success && order.is_open() {
            let _ = self.core.delete_order(command.client_order_id);
            let match_info = OrderMatchInfo::new(
                order.client_order_id(),
                order.order_side().as_specified(),
                order.order_type(),
                order.trigger_price(),
                order.price(),
                true,
            );
            self.core.add_order(match_info);
        }
    }

    /// Processes an order cancel command.
    pub fn process_cancel(&mut self, command: &CancelOrder, account_id: AccountId) {
        if !self.core.order_exists(command.client_order_id) {
            self.generate_order_cancel_rejected(
                command.trader_id,
                command.strategy_id,
                account_id,
                command.instrument_id,
                command.client_order_id,
                command.venue_order_id,
                Ustr::from(format!("Order {} not found", command.client_order_id).as_str()),
            );
            return;
        }

        let order = match self.cache.borrow().order(&command.client_order_id).cloned() {
            Some(order) => order,
            None => {
                log::error!(
                    "Cannot cancel order: order {} not found in cache",
                    command.client_order_id
                );
                return;
            }
        };

        if order.is_inflight() || order.is_open() {
            self.cancel_order(&order, None);
        }
    }

    /// Processes a cancel all orders command for an instrument.
    pub fn process_cancel_all(&mut self, command: &CancelAllOrders, account_id: AccountId) {
        let instrument_id = command.instrument_id;
        let open_orders = self
            .cache
            .borrow()
            .orders_open(None, Some(&instrument_id), None, None, None)
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

    /// Processes a batch cancel orders command.
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
        if (order.order_side() == OrderSide::Buy && !self.core.is_ask_initialized)
            || (order.order_side() == OrderSide::Sell && !self.core.is_bid_initialized)
        {
            self.generate_order_rejected(
                order,
                format!("No market for {}", order.instrument_id()).into(),
            );
            return;
        }

        if self.config.use_market_order_acks {
            let venue_order_id = self.ids_generator.get_venue_order_id(order).unwrap();
            self.generate_order_accepted(order, venue_order_id);
        }

        // Add order to cache for fill_market_order to fetch
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }

        self.fill_market_order(order.client_order_id());
    }

    fn process_market_order_with_protection(&mut self, order: &mut OrderAny) {
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
        if (order.order_side() == OrderSide::Buy && !self.core.is_ask_initialized)
            || (order.order_side() == OrderSide::Sell && !self.core.is_bid_initialized)
        {
            self.generate_order_rejected(
                order,
                format!("No market for {}", order.instrument_id()).into(),
            );
            return;
        }

        self.update_protection_price(order);

        let protection_price = order
            .price()
            .expect("Market order with protection must have a protection price");

        // Check for immediate fill
        if self
            .core
            .is_limit_matched(order.order_side_specified(), protection_price)
        {
            if self.config.use_market_order_acks {
                let venue_order_id = self.ids_generator.get_venue_order_id(order).unwrap();
                self.generate_order_accepted(order, venue_order_id);
            }

            // Filling as liquidity taker
            if order.liquidity_side().is_some()
                && order.liquidity_side().unwrap() == LiquiditySide::NoLiquiditySide
            {
                order.set_liquidity_side(LiquiditySide::Taker);
            }
            if let Err(e) = self
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
            {
                log::debug!("Order already in cache: {e}");
            }
            self.fill_limit_order(order.client_order_id());
        } else {
            // Order won't fill immediately, must accept into order book
            self.accept_order(order);

            if matches!(order.time_in_force(), TimeInForce::Fok | TimeInForce::Ioc) {
                self.cancel_order(order, None);
            }
        }
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
            order.set_liquidity_side(LiquiditySide::Taker);

            if self
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .is_err()
                && let Err(e) = self.cache.borrow_mut().update_order(order)
            {
                log::debug!("Failed to update order in cache: {e}");
            }
            self.fill_limit_order(order.client_order_id());
        } else if matches!(order.time_in_force(), TimeInForce::Fok | TimeInForce::Ioc) {
            self.cancel_order(order, None);
        } else {
            // Add passive order to cache for later modify/cancel operations
            order.set_liquidity_side(LiquiditySide::Maker);
            if let Err(e) = self
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
            {
                log::debug!("Order already in cache: {e}");
            }
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

        if self.config.use_market_order_acks {
            let venue_order_id = self.ids_generator.get_venue_order_id(order).unwrap();
            self.generate_order_accepted(order, venue_order_id);
        }

        // Immediately fill marketable order
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }
        let client_order_id = order.client_order_id();
        self.fill_market_order(client_order_id);

        // Check for remaining quantity to rest as limit order
        let filled_qty = self
            .cached_filled_qty
            .get(&client_order_id)
            .copied()
            .unwrap_or_default();
        let leaves_qty = order.quantity().saturating_sub(filled_qty);
        if !leaves_qty.is_zero() {
            // Re-fetch from cache to get updated price from partial fill
            let updated_order = self.cache.borrow().order(&client_order_id).cloned();
            if let Some(mut updated_order) = updated_order {
                self.accept_order(&mut updated_order);
            }
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
            if let Err(e) = self
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
            {
                log::debug!("Order already in cache: {e}");
            }
            self.fill_market_order(order.client_order_id());
            return;
        }

        // order is not matched but is valid and we accept it
        self.accept_order(order);

        // Add passive order to cache for later modify/cancel operations
        order.set_liquidity_side(LiquiditySide::Maker);
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }
    }

    fn process_stop_market_order_with_protection(&mut self, order: &mut OrderAny) {
        let stop_px = order
            .trigger_price()
            .expect("Stop order must have a trigger price");

        let order_side = order.order_side();
        let is_ask_initialized = self.core.is_ask_initialized;
        let is_bid_initialized = self.core.is_bid_initialized;
        if (order_side == OrderSide::Buy && !self.core.is_ask_initialized)
            || (order_side == OrderSide::Sell && !self.core.is_bid_initialized)
        {
            self.generate_order_rejected(
                order,
                format!("No market for {}", order.instrument_id()).into(),
            );
            return;
        }

        self.update_protection_price(order);
        let protection_price = order
            .price()
            .expect("Market order with protection must have a protection price");

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
            } else {
                // Order is valid and accepted
                self.accept_order(order);
            }

            if self
                .core
                .is_limit_matched(order.order_side_specified(), protection_price)
            {
                if let Err(e) = self
                    .cache
                    .borrow_mut()
                    .add_order(order.clone(), None, None, false)
                {
                    log::debug!("Order already in cache: {e}");
                }
                self.fill_limit_order(order.client_order_id());
            }
            return;
        }
        // Order is valid and accepted
        self.accept_order(order);

        // Add passive order to cache for later modify/cancel operations
        order.set_liquidity_side(LiquiditySide::Maker);
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }
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
                if let Err(e) = self
                    .cache
                    .borrow_mut()
                    .add_order(order.clone(), None, None, false)
                {
                    log::debug!("Order already in cache: {e}");
                }
                self.fill_limit_order(order.client_order_id());
            }

            // Order was triggered (and possibly filled), don't accept again
            return;
        }

        self.accept_order(order);

        // Add passive order to cache for later modify/cancel operations
        order.set_liquidity_side(LiquiditySide::Maker);
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }
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
            if let Err(e) = self
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
            {
                log::debug!("Order already in cache: {e}");
            }
            self.fill_market_order(order.client_order_id());
            return;
        }

        // Order is valid and accepted
        self.accept_order(order);

        // Add passive order to cache for later modify/cancel operations
        order.set_liquidity_side(LiquiditySide::Maker);
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }
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
                if let Err(e) = self
                    .cache
                    .borrow_mut()
                    .add_order(order.clone(), None, None, false)
                {
                    log::debug!("Order already in cache: {e}");
                }
                self.fill_limit_order(order.client_order_id());
            }
            return;
        }

        // Order is valid and accepted
        self.accept_order(order);

        // Add passive order to cache for later modify/cancel operations
        order.set_liquidity_side(LiquiditySide::Maker);
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }
    }

    fn process_trailing_stop_order(&mut self, order: &mut OrderAny) {
        if let Some(trigger_price) = order.trigger_price()
            && self
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

        // Order is valid and accepted
        self.accept_order(order);

        // Add passive order to cache for later modify/cancel operations
        order.set_liquidity_side(LiquiditySide::Maker);
        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
        {
            log::debug!("Order already in cache: {e}");
        }
    }

    // -- ORDER PROCESSING ----------------------------------------------------

    /// Iterate the matching engine by processing the bid and ask order sides
    /// and advancing time up to the given UNIX `timestamp_ns`.
    ///
    /// The `aggressor_side` parameter is used for trade execution processing.
    /// When not `NoAggressor`, the book-based bid/ask reset is skipped to preserve
    /// transient trade price overrides.
    ///
    /// # Panics
    ///
    /// Panics if the best bid or ask price is unavailable when iterating.
    pub fn iterate(&mut self, timestamp_ns: UnixNanos, aggressor_side: AggressorSide) {
        // TODO implement correct clock fixed time setting self.clock.set_time(ts_now);

        // Only reset bid/ask from book when not processing trade execution
        // (preserves transient trade price override for L2/L3 books)
        if aggressor_side == AggressorSide::NoAggressor {
            if self.book.has_bid() {
                self.core.set_bid_raw(self.book.best_bid_price().unwrap());
            }
            if self.book.has_ask() {
                self.core.set_ask_raw(self.book.best_ask_price().unwrap());
            }
        }

        self.core.iterate();

        let orders_bid = self.core.get_orders_bid().to_vec();
        let orders_ask = self.core.get_orders_ask().to_vec();

        self.iterate_orders(timestamp_ns, &orders_bid);
        self.iterate_orders(timestamp_ns, &orders_ask);

        // Restore core bid/ask to book values after order iteration
        // (during trade execution, transient override was used for matching)
        self.core.bid = self.book.best_bid_price();
        self.core.ask = self.book.best_ask_price();
    }

    fn maybe_activate_trailing_stop(
        &mut self,
        order: &mut OrderAny,
        bid: Option<Price>,
        ask: Option<Price>,
    ) -> bool {
        match order {
            OrderAny::TrailingStopMarket(inner) => {
                if inner.is_activated {
                    return true;
                }

                if inner.activation_price.is_none() {
                    let px = match inner.order_side() {
                        OrderSide::Buy => ask,
                        OrderSide::Sell => bid,
                        _ => None,
                    };
                    if let Some(p) = px {
                        inner.activation_price = Some(p);
                        inner.set_activated();
                        if let Err(e) = self.cache.borrow_mut().update_order(order) {
                            log::error!("Failed to update order: {e}");
                        }
                        return true;
                    }
                    return false;
                }

                let activation_price = inner.activation_price.unwrap();
                let hit = match inner.order_side() {
                    OrderSide::Buy => ask.is_some_and(|a| a <= activation_price),
                    OrderSide::Sell => bid.is_some_and(|b| b >= activation_price),
                    _ => false,
                };
                if hit {
                    inner.set_activated();
                    if let Err(e) = self.cache.borrow_mut().update_order(order) {
                        log::error!("Failed to update order: {e}");
                    }
                }
                hit
            }
            OrderAny::TrailingStopLimit(inner) => {
                if inner.is_activated {
                    return true;
                }

                if inner.activation_price.is_none() {
                    let px = match inner.order_side() {
                        OrderSide::Buy => ask,
                        OrderSide::Sell => bid,
                        _ => None,
                    };
                    if let Some(p) = px {
                        inner.activation_price = Some(p);
                        inner.set_activated();
                        if let Err(e) = self.cache.borrow_mut().update_order(order) {
                            log::error!("Failed to update order: {e}");
                        }
                        return true;
                    }
                    return false;
                }

                let activation_price = inner.activation_price.unwrap();
                let hit = match inner.order_side() {
                    OrderSide::Buy => ask.is_some_and(|a| a <= activation_price),
                    OrderSide::Sell => bid.is_some_and(|b| b >= activation_price),
                    _ => false,
                };
                if hit {
                    inner.set_activated();
                    if let Err(e) = self.cache.borrow_mut().update_order(order) {
                        log::error!("Failed to update order: {e}");
                    }
                }
                hit
            }
            _ => true,
        }
    }

    fn iterate_orders(&mut self, timestamp_ns: UnixNanos, orders: &[OrderMatchInfo]) {
        for match_info in orders {
            let order = match self
                .cache
                .borrow()
                .order(&match_info.client_order_id)
                .cloned()
            {
                Some(order) => order,
                None => {
                    log::warn!(
                        "Order {} not found in cache during iteration, skipping",
                        match_info.client_order_id
                    );
                    continue;
                }
            };

            if order.is_closed() {
                continue;
            }

            if self.config.support_gtd_orders
                && order
                    .expire_time()
                    .is_some_and(|expire_timestamp_ns| timestamp_ns >= expire_timestamp_ns)
            {
                let _ = self.core.delete_order(match_info.client_order_id);
                self.cached_filled_qty.remove(&match_info.client_order_id);
                self.expire_order(&order);
                continue;
            }

            if matches!(
                match_info.order_type,
                OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
            ) {
                let mut any = order;

                if !self.maybe_activate_trailing_stop(&mut any, self.core.bid, self.core.ask) {
                    continue;
                }

                self.update_trailing_stop_order(&mut any);

                // Persist the activated/updated trailing stop back to the core
                let _ = self.core.delete_order(match_info.client_order_id);
                let updated_match_info = OrderMatchInfo::new(
                    any.client_order_id(),
                    any.order_side().as_specified(),
                    any.order_type(),
                    any.trigger_price(),
                    any.price(),
                    match &any {
                        OrderAny::TrailingStopMarket(o) => o.is_activated,
                        OrderAny::TrailingStopLimit(o) => o.is_activated,
                        _ => true,
                    },
                );
                self.core.add_order(updated_match_info);
            }

            // Move market back to targets
            if let Some(target_bid) = self.target_bid {
                self.core.bid = Some(target_bid);
                self.target_bid = None;
            }
            if let Some(target_bid) = self.target_bid.take() {
                self.core.bid = Some(target_bid);
                self.target_bid = None;
            }
            if let Some(target_ask) = self.target_ask.take() {
                self.core.ask = Some(target_ask);
                self.target_ask = None;
            }
            if let Some(target_last) = self.target_last.take() {
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
                // When liquidity consumption is enabled, get ALL crossed levels so that
                // consumed levels can be filtered out while still finding valid ones.
                // Otherwise simulate_fills only returns enough levels to satisfy leaves_qty,
                // which may all be consumed, missing other valid crossed levels.
                let mut fills = if self.config.liquidity_consumption {
                    let size_prec = self.instrument.size_precision();
                    self.book
                        .get_all_crossed_levels(order.order_side(), order_price, size_prec)
                } else {
                    let book_order =
                        BookOrder::new(order.order_side(), order_price, order.quantity(), 1);
                    self.book.simulate_fills(&book_order)
                };

                // Trade execution: use trade-driven fill when book doesn't reflect trade price
                if let Some(trade_size) = self.last_trade_size
                    && let Some(trade_price) = self.core.last
                {
                    let fills_at_trade_price = fills.iter().any(|(px, _)| *px == trade_price);

                    if !fills_at_trade_price
                        && self
                            .core
                            .is_limit_matched(order.order_side_specified(), order_price)
                    {
                        // Fill model check for MAKER at limit is already handled in fill_limit_order,
                        // don't re-check here to avoid calling is_limit_filled() twice (p² probability).
                        let leaves_qty = order.leaves_qty();
                        let available_qty = if self.config.liquidity_consumption {
                            let remaining = trade_size.raw.saturating_sub(self.trade_consumption);
                            Quantity::from_raw(remaining, trade_size.precision)
                        } else {
                            trade_size
                        };

                        let fill_qty = min(leaves_qty, available_qty);

                        if !fill_qty.is_zero() {
                            log::debug!(
                                "Trade execution fill: {} @ {} (trade_price={}, available: {}, book had {} fills)",
                                fill_qty,
                                order_price,
                                trade_price,
                                available_qty,
                                fills.len()
                            );

                            if self.config.liquidity_consumption {
                                self.trade_consumption += fill_qty.raw;
                            }

                            // Fill at the limit price (conservative) rather than the trade price.
                            // Trade execution fills already account for consumption via trade_consumption,
                            // return early to bypass apply_liquidity_consumption which would incorrectly
                            // discard these fills when the trade price isn't in the order book.
                            return vec![(order_price, fill_qty)];
                        }
                    }
                }

                // Return immediately if no fills
                if fills.is_empty() {
                    return fills;
                }

                // Save original book prices BEFORE any fill price modifications for consumption tracking,
                // since the TAKER and MAKER loops below may adjust fill prices. Consumption should be
                // tracked against the original book price levels where liquidity was sourced from.
                let book_prices: Vec<Price> = if self.config.liquidity_consumption {
                    fills.iter().map(|(px, _)| *px).collect()
                } else {
                    Vec::new()
                };
                let book_prices_ref: Option<&[Price]> = if book_prices.is_empty() {
                    None
                } else {
                    Some(&book_prices)
                };

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
                            for fill in &mut fills {
                                let last_px = fill.0;
                                if last_px < order_price {
                                    // Marketable BUY would have filled at limit
                                    self.target_bid = self.core.bid;
                                    self.target_ask = self.core.ask;
                                    self.target_last = self.core.last;
                                    self.core.set_ask_raw(target_price);
                                    self.core.set_last_raw(target_price);
                                    fill.0 = target_price;
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
                            for fill in &mut fills {
                                let last_px = fill.0;
                                if last_px > order_price {
                                    // Marketable SELL would have filled at limit
                                    self.target_bid = self.core.bid;
                                    self.target_ask = self.core.ask;
                                    self.target_last = self.core.last;
                                    self.core.set_bid_raw(target_price);
                                    self.core.set_last_raw(target_price);
                                    fill.0 = target_price;
                                }
                            }
                        }
                    }
                }

                self.apply_liquidity_consumption(
                    fills,
                    order.order_side(),
                    order.leaves_qty(),
                    book_prices_ref,
                )
            }
            None => panic!("Limit order must have a price"),
        }
    }

    fn determine_market_price_and_volume(&mut self, order: &OrderAny) -> Vec<(Price, Quantity)> {
        let price = match order.order_side().as_specified() {
            OrderSideSpecified::Buy => Price::max(FIXED_PRECISION),
            OrderSideSpecified::Sell => Price::min(FIXED_PRECISION),
        };

        // When liquidity consumption is enabled, get ALL crossed levels so that
        // consumed levels can be filtered out while still finding valid ones.
        let mut fills = if self.config.liquidity_consumption {
            let size_prec = self.instrument.size_precision();
            self.book
                .get_all_crossed_levels(order.order_side(), price, size_prec)
        } else {
            let book_order = BookOrder::new(order.order_side(), price, order.quantity(), 0);
            self.book.simulate_fills(&book_order)
        };

        // For stop market and market-if-touched orders during bar H/L/C processing, fill at trigger price
        // (market moved through the trigger). For gaps/immediate triggers, fill at market.
        if !self.fill_at_market
            && self.book_type == BookType::L1_MBP
            && !fills.is_empty()
            && matches!(
                order.order_type(),
                OrderType::StopMarket | OrderType::TrailingStopMarket | OrderType::MarketIfTouched
            )
            && let Some(trigger_price) = order.trigger_price()
        {
            fills[0] = (trigger_price, fills[0].1);

            // Skip liquidity consumption for trigger price fills (gap price may not exist in book).
            let mut remaining_qty = order.leaves_qty().raw;
            let mut capped_fills = Vec::with_capacity(fills.len());

            for (price, qty) in fills {
                if remaining_qty == 0 {
                    break;
                }

                let capped_qty_raw = min(qty.raw, remaining_qty);
                if capped_qty_raw == 0 {
                    continue;
                }

                remaining_qty -= capped_qty_raw;
                capped_fills.push((price, Quantity::from_raw(capped_qty_raw, qty.precision)));
            }

            return capped_fills;
        }

        self.apply_liquidity_consumption(fills, order.order_side(), order.leaves_qty(), None)
    }

    /// Fills a market order against the current order book.
    ///
    /// The order is filled as a taker against available liquidity.
    /// Reduce-only orders are canceled if no position exists.
    pub fn fill_market_order(&mut self, client_order_id: ClientOrderId) {
        let mut order = match self.cache.borrow().order(&client_order_id).cloned() {
            Some(order) => order,
            None => {
                log::error!("Cannot fill market order: order {client_order_id} not found in cache");
                return;
            }
        };

        if let Some(filled_qty) = self.cached_filled_qty.get(&order.client_order_id())
            && filled_qty >= &order.quantity()
        {
            log::info!(
                "Ignoring fill as already filled pending application of events: {:?}, {:?}, {:?}, {:?}",
                filled_qty,
                order.quantity(),
                order.filled_qty(),
                order.quantity()
            );
            return;
        }

        let venue_position_id = self.ids_generator.get_position_id(&order, Some(true));
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
            self.cancel_order(&order, None);
            return;
        }
        // set order side as taker
        order.set_liquidity_side(LiquiditySide::Taker);
        let fills = self.determine_market_price_and_volume(&order);
        self.apply_fills(&mut order, fills, LiquiditySide::Taker, None, position);
    }

    /// Attempts to fill a limit order against the current order book.
    ///
    /// Determines fill prices and quantities based on available liquidity,
    /// then applies the fills to the order.
    ///
    /// # Panics
    ///
    /// Panics if the order has no price, or if fill price or quantity precision mismatches occur.
    pub fn fill_limit_order(&mut self, client_order_id: ClientOrderId) {
        let mut order = match self.cache.borrow().order(&client_order_id).cloned() {
            Some(order) => order,
            None => {
                log::error!("Cannot fill limit order: order {client_order_id} not found in cache");
                return;
            }
        };

        match order.price() {
            Some(order_price) => {
                let cached_filled_qty = self.cached_filled_qty.get(&order.client_order_id());
                if let Some(&qty) = cached_filled_qty
                    && qty >= order.quantity()
                {
                    log::debug!(
                        "Ignoring fill as already filled pending pending application of events: {}, {}, {}, {}",
                        qty,
                        order.quantity(),
                        order.filled_qty(),
                        order.leaves_qty(),
                    );
                    return;
                }

                // Check fill model for MAKER orders at the limit price
                if order
                    .liquidity_side()
                    .is_some_and(|liquidity_side| liquidity_side == LiquiditySide::Maker)
                {
                    // For trade execution: check if trade price equals order price
                    // For quote updates: check if bid/ask equals order price
                    let at_limit = if self.last_trade_size.is_some() && self.core.last.is_some() {
                        self.core.last.is_some_and(|last| last == order_price)
                    } else if order.order_side() == OrderSide::Buy {
                        self.core.bid.is_some_and(|bid| bid == order_price)
                    } else {
                        self.core.ask.is_some_and(|ask| ask == order_price)
                    };

                    if at_limit && !self.fill_model.is_limit_filled() {
                        return; // Not filled (simulates queue position)
                    }
                }

                let venue_position_id = self.ids_generator.get_position_id(&order, None);
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
                    self.cancel_order(&order, None);
                    return;
                }

                let fills = self.determine_limit_price_and_volume(&order);

                // Skip apply_fills when consumed-liquidity adjustment produces no fills.
                // This occurs for partially filled orders when an unrelated delta arrives
                // and no new liquidity is available at the order's price level.
                if fills.is_empty() && self.config.liquidity_consumption {
                    log::debug!(
                        "Skipping fill for {}: no liquidity available after consumption",
                        order.client_order_id()
                    );

                    if matches!(order.time_in_force(), TimeInForce::Fok | TimeInForce::Ioc) {
                        self.cancel_order(&order, None);
                    }

                    return;
                }

                let liquidity_side = order.liquidity_side().unwrap();
                self.apply_fills(
                    &mut order,
                    fills,
                    liquidity_side,
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

        // For netting mode, don't use venue position ID (use None instead)
        let venue_position_id = if self.oms_type == OmsType::Netting {
            None
        } else {
            venue_position_id
        };

        let mut initial_market_to_limit_fill = false;

        for &(mut fill_px, ref fill_qty) in &fills {
            assert!(
                (fill_px.precision == self.instrument.price_precision()),
                "Invalid price precision for fill price {} when instrument price precision is {}.\
                     Check that the data price precision matches the {} instrument",
                fill_px.precision,
                self.instrument.price_precision(),
                self.instrument.id()
            );

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
                self.generate_order_updated(order, order.quantity(), Some(fill_px), None, None);
                initial_market_to_limit_fill = true;
            }

            if self.book_type == BookType::L1_MBP && self.fill_model.is_slipped() {
                fill_px = match order.order_side().as_specified() {
                    OrderSideSpecified::Buy => fill_px.add(self.instrument.price_increment()),
                    OrderSideSpecified::Sell => fill_px.sub(self.instrument.price_increment()),
                }
            }

            // Check reduce only order
            // If the incoming simulated fill would exceed the position when reduce-only is honored,
            // clamp the effective fill size to the adjusted (remaining position) quantity.
            let mut effective_fill_qty = *fill_qty;

            if self.config.use_reduce_only
                && order.is_reduce_only()
                && let Some(position) = &position
                && *fill_qty > position.quantity
            {
                if position.quantity == Quantity::zero(position.quantity.precision) {
                    // Done
                    return;
                }

                // Adjusted target quantity equals the remaining position size
                let adjusted_fill_qty =
                    Quantity::from_raw(position.quantity.raw, fill_qty.precision);

                // Determine the effective fill size for this iteration first
                effective_fill_qty = min(effective_fill_qty, adjusted_fill_qty);

                // Only emit an update if the order quantity actually changes
                if order.quantity() != adjusted_fill_qty {
                    self.generate_order_updated(order, adjusted_fill_qty, None, None, None);
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
                effective_fill_qty,
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
                OrderType::Market
                    | OrderType::MarketIfTouched
                    | OrderType::StopMarket
                    | OrderType::TrailingStopMarket
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
        let size_prec = self.instrument.size_precision();
        let instrument_id = self.instrument.id();
        assert!(
            last_qty.precision == size_prec,
            "Invalid fill quantity precision {}, expected {size_prec} for {instrument_id}",
            last_qty.precision
        );

        match self.cached_filled_qty.get(&order.client_order_id()) {
            Some(filled_qty) => {
                // Use saturating_sub to prevent panic if filled_qty > quantity
                let leaves_qty = order.quantity().saturating_sub(*filled_qty);
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
                let _ = self.core.delete_order(order.client_order_id());
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

            self.generate_order_updated(order, quantity, Some(price), None, None);
            order.set_liquidity_side(LiquiditySide::Taker);
            if let Err(e) = self.cache.borrow_mut().update_order(order) {
                log::debug!("Failed to update order in cache: {e}");
            }
            self.fill_limit_order(order.client_order_id());
            return;
        }
        self.generate_order_updated(order, quantity, Some(price), None, None);
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

        self.generate_order_updated(order, quantity, None, Some(trigger_price), None);
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
                self.generate_order_updated(order, quantity, Some(price), None, None);
                order.set_liquidity_side(LiquiditySide::Taker);
                if let Err(e) = self
                    .cache
                    .borrow_mut()
                    .add_order(order.clone(), None, None, false)
                {
                    log::debug!("Order already in cache: {e}");
                }
                self.fill_limit_order(order.client_order_id());
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

        self.generate_order_updated(order, quantity, Some(price), Some(trigger_price), None);
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

        self.generate_order_updated(order, quantity, None, Some(trigger_price), None);
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
                self.generate_order_updated(order, quantity, Some(price), None, None);
                order.set_liquidity_side(LiquiditySide::Taker);
                self.fill_limit_order(order.client_order_id());
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

        self.generate_order_updated(order, quantity, Some(price), Some(trigger_price), None);
    }

    fn update_trailing_stop_order(&mut self, order: &mut OrderAny) {
        let (new_trigger_price, new_price) = trailing_stop_calculate(
            self.instrument.price_increment(),
            order.trigger_price(),
            order.activation_price(),
            order,
            self.core.bid,
            self.core.ask,
            self.core.last,
        )
        .unwrap();

        if new_trigger_price.is_none() && new_price.is_none() {
            return;
        }

        self.generate_order_updated(order, order.quantity(), new_price, new_trigger_price, None);
    }

    fn update_protection_price(&mut self, order: &mut OrderAny) {
        let protection_price = protection_price_calculate(
            self.instrument.price_increment(),
            order,
            self.config.price_protection_points,
            self.core.bid,
            self.core.ask,
        );

        if let Ok(protection_price) = protection_price {
            self.generate_order_updated(
                order,
                order.quantity(),
                None,
                None,
                Some(protection_price),
            );
        }
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

        let match_info = OrderMatchInfo::new(
            order.client_order_id(),
            order.order_side().as_specified(),
            order.order_type(),
            order.trigger_price(),
            order.price(),
            match order {
                OrderAny::TrailingStopMarket(o) => o.is_activated,
                OrderAny::TrailingStopLimit(o) => o.is_activated,
                _ => true,
            },
        );
        self.core.add_order(match_info);
    }

    fn expire_order(&mut self, order: &OrderAny) {
        if self.config.support_contingent_orders
            && order
                .contingency_type()
                .is_some_and(|c| c != ContingencyType::NoContingency)
        {
            self.cancel_contingent_orders(order);
        }

        self.generate_order_expired(order);
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
            let _ = self.core.delete_order(order.client_order_id());
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
    ) -> bool {
        let update_contingencies = update_contingencies.unwrap_or(true);
        let quantity = quantity.unwrap_or(order.quantity());

        let price_prec = self.instrument.price_precision();
        let size_prec = self.instrument.size_precision();
        let instrument_id = self.instrument.id();
        if quantity.precision != size_prec {
            self.generate_order_modify_rejected(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                Ustr::from(&format!(
                    "Invalid update quantity precision {}, expected {size_prec} for {instrument_id}",
                    quantity.precision
                )),
                order.venue_order_id(),
                order.account_id(),
            );
            return false;
        }
        if let Some(px) = price
            && px.precision != price_prec
        {
            self.generate_order_modify_rejected(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                Ustr::from(&format!(
                    "Invalid update price precision {}, expected {price_prec} for {instrument_id}",
                    px.precision
                )),
                order.venue_order_id(),
                order.account_id(),
            );
            return false;
        }
        if let Some(tp) = trigger_price
            && tp.precision != price_prec
        {
            self.generate_order_modify_rejected(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                Ustr::from(&format!(
                    "Invalid update trigger_price precision {}, expected {price_prec} for {instrument_id}",
                    tp.precision
                )),
                order.venue_order_id(),
                order.account_id(),
            );
            return false;
        }

        // Use cached_filled_qty since PassiveOrderAny in core is not updated with fills
        let filled_qty = self
            .cached_filled_qty
            .get(&order.client_order_id())
            .copied()
            .unwrap_or(order.filled_qty());
        if quantity < filled_qty {
            self.generate_order_modify_rejected(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                Ustr::from(&format!(
                    "Cannot reduce order quantity {quantity} below filled quantity {filled_qty}",
                )),
                order.venue_order_id(),
                order.account_id(),
            );
            return false;
        }

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

        // If order now has zero leaves after update, cancel it
        let new_leaves_qty = quantity.saturating_sub(filled_qty);
        if new_leaves_qty.is_zero() {
            if self.config.support_contingent_orders
                && order
                    .contingency_type()
                    .is_some_and(|c| c != ContingencyType::NoContingency)
                && update_contingencies
            {
                self.update_contingent_order(order);
            }
            // Pass false since we already handled contingents above
            self.cancel_order(order, Some(false));
            return true;
        }

        if self.config.support_contingent_orders
            && order
                .contingency_type()
                .is_some_and(|c| c != ContingencyType::NoContingency)
            && update_contingencies
        {
            self.update_contingent_order(order);
        }

        true
    }

    /// Triggers a stop order, converting it to an active market or limit order.
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
            OrderType::StopMarket | OrderType::MarketIfTouched | OrderType::TrailingStopMarket => {
                self.fill_market_order(client_order_id);
            }
            _ => {
                log::error!(
                    "Cannot trigger stop order: invalid order type {}",
                    order.order_type()
                );
            }
        }
    }

    fn update_contingent_order(&mut self, order: &OrderAny) {
        log::debug!("Updating OUO orders from {}", order.client_order_id());
        if let Some(linked_order_ids) = order.linked_order_ids() {
            let parent_filled_qty = self
                .cached_filled_qty
                .get(&order.client_order_id())
                .copied()
                .unwrap_or(order.filled_qty());
            let parent_leaves_qty = order.quantity().saturating_sub(parent_filled_qty);

            for client_order_id in linked_order_ids {
                let mut child_order = match self.cache.borrow().order(client_order_id) {
                    Some(order) => order.clone(),
                    None => panic!("Order {client_order_id} not found in cache."),
                };

                if child_order.is_active_local() {
                    continue;
                }

                let child_filled_qty = self
                    .cached_filled_qty
                    .get(&child_order.client_order_id())
                    .copied()
                    .unwrap_or(child_order.filled_qty());

                if parent_leaves_qty.is_zero() {
                    self.cancel_order(&child_order, Some(false));
                } else if child_filled_qty >= parent_leaves_qty {
                    // Child already filled beyond parent's remaining qty, cancel it
                    self.cancel_order(&child_order, Some(false));
                } else {
                    let child_leaves_qty = child_order.quantity().saturating_sub(child_filled_qty);
                    if child_leaves_qty != parent_leaves_qty {
                        let price = child_order.price();
                        let trigger_price = child_order.trigger_price();
                        self.update_order(
                            &mut child_order,
                            Some(parent_leaves_qty),
                            price,
                            trigger_price,
                            Some(false),
                        );
                    }
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

        // Check if rejection is due to post-only
        let due_post_only = reason.as_str().starts_with("POST_ONLY");

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
            due_post_only,
        ));
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
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

        // TODO: Remove when tests wire up ExecutionEngine to process events
        order
            .apply(event.clone())
            .expect("Failed to apply order event");

        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
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
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_order_cancel_rejected(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
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
            venue_order_id,
            Some(account_id),
        ));
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
    }

    fn generate_order_updated(
        &self,
        order: &mut OrderAny,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        protection_price: Option<Price>,
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
            protection_price,
        ));

        // TODO: Remove when tests wire up ExecutionEngine to process events
        order
            .apply(event.clone())
            .expect("Failed to apply order event");

        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
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
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
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
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
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
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
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
        debug_assert!(
            last_qty <= order.quantity(),
            "Fill quantity {last_qty} exceeds order quantity {order_qty} for {client_order_id}",
            order_qty = order.quantity(),
            client_order_id = order.client_order_id()
        );

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

        // TODO: Remove when tests wire up ExecutionEngine to process events
        order
            .apply(event.clone())
            .expect("Failed to apply order event");

        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
    }
}
