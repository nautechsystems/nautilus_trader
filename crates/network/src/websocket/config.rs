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

//! Static transport and lifecycle configuration for WebSocket connections.
//!
//! [`WebSocketConfig`] selects the endpoint, upgrade headers, heartbeat and idle detection,
//! reconnect policy, transport backend, and optional proxy. Runtime handlers and rate limiting are
//! supplied to the client constructors instead.
//!
//! # Reconnection strategy
//!
//! Reconnect settings apply only in handler mode; stream mode ignores them.
//! `reconnect_max_attempts: None` permits unlimited attempts with exponential backoff, while
//! `Some(n)` closes the client once `n` consecutive reconnect attempts have either failed or
//! established connections active for less than 10 seconds. A reconnect active for at least 10
//! seconds resets its attempt count and backoff delay; shorter-lived connections continue the
//! current cycle.

use std::fmt::Debug;

use nautilus_core::string::secret::REDACTED;
use serde::{Deserialize, Serialize};

use crate::error::{NetworkConfigError, NetworkConfigResult};

/// WebSocket transport backend selection.
///
/// Selection is runtime so multiple backends can compile side-by-side without
/// a `compile_error!` collision under `--all-features`.
///
/// `Sockudo` is the default backend and is enabled by the `transport-sockudo`
/// Cargo feature (on by default); it uses a local HTTP/1.1 handshake path to
/// pass custom upgrade headers through. When the feature is disabled the
/// default falls back to `Tungstenite`, which is always compiled and supports
/// custom HTTP upgrade headers on the WebSocket handshake (see
/// [`WebSocketConfig::headers`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.network",
        eq,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.network")
)]
#[allow(
    clippy::unsafe_derive_deserialize,
    reason = "network configuration requires strict serde decoding"
)]
pub enum TransportBackend {
    /// `tokio-tungstenite` backed transport (default when `transport-sockudo` is disabled).
    #[cfg_attr(not(feature = "transport-sockudo"), default)]
    Tungstenite,
    /// `sockudo-ws` backed transport (default; gated on `transport-sockudo` feature).
    #[cfg_attr(feature = "transport-sockudo", default)]
    Sockudo,
}

/// Static configuration for WebSocket client connections.
///
/// Runtime handlers and rate limiters are passed separately to the client constructors.
///
/// # Connection modes
///
/// ## Handler mode
///
/// - Uses [`WebSocketClient::connect`](crate::websocket::WebSocketClient::connect).
/// - Delivers messages through the supplied callback.
/// - Runs the reader in an internal task.
/// - Supports automatic reconnection with exponential backoff.
/// - Applies `reconnect_*` and `idle_timeout_ms` settings.
/// - Suits long‑lived connections and callback‑based APIs.
///
/// ## Stream mode
///
/// - Uses [`WebSocketClient::connect_stream`](crate::websocket::WebSocketClient::connect_stream).
/// - Returns a [`MessageReader`](super::types::MessageReader) owned by the caller.
/// - Does not support automatic reconnection because the client cannot replace the caller's reader.
/// - Ignores `reconnect_*` and `idle_timeout_ms` settings.
/// - Enters the closed state after disconnection, requiring the caller to create a new connection.
#[allow(
    clippy::unsafe_derive_deserialize,
    reason = "network configuration requires strict serde decoding"
)]
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(deny_unknown_fields)]
pub struct WebSocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The default headers.
    #[serde(default)]
    #[builder(default)]
    pub headers: Vec<(String, String)>,
    /// The optional heartbeat interval (seconds).
    #[serde(default)]
    pub heartbeat: Option<u64>,
    /// The optional heartbeat message.
    #[serde(default)]
    pub heartbeat_msg: Option<String>,
    /// The timeout (milliseconds) for reconnection attempts.
    ///
    /// Only applies to handler mode and must be non‑zero when set. Stream mode ignores this
    /// field.
    #[serde(default)]
    pub reconnect_timeout_ms: Option<u64>,
    /// The initial reconnection delay (milliseconds) for reconnects.
    ///
    /// Only applies to handler mode. Stream mode ignores this field.
    #[serde(default)]
    pub reconnect_delay_initial_ms: Option<u64>,
    /// The maximum reconnect delay (milliseconds) for exponential backoff.
    ///
    /// Only applies to handler mode. Stream mode ignores this field.
    #[serde(default)]
    pub reconnect_delay_max_ms: Option<u64>,
    /// The exponential backoff factor for reconnection delays.
    ///
    /// Only applies to handler mode. Stream mode ignores this field.
    #[serde(default)]
    pub reconnect_backoff_factor: Option<f64>,
    /// The maximum jitter (milliseconds) added to reconnection delays.
    ///
    /// Only applies to handler mode. Stream mode ignores this field.
    #[serde(default)]
    pub reconnect_jitter_ms: Option<u64>,
    /// The maximum number of reconnection attempts before giving up.
    ///
    /// Only applies to handler mode. Stream mode ignores this field.
    ///
    /// - `None`: Unlimited reconnection attempts (default, recommended for production).
    /// - `Some(n)`: Transitions to CLOSED once `n` consecutive reconnect attempts have either
    ///   failed or established connections active for less than 10 seconds.
    #[serde(default)]
    pub reconnect_max_attempts: Option<u32>,
    /// The idle timeout (milliseconds) for the read task.
    ///
    /// When set, the read task stops and triggers reconnection if it receives no data within this
    /// duration. This detects silently dead connections where the server stops sending without
    /// closing the connection. Only applies to handler mode; stream mode ignores this field.
    #[serde(default)]
    pub idle_timeout_ms: Option<u64>,
    /// The transport backend to use for the WebSocket connection.
    ///
    /// Defaults to [`TransportBackend::Sockudo`] when the `transport-sockudo`
    /// Cargo feature is enabled (the default), otherwise [`TransportBackend::Tungstenite`].
    /// When the feature is disabled, `connect_with_server` returns an error if
    /// `Sockudo` is selected. Both backends pass `headers` into the HTTP
    /// upgrade request. The Sockudo backend does not yet support proxy tunnels;
    /// when [`Self::proxy_url`] is set, `connect_with_server` logs a warning
    /// and routes through Tungstenite regardless of this field.
    #[serde(default)]
    #[builder(default)]
    pub backend: TransportBackend,
    /// Optional forward proxy URL for the WebSocket connection.
    ///
    /// Routes the connection through an HTTP `CONNECT` tunnel. Accepts
    /// `http://` and `https://` schemes; SOCKS schemes are not yet supported.
    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl Debug for WebSocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketConfig))
            .field("url", &self.url)
            .field(
                "headers",
                &format_args!("<{} header(s)>", self.headers.len()),
            )
            .field("heartbeat", &self.heartbeat)
            .field("heartbeat_msg", &self.heartbeat_msg)
            .field("reconnect_timeout_ms", &self.reconnect_timeout_ms)
            .field(
                "reconnect_delay_initial_ms",
                &self.reconnect_delay_initial_ms,
            )
            .field("reconnect_delay_max_ms", &self.reconnect_delay_max_ms)
            .field("reconnect_backoff_factor", &self.reconnect_backoff_factor)
            .field("reconnect_jitter_ms", &self.reconnect_jitter_ms)
            .field("reconnect_max_attempts", &self.reconnect_max_attempts)
            .field("idle_timeout_ms", &self.idle_timeout_ms)
            .field("backend", &self.backend)
            .field("proxy_url", &self.proxy_url.as_ref().map(|_| REDACTED))
            .finish()
    }
}

