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
//!
//! The default environment is mainnet. Deployment and environment select the credential namespace
//! and protocol settings as well as endpoint defaults. URL overrides change only transport.
//!
//! Fields follow this order:
//!
//! - Environment
//! - Deployment
//! - Nautilus identity
//! - Authentication
//! - Connectivity
//! - Operational behavior

use nautilus_core::string::secret::SecretString;
use nautilus_live::book::DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS;
use nautilus_model::{
    identifiers::{AccountId, Venue},
    types::Currency,
};
use nautilus_network::websocket::{TransportBackend, proxy::ProxyUrl};
use serde::{Deserialize, Deserializer, Serialize};

use crate::common::{
    consts::{
        SCHEDULED_CANCEL_CLOCK_HEADROOM_SECS, SCHEDULED_CANCEL_MAX_DELAY_SECS,
        SCHEDULED_CANCEL_MIN_DELAY_SECS, SCHEDULED_CANCEL_REQUEST_TIMEOUT,
    },
    credential::credential_env_vars_for_deployment,
    deployment,
    enums::{LighterDeployment, LighterEnvironment},
};

const WS_READONLY_QUERY_PARAM: &str = "readonly";

/// Configuration for the Lighter data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.lighter", from_py_object,)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.lighter")
)]
pub struct LighterDataClientConfig {
    /// Target environment within the selected deployment.
    #[builder(default)]
    pub environment: LighterEnvironment,
    /// Lighter protocol deployment, which controls endpoint defaults and protocol settings.
    #[builder(default)]
    pub deployment: LighterDeployment,
    /// Optional Nautilus venue identifier override.
    ///
    /// This scopes instruments, cache entries, and message routing without changing the
    /// deployment's signing or settlement settings.
    pub venue: Option<Venue>,
    /// Optional account index retained for config compatibility.
    /// Public data requests do not use credentials.
    pub account_index: Option<u64>,
    /// Optional API key index retained for config compatibility.
    /// Public data requests do not use credentials.
    pub api_key_index: Option<u8>,
    /// Optional private key retained for config compatibility.
    /// Public data requests do not use credentials.
    pub private_key: Option<SecretString>,
    /// Optional REST URL override.
    pub base_url_http: Option<String>,
    /// Optional WebSocket URL override. The client enforces `readonly=true`.
    /// Different resolved data and execution URLs use separate control-message limiters.
    pub base_url_ws: Option<String>,
    /// Optional absolute HTTP or HTTPS proxy URL for both transports, including its scheme.
    #[serde(deserialize_with = "deserialize_proxy_url")]
    pub proxy_url: Option<SecretString>,
    /// HTTP request timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket connection and reconnection timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Refresh interval for instrument metadata in minutes. Set to 0 to disable refresh.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// Maximum time to wait for an initial, post-reconnect, or recovery order book
    /// snapshot in seconds.
    ///
    /// Set to 0 to disable snapshot deadlines.
    #[builder(default = DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS)]
    pub book_snapshot_timeout_secs: u64,
    /// Optional REST read-bucket quota in requests per minute. `None` or 0 uses 60.
    /// Each client has its own limiter; budget combined data and execution traffic.
    /// Raising the quota requires venue IP registration and does not authenticate public data.
    pub rest_quota_per_min: Option<u32>,
    /// WebSocket transport backend.
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(LighterDataClientConfig {
    environment: LighterEnvironment,
    deployment: LighterDeployment,
    venue: Option<Venue>,
    account_index: Option<u64>,
    api_key_index: Option<u8>,
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    http_timeout_secs: u64,
    ws_timeout_secs: u64,
    update_instruments_interval_mins: u64,
    book_snapshot_timeout_secs: u64,
    rest_quota_per_min: Option<u32>,
    transport_backend: TransportBackend,
});

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
        self.base_url_http.clone().unwrap_or_else(|| {
            deployment::http_base_url(self.deployment, self.environment).to_string()
        })
    }

    /// Returns the resolved WebSocket URL.
    #[must_use]
    pub fn ws_url(&self) -> String {
        let url = self
            .base_url_ws
            .clone()
            .unwrap_or_else(|| deployment::ws_url(self.deployment, self.environment).to_string());

        ensure_readonly_ws_url(url)
    }

    /// Returns the configured venue or the deployment default.
    #[must_use]
    pub fn resolved_venue(&self) -> Venue {
        self.venue
            .unwrap_or_else(|| deployment::venue(self.deployment))
    }

    /// Returns the deployment settlement currency.
    #[must_use]
    pub fn settlement_currency(&self) -> Currency {
        deployment::settlement_currency(self.deployment)
    }

    /// Returns `true` when all credential fields are present in config or the selected environment.
    ///
    /// This checks presence without validating credentials. Public data requests do not use them.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        credentials_available(
            self.private_key.as_ref(),
            self.account_index,
            self.api_key_index,
            self.deployment,
            self.environment,
        )
    }
}

