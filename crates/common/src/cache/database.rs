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

//! Provides a `Cache` database backing.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]
// TODO: Remove once error documentation is complete
#![allow(clippy::missing_errors_doc)]

use std::collections::HashMap;

use bytes::Bytes;
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, DataType, GreeksData, QuoteTick, TradeTick, YieldCurveData},
    events::{OrderEventAny, OrderSnapshot, position::snapshot::PositionSnapshot},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
        VenueOrderId,
    },
    instruments::{InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
    types::Currency,
};
use ustr::Ustr;

use crate::{custom::CustomData, signal::Signal};

#[derive(Debug, Default)]
pub struct CacheMap {
    pub currencies: HashMap<Ustr, Currency>,
    pub instruments: HashMap<InstrumentId, InstrumentAny>,
    pub synthetics: HashMap<InstrumentId, SyntheticInstrument>,
    pub accounts: HashMap<AccountId, AccountAny>,
    pub orders: HashMap<ClientOrderId, OrderAny>,
    pub positions: HashMap<PositionId, Position>,
    pub greeks: HashMap<InstrumentId, GreeksData>,
    pub yield_curves: HashMap<String, YieldCurveData>,
}

#[async_trait::async_trait]
pub trait CacheDatabaseAdapter {
    /// Closes the cache database connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the database fails to close properly.
    /// # Errors
    ///
    /// Returns an error if closing the cache database fails.
    fn close(&mut self) -> anyhow::Result<()>;

    /// Flushes any pending changes to the database.
    ///
    /// # Errors
    ///
    /// Returns an error if flushing changes fails.
    /// # Errors
    ///
    /// Returns an error if flushing the cache database fails.
    fn flush(&mut self) -> anyhow::Result<()>;

    /// Loads all cached data into memory.
    ///
    /// # Errors
    ///
    /// Returns an error if loading data from the database fails.
    /// # Errors
    ///
    /// Returns an error if loading all cache data fails.
    async fn load_all(&self) -> anyhow::Result<CacheMap>;

    /// Loads raw key-value data from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the load operation fails.
    /// # Errors
    ///
    /// Returns an error if loading raw key-value data fails.
    fn load(&self) -> anyhow::Result<HashMap<String, Bytes>>;

    /// Loads all currencies from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading currencies fails.
    /// # Errors
    ///
    /// Returns an error if loading currencies fails.
    async fn load_currencies(&self) -> anyhow::Result<HashMap<Ustr, Currency>>;

    /// Loads all instruments from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading instruments fails.
    /// # Errors
    ///
    /// Returns an error if loading instruments fails.
    async fn load_instruments(&self) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>>;

