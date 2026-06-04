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

//! Configuration structures for the Derive adapter.

use std::fmt::Debug;

use nautilus_network::websocket::TransportBackend;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::common::{enums::DeriveEnvironment, urls};

/// Configuration for the Derive data client.
#[derive(Clone, Debug, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.derive", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.derive")
)]
pub struct DeriveDataClientConfig {
    /// Override for the REST API base URL.
    pub base_url_rest: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The Derive environment to connect to.
    #[builder(default)]
    pub environment: DeriveEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
    pub http_timeout_secs: u64,
    /// WebSocket timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// Underlying currencies to load on connect. Empty means lazy-load by
    /// instrument ID when subscribing.
    #[builder(default)]
    pub currencies: Vec<String>,
    /// Whether instrument loading includes expired instruments.
    #[builder(default)]
    pub include_expired: bool,
    /// Whether subscriptions may fetch missing instruments before sending the
    /// WebSocket request.
    #[builder(default = true)]
    pub auto_load_missing_instruments: bool,
    /// WebSocket transport backend (defaults to `Sockudo` when that feature is enabled).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for DeriveDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl DeriveDataClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the REST API base URL, respecting environment and overrides.
    #[must_use]
    pub fn rest_url(&self) -> String {
        self.base_url_rest
            .clone()
            .unwrap_or_else(|| urls::rest_url(self.environment).to_string())
    }

    /// Returns the WebSocket URL, respecting environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::ws_url(self.environment).to_string())
    }
}

/// Configuration for the Derive execution client.
///
/// `Debug` is implemented manually so that `session_key` is redacted; the
/// derived `Debug` would leak the raw secret through any logger or Python
/// `__repr__`.
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.derive", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.derive")
)]
pub struct DeriveExecClientConfig {
    /// Derive Chain smart-contract wallet address (`X-LYRAWALLET`). Falls back
    /// to `DERIVE_WALLET_ADDRESS` (or `DERIVE_TESTNET_WALLET_ADDRESS` on
    /// testnet) when unset.
    pub wallet_address: Option<String>,
    /// secp256k1 session-key private key in hex (with or without `0x` prefix).
    /// Falls back to `DERIVE_SESSION_PRIVATE_KEY` (or
    /// `DERIVE_TESTNET_SESSION_PRIVATE_KEY` on testnet) when unset.
    pub session_key: Option<String>,
    /// Subaccount identifier. Falls back to `DERIVE_SUBACCOUNT_ID` (or
    /// `DERIVE_TESTNET_SUBACCOUNT_ID` on testnet) when unset.
    pub subaccount_id: Option<u64>,
    /// Override for the REST API base URL.
    pub base_url_rest: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The Derive environment to connect to.
    #[builder(default)]
    pub environment: DeriveEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
    pub http_timeout_secs: u64,
    /// Maximum number of retry attempts for HTTP requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    #[builder(default = 100)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[builder(default = 5000)]
    pub retry_delay_max_ms: u64,
    /// Per-contract USDC fee cap signed into every order.
    pub max_fee_per_contract: Option<Decimal>,
    /// WebSocket transport backend (defaults to `Sockudo` when that feature is enabled).
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Override for the EIP-712 domain separator. Falls back to the constant
    /// for the configured environment when unset. The shipped constants are
    /// placeholders that must be replaced or overridden before signing.
    pub domain_separator: Option<String>,
    /// Override for the EIP-712 action typehash. Falls back to the shipped
    /// [`crate::common::consts::ACTION_TYPEHASH`] when unset.
    pub action_typehash: Option<String>,
    /// Override for the Trade module contract address. Falls back to the
    /// shipped per-environment constant when unset.
    pub trade_module_address: Option<String>,
    /// Signature expiry TTL in seconds for normal orders and replaces (added
    /// to the wall clock before signing). Must be greater than the venue
    /// minimum ([`crate::common::consts::MIN_SIGNATURE_TTL`], 300s).
    #[builder(default = 600)]
    pub signature_expiry_secs: u64,
    /// Slippage bound applied to market orders when deriving a worst-acceptable
    /// limit price from the cached top-of-book quote. Expressed in basis points
    /// (1 bp = 0.01%). Defaults to 50 bp = 0.5%.
    #[builder(default = 50)]
    pub market_order_slippage_bps: u32,
    /// Maximum matching-engine requests per second for order writes sent over
    /// the WebSocket (create/cancel/replace). Defaults to the Trader-tier limit
    /// of 1 when unset; raise it for Market Maker accounts with higher
    /// negotiated limits. See <https://docs.derive.xyz/reference/rate-limits>.
    pub max_matching_requests_per_second: Option<u32>,
}