fn ensure_readonly_ws_url(url: String) -> String {
    let Ok(mut parsed) = url::Url::parse(&url) else {
        return url;
    };

    let pairs = parsed
        .query_pairs()
        .filter(|(key, _)| key != WS_READONLY_QUERY_PARAM)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    parsed.set_query(None);
    {
        let mut query = parsed.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(&key, &value);
        }
        query.append_pair(WS_READONLY_QUERY_PARAM, "true");
    }

    parsed.to_string()
}

/// Configuration for the Lighter execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.lighter", from_py_object,)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.lighter")
)]
pub struct LighterExecutionClientConfig {
    /// Target environment within the selected deployment.
    #[builder(default)]
    pub environment: LighterEnvironment,
    /// Lighter protocol deployment, which controls endpoint defaults and protocol settings.
    #[builder(default)]
    pub deployment: LighterDeployment,
    /// Optional Nautilus venue identifier override.
    ///
    /// This scopes instruments, cache entries, and execution routing without changing the
    /// deployment's signing, settlement, or protocol behavior.
    /// Use underscores instead of hyphens so the account ID issuer can match.
    #[serde(deserialize_with = "deserialize_execution_venue")]
    pub venue: Option<Venue>,
    /// Account identifier on the venue. Its issuer must match the resolved venue.
    /// Rust defaults to `LIGHTER-001`; override it for Robinhood or a custom venue.
    /// Python requires an explicit account ID.
    #[builder(default = AccountId::from("LIGHTER-001"))]
    pub account_id: AccountId,
    /// Lighter account index (numeric, assigned at registration). Falls back
    /// to the environment variable selected by `deployment` and `environment`.
    pub account_index: Option<u64>,
    /// API key index for a user-created Lighter key. Low indexes are reserved
    /// for Lighter clients; 255 is the `apikeys` all-keys sentinel. Falls back
    /// to the environment variable selected by `deployment` and `environment`.
    pub api_key_index: Option<u8>,
    /// Hex-encoded private key for the API key (Schnorr / ecgfp5). Falls back
    /// to the environment variable selected by `deployment` and `environment`.
    pub private_key: Option<SecretString>,
    /// Optional REST URL override.
    pub base_url_http: Option<String>,
    /// Optional WebSocket URL override. URLs containing `readonly=true` are rejected because
    /// the query parameter does not prevent authenticated trading.
    #[serde(deserialize_with = "deserialize_execution_ws_url")]
    pub base_url_ws: Option<String>,
    /// Optional absolute HTTP or HTTPS proxy URL for both transports, including its scheme.
    #[serde(deserialize_with = "deserialize_proxy_url")]
    pub proxy_url: Option<SecretString>,
    /// HTTP request timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket connection and reconnection timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Slippage buffer in basis points for market-style orders, in `[0, 10_000)`.
    /// Per-order `market_order_slippage_bps` overrides must be integers in the same range;
    /// invalid values cause local denial before signing or dispatch.
    #[builder(default = 50)]
    #[serde(deserialize_with = "deserialize_market_order_slippage_bps")]
    pub market_order_slippage_bps: u32,
    /// Optional REST read-bucket quota in requests per minute. `None` or 0 uses 60.
    /// Raising the quota requires venue IP registration. Each client has its own limiter;
    /// Standard accounts must budget combined REST and transaction traffic.
    pub rest_quota_per_min: Option<u32>,
    /// Optional transaction quota (req/min), independent of `rest_quota_per_min`.
    /// `None` or 0 uses 60. HTTP and WebSocket sends share this limiter, including trading,
    /// startup integrator approval, and scheduled cancellation. Low quotas can delay arming
    /// and renewal; Standard accounts must budget combined REST and transaction traffic.
    pub sendtx_quota_per_min: Option<u32>,
    /// WebSocket transport backend.
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Whether to use Lighter-native GTD orders.
    ///
    /// The current Lighter venue validation requires a `GoodTillTime` expiry of at least five
    /// minutes, so a shorter strategy GTD lifetime cannot be represented as an explicit venue
    /// expiry. Set to false only when the strategy manages GTD expiry locally. Lighter then uses
    /// a 28-day fallback expiry and the strategy must enable `manage_gtd_expiry` so the local
    /// expiry timer sends the cancel. Local strategy expiries beyond 28 days are denied because
    /// the fallback would expire first; use native GTD for those orders.
    #[builder(default = true)]
    pub use_gtd: bool,
    /// Arms and renews account-wide scheduled cancellation with this timeout in seconds.
    ///
    /// Valid values are `[420, 1_295_940]`. `None` leaves the venue schedule untouched.
    /// Connection fails if arming does not execute within 30 seconds after account streams are ready.
    /// Shutdown stops renewal and preserves the last deadline. Scheduled cancellation does not close positions.
    #[serde(deserialize_with = "deserialize_scheduled_cancel_timeout")]
    pub scheduled_cancel_timeout_secs: Option<u64>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(LighterExecutionClientConfig {
    environment: LighterEnvironment,
    deployment: LighterDeployment,
    venue: Option<Venue>,
    account_id: AccountId,
    account_index: Option<u64>,
    api_key_index: Option<u8>,
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    http_timeout_secs: u64,
    ws_timeout_secs: u64,
    market_order_slippage_bps: u32,
    rest_quota_per_min: Option<u32>,
    sendtx_quota_per_min: Option<u32>,
    transport_backend: TransportBackend,
    use_gtd: bool,
    scheduled_cancel_timeout_secs: Option<u64>,
});

impl Default for LighterExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl LighterExecutionClientConfig {
    /// Returns `true` when all credential fields are present in config or the selected environment.
    ///
    /// This checks presence without validating credentials. Client construction validates
    /// the resolved account index, API key index, and private key.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        credentials_available(
            self.private_key.as_ref(),
            self.account_index,
            self.api_key_index,
            self.deployment,
            self.environment,
        )
    }

    /// Returns the resolved REST base URL.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http.clone().unwrap_or_else(|| {
            deployment::http_base_url(self.deployment, self.environment).to_string()
        })
    }

    /// Returns the resolved WebSocket URL.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| deployment::ws_url(self.deployment, self.environment).to_string())
    }

    /// Returns the configured venue or the deployment default.
    #[must_use]
    pub fn resolved_venue(&self) -> Venue {
        self.venue
            .unwrap_or_else(|| deployment::venue(self.deployment))
    }

    /// Returns the deployment settlement currency.
    #[must_use]
    pub fn settlement_currency(&self) -> Currency {
        deployment::settlement_currency(self.deployment)
    }

    /// Returns the L2 signing-domain chain ID for the deployment and environment.
    #[must_use]
    pub const fn chain_id(&self) -> u32 {
        deployment::chain_id(self.deployment, self.environment)
    }
}

