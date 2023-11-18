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
use nautilus_model::identifiers::trader_id::TraderId;
use redis::{Commands, Connection};
use serde_json::Value;

use crate::cache::{CacheDatabase, DatabaseOperation};

pub struct RedisCacheDatabase {
    _trader_id: TraderId,
    read_conn: Connection,
    tx: Sender<DatabaseOperation>,
}

impl CacheDatabase for RedisCacheDatabase {
    fn new(trader_id: TraderId, config: HashMap<String, Value>) -> Self {
        let redis_url = get_redis_url(config.clone());
        let client = redis::Client::open(redis_url).unwrap();
        let read_conn = client.get_connection().unwrap();

        let (tx, rx) = channel::<DatabaseOperation>();

        thread::spawn(move || {
            Self::handle_ops(trader_id, config, rx);
        });

        RedisCacheDatabase {
            _trader_id: trader_id,
            read_conn,
            tx,
        }
    }

    fn read(&mut self, op_type: String) -> Vec<Vec<u8>> {
        // TODO: Implement
        let result: Vec<Vec<u8>> = self.read_conn.get(op_type).unwrap();
        result
    }

    fn write(&mut self, op_type: String, payload: Vec<Vec<u8>>) -> Result<(), String> {
        let op = DatabaseOperation::new(op_type, payload);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow!("Failed to send to channel: {e}").to_string()),
        }
    }

    fn handle_ops(
        trader_id: TraderId,
        config: HashMap<String, Value>,
        rx: Receiver<DatabaseOperation>,
    ) {
        let redis_url = get_redis_url(config);
        let client = redis::Client::open(redis_url).unwrap();
        let _conn = client.get_connection().unwrap();

        println!("{:?}", trader_id); // TODO: Temp

        // Continue to receive and handle bus messages until channel is hung up
        while let Ok(op) = rx.recv() {
            println!("{:?} {:?}", op.op_type, op.payload);
        }
    }
}

// Consolidate this with the MessageBus version
pub fn get_redis_url(config: HashMap<String, Value>) -> String {
    let host_default = Value::String("127.0.0.1".to_string());
    let port_default = Value::String("6379".to_string());
    let host = config.get("host").unwrap_or(&host_default);
    let port = config.get("port").unwrap_or(&port_default);
    let username = config
        .get("username")
        .map(|v| v.as_str().unwrap_or_default());
    let password = config
        .get("password")
        .map(|v| v.as_str().unwrap_or_default());
    let use_ssl = config.get("ssl").unwrap_or(&Value::Bool(false));

    format!(
        "redis{}://{}:{}@{}:{}",
        if use_ssl.as_bool().unwrap_or(false) {
            "s"
        } else {
            ""
        },
        username.unwrap_or(""),
        password.unwrap_or(""),
        host.as_str().unwrap(),
        port.as_str().unwrap(),
    )
}
