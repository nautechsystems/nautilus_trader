//! Binance adapter configuration structures.

use std::any::Any;

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_system::factories::ClientConfig;

use crate::common::enums::{BinanceEnvironment, BinanceProductType};

/// Configuration for Binance data client.
///
/// Ed25519 API keys are required for SBE WebSocket streams.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
)]
pub struct BinanceDataClientConfig {
    /// Product types to subscribe to.
    pub product_types: Vec<BinanceProductType>,
    /// Environment (mainnet or testnet).
    pub environment: BinanceEnvironment,
    /// Optional base URL override for HTTP API.
    pub base_url_http: Option<String>,
    /// Optional base URL override for WebSocket.
    pub base_url_ws: Option<String>,
    /// API key (Ed25519).
    pub api_key: Option<String>,
    /// API secret (Ed25519 base64-encoded or PEM).
    pub api_secret: Option<String>,
}

impl Default for BinanceDataClientConfig {
    fn default() -> Self {
        Self {
            product_types: vec![BinanceProductType::Spot],
            environment: BinanceEnvironment::Mainnet,
            base_url_http: None,
            base_url_ws: None,
            api_key: None,
            api_secret: None,
        }
    }
}

impl ClientConfig for BinanceDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Configuration for Binance execution client.
///
/// Ed25519 API keys are required for execution clients. Binance deprecated
/// listenKey-based user data streams in favor of WebSocket API authentication,
/// which only supports Ed25519.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
)]
pub struct BinanceExecClientConfig {
    /// Trader ID for the client.
    pub trader_id: TraderId,
    /// Account ID for the client.
    pub account_id: AccountId,
    /// Product types to trade.
    pub product_types: Vec<BinanceProductType>,
    /// Environment (mainnet or testnet).
    pub environment: BinanceEnvironment,
    /// Optional base URL override for HTTP API.
    pub base_url_http: Option<String>,
    /// Optional base URL override for WebSocket.
    pub base_url_ws: Option<String>,
    /// API key (Ed25519 required, uses env var if not provided).
    pub api_key: Option<String>,
    /// API secret (Ed25519 base64-encoded, required, uses env var if not provided).
    pub api_secret: Option<String>,
}

impl Default for BinanceExecClientConfig {
    fn default() -> Self {
        Self {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("BINANCE-001"),
            product_types: vec![BinanceProductType::Spot],
            environment: BinanceEnvironment::Mainnet,
            base_url_http: None,
            base_url_ws: None,
            api_key: None,
            api_secret: None,
        }
    }
}

impl ClientConfig for BinanceExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
