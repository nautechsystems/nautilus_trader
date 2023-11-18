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
use nautilus_model::identifiers::trader_id::TraderId;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct DatabaseOperation {
    pub op_type: String,
    pub payload: Vec<Vec<u8>>,
}

impl DatabaseOperation {
    pub fn new(op_type: String, payload: Vec<Vec<u8>>) -> Self {
        Self { op_type, payload }
    }
}

pub trait CacheDatabase {
    fn new(trader_id: TraderId, config: HashMap<String, Value>) -> Self;
    fn read(&mut self, op_type: String) -> Vec<Vec<u8>>;
    fn write(&mut self, op_type: String, payload: Vec<Vec<u8>>) -> Result<(), String>;
    fn handle_ops(
        trader_id: TraderId,
        config: HashMap<String, Value>,
        rx: Receiver<DatabaseOperation>,
    );
}
