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

//! Configuration types for the Interactive Brokers adapter.

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
};

use nautilus_core::string::secret::SecretString;
use nautilus_model::identifiers::InstrumentId;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

use crate::common::{
    consts::{DEFAULT_CLIENT_ID, DEFAULT_HOST, DEFAULT_PORT},
    contracts::{ConfiguredContract, parse_configured_contract_from_json},
    enums::IbSecurityType,
};

/// Market data type for switching between real-time and frozen/delayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
#[derive(Default)]
pub enum MarketDataType {
    /// Live market data
    #[default]
    Realtime = 1,
    /// Frozen market data (for when market is closed)
    Frozen = 2,
    /// Delayed market data (usually 15-20 minutes)
    Delayed = 3,
    /// Delayed frozen market data
    DelayedFrozen = 4,
}

impl From<MarketDataType> for ibapi::market_data::MarketDataType {
    fn from(data_type: MarketDataType) -> Self {
        match data_type {
            MarketDataType::Realtime => Self::Realtime,
            MarketDataType::Frozen => Self::Frozen,
            MarketDataType::Delayed => Self::Delayed,
            MarketDataType::DelayedFrozen => Self::DelayedFrozen,
        }
    }
}

/// Configuration for Interactive Brokers data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        subclass,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
pub struct InteractiveBrokersDataClientConfig {
    /// Host for IB Gateway/TWS.
    #[builder(default = DEFAULT_HOST.to_string())]
    pub host: String,
    /// Port for IB Gateway/TWS.
    #[builder(default = DEFAULT_PORT)]
    pub port: u16,
    /// Client ID.
    #[builder(default = DEFAULT_CLIENT_ID)]
    pub client_id: i32,
    /// Whether to use regular trading hours only (RTH filtering).
    #[builder(default = true)]
    pub use_regular_trading_hours: bool,
    /// Market data type (realtime, delayed, frozen).
    #[builder(default)]
    pub market_data_type: MarketDataType,
    /// Whether to ignore quote tick size updates (filters size-only updates).
    #[builder(default)]
    pub ignore_quote_tick_size_updates: bool,
    /// Connection timeout in seconds.
    #[builder(default = 300)]
    pub connection_timeout: u64,
    /// Request timeout in seconds. Applied to IB API requests (open orders, executions, positions,
    /// account summary, order update stream, next order id). See execution/core.rs and
    /// execution/account.rs for call sites.
    #[builder(default = 60)]
    pub request_timeout: u64,
    /// Whether to handle revised bars.
    #[builder(default)]
    pub handle_revised_bars: bool,
    /// Whether to use batch quotes (reqMktData) by default instead of tick-by-tick.
    #[builder(default = true)]
    pub batch_quotes: bool,
    /// Whether to include special-condition trades in tick-by-tick subscriptions.
    #[builder(default = true)]
    pub all_last_trades: bool,
    /// Optional interval without incoming market data before emitting a subscription idle event.
    pub subscription_idle_timeout_secs: Option<u64>,
    /// Instrument provider configuration.
    #[builder(default)]
    pub instrument_provider: InteractiveBrokersInstrumentProviderConfig,
}

impl InteractiveBrokersDataClientConfig {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.subscription_idle_timeout_secs != Some(0),
            "subscription_idle_timeout_secs must be positive when set",
        );

        if let Some(timeout) = self.subscription_idle_timeout_secs {
            anyhow::ensure!(
                std::time::Instant::now()
                    .checked_add(std::time::Duration::from_secs(timeout))
                    .is_some(),
                "subscription_idle_timeout_secs exceeds the supported clock range",
            );
        }
        Ok(())
    }
}

