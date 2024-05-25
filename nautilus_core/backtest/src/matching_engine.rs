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

use std::collections::HashMap;

use log::{debug, info};
use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{nanos::UnixNanos, time::AtomicTime};
use nautilus_execution::matching_core::OrderMatchingCore;
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        delta::OrderBookDelta,
    },
    enums::{AccountType, BookType, MarketStatus, OmsType},
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        trader_id::TraderId, venue::Venue,
    },
    instruments::Instrument,
    orderbook::book::OrderBook,
    orders::{
        any::{PassiveOrderAny, StopOrderAny},
        trailing_stop_limit::TrailingStopLimitOrder,
        trailing_stop_market::TrailingStopMarketOrder,
    },
    types::price::Price,
};

pub struct OrderMatchingEngineConfig {
    pub bar_execution: bool,
    pub reject_stop_orders: bool,
    pub support_gtd_orders: bool,
    pub support_contingent_orders: bool,
    pub use_position_ids: bool,
    pub use_random_ids: bool,
    pub use_reduce_only: bool,
}

/// An order matching engine for a single market.
pub struct OrderMatchingEngine {
    /// The venue for the matching engine.
    pub venue: Venue,
    /// The instrument for the matching engine.
    pub instrument: Box<dyn Instrument>,
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
    msgbus: &'static MessageBus,
    cache: &'static Cache,
    book: OrderBook,
    core: OrderMatchingCore,
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

// TODO: we'll probably be changing the `FillModel` (don't add for now)
impl OrderMatchingEngine {
    /// Creates a new [`OrderMatchingEngine`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument: Box<dyn Instrument>,
        raw_id: u32,
        book_type: BookType,
        oms_type: OmsType,
        account_type: AccountType,
        clock: &'static AtomicTime,
        msgbus: &'static MessageBus,
        cache: &'static Cache,
        config: OrderMatchingEngineConfig,
    ) -> Self {
        let book = OrderBook::new(book_type, instrument.id());
        let core = OrderMatchingCore::new(
            instrument.id(),
            instrument.price_increment(),
            None, // TBD (will be a function on the engine)
            None, // TBD (will be a function on the engine)
            None, // TBD (will be a function on the engine)
        );
        Self {
            venue: instrument.venue(),
            instrument,
            raw_id,
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

        info!("Reset {}", self.instrument.id());
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
    pub fn get_book(&self) -> &OrderBook {
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

    // -- DATA PROCESSING -----------------------------------------------------

    /// Process the venues market for the given order book delta.
    pub fn process_order_book_delta(&mut self, delta: OrderBookDelta) {
        debug!("Processing {delta}");

        self.book.apply_delta(delta);
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

    fn expire_order(&mut self, order: &PassiveOrderAny) {
        todo!();
    }

    fn update_trailing_stop_market(&mut self, order: &TrailingStopMarketOrder) {
        todo!()
    }

    fn update_trailing_stop_limit(&mut self, order: &TrailingStopLimitOrder) {
        todo!()
    }
}
