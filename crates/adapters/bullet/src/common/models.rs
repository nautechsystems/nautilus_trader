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

//! Typed response models for the Bullet REST API.
//!
//! These types map directly to the OpenAPI schema at `/docs/rest/openapi.json`.
//! All decimal fields are strings on the wire (Binance FAPI convention).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ── Shared ────────────────────────────────────────────────────────────────────

/// Error response from the Bullet REST API.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorResponse {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API error {}: {}", self.status, self.message)
    }
}

// ── exchangeInfo ──────────────────────────────────────────────────────────────

/// Response from `GET /fapi/v1/exchangeInfo`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeInfo {
    pub assets: Vec<Asset>,
    pub symbols: Vec<SymbolInfo>,
    /// Hex-encoded 32-byte chain hash, e.g. `"0x1a2b..."`.
    pub chain_hash: Option<String>,
    pub chain_info: Option<ChainInfo>,
    pub rate_limits: Vec<RateLimit>,
}

impl ExchangeInfo {
    /// Decode `chain_hash` to raw `[u8; 32]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the field is absent or not valid 32-byte hex.
    pub fn decode_chain_hash(&self) -> Result<[u8; 32], crate::common::error::BulletError> {
        let hex_str = self.chain_hash.as_deref().ok_or_else(|| {
            crate::common::error::BulletError::Parse("exchangeInfo missing chainHash".to_string())
        })?;
        let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let bytes = ::hex::decode(hex_str).map_err(|e| {
            crate::common::error::BulletError::Parse(format!("invalid chainHash hex: {e}"))
        })?;
        bytes.try_into().map_err(|_| {
            crate::common::error::BulletError::Parse(
                "chainHash must be exactly 32 bytes".to_string(),
            )
        })
    }
}

/// Chain-level constants embedded in `exchangeInfo`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    pub chain_id: u64,
    pub chain_name: String,
}

/// Rate limit descriptor.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub rate_limit_type: String,
    pub interval: String,
    pub interval_num: i32,
    pub limit: i32,
}

/// Asset metadata from `exchangeInfo`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub asset: String,
    pub asset_id: u16,
    pub margin_available: bool,
    pub decimals: u8,
}

/// Symbol (market) metadata from `exchangeInfo`.
///
/// Maps to the `Symbol` schema in the OpenAPI spec.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfo {
    pub market_id: u16,
    pub symbol: String,
    pub pair: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub margin_asset: String,
    pub price_precision: u8,
    pub quantity_precision: u8,
    pub base_asset_precision: u8,
    pub quote_precision: u8,
    pub status: String,
    pub maker_fee_bps: Vec<String>,
    pub taker_fee_bps: Vec<String>,
    /// Raw filter objects; we scan for `PRICE_FILTER` and `LOT_SIZE` by type.
    pub filters: Vec<serde_json::Value>,
}

impl SymbolInfo {
    /// Extract `tickSize` from the `PRICE_FILTER` filter entry, if present.
    #[must_use]
    pub fn tick_size(&self) -> Option<Decimal> {
        self.filters.iter().find_map(|f| {
            if f.get("filterType")?.as_str()? == "PRICE_FILTER" {
                f.get("tickSize")?.as_str()?.parse().ok()
            } else {
                None
            }
        })
    }

    /// Extract `stepSize` from the `LOT_SIZE` filter entry, if present.
    #[must_use]
    pub fn step_size(&self) -> Option<Decimal> {
        self.filters.iter().find_map(|f| {
            if f.get("filterType")?.as_str()? == "LOT_SIZE" {
                f.get("stepSize")?.as_str()?.parse().ok()
            } else {
                None
            }
        })
    }
}

// ── depth ─────────────────────────────────────────────────────────────────────

/// Response from `GET /fapi/v1/depth`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderBook {
    pub last_update_id: u64,
    /// UTC timestamp millis.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Bids as `[[price, qty], ...]`.
    pub bids: Vec<[String; 2]>,
    /// Asks as `[[price, qty], ...]`.
    pub asks: Vec<[String; 2]>,
}

// ── account ───────────────────────────────────────────────────────────────────

