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

//! Provides a Redis backed `CacheDatabase` and `MessageBusDatabase` implementation.

pub mod cache;
pub mod msgbus;

use std::time::Duration;

use nautilus_common::msgbus::database::{DatabaseConfig, MessageBusConfig};
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::TraderId;
use redis::*;
use semver::Version;

const REDIS_MIN_VERSION: &str = "6.2.0";
const REDIS_DELIMITER: char = ':';
const REDIS_XTRIM: &str = "XTRIM";
const REDIS_MINID: &str = "MINID";
const REDIS_FLUSHDB: &str = "FLUSHDB";

/// Parse a Redis connection url from the given database config.
pub fn get_redis_url(config: DatabaseConfig) -> (String, String) {
    let host = config.host.unwrap_or("127.0.0.1".to_string());
    let port = config.port.unwrap_or(6379);
    let username = config.username.unwrap_or("".to_string());
    let password = config.password.unwrap_or("".to_string());
    let use_ssl = config.ssl;

    let redacted_password = if password.len() > 4 {
        format!("{}...{}", &password[..2], &password[password.len() - 2..],)
    } else {
        password.to_string()
    };

    let auth_part = if !username.is_empty() && !password.is_empty() {
        format!("{}:{}@", username, password)
    } else {
        String::new()
    };

    let redacted_auth_part = if !username.is_empty() && !password.is_empty() {
        format!("{}:{}@", username, redacted_password)
    } else {
        String::new()
    };

    let url = format!(
        "redis{}://{}{}:{}",
        if use_ssl { "s" } else { "" },
        auth_part,
        host,
        port
    );

    let redacted_url = format!(
        "redis{}://{}{}:{}",
        if use_ssl { "s" } else { "" },
        redacted_auth_part,
        host,
        port
    );

    (url, redacted_url)
}

/// Create a new Redis database connection from the given database config.
pub fn create_redis_connection(
    con_name: &str,
    config: DatabaseConfig,
) -> anyhow::Result<redis::Connection> {
    tracing::debug!("Creating {con_name} redis connection");
    let (redis_url, redacted_url) = get_redis_url(config.clone());
    tracing::debug!("Connecting to {redacted_url}");
    let timeout = Duration::from_secs(config.timeout as u64);
    let client = redis::Client::open(redis_url)?;
    let mut con = client.get_connection_with_timeout(timeout)?;

    let redis_version = get_redis_version(&mut con)?;
    let con_msg = format!("Connected to redis v{redis_version}");
    let version = Version::parse(&redis_version)?;
    let min_version = Version::parse(REDIS_MIN_VERSION)?;

    if version >= min_version {
        tracing::info!(con_msg);
    } else {
        // TODO: Using `log` error here so that the message is displayed regardless of whether
        // the logging config has pyo3 enabled. Later we can standardize this to `tracing`.
        log::error!("{con_msg}, but minimum supported verson {REDIS_MIN_VERSION}");
    };

    Ok(con)
}

/// Flush the Redis database for the given connection.
pub fn flush_redis(con: &mut redis::Connection) -> anyhow::Result<(), RedisError> {
    redis::cmd(REDIS_FLUSHDB).exec(con)
}

/// Parse the stream key from the given identifiers and config.
pub fn get_stream_key(
    trader_id: TraderId,
    instance_id: UUID4,
    config: &MessageBusConfig,
) -> String {
    let mut stream_key = String::new();

    if config.use_trader_prefix {
        stream_key.push_str("trader-");
    }

    if config.use_trader_id {
        stream_key.push_str(trader_id.as_str());
        stream_key.push(REDIS_DELIMITER);
    }

    if config.use_instance_id {
        stream_key.push_str(&format!("{instance_id}"));
        stream_key.push(REDIS_DELIMITER);
    }

    stream_key.push_str(&config.streams_prefix);
    stream_key
}

/// Parse the Redis database version with the given connection.
pub fn get_redis_version(conn: &mut Connection) -> anyhow::Result<String> {
    let info: String = redis::cmd("INFO").query(conn)?;
    parse_redis_version(&info)
}

fn parse_redis_version(info: &str) -> anyhow::Result<String> {
    for line in info.lines() {
        if line.starts_with("redis_version:") {
            let version = line
                .split(':')
                .nth(1)
                .ok_or(anyhow::anyhow!("Version not found"))?;
            return Ok(version.trim().to_string());
        }
    }
    Err(anyhow::anyhow!("redis version not found in info"))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_get_redis_url_default_values() {
        let config: DatabaseConfig = serde_json::from_value(json!({})).unwrap();
        let (url, redacted_url) = get_redis_url(config);
        assert_eq!(url, "redis://127.0.0.1:6379");
        assert_eq!(redacted_url, "redis://127.0.0.1:6379");
    }

    #[rstest]
    fn test_get_redis_url_full_config_with_ssl() {
        let config_json = json!({
            "host": "example.com",
            "port": 6380,
            "username": "user",
            "password": "pass",
            "ssl": true,
        });
        let config: DatabaseConfig = serde_json::from_value(config_json).unwrap();
        let (url, redacted_url) = get_redis_url(config);
        assert_eq!(url, "rediss://user:pass@example.com:6380");
        assert_eq!(redacted_url, "rediss://user:pass@example.com:6380");
    }

    #[rstest]
    fn test_get_redis_url_full_config_without_ssl() {
        let config_json = json!({
            "host": "example.com",
            "port": 6380,
            "username": "username",
            "password": "password",
            "ssl": false,
        });
        let config: DatabaseConfig = serde_json::from_value(config_json).unwrap();
        let (url, redacted_url) = get_redis_url(config);
        assert_eq!(url, "redis://username:password@example.com:6380");
        assert_eq!(redacted_url, "redis://username:pa...rd@example.com:6380");
    }

    #[rstest]
    fn test_get_redis_url_missing_username_and_password() {
        let config_json = json!({
            "host": "example.com",
            "port": 6380,
            "ssl": false,
        });
        let config: DatabaseConfig = serde_json::from_value(config_json).unwrap();
        let (url, redacted_url) = get_redis_url(config);
        assert_eq!(url, "redis://example.com:6380");
        assert_eq!(redacted_url, "redis://example.com:6380");
    }

    #[rstest]
    fn test_get_redis_url_ssl_default_false() {
        let config_json = json!({
            "host": "example.com",
            "port": 6380,
            "username": "username",
            "password": "password",
            // "ssl" is intentionally omitted to test default behavior
        });
        let config: DatabaseConfig = serde_json::from_value(config_json).unwrap();
        let (url, redacted_url) = get_redis_url(config);
        assert_eq!(url, "redis://username:password@example.com:6380");
        assert_eq!(redacted_url, "redis://username:pa...rd@example.com:6380");
    }

    #[rstest]
    fn test_get_stream_key_with_trader_prefix_and_instance_id() {
        let trader_id = TraderId::from("tester-123");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.use_instance_id = true;

        let key = get_stream_key(trader_id, instance_id, &config);
        assert_eq!(key, format!("trader-tester-123:{instance_id}:stream"));
    }

    #[rstest]
    fn test_get_stream_key_without_trader_prefix_or_instance_id() {
        let trader_id = TraderId::from("tester-123");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.use_trader_prefix = false;
        config.use_trader_id = false;

        let key = get_stream_key(trader_id, instance_id, &config);
        assert_eq!(key, format!("stream"));
    }
}
