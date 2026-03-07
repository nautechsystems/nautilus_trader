// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use std::fmt;

use crate::common::enums::{BitgetEnvironment, BitgetProductType};

#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget", from_py_object)
)]
pub struct BitgetDataClientConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub api_passphrase: Option<String>,
    pub environment: BitgetEnvironment,
    pub product_types: Vec<BitgetProductType>,
    pub base_url_http: Option<String>,
    pub base_url_ws_public: Option<String>,
    pub base_url_ws_private: Option<String>,
    pub max_retries: Option<u32>,
    pub retry_delay_initial_ms: Option<u64>,
    pub retry_delay_max_ms: Option<u64>,
    pub update_instruments_interval_mins: Option<u64>,
}

impl fmt::Debug for BitgetDataClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BitgetDataClientConfig")
            .field("api_key_set", &self.api_key.is_some())
            .field("api_secret_set", &self.api_secret.is_some())
            .field("api_passphrase_set", &self.api_passphrase.is_some())
            .field("environment", &self.environment)
            .field("product_types", &self.product_types)
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws_public", &self.base_url_ws_public)
            .field("base_url_ws_private", &self.base_url_ws_private)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .field(
                "update_instruments_interval_mins",
                &self.update_instruments_interval_mins,
            )
            .finish()
    }
}

impl Default for BitgetDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            api_passphrase: None,
            environment: BitgetEnvironment::Mainnet,
            product_types: vec![
                BitgetProductType::Spot,
                BitgetProductType::UsdtFutures,
                BitgetProductType::CoinFutures,
                BitgetProductType::UsdcFutures,
            ],
            base_url_http: None,
            base_url_ws_public: None,
            base_url_ws_private: None,
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
            update_instruments_interval_mins: Some(60),
        }
    }
}

impl BitgetDataClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget", from_py_object)
)]
pub struct BitgetExecClientConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub api_passphrase: Option<String>,
    pub environment: BitgetEnvironment,
    pub product_types: Vec<BitgetProductType>,
    pub base_url_http: Option<String>,
    pub base_url_ws_private: Option<String>,
    pub max_retries: Option<u32>,
    pub retry_delay_initial_ms: Option<u64>,
    pub retry_delay_max_ms: Option<u64>,
}

impl fmt::Debug for BitgetExecClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BitgetExecClientConfig")
            .field("api_key_set", &self.api_key.is_some())
            .field("api_secret_set", &self.api_secret.is_some())
            .field("api_passphrase_set", &self.api_passphrase.is_some())
            .field("environment", &self.environment)
            .field("product_types", &self.product_types)
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws_private", &self.base_url_ws_private)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .finish()
    }
}

impl Default for BitgetExecClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            api_passphrase: None,
            environment: BitgetEnvironment::Mainnet,
            product_types: vec![
                BitgetProductType::Spot,
                BitgetProductType::UsdtFutures,
                BitgetProductType::CoinFutures,
                BitgetProductType::UsdcFutures,
            ],
            base_url_http: None,
            base_url_ws_private: None,
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
        }
    }
}

impl BitgetExecClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}