fn credentials_available(
    private_key: Option<&SecretString>,
    account_index: Option<u64>,
    api_key_index: Option<u8>,
    deployment: LighterDeployment,
    environment: LighterEnvironment,
) -> bool {
    let (key_var, secret_var, account_var) =
        credential_env_vars_for_deployment(deployment, environment);
    let has_key = api_key_index.is_some() || env_var_is_set(key_var);
    let has_account = account_index.is_some() || env_var_is_set(account_var);
    let has_secret = private_key
        .map(SecretString::expose_secret)
        .is_some_and(|s| !s.trim().is_empty())
        || env_var_is_set(secret_var);

    has_key && has_account && has_secret
}

fn env_var_is_set(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}

fn deserialize_proxy_url<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<SecretString>, D::Error> {
    let value = Option::<SecretString>::deserialize(deserializer)?;
    validate_proxy_url(value.as_ref()).map_err(serde::de::Error::custom)?;
    Ok(value)
}

pub(crate) fn validate_proxy_url(value: Option<&SecretString>) -> anyhow::Result<()> {
    if let Some(value) = value {
        ProxyUrl::parse(value.expose_secret()).map_err(|_| {
            anyhow::anyhow!(
                "Lighter proxy_url must be an absolute http:// or https:// URL with a host"
            )
        })?;
    }

    Ok(())
}