/// Response from `GET /fapi/v3/account`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub total_wallet_balance: Decimal,
    pub total_unrealized_profit: Decimal,
    pub total_margin_balance: Decimal,
    pub available_balance: Decimal,
    pub assets: Vec<AccountAsset>,
    pub positions: Vec<AccountPosition>,
}

/// Per-asset balance in `Account`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountAsset {
    pub asset: String,
    pub asset_id: u16,
    pub wallet_balance: Decimal,
    pub update_time: i64,
}

/// Per-position info in `Account`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPosition {
    pub symbol: String,
    pub market_id: u16,
    pub position_amt: Decimal,
    pub entry_price: Decimal,
    pub unrealized_profit: Decimal,
    pub initial_margin: Decimal,
    pub maint_margin: Decimal,
    pub position_side: String,
    pub update_time: i64,
}

// ── balance ───────────────────────────────────────────────────────────────────

/// One entry in the `GET /fapi/v3/balance` response.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub asset: String,
    pub balance: Decimal,
    pub available_balance: Decimal,
    pub cross_wallet_balance: Decimal,
    pub cross_un_pnl: Decimal,
    pub max_withdraw_amount: Decimal,
    pub margin_available: bool,
    pub update_time: i64,
}

// ── open orders ───────────────────────────────────────────────────────────────

/// Open order from `GET /fapi/v1/openOrders` or `GET /fapi/v1/openOrder`.
///
/// Maps to the `BinanceOrder` schema in the OpenAPI spec.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrder {
    pub order_id: u64,
    pub symbol: String,
    pub status: String,
    pub price: Decimal,
    pub avg_price: Decimal,
    pub orig_qty: Decimal,
    pub executed_qty: Decimal,
    pub cum_qty: Decimal,
    pub side: String,
    pub order_type: String,
    pub reduce_only: bool,
    pub client_order_id: Option<String>,
    pub update_time: i64,
}

// ── funding rate ──────────────────────────────────────────────────────────────

/// Response from `GET /fapi/v1/fundingRate`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingRate {
    pub symbol: String,
    pub funding_rate: Decimal,
    pub funding_time: i64,
    pub mark_price: Decimal,
}

// ── transaction submission ────────────────────────────────────────────────────

/// Request body for `POST /tx/submit`.
#[derive(Debug, Serialize)]
pub struct SubmitTxRequest {
    /// Base64-encoded borsh-serialized signed transaction.
    pub body: String,
}

/// Response from `POST /tx/submit`.
#[derive(Debug, Clone, Deserialize)]
pub struct SubmitTxResponse {
    /// Transaction hash assigned by the rollup.
    pub id: String,
    pub status: String,
}

// ── rollup constants ──────────────────────────────────────────────────────────

/// Response from `GET /rollup/constants`.
#[derive(Debug, Clone, Deserialize)]
pub struct RollupConstants {
    pub chain_id: u64,
    pub chain_name: String,
}

// ── instrument cache ──────────────────────────────────────────────────────────

/// Per-symbol precision and sizing derived from `exchangeInfo`.
#[derive(Debug, Clone)]
pub struct SymbolPrecision {
    pub market_id: u16,
    pub symbol: String,
    pub price_precision: u8,
    pub quantity_precision: u8,
    pub tick_size: Option<Decimal>,
    pub step_size: Option<Decimal>,
    pub maker_fee_bps: Option<Decimal>,
    pub taker_fee_bps: Option<Decimal>,
}

impl SymbolPrecision {
    /// Build from a `SymbolInfo` as returned by `exchangeInfo`.
    #[must_use]
    pub fn from_symbol_info(info: &SymbolInfo) -> Self {
        let maker_fee_bps = info.maker_fee_bps.first().and_then(|s| s.parse().ok());
        let taker_fee_bps = info.taker_fee_bps.first().and_then(|s| s.parse().ok());
        Self {
            market_id: info.market_id,
            symbol: info.symbol.clone(),
            price_precision: info.price_precision,
            quantity_precision: info.quantity_precision,
            tick_size: info.tick_size(),
            step_size: info.step_size(),
            maker_fee_bps,
            taker_fee_bps,
        }
    }
}
