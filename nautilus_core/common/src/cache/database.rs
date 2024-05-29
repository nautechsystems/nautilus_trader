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

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    identifiers::{
        account_id::AccountId, client_id::ClientId, client_order_id::ClientOrderId,
        component_id::ComponentId, instrument_id::InstrumentId, position_id::PositionId,
        strategy_id::StrategyId, venue_order_id::VenueOrderId,
    },
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
    orders::{any::OrderAny, base::Order},
    position::Position,
    types::currency::Currency,
};
use ustr::Ustr;

use crate::interface::account::Account;

pub trait CacheDatabaseAdapter {
    fn close(&self) -> anyhow::Result<()>;

    fn flush(&self) -> anyhow::Result<()>;

    fn load(&self) -> anyhow::Result<HashMap<String, Vec<u8>>>;

    fn load_currencies(&self) -> anyhow::Result<HashMap<Ustr, Currency>>;

    fn load_instruments(&self) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>>;

    fn load_synthetics(&self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>>;

    fn load_accounts(&self) -> anyhow::Result<HashMap<AccountId, Box<dyn Account>>>;

    fn load_orders(&self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>>;

    fn load_positions(&self) -> anyhow::Result<HashMap<PositionId, Position>>;

    fn load_index_order_position(&self) -> anyhow::Result<HashMap<ClientOrderId, Position>>;

    fn load_index_order_client(&self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>>;

    fn load_currency(&self, code: &Ustr) -> anyhow::Result<Currency>;

    fn load_instrument(&self, instrument_id: &InstrumentId) -> anyhow::Result<InstrumentAny>;

    fn load_synthetic(&self, instrument_id: &InstrumentId) -> anyhow::Result<SyntheticInstrument>;

    fn load_account(&self, account_id: &AccountId) -> anyhow::Result<()>;

    fn load_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<Box<dyn Order>>;

    fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Position>;

    fn load_actor(&self, component_id: &ComponentId) -> anyhow::Result<HashMap<String, Vec<u8>>>;

    fn delete_actor(&self, component_id: &ComponentId) -> anyhow::Result<()>;

    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<HashMap<String, Vec<u8>>>;

    fn delete_strategy(&self, component_id: &StrategyId) -> anyhow::Result<()>;

    fn add(&self, key: String, value: Vec<u8>) -> anyhow::Result<()>;

    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()>;

    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()>;

    fn add_synthetic(&self, synthetic: &SyntheticInstrument) -> anyhow::Result<()>;

    fn add_account(&self, account: &dyn Account) -> anyhow::Result<Box<dyn Account>>;

    fn add_order(&self, order: &OrderAny) -> anyhow::Result<()>;

    fn add_position(&self, position: &Position) -> anyhow::Result<()>;

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

    fn update_account(&self, account: &dyn Account) -> anyhow::Result<()>;

    fn update_order(&self, order: &OrderAny) -> anyhow::Result<()>;

    fn update_position(&self, position: &Position) -> anyhow::Result<()>;

    fn snapshot_order_state(&self, order: &OrderAny) -> anyhow::Result<()>;

    fn snapshot_position_state(&self, position: &Position) -> anyhow::Result<()>;

    fn heartbeat(&self, timestamp: UnixNanos) -> anyhow::Result<()>;
}