fn deserialize_execution_venue<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Venue>, D::Error> {
    let value = Option::<Venue>::deserialize(deserializer)?;
    validate_execution_venue(value).map_err(serde::de::Error::custom)?;
    Ok(value)
}

pub(crate) fn validate_execution_venue(value: Option<Venue>) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.is_none_or(|venue| !venue.as_str().contains('-')),
        "Lighter execution venue must not contain '-'; use '_' so the account ID issuer can match"
    );
    Ok(())
}

fn deserialize_execution_ws_url<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    validate_execution_ws_url(value.as_deref()).map_err(serde::de::Error::custom)?;
    Ok(value)
}

pub(crate) fn validate_execution_ws_url(value: Option<&str>) -> anyhow::Result<()> {
    if let Some(value) = value.and_then(|value| url::Url::parse(value).ok()) {
        anyhow::ensure!(
            !value.query_pairs().any(|(key, value)| {
                key.eq_ignore_ascii_case(WS_READONLY_QUERY_PARAM)
                    && value.trim().eq_ignore_ascii_case("true")
            }),
            "Lighter execution WebSocket URL must not set readonly=true; it does not disable trading"
        );
    }

    Ok(())
}

fn deserialize_market_order_slippage_bps<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    validate_market_order_slippage_bps(value).map_err(serde::de::Error::custom)?;
    Ok(value)
}

pub(crate) fn validate_market_order_slippage_bps(value: u32) -> anyhow::Result<()> {
    anyhow::ensure!(
        value < 10_000,
        "Lighter market_order_slippage_bps must be in [0, 10000)"
    );
    Ok(())
}

fn deserialize_scheduled_cancel_timeout<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let timeout = Option::<u64>::deserialize(deserializer)?;
    validate_scheduled_cancel_timeout(timeout).map_err(serde::de::Error::custom)?;
    Ok(timeout)
}

