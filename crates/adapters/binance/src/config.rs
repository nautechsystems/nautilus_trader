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

//! Binance adapter configuration structures.

use std::{any::Any, collections::HashMap, fmt::Debug, str::FromStr};

use nautilus_common::factories::ClientConfig;
#[cfg(test)]
use nautilus_core::string::secret::REDACTED;
use nautilus_core::string::secret::SecretString;
use nautilus_model::{
    enums::OmsType,
    identifiers::{AccountId, InstrumentId},
    types::Currency,
};
use nautilus_network::websocket::TransportBackend;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::common::enums::{BinanceEnvironment, BinanceMarginType, BinanceProductType};

/// Configuration for Binance instrument loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.binance")
)]
pub struct BinanceInstrumentProviderConfig {
    /// Whether to load all instruments on startup.
    #[builder(default = true)]
    pub load_all: bool,
    /// Specific Nautilus instrument IDs to load when `load_all` is false.
    pub load_ids: Option<Vec<String>>,
    /// Venue filters applied while loading instruments.
    ///
    /// Supported keys are `symbols`, `bases`, `quotes`, and, for Futures,
    /// `contract_types`. Each value may be a string or an array of strings.
    #[builder(default)]
    pub filters: HashMap<String, serde_json::Value>,
    /// Fully qualified Python callable path requested by legacy configuration.
    ///
    /// Binance v2 rejects this field because the legacy Binance provider never
    /// applied it and Rust live clients cannot safely invoke arbitrary Python.
    pub filter_callable: Option<String>,
    /// Whether instrument parser failures should be logged as warnings.
    #[builder(default = true)]
    pub log_warnings: bool,
    /// Whether to query account-specific commission rates for every loaded symbol.
    #[builder(default)]
    pub query_commission_rates: bool,
}

impl Default for BinanceInstrumentProviderConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BinanceInstrumentProviderConfig {
    /// Validates instrument loading configuration.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed IDs, unsupported filters, or a legacy
    /// callable filter that Binance v2 cannot execute safely.
    pub fn validate(&self, product_type: BinanceProductType) -> anyhow::Result<()> {
        if let Some(filter_callable) = self
            .filter_callable
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            anyhow::bail!(
                "Binance v2 does not support instrument filter_callable {filter_callable:?}; \
                 the legacy Binance provider never applied callable filters"
            );
        }

        if let Some(load_ids) = &self.load_ids {
            for raw in load_ids {
                let instrument_id = InstrumentId::from_str(raw)
                    .map_err(|e| anyhow::anyhow!("invalid Binance load_ids value {raw:?}: {e}"))?;
                anyhow::ensure!(
                    instrument_id.venue.as_str() == "BINANCE",
                    "Binance load_ids value {raw:?} must use venue BINANCE"
                );
            }
        }

        for (key, value) in &self.filters {
            let supported = matches!(key.as_str(), "symbols" | "bases" | "quotes")
                || key == "contract_types"
                    && matches!(
                        product_type,
                        BinanceProductType::UsdM | BinanceProductType::CoinM
                    );
            anyhow::ensure!(
                supported,
                "unsupported Binance instrument filter {key:?} for {product_type:?}"
            );
            validate_filter_strings(key, value)?;
        }

        Ok(())
    }
}

fn validate_filter_strings(name: &str, value: &serde_json::Value) -> anyhow::Result<()> {
    let valid = match value {
        serde_json::Value::String(value) => !value.trim().is_empty(),
        serde_json::Value::Array(values) => {
            !values.is_empty()
                && values
                    .iter()
                    .all(|value| value.as_str().is_some_and(|value| !value.trim().is_empty()))
        }
        _ => false,
    };

    anyhow::ensure!(
        valid,
        "Binance instrument filter {name:?} must be a non-empty string or array of strings"
    );
    Ok(())
}

/// Spot market-data transport mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", eq, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.binance")
)]
pub enum BinanceSpotMarketDataMode {
    #[default]
    /// Spot SBE streams (requires Ed25519 credentials).
    Sbe,
    /// Force Spot public JSON streams (does not require credentials).
    Json,
}