impl Default for InteractiveBrokersDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Configuration for Interactive Brokers execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        subclass,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
pub struct InteractiveBrokersExecutionClientConfig {
    /// Host for IB Gateway/TWS.
    #[builder(default = DEFAULT_HOST.to_string())]
    pub host: String,
    /// Port for IB Gateway/TWS.
    #[builder(default = DEFAULT_PORT)]
    pub port: u16,
    /// Client ID.
    #[builder(default = DEFAULT_CLIENT_ID)]
    pub client_id: i32,
    /// Raw IB account code, such as `DU123456`. The Nautilus account ID becomes
    /// `{client name}-{code}`.
    pub account_id: Option<String>,
    /// Connection timeout in seconds.
    #[builder(default = 300)]
    pub connection_timeout: u64,
    /// Request timeout in seconds for IB API requests (open orders, executions, positions, etc.).
    #[builder(default = 60)]
    pub request_timeout: u64,
    /// Whether to fetch all open orders (reqAllOpenOrders vs reqOpenOrders).
    #[builder(default)]
    pub fetch_all_open_orders: bool,
    /// Whether to track option exercise from position updates.
    #[builder(default)]
    pub track_option_exercise_from_position_update: bool,
    /// Instrument provider configuration.
    #[builder(default)]
    pub instrument_provider: InteractiveBrokersInstrumentProviderConfig,
}

impl Default for InteractiveBrokersExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Symbology method for converting between IB contracts and Nautilus instrument IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
#[derive(Default)]
pub enum SymbologyMethod {
    /// Simplified symbology: clean, readable symbols (e.g., "EUR/USD", "ESM23")
    #[serde(rename = "simplified")]
    #[default]
    Simplified,
    /// Raw symbology: preserves IB raw format with security type suffix (e.g., "EUR.USD=CASH", "AAPL=STK")
    #[serde(rename = "raw")]
    Raw,
}

/// Configuration for Interactive Brokers instrument provider.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        subclass,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
pub struct InteractiveBrokersInstrumentProviderConfig {
    /// Symbology method to use for instrument ID conversion.
    #[builder(default)]
    pub symbology_method: SymbologyMethod,
    /// Instrument IDs to load on startup.
    #[builder(default)]
    pub load_ids: HashSet<InstrumentId>,
    /// IB contracts to load on startup.
    #[builder(default)]
    #[serde(
        deserialize_with = "deserialize_configured_contracts",
        serialize_with = "serialize_configured_contracts"
    )]
    pub load_contracts: Vec<ConfiguredContract>,
    /// Minimum expiry days for options and futures chains.
    pub min_expiry_days: Option<u32>,
    /// Maximum expiry days for options and futures chains.
    pub max_expiry_days: Option<u32>,
    /// Whether to build full options chain.
    pub build_options_chain: Option<bool>,
    /// Whether to build full futures chain.
    pub build_futures_chain: Option<bool>,
    /// Cache validity in days (None means no caching).
    pub cache_validity_days: Option<u32>,
    /// Whether to convert IB exchanges to MIC venues.
    #[builder(default)]
    pub convert_exchange_to_mic_venue: bool,
    /// Symbol to MIC venue mapping override.
    #[builder(default)]
    pub symbol_to_mic_venue: HashMap<String, String>,
    /// Security types to filter out.
    #[builder(default)]
    pub filter_sec_types: HashSet<IbSecurityType>,
    /// Fully-qualified Python callable path for custom instrument filtering.
    ///
    /// Configuring this without the Python feature enabled is an error.
    pub filter_callable: Option<String>,
    /// Path to cache file for persistent instrument caching (equivalent to pickle_path in Python).
    /// If provided, instruments will be cached to disk and loaded from cache if still valid.
    pub cache_path: Option<String>,
}

impl Default for InteractiveBrokersInstrumentProviderConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

fn deserialize_configured_contracts<'de, D>(
    deserializer: D,
) -> Result<Vec<ConfiguredContract>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<serde_json::Value>::deserialize(deserializer)?
        .iter()
        .map(parse_configured_contract_from_json)
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(D::Error::custom)
}