impl Default for DeriveExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl Debug for DeriveExecClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DeriveExecClientConfig))
            .field("wallet_address", &self.wallet_address)
            .field(
                "session_key",
                &self.session_key.as_deref().map(|_| "***redacted***"),
            )
            .field("subaccount_id", &self.subaccount_id)
            .field("base_url_rest", &self.base_url_rest)
            .field("base_url_ws", &self.base_url_ws)
            .field("proxy_url", &self.proxy_url)
            .field("environment", &self.environment)
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .field("max_fee_per_contract", &self.max_fee_per_contract)
            .field("transport_backend", &self.transport_backend)
            .field("domain_separator", &self.domain_separator)
            .field("action_typehash", &self.action_typehash)
            .field("trade_module_address", &self.trade_module_address)
            .field("signature_expiry_secs", &self.signature_expiry_secs)
            .field("market_order_slippage_bps", &self.market_order_slippage_bps)
            .field(
                "max_matching_requests_per_second",
                &self.max_matching_requests_per_second,
            )
            .finish()
    }
}

impl DeriveExecClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when wallet, session-key, and subaccount are all populated
    /// **in this config**. Environment-variable fallbacks documented on the
    /// individual fields are resolved at factory-construction time, not here;
    /// callers that need a "credentials available anywhere" check should
    /// inspect both this method and the relevant env vars.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.wallet_address
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            && self
                .session_key
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            && self.subaccount_id.is_some()
    }

    /// Returns the REST API base URL, respecting environment and overrides.
    #[must_use]
    pub fn rest_url(&self) -> String {
        self.base_url_rest
            .clone()
            .unwrap_or_else(|| urls::rest_url(self.environment).to_string())
    }

    /// Returns the WebSocket URL, respecting environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::ws_url(self.environment).to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_defaults() {
        let config = DeriveDataClientConfig::default();
        assert_eq!(config.environment, DeriveEnvironment::Mainnet);
        assert_eq!(config.http_timeout_secs, 10);
        assert_eq!(config.ws_timeout_secs, 30);
        assert_eq!(config.update_instruments_interval_mins, 60);
        assert!(config.currencies.is_empty());
        assert!(!config.include_expired);
        assert!(config.auto_load_missing_instruments);
    }

    #[rstest]
    fn test_data_config_urls_mainnet() {
        let config = DeriveDataClientConfig::default();
        assert!(config.rest_url().contains("api.lyra.finance"));
        assert!(config.ws_url().contains("api.lyra.finance"));
    }

    #[rstest]
    fn test_data_config_urls_testnet() {
        let config = DeriveDataClientConfig {
            environment: DeriveEnvironment::Testnet,
            ..DeriveDataClientConfig::default()
        };
        assert!(config.rest_url().contains("demo"));
        assert!(config.ws_url().contains("demo"));
    }

    #[rstest]
    fn test_exec_config_defaults() {
        let config = DeriveExecClientConfig::default();
        assert_eq!(config.environment, DeriveEnvironment::Mainnet);
        assert_eq!(config.http_timeout_secs, 10);
        assert_eq!(config.max_retries, 3);
        assert!(config.max_matching_requests_per_second.is_none());
        assert!(!config.has_credentials());
    }

    #[rstest]
    fn test_exec_config_has_credentials_requires_all_three_fields() {
        let mut config = DeriveExecClientConfig {
            wallet_address: Some("0x1234".to_string()),
            ..DeriveExecClientConfig::default()
        };
        assert!(!config.has_credentials());

        config.session_key = Some("0xabcd".to_string());
        assert!(!config.has_credentials());

        config.subaccount_id = Some(1);
        assert!(config.has_credentials());
    }

    #[rstest]
    fn test_exec_config_has_credentials_rejects_blank_strings() {
        let config = DeriveExecClientConfig {
            wallet_address: Some("   ".to_string()),
            session_key: Some("0xabcd".to_string()),
            subaccount_id: Some(1),
            ..DeriveExecClientConfig::default()
        };
        assert!(!config.has_credentials());
    }

    #[rstest]
    fn test_exec_config_debug_redacts_session_key() {
        // Use a low-entropy sentinel rather than a hex private key so the
        // assertion exercises Debug-redaction without tripping the secrets
        // scanner on a synthetic test value. The redaction logic is
        // string-content-agnostic.
        let session_key = "FAKE_SESSION_KEY_SENTINEL";
        let config = DeriveExecClientConfig {
            wallet_address: Some("0xWALLET".to_string()),
            session_key: Some(session_key.to_string()),
            subaccount_id: Some(42),
            ..DeriveExecClientConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("redacted"));
        assert!(!debug.contains(session_key));
        assert!(debug.contains("0xWALLET"));
        assert!(debug.contains("42"));
    }

    #[rstest]
    fn test_exec_config_debug_omits_session_key_marker_when_unset() {
        let config = DeriveExecClientConfig::default();
        let debug = format!("{config:?}");
        assert!(!debug.contains("redacted"));
        assert!(debug.contains("session_key: None"));
    }
}
