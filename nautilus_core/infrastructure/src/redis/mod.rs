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

use std::{collections::HashMap, time::Duration};

use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use redis::*;
use semver::Version;
use serde_json::{json, Value};
use tracing::{debug, info};

const REDIS_MIN_VERSION: &str = "6.2.0";
const REDIS_DELIMITER: char = ':';

pub fn get_redis_url(database_config: &serde_json::Value) -> (String, String) {
    let host = database_config
        .get("host")
        .and_then(|v| v.as_str())
        .unwrap_or("127.0.0.1");
    let port = database_config
        .get("port")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(6379);
    let username = database_config
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let password = database_config
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let use_ssl = database_config
        .get("ssl")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

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

pub fn create_redis_connection(database_config: &serde_json::Value) -> anyhow::Result<Connection> {
    let (redis_url, redacted_url) = get_redis_url(database_config);
    debug!("Connecting to {redacted_url}");
    let default_timeout = 20;
    let timeout = get_timeout_duration(database_config, default_timeout);
    let client = redis::Client::open(redis_url)?;
    let mut conn = client.get_connection_with_timeout(timeout)?;

    let redis_version = get_redis_version(&mut conn)?;
    let conn_msg = format!("Connected to redis v{redis_version}");
    let version = Version::parse(&redis_version)?;
    let min_version = Version::parse(REDIS_MIN_VERSION)?;

    if version >= min_version {
        info!(conn_msg);
    } else {
        // TODO: Using `log` error here so that the message is displayed regardless of whether
        // the logging config has pyo3 enabled. Later we can standardize this to `tracing`.
        log::error!("{conn_msg}, but minimum supported verson {REDIS_MIN_VERSION}");
    };

    Ok(conn)
}

pub fn get_timeout_duration(database_config: &serde_json::Value, default: u64) -> Duration {
    let timeout_seconds = database_config
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(default);
    Duration::from_secs(timeout_seconds)
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
        stream_name.push_str(trader_id.as_str());
        stream_name.push(REDIS_DELIMITER);
    }

    if let Some(json!(true)) = config.get("use_instance_id") {
        stream_name.push_str(&format!("{instance_id}"));
        stream_name.push(REDIS_DELIMITER);
    }

    let stream_prefix = config
        .get("streams_prefix")
        .expect("Invalid configuration: no `streams_prefix` key found")
        .as_str()
        .expect("Invalid configuration: `streams_prefix` is not a string");
    stream_name.push_str(stream_prefix);
    stream_name.push(REDIS_DELIMITER);
    stream_name
}

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
    Err(anyhow::anyhow!("Redis version not found in info"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_get_redis_url_default_values() {
        let config = json!({});
        let (url, redacted_url) = get_redis_url(&config);
        assert_eq!(url, "redis://127.0.0.1:6379");
        assert_eq!(redacted_url, "redis://127.0.0.1:6379");
    }

    #[rstest]
    fn test_get_redis_url_full_config_with_ssl() {
        let config = json!({
            "host": "example.com",
            "port": 6380,
            "username": "user",
            "password": "pass",
            "ssl": true,
        });
        let (url, redacted_url) = get_redis_url(&config);
        assert_eq!(url, "rediss://user:pass@example.com:6380");
        assert_eq!(redacted_url, "rediss://user:pass@example.com:6380");
    }

    #[rstest]
    fn test_get_redis_url_full_config_without_ssl() {
        let config = json!({
            "host": "example.com",
            "port": 6380,
            "username": "username",
            "password": "password",
            "ssl": false,
        });
        let (url, redacted_url) = get_redis_url(&config);
        assert_eq!(url, "redis://username:password@example.com:6380");
        assert_eq!(redacted_url, "redis://username:pa...rd@example.com:6380");
    }

    #[rstest]
    fn test_get_redis_url_missing_username_and_password() {
        let config = json!({
            "host": "example.com",
            "port": 6380,
            "ssl": false,
        });
        let (url, redacted_url) = get_redis_url(&config);
        assert_eq!(url, "redis://example.com:6380");
        assert_eq!(redacted_url, "redis://example.com:6380");
    }

    #[rstest]
    fn test_get_redis_url_ssl_default_false() {
        let config = json!({
            "host": "example.com",
            "port": 6380,
            "username": "username",
            "password": "password",
            // "ssl" is intentionally omitted to test default behavior
        });
        let (url, redacted_url) = get_redis_url(&config);
        assert_eq!(url, "redis://username:password@example.com:6380");
        assert_eq!(redacted_url, "redis://username:pa...rd@example.com:6380");
    }

    #[rstest]
    fn test_get_timeout_duration_default() {
        let database_config = json!({});

        let timeout_duration = get_timeout_duration(&database_config, 20);
        assert_eq!(timeout_duration, Duration::from_secs(20));
    }

    #[rstest]
    fn test_get_timeout_duration() {
        let mut database_config = HashMap::new();
        database_config.insert("timeout".to_string(), json!(2));

        let timeout_duration = get_timeout_duration(&json!(database_config), 20);
        assert_eq!(timeout_duration, Duration::from_secs(2));
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
}
