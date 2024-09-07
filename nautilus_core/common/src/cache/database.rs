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
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
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

pub trait CacheDatabaseAdapter {
    fn close(&mut self);

    fn flush(&mut self);

    fn load(&mut self) -> anyhow::Result<HashMap<String, Bytes>>;

    fn load_currencies(&mut self) -> anyhow::Result<HashMap<Ustr, Currency>>;

    fn load_instruments(&mut self) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>>;

    fn load_synthetics(&mut self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>>;

    fn load_accounts(&mut self) -> anyhow::Result<HashMap<AccountId, AccountAny>>;

    fn load_orders(&mut self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>>;

    fn load_positions(&mut self) -> anyhow::Result<HashMap<PositionId, Position>>;

    fn load_index_order_position(&mut self) -> anyhow::Result<HashMap<ClientOrderId, Position>>;

    fn load_index_order_client(&mut self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>>;

    fn load_currency(&mut self, code: &Ustr) -> anyhow::Result<Option<Currency>>;

    fn load_instrument(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>>;

    fn load_synthetic(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<SyntheticInstrument>;

    fn load_account(&mut self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>>;

    fn load_order(&mut self, client_order_id: &ClientOrderId) -> anyhow::Result<Option<OrderAny>>;

    fn load_position(&mut self, position_id: &PositionId) -> anyhow::Result<Position>;

    fn load_actor(&mut self, component_id: &ComponentId) -> anyhow::Result<HashMap<String, Bytes>>;

    fn delete_actor(&mut self, component_id: &ComponentId) -> anyhow::Result<()>;

    fn load_strategy(&mut self, strategy_id: &StrategyId)
        -> anyhow::Result<HashMap<String, Bytes>>;

    fn delete_strategy(&mut self, component_id: &StrategyId) -> anyhow::Result<()>;

    fn add(&mut self, key: String, value: Bytes) -> anyhow::Result<()>;

    fn add_currency(&mut self, currency: &Currency) -> anyhow::Result<()>;

    fn add_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()>;

    fn add_synthetic(&mut self, synthetic: &SyntheticInstrument) -> anyhow::Result<()>;

    fn add_account(&mut self, account: &AccountAny) -> anyhow::Result<()>;

    fn add_order(&mut self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()>;

    fn add_position(&mut self, position: &Position) -> anyhow::Result<()>;

    fn add_order_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()>;

    fn add_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()>;

    fn load_quotes(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>>;

    fn add_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()>;

    fn load_trades(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>>;

    fn add_bar(&mut self, bar: &Bar) -> anyhow::Result<()>;

    fn load_bars(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>>;

    fn index_venue_order_id(
        &mut self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()>;

    fn index_order_position(
        &mut self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()>;

    fn update_actor(&mut self) -> anyhow::Result<()>;

    fn update_strategy(&mut self) -> anyhow::Result<()>;

    fn update_account(&mut self, account: &AccountAny) -> anyhow::Result<()>;

    fn update_order(&mut self, order: &OrderAny) -> anyhow::Result<()>;

    fn update_position(&mut self, position: &Position) -> anyhow::Result<()>;

    fn snapshot_order_state(&mut self, order: &OrderAny) -> anyhow::Result<()>;

    fn snapshot_position_state(&mut self, position: &Position) -> anyhow::Result<()>;

    fn heartbeat(&mut self, timestamp: UnixNanos) -> anyhow::Result<()>;
}
