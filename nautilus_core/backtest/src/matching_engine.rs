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

#![allow(dead_code)] // Under development

use std::collections::HashMap;

use nautilus_common::msgbus::MessageBus;
use nautilus_core::time::AtomicTime;
use nautilus_execution::matching_core::OrderMatchingCore;
use nautilus_model::{
    data::bar::Bar,
    enums::{AccountType, BookType, MarketStatus, OmsType},
    identifiers::{account_id::AccountId, trader_id::TraderId, venue::Venue},
    instruments::Instrument,
    orderbook::{book_mbo::OrderBookMbo, book_mbp::OrderBookMbp},
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

pub struct OrderMatchingEngine {
    pub venue: Venue,
    pub instrument: Box<dyn Instrument>,
    pub raw_id: u64,
    pub book_type: BookType,
    pub oms_type: OmsType,
    pub account_type: AccountType,
    pub market_status: MarketStatus,
    pub config: OrderMatchingEngineConfig,
    // pub cache: Cache  // TODO
    clock: &'static AtomicTime,
    msgbus: &'static MessageBus,
    book_mbo: Option<OrderBookMbo>,
    book_mbp: Option<OrderBookMbp>,
    account_ids: HashMap<TraderId, AccountId>,
    core: OrderMatchingCore,
    target_bid: Option<Price>,
    target_ask: Option<Price>,
    target_last: Option<Price>,
    last_bar_bid: Option<Bar>,
    last_bar_ask: Option<Bar>,
    position_count: usize,
    order_count: usize,
    execution_count: usize,
}
