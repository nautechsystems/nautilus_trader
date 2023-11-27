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

use std::{collections::HashMap, sync::mpsc::Receiver, time::Duration};

use nautilus_core::{time::duration_since_unix_epoch, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use redis::*;
use serde_json::Value;

use crate::msgbus::BusMessage;

const DELIMITER: char = ':';
const XTRIM: &str = "XTRIM";
const MINID: &str = "MINID";

pub fn get_redis_url(config: &HashMap<String, Value>) -> String {
    let empty = Value::Object(serde_json::Map::new());
    let database = config.get("database").unwrap_or(&empty);

    let host = database
        .get("host")
        .map(|v| v.as_str().unwrap_or("127.0.0.1"));
    let port = database.get("port").map(|v| v.as_str().unwrap_or("6379"));
    let username = database
        .get("username")
        .map(|v| v.as_str().unwrap_or_default());
    let password = database
        .get("password")
        .map(|v| v.as_str().unwrap_or_default());
    let use_ssl = database.get("ssl").unwrap_or(&Value::Bool(false));

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

pub fn handle_messages_with_redis(
    trader_id: TraderId,
    instance_id: UUID4,
    config: HashMap<String, Value>,
    rx: Receiver<BusMessage>,
) {
    let redis_url = get_redis_url(&config);
    let client = redis::Client::open(redis_url).unwrap();
    let stream_name = get_stream_name(&config, trader_id, instance_id);
    let autotrim_mins = config
        .get("autotrim_mins")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let autotrim_duration = Duration::from_secs(autotrim_mins as u64 * 60);
    let mut last_trim_index: HashMap<String, usize> = HashMap::new();
    let mut conn = client.get_connection().unwrap();

    // Continue to receive and handle bus messages until channel is hung up
    while let Ok(msg) = rx.recv() {
        let key = format!("{stream_name}{}", &msg.topic);
        let items: Vec<(&str, &Vec<u8>)> = vec![("payload", &msg.payload)];
        let result: Result<(), redis::RedisError> = conn.xadd(&key, "*", &items);

        if let Err(e) = result {
            eprintln!("Error publishing message: {e}");
        }

        if autotrim_mins == 0 {
            return; // Nothing else to do
        }

        // Autotrim stream
        let last_trim_ms = last_trim_index.entry(key.clone()).or_insert(0); // Remove clone
        let unix_duration_now = duration_since_unix_epoch();

        // Improve efficiency of this by batching
        if *last_trim_ms < (unix_duration_now - Duration::from_secs(60)).as_millis() as usize {
            let min_timestamp_ms = (unix_duration_now - autotrim_duration).as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(XTRIM)
                .arg(&key)
                .arg(MINID)
                .arg(min_timestamp_ms)
                .query(&mut conn);

            if let Err(e) = result {
                eprintln!("Error trimming stream '{key}': {e}");
            } else {
                last_trim_index.insert(key, unix_duration_now.as_millis() as usize);
            }
        }
    }
}

fn get_stream_name(
    config: &HashMap<String, Value>,
    trader_id: TraderId,
    instance_id: UUID4,
) -> String {
    let mut stream_name = String::new();

    if let Some(Value::String(s)) = config.get("stream") {
        if !s.is_empty() {
            stream_name.push_str(s.trim_matches('"'));
            stream_name.push(DELIMITER);
        }
    }

    stream_name.push_str(trader_id.value.as_str());
    stream_name.push(DELIMITER);

    if let Some(Value::Bool(true)) = config.get("use_instance_id") {
        stream_name.push_str(&format!("{instance_id}"));
        stream_name.push(DELIMITER);
    }

    stream_name
}