/// Configuration for Binance data client.
///
/// Ed25519 API keys are required for SBE WebSocket streams.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.binance")
)]
pub struct BinanceDataClientConfig {
    /// Product type to subscribe to.
    #[builder(default = BinanceProductType::Spot)]
    pub product_type: BinanceProductType,
    /// Environment (live, testnet, or demo).
    #[builder(default = BinanceEnvironment::Live)]
    pub environment: BinanceEnvironment,
    /// Optional base URL override for HTTP API.
    pub base_url_http: Option<String>,
    /// Optional base URL override for WebSocket.
    ///
    /// Live USD-M Futures data overrides are normalized onto the matching
    /// `/market/ws` and `/public/ws` routes.
    pub base_url_ws: Option<String>,
    /// API key (Ed25519).
    pub api_key: Option<SecretString>,
    /// API secret (Ed25519 base64-encoded or PEM).
    pub api_secret: Option<SecretString>,
    /// Spot market-data transport mode.
    ///
    /// - `Sbe` uses SBE streams and requires Ed25519 credentials.
    /// - `Json` forces public JSON streams with no credentials.
    #[builder(default)]
    pub spot_market_data_mode: BinanceSpotMarketDataMode,
    /// Instrument loading and fee configuration.
    #[builder(default)]
    pub instrument_provider: BinanceInstrumentProviderConfig,
    /// Interval in seconds for a full instrument catalogue refresh.
    ///
    /// Set to 0 to disable. Defaults to 3600 (60 minutes).
    #[builder(default = 3600)]
    pub instrument_refresh_interval_secs: u64,
    /// Interval in seconds for polling exchange info to detect instrument status
    /// changes (e.g. Trading -> Halt). Set to 0 to disable. Defaults to 3600 (60 minutes).
    #[builder(default = 3600)]
    pub instrument_status_poll_secs: u64,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// Receive window in milliseconds for signed HTTP requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Whether to route this Spot client to Binance US.
    #[builder(default)]
    pub us: bool,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BinanceDataClientConfig {
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    spot_market_data_mode: BinanceSpotMarketDataMode,
    instrument_provider: BinanceInstrumentProviderConfig,
    instrument_refresh_interval_secs: u64,
    instrument_status_poll_secs: u64,
    recv_window_ms: u64,
    us: bool,
    transport_backend: TransportBackend,
});

impl Default for BinanceDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BinanceDataClientConfig {
    /// Validates Binance data client configuration.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid receive-window, provider, or Binance US settings.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_recv_window(self.recv_window_ms)?;
        self.instrument_provider.validate(self.product_type)?;

        if self.us {
            anyhow::ensure!(
                self.product_type == BinanceProductType::Spot,
                "Binance US supports Spot clients only"
            );
            anyhow::ensure!(
                self.environment == BinanceEnvironment::Live,
                "Binance US supports the Live environment only"
            );
            anyhow::ensure!(
                self.spot_market_data_mode == BinanceSpotMarketDataMode::Json,
                "Binance US market data requires spot_market_data_mode=Json"
            );
        }

        Ok(())
    }
}

