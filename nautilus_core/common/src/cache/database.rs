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

//! Provides a `Cache` database backing.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::collections::HashMap;

use bytes::Bytes;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    accounts::any::AccountAny,
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick, DataType},
    events::{order::OrderEventAny, position::snapshot::PositionSnapshot},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
        VenueOrderId,
    },
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
    orderbook::book::OrderBook,
    orders::any::OrderAny,
    position::Position,
    types::currency::Currency,
};
use ustr::Ustr;

use crate::{custom::CustomData, signal::Signal};

pub trait CacheDatabaseAdapter {
    fn close(&mut self) -> anyhow::Result<()>;

    fn flush(&mut self) -> anyhow::Result<()>;

    fn load(&self) -> anyhow::Result<HashMap<String, Bytes>>;

    fn load_currencies(&mut self) -> anyhow::Result<HashMap<Ustr, Currency>>;

    fn load_instruments(&mut self) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>>;

    fn load_synthetics(&mut self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>>;

    fn load_accounts(&mut self) -> anyhow::Result<HashMap<AccountId, AccountAny>>;

    fn load_orders(&mut self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>>;

    fn load_positions(&mut self) -> anyhow::Result<HashMap<PositionId, Position>>;

    fn load_index_order_position(&self) -> anyhow::Result<HashMap<ClientOrderId, Position>>;

    fn load_index_order_client(&self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>>;

    fn load_currency(&self, code: &Ustr) -> anyhow::Result<Option<Currency>>;

    fn load_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>>;

    fn load_synthetic(&self, instrument_id: &InstrumentId) -> anyhow::Result<SyntheticInstrument>;

    fn load_account(&self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>>;

    fn load_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<Option<OrderAny>>;

    fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Position>;

    fn load_actor(&self, component_id: &ComponentId) -> anyhow::Result<HashMap<String, Bytes>>;

    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<HashMap<String, Bytes>>;

    fn load_signals(&self, name: &str) -> anyhow::Result<Vec<Signal>>;

    fn load_custom_data(&self, data_type: &DataType) -> anyhow::Result<Vec<CustomData>>;

    fn load_quotes(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>>;

    fn load_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>>;

    fn load_bars(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>>;

    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()>;

    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()>;

    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()>;

    fn add_synthetic(&self, synthetic: &SyntheticInstrument) -> anyhow::Result<()>;

    fn add_account(&self, account: &AccountAny) -> anyhow::Result<()>;

    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()>;

    fn add_position(&self, position: &Position) -> anyhow::Result<()>;

    fn add_position_snapshot(&self, snapshot: &PositionSnapshot) -> anyhow::Result<()>;

    fn add_order_book(&self, order_book: &OrderBook) -> anyhow::Result<()>;

    fn add_signal(&self, signal: &Signal) -> anyhow::Result<()>;

    fn add_custom_data(&self, data: &CustomData) -> anyhow::Result<()>;

    fn add_quote(&self, quote: &QuoteTick) -> anyhow::Result<()>;

    fn add_trade(&self, trade: &TradeTick) -> anyhow::Result<()>;

    fn add_bar(&self, bar: &Bar) -> anyhow::Result<()>;

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
