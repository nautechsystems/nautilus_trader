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

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::mpsc::Receiver,
};

use nautilus_core::uuid::UUID4;
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        quote::QuoteTick,
        trade::TradeTick,
    },
    identifiers::{
        account_id::AccountId, client_id::ClientId, client_order_id::ClientOrderId,
        component_id::ComponentId, exec_algorithm_id::ExecAlgorithmId, instrument_id::InstrumentId,
        position_id::PositionId, strategy_id::StrategyId, symbol::Symbol, trader_id::TraderId,
        venue::Venue, venue_order_id::VenueOrderId,
    },
    instruments::{synthetic::SyntheticInstrument, Instrument},
    orders::base::Order,
    position::Position,
    types::currency::Currency,
};
use ustr::Ustr;

/// A type of database operation.
#[derive(Clone, Debug)]
pub enum DatabaseOperation {
    Insert,
    Update,
    Delete,
}

/// Represents a database command to be performed which may be executed 'remotely' across a thread.
#[derive(Clone, Debug)]
pub struct DatabaseCommand {
    /// The database operation type.
    pub op_type: DatabaseOperation,
    /// The primary key for the operation.
    pub key: String,
    /// The data payload for the operation.
    pub payload: Option<Vec<Vec<u8>>>,
}

impl DatabaseCommand {
    pub fn new(op_type: DatabaseOperation, key: String, payload: Option<Vec<Vec<u8>>>) -> Self {
        Self {
            op_type,
            key,
            payload,
        }
    }
}

/// Provides a generic cache database facade.
///
/// The main operations take a consistent `key` and `payload` which should provide enough
/// information to implement the cache database in many different technologies.
///
/// Delete operations may need a `payload` to target specific values.
pub trait CacheDatabase {
    type DatabaseType;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<Self::DatabaseType>;
    fn flushdb(&mut self) -> anyhow::Result<()>;
    fn keys(&mut self, pattern: &str) -> anyhow::Result<Vec<String>>;
    fn read(&mut self, key: &str) -> anyhow::Result<Vec<Vec<u8>>>;
    fn insert(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> anyhow::Result<()>;
    fn update(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> anyhow::Result<()>;
    fn delete(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> anyhow::Result<()>;
    fn handle_messages(
        rx: Receiver<DatabaseCommand>,
        trader_key: String,
        config: HashMap<String, serde_json::Value>,
    );
}

pub struct CacheConfig {
    pub tick_capacity: usize,
    pub bar_capacity: usize,
    pub snapshot_orders: bool,
    pub snapshot_positions: bool,
}

pub struct CacheIndex {
    venue_account: HashMap<Venue, AccountId>,
    venue_orders: HashMap<Venue, HashSet<ClientOrderId>>,
    venue_positions: HashMap<Venue, HashSet<PositionId>>,
    order_ids: HashMap<VenueOrderId, ClientOrderId>,
    order_position: HashMap<ClientOrderId, PositionId>,
    order_strategy: HashMap<ClientOrderId, StrategyId>,
    order_client: HashMap<ClientOrderId, ClientId>,
    position_strategy: HashMap<PositionId, StrategyId>,
    position_orders: HashMap<PositionId, HashSet<ClientOrderId>>,
    instrument_orders: HashMap<InstrumentId, HashSet<ClientOrderId>>,
    instrument_positions: HashMap<InstrumentId, HashSet<PositionId>>,
    strategy_orders: HashMap<StrategyId, HashSet<ClientOrderId>>,
    strategy_positions: HashMap<StrategyId, HashSet<PositionId>>,
    exec_algorithm_orders: HashMap<ExecAlgorithmId, HashSet<ClientOrderId>>,
    exec_spawn_orders: HashMap<ExecAlgorithmId, HashSet<ClientOrderId>>,
    orders: HashSet<ClientOrderId>,
    orders_open: HashSet<ClientOrderId>,
    orders_closed: HashSet<ClientOrderId>,
    orders_emulated: HashSet<ClientOrderId>,
    orders_inflight: HashSet<ClientOrderId>,
    orders_pending_cancel: HashSet<ClientOrderId>,
    positions: HashSet<PositionId>,
    positions_open: HashSet<PositionId>,
    positions_closed: HashSet<PositionId>,
    actors: HashSet<ComponentId>,
    strategies: HashSet<StrategyId>,
    exec_algorithms: HashSet<ExecAlgorithmId>,
}

pub struct Cache {
    config: CacheConfig,
    index: CacheIndex,
    // database: Option<Box<dyn CacheDatabase>>,  TODO
    // xrate_calculator: ExchangeRateCalculator TODO
    general: HashMap<Ustr, Vec<u8>>,
    xrate_symbols: HashMap<InstrumentId, Symbol>,
    quote_ticks: HashMap<InstrumentId, VecDeque<QuoteTick>>,
    trade_ticks: HashMap<InstrumentId, VecDeque<TradeTick>>,
    // order_books: HashMap<InstrumentId, OrderBook>>,  TODO: Needs single book
    bars: HashMap<BarType, VecDeque<Bar>>,
    bars_bid: HashMap<BarType, Bar>,
    bars_ask: HashMap<BarType, Bar>,
    currencies: HashMap<Ustr, Currency>,
    instruments: HashMap<InstrumentId, Box<dyn Instrument>>,
    synthetics: HashMap<InstrumentId, SyntheticInstrument>,
    // accounts: HashMap<AccountId, Box<dyn Account>>,  TODO: Decide where trait should go
    orders: HashMap<ClientOrderId, VecDeque<Box<dyn Order>>>, // TODO: Efficency (use enum)
    // order_lists: HashMap<OrderListId, VecDeque<OrderList>>,  TODO: Need `OrderList`
    positions: HashMap<PositionId, Position>,
    position_snapshots: HashMap<PositionId, Vec<u8>>,
}
