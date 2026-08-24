// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Static transport, framing, heartbeat, and reconnect configuration for TCP sockets.
//!
//! # Reconnection strategy
//!
//! The default configuration uses unlimited reconnection attempts (`reconnect_max_attempts: None`).
//! This suits long-lived trading connections because:
//!
//! - Venues may remain unavailable for an extended period and later recover.
//! - Exponential backoff bounds retry frequency during the outage.
//! - Automatic recovery avoids requiring manual intervention for a transient failure.
//!
//! A connection active for at least 10 seconds resets the attempt count and backoff delay.
//! Shorter-lived connections remain part of the same reconnect cycle. Use `Some(n)` primarily for
//! tests, development, or connections that should stop retrying without intervention.

use std::fmt::Debug;

use tokio_tungstenite::tungstenite::stream::Mode;

use super::types::TcpMessageHandler;
use crate::error::{NetworkConfigError, NetworkConfigResult};

/// Application keepalive for a raw TCP socket.
///
/// A raw socket has no control frames, so [`Self::payload`] is required. WebSocket keepalives stay
/// on [`crate::websocket::WebSocketConfig`] and may omit a payload to send a protocol Ping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketHeartbeat {
    /// Interval between keepalives, in seconds.
    ///
    /// Must be positive.
    pub interval_secs: u64,
    /// Bytes sent as each heartbeat, framed with [`SocketConfig::suffix`].
    pub payload: Vec<u8>,
}

/// Configuration for a TCP socket connection.
#[derive(bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
pub struct SocketConfig {
    /// The server address as `host:port` or a URL.
    pub url: String,
    /// The plain or TLS connection mode.
    pub mode: Mode,
    /// The byte sequence that frames messages in both directions.
    pub suffix: Vec<u8>,
    /// The function called for each complete incoming message.
    pub message_handler: Option<TcpMessageHandler>,
    /// Optional application keepalive.
    ///
    /// When set, the client sends [`SocketHeartbeat::payload`] every
    /// [`SocketHeartbeat::interval_secs`] seconds. A raw socket has no Ping frames, so this type
    /// requires a payload; the WebSocket Ping-frame case stays on
    /// [`crate::websocket::WebSocketConfig::heartbeat_payload`].
    ///
    /// Each timing field carries the coarsest unit that expresses every legitimate value, and
    /// quantities compared against each other share a unit: the interval and
    /// [`Self::heartbeat_timeout_secs`] are bounded below by whole-second cadences, while reconnect
    /// delays and jitter have real sub-second values and stay in milliseconds.
    pub heartbeat: Option<SocketHeartbeat>,
    /// The timeout (milliseconds) for establishing a usable connection. Defaults to 10 seconds.
    ///
    /// Bounds the initial connection attempt, each reconnect attempt, and how long a send waits for
    /// the client to become active again. Keep it above the reconnect backoff so a send does not
    /// give up part-way through a normal reconnect.
    pub connect_timeout_ms: Option<u64>,
    /// The initial reconnection delay (milliseconds) for reconnects.
    pub reconnect_delay_initial_ms: Option<u64>,
    /// The maximum reconnect delay (milliseconds) for exponential backoff.
    pub reconnect_delay_max_ms: Option<u64>,
    /// The exponential backoff factor for reconnection delays.
    pub reconnect_backoff_factor: Option<f64>,
    /// The maximum jitter (milliseconds) added to reconnection delays.
    pub reconnect_jitter_ms: Option<u64>,
    /// The maximum number of initial connection attempts. Defaults to 5.
    pub connection_max_retries: Option<u32>,
    /// The maximum number of reconnection attempts before closing the client.
    ///
    /// - `None`: Unlimited reconnection attempts (default, recommended for production).
    /// - `Some(n)`: Transitions to CLOSED once `n` consecutive reconnect attempts have either
    ///   failed or established connections active for less than 10 seconds.
    pub reconnect_max_attempts: Option<u32>,
    /// The dead-peer timeout (seconds) for the read task.
    ///
    /// Seconds rather than milliseconds because this is a multiple of the heartbeat interval: it
    /// can never sensibly sit below one heartbeat cycle.
    ///
    /// When set, the read task stops and triggers reconnection if no bytes at all arrive within
    /// this duration. A raw socket has no control frames, so any inbound byte refreshes it,
    /// including the venue's reply to a heartbeat. That makes this the byte-level equivalent of
    /// the WebSocket client's `heartbeat_timeout_secs`, not of its `idle_timeout_ms`: there is
    /// no transport-level way to tell keepalive traffic from data here.
    ///
    /// `None` derives three heartbeat intervals when [`Self::heartbeat`] is set, and disables
    /// detection otherwise. `Some(0)` is rejected. Set an explicit value above the heartbeat
    /// interval so a healthy connection cannot trip it.
    pub heartbeat_timeout_secs: Option<u64>,
    /// The path to the certificates directory.
    pub certs_dir: Option<String>,
}

