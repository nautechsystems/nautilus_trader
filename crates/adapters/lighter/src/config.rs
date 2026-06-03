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

//! Configuration structures for the Lighter adapter.

use std::fmt::Debug;

use nautilus_core::string::secret::REDACTED;
use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::common::{
    credential::credential_env_vars,
    enums::LighterEnvironment,
    urls::{lighter_http_base_url, lighter_ws_url},
};

/// Configuration for the Lighter data client.
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter", from_py_object,)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.lighter")
)]
pub struct LighterDataClientConfig {
    /// Optional REST URL override.
    pub base_url_http: Option<String>,
    /// Optional WebSocket URL override.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// Target environment.
    #[builder(default)]
    pub environment: LighterEnvironment,
    /// Lighter account index for authenticated REST data requests. Falls back
    /// to `LIGHTER_ACCOUNT_INDEX` / `LIGHTER_TESTNET_ACCOUNT_INDEX`.
    pub account_index: Option<u64>,
    /// API key index for authenticated REST data requests. Falls back to
    /// `LIGHTER_API_KEY_INDEX` / `LIGHTER_TESTNET_API_KEY_INDEX`.
    pub api_key_index: Option<u8>,
    /// Hex-encoded private key for REST auth tokens. Falls back to
    /// `LIGHTER_API_SECRET` / `LIGHTER_TESTNET_API_SECRET`.
    pub private_key: Option<String>,
    /// HTTP request timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket connect timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Refresh interval for instrument metadata in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// WebSocket transport backend.
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for LighterDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl LighterDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the resolved REST base URL.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| lighter_http_base_url(self.environment).to_string())
    }

    /// Returns the resolved WebSocket URL.
    #[must_use]
    pub fn ws_url(&self) -> String {
        let url = self
            .base_url_ws
            .clone()
            .unwrap_or_else(|| lighter_ws_url(self.environment).to_string());

        ensure_readonly_ws_url(url)
    }

    /// Returns `true` when all REST auth credential fields are available.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        let (key_var, secret_var, account_var) = credential_env_vars(self.environment);
        let has_key = self.api_key_index.is_some() || env_var_is_set(key_var);
        let has_account = self.account_index.is_some() || env_var_is_set(account_var);
        let has_secret = self
            .private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || env_var_is_set(secret_var);

        has_key && has_account && has_secret
    }
}

impl Debug for LighterDataClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LighterDataClientConfig))
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws", &self.base_url_ws)
            .field("proxy_url", &self.proxy_url)
            .field("environment", &self.environment)
            .field("account_index", &self.account_index)
            .field("api_key_index", &self.api_key_index)
            .field("private_key", &self.private_key.as_ref().map(|_| REDACTED))
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("ws_timeout_secs", &self.ws_timeout_secs)
            .field(
                "update_instruments_interval_mins",
                &self.update_instruments_interval_mins,
            )
            .field("transport_backend", &self.transport_backend)
            .finish()
    }
}

fn env_var_is_set(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}

fn ensure_readonly_ws_url(url: String) -> String {
    let Ok(mut parsed) = url::Url::parse(&url) else {
        return url;
    };

    let pairs = parsed
        .query_pairs()
        .filter(|(key, _)| key != "readonly")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    parsed.set_query(None);
    {
        let mut query = parsed.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(&key, &value);
        }
        query.append_pair("readonly", "true");
    }

    parsed.to_string()
}

/// Configuration for the Lighter execution client.
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter", from_py_object,)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.lighter")
)]
pub struct LighterExecClientConfig {
    /// Trader identifier.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// Account identifier on the venue.
    #[builder(default = AccountId::from("LIGHTER-001"))]
    pub account_id: AccountId,
    /// Lighter account index (numeric, assigned at registration). Falls back
    /// to `LIGHTER_ACCOUNT_INDEX` / `LIGHTER_TESTNET_ACCOUNT_INDEX` when
    /// resolved through `common::credential`.
    pub account_index: Option<u64>,
    /// API key index (0-254; indices 0-3 are reserved for desktop/mobile
    /// clients). Falls back to `LIGHTER_API_KEY_INDEX` /
    /// `LIGHTER_TESTNET_API_KEY_INDEX` when resolved through
    /// `common::credential`.
    pub api_key_index: Option<u8>,
    /// Hex-encoded private key for the API key (Schnorr / ecgfp5). Falls back
    /// to `LIGHTER_API_SECRET` / `LIGHTER_TESTNET_API_SECRET` when resolved
    /// through `common::credential`.
    pub private_key: Option<String>,
    /// Optional REST URL override.
    pub base_url_http: Option<String>,
    /// Optional WebSocket URL override.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// Target environment.
    #[builder(default)]
    pub environment: LighterEnvironment,
    /// HTTP request timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket connect timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Venue market IDs to poll during unscoped reconciliation.
    #[builder(default)]
    pub active_markets: Vec<i16>,
    /// Slippage buffer in basis points for market-style orders.
    #[builder(default = 50)]
    pub market_order_slippage_bps: u32,
    /// WebSocket transport backend.
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for LighterExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl Debug for LighterExecClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LighterExecClientConfig))
            .field("trader_id", &self.trader_id)
            .field("account_id", &self.account_id)
            .field("account_index", &self.account_index)
            .field("api_key_index", &self.api_key_index)
            .field("private_key", &self.private_key.as_ref().map(|_| REDACTED))
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws", &self.base_url_ws)
            .field("proxy_url", &self.proxy_url)
            .field("environment", &self.environment)
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("ws_timeout_secs", &self.ws_timeout_secs)
            .field("active_markets", &self.active_markets)
            .field("market_order_slippage_bps", &self.market_order_slippage_bps)
            .field("transport_backend", &self.transport_backend)
            .finish()
    }
}

