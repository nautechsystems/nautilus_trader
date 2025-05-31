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

//! Provides a Redis backed `CacheDatabase` and `MessageBusDatabase` implementation.

pub mod cache;
pub mod msgbus;
pub mod queries;

use std::time::Duration;

use nautilus_common::{
    logging::log_task_awaiting,
    msgbus::database::{DatabaseConfig, MessageBusConfig},
};
use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;
use redis::RedisError;
use semver::Version;

const REDIS_MIN_VERSION: &str = "6.2.0";
const REDIS_DELIMITER: char = ':';
const REDIS_XTRIM: &str = "XTRIM";
const REDIS_MINID: &str = "MINID";
const REDIS_FLUSHDB: &str = "FLUSHDB";

async fn await_handle(handle: Option<tokio::task::JoinHandle<()>>, task_name: &str) {
    if let Some(handle) = handle {
        log_task_awaiting(task_name);

        let timeout = Duration::from_secs(2);
        match tokio::time::timeout(timeout, handle).await {
            Ok(result) => {
                if let Err(e) = result {
                    log::error!("Error awaiting task '{task_name}': {e:?}");
                }
            }
            Err(_) => {
                log::error!("Timeout {timeout:?} awaiting task '{task_name}'");
            }
        }
    }
}

/// Parses a Redis connection URL from the given database config, returning the
/// full URL and a redacted version with the password obfuscated.
///
/// Authentication matrix handled:
/// ┌───────────┬───────────┬────────────────────────────┐
/// │ Username  │ Password  │ Resulting user-info part   │
/// ├───────────┼───────────┼────────────────────────────┤
/// │ non-empty │ non-empty │ user:pass@                 │
/// │ empty     │ non-empty │ :pass@                     │
/// │ empty     │ empty     │ (omitted)                  │
/// └───────────┴───────────┴────────────────────────────┘
///
/// # Panics
///
/// Panics if a username is provided without a corresponding password.
#[must_use]
pub fn get_redis_url(config: DatabaseConfig) -> (String, String) {
    let host = config.host.unwrap_or("127.0.0.1".to_string());
    let port = config.port.unwrap_or(6379);
    let username = config.username.unwrap_or_default();
    let password = config.password.unwrap_or_default();
    let ssl = config.ssl;

    // Redact the password for logging/metrics: keep the first & last two chars.
    let redact_pw = |pw: &str| {
        if pw.len() > 4 {
            format!("{}...{}", &pw[..2], &pw[pw.len() - 2..])
        } else {
            pw.to_owned()
        }
    };

    // Build the `userinfo@` portion for both the real and redacted URLs.
    let (auth, auth_redacted) = match (username.is_empty(), password.is_empty()) {
        // user:pass@
        (false, false) => (
            format!("{username}:{password}@"),
            format!("{username}:{}@", redact_pw(&password)),
        ),
        // :pass@
        (true, false) => (
            format!(":{password}@"),
            format!(":{}@", redact_pw(&password)),
        ),
        // username but no password ⇒  configuration error
        (false, true) => panic!(
            "Redis config error: username supplied without password. \
            Either supply a password or omit the username."
        ),
        // no credentials
        (true, true) => (String::new(), String::new()),
    };

    let scheme = if ssl { "rediss" } else { "redis" };

    let url = format!("{scheme}://{auth}{host}:{port}");
    let redacted_url = format!("{scheme}://{auth_redacted}{host}:{port}");

    (url, redacted_url)
}
/// Creates a new Redis connection manager based on the provided database `config` and connection name.
///
/// # Errors
///
/// Returns an error if:
/// - Constructing the Redis client fails.
/// - Establishing or configuring the connection manager fails.
///
/// In case of reconnection issues, the connection will retry reconnection
/// `number_of_retries` times, with an exponentially increasing delay, calculated as
/// `rand(0 .. factor * (exponent_base ^ current-try))`.
///
/// The new connection will time out operations after `response_timeout` has passed.
/// Each connection attempt to the server will time out after `connection_timeout`.
pub async fn create_redis_connection(
    con_name: &str,
    config: DatabaseConfig,
) -> anyhow::Result<redis::aio::ConnectionManager> {
    tracing::debug!("Creating {con_name} redis connection");
    let (redis_url, redacted_url) = get_redis_url(config.clone());
    tracing::debug!("Connecting to {redacted_url}");

    let connection_timeout = Duration::from_secs(u64::from(config.connection_timeout));
    let response_timeout = Duration::from_secs(u64::from(config.response_timeout));
    let number_of_retries = config.number_of_retries;
    let exponent_base = config.exponent_base;
    let factor = config.factor;

    // into milliseconds
    let max_delay = config.max_delay * 1000;

    let client = redis::Client::open(redis_url)?;

    let connection_manager_config = redis::aio::ConnectionManagerConfig::new()
        .set_exponent_base(exponent_base)
        .set_factor(factor)
        .set_number_of_retries(number_of_retries)
        .set_response_timeout(response_timeout)
        .set_connection_timeout(connection_timeout)
        .set_max_delay(max_delay);

    let mut con = client
        .get_connection_manager_with_config(connection_manager_config)
        .await?;

    let version = get_redis_version(&mut con).await?;
    let min_version = Version::parse(REDIS_MIN_VERSION)?;
    let con_msg = format!("Connected to redis v{version}");

    if version >= min_version {
        tracing::info!(con_msg);
    } else {
        // TODO: Using `log` error here so that the message is displayed regardless of whether
        // the logging config has pyo3 enabled. Later we can standardize this to `tracing`.
        log::error!("{con_msg}, but minimum supported version is {REDIS_MIN_VERSION}");
    }

    Ok(con)
}

/// Flushes the entire Redis database for the specified connection.
///
/// # Errors
///
/// Returns an error if the FLUSHDB command fails.
pub async fn flush_redis(
    con: &mut redis::aio::ConnectionManager,
) -> anyhow::Result<(), RedisError> {
    redis::cmd(REDIS_FLUSHDB).exec_async(con).await
}

/// Parse the stream key from the given identifiers and config.
#[must_use]
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

/// Retrieves and parses the Redis server version via the INFO command.
///
/// # Errors
///
/// Returns an error if the INFO command fails or version parsing fails.
pub async fn get_redis_version(
    conn: &mut redis::aio::ConnectionManager,
) -> anyhow::Result<Version> {
    let info: String = redis::cmd("INFO").query_async(conn).await?;
    let version_str = match info.lines().find_map(|line| {
        if line.starts_with("redis_version:") {
            line.split(':').nth(1).map(|s| s.trim().to_string())
        } else {
            None
        }
    }) {
        Some(info) => info,
        None => {
            anyhow::bail!("Redis version not available");
        }
    };

    parse_redis_version(&version_str)
}

fn parse_redis_version(version_str: &str) -> anyhow::Result<Version> {
    let mut components = version_str.split('.').map(str::parse::<u64>);

    let major = components.next().unwrap_or(Ok(0))?;
    let minor = components.next().unwrap_or(Ok(0))?;
    let patch = components.next().unwrap_or(Ok(0))?;

    Ok(Version::new(major, minor, patch))
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
    fn test_get_redis_url_password_only() {
        // Username omitted, but password present
        let config_json = json!({
            "host": "example.com",
            "port": 6380,
            "password": "secretpw",   // >4 chars ⇒ will be redacted
        });
        let config: DatabaseConfig = serde_json::from_value(config_json).unwrap();
        let (url, redacted_url) = get_redis_url(config);
        assert_eq!(url, "redis://:secretpw@example.com:6380");
        assert_eq!(redacted_url, "redis://:se...pw@example.com:6380");
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