impl<S: socket_config_builder::IsComplete> SocketConfigBuilder<S> {
    /// Validates and builds the [`SocketConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`NetworkConfigError`] if any field fails validation
    /// (see [`SocketConfig::validate`]).
    pub fn build(self) -> NetworkConfigResult<SocketConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl SocketConfig {
    /// Checks whether all socket settings are valid.
    ///
    /// # Errors
    ///
    /// Returns a [`NetworkConfigError`] if `url` is empty, the heartbeat interval or a
    /// reconnection timing field is not positive, `reconnect_backoff_factor` is outside
    /// `[1.0, 100.0]`, or `reconnect_delay_initial_ms` exceeds `reconnect_delay_max_ms`.
    pub fn validate(&self) -> NetworkConfigResult<()> {
        let mut errors = Vec::new();

        if self.url.trim().is_empty() {
            errors.push(NetworkConfigError::invalid("url", "must not be empty"));
        }

        if let Some(heartbeat) = &self.heartbeat
            && heartbeat.interval_secs == 0
        {
            errors.push(NetworkConfigError::invalid(
                "heartbeat",
                "interval must be positive",
            ));
        }

        // A timeout at or below the send cadence tears every connection down before its first
        // reply is due, so a healthy socket would reconnect forever.
        if let (Some(heartbeat), Some(timeout_secs)) =
            (&self.heartbeat, self.heartbeat_timeout_secs)
            && timeout_secs <= heartbeat.interval_secs
        {
            errors.push(NetworkConfigError::invalid(
                "heartbeat_timeout_secs",
                format!(
                    "must exceed heartbeat interval ({}s), was {timeout_secs}s",
                    heartbeat.interval_secs
                ),
            ));
        }

        // `reconnect_jitter_ms` is intentionally unchecked: zero disables jitter and
        // `ExponentialBackoff::new` accepts it.
        for (field, value) in [
            ("connect_timeout_ms", self.connect_timeout_ms),
            (
                "reconnect_delay_initial_ms",
                self.reconnect_delay_initial_ms,
            ),
            ("reconnect_delay_max_ms", self.reconnect_delay_max_ms),
            ("heartbeat_timeout_secs", self.heartbeat_timeout_secs),
        ] {
            if let Some(value) = value
                && value == 0
            {
                errors.push(NetworkConfigError::invalid(
                    field,
                    format!("must be positive, was {value}"),
                ));
            }
        }

        if let Some(factor) = self.reconnect_backoff_factor
            && !(1.0..=100.0).contains(&factor)
        {
            errors.push(NetworkConfigError::invalid(
                "reconnect_backoff_factor",
                format!("must be in range [1.0, 100.0], was {factor}"),
            ));
        }

        if let (Some(initial), Some(max)) =
            (self.reconnect_delay_initial_ms, self.reconnect_delay_max_ms)
            && initial > max
        {
            errors.push(NetworkConfigError::invalid(
                "reconnect_delay_initial_ms",
                format!("must not exceed reconnect_delay_max_ms ({max}), was {initial}"),
            ));
        }

        NetworkConfigError::collect(errors)
    }

    pub(crate) fn resolved_heartbeat_timeout(&self) -> Option<u64> {
        crate::heartbeat::resolve_heartbeat_timeout(
            self.heartbeat_timeout_secs,
            self.heartbeat
                .as_ref()
                .map(|heartbeat| heartbeat.interval_secs),
        )
    }
}

impl Debug for SocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketConfig))
            .field("url", &self.url)
            .field("mode", &self.mode)
            .field("suffix", &self.suffix)
            .field(
                "message_handler",
                &self.message_handler.as_ref().map(|_| "<function>"),
            )
            .field("heartbeat", &self.heartbeat)
            .field("connect_timeout_ms", &self.connect_timeout_ms)
            .field(
                "reconnect_delay_initial_ms",
                &self.reconnect_delay_initial_ms,
            )
            .field("reconnect_delay_max_ms", &self.reconnect_delay_max_ms)
            .field("reconnect_backoff_factor", &self.reconnect_backoff_factor)
            .field("reconnect_jitter_ms", &self.reconnect_jitter_ms)
            .field("connection_max_retries", &self.connection_max_retries)
            .field("reconnect_max_attempts", &self.reconnect_max_attempts)
            .field("heartbeat_timeout_secs", &self.heartbeat_timeout_secs)
            .field("certs_dir", &self.certs_dir)
            .finish()
    }
}

impl Clone for SocketConfig {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            mode: self.mode,
            suffix: self.suffix.clone(),
            message_handler: self.message_handler.clone(),
            heartbeat: self.heartbeat.clone(),
            connect_timeout_ms: self.connect_timeout_ms,
            reconnect_delay_initial_ms: self.reconnect_delay_initial_ms,
            reconnect_delay_max_ms: self.reconnect_delay_max_ms,
            reconnect_backoff_factor: self.reconnect_backoff_factor,
            reconnect_jitter_ms: self.reconnect_jitter_ms,
            connection_max_retries: self.connection_max_retries,
            reconnect_max_attempts: self.reconnect_max_attempts,
            heartbeat_timeout_secs: self.heartbeat_timeout_secs,
            certs_dir: self.certs_dir.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tokio_tungstenite::tungstenite::stream::Mode;

