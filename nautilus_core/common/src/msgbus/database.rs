// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_core::UUID4;
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
/// # Notes
///
/// If `database_type` is `"redis"`, it requires Redis version 6.2 or higher for correct operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// The database type.
    #[serde(alias = "type")]
    pub database_type: String,
    /// The database host address. If `None`, the typical default should be used.
    pub host: Option<String>,
    /// The database port. If `None`, the typical default should be used.
    pub port: Option<u16>,
    /// The account username for the database connection.
    pub username: Option<String>,
    /// The account password for the database connection.
    pub password: Option<String>,
    /// If the database should use an SSL-enabled connection.
    pub ssl: bool,
    /// The timeout (in seconds) to wait for a new connection.
    pub connection_timeout: u16,
    /// The timeout (in seconds) to wait for a response.
    pub response_timeout: u16,
    /// The number of retry attempts with exponential backoff for connection attempts.
    pub number_of_retries: usize,
    /// The base value for exponential backoff calculation.
    pub exponent_base: u64,
    /// The maximum delay between retry attempts (in seconds).
    pub max_delay: u64,
    /// The multiplication factor for retry delay calculation.
    pub factor: u64,
}

impl Default for DatabaseConfig {
    /// Creates a new default [`DatabaseConfig`] instance.
    fn default() -> Self {
        Self {
            database_type: "redis".to_string(),
            host: None,
            port: None,
            username: None,
            password: None,
            ssl: false,
            connection_timeout: 20,
            response_timeout: 20,
            number_of_retries: 100,
            exponent_base: 2,
            max_delay: 1000,
            factor: 2,
        }
    }
}

/// Configuration for `MessageBus` instances.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageBusConfig {
    /// The configuration for the message bus backing database.
    pub database: Option<DatabaseConfig>,
    /// The encoding for database operations, controls the type of serializer used.
    pub encoding: SerializationEncoding,
    /// If timestamps should be persisted as ISO 8601 strings.
    /// If `false`, then timestamps will be persisted as UNIX nanoseconds.
    pub timestamps_as_iso8601: bool,
    /// The buffer interval (milliseconds) between pipelined/batched transactions.
    /// The recommended range if using buffered pipelining is [10, 1000] milliseconds,
    /// with a good compromise being 100 milliseconds.
    pub buffer_interval_ms: Option<u32>,
    /// The lookback window in minutes for automatic stream trimming.
    /// The actual window may extend up to one minute beyond the specified value since streams are trimmed at most once every minute.
    /// This feature requires Redis version 6.2 or higher; otherwise, it will result in a command syntax error.
    pub autotrim_mins: Option<u32>,
    /// If a 'trader-' prefix is used for stream names.
    pub use_trader_prefix: bool,
    /// If the trader's ID is used for stream names.
    pub use_trader_id: bool,
    /// If the trader's instance ID is used for stream names. Default is `false`.
    pub use_instance_id: bool,
    /// The prefix for externally published stream names. Must have a `database` config.
    pub streams_prefix: String,
    /// If `true`, messages will be written to separate streams per topic.
    /// If `false`, all messages will be written to the same stream.
    pub stream_per_topic: bool,
    /// The external stream keys the message bus will listen to for publishing deserialized message payloads internally.
    pub external_streams: Option<Vec<String>>,
    /// A list of serializable types **not** to publish externally.
    pub types_filter: Option<Vec<String>>,
    /// The heartbeat interval (seconds).
    pub heartbeat_interval_secs: Option<u16>,
}

impl Default for MessageBusConfig {
    /// Creates a new default [`MessageBusConfig`] instance.
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
            heartbeat_interval_secs: None,
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
    fn is_closed(&self) -> bool;
    fn publish(&self, topic: String, payload: Bytes);
    fn close(&mut self);
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
        assert_eq!(config.connection_timeout, 20);
        assert_eq!(config.response_timeout, 20);
        assert_eq!(config.number_of_retries, 100);
        assert_eq!(config.exponent_base, 2);
        assert_eq!(config.max_delay, 1000);
        assert_eq!(config.factor, 2);
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
            "connection_timeout": 30,
            "response_timeout": 10,
            "number_of_retries": 3,
            "exponent_base": 2,
            "max_delay": 10,
            "factor": 2
        });
        let config: DatabaseConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.database_type, "redis");
        assert_eq!(config.host, Some("localhost".to_string()));
        assert_eq!(config.port, Some(6379));
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
        assert!(config.ssl);
        assert_eq!(config.connection_timeout, 30);
        assert_eq!(config.response_timeout, 10);
        assert_eq!(config.number_of_retries, 3);
        assert_eq!(config.exponent_base, 2);
        assert_eq!(config.max_delay, 10);
        assert_eq!(config.factor, 2);
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
                "connection_timeout": 30,
                "response_timeout": 10,
                "number_of_retries": 3,
                "exponent_base": 2,
                "max_delay": 10,
                "factor": 2
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
