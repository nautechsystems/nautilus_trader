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

#[derive(Clone, Debug)]
pub enum DatabaseOperation {
    Insert,
    Update,
    Delete,
}

#[derive(Clone, Debug)]
pub struct DatabaseCommand {
    pub op_type: DatabaseOperation,
    pub key: String,
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

pub trait CacheDatabase {
    type DatabaseType;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, Value>,
    ) -> Result<Self::DatabaseType>;
    fn read(&mut self, op_type: String) -> Result<Vec<Vec<u8>>>;
    fn insert(&mut self, key: String, payload: Vec<Vec<u8>>) -> Result<(), String>;
    fn update(&mut self, key: String, payload: Vec<Vec<u8>>) -> Result<(), String>;
    fn delete(&mut self, key: String) -> Result<(), String>;
    fn handle_ops(
        rx: Receiver<DatabaseCommand>,
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, Value>,
    );
}