    /// Loads all synthetic instruments from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading synthetic instruments fails.
    /// # Errors
    ///
    /// Returns an error if loading synthetic instruments fails.
    async fn load_synthetics(&self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>>;

    /// Loads all accounts from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading accounts fails.
    /// # Errors
    ///
    /// Returns an error if loading accounts fails.
    async fn load_accounts(&self) -> anyhow::Result<HashMap<AccountId, AccountAny>>;

    /// Loads all orders from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading orders fails.
    /// # Errors
    ///
    /// Returns an error if loading orders fails.
    async fn load_orders(&self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>>;

    /// Loads all positions from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading positions fails.
    /// # Errors
    ///
    /// Returns an error if loading positions fails.
    async fn load_positions(&self) -> anyhow::Result<HashMap<PositionId, Position>>;

    /// Loads all [`GreeksData`] from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading greeks data fails.
    async fn load_greeks(&self) -> anyhow::Result<HashMap<InstrumentId, GreeksData>> {
        Ok(HashMap::new())
    }

    /// Loads all [`YieldCurveData`] from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading yield curve data fails.
    async fn load_yield_curves(&self) -> anyhow::Result<HashMap<String, YieldCurveData>> {
        Ok(HashMap::new())
    }

    /// Loads mapping from order IDs to position IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the index order-position mapping fails.
    /// # Errors
    ///
    /// Returns an error if loading order-position index fails.
    fn load_index_order_position(&self) -> anyhow::Result<HashMap<ClientOrderId, Position>>;

    /// Loads mapping from order IDs to client IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the index order-client mapping fails.
    /// # Errors
    ///
    /// Returns an error if loading order-client index fails.
    fn load_index_order_client(&self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>>;

    /// Loads a single currency by code.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the currency fails.
    /// # Errors
    ///
    /// Returns an error if loading a single currency fails.
    async fn load_currency(&self, code: &Ustr) -> anyhow::Result<Option<Currency>>;

    /// Loads a single instrument by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the instrument fails.
    /// # Errors
    ///
    /// Returns an error if loading a single instrument fails.
    async fn load_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>>;

    /// Loads a single synthetic instrument by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the synthetic instrument fails.
    /// # Errors
    ///
    /// Returns an error if loading a single synthetic instrument fails.
    async fn load_synthetic(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>>;

    /// Loads a single account by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the account fails.
    /// # Errors
    ///
    /// Returns an error if loading a single account fails.
    async fn load_account(&self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>>;

    /// Loads a single order by client order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the order fails.
    /// # Errors
    ///
    /// Returns an error if loading a single order fails.
    async fn load_order(&self, client_order_id: &ClientOrderId)
    -> anyhow::Result<Option<OrderAny>>;

    /// Loads a single position by position ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the position fails.
    /// # Errors
    ///
    /// Returns an error if loading a single position fails.
    async fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Option<Position>>;

    /// Loads actor state by component ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading actor state fails.
    /// # Errors
    ///
    /// Returns an error if loading actor state fails.
    fn load_actor(&self, component_id: &ComponentId) -> anyhow::Result<HashMap<String, Bytes>>;

    /// Loads strategy state by strategy ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading strategy state fails.
    /// # Errors
    ///
    /// Returns an error if loading strategy state fails.
    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<HashMap<String, Bytes>>;

    /// Loads signals by name.
    ///
    /// # Errors
    ///
    /// Returns an error if loading signals fails.
    /// # Errors
    ///
    /// Returns an error if loading signals fails.
    fn load_signals(&self, name: &str) -> anyhow::Result<Vec<Signal>>;

    /// Loads custom data by data type.
    ///
    /// # Errors
    ///
    /// Returns an error if loading custom data fails.
    /// # Errors
    ///
    /// Returns an error if loading custom data fails.
    fn load_custom_data(&self, data_type: &DataType) -> anyhow::Result<Vec<CustomData>>;

    /// Loads an order snapshot by client order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the order snapshot fails.
    /// # Errors
    ///
    /// Returns an error if loading order snapshot fails.
    fn load_order_snapshot(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderSnapshot>>;

    /// Loads a position snapshot by position ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the position snapshot fails.
    /// # Errors
    ///
    /// Returns an error if loading position snapshot fails.
    fn load_position_snapshot(
        &self,
        position_id: &PositionId,
    ) -> anyhow::Result<Option<PositionSnapshot>>;

    /// Loads quote ticks by instrument ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading quotes fails.
    /// # Errors
    ///
    /// Returns an error if loading quotes fails.
    fn load_quotes(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>>;

    /// Loads trade ticks by instrument ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading trades fails.
    /// # Errors
    ///
    /// Returns an error if loading trades fails.
    fn load_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>>;

    /// Loads bars by instrument ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading bars fails.
    /// # Errors
    ///
    /// Returns an error if loading bars fails.
    fn load_bars(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>>;

    /// # Errors
    ///
    /// Returns an error if adding a generic key/value fails.
    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding a currency fails.
    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding an instrument fails.
    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding a synthetic instrument fails.
    fn add_synthetic(&self, synthetic: &SyntheticInstrument) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding an account fails.
    fn add_account(&self, account: &AccountAny) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding an order fails.
    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding an order snapshot fails.
    fn add_order_snapshot(&self, snapshot: &OrderSnapshot) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding a position fails.
    fn add_position(&self, position: &Position) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding a position snapshot fails.
    fn add_position_snapshot(&self, snapshot: &PositionSnapshot) -> anyhow::Result<()>;

    /// # Errors
    ///
    /// Returns an error if adding an order book fails.
    fn add_order_book(&self, order_book: &OrderBook) -> anyhow::Result<()>;

    fn add_signal(&self, signal: &Signal) -> anyhow::Result<()>;

    fn add_custom_data(&self, data: &CustomData) -> anyhow::Result<()>;

    fn add_quote(&self, quote: &QuoteTick) -> anyhow::Result<()>;

    fn add_trade(&self, trade: &TradeTick) -> anyhow::Result<()>;

    fn add_bar(&self, bar: &Bar) -> anyhow::Result<()>;

    fn add_greeks(&self, greeks: &GreeksData) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_yield_curve(&self, yield_curve: &YieldCurveData) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_actor(&self, component_id: &ComponentId) -> anyhow::Result<()>;

    fn delete_strategy(&self, component_id: &StrategyId) -> anyhow::Result<()>;

    fn index_venue_order_id(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()>;

    fn index_order_position(
        &self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()>;

    fn update_actor(&self) -> anyhow::Result<()>;

    fn update_strategy(&self) -> anyhow::Result<()>;

    fn update_account(&self, account: &AccountAny) -> anyhow::Result<()>;

    fn update_order(&self, order_event: &OrderEventAny) -> anyhow::Result<()>;

    fn update_position(&self, position: &Position) -> anyhow::Result<()>;

    fn snapshot_order_state(&self, order: &OrderAny) -> anyhow::Result<()>;

    fn snapshot_position_state(&self, position: &Position) -> anyhow::Result<()>;

    fn heartbeat(&self, timestamp: UnixNanos) -> anyhow::Result<()>;
}
