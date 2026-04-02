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

//! Configuration structures for the Polymarket adapter.

use std::{fmt::Debug, sync::Arc};

use nautilus_model::identifiers::{AccountId, TraderId};

use crate::{
    common::{enums::SignatureType, urls},
    filters::InstrumentFilter,
};

/// Configuration for the Polymarket data client.
#[derive(bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.polymarket")
)]
pub struct PolymarketDataClientConfig {
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_gamma: Option<String>,
    pub base_url_data_api: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    #[builder(default = crate::common::consts::WS_DEFAULT_SUBSCRIPTIONS)]
    pub ws_max_subscriptions: usize,
    /// Instrument reload interval in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// Whether to subscribe to new market discovery events via WebSocket.
    #[builder(default)]
    pub subscribe_new_markets: bool,
    /// Instrument filters applied to all instruments during loading and discovery.
    #[builder(default)]
    pub filters: Vec<Arc<dyn InstrumentFilter>>,
    /// Optional filter applied to newly discovered markets before instrument emission.
    pub new_market_filter: Option<Arc<dyn InstrumentFilter>>,
}

impl Clone for PolymarketDataClientConfig {
    fn clone(&self) -> Self {
        Self {
            base_url_http: self.base_url_http.clone(),
            base_url_ws: self.base_url_ws.clone(),
            base_url_gamma: self.base_url_gamma.clone(),
            base_url_data_api: self.base_url_data_api.clone(),
            http_timeout_secs: self.http_timeout_secs,
            ws_timeout_secs: self.ws_timeout_secs,
            ws_max_subscriptions: self.ws_max_subscriptions,
            update_instruments_interval_mins: self.update_instruments_interval_mins,
            subscribe_new_markets: self.subscribe_new_markets,
            filters: self.filters.clone(),
            new_market_filter: self.new_market_filter.clone(),
        }
    }
}

impl Debug for PolymarketDataClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PolymarketDataClientConfig))
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws", &self.base_url_ws)
            .field("base_url_gamma", &self.base_url_gamma)
            .field("base_url_data_api", &self.base_url_data_api)
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("ws_timeout_secs", &self.ws_timeout_secs)
            .field("ws_max_subscriptions", &self.ws_max_subscriptions)
            .field(
                "update_instruments_interval_mins",
                &self.update_instruments_interval_mins,
            )
            .field("subscribe_new_markets", &self.subscribe_new_markets)
            .field("filters", &self.filters)
            .field("new_market_filter", &self.new_market_filter)
            .finish()
    }
}

impl Default for PolymarketDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl PolymarketDataClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| urls::clob_http_url().to_string())
    }

    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::clob_ws_url().to_string())
    }

    #[must_use]
    pub fn gamma_url(&self) -> String {
        self.base_url_gamma
            .clone()
            .unwrap_or_else(|| urls::gamma_api_url().to_string())
    }

    #[must_use]
    pub fn data_api_url(&self) -> String {
        self.base_url_data_api
            .clone()
            .unwrap_or_else(|| "https://data-api.polymarket.com".to_string())
    }
}

/// Configuration for the Polymarket execution client.
#[derive(bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.polymarket")
)]
pub struct PolymarketExecClientConfig {
    #[builder(default)]
    pub trader_id: TraderId,
    #[builder(default = AccountId::from("POLYMARKET-001"))]
    pub account_id: AccountId,
    /// Falls back to `POLYMARKET_PK` env var.
    pub private_key: Option<String>,
    /// Falls back to `POLYMARKET_API_KEY` env var.
    pub api_key: Option<String>,
    /// Falls back to `POLYMARKET_API_SECRET` env var.
    pub api_secret: Option<String>,
    /// Falls back to `POLYMARKET_PASSPHRASE` env var.
    pub passphrase: Option<String>,
    /// Falls back to `POLYMARKET_FUNDER` env var.
    pub funder: Option<String>,
    #[builder(default = SignatureType::Eoa)]
    pub signature_type: SignatureType,
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_data_api: Option<String>,
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    #[builder(default = 3)]
    pub max_retries: u32,
    #[builder(default = 1000)]
    pub retry_delay_initial_ms: u64,
    #[builder(default = 10000)]
    pub retry_delay_max_ms: u64,
    /// Timeout waiting for WS order acknowledgment (seconds).
    #[builder(default = 5)]
    pub ack_timeout_secs: u64,
}

impl Clone for PolymarketExecClientConfig {
    fn clone(&self) -> Self {
        Self {
            trader_id: self.trader_id,
            account_id: self.account_id,
            private_key: self.private_key.clone(),
            api_key: self.api_key.clone(),
            api_secret: self.api_secret.clone(),
            passphrase: self.passphrase.clone(),
            funder: self.funder.clone(),
            signature_type: self.signature_type,
            base_url_http: self.base_url_http.clone(),
            base_url_ws: self.base_url_ws.clone(),
            base_url_data_api: self.base_url_data_api.clone(),
            http_timeout_secs: self.http_timeout_secs,
            max_retries: self.max_retries,
            retry_delay_initial_ms: self.retry_delay_initial_ms,
            retry_delay_max_ms: self.retry_delay_max_ms,
            ack_timeout_secs: self.ack_timeout_secs,
        }
    }
}

impl Debug for PolymarketExecClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PolymarketExecClientConfig))
            .field("trader_id", &self.trader_id)
            .field("account_id", &self.account_id)
            .field("private_key", &"***")
            .field("api_key", &"***")
            .field("api_secret", &"***")
            .field("passphrase", &"***")
            .field("funder", &self.funder)
            .field("signature_type", &self.signature_type)
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws", &self.base_url_ws)
            .field("base_url_data_api", &self.base_url_data_api)
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .field("ack_timeout_secs", &self.ack_timeout_secs)
            .finish()
    }
}

impl Default for PolymarketExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl PolymarketExecClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || self
                .api_key
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    }

    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| urls::clob_http_url().to_string())
    }

    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::clob_ws_url().to_string())
    }

    #[must_use]
    pub fn data_api_url(&self) -> String {
        self.base_url_data_api
            .clone()
            .unwrap_or_else(|| "https://data-api.polymarket.com".to_string())
    }
}