impl LighterExecClientConfig {
    /// Returns `true` when all fields required to sign and submit
    /// authenticated transactions are configured.
    ///
    /// Lighter signing requires the private key, the account index, and the
    /// API key index together; any missing field invalidates the credential.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        let key_set = self
            .private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        key_set && self.account_index.is_some() && self.api_key_index.is_some()
    }

    /// Returns the resolved REST base URL.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| lighter_http_base_url(self.environment).to_string())
    }

    /// Returns the resolved WebSocket URL.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| lighter_ws_url(self.environment).to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const PRIVATE_KEY_HEX: &str =
        "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";

    #[rstest]
    fn data_config_has_credentials_when_all_fields_set() {
        let config = LighterDataClientConfig {
            api_key_index: Some(5),
            account_index: Some(12_345),
            private_key: Some(PRIVATE_KEY_HEX.to_string()),
            ..Default::default()
        };

        assert!(config.has_credentials());
    }

    #[rstest]
    fn data_config_debug_redacts_private_key() {
        let config = LighterDataClientConfig {
            api_key_index: Some(5),
            account_index: Some(12_345),
            private_key: Some(PRIVATE_KEY_HEX.to_string()),
            ..Default::default()
        };

        let dbg_out = format!("{config:?}");

        assert!(dbg_out.contains(REDACTED));
        assert!(!dbg_out.contains(PRIVATE_KEY_HEX));
    }

    #[rstest]
    fn data_config_debug_omits_private_key_when_unset() {
        let config = LighterDataClientConfig::default();

        let dbg_out = format!("{config:?}");

        assert!(dbg_out.contains("private_key: None"));
    }

    #[rstest]
    fn data_config_ws_url_sets_readonly_query() {
        let config = LighterDataClientConfig::default();

        assert_eq!(
            config.ws_url(),
            "wss://mainnet.zklighter.elliot.ai/stream?readonly=true",
        );
    }

    #[rstest]
    fn data_config_ws_url_preserves_existing_query_params() {
        let config = LighterDataClientConfig {
            base_url_ws: Some("wss://mainnet.zklighter.elliot.ai/stream?foo=bar".to_string()),
            ..Default::default()
        };

        assert_eq!(
            config.ws_url(),
            "wss://mainnet.zklighter.elliot.ai/stream?foo=bar&readonly=true",
        );
    }

    #[rstest]
    fn data_config_ws_url_overrides_readonly_query() {
        let config = LighterDataClientConfig {
            base_url_ws: Some(
                "wss://mainnet.zklighter.elliot.ai/stream?readonly=false&foo=bar".to_string(),
            ),
            ..Default::default()
        };

        assert_eq!(
            config.ws_url(),
            "wss://mainnet.zklighter.elliot.ai/stream?foo=bar&readonly=true",
        );
    }

    #[rstest]
    fn exec_config_debug_redacts_private_key() {
        let config = LighterExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("LIGHTER-001"),
            api_key_index: Some(5),
            account_index: Some(12_345),
            private_key: Some(PRIVATE_KEY_HEX.to_string()),
            base_url_http: None,
            base_url_ws: None,
            proxy_url: None,
            environment: LighterEnvironment::Mainnet,
            http_timeout_secs: 60,
            ws_timeout_secs: 30,
            active_markets: Vec::new(),
            market_order_slippage_bps: 50,
            transport_backend: TransportBackend::default(),
        };

        let dbg_out = format!("{config:?}");

        assert!(dbg_out.contains(REDACTED));
        assert!(!dbg_out.contains(PRIVATE_KEY_HEX));
    }

    #[rstest]
    fn exec_config_ws_url_keeps_regular_stream_url() {
        let config = LighterExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("LIGHTER-001"),
            environment: LighterEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.ws_url(), "wss://mainnet.zklighter.elliot.ai/stream");
    }

    // Tests that observe the `env_var_is_set` fallback live in the workspace
    // `serial_tests` group (see `.config/nextest.toml`) so env-var mutation is
    // pinned to a single thread.
    #[allow(unsafe_code)] // env-var mutation in tests; restored via `EnvGuard`.
    mod serial_tests {
        use super::*;

        const LIGHTER_ENV_VARS: &[&str] = &[
            "LIGHTER_API_KEY_INDEX",
            "LIGHTER_API_SECRET",
            "LIGHTER_ACCOUNT_INDEX",
            "LIGHTER_TESTNET_API_KEY_INDEX",
            "LIGHTER_TESTNET_API_SECRET",
            "LIGHTER_TESTNET_ACCOUNT_INDEX",
        ];

        /// Snapshots and clears the Lighter credential env vars, restoring the
        /// original values on drop.
        struct EnvGuard {
            saved: Vec<(&'static str, Option<String>)>,
        }

        impl EnvGuard {
            fn clear_lighter() -> Self {
                let saved = LIGHTER_ENV_VARS
                    .iter()
                    .map(|&name| (name, std::env::var(name).ok()))
                    .collect::<Vec<_>>();
                for &(name, _) in &saved {
                    // SAFETY: the `serial_tests` nextest group serializes
                    // these tests, and no other lighter test reads or writes
                    // the LIGHTER_* env vars.
                    unsafe { std::env::remove_var(name) };
                }
                Self { saved }
            }
        }

        impl Drop for EnvGuard {
            fn drop(&mut self) {
                for (name, original) in &self.saved {
                    match original {
                        // SAFETY: see `EnvGuard::clear_lighter`.
                        Some(value) => unsafe { std::env::set_var(name, value) },
                        None => unsafe { std::env::remove_var(name) },
                    }
                }
            }
        }

        #[rstest]
        fn data_config_has_credentials_false_when_all_unset() {
            let _guard = EnvGuard::clear_lighter();
            let config = LighterDataClientConfig::default();

            assert!(!config.has_credentials());
        }

        #[rstest]
        #[case::only_api_key_index(Some(5), None, None)]
        #[case::only_account_index(None, Some(12_345), None)]
        #[case::only_private_key(None, None, Some(PRIVATE_KEY_HEX.to_string()))]
        #[case::missing_api_key_index(None, Some(12_345), Some(PRIVATE_KEY_HEX.to_string()))]
        #[case::missing_account_index(Some(5), None, Some(PRIVATE_KEY_HEX.to_string()))]
        #[case::missing_private_key(Some(5), Some(12_345), None)]
        fn data_config_has_credentials_false_for_partial_config(
            #[case] api_key_index: Option<u8>,
            #[case] account_index: Option<u64>,
            #[case] private_key: Option<String>,
        ) {
            let _guard = EnvGuard::clear_lighter();
            let config = LighterDataClientConfig {
                account_index,
                api_key_index,
                private_key,
                ..Default::default()
            };

            assert!(!config.has_credentials());
        }

        #[rstest]
        #[case::empty("")]
        #[case::whitespace("   ")]
        fn data_config_has_credentials_false_for_blank_private_key(#[case] private_key: &str) {
            let _guard = EnvGuard::clear_lighter();
            let config = LighterDataClientConfig {
                api_key_index: Some(5),
                account_index: Some(12_345),
                private_key: Some(private_key.to_string()),
                ..Default::default()
            };

            assert!(!config.has_credentials());
        }

        #[rstest]
        fn data_config_has_credentials_reads_testnet_env_vars() {
            let _guard = EnvGuard::clear_lighter();
            // SAFETY: see `EnvGuard::clear_lighter`; the guard restores values on drop.
            unsafe { std::env::set_var("LIGHTER_TESTNET_API_KEY_INDEX", "5") };
            // SAFETY: see above.
            unsafe { std::env::set_var("LIGHTER_TESTNET_API_SECRET", PRIVATE_KEY_HEX) };
            // SAFETY: see above.
            unsafe { std::env::set_var("LIGHTER_TESTNET_ACCOUNT_INDEX", "12345") };
            let config = LighterDataClientConfig {
                environment: LighterEnvironment::Testnet,
                ..Default::default()
            };

            assert!(config.has_credentials());
        }

        #[rstest]
        fn data_config_has_credentials_ignores_mismatched_environment_env_vars() {
            let _guard = EnvGuard::clear_lighter();
            // SAFETY: see `EnvGuard::clear_lighter`.
            unsafe { std::env::set_var("LIGHTER_TESTNET_API_KEY_INDEX", "5") };
            // SAFETY: see above.
            unsafe { std::env::set_var("LIGHTER_TESTNET_API_SECRET", PRIVATE_KEY_HEX) };
            // SAFETY: see above.
            unsafe { std::env::set_var("LIGHTER_TESTNET_ACCOUNT_INDEX", "12345") };
            let config = LighterDataClientConfig {
                environment: LighterEnvironment::Mainnet,
                ..Default::default()
            };

            assert!(!config.has_credentials());
        }
    }
}
