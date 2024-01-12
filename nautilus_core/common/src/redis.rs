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

use std::{
    collections::{HashMap, VecDeque},
    sync::mpsc::{Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use nautilus_core::{time::duration_since_unix_epoch, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use redis::*;
use serde_json::{json, Value};

use crate::msgbus::BusMessage;

const DELIMITER: char = ':';
const XTRIM: &str = "XTRIM";
const MINID: &str = "MINID";

pub fn handle_messages_with_redis(
    rx: Receiver<BusMessage>,
    trader_id: TraderId,
    instance_id: UUID4,
    config: HashMap<String, Value>,
) {
    let redis_url = get_redis_url(&config);
    let client = redis::Client::open(redis_url).unwrap();
    let mut conn = client.get_connection().unwrap();
    let stream_name = get_stream_name(trader_id, instance_id, &config);

    // Autotrimming
    let autotrim_mins = config
        .get("autotrim_mins")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let autotrim_duration = if autotrim_mins > 0 {
        Some(Duration::from_secs(autotrim_mins as u64 * 60))
    } else {
        None
    };
    let mut last_trim_index: HashMap<String, usize> = HashMap::new();

    // Buffering
    let mut buffer: VecDeque<BusMessage> = VecDeque::new();
    let mut last_drain = Instant::now();
    let recv_interval = Duration::from_millis(1);
    let buffer_interval = get_buffer_interval(&config);

    loop {
        if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
            drain_buffer(
                &mut conn,
                &stream_name,
                autotrim_duration,
                &mut last_trim_index,
                &mut buffer,
            );
            last_drain = Instant::now();
        } else {
            // Continue to receive and handle messages until channel is hung up
            match rx.try_recv() {
                Ok(msg) => buffer.push_back(msg),
                Err(TryRecvError::Empty) => thread::sleep(recv_interval),
                Err(TryRecvError::Disconnected) => break, // Channel hung up
            }
        }
    }

    // Drain any remaining messages
    if !buffer.is_empty() {
        drain_buffer(
            &mut conn,
            &stream_name,
            autotrim_duration,
            &mut last_trim_index,
            &mut buffer,
        );
    }
}

fn drain_buffer(
    conn: &mut Connection,
    stream_name: &str,
    autotrim_duration: Option<Duration>,
    last_trim_index: &mut HashMap<String, usize>,
    buffer: &mut VecDeque<BusMessage>,
) {
    let mut pipe = redis::pipe();
    pipe.atomic();

    for msg in buffer.drain(..) {
        let key = format!("{stream_name}{}", &msg.topic);
        let items: Vec<(&str, &Vec<u8>)> = vec![("payload", &msg.payload)];
        pipe.xadd(&key, "*", &items);

        if autotrim_duration.is_none() {
            continue; // Nothing else to do
        }

        // Autotrim stream
        let last_trim_ms = last_trim_index.entry(key.clone()).or_insert(0); // Remove clone
        let unix_duration_now = duration_since_unix_epoch();

        // Improve efficiency of this by batching
        if *last_trim_ms < (unix_duration_now - Duration::from_secs(60)).as_millis() as usize {
            let min_timestamp_ms =
                (unix_duration_now - autotrim_duration.unwrap()).as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(XTRIM)
                .arg(&key)
                .arg(MINID)
                .arg(min_timestamp_ms)
                .query(conn);

            if let Err(e) = result {
                eprintln!("Error trimming stream '{key}': {e}");
            } else {
                last_trim_index.insert(key, unix_duration_now.as_millis() as usize);
            }
        }
    }

    if let Err(e) = pipe.query::<()>(conn) {
        eprintln!("{e}");
    }
}

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
    let use_ssl = database.get("ssl").unwrap_or(&json!(false));

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

pub fn get_buffer_interval(config: &HashMap<String, Value>) -> Duration {
    let buffer_interval_ms = config
        .get("buffer_interval_ms")
        .map(|v| v.as_u64().unwrap_or(0));
    Duration::from_millis(buffer_interval_ms.unwrap_or(0))
}

fn get_stream_name(
    trader_id: TraderId,
    instance_id: UUID4,
    config: &HashMap<String, Value>,
) -> String {
    let mut stream_name = String::new();

    if let Some(json!(true)) = config.get("use_trader_prefix") {
        stream_name.push_str("trader-");
    }

    if let Some(json!(true)) = config.get("use_trader_id") {
        stream_name.push_str(trader_id.value.as_str());
        stream_name.push(DELIMITER);
    }

    if let Some(json!(true)) = config.get("use_instance_id") {
        stream_name.push_str(&format!("{instance_id}"));
        stream_name.push(DELIMITER);
    }

    let stream_prefix = config
        .get("streams_prefix")
        .expect("Invalid configuration: no `streams_prefix` key found")
        .as_str()
        .expect("Invalid configuration: `streams_prefix` is not a string");
    stream_name.push_str(stream_prefix);
    stream_name.push(DELIMITER);

    stream_name
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_get_stream_name_with_trader_prefix_and_instance_id() {
        let trader_id = TraderId::from("tester-123");
        let instance_id = UUID4::new();
        let mut config = HashMap::new();
        config.insert("use_trader_prefix".to_string(), json!(true));
        config.insert("use_trader_id".to_string(), json!(true));
        config.insert("use_instance_id".to_string(), json!(true));
        config.insert("streams_prefix".to_string(), json!("streams"));

        let key = get_stream_name(trader_id, instance_id, &config);
        assert_eq!(key, format!("trader-tester-123:{instance_id}:streams:"));
    }

    #[rstest]
    fn test_get_stream_name_without_trader_prefix_or_instance_id() {
        let trader_id = TraderId::from("tester-123");
        let instance_id = UUID4::new();
        let mut config = HashMap::new();
        config.insert("use_trader_prefix".to_string(), json!(false));
        config.insert("use_trader_id".to_string(), json!(false));
        config.insert("use_instance_id".to_string(), json!(false));
        config.insert("streams_prefix".to_string(), json!("streams"));

        let key = get_stream_name(trader_id, instance_id, &config);
        assert_eq!(key, format!("streams:"));
    }

    #[rstest]
    fn test_get_buffer_interval_default() {
        let config = HashMap::new();
        let buffer_interval = get_buffer_interval(&config);
        assert_eq!(buffer_interval, Duration::from_millis(0));
    }

    #[rstest]
    fn test_get_buffer_interval() {
        let mut config = HashMap::new();
        config.insert("buffer_interval_ms".to_string(), json!(100));

        let buffer_interval = get_buffer_interval(&config);
        assert_eq!(buffer_interval, Duration::from_millis(100));
    }
}
