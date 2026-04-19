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

use std::collections::{HashMap, HashSet};

use nautilus_model::identifiers::InstrumentId;
use serde::{Deserialize, Serialize};

use crate::common::consts::{DEFAULT_CLIENT_ID, DEFAULT_HOST, DEFAULT_PORT};

/// Market data type for switching between real-time and frozen/delayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
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
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        subclass,
        from_py_object
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
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        subclass,
        from_py_object
    )
)]
pub struct InteractiveBrokersExecClientConfig {
    /// Host for IB Gateway/TWS.
    #[builder(default = DEFAULT_HOST.to_string())]
    pub host: String,
    /// Port for IB Gateway/TWS.
    #[builder(default = DEFAULT_PORT)]
    pub port: u16,
    /// Client ID.
    #[builder(default = DEFAULT_CLIENT_ID)]
    pub client_id: i32,
    /// Account ID.
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
}

impl Default for InteractiveBrokersExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Symbology method for converting between IB contracts and Nautilus instrument IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
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
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        subclass,
        from_py_object
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
    pub load_contracts: Vec<serde_json::Value>,
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
    pub filter_sec_types: HashSet<String>,
    /// Path to cache file for persistent instrument caching (equivalent to pickle_path in Python).
    /// If provided, instruments will be cached to disk and loaded from cache if still valid.
    pub cache_path: Option<String>,
}

impl Default for InteractiveBrokersInstrumentProviderConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Trading mode for Dockerized IB Gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
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
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        subclass,
        from_py_object
    )
)]
pub struct DockerizedIBGatewayConfig {
    /// Username for IB account (falls back to `TWS_USERNAME` env var via [`Default`]).
    pub username: Option<String>,
    /// Password for IB account (falls back to `TWS_PASSWORD` env var via [`Default`]).
    pub password: Option<String>,
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
    /// Mask sensitive information for display.
    pub fn mask_sensitive_info(value: &str) -> String {
        if value.len() <= 2 {
            "*".repeat(value.len())
        } else {
            format!(
                "{}{}{}",
                &value[0..1],
                "*".repeat(value.len() - 2),
                &value[value.len() - 1..]
            )
        }
    }

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

impl Default for DockerizedIBGatewayConfig {
    fn default() -> Self {
        Self::builder()
            .maybe_username(std::env::var("TWS_USERNAME").ok())
            .maybe_password(std::env::var("TWS_PASSWORD").ok())
            .build()
    }
}