fn serialize_configured_contracts<S>(
    contracts: &[ConfiguredContract],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    contracts
        .iter()
        .map(ConfiguredContract::to_json_value)
        .collect::<Vec<_>>()
        .serialize(serializer)
}

/// Trading mode for Dockerized IB Gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
#[derive(Default)]
pub enum TradingMode {
    /// Paper trading mode.
    #[serde(rename = "paper")]
    #[default]
    Paper,
    /// Live trading mode.
    #[serde(rename = "live")]
    Live,
}

/// Configuration for Dockerized IB Gateway.
///
/// This configuration is for managing containerized IB Gateway instances.
/// It supports environment variable loading and sensitive data masking.
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        subclass,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
pub struct DockerizedIBGatewayConfig {
    /// Username for IB account (falls back to `TWS_USERNAME` env var via [`Default`]).
    pub username: Option<SecretString>,
    /// Password for IB account (falls back to `TWS_PASSWORD` env var via [`Default`]).
    #[serde(skip_serializing)]
    pub password: Option<SecretString>,
    /// Trading mode (paper or live).
    #[builder(default)]
    pub trading_mode: TradingMode,
    /// Whether to enable read-only API mode.
    #[builder(default = true)]
    pub read_only_api: bool,
    /// Timeout in seconds for container startup.
    #[builder(default = 300)]
    pub timeout: u64,
    /// Container image reference.
    #[builder(default = "ghcr.io/gnzsnz/ib-gateway:stable".to_string())]
    pub container_image: String,
    /// VNC port for remote desktop access (None to disable).
    pub vnc_port: Option<u16>,
}

impl DockerizedIBGatewayConfig {
    /// Validate configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.timeout == 0 {
            anyhow::bail!("Timeout must be greater than 0");
        }

        if self.timeout > 3600 {
            anyhow::bail!("Timeout must be less than 3600 seconds");
        }

        if let Some(port) = self.vnc_port
            && (!(5900..=5999).contains(&port))
        {
            anyhow::bail!("VNC port must be between 5900 and 5999");
        }

        Ok(())
    }
}

impl Debug for DockerizedIBGatewayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DockerizedIBGatewayConfig))
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("trading_mode", &self.trading_mode)
            .field("read_only_api", &self.read_only_api)
            .field("timeout", &self.timeout)
            .field("container_image", &self.container_image)
            .field("vnc_port", &self.vnc_port)
            .finish()
    }
}