impl<S: web_socket_config_builder::IsComplete> WebSocketConfigBuilder<S> {
    /// Validates and builds the [`WebSocketConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`NetworkConfigError`] if any field fails validation
    /// (see [`WebSocketConfig::validate`]).
    pub fn build(self) -> NetworkConfigResult<WebSocketConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl WebSocketConfig {
    /// Checks whether all WebSocket settings are valid.
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

        if let Some(interval) = self.heartbeat
            && interval == 0
        {
            errors.push(NetworkConfigError::invalid(
                "heartbeat",
                "interval must be positive",
            ));
        }

        // `reconnect_jitter_ms` is intentionally unchecked: zero disables jitter and
        // `ExponentialBackoff::new` accepts it.
        for (field, value) in [
            ("reconnect_timeout_ms", self.reconnect_timeout_ms),
            (
                "reconnect_delay_initial_ms",
                self.reconnect_delay_initial_ms,
            ),
            ("reconnect_delay_max_ms", self.reconnect_delay_max_ms),
            ("idle_timeout_ms", self.idle_timeout_ms),
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
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::WebSocketConfig;
    use crate::error::NetworkConfigError;

    #[rstest]
    fn test_deserialize_websocket_config_rejects_unknown_field() {
        let config = json!({
            "url": "wss://example.com/ws",
            "unexpected": true,
        });

        let error = serde_json::from_value::<WebSocketConfig>(config).unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    fn valid_config() -> WebSocketConfig {
        WebSocketConfig::builder()
            .url("wss://example.com/ws".to_string())
            .build()
            .expect("baseline websocket config should be valid")
    }

    #[rstest]
    fn test_builder_accepts_valid_config() {
        let result = WebSocketConfig::builder()
            .url("wss://example.com/ws".to_string())
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
    #[case::empty_url(|c: &mut WebSocketConfig| c.url = String::new(), "url")]
    #[case::heartbeat(|c: &mut WebSocketConfig| c.heartbeat = Some(0), "heartbeat")]
    #[case::reconnect_timeout(|c: &mut WebSocketConfig| c.reconnect_timeout_ms = Some(0), "reconnect_timeout_ms")]
    #[case::reconnect_delay_initial(|c: &mut WebSocketConfig| c.reconnect_delay_initial_ms = Some(0), "reconnect_delay_initial_ms")]
    #[case::reconnect_delay_max(|c: &mut WebSocketConfig| c.reconnect_delay_max_ms = Some(0), "reconnect_delay_max_ms")]
    #[case::idle_timeout(|c: &mut WebSocketConfig| c.idle_timeout_ms = Some(0), "idle_timeout_ms")]
    fn test_validate_rejects_invalid_field(
        #[case] mutate: fn(&mut WebSocketConfig),
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
        config.reconnect_timeout_ms = Some(0);

        let err = config.validate().expect_err("multiple invalid fields");

        match err {
            NetworkConfigError::Multiple { errors } => assert_eq!(errors.len(), 2),
            other @ NetworkConfigError::Invalid { .. } => {
                panic!("expected Multiple, was {other:?}")
            }
        }
    }

    #[rstest]
    fn test_debug_redacts_proxy_credentials() {
        const SECRET: &str = "unique-proxy-secret";
        let mut config = valid_config();
        config.proxy_url = Some(format!("http://proxytest:{SECRET}@proxy.example.com:8080"));

        let debug = format!("{config:?}");

        assert!(debug.contains("proxy_url: Some(\"<redacted>\")"));
        assert!(!debug.contains(SECRET));
    }
}
