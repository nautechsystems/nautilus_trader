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
    data::{
        Bar, BarType, InstrumentClose, OrderBookDelta, OrderBookDeltas, OrderBookDepth10,
        QuoteTick, TradeTick, order::BookOrder,
    },
    enums::{
        AccountType, AggregationSource, AggressorSide, BookAction, BookType, ContingencyType,
        InstrumentCloseType, LiquiditySide, MarketStatus, MarketStatusAction, OmsType, OrderSide,
        OrderSideSpecified, OrderStatus, OrderType, PositionSide, PriceType, TimeInForce,
        TriggerType,
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
    orders::{MarketOrder, Order, OrderAny, OrderCore},
    position::Position,
    types::{
        Currency, Money, Price, Quantity, fixed::FIXED_PRECISION, price::PriceRaw,
        quantity::QuantityRaw,
    },
};
use ustr::Ustr;

use crate::{
    matching_core::{MatchAction, OrderMatchInfo, OrderMatchingCore},
    matching_engine::{config::OrderMatchingEngineConfig, ids_generator::IdsGenerator},
    models::{
        fee::{FeeModel, FeeModelAny},
        fill::{FillModel, FillModelAny},
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
    fill_model: FillModelAny,
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
    queue_ahead: AHashMap<ClientOrderId, (PriceRaw, QuantityRaw)>,
    queue_excess: AHashMap<ClientOrderId, QuantityRaw>,
    queue_pending: AHashMap<ClientOrderId, PriceRaw>,
    prev_bid_price_raw: PriceRaw,
    prev_bid_size_raw: QuantityRaw,
    prev_ask_price_raw: PriceRaw,
    prev_ask_size_raw: QuantityRaw,
    tob_initialized: bool,
    instrument_close: Option<InstrumentClose>,
    settlement_price: Option<Price>,
    expiration_processed: bool,
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
        fill_model: FillModelAny,
        fee_model: FeeModelAny,
        book_type: BookType,
        oms_type: OmsType,
        account_type: AccountType,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: OrderMatchingEngineConfig,
    ) -> Self {
        let book = OrderBook::new(instrument.id(), book_type);
        let mut core = OrderMatchingCore::new(instrument.id(), instrument.price_increment());
        core.set_fill_limit_inside_spread(fill_model.fill_limit_inside_spread());
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
            queue_ahead: AHashMap::new(),
            queue_excess: AHashMap::new(),
            queue_pending: AHashMap::new(),
            prev_bid_price_raw: 0,
            prev_bid_size_raw: 0,
            prev_ask_price_raw: 0,
            prev_ask_size_raw: 0,
            tob_initialized: false,
            instrument_close: None,
            settlement_price: None,
            expiration_processed: false,
        }
    }

    /// Resets the matching engine to its initial state.
    ///
    /// Clears the order book, execution state, cached data, and resets all
    /// internal components. This is typically used for backtesting scenarios
    /// where the engine needs to be reset between test runs.
    pub fn reset(&mut self) {
        self.book.reset();
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
        self.queue_ahead.clear();
        self.queue_excess.clear();
        self.queue_pending.clear();
        self.prev_bid_price_raw = 0;
        self.prev_bid_size_raw = 0;
        self.prev_ask_price_raw = 0;
        self.prev_ask_size_raw = 0;
        self.tob_initialized = false;
        self.instrument_close = None;
        self.settlement_price = None;
        self.expiration_processed = false;
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

    fn seed_trade_consumption(
        &mut self,
        trade_price_raw: PriceRaw,
        trade_size_raw: QuantityRaw,
        trade_ts_event: UnixNanos,
        aggressor_side: AggressorSide,
    ) {
        if trade_size_raw == 0 {
            return;
        }

        // If the book was updated after the trade's event time, depth deltas
        // already reflect this trade's consumed volume, skip to avoid double-counting
        if self.book.ts_last > trade_ts_event {
            return;
        }

        let consumption = match aggressor_side {
            AggressorSide::Buyer => &mut self.ask_consumption,
            AggressorSide::Seller => &mut self.bid_consumption,
            AggressorSide::NoAggressor => return,
        };

        let levels: Vec<_> = match aggressor_side {
            AggressorSide::Buyer => self
                .book
                .asks(None)
                .take_while(|l| l.price.value.raw <= trade_price_raw)
                .collect(),
            AggressorSide::Seller => self
                .book
                .bids(None)
                .take_while(|l| l.price.value.raw >= trade_price_raw)
                .collect(),
            _ => unreachable!(),
        };

        let mut remaining = trade_size_raw;
        for level in &levels {
            if remaining == 0 {
                break;
            }
            let level_size = level.size_raw();
            let entry = consumption
                .entry(level.price.value.raw)
                .or_insert((level_size, 0));

            // Reconcile stale level size to prevent reset in apply_liquidity_consumption
            if entry.0 != level_size {
                entry.0 = level_size;
                entry.1 = 0;
            }

            let available = level_size.saturating_sub(entry.1);
            let consume = min(remaining, available);
            entry.1 += consume;
            remaining -= consume;
        }
    }

    /// Sets the fill model for the matching engine.
    pub fn set_fill_model(&mut self, fill_model: FillModelAny) {
        self.core
            .set_fill_limit_inside_spread(fill_model.fill_limit_inside_spread());
        self.fill_model = fill_model;
    }

    pub fn set_settlement_price(&mut self, price: Price) {
        self.settlement_price = Some(price);
    }

    fn snapshot_queue_position(&mut self, order: &OrderAny, price: Price) {
        if !self.config.queue_position {
            return;
        }
        let size_prec = self.instrument.size_precision();

        // Pass opposite side because get_quantity_at_level flips internally
        // (BUY reads asks, SELL reads bids). We want the resting side depth.
        let qty_ahead = self.book.get_quantity_at_level(
            price,
            OrderCore::opposite_side(order.order_side()),
            size_prec,
        );

        let client_order_id = order.client_order_id();

        // Clear stale entries from both maps (e.g. order modified to new price)
        self.queue_pending.remove(&client_order_id);
        self.queue_ahead.remove(&client_order_id);

        // For L1 books, levels behind the BBO have no visible depth. Track
        // these orders separately so fills are blocked until the BBO reaches
        // this price. Only truly behind-BBO prices are pending (BUY below
        // best bid / SELL above best ask); inside-spread and no-book keep 0.
        if self.book_type == BookType::L1_MBP && qty_ahead.raw == 0 {
            let behind_bbo = match order.order_side() {
                OrderSide::Buy => self.book.best_bid_price().is_some_and(|bid| price < bid),
                OrderSide::Sell => self.book.best_ask_price().is_some_and(|ask| price > ask),
                _ => false,
            };

            if behind_bbo {
                self.queue_pending.insert(client_order_id, price.raw);
                return;
            }
        }

        self.queue_ahead
            .insert(client_order_id, (price.raw, qty_ahead.raw));
    }

    fn decrement_queue_on_trade(
        &mut self,
        price_raw: PriceRaw,
        trade_size_raw: QuantityRaw,
        aggressor_side: AggressorSide,
    ) {
        if !self.config.queue_position {
            return;
        }

        self.queue_excess.clear();

        let keys: Vec<ClientOrderId> = self.queue_ahead.keys().copied().collect();
        let mut entries: Vec<(ClientOrderId, QuantityRaw, QuantityRaw)> = Vec::new();
        let mut stale: Vec<ClientOrderId> = Vec::new();

        for client_order_id in keys {
            let (order_price_raw, ahead_raw) = match self.queue_ahead.get(&client_order_id).copied()
            {
                Some(v) => v,
                None => continue,
            };

            let cache = self.cache.borrow();
            let order_info = cache.order(&client_order_id).and_then(|order| {
                if order.is_closed() {
                    None
                } else {
                    Some((order.order_side(), order.leaves_qty().raw))
                }
            });
            drop(cache);

            let Some((order_side, leaves_raw)) = order_info else {
                stale.push(client_order_id);
                continue;
            };

            if order_price_raw != price_raw || ahead_raw == 0 {
                continue;
            }

            let should_decrement = matches!(aggressor_side, AggressorSide::NoAggressor)
                || (aggressor_side == AggressorSide::Buyer && order_side == OrderSide::Sell)
                || (aggressor_side == AggressorSide::Seller && order_side == OrderSide::Buy);

            if should_decrement {
                entries.push((client_order_id, ahead_raw, leaves_raw));
            }
        }

        for id in stale {
            self.queue_ahead.remove(&id);
        }

        // Sort by queue position (earliest first) for shared budget allocation
        entries.sort_by_key(|&(_, ahead, _)| ahead);

        let mut remaining = trade_size_raw;
        let mut prev_position: QuantityRaw = 0;

        for (client_order_id, ahead_raw, leaves_raw) in &entries {
            if remaining == 0 {
                let new_ahead = ahead_raw.saturating_sub(trade_size_raw);
                self.queue_ahead
                    .insert(*client_order_id, (price_raw, new_ahead));
                if new_ahead == 0 {
                    // Queue cleared but no trade volume left for this order
                    self.queue_excess.insert(*client_order_id, 0);
                }
                continue;
            }

            // Consume the gap between previous position and this order's depth
            let gap = ahead_raw.saturating_sub(prev_position);
            let queue_consumed = remaining.min(gap);
            remaining -= queue_consumed;

            if remaining == 0 && queue_consumed < gap {
                let new_ahead = ahead_raw.saturating_sub(trade_size_raw);
                self.queue_ahead
                    .insert(*client_order_id, (price_raw, new_ahead));
                continue;
            }

            self.queue_ahead.insert(*client_order_id, (price_raw, 0));
            let excess = remaining.min(*leaves_raw);
            self.queue_excess.insert(*client_order_id, excess);
            remaining -= excess;
            prev_position = ahead_raw + excess;
        }
    }

    fn determine_trade_fill_qty(&self, order: &OrderAny) -> Option<QuantityRaw> {
        if !self.config.queue_position {
            return Some(order.leaves_qty().raw);
        }

        let client_order_id = order.client_order_id();

        // Block fills for L1 orders pending a deferred snapshot
        if self.queue_pending.contains_key(&client_order_id) {
            return None;
        }

        if let Some(&(tracked_price_raw, ahead_raw)) = self.queue_ahead.get(&client_order_id)
            && let Some(order_price) = order.price()
            && order_price.raw == tracked_price_raw
            && ahead_raw > 0
        {
            // Allow fill when a current trade has crossed through the order's price
            let crossed = self.last_trade_size.is_some()
                && self
                    .core
                    .last
                    .is_some_and(|trade_price| match order.order_side() {
                        OrderSide::Buy => trade_price.raw < order_price.raw,
                        OrderSide::Sell => trade_price.raw > order_price.raw,
                        _ => false,
                    });

            if !crossed {
                return None;
            }
        }

        let leaves_raw = order.leaves_qty().raw;
        if leaves_raw == 0 {
            return None;
        }

        let mut available_raw = leaves_raw;

        // Cap by remaining trade volume and queue excess (only during trade processing)
        if let Some(trade_size) = self.last_trade_size {
            let remaining = trade_size.raw.saturating_sub(self.trade_consumption);
            available_raw = available_raw.min(remaining);

            if let Some(&excess_raw) = self.queue_excess.get(&client_order_id) {
                if excess_raw == 0 {
                    return None;
                }
                available_raw = available_raw.min(excess_raw);
            }
        }

        if available_raw == 0 {
            return None;
        }

        Some(available_raw)
    }

    fn clear_all_queue_positions(&mut self) {
        for (_, (_, ahead_raw)) in &mut self.queue_ahead {
            *ahead_raw = 0;
        }
    }

    fn clear_queue_on_delete(&mut self, deleted_price_raw: PriceRaw, deleted_side: OrderSide) {
        let keys: Vec<ClientOrderId> = self.queue_ahead.keys().copied().collect();
        for client_order_id in keys {
            if let Some(&(order_price_raw, _)) = self.queue_ahead.get(&client_order_id)
                && order_price_raw == deleted_price_raw
            {
                let cache = self.cache.borrow();
                if let Some(order) = cache.order(&client_order_id)
                    && order.order_side() == deleted_side
                {
                    drop(cache);
                    self.queue_ahead
                        .insert(client_order_id, (order_price_raw, 0));
                }
            }
        }
    }

    fn cap_queue_ahead(
        &mut self,
        price_raw: PriceRaw,
        size_raw: QuantityRaw,
        order_side: OrderSide,
    ) {
        let keys: Vec<ClientOrderId> = self.queue_ahead.keys().copied().collect();
        let mut stale: Vec<ClientOrderId> = Vec::new();

        for client_order_id in keys {
            let (order_price_raw, ahead_raw) = match self.queue_ahead.get(&client_order_id).copied()
            {
                Some(v) => v,
                None => continue,
            };

            if order_price_raw != price_raw || ahead_raw <= size_raw {
                continue;
            }

            let cache = self.cache.borrow();
            let order_info = cache.order(&client_order_id).and_then(|order| {
                if order.is_closed() {
                    None
                } else {
                    Some(order.order_side())
                }
            });
            drop(cache);

            let Some(side) = order_info else {
                stale.push(client_order_id);
                continue;
            };

            if side != order_side {
                continue;
            }

            self.queue_ahead
                .insert(client_order_id, (order_price_raw, size_raw));
        }

        for id in stale {
            self.queue_ahead.remove(&id);
        }
    }

    fn seed_tob_baseline(&mut self) {
        let bid = self.book.best_bid_price();
        let ask = self.book.best_ask_price();
        self.prev_bid_price_raw = bid.map_or(0, |p| p.raw);
        self.prev_bid_size_raw = self.book.best_bid_size().map_or(0, |q| q.raw);
        self.prev_ask_price_raw = ask.map_or(0, |p| p.raw);
        self.prev_ask_size_raw = self.book.best_ask_size().map_or(0, |q| q.raw);
        self.tob_initialized = bid.is_some() || ask.is_some();
    }

    fn decrement_l1_queue_on_quote(
        &mut self,
        bid_price_raw: PriceRaw,
        bid_size_raw: QuantityRaw,
        ask_price_raw: PriceRaw,
        ask_size_raw: QuantityRaw,
    ) {
        if !self.config.queue_position {
            return;
        }

        // Price-move detection requires a valid prior TOB snapshot
        if self.tob_initialized {
            // BID side (BUY limit orders): handle price drops (crossed/snapshot)
            if bid_price_raw < self.prev_bid_price_raw {
                self.adjust_l1_queue_on_price_move(bid_price_raw, bid_size_raw, OrderSide::Buy);
            }

            // ASK side (SELL limit orders): handle price rises (crossed/snapshot)
            if ask_price_raw > self.prev_ask_price_raw {
                self.adjust_l1_queue_on_price_move(ask_price_raw, ask_size_raw, OrderSide::Sell);
            }
        }

        // Resolve pending snapshots when BBO reaches a tracked order's price
        self.resolve_pending_l1_snapshots(bid_price_raw, bid_size_raw, ask_price_raw, ask_size_raw);
    }

    fn adjust_l1_queue_on_price_move(
        &mut self,
        new_price_raw: PriceRaw,
        new_size_raw: QuantityRaw,
        order_side: OrderSide,
    ) {
        let keys: Vec<ClientOrderId> = self.queue_ahead.keys().copied().collect();
        let mut stale: Vec<ClientOrderId> = Vec::new();

        for client_order_id in keys {
            let Some(&(order_price_raw, ahead_raw)) = self.queue_ahead.get(&client_order_id) else {
                continue;
            };

            let cache = self.cache.borrow();
            let order_info = cache.order(&client_order_id).and_then(|order| {
                if order.is_closed() {
                    None
                } else {
                    Some(order.order_side())
                }
            });
            drop(cache);

            let Some(side) = order_info else {
                stale.push(client_order_id);
                continue;
            };

            if side != order_side {
                continue;
            }

            // BUY orders crossed when bid drops below order price
            // SELL orders crossed when ask rises above order price
            let crossed = match order_side {
                OrderSide::Buy => order_price_raw > new_price_raw,
                _ => order_price_raw < new_price_raw,
            };

            if crossed {
                self.queue_ahead
                    .insert(client_order_id, (order_price_raw, 0));
            } else if order_price_raw == new_price_raw && ahead_raw > new_size_raw {
                self.queue_ahead
                    .insert(client_order_id, (order_price_raw, new_size_raw));
            }
        }

        for id in stale {
            self.queue_ahead.remove(&id);
        }

        // Also resolve pending L1 orders affected by this price move
        let pending_keys: Vec<ClientOrderId> = self.queue_pending.keys().copied().collect();
        let mut pending_stale: Vec<ClientOrderId> = Vec::new();

        for client_order_id in pending_keys {
            let Some(&order_price_raw) = self.queue_pending.get(&client_order_id) else {
                continue;
            };

            let cache = self.cache.borrow();
            let order_info = cache.order(&client_order_id).and_then(|order| {
                if order.is_closed() {
                    None
                } else {
                    Some(order.order_side())
                }
            });
            drop(cache);

            let Some(side) = order_info else {
                pending_stale.push(client_order_id);
                continue;
            };

            if side != order_side {
                continue;
            }

            let crossed = match order_side {
                OrderSide::Buy => order_price_raw > new_price_raw,
                _ => order_price_raw < new_price_raw,
            };

            if crossed {
                self.queue_pending.remove(&client_order_id);
                self.queue_ahead
                    .insert(client_order_id, (order_price_raw, 0));
            } else if order_price_raw == new_price_raw {
                self.queue_pending.remove(&client_order_id);
                self.queue_ahead
                    .insert(client_order_id, (order_price_raw, new_size_raw));
            }
        }

        for id in pending_stale {
            self.queue_pending.remove(&id);
        }
    }

    fn resolve_pending_l1_snapshots(
        &mut self,
        bid_price_raw: PriceRaw,
        bid_size_raw: QuantityRaw,
        ask_price_raw: PriceRaw,
        ask_size_raw: QuantityRaw,
    ) {
        let keys: Vec<ClientOrderId> = self.queue_pending.keys().copied().collect();
        let mut stale: Vec<ClientOrderId> = Vec::new();

        for client_order_id in keys {
            let Some(&order_price_raw) = self.queue_pending.get(&client_order_id) else {
                continue;
            };

            let cache = self.cache.borrow();
            let order_info = cache.order(&client_order_id).and_then(|order| {
                if order.is_closed() {
                    None
                } else {
                    Some(order.order_side())
                }
            });
            drop(cache);

            let Some(side) = order_info else {
                stale.push(client_order_id);
                continue;
            };

            // Initialize snapshot when BBO reaches the order's price level
            let matched_size = match side {
                OrderSide::Buy if order_price_raw == bid_price_raw => Some(bid_size_raw),
                OrderSide::Sell if order_price_raw == ask_price_raw => Some(ask_size_raw),
                _ => None,
            };

            if let Some(size) = matched_size {
                self.queue_pending.remove(&client_order_id);
                self.queue_ahead
                    .insert(client_order_id, (order_price_raw, size));
            }
        }

        for id in stale {
            self.queue_pending.remove(&id);
        }
    }

    fn resolve_pending_on_trade(&mut self, trade_price_raw: PriceRaw) {
        let keys: Vec<ClientOrderId> = self.queue_pending.keys().copied().collect();
        let mut stale: Vec<ClientOrderId> = Vec::new();

        for client_order_id in keys {
            let Some(&order_price_raw) = self.queue_pending.get(&client_order_id) else {
                continue;
            };

            let cache = self.cache.borrow();
            let order_side = cache.order(&client_order_id).and_then(|order| {
                if order.is_closed() {
                    None
                } else {
                    Some(order.order_side())
                }
            });
            drop(cache);

            let Some(side) = order_side else {
                stale.push(client_order_id);
                continue;
            };

            // Trade through a pending level proves the queue was crossed
            let crossed = match side {
                OrderSide::Buy => trade_price_raw < order_price_raw,
                OrderSide::Sell => trade_price_raw > order_price_raw,
                _ => false,
            };

            if crossed {
                self.queue_pending.remove(&client_order_id);
                self.queue_ahead
                    .insert(client_order_id, (order_price_raw, 0));
            }
        }

        for id in stale {
            self.queue_pending.remove(&id);
        }
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

    pub fn set_fill_at_market(&mut self, value: bool) {
        self.fill_at_market = value;
    }

    fn check_price_precision(&self, actual: u8, field: &str) -> anyhow::Result<()> {
        let expected = self.instrument.price_precision();
        if actual != expected {
            anyhow::bail!(
                "Invalid {field} precision {actual}, expected {expected} for {}",
                self.instrument.id()
            );
        }
        Ok(())
    }

    fn check_size_precision(&self, actual: u8, field: &str) -> anyhow::Result<()> {
        let expected = self.instrument.size_precision();
        if actual != expected {
            anyhow::bail!(
                "Invalid {field} precision {actual}, expected {expected} for {}",
                self.instrument.id()
            );
        }
        Ok(())
    }

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
            self.check_price_precision(delta.order.price.precision, "delta order price")?;
            self.check_size_precision(delta.order.size.precision, "delta order size")?;
        }

        // L1 books are driven by top-of-book data only, ignore deltas
        if self.book_type == BookType::L1_MBP {
            self.iterate(delta.ts_init, AggressorSide::NoAggressor);
            return Ok(());
        }

        self.book.apply_delta(delta)?;

        let delta_snapshot_or_clear = (delta.flags & 32) != 0 || delta.action == BookAction::Clear;

        if self.config.queue_position {
            if delta_snapshot_or_clear {
                self.clear_all_queue_positions();
            } else if delta.action == BookAction::Delete {
                self.clear_queue_on_delete(delta.order.price.raw, delta.order.side);
            } else if delta.action == BookAction::Update {
                self.cap_queue_ahead(
                    delta.order.price.raw,
                    delta.order.size.raw,
                    delta.order.side,
                );
            }
        }

        if self.config.queue_position && delta_snapshot_or_clear {
            self.seed_tob_baseline();
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
        for delta in &deltas.deltas {
            if matches!(delta.action, BookAction::Add | BookAction::Update) {
                self.check_price_precision(delta.order.price.precision, "delta order price")?;
                self.check_size_precision(delta.order.size.precision, "delta order size")?;
            }
        }

        // L1 books are driven by top-of-book data only, ignore deltas
        if self.book_type == BookType::L1_MBP {
            self.iterate(deltas.ts_init, AggressorSide::NoAggressor);
            return Ok(());
        }

        self.book.apply_deltas(deltas)?;

        let mut has_snapshot_or_clear = false;

        if self.config.queue_position {
            for delta in &deltas.deltas {
                if (delta.flags & 32) != 0 || delta.action == BookAction::Clear {
                    self.clear_all_queue_positions();
                    has_snapshot_or_clear = true;
                    break;
                } else if delta.action == BookAction::Delete {
                    self.clear_queue_on_delete(delta.order.price.raw, delta.order.side);
                } else if delta.action == BookAction::Update {
                    self.cap_queue_ahead(
                        delta.order.price.raw,
                        delta.order.size.raw,
                        delta.order.side,
                    );
                }
            }
        }

        if self.config.queue_position && has_snapshot_or_clear {
            self.seed_tob_baseline();
        }

        self.iterate(deltas.ts_init, AggressorSide::NoAggressor);
        Ok(())
    }

    /// Process the venues market for the given order book depth10.
    ///
    /// # Errors
    ///
    /// - If any bid/ask price precision does not match the instrument.
    /// - If any bid/ask size precision does not match the instrument.
    /// - If applying the depth to the book fails.
    /// - If updating the L1 order book with the top-of-book quote fails.
    pub fn process_order_book_depth10(&mut self, depth: &OrderBookDepth10) -> anyhow::Result<()> {
        log::debug!("Processing OrderBookDepth10 for {}", depth.instrument_id);

        // Validate precision for non-padding entries
        for order in &depth.bids {
            if order.side == OrderSide::NoOrderSide || !order.size.is_positive() {
                continue;
            }
            self.check_price_precision(order.price.precision, "bid price")?;
            self.check_size_precision(order.size.precision, "bid size")?;
        }
        for order in &depth.asks {
            if order.side == OrderSide::NoOrderSide || !order.size.is_positive() {
                continue;
            }
            self.check_price_precision(order.price.precision, "ask price")?;
            self.check_size_precision(order.size.precision, "ask size")?;
        }

        // For L1 books, only apply top-of-book to avoid mispricing
        // against worst-level entries when full depth is applied
        if self.book_type == BookType::L1_MBP {
            let quote = QuoteTick::new(
                depth.instrument_id,
                depth.bids[0].price,
                depth.asks[0].price,
                depth.bids[0].size,
                depth.asks[0].size,
                depth.ts_event,
                depth.ts_init,
            );
            self.book.update_quote_tick(&quote)?;
        } else {
            self.book.apply_depth(depth)?;
        }

        // Depth10 always replaces the full book via apply_depth regardless of flags
        if self.config.queue_position {
            self.clear_all_queue_positions();
            let bid_price_raw = depth.bids[0].price.raw;
            let bid_size_raw = depth.bids[0].size.raw;
            let ask_price_raw = depth.asks[0].price.raw;
            let ask_size_raw = depth.asks[0].size.raw;

            // Handle crossed/matched pending orders (same as quote path)
            if self.tob_initialized {
                if bid_price_raw < self.prev_bid_price_raw {
                    self.adjust_l1_queue_on_price_move(bid_price_raw, bid_size_raw, OrderSide::Buy);
                }

                if ask_price_raw > self.prev_ask_price_raw {
                    self.adjust_l1_queue_on_price_move(
                        ask_price_raw,
                        ask_size_raw,
                        OrderSide::Sell,
                    );
                }
            }

            self.resolve_pending_l1_snapshots(
                bid_price_raw,
                bid_size_raw,
                ask_price_raw,
                ask_size_raw,
            );

            self.prev_bid_price_raw = bid_price_raw;
            self.prev_bid_size_raw = bid_size_raw;
            self.prev_ask_price_raw = ask_price_raw;
            self.prev_ask_size_raw = ask_size_raw;
            self.tob_initialized = true;
        }

        self.iterate(depth.ts_init, AggressorSide::NoAggressor);
        Ok(())
    }

    /// Processes a quote tick to update the market state.
    ///
    /// # Panics
    ///
    /// - If updating the order book with the quote tick fails.
    /// - If bid/ask price precision does not match the instrument.
    /// - If bid/ask size precision does not match the instrument.
    pub fn process_quote_tick(&mut self, quote: &QuoteTick) {
        log::debug!("Processing {quote}");

        self.check_price_precision(quote.bid_price.precision, "bid_price")
            .unwrap();
        self.check_price_precision(quote.ask_price.precision, "ask_price")
            .unwrap();
        self.check_size_precision(quote.bid_size.precision, "bid_size")
            .unwrap();
        self.check_size_precision(quote.ask_size.precision, "ask_size")
            .unwrap();

        if self.book_type == BookType::L1_MBP {
            if self.config.queue_position {
                self.decrement_l1_queue_on_quote(
                    quote.bid_price.raw,
                    quote.bid_size.raw,
                    quote.ask_price.raw,
                    quote.ask_size.raw,
                );
                self.prev_bid_price_raw = quote.bid_price.raw;
                self.prev_bid_size_raw = quote.bid_size.raw;
                self.prev_ask_price_raw = quote.ask_price.raw;
                self.prev_ask_size_raw = quote.ask_size.raw;
                self.tob_initialized = true;
            }
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

        self.check_price_precision(bar.open.precision, "bar open")
            .unwrap();
        self.check_price_precision(bar.high.precision, "bar high")
            .unwrap();
        self.check_price_precision(bar.low.precision, "bar low")
            .unwrap();
        self.check_price_precision(bar.close.precision, "bar close")
            .unwrap();
        self.check_size_precision(bar.volume.precision, "bar volume")
            .unwrap();

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

        // Determine high/low processing order.
        // Default: O→H→L→C. With adaptive ordering, swap if low is closer to open.
        let high_first = !self.config.bar_adaptive_high_low_ordering
            || (bar.high.raw - bar.open.raw).abs() < (bar.low.raw - bar.open.raw).abs();

        if high_first {
            self.process_bar_high(&mut trade_tick, bar);
            self.process_bar_low(&mut trade_tick, bar);
        } else {
            self.process_bar_low(&mut trade_tick, bar);
            self.process_bar_high(&mut trade_tick, bar);
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

    fn process_bar_high(&mut self, trade_tick: &mut TradeTick, bar: &Bar) {
        if self.core.last.is_some_and(|last| bar.high > last) {
            self.fill_at_market = false;
            trade_tick.price = bar.high;
            trade_tick.aggressor_side = AggressorSide::Buyer;
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

            self.book.update_trade_tick(trade_tick).unwrap();
            self.iterate(trade_tick.ts_init, AggressorSide::NoAggressor);

            self.core.set_last_raw(trade_tick.price);
        }
    }

    fn process_bar_low(&mut self, trade_tick: &mut TradeTick, bar: &Bar) {
        if self.core.last.is_some_and(|last| bar.low < last) {
            self.fill_at_market = false;
            trade_tick.price = bar.low;
            trade_tick.aggressor_side = AggressorSide::Seller;
            trade_tick.trade_id = self.ids_generator.generate_trade_id();

            self.book.update_trade_tick(trade_tick).unwrap();
            self.iterate(trade_tick.ts_init, AggressorSide::NoAggressor);

            self.core.set_last_raw(trade_tick.price);
        }
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
    /// For L1 books, always updates the order book with the trade tick to maintain
    /// market state. When `trade_execution` is disabled, order matching and maintenance
    /// operations (GTD order expiry, trailing stop activation, instrument expiration)
    /// are skipped. These maintenance operations will run on the next quote tick or bar.
    ///
    /// # Panics
    ///
    /// - If updating the order book with the trade tick fails.
    /// - If trade price precision does not match the instrument.
    /// - If trade size precision does not match the instrument.
    pub fn process_trade_tick(&mut self, trade: &TradeTick) {
        log::debug!("Processing {trade}");

        self.check_price_precision(trade.price.precision, "trade price")
            .unwrap();
        self.check_size_precision(trade.size.precision, "trade size")
            .unwrap();

        let price_raw = trade.price.raw;

        if self.book_type == BookType::L1_MBP {
            self.book.update_trade_tick(trade).unwrap();
        }

        self.core.set_last_raw(trade.price);

        if !self.config.trade_execution {
            // Sync core to L1 book, skip order matching
            if self.book_type == BookType::L1_MBP {
                if let Some(bid) = self.book.best_bid_price() {
                    self.core.set_bid_raw(bid);
                }

                if let Some(ask) = self.book.best_ask_price() {
                    self.core.set_ask_raw(ask);
                }
            }
            return;
        }

        let aggressor_side = trade.aggressor_side;

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

        let original_bid = self.core.bid;
        let original_ask = self.core.ask;

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
            AggressorSide::NoAggressor => {
                // Force both sides to trade price (parity with Cython)
                self.core.set_bid_raw(trade.price);
                self.core.set_ask_raw(trade.price);
            }
        }

        self.last_trade_size = Some(trade.size);
        self.trade_consumption = 0;

        if self.config.liquidity_consumption && self.book_type != BookType::L1_MBP {
            self.seed_trade_consumption(price_raw, trade.size.raw, trade.ts_event, aggressor_side);
        }

        self.resolve_pending_on_trade(price_raw);
        self.decrement_queue_on_trade(price_raw, trade.size.raw, aggressor_side);

        self.iterate(trade.ts_init, aggressor_side);

        self.last_trade_size = None;
        self.trade_consumption = 0;

        // Restore original bid/ask after temporary trade price override
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

    /// Processes an instrument close event.
    ///
    /// For `ContractExpired` close types, stores the close and triggers expiration
    /// processing which cancels all open orders and closes all open positions.
    pub fn process_instrument_close(&mut self, close: InstrumentClose) {
        if close.instrument_id != self.instrument.id() {
            log::warn!(
                "Received instrument close for unknown instrument_id: {}",
                close.instrument_id
            );
            return;
        }

        if close.close_type == InstrumentCloseType::ContractExpired {
            self.instrument_close = Some(close);
            self.iterate(close.ts_init, AggressorSide::NoAggressor);
        }
    }

    fn check_instrument_expiration(&mut self) {
        if self.expiration_processed || self.instrument_close.is_none() {
            return;
        }

        self.expiration_processed = true;
        let close = self.instrument_close.take().unwrap();
        log::info!("{} reached expiration", self.instrument.id());

        let open_orders: Vec<OrderMatchInfo> = self.get_open_orders();
        for order_info in &open_orders {
            let order = {
                let cache = self.cache.borrow();
                cache.order(&order_info.client_order_id).cloned()
            };

            if let Some(order) = order {
                self.cancel_order(&order, None);
            }
        }

        let instrument_id = self.instrument.id();
        let positions: Vec<(TraderId, StrategyId, PositionId, OrderSide, Quantity)> = {
            let cache = self.cache.borrow();
            cache
                .positions_open(None, Some(&instrument_id), None, None, None)
                .into_iter()
                .map(|pos| {
                    let closing_side = match pos.side {
                        PositionSide::Long => OrderSide::Sell,
                        PositionSide::Short => OrderSide::Buy,
                        _ => OrderSide::NoOrderSide,
                    };
                    (
                        pos.trader_id,
                        pos.strategy_id,
                        pos.id,
                        closing_side,
                        pos.quantity,
                    )
                })
                .collect()
        };

        let ts_now = self.clock.borrow().timestamp_ns();

        for (trader_id, strategy_id, position_id, closing_side, quantity) in positions {
            let client_order_id =
                ClientOrderId::from(format!("EXPIRATION-{}-{}", self.venue, UUID4::new()).as_str());
            let mut order = OrderAny::Market(MarketOrder::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                closing_side,
                quantity,
                TimeInForce::Gtc,
                UUID4::new(),
                ts_now,
                true, // reduce_only
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(vec![Ustr::from(&format!(
                    "EXPIRATION_{}_CLOSE",
                    self.venue
                ))]),
            ));

            if self
                .cache
                .borrow_mut()
                .add_order(order.clone(), Some(position_id), None, false)
                .is_err()
            {
                log::debug!("Expiration order already in cache: {client_order_id}");
            }

            let venue_order_id = self.ids_generator.get_venue_order_id(&order).unwrap();
            self.generate_order_accepted(&mut order, venue_order_id);
            let fill_price = self.settlement_price.unwrap_or(close.close_price);
            self.apply_fills(
                &mut order,
                vec![(fill_price, quantity)],
                LiquiditySide::Taker,
                Some(position_id),
                None,
            );
        }
    }

    /// Processes a new order submission.
    ///
    /// Validates the order against instrument precision, expiration, and contingency
    /// rules before accepting or rejecting it.
    ///
    /// # Panics
    ///
    /// Panics if an OTO child order references a missing or non-OTO parent.
    #[allow(clippy::needless_return)]
    pub fn process_order(&mut self, order: &mut OrderAny, account_id: AccountId) {
        // Validate inside a cache borrow scope, collecting any rejection
        // reason rather than emitting events while the borrow is held.
        // This avoids RefCell re-entrancy panics from synchronous event
        // dispatch that calls back into the execution engine.
        let reject_reason: Option<Ustr> = 'validate: {
            let cache_borrow = self.cache.as_ref().borrow();

            if self.core.order_exists(order.client_order_id()) {
                break 'validate Some("Order already exists".into());
            }

            // Index identifiers
            self.account_ids.insert(order.trader_id(), account_id);

            // Check for instrument expiration or activation
            if self.instrument.has_expiration() {
                if let Some(activation_ns) = self.instrument.activation_ns()
                    && self.clock.borrow().timestamp_ns() < activation_ns
                {
                    break 'validate Some(
                        format!(
                            "Contract {} is not yet active, activation {activation_ns}",
                            self.instrument.id(),
                        )
                        .into(),
                    );
                }

                if let Some(expiration_ns) = self.instrument.expiration_ns()
                    && self.clock.borrow().timestamp_ns() >= expiration_ns
                {
                    break 'validate Some(
                        format!(
                            "Contract {} has expired, expiration {expiration_ns}",
                            self.instrument.id(),
                        )
                        .into(),
                    );
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
                        if parent_order.status() == OrderStatus::Rejected && order.is_open() {
                            break 'validate Some(
                                format!("Rejected OTO order from {parent_order_id}").into(),
                            );
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
                                break 'validate Some(
                                    format!("Contingent order {client_order_id} already closed")
                                        .into(),
                                );
                            }
                            None => panic!("Cannot find contingent order for {client_order_id}"),
                            _ => {}
                        }
                    }
                }
            }

            // Check for valid order quantity precision
            if order.quantity().precision != self.instrument.size_precision() {
                break 'validate Some(
                    format!(
                        "Invalid order quantity precision for order {}, was {} when {} size precision is {}",
                        order.client_order_id(),
                        order.quantity().precision,
                        self.instrument.id(),
                        self.instrument.size_precision()
                    )
                    .into(),
                );
            }

            // Check for valid order price precision
            if let Some(price) = order.price()
                && price.precision != self.instrument.price_precision()
            {
                break 'validate Some(
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

            // Check for valid order trigger price precision
            if let Some(trigger_price) = order.trigger_price()
                && trigger_price.precision != self.instrument.price_precision()
            {
                break 'validate Some(
                    format!(
                        "Invalid order trigger price precision for order {}, was {} when {} price precision is {}",
                        order.client_order_id(),
                        trigger_price.precision,
                        self.instrument.id(),
                        self.instrument.price_precision()
                    )
                    .into(),
                );
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
                break 'validate Some(
                    format!(
                        "Short selling not permitted on a CASH account with position {position_string} and order {order}",
                    )
                    .into(),
                );
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
                break 'validate Some(
                    format!(
                        "Reduce-only order {} ({}-{}) would have increased position",
                        order.client_order_id(),
                        order.order_type().to_string().to_uppercase(),
                        order.order_side().to_string().to_uppercase()
                    )
                    .into(),
                );
            }

            None
        };

        if let Some(reason) = reject_reason {
            self.generate_order_rejected(order, reason);
            return;
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

            if self.config.queue_position
                && let Some(new_price) = order.price()
            {
                self.snapshot_queue_position(&order, new_price);
                self.queue_excess.remove(&order.client_order_id());
            }
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

            // If fill didn't execute (e.g. all liquidity consumed), revert to
            // maker so the fill model check applies on subsequent iterations
            if self.core.order_exists(order.client_order_id())
                && let Some(cached) = self.cache.borrow_mut().mut_order(&order.client_order_id())
            {
                cached.set_liquidity_side(LiquiditySide::Maker);
            }
        } else if matches!(order.time_in_force(), TimeInForce::Fok | TimeInForce::Ioc) {
            self.cancel_order(order, None);
        } else {
            // Add passive order to cache for later modify/cancel operations
            order.set_liquidity_side(LiquiditySide::Maker);

            if let Some(price) = order.price() {
                self.snapshot_queue_position(order, price);
            }

            let add_result = self
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false);

            if let Err(e) = add_result {
                log::debug!("Failed to add order to cache: {e}");

                // Persist Maker side on the cached copy when exec engine
                // already cached the order (only if not already Maker/Taker)
                if let Some(cached) = self.cache.borrow_mut().mut_order(&order.client_order_id())
                    && !matches!(
                        cached.liquidity_side(),
                        Some(LiquiditySide::Maker | LiquiditySide::Taker)
                    )
                {
                    cached.set_liquidity_side(LiquiditySide::Maker);
                }
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

    /// Iterate the matching engine by processing the bid and ask order sides
    /// and advancing time up to the given UNIX `timestamp_ns`.
    ///
    /// The `aggressor_side` parameter is used for trade execution processing.
    /// When not `NoAggressor`, the book-based bid/ask reset is skipped to preserve
    /// transient trade price overrides.
    pub fn iterate(&mut self, timestamp_ns: UnixNanos, aggressor_side: AggressorSide) {
        // TODO implement correct clock fixed time setting self.clock.set_time(ts_now);

        // Only reset bid/ask from book when not processing trade execution
        // (preserves transient trade price override for L2/L3 books)
        if aggressor_side == AggressorSide::NoAggressor {
            if let Some(bid) = self.book.best_bid_price() {
                self.core.set_bid_raw(bid);
            }

            if let Some(ask) = self.book.best_ask_price() {
                self.core.set_ask_raw(ask);
            }
        }

        // Process expiration before matching to prevent fills on expired instruments
        self.check_instrument_expiration();

        // Process bid actions before snapshotting asks so cross-side
        // contingencies (OCO/OUO) mutate state between sides
        for action in self.core.iterate_bids() {
            match action {
                MatchAction::FillLimit(id) => self.fill_limit_order(id),
                MatchAction::TriggerStop(id) => self.trigger_stop_order(id),
            }
        }
        for action in self.core.iterate_asks() {
            match action {
                MatchAction::FillLimit(id) => self.fill_limit_order(id),
                MatchAction::TriggerStop(id) => self.trigger_stop_order(id),
            }
        }

        let orders_bid = self.core.get_orders_bid().to_vec();
        let orders_ask = self.core.get_orders_ask().to_vec();

        self.iterate_orders(timestamp_ns, &orders_bid);
        self.iterate_orders(timestamp_ns, &orders_ask);

        // Restore core bid/ask to book values after order iteration
        // (during trade execution, transient override was used for matching)
        self.core.bid = self.book.best_bid_price();
        self.core.ask = self.book.best_ask_price();
    }

    fn get_trailing_activation_price(
        &self,
        trigger_type: TriggerType,
        order_side: OrderSide,
        bid: Option<Price>,
        ask: Option<Price>,
        last: Option<Price>,
    ) -> Option<Price> {
        match trigger_type {
            TriggerType::LastPrice => last,
            TriggerType::LastOrBidAsk => last.or(match order_side {
                OrderSide::Buy => ask,
                OrderSide::Sell => bid,
                _ => None,
            }),
            // Default, BidAsk, DoubleBidAsk, DoubleLastPrice, IndexPrice, MarkPrice
            _ => match order_side {
                OrderSide::Buy => ask,
                OrderSide::Sell => bid,
                _ => None,
            },
        }
    }

    fn maybe_activate_trailing_stop(
        &mut self,
        order: &mut OrderAny,
        bid: Option<Price>,
        ask: Option<Price>,
        last: Option<Price>,
    ) -> bool {
        match order {
            OrderAny::TrailingStopMarket(inner) => {
                if inner.is_activated {
                    return true;
                }

                if inner.activation_price.is_none() {
                    let px = self.get_trailing_activation_price(
                        inner.trigger_type,
                        inner.order_side(),
                        bid,
                        ask,
                        last,
                    );

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
                    let px = self.get_trailing_activation_price(
                        inner.trigger_type,
                        inner.order_side(),
                        bid,
                        ask,
                        last,
                    );

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

                if !self.maybe_activate_trailing_stop(
                    &mut any,
                    self.core.bid,
                    self.core.ask,
                    self.core.last,
                ) {
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

        fills
    }

    fn determine_market_fill_model_price_and_volume(
        &mut self,
        order: &OrderAny,
    ) -> (Vec<(Price, Quantity)>, bool) {
        if let (Some(best_bid), Some(best_ask)) = (self.core.bid, self.core.ask)
            && let Some(book) = self.fill_model.get_orderbook_for_fill_simulation(
                &self.instrument,
                order,
                best_bid,
                best_ask,
            )
        {
            let price = match order.order_side().as_specified() {
                OrderSideSpecified::Buy => Price::max(FIXED_PRECISION),
                OrderSideSpecified::Sell => Price::min(FIXED_PRECISION),
            };
            let book_order = BookOrder::new(order.order_side(), price, order.quantity(), 0);
            let fills = book.simulate_fills(&book_order);
            if !fills.is_empty() {
                return (fills, true);
            }
        }
        (self.determine_market_price_and_volume(order), false)
    }

    fn determine_limit_fill_model_price_and_volume(
        &mut self,
        order: &OrderAny,
    ) -> Vec<(Price, Quantity)> {
        if let (Some(best_bid), Some(best_ask)) = (self.core.bid, self.core.ask)
            && let Some(book) = self.fill_model.get_orderbook_for_fill_simulation(
                &self.instrument,
                order,
                best_bid,
                best_ask,
            )
            && let Some(limit_price) = order.price()
        {
            let book_order = BookOrder::new(order.order_side(), limit_price, order.quantity(), 0);
            let fills = book.simulate_fills(&book_order);
            if !fills.is_empty() {
                return fills;
            }
        }
        self.determine_limit_price_and_volume(order)
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

        order.set_liquidity_side(LiquiditySide::Taker);
        let (mut fills, from_synthetic) = self.determine_market_fill_model_price_and_volume(&order);

        // Apply protection price filtering at fill time (trigger-time semantics for stops)
        if let Some(protection_points) = self.config.price_protection_points
            && matches!(
                order.order_type(),
                OrderType::Market | OrderType::StopMarket
            )
            && let Ok(protection_price) = protection_price_calculate(
                self.instrument.price_increment(),
                &order,
                protection_points,
                self.core.bid,
                self.core.ask,
            )
        {
            fills = self.filter_fills_by_protection(fills, &order, protection_price);
        }

        // Skip consumption for synthetic fill-model books (prices may not exist
        // in the real book) and trigger price fills (gap price may not exist)
        let is_trigger_price_fill = !self.fill_at_market
            && self.book_type == BookType::L1_MBP
            && matches!(
                order.order_type(),
                OrderType::StopMarket | OrderType::TrailingStopMarket | OrderType::MarketIfTouched
            )
            && order.trigger_price().is_some();

        if !from_synthetic && !is_trigger_price_fill {
            fills = self.apply_liquidity_consumption(
                fills,
                order.order_side(),
                order.leaves_qty(),
                None,
            );
        }

        self.apply_fills(&mut order, fills, LiquiditySide::Taker, None, position);
    }

    fn filter_fills_by_protection(
        &self,
        fills: Vec<(Price, Quantity)>,
        order: &OrderAny,
        protection_price: Price,
    ) -> Vec<(Price, Quantity)> {
        let protection_raw = protection_price.raw;
        fills
            .into_iter()
            .filter(|(fill_price, _)| {
                match order.order_side() {
                    // BUY: only fill at prices <= protection_price
                    OrderSide::Buy => fill_price.raw <= protection_raw,
                    // SELL: only fill at prices >= protection_price
                    OrderSide::Sell => fill_price.raw >= protection_raw,
                    OrderSide::NoOrderSide => false,
                }
            })
            .collect()
    }

    /// Attempts to fill a limit order against the current order book.
    ///
    /// Determines fill prices and quantities based on available liquidity,
    /// then applies the fills to the order.
    ///
    /// # Panics
    ///
    /// Panics if the order has no price (design error).
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

                let queue_allowed_raw = if self.config.queue_position {
                    match self.determine_trade_fill_qty(&order) {
                        None | Some(0) => {
                            if matches!(order.time_in_force(), TimeInForce::Fok | TimeInForce::Ioc)
                            {
                                self.cancel_order(&order, None);
                            }
                            return;
                        }
                        Some(allowed) => Some(allowed),
                    }
                } else {
                    None
                };

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

                let tc_before = self.trade_consumption;
                let mut fills = self.determine_limit_fill_model_price_and_volume(&order);

                if let Some(allowed_raw) = queue_allowed_raw {
                    let size_prec = self.instrument.size_precision();
                    let mut remaining = allowed_raw;
                    fills = fills
                        .into_iter()
                        .filter_map(|(price, qty)| {
                            if remaining == 0 {
                                return None;
                            }
                            let capped = qty.raw.min(remaining);
                            remaining -= capped;
                            Some((price, Quantity::from_raw(capped, size_prec)))
                        })
                        .collect();

                    // Consume excess and reconcile trade budget after capping
                    let consumed: QuantityRaw = fills.iter().map(|(_, qty)| qty.raw).sum();

                    if let Some(excess) = self.queue_excess.get_mut(&order.client_order_id()) {
                        *excess = excess.saturating_sub(consumed);
                    }
                    self.trade_consumption = tc_before + consumed;
                }

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
        self.check_size_precision(last_qty.precision, "fill quantity")
            .unwrap();

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

            // Re-read from cache to get the order with events applied
            let client_order_id = order.client_order_id();
            if let Some(cached) = self.cache.borrow_mut().mut_order(&client_order_id) {
                cached.set_liquidity_side(LiquiditySide::Taker);
            }
            self.fill_limit_order(client_order_id);
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

        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
    }
}