impl Default for DockerizedIBGatewayConfig {
    fn default() -> Self {
        Self::builder()
            .maybe_username(std::env::var("TWS_USERNAME").ok().map(SecretString::from))
            .maybe_password(std::env::var("TWS_PASSWORD").ok().map(SecretString::from))
            .build()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{DockerizedIBGatewayConfig, InteractiveBrokersInstrumentProviderConfig};
    use crate::common::enums::IbSecurityType;

    #[rstest]
    fn test_gateway_password_is_redacted_from_debug_and_serialization() {
        let config = DockerizedIBGatewayConfig::builder()
            .username(String::from("user").into())
            .password(String::from("secret-password").into())
            .build();

        let debug = format!("{config:?}");
        let json = serde_json::to_string(&config).unwrap();

        assert!(!debug.contains("secret-password"));
        assert!(debug.contains("<redacted>"));
        assert!(!json.contains("secret-password"));
        assert!(!json.contains("password"));
    }

    #[rstest]
    fn test_instrument_provider_config_decodes_typed_contracts_and_filters() {
        let config: InteractiveBrokersInstrumentProviderConfig =
            serde_json::from_value(serde_json::json!({
                "load_contracts": [{
                    "secType": "STK",
                    "symbol": "AAPL",
                    "exchange": "SMART",
                    "conId": 265598
                }],
                "filter_sec_types": ["opt"]
            }))
            .unwrap();

        assert_eq!(config.load_contracts.len(), 1);
        assert_eq!(config.load_contracts[0].contract.contract_id, 265598);
        assert_eq!(
            config.load_contracts[0].contract.security_type,
            ibapi::contracts::SecurityType::Stock
        );
        assert!(config.filter_sec_types.contains(&IbSecurityType::Option));
    }

    #[rstest]
    fn test_instrument_provider_config_keeps_per_contract_chain_settings() {
        let config: InteractiveBrokersInstrumentProviderConfig =
            serde_json::from_value(serde_json::json!({
                "build_futures_chain": true,
                "load_contracts": [
                    {
                        "secType": "STK",
                        "symbol": "SPY",
                        "exchange": "SMART",
                        "primaryExchange": "ARCA",
                        "build_options_chain": true,
                        "min_expiry_days": 7,
                        "max_expiry_days": 14
                    },
                    {
                        "secType": "FUT",
                        "exchange": "NYMEX",
                        "localSymbol": "CLZ6",
                        "build_futures_chain": false
                    }
                ]
            }))
            .unwrap();

        let spy = &config.load_contracts[0];
        assert_eq!(spy.build_options_chain, Some(true));
        assert_eq!(spy.build_futures_chain, None);
        assert_eq!(spy.min_expiry_days, Some(7));
        assert_eq!(spy.max_expiry_days, Some(14));
        assert_eq!(spy.options_chain_exchange, None);
        assert_eq!(
            spy.chain_spec_json().unwrap(),
            serde_json::json!({
                "build_options_chain": true,
                "min_expiry_days": 7,
                "max_expiry_days": 14
            })
        );

        let cl = &config.load_contracts[1];
        assert_eq!(cl.build_futures_chain, Some(false));
        assert_eq!(
            cl.chain_spec_json().unwrap(),
            serde_json::json!({"build_futures_chain": false})
        );

        let round_trip = serde_json::to_value(&config).unwrap();
        assert_eq!(
            round_trip["load_contracts"][0]["build_options_chain"],
            serde_json::json!(true)
        );
        assert_eq!(
            round_trip["load_contracts"][1]["build_futures_chain"],
            serde_json::json!(false)
        );
    }

    #[rstest]
    fn test_instrument_provider_config_accepts_con_id_only_contract() {
        let config: InteractiveBrokersInstrumentProviderConfig =
            serde_json::from_value(serde_json::json!({
                "load_contracts": [{"conId": 265598}]
            }))
            .unwrap();

        assert_eq!(config.load_contracts[0].contract.contract_id, 265598);
        assert_eq!(
            config.load_contracts[0].contract.security_type,
            ibapi::contracts::SecurityType::Stock
        );
    }

    #[rstest]
    #[case(
        serde_json::json!({"load_contracts": [{"secType": "UNKNOWN"}]}),
        "Unknown IB security type: UNKNOWN"
    )]
    #[case(
        serde_json::json!({"load_contracts": [{"secType": "STK", "conId": "265598"}]}),
        "Configured contract field 'conId' must be an integer"
    )]
    #[case(
        serde_json::json!({"load_contracts": [{"symbol": "SPY"}]}),
        "Configured contract requires 'secType' as a known IB security type string, or a positive 'conId'"
    )]
    #[case(
        serde_json::json!({"load_contracts": [{"secType": "STK", "build_options_chain": "yes"}]}),
        "Configured contract field 'build_options_chain' must be a boolean"
    )]
    #[case(
        serde_json::json!({"load_contracts": [{"secType": "STK", "min_expiry_days": -1}]}),
        "Configured contract field 'min_expiry_days' must be a non-negative integer"
    )]
    #[case(
        serde_json::json!({"filter_sec_types": ["UNKNOWN"]}),
        "Unknown IB security type: UNKNOWN"
    )]
    fn test_instrument_provider_config_rejects_invalid_typed_values(
        #[case] value: serde_json::Value,
        #[case] expected: &str,
    ) {
        let error = serde_json::from_value::<InteractiveBrokersInstrumentProviderConfig>(value)
            .unwrap_err();

        assert!(error.to_string().contains(expected), "{error}");
    }
}