impl ClientConfig for BinanceDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Configuration for Binance execution client.
///
/// Global execution uses WebSocket API authentication with Ed25519 credentials.
/// Binance US uses HMAC-signed HTTP requests and listen-key user data streams.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.binance")
)]
pub struct BinanceExecutionClientConfig {
    /// Account ID for the client.
    #[builder(default = AccountId::from("BINANCE-001"))]
    pub account_id: AccountId,
    /// Product type to trade.
    #[builder(default = BinanceProductType::Spot)]
    pub product_type: BinanceProductType,
    /// Environment (live, testnet, or demo).
    #[builder(default = BinanceEnvironment::Live)]
    pub environment: BinanceEnvironment,
    /// Optional base URL override for HTTP API.
    pub base_url_http: Option<String>,
    /// Optional base URL override for WebSocket user data stream.
    ///
    /// Live USD-M Futures stream overrides are normalized onto the `/private/ws` route.
    pub base_url_ws: Option<String>,
    /// Optional base URL override for WebSocket trading API (Spot and USD-M Futures).
    pub base_url_ws_trading: Option<String>,
    /// Whether to use the WebSocket trading API for order operations (Spot and USD-M Futures).
    #[builder(default = true)]
    pub use_ws_trading: bool,
    /// Timeout in milliseconds for each Binance Spot WS trading setup response.
    #[builder(default = 10_000)]
    pub ws_trading_setup_timeout_ms: u64,
    /// Instrument loading and fee configuration.
    #[builder(default)]
    pub instrument_provider: BinanceInstrumentProviderConfig,
    /// Interval in seconds for refreshing the execution instrument cache.
    ///
    /// Set to 0 to disable. Defaults to 3600 (60 minutes).
    #[builder(default = 3600)]
    pub instrument_refresh_interval_secs: u64,
    /// Whether to use Binance-native GTD orders.
    ///
    /// Set to false only when the strategy manages GTD expiry locally. The adapter then maps GTD
    /// to GTC and the strategy must enable `manage_gtd_expiry`.
    #[builder(default = true)]
    pub use_gtd: bool,
    /// Whether to use canonical Binance Futures position IDs.
    ///
    /// When true, Futures hedge-mode order and fill reports include a `venue_position_id` derived
    /// from the instrument and Binance position side (e.g. `ETHUSDT-PERP.BINANCE-LONG`). Hedge-mode
    /// REST position reports use the same IDs. One-way `BOTH` reports remain unkeyed. When false,
    /// `venue_position_id` is None, allowing virtual positions with `OmsType::Hedging`.
    #[builder(default = true)]
    pub use_position_ids: bool,
    /// Optional OMS type override for Binance Futures accounts.
    ///
    /// Set to `Hedging` when the account uses dual-side position mode. When
    /// `None`, Binance Futures clients use `Netting`. Ignored for Spot clients.
    pub oms_type: Option<OmsType>,
    /// Default taker fee rate for commission estimation.
    ///
    /// Used as a fallback when the venue omits commission fields in
    /// exchange-generated fills (liquidation, ADL, settlement).
    /// Standard Binance Futures taker fee is 0.0004 (0.04%).
    #[builder(default = Decimal::new(4, 4))]
    pub default_taker_fee: Decimal,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// Receive window in milliseconds for signed HTTP requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Whether to route this Spot client to Binance US.
    #[builder(default)]
    pub us: bool,
    /// API key (uses an environment variable if not provided).
    pub api_key: Option<SecretString>,
    /// API secret (Ed25519 for Global or HMAC for Binance US).
    pub api_secret: Option<SecretString>,
    /// Initial leverage per Binance symbol (e.g. BTCUSDT -> 20), applied during connect.
    pub futures_leverages: Option<HashMap<String, u32>>,
    /// Margin type per Binance symbol (e.g. BTCUSDT -> Cross), applied during connect.
    pub futures_margin_types: Option<HashMap<String, BinanceMarginType>>,
    /// Currency that Binance Futures Credits (`BNFCR`) balances and fees resolve to (defaults to USDT).
    #[builder(default = Currency::USDT())]
    pub bnfcr_currency: Currency,
    /// If true, the EXPIRED execution type emits `OrderCanceled` instead of `OrderExpired`.
    ///
    /// Binance uses EXPIRED for certain cancel scenarios depending on order type
    /// and time-in-force combination.
    #[builder(default = false)]
    pub treat_expired_as_canceled: bool,
    /// If true, drive fills from the lower-latency `TRADE_LITE` user data event
    /// and dedup the matching fill portion of `ORDER_TRADE_UPDATE`. If false,
    /// `TRADE_LITE` events are ignored and fills come from `ORDER_TRADE_UPDATE`.
    #[builder(default = false)]
    pub use_trade_lite: bool,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BinanceExecutionClientConfig {
    account_id: AccountId,
    product_type: BinanceProductType,
    environment: BinanceEnvironment,
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    base_url_ws_trading: Option<String>,
    use_ws_trading: bool,
    ws_trading_setup_timeout_ms: u64,
    instrument_provider: BinanceInstrumentProviderConfig,
    instrument_refresh_interval_secs: u64,
    use_gtd: bool,
    use_position_ids: bool,
    oms_type: Option<OmsType>,
    default_taker_fee: Decimal,
    recv_window_ms: u64,
    us: bool,
    futures_leverages: Option<HashMap<String, u32>>,
    futures_margin_types: Option<HashMap<String, BinanceMarginType>>,
    treat_expired_as_canceled: bool,
    use_trade_lite: bool,
    bnfcr_currency: Currency,
    transport_backend: TransportBackend,
});

impl Default for BinanceExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BinanceExecutionClientConfig {
    /// Validates Binance execution client configuration.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid receive-window, WS trading setup timeout, provider, or
    /// Binance US settings.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_recv_window(self.recv_window_ms)?;
        anyhow::ensure!(
            self.ws_trading_setup_timeout_ms > 0,
            "ws_trading_setup_timeout_ms must be greater than 0, was {}",
            self.ws_trading_setup_timeout_ms
        );
        self.instrument_provider.validate(self.product_type)?;

        if self.us {
            anyhow::ensure!(
                self.product_type == BinanceProductType::Spot,
                "Binance US supports Spot clients only"
            );
            anyhow::ensure!(
                self.environment == BinanceEnvironment::Live,
                "Binance US supports the Live environment only"
            );
        }

        Ok(())
    }
}

