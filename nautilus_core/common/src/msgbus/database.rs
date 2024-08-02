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

use std::fmt::Display;

use bytes::Bytes;
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::TraderId;
use serde::{Deserialize, Serialize};

use crate::enums::SerializationEncoding;

/// Represents a bus message including a topic and payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct BusMessage {
    /// The topic to publish on.
    pub topic: String,
    /// The serialized payload for the message.
    pub payload: Bytes,
}

impl Display for BusMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}",
            self.topic,
            String::from_utf8_lossy(&self.payload)
        )
    }
}

/// Configuration for database connections.
///
/// # Parameters
///
/// - `database_type` (alias: `type`): The database type. Default is `"redis"`.
/// - `host`: The database host address. If `None`, the typical default should be used.
/// - `port`: The database port. If `None`, the typical default should be used.
/// - `username`: The account username for the database connection.
/// - `password`: The account password for the database connection. If a value is provided, it will be redacted in the string representation of this object.
/// - `ssl`: If the database should use an SSL-enabled connection. Default is `false`.
/// - `timeout`: The timeout (in seconds) to wait for a new connection. Default is `20`.
///
/// # Notes
///
/// If `database_type` is `"redis"`, it requires Redis version 6.2.0 and above for correct operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    #[serde(alias = "type")]
    pub database_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ssl: bool,
    pub timeout: u16,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            database_type: "redis".to_string(),
            host: None,
            port: None,
            username: None,
            password: None,
            ssl: false,
            timeout: 20,
        }
    }
}

/// Configuration for `MessageBus` instances.
///
/// # Parameters
///
/// - `database`: The configuration for the message bus backing database.
/// - `encoding`: The encoding for database operations, controls the type of serializer used. Default is `"msgpack"`.
/// - `timestamps_as_iso8601`: If timestamps should be persisted as ISO 8601 strings. If `false`, they will be persisted as UNIX nanoseconds. Default is `false`.
/// - `buffer_interval_ms`: The buffer interval (milliseconds) between pipelined/batched transactions. The recommended range if using buffered pipelining is [10, 1000] milliseconds, with a good compromise being 100 milliseconds.
/// - `autotrim_mins`: The lookback window in minutes for automatic stream trimming. The actual window may extend up to one minute beyond the specified value since streams are trimmed at most once every minute. This feature requires Redis version 6.2.0 or higher; otherwise, it will result in a command syntax error.
/// - `use_trader_prefix`: If a 'trader-' prefix is used for stream names. Default is `true`.
/// - `use_trader_id`: If the trader's ID is used for stream names. Default is `true`.
/// - `use_instance_id`: If the trader's instance ID is used for stream names. Default is `false`.
/// - `streams_prefix`: The prefix for externally published stream names. Must have a `database` config. Default is `"stream"`.
/// - `stream_per_topic`: If `true`, messages will be written to separate streams per topic. If `false`, all messages will be written to the same stream. Default is `true`.
/// - `external_streams`: The external stream keys the message bus will listen to for publishing deserialized message payloads internally.
/// - `types_filter`: A list of serializable types **not** to publish externally.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageBusConfig {
    pub database: Option<DatabaseConfig>,
    pub encoding: SerializationEncoding,
    pub timestamps_as_iso8601: bool,
    pub buffer_interval_ms: Option<u32>,
    pub autotrim_mins: Option<u32>,
    pub use_trader_prefix: bool,
    pub use_trader_id: bool,
    pub use_instance_id: bool,
    pub streams_prefix: String,
    pub stream_per_topic: bool,
    pub external_streams: Option<Vec<String>>,
    pub types_filter: Option<Vec<String>>,
}

impl Default for MessageBusConfig {
    fn default() -> Self {
        Self {
            database: None,
            encoding: SerializationEncoding::MsgPack,
            timestamps_as_iso8601: false,
            buffer_interval_ms: None,
            autotrim_mins: None,
            use_trader_prefix: true,
            use_trader_id: true,
            use_instance_id: false,
            streams_prefix: "stream".to_string(),
            stream_per_topic: true,
            external_streams: None,
            types_filter: None,
        }
    }
}

/// A generic message bus database facade.
///
/// The main operations take a consistent `key` and `payload` which should provide enough
/// information to implement the message bus database in many different technologies.
///
/// Delete operations may need a `payload` to target specific values.
pub trait MessageBusDatabaseAdapter {
    type DatabaseType;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: MessageBusConfig,
    ) -> anyhow::Result<Self::DatabaseType>;
    fn publish(&self, topic: String, payload: Bytes) -> anyhow::Result<()>;
    fn close(&mut self) -> anyhow::Result<()>;
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_default_database_config() {
        let config = DatabaseConfig::default();
        assert_eq!(config.database_type, "redis");
        assert_eq!(config.host, None);
        assert_eq!(config.port, None);
        assert_eq!(config.username, None);
        assert_eq!(config.password, None);
        assert!(!config.ssl);
        assert_eq!(config.timeout, 20);
    }

    #[rstest]
    fn test_deserialize_database_config() {
        let config_json = json!({
            "type": "redis",
            "host": "localhost",
            "port": 6379,
            "username": "user",
            "password": "pass",
            "ssl": true,
            "timeout": 30
        });
        let config: DatabaseConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.database_type, "redis");
        assert_eq!(config.host, Some("localhost".to_string()));
        assert_eq!(config.port, Some(6379));
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
        assert!(config.ssl);
        assert_eq!(config.timeout, 30);
    }

    #[rstest]
    fn test_default_message_bus_config() {
        let config = MessageBusConfig::default();
        assert_eq!(config.encoding, SerializationEncoding::MsgPack);
        assert!(!config.timestamps_as_iso8601);
        assert_eq!(config.buffer_interval_ms, None);
        assert_eq!(config.autotrim_mins, None);
        assert!(config.use_trader_prefix);
        assert!(config.use_trader_id);
        assert!(!config.use_instance_id);
        assert_eq!(config.streams_prefix, "stream");
        assert!(config.stream_per_topic);
        assert_eq!(config.external_streams, None);
        assert_eq!(config.types_filter, None);
    }

    #[test]
    fn test_deserialize_message_bus_config() {
        let config_json = json!({
            "database": {
                "type": "redis",
                "host": "localhost",
                "port": 6379,
                "username": "user",
                "password": "pass",
                "ssl": true,
                "timeout": 30
            },
            "encoding": "json",
            "timestamps_as_iso8601": true,
            "buffer_interval_ms": 100,
            "autotrim_mins": 60,
            "use_trader_prefix": false,
            "use_trader_id": false,
            "use_instance_id": true,
            "streams_prefix": "data_streams",
            "stream_per_topic": false,
            "external_streams": ["stream1", "stream2"],
            "types_filter": ["type1", "type2"]
        });
        let config: MessageBusConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.encoding, SerializationEncoding::Json);
        assert!(config.timestamps_as_iso8601);
        assert_eq!(config.buffer_interval_ms, Some(100));
        assert_eq!(config.autotrim_mins, Some(60));
        assert!(!config.use_trader_prefix);
        assert!(!config.use_trader_id);
        assert!(config.use_instance_id);
        assert_eq!(config.streams_prefix, "data_streams");
        assert!(!config.stream_per_topic);
        assert_eq!(
            config.external_streams,
            Some(vec!["stream1".to_string(), "stream2".to_string()])
        );
        assert_eq!(
            config.types_filter,
            Some(vec!["type1".to_string(), "type2".to_string()])
        );
    }
}
