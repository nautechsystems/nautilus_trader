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

use std::{
    collections::HashMap,
    sync::mpsc::{channel, Receiver, Sender},
    thread,
};

use anyhow::{anyhow, Result};
use nautilus_common::redis::get_redis_url;
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::prelude::*;
use redis::{Commands, Connection};
use serde_json::Value;

use crate::cache::{CacheDatabase, DatabaseOperation};

#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct RedisCacheDatabase {
    pub trader_id: TraderId,
    conn_read: Connection,
    tx: Sender<DatabaseOperation>,
}

impl CacheDatabase for RedisCacheDatabase {
    type DatabaseType = RedisCacheDatabase;

    fn new(trader_id: TraderId, config: HashMap<String, Value>) -> Result<RedisCacheDatabase> {
        let redis_url = get_redis_url(&config);
        let client = redis::Client::open(redis_url)?;
        let conn_read = client.get_connection().unwrap();

        let (tx, rx) = channel::<DatabaseOperation>();

        thread::spawn(move || {
            Self::handle_ops(trader_id, config, rx);
        });

        Ok(RedisCacheDatabase {
            trader_id,
            conn_read,
            tx,
        })
    }

    fn read(&mut self, op_type: String) -> Result<Vec<Vec<u8>>> {
        let result: Vec<Vec<u8>> = self.conn_read.get(op_type)?;
        Ok(result)
    }

    fn write(&mut self, op_type: String, payload: Vec<Vec<u8>>) -> Result<String> {
        let op = DatabaseOperation::new(op_type, payload);
        match self.tx.send(op) {
            Ok(_) => Ok("OK".to_string()),
            Err(e) => Err(anyhow!("Failed to send to channel: {e}")),
        }
    }

    fn handle_ops(
        trader_id: TraderId,
        config: HashMap<String, Value>,
        rx: Receiver<DatabaseOperation>,
    ) {
        let redis_url = get_redis_url(&config);
        let client = redis::Client::open(redis_url).unwrap();
        let _conn_write = client.get_connection().unwrap();

        println!("{:?}", trader_id); // TODO: Temp

        // Continue to receive and handle bus messages until channel is hung up
        while let Ok(op) = rx.recv() {
            println!("{:?} {:?}", op.op_type, op.payload);
        }
    }
}