    use super::{SocketConfig, SocketHeartbeat};
    use crate::error::NetworkConfigError;

    fn valid_config() -> SocketConfig {
        SocketConfig::builder()
            .url("tcp://127.0.0.1:8080".to_string())
            .mode(Mode::Plain)
            .suffix(vec![b'\n'])
            .build()
            .expect("baseline socket config should be valid")
    }

    #[rstest]
    fn test_builder_accepts_valid_config() {
        let result = SocketConfig::builder()
            .url("tcp://127.0.0.1:8080".to_string())
            .mode(Mode::Plain)
            .suffix(vec![b'\n'])
            .build();

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_validate_accepts_zero_jitter() {
        let mut config = valid_config();
        config.reconnect_jitter_ms = Some(0);

        assert!(config.validate().is_ok());
    }

    #[rstest]
    fn test_validate_accepts_heartbeat_with_payload() {
        let mut config = valid_config();
        config.heartbeat = Some(SocketHeartbeat {
            interval_secs: 5,
            payload: b"ping".to_vec(),
        });

        assert!(config.validate().is_ok());
    }

    #[rstest]
    #[case::derived(None, Some(15))]
    #[case::explicit_wins(Some(20), Some(20))]
    fn test_resolve_timeout_from_socket_heartbeat(
        #[case] timeout_secs: Option<u64>,
        #[case] expected: Option<u64>,
    ) {
        let mut config = valid_config();
        config.heartbeat = Some(SocketHeartbeat {
            interval_secs: 5,
            payload: b"ping".to_vec(),
        });
        config.heartbeat_timeout_secs = timeout_secs;

        assert_eq!(config.resolved_heartbeat_timeout(), expected);
    }

    #[rstest]
    #[case::empty_url(|c: &mut SocketConfig| c.url = String::new(), "url")]
    #[case::heartbeat_interval(|c: &mut SocketConfig| { c.heartbeat = Some(SocketHeartbeat { interval_secs: 0, payload: vec![] }); }, "heartbeat")]
    #[case::heartbeat_timeout_below_interval(|c: &mut SocketConfig| { c.heartbeat = Some(SocketHeartbeat { interval_secs: 5, payload: vec![b'p'] }); c.heartbeat_timeout_secs = Some(5); }, "heartbeat_timeout_secs")]
    #[case::connect_timeout(|c: &mut SocketConfig| c.connect_timeout_ms = Some(0), "connect_timeout_ms")]
    #[case::reconnect_delay_initial(|c: &mut SocketConfig| c.reconnect_delay_initial_ms = Some(0), "reconnect_delay_initial_ms")]
    #[case::reconnect_delay_max(|c: &mut SocketConfig| c.reconnect_delay_max_ms = Some(0), "reconnect_delay_max_ms")]
    #[case::heartbeat_timeout_zero(|c: &mut SocketConfig| c.heartbeat_timeout_secs = Some(0), "heartbeat_timeout_secs")]
    fn test_validate_rejects_invalid_field(
        #[case] mutate: fn(&mut SocketConfig),
        #[case] expected_field: &str,
    ) {
        let mut config = valid_config();
        mutate(&mut config);

        let err = config
            .validate()
            .expect_err("invalid value should be rejected");

        assert!(
            matches!(err, NetworkConfigError::Invalid { field, .. } if field == expected_field)
        );
    }

    #[rstest]
    #[case::too_small(0.5)]
    #[case::too_large(100.1)]
    #[case::nan(f64::NAN)]
    #[case::infinite(f64::INFINITY)]
    fn test_validate_rejects_invalid_backoff_factor(#[case] factor: f64) {
        let mut config = valid_config();
        config.reconnect_backoff_factor = Some(factor);

        let err = config
            .validate()
            .expect_err("invalid backoff factor should be rejected");

        assert!(
            matches!(err, NetworkConfigError::Invalid { field, .. } if field == "reconnect_backoff_factor")
        );
    }

    #[rstest]
    fn test_validate_rejects_delay_initial_exceeding_max() {
        let mut config = valid_config();
        config.reconnect_delay_initial_ms = Some(5_000);
        config.reconnect_delay_max_ms = Some(1_000);

        let err = config
            .validate()
            .expect_err("initial delay above max should be rejected");

        assert!(
            matches!(err, NetworkConfigError::Invalid { field, .. } if field == "reconnect_delay_initial_ms")
        );
    }

    #[rstest]
    fn test_validate_collects_multiple_errors() {
        let mut config = valid_config();
        config.url = String::new();
        config.connect_timeout_ms = Some(0);

        let err = config.validate().expect_err("multiple invalid fields");

        match err {
            NetworkConfigError::Multiple { errors } => assert_eq!(errors.len(), 2),
            other @ NetworkConfigError::Invalid { .. } => {
                panic!("expected Multiple, was {other:?}")
            }
        }
    }
}
