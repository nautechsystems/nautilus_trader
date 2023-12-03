// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, sync::mpsc::Receiver};

use anyhow::Result;
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use serde_json::Value;

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
        config: HashMap<String, Value>,
    ) -> Result<Self::DatabaseType>;
    fn flushdb(&mut self) -> Result<()>;
    fn keys(&mut self, pattern: &str) -> Result<Vec<String>>;
    fn read(&mut self, key: &str) -> Result<Vec<Vec<u8>>>;
    fn insert(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> Result<()>;
    fn update(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> Result<()>;
    fn delete(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> Result<()>;
    fn handle_ops(
        rx: Receiver<DatabaseCommand>,
        trader_key: String,
        config: HashMap<String, Value>,
    );
}