fn validate_recv_window(recv_window_ms: u64) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=60_000).contains(&recv_window_ms),
        "recv_window_ms must be in the inclusive range 1..=60000, was {recv_window_ms}"
    );
    Ok(())
}

impl ClientConfig for BinanceExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_config_debug_redacts_credentials() {
        let data = BinanceDataClientConfig {
            api_key: Some("data-api-key".into()),
            api_secret: Some("data-api-secret".into()),
            proxy_url: Some("http://user:data-proxy@localhost".into()),
            ..Default::default()
        };
        let execution = BinanceExecutionClientConfig {
            api_key: Some("exec-api-key".into()),
            api_secret: Some("exec-api-secret".into()),
            proxy_url: Some("http://user:exec-proxy@localhost".into()),
            ..Default::default()
        };

        let formatted = format!("{data:?} {execution:?}");

        assert_eq!(formatted.matches(REDACTED).count(), 6);

        for secret in [
            "data-api-key",
            "data-api-secret",
            "data-proxy",
            "exec-api-key",
            "exec-api-secret",
            "exec-proxy",
        ] {
            assert!(!formatted.contains(secret));
        }
    }

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: BinanceDataClientConfig = toml::from_str(
            r#"
environment = "Testnet"
product_type = "USD_M"
instrument_status_poll_secs = 600
"#,
        )
        .unwrap();

        assert_eq!(config.environment, BinanceEnvironment::Testnet);
        assert_eq!(config.product_type, BinanceProductType::UsdM);
        assert_eq!(config.spot_market_data_mode, BinanceSpotMarketDataMode::Sbe);
        assert_eq!(config.instrument_status_poll_secs, 600);
    }

    #[rstest]
    fn test_data_config_toml_spot_market_data_mode_override() {
        let config: BinanceDataClientConfig = toml::from_str(
            r#"
spot_market_data_mode = "Json"
"#,
        )
        .unwrap();

        assert_eq!(
            config.spot_market_data_mode,
            BinanceSpotMarketDataMode::Json
        );
    }

    #[rstest]
    fn test_data_config_toml_rejects_plural_product_types() {
        let result = toml::from_str::<BinanceDataClientConfig>(
            r#"
product_types = ["SPOT", "USD_M"]
"#,
        );

        let message = result.unwrap_err().to_string();
        assert!(message.contains("unknown field `product_types`"));
    }

    #[rstest]
    fn test_exec_config_toml_empty_uses_defaults() {
        let config: BinanceExecutionClientConfig = toml::from_str("").unwrap();
        let expected = BinanceExecutionClientConfig::default();

        assert_eq!(config.environment, expected.environment);
        assert_eq!(config.product_type, expected.product_type);
        assert_eq!(config.use_ws_trading, expected.use_ws_trading);
        assert_eq!(config.ws_trading_setup_timeout_ms, 10_000);
        assert_eq!(config.instrument_provider, expected.instrument_provider);
        assert_eq!(
            config.instrument_refresh_interval_secs,
            expected.instrument_refresh_interval_secs
        );
        assert_eq!(config.use_gtd, expected.use_gtd);
        assert_eq!(config.use_position_ids, expected.use_position_ids);
        assert_eq!(config.oms_type, expected.oms_type);
        assert_eq!(config.default_taker_fee, expected.default_taker_fee);
        assert_eq!(config.proxy_url, expected.proxy_url);
        assert_eq!(config.recv_window_ms, expected.recv_window_ms);
        assert_eq!(config.us, expected.us);
        assert_eq!(
            config.treat_expired_as_canceled,
            expected.treat_expired_as_canceled,
        );
        assert_eq!(config.use_trade_lite, expected.use_trade_lite);
        assert_eq!(config.transport_backend, expected.transport_backend);
    }

    #[rstest]
    fn test_exec_config_toml_oms_type_override() {
        let config: BinanceExecutionClientConfig = toml::from_str(
            r#"
oms_type = "Hedging"
"#,
        )
        .unwrap();

        assert_eq!(config.oms_type, Some(OmsType::Hedging));
    }

    #[rstest]
    fn test_exec_config_toml_use_gtd_override() {
        let config: BinanceExecutionClientConfig = toml::from_str("use_gtd = false").unwrap();

        assert!(!config.use_gtd);
    }

    #[rstest]
    fn test_exec_config_toml_ws_trading_setup_timeout_override() {
        let config: BinanceExecutionClientConfig =
            toml::from_str("ws_trading_setup_timeout_ms = 250").unwrap();

        assert_eq!(config.ws_trading_setup_timeout_ms, 250);
    }

    #[rstest]
    #[case(0)]
    #[case(60_001)]
    fn test_data_config_rejects_recv_window_out_of_bounds(#[case] recv_window_ms: u64) {
        let config = BinanceDataClientConfig {
            recv_window_ms,
            ..Default::default()
        };

        let message = config.validate().unwrap_err().to_string();

        assert_eq!(
            message,
            format!(
                "recv_window_ms must be in the inclusive range 1..=60000, was {recv_window_ms}"
            )
        );
    }

    #[rstest]
    fn test_exec_config_rejects_zero_ws_trading_setup_timeout() {
        let config = BinanceExecutionClientConfig {
            ws_trading_setup_timeout_ms: 0,
            ..Default::default()
        };

        let message = config.validate().unwrap_err().to_string();

        assert_eq!(
            message,
            "ws_trading_setup_timeout_ms must be greater than 0, was 0"
        );
    }

    #[rstest]
    #[case(1)]
    #[case(60_000)]
    fn test_exec_config_accepts_recv_window_bounds(#[case] recv_window_ms: u64) {
        let config = BinanceExecutionClientConfig {
            recv_window_ms,
            ..Default::default()
        };

        assert!(config.validate().is_ok());
    }

    #[rstest]
    #[case(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        BinanceSpotMarketDataMode::Json,
        "Binance US supports Spot clients only"
    )]
    #[case(
        BinanceProductType::Spot,
        BinanceEnvironment::Testnet,
        BinanceSpotMarketDataMode::Json,
        "Binance US supports the Live environment only"
    )]
    #[case(
        BinanceProductType::Spot,
        BinanceEnvironment::Live,
        BinanceSpotMarketDataMode::Sbe,
        "Binance US market data requires spot_market_data_mode=Json"
    )]
    fn test_data_config_rejects_unsupported_binance_us_combinations(
        #[case] product_type: BinanceProductType,
        #[case] environment: BinanceEnvironment,
        #[case] spot_market_data_mode: BinanceSpotMarketDataMode,
        #[case] expected: &str,
    ) {
        let config = BinanceDataClientConfig {
            product_type,
            environment,
            spot_market_data_mode,
            us: true,
            ..Default::default()
        };

        assert_eq!(config.validate().unwrap_err().to_string(), expected);
    }

    #[rstest]
    #[case(
        BinanceProductType::CoinM,
        BinanceEnvironment::Live,
        "Binance US supports Spot clients only"
    )]
    #[case(
        BinanceProductType::Spot,
        BinanceEnvironment::Demo,
        "Binance US supports the Live environment only"
    )]
    fn test_exec_config_rejects_unsupported_binance_us_combinations(
        #[case] product_type: BinanceProductType,
        #[case] environment: BinanceEnvironment,
        #[case] expected: &str,
    ) {
        let config = BinanceExecutionClientConfig {
            product_type,
            environment,
            us: true,
            ..Default::default()
        };

        assert_eq!(config.validate().unwrap_err().to_string(), expected);
    }

    #[rstest]
    fn test_instrument_provider_rejects_callable_and_spot_contract_filter() {
        let callable = BinanceInstrumentProviderConfig {
            filter_callable: Some("package.module:predicate".to_string()),
            ..Default::default()
        };
        let contract_filter = BinanceInstrumentProviderConfig {
            filters: HashMap::from([(
                "contract_types".to_string(),
                serde_json::json!("PERPETUAL"),
            )]),
            ..Default::default()
        };

        assert_eq!(
            callable
                .validate(BinanceProductType::Spot)
                .unwrap_err()
                .to_string(),
            "Binance v2 does not support instrument filter_callable \"package.module:predicate\"; the legacy Binance provider never applied callable filters"
        );
        assert_eq!(
            contract_filter
                .validate(BinanceProductType::Spot)
                .unwrap_err()
                .to_string(),
            "unsupported Binance instrument filter \"contract_types\" for Spot"
        );
    }

    #[rstest]
    fn test_instrument_provider_rejects_empty_and_non_string_filter_values() {
        for value in [serde_json::json!([]), serde_json::json!(["BTC", 7])] {
            let config = BinanceInstrumentProviderConfig {
                filters: HashMap::from([("bases".to_string(), value)]),
                ..Default::default()
            };

            assert_eq!(
                config
                    .validate(BinanceProductType::UsdM)
                    .unwrap_err()
                    .to_string(),
                "Binance instrument filter \"bases\" must be a non-empty string or array of strings"
            );
        }
    }
}
