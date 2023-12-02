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

use anyhow::Result;
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::prelude::*;
use redis::{Commands, Connection};
use serde_json::Value;

use crate::cache::{CacheDatabase, DatabaseCommand, DatabaseOperation};

const DELIMITER: char = ':';
const GENERAL: &str = "general";
const CURRENCIES: &str = "currencies";

#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct RedisCacheDatabase {
    pub trader_id: TraderId,
    trader_key: String,
    conn: Connection,
    tx: Sender<DatabaseCommand>,
}

impl CacheDatabase for RedisCacheDatabase {
    type DatabaseType = RedisCacheDatabase;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, Value>,
    ) -> Result<RedisCacheDatabase> {
        let redis_url = get_redis_url(&config);
        let client = redis::Client::open(redis_url)?;
        let conn = client.get_connection().unwrap();

        let (tx, rx) = channel::<DatabaseCommand>();
        let _encoding = get_encoding(&config);
        let trader_key = get_trader_key(trader_id, instance_id, &config);
        let trader_key_clone = trader_key.clone();

        thread::spawn(move || {
            Self::handle_ops(rx, trader_key_clone, config);
        });

        Ok(RedisCacheDatabase {
            trader_id,
            trader_key,
            conn,
            tx,
        })
    }

    fn keys(&mut self, pattern: &str) -> Result<Vec<String>> {
        match self.conn.keys(pattern) {
            Ok(keys) => Ok(keys),
            Err(e) => Err(e.into()),
        }
    }

    fn read(&mut self, key: &str) -> Result<Vec<Vec<u8>>> {
        let collection = get_collection_key(key);
        let key = format!("{}{DELIMITER}{}", self.trader_key, key);

        match collection {
            GENERAL => read_general(&mut self.conn, &key),
            CURRENCIES => read_general(&mut self.conn, &key),
            _ => panic!("Collection '{collection}' not recognized"),
        }
    }

    fn insert(&mut self, key: String, payload: Vec<Vec<u8>>) -> Result<(), String> {
        let op = DatabaseCommand::new(DatabaseOperation::Insert, key, Some(payload));
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to send to channel: {e}").to_string()),
        }
    }

    fn update(&mut self, key: String, payload: Vec<Vec<u8>>) -> Result<(), String> {
        let op = DatabaseCommand::new(DatabaseOperation::Update, key, Some(payload));
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to send to channel: {e}").to_string()),
        }
    }

    fn delete(&mut self, key: String) -> Result<(), String> {
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, None);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to send to channel: {e}").to_string()),
        }
    }

    fn handle_ops(
        rx: Receiver<DatabaseCommand>,
        trader_key: String,
        config: HashMap<String, Value>,
    ) {
        let redis_url = get_redis_url(&config);
        let client = redis::Client::open(redis_url).unwrap();
        let mut conn = client.get_connection().unwrap();

        // Continue to receive and handle bus messages until channel is hung up
        while let Ok(msg) = rx.recv() {
            let collection = get_collection_key(&msg.key);
            let key = format!("{trader_key}{DELIMITER}{}", msg.key);

            match msg.op_type {
                DatabaseOperation::Insert => insert(
                    &mut conn,
                    collection,
                    &key,
                    &msg.payload.expect("Null `payload` for `insert`"),
                )
                .unwrap(),
                _ => panic!("Unsupported `op_type`"),
            }
        }
    }
}

fn read_general(conn: &mut Connection, key: &str) -> Result<Vec<Vec<u8>>> {
    let result: Vec<u8> = conn.get(key)?;

    if result.is_empty() {
        Ok(vec![])
    } else {
        Ok(vec![result])
    }
}

fn insert(
    conn: &mut Connection,
    collection: &str,
    key: &str,
    value: &Vec<Vec<u8>>,
) -> Result<(), String> {
    assert!(!value.is_empty(), "Empty `payload` for `insert`");

    match collection {
        GENERAL => insert_general(conn, key, &value[0]),
        CURRENCIES => insert_general(conn, key, &value[0]),
        _ => panic!("Collection '{collection}' not recognized"),
    }
}

fn insert_general(conn: &mut Connection, key: &str, value: &Vec<u8>) -> Result<(), String> {
    conn.set(key, value)
        .map_err(|e| format!("Failed to set '{key}' in Redis: {e}"))
}

fn get_redis_url(config: &HashMap<String, Value>) -> String {
    let host = config
        .get("host")
        .map(|v| v.as_str().unwrap_or("127.0.0.1"));
    let port = config.get("port").map(|v| v.as_str().unwrap_or("6379"));
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
        host.unwrap(),
        port.unwrap(),
    )
}

fn get_trader_key(
    trader_id: TraderId,
    instance_id: UUID4,
    config: &HashMap<String, Value>,
) -> String {
    let mut key = String::new();

    if let Some(Value::Bool(true)) = config.get("use_trader_prefix") {
        key.push_str("trader-");
    }

    key.push_str(trader_id.value.as_str());

    if let Some(Value::Bool(true)) = config.get("use_instance_id") {
        key.push(DELIMITER);
        key.push_str(&format!("{instance_id}"));
    }

    key
}

fn get_encoding(config: &HashMap<String, Value>) -> String {
    config
        .get("encoding")
        .and_then(|v| v.as_str())
        .unwrap_or("msgpack")
        .to_string()
}

fn get_collection_key(key: &str) -> &str {
    key.split_once(DELIMITER)
        .unwrap_or_else(|| panic!("Invalid `key` '{}'", key))
        .0
}

#[allow(dead_code)]
fn deserialize_payload(encoding: &str, payload: &[u8]) -> Result<HashMap<String, Value>, String> {
    match encoding {
        "msgpack" => rmp_serde::from_slice(payload)
            .map_err(|e| format!("Failed to deserialize msgpack `payload`: {e}")),
        "json" => serde_json::from_slice(payload)
            .map_err(|e| format!("Failed to deserialize json `payload`: {e}")),
        _ => Err(format!("Unsupported encoding: {encoding}")),
    }
}
