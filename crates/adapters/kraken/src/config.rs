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

//! Configuration types for Kraken data and execution clients.

use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, TraderId},
};
use nautilus_network::websocket::TransportBackend;

use crate::common::{
    enums::{KrakenEnvironment, KrakenProductType},
    urls::{get_kraken_http_base_url, get_kraken_ws_private_url, get_kraken_ws_public_url},
};

/// Configuration for the Kraken data client.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.kraken")
)]
pub struct KrakenDataClientConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    #[builder(default = KrakenProductType::Spot)]
    pub product_type: KrakenProductType,
    #[builder(default = KrakenEnvironment::Mainnet)]
    pub environment: KrakenEnvironment,
    pub base_url: Option<String>,
    pub ws_public_url: Option<String>,
    pub ws_private_url: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    #[builder(default = 30)]
    pub timeout_secs: u64,
    #[builder(default = 30)]
    pub heartbeat_interval_secs: u64,
    pub max_requests_per_second: Option<u32>,
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for KrakenDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl KrakenDataClientConfig {
    /// Returns true if both API key and secret are set.
    pub fn has_api_credentials(&self) -> bool {
        self.api_key.is_some() && self.api_secret.is_some()
    }

    /// Returns the HTTP base URL for the configured product type and environment.
    pub fn http_base_url(&self) -> String {
        self.base_url.clone().unwrap_or_else(|| {
            get_kraken_http_base_url(self.product_type, self.environment).to_string()
        })
    }

    /// Returns the public WebSocket URL for the configured product type and environment.
    pub fn ws_public_url(&self) -> String {
        self.ws_public_url.clone().unwrap_or_else(|| {
            get_kraken_ws_public_url(self.product_type, self.environment).to_string()
        })
    }

    /// Returns the private WebSocket URL for the configured product type and environment.
    pub fn ws_private_url(&self) -> String {
        self.ws_private_url.clone().unwrap_or_else(|| {
            get_kraken_ws_private_url(self.product_type, self.environment).to_string()
        })
    }
}

/// Configuration for the Kraken execution client.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.kraken")
)]
pub struct KrakenExecClientConfig {
    #[builder(default)]
    pub trader_id: TraderId,
    #[builder(default = AccountId::from("KRAKEN-001"))]
    pub account_id: AccountId,
    #[builder(default)]
    pub api_key: String,
    #[builder(default)]
    pub api_secret: String,
    #[builder(default = KrakenProductType::Spot)]
    pub product_type: KrakenProductType,
    #[builder(default = KrakenEnvironment::Mainnet)]
    pub environment: KrakenEnvironment,
    pub base_url: Option<String>,
    pub ws_url: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    #[builder(default = 30)]
    pub timeout_secs: u64,
    #[builder(default = 30)]
    pub heartbeat_interval_secs: u64,
    pub max_requests_per_second: Option<u32>,
    #[builder(default)]
    pub transport_backend: TransportBackend,

    /// Account type for spot trading (`Cash` or `Margin`).
    ///
    /// When set to `Margin`, the adapter calls `TradeBalance` for margin reporting
    /// and `OpenPositions` for position reconciliation.
    /// Per-order leverage is set via `SubmitOrder.params["leverage"]` (u16 multiplier).
    #[builder(default = AccountType::Cash)]
    pub spot_account_type: AccountType,

    /// Default leverage multiplier for spot margin orders when not overridden per-order.
    ///
    /// Sent as `"N:1"` to Kraken (e.g., `3` becomes `"3:1"`).
    /// Valid tiers per pair are in `AssetPairInfo.leverage_buy` / `leverage_sell`.
    /// `None` means cash orders (no leverage field sent).
    pub default_leverage: Option<u16>,

    /// Whether to generate `PositionStatusReport`s from spot wallet balances.
    ///
    /// Set `true` for spot-only (cash) accounts that need position tracking from
    /// balance snapshots. For margin accounts leave `false`; positions are
    /// reconciled via `OpenPositions` instead.
    #[builder(default = false)]
    pub use_spot_position_reports: bool,

    /// Quote currency used for synthetic spot position reports.
    ///
    /// Only relevant when `use_spot_position_reports` is `true`.
    #[builder(default = "USDT".to_string())]
    pub spot_positions_quote_currency: String,

    /// Summary-display asset for `TradeBalance` margin metrics.
    ///
    /// Controls the denomination of equity, free margin, used margin, and other
    /// summary figures returned by Kraken's `TradeBalance` endpoint (e.g. `"ZUSD"`,
    /// `"ZGBP"`, `"ZEUR"`, `"USDT"`). `None` lets Kraken default to `ZUSD`.
    /// Display-only: Kraken converts internally; per-position figures from
    /// `OpenPositions` remain in the traded pair's quote currency.
    pub margin_balance_asset: Option<String>,
}

impl Default for KrakenExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl KrakenExecClientConfig {
    /// Returns the HTTP base URL for the configured product type and environment.
    pub fn http_base_url(&self) -> String {
        self.base_url.clone().unwrap_or_else(|| {
            get_kraken_http_base_url(self.product_type, self.environment).to_string()
        })
    }

    /// Returns the WebSocket URL for the configured product type and environment.
    pub fn ws_url(&self) -> String {
        self.ws_url.clone().unwrap_or_else(|| {
            get_kraken_ws_private_url(self.product_type, self.environment).to_string()
        })
    }

    /// Validates config invariants.
    ///
    /// # Errors
    ///
    /// Returns an error if `default_leverage` is set on a Cash account.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.default_leverage.is_some() && self.spot_account_type == AccountType::Cash {
            anyhow::bail!("default_leverage requires spot_account_type=Margin");
        }
        Ok(())
    }
}
