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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{collections::HashMap, sync::mpsc::Receiver};

use nautilus_core::{time::UnixNanos, uuid::UUID4};
use nautilus_model::{
    identifiers::{
        account_id::AccountId, client_id::ClientId, client_order_id::ClientOrderId,
        component_id::ComponentId, instrument_id::InstrumentId, position_id::PositionId,
        strategy_id::StrategyId, trader_id::TraderId, venue_order_id::VenueOrderId,
    },
    instruments::{synthetic::SyntheticInstrument, Instrument},
    orders::base::{Order, OrderAny},
    position::Position,
    types::currency::Currency,
};
use ustr::Ustr;

use crate::enums::SerializationEncoding;

/// A type of database operation.
#[derive(Clone, Debug)]
pub enum DatabaseOperation {
    Insert,
    Update,
    Delete,
    Close,
}

/// Represents a database command to be performed which may be executed in another thread.
#[derive(Clone, Debug)]
pub struct DatabaseCommand {
    /// The database operation type.
    pub op_type: DatabaseOperation,
    /// The primary key for the operation.
    pub key: Option<String>,
    /// The data payload for the operation.
    pub payload: Option<Vec<Vec<u8>>>,
}

impl DatabaseCommand {
    pub fn new(op_type: DatabaseOperation, key: String, payload: Option<Vec<Vec<u8>>>) -> Self {
        Self {
            op_type,
            key: Some(key),
            payload,
        }
    }

    /// Initialize a `Close` database command, this is meant to close the database cache channel.
    pub fn close() -> Self {
        Self {
            op_type: DatabaseOperation::Close,
            key: None,
            payload: None,
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
    fn close(&mut self) -> anyhow::Result<()>;
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

pub struct CacheDatabaseAdapter {
    pub encoding: SerializationEncoding,
    // database: Box<dyn CacheDatabase>,  // TBD
}

impl CacheDatabaseAdapter {
    pub fn close(&self) -> anyhow::Result<()> {
        Ok(()) // TODO
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        Ok(()) // TODO
    }

    pub fn keys(&self) -> anyhow::Result<Vec<String>> {
        Ok(vec![])
    }

    pub fn load(&self) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        Ok(HashMap::new()) // TODO
    }

    pub fn load_currencies(&self) -> anyhow::Result<HashMap<Ustr, Currency>> {
        Ok(HashMap::new()) // TODO
    }

    pub fn load_instruments(&self) -> anyhow::Result<HashMap<InstrumentId, Box<dyn Instrument>>> {
        Ok(HashMap::new()) // TODO
    }

    pub fn load_synthetics(&self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>> {
        Ok(HashMap::new()) // TODO
    }

    // pub fn load_accounts() -> anyhow::Result<HashMap<AccountId, Box<dyn Account>>> {
    //     Ok(HashMap::new()) // TODO
    // }

    pub fn load_orders(&self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>> {
        Ok(HashMap::new()) // TODO
    }

    pub fn load_positions(&self) -> anyhow::Result<HashMap<PositionId, Position>> {
        Ok(HashMap::new()) // TODO
    }

    pub fn load_index_order_position(&self) -> anyhow::Result<HashMap<ClientOrderId, Position>> {
        Ok(HashMap::new()) // TODO
    }

    pub fn load_index_order_client(&self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>> {
        Ok(HashMap::new()) // TODO
    }

    pub fn load_currency(&self, code: &Ustr) -> anyhow::Result<Currency> {
        todo!() // TODO
    }

    pub fn load_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Box<dyn Instrument>> {
        todo!() // TODO
    }

    pub fn load_synthetic(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<SyntheticInstrument> {
        todo!() // TODO
    }

    pub fn load_account(&self, account_id: &AccountId) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn load_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<Box<dyn Order>> {
        todo!() // TODO
    }

    pub fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Position> {
        todo!() // TODO
    }

    pub fn load_actor(
        &self,
        component_id: &ComponentId,
    ) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        todo!() // TODO
    }

    pub fn delete_actor(&self, component_id: &ComponentId) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn load_strategy(
        &self,
        strategy_id: &StrategyId,
    ) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        todo!() // TODO
    }

    pub fn delete_strategy(&self, component_id: &StrategyId) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn add(&self, key: String, value: Vec<u8>) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn add_currency(&self, currency: Currency) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn add_instrument(&self, instrument: Box<dyn Instrument>) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn add_synthetic(&self, synthetic: SyntheticInstrument) -> anyhow::Result<()> {
        todo!() // TODO
    }

    // pub fn add_account(&self) -> anyhow::Result<Box<dyn Account>> {
    //     todo!() // TODO
    // }

    pub fn add_order(&self, order: &OrderAny) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn add_position(&self, position: Position) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn index_venue_order_id(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn index_order_position(
        &self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn update_actor(&self) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn update_strategy(&self) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn update_account(&self) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn update_order(&self, order: Box<dyn Order>) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn update_position(&self, position: Position) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn snapshot_order_state(&self, order: OrderAny) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn snapshot_position_state(&self, position: Position) -> anyhow::Result<()> {
        todo!() // TODO
    }

    pub fn heartbeat(&self, timestamp: UnixNanos) -> anyhow::Result<()> {
        todo!() // TODO
    }
}