pub(crate) fn validate_scheduled_cancel_timeout(timeout_secs: Option<u64>) -> anyhow::Result<()> {
    let min = SCHEDULED_CANCEL_MIN_DELAY_SECS
        + SCHEDULED_CANCEL_CLOCK_HEADROOM_SECS
        + 2 * SCHEDULED_CANCEL_REQUEST_TIMEOUT.as_secs();
    let max = SCHEDULED_CANCEL_MAX_DELAY_SECS - SCHEDULED_CANCEL_CLOCK_HEADROOM_SECS;

    if let Some(timeout_secs) = timeout_secs {
        anyhow::ensure!(
            (min..=max).contains(&timeout_secs),
            "scheduled_cancel_timeout_secs must be in [{min}, {max}]"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use nautilus_core::string::secret::REDACTED;
    use rstest::rstest;

    use super::*;

    const PRIVATE_KEY_HEX: &str =
        "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";

    #[rstest]
    #[case(None, true)]
    #[case(Some("http://127.0.0.1:8080"), true)]
    #[case(Some("https://fixture:proxy-secret@proxy.example:8443"), true)]
    #[case(Some("127.0.0.1:8080"), false)]
    #[case(Some("socks5://127.0.0.1:1080"), false)]
    #[case(Some("http://fixture:proxy-secret@[::1"), false)]
    fn test_proxy_url_requires_shared_transport_support(
        #[case] value: Option<&str>,
        #[case] valid: bool,
    ) {
        let json = serde_json::json!({"proxy_url": value});
        let data = serde_json::from_value::<LighterDataClientConfig>(json.clone());
        let execution = serde_json::from_value::<LighterExecutionClientConfig>(json);

        if valid {
            assert_eq!(
                data.unwrap()
                    .proxy_url
                    .as_ref()
                    .map(SecretString::expose_secret),
                value
            );
            assert_eq!(
                execution
                    .unwrap()
                    .proxy_url
                    .as_ref()
                    .map(SecretString::expose_secret),
                value
            );
        } else {
            let expected =
                "Lighter proxy_url must be an absolute http:// or https:// URL with a host";
            assert_eq!(data.unwrap_err().to_string(), expected);
            assert_eq!(execution.unwrap_err().to_string(), expected);
        }
    }

    #[rstest]
    #[case(None, true)]
    #[case(Some("LIGHTER_ALT"), true)]
    #[case(Some("LIGHTER-ALT"), false)]
    fn test_execution_venue_requires_representable_account_issuer(
        #[case] value: Option<&str>,
        #[case] valid: bool,
    ) {
        let json = serde_json::json!({"venue": value});
        let data: LighterDataClientConfig = serde_json::from_value(json.clone()).unwrap();
        let execution = serde_json::from_value::<LighterExecutionClientConfig>(json);
        assert_eq!(
            data.venue.map(|venue| venue.to_string()),
            value.map(str::to_string)
        );

        if valid {
            assert_eq!(execution.unwrap().venue, data.venue);
        } else {
            assert_eq!(
                execution.unwrap_err().to_string(),
                "Lighter execution venue must not contain '-'; use '_' so the account ID issuer can match"
            );
        }
    }

    #[rstest]
    #[case(None, true)]
    #[case(Some("wss://example.com/stream"), true)]
    #[case(Some("wss://example.com/stream?readonly=false"), true)]
    #[case(Some("wss://example.com/stream?readonly=true"), false)]
    #[case(Some("wss://example.com/stream?readonly=false&readonly=true"), false)]
    #[case(Some("wss://example.com/stream?read%6Fnly=tr%75e"), false)]
    #[case(Some("wss://example.com/stream?ReadOnly=%20TRUE%20"), false)]
    fn test_execution_ws_url_rejects_misleading_readonly_query(
        #[case] value: Option<&str>,
        #[case] valid: bool,
    ) {
        let json = serde_json::json!({"base_url_ws": value});
        let data: LighterDataClientConfig = serde_json::from_value(json.clone()).unwrap();
        let execution = serde_json::from_value::<LighterExecutionClientConfig>(json);
        assert_eq!(data.base_url_ws.as_deref(), value);

        if valid {
            assert_eq!(execution.unwrap().base_url_ws.as_deref(), value);
        } else {
            assert_eq!(
                execution.unwrap_err().to_string(),
                "Lighter execution WebSocket URL must not set readonly=true; it does not disable trading"
            );
        }
    }

    #[rstest]
    #[case(0, true)]
    #[case(9_999, true)]
    #[case(10_000, false)]
    #[case(u32::MAX, false)]
    fn test_market_order_slippage_bps(#[case] value: u32, #[case] valid: bool) {
        let decoded = serde_json::from_value::<LighterExecutionClientConfig>(serde_json::json!({
            "market_order_slippage_bps": value,
        }));

        if valid {
            assert_eq!(decoded.unwrap().market_order_slippage_bps, value);
        } else {
            assert_eq!(
                decoded.unwrap_err().to_string(),
                "Lighter market_order_slippage_bps must be in [0, 10000)"
            );
        }
    }

    #[rstest]
    #[case(None, true)]
    #[case(Some(0), false)]
    #[case(Some(15), false)]
    #[case(Some(419), false)]
    #[case(Some(420), true)]
    #[case(Some(1_295_940), true)]
    #[case(Some(1_295_941), false)]
    #[case(Some(u64::MAX), false)]
    fn test_scheduled_cancel_timeout(#[case] timeout: Option<u64>, #[case] valid: bool) {
        let config = LighterExecutionClientConfig {
            scheduled_cancel_timeout_secs: timeout,
            ..Default::default()
        };

        let encoded = serde_json::to_value(&config).unwrap();
        assert_eq!(
            encoded["scheduled_cancel_timeout_secs"],
            serde_json::json!(timeout)
        );
        let decoded = serde_json::from_value::<LighterExecutionClientConfig>(encoded);
        assert_eq!(decoded.is_ok(), valid);

        if valid {
            assert_eq!(decoded.unwrap().scheduled_cancel_timeout_secs, timeout);
        }
    }

    #[rstest]
    fn test_scheduled_cancel_timeout_default() {
        assert_eq!(
            LighterExecutionClientConfig::default().scheduled_cancel_timeout_secs,
            None
        );
        let decoded: LighterExecutionClientConfig =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(decoded.scheduled_cancel_timeout_secs, None);
    }

    #[rstest]
    fn data_config_has_credentials_when_all_fields_set() {
        let config = LighterDataClientConfig {
            api_key_index: Some(5),
            account_index: Some(12_345),
            private_key: Some(PRIVATE_KEY_HEX.into()),
            ..Default::default()
        };

        assert!(config.has_credentials());
    }

    #[rstest]
    fn data_config_debug_redacts_private_key() {
        let config = LighterDataClientConfig {
            api_key_index: Some(5),
            account_index: Some(12_345),
            private_key: Some(PRIVATE_KEY_HEX.into()),
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
    fn data_config_book_snapshot_timeout_default_is_ten_seconds() {
        let config = LighterDataClientConfig::default();

        assert_eq!(config.book_snapshot_timeout_secs, 10);
    }

    #[derive(Debug)]
    struct ExpectedDeploymentSettings {
        http_url: &'static str,
        data_ws_url: &'static str,
        chain_id: u32,
        venue: &'static str,
        currency: &'static str,
    }

    #[rstest]
    #[case::lighter_mainnet(
        LighterDeployment::Lighter,
        LighterEnvironment::Mainnet,
        ExpectedDeploymentSettings {
            http_url: "https://mainnet.zklighter.elliot.ai",
            data_ws_url: "wss://mainnet.zklighter.elliot.ai/stream?readonly=true",
            chain_id: 304,
            venue: "LIGHTER",
            currency: "USDC",
        }
    )]
    #[case::lighter_testnet(
        LighterDeployment::Lighter,
        LighterEnvironment::Testnet,
        ExpectedDeploymentSettings {
            http_url: "https://testnet.zklighter.elliot.ai",
            data_ws_url: "wss://testnet.zklighter.elliot.ai/stream?readonly=true",
            chain_id: 300,
            venue: "LIGHTER",
            currency: "USDC",
        }
    )]
    #[case::robinhood_mainnet(
        LighterDeployment::Robinhood,
        LighterEnvironment::Mainnet,
        ExpectedDeploymentSettings {
            http_url: "https://api.rh.lighter.xyz",
            data_ws_url: "wss://api.rh.lighter.xyz/stream?readonly=true",
            chain_id: 466_324,
            venue: "LIGHTER_ROBINHOOD",
            currency: "USDG",
        }
    )]
    #[case::robinhood_testnet(
        LighterDeployment::Robinhood,
        LighterEnvironment::Testnet,
        ExpectedDeploymentSettings {
            http_url: "https://api.rh-testnet.lighter.xyz",
            data_ws_url: "wss://api.rh-testnet.lighter.xyz/stream?readonly=true",
            chain_id: 300,
            venue: "LIGHTER_ROBINHOOD",
            currency: "USDG",
        }
    )]
    fn configs_resolve_deployment_settings(
        #[case] deployment: LighterDeployment,
        #[case] environment: LighterEnvironment,
        #[case] expected: ExpectedDeploymentSettings,
    ) {
        let data = LighterDataClientConfig {
            environment,
            deployment,
            ..Default::default()
        };

        let execution = LighterExecutionClientConfig {
            environment,
            deployment,
            ..Default::default()
        };

        assert_eq!(data.http_url(), expected.http_url);
        assert_eq!(data.ws_url(), expected.data_ws_url);
        assert_eq!(data.resolved_venue().as_str(), expected.venue);
        assert_eq!(data.settlement_currency().code, expected.currency);
        assert_eq!(execution.http_url(), expected.http_url);
        assert_eq!(
            execution.ws_url(),
            expected.data_ws_url.replace("?readonly=true", "")
        );
        assert_eq!(execution.resolved_venue().as_str(), expected.venue);
        assert_eq!(execution.settlement_currency().code, expected.currency);
        assert_eq!(execution.chain_id(), expected.chain_id);
    }

    #[rstest]
    fn configs_preserve_custom_venue() {
        let venue = Venue::from("LIGHTER_CUSTOM");
        let data = LighterDataClientConfig {
            deployment: LighterDeployment::Robinhood,
            venue: Some(venue),
            ..Default::default()
        };

        let execution = LighterExecutionClientConfig {
            deployment: LighterDeployment::Robinhood,
            venue: Some(venue),
            ..Default::default()
        };

        assert_eq!(data.resolved_venue(), venue);
        assert_eq!(execution.resolved_venue(), venue);
        assert_eq!(execution.chain_id(), 466_324);
    }

    #[rstest]
    fn exec_config_debug_redacts_private_key() {
        let config = LighterExecutionClientConfig {
            account_id: AccountId::from("LIGHTER-001"),
            api_key_index: Some(5),
            account_index: Some(12_345),
            private_key: Some(PRIVATE_KEY_HEX.into()),
            base_url_http: None,
            base_url_ws: None,
            proxy_url: None,
            environment: LighterEnvironment::Mainnet,
            deployment: LighterDeployment::Lighter,
            venue: None,
            http_timeout_secs: 60,
            ws_timeout_secs: 30,
            market_order_slippage_bps: 50,
            rest_quota_per_min: None,
            sendtx_quota_per_min: None,
            transport_backend: TransportBackend::default(),
            use_gtd: true,
            scheduled_cancel_timeout_secs: None,
        };

        let dbg_out = format!("{config:?}");

        assert!(dbg_out.contains(REDACTED));
        assert!(!dbg_out.contains(PRIVATE_KEY_HEX));
    }

    #[rstest]
    fn exec_config_use_gtd_defaults_to_true() {
        // Backwards compatibility: the default preserves the native Lighter GTD
        // behavior so existing configs are unchanged.
        let config = LighterExecutionClientConfig::default();

        assert!(config.use_gtd);
    }

    #[rstest]
    fn exec_config_toml_use_gtd_override() {
        let config: LighterExecutionClientConfig = toml::from_str("use_gtd = false").unwrap();

        assert!(!config.use_gtd);
    }

    #[rstest]
    fn exec_config_ws_url_keeps_regular_stream_url() {
        let config = LighterExecutionClientConfig {
            account_id: AccountId::from("LIGHTER-001"),
            environment: LighterEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.ws_url(), "wss://mainnet.zklighter.elliot.ai/stream");
    }
}
