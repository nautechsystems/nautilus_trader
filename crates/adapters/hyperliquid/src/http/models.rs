// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use alloy_primitives::Address;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{
    HyperliquidSide, HyperliquidTpSl, HyperliquidTrailingOffsetType, HyperliquidTriggerPriceType,
};

/// Represents metadata about available markets from `POST /info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidMeta {
    #[serde(default)]
    pub universe: Vec<HyperliquidAssetInfo>,
}

/// Represents a single candle (OHLCV bar) from Hyperliquid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidCandle {
    /// Candle open timestamp in milliseconds.
    #[serde(rename = "t")]
    pub timestamp: u64,
    /// Open price.
    #[serde(rename = "o")]
    pub open: String,
    /// High price.
    #[serde(rename = "h")]
    pub high: String,
    /// Low price.
    #[serde(rename = "l")]
    pub low: String,
    /// Close price.
    #[serde(rename = "c")]
    pub close: String,
    /// Volume.
    #[serde(rename = "v")]
    pub volume: String,
    /// Number of trades (optional).
    #[serde(rename = "n", default)]
    pub num_trades: Option<u64>,
}

/// Response from candleSnapshot endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidCandleSnapshot {
    /// Array of candles.
    #[serde(default)]
    pub data: Vec<HyperliquidCandle>,
}

/// Represents asset information from the meta endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidAssetInfo {
    /// Asset name (e.g., "BTC").
    pub name: Ustr,
    /// Number of decimal places for size.
    pub sz_decimals: u32,
    /// Maximum leverage allowed for this asset.
    #[serde(default)]
    pub max_leverage: Option<u32>,
    /// Whether this asset requires isolated margin only.
    #[serde(default)]
    pub only_isolated: Option<bool>,
    /// Whether this asset is delisted/inactive.
    #[serde(default)]
    pub is_delisted: Option<bool>,
}

// -------------------------------------------------------------------------------------------------
// === Extended Instrument Metadata Models ===
// -------------------------------------------------------------------------------------------------

/// Complete perpetuals metadata response from `POST /info` with `{ "type": "meta" }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpMeta {
    /// Perpetual assets universe.
    pub universe: Vec<PerpAsset>,
    /// Margin tables for leverage tiers.
    #[serde(default)]
    pub margin_tables: Vec<(u32, MarginTable)>,
}

/// A single perpetual asset from the universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpAsset {
    /// Asset name (e.g., "BTC").
    pub name: String,
    /// Number of decimal places for size.
    pub sz_decimals: u32,
    /// Maximum leverage allowed for this asset.
    #[serde(default)]
    pub max_leverage: Option<u32>,
    /// Whether this asset requires isolated margin only.
    #[serde(default)]
    pub only_isolated: Option<bool>,
    /// Whether this asset is delisted/inactive.
    #[serde(default)]
    pub is_delisted: Option<bool>,
}

/// Margin table with leverage tiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginTable {
    /// Description of the margin table.
    pub description: String,
    /// Margin tiers for different position sizes.
    #[serde(default)]
    pub margin_tiers: Vec<MarginTier>,
}

/// Individual margin tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginTier {
    /// Lower bound for this tier (as string to preserve precision).
    pub lower_bound: String,
    /// Maximum leverage for this tier.
    pub max_leverage: u32,
}

/// Complete spot metadata response from `POST /info` with `{ "type": "spotMeta" }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotMeta {
    /// Spot tokens available.
    pub tokens: Vec<SpotToken>,
    /// Spot pairs universe.
    pub universe: Vec<SpotPair>,
}

/// EVM contract information for a spot token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EvmContract {
    /// EVM contract address (20 bytes).
    pub address: Address,
    /// Extra wei decimals for EVM precision (can be negative).
    pub evm_extra_wei_decimals: i32,
}

/// A single spot token from the tokens list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotToken {
    /// Token name (e.g., "USDC").
    pub name: String,
    /// Number of decimal places for size.
    pub sz_decimals: u32,
    /// Wei decimals (on-chain precision).
    pub wei_decimals: u32,
    /// Token index used for pair references.
    pub index: u32,
    /// Token contract ID/address.
    pub token_id: String,
    /// Whether this is the canonical token.
    pub is_canonical: bool,
    /// Optional EVM contract information.
    #[serde(default)]
    pub evm_contract: Option<EvmContract>,
    /// Optional full name.
    #[serde(default)]
    pub full_name: Option<String>,
    /// Optional deployer trading fee share.
    #[serde(default)]
    pub deployer_trading_fee_share: Option<String>,
}

/// A single spot pair from the universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotPair {
    /// Pair display name (e.g., "PURR/USDC").
    pub name: String,
    /// Token indices [base_token_index, quote_token_index].
    pub tokens: [u32; 2],
    /// Pair index.
    pub index: u32,
    /// Whether this is the canonical pair.
    pub is_canonical: bool,
}

// -------------------------------------------------------------------------------------------------
// === Optional Context Payloads (for price precision refinement) ===
// -------------------------------------------------------------------------------------------------

/// Optional perpetuals metadata with asset contexts from `{ "type": "metaAndAssetCtxs" }`.
/// Returns a tuple: `[PerpMeta, Vec<PerpAssetCtx>]`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PerpMetaAndCtxs {
    /// Tuple format: [meta, contexts]
    Payload(Box<(PerpMeta, Vec<PerpAssetCtx>)>),
}

/// Runtime context for a perpetual asset (mark prices, funding, etc).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpAssetCtx {
    /// Mark price as string.
    #[serde(default)]
    pub mark_px: Option<String>,
    /// Mid price as string.
    #[serde(default)]
    pub mid_px: Option<String>,
    /// Funding rate as string.
    #[serde(default)]
    pub funding: Option<String>,
    /// Open interest as string.
    #[serde(default)]
    pub open_interest: Option<String>,
}

/// Optional spot metadata with asset contexts from `{ "type": "spotMetaAndAssetCtxs" }`.
/// Returns a tuple: `[SpotMeta, Vec<SpotAssetCtx>]`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpotMetaAndCtxs {
    /// Tuple format: [meta, contexts]
    Payload(Box<(SpotMeta, Vec<SpotAssetCtx>)>),
}

/// Runtime context for a spot pair (prices, volumes, etc).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotAssetCtx {
    /// Mark price as string.
    #[serde(default)]
    pub mark_px: Option<String>,
    /// Mid price as string.
    #[serde(default)]
    pub mid_px: Option<String>,
    /// 24h volume as string.
    #[serde(default)]
    pub day_volume: Option<String>,
}

/// Represents an L2 order book snapshot from `POST /info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidL2Book {
    /// Coin symbol.
    pub coin: Ustr,
    /// Order book levels: [bids, asks].
    pub levels: Vec<Vec<HyperliquidLevel>>,
    /// Timestamp in milliseconds.
    pub time: u64,
}

/// Represents an order book level with price and size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidLevel {
    /// Price level.
    pub px: String,
    /// Size at this level.
    pub sz: String,
}

/// Represents user fills response from `POST /info`.
///
/// The Hyperliquid API returns fills directly as an array, not wrapped in an object.
pub type HyperliquidFills = Vec<HyperliquidFill>;

/// Represents an individual fill from user fills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidFill {
    /// Coin symbol.
    pub coin: Ustr,
    /// Fill price.
    pub px: String,
    /// Fill size.
    pub sz: String,
    /// Order side (buy/sell).
    pub side: HyperliquidSide,
    /// Fill timestamp in milliseconds.
    pub time: u64,
    /// Position size before this fill.
    #[serde(rename = "startPosition")]
    pub start_position: String,
    /// Directory (order book path).
    pub dir: String,
    /// Closed P&L from this fill.
    #[serde(rename = "closedPnl")]
    pub closed_pnl: String,
    /// Hash reference.
    pub hash: String,
    /// Order ID that generated this fill.
    pub oid: u64,
    /// Crossed status.
    pub crossed: bool,
    /// Fee paid for this fill.
    pub fee: String,
}

/// Represents order status response from `POST /info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOrderStatus {
    #[serde(default)]
    pub statuses: Vec<HyperliquidOrderStatusEntry>,
}

/// Represents an individual order status entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOrderStatusEntry {
    /// Order information.
    pub order: HyperliquidOrderInfo,
    /// Current status string.
    pub status: String,
    /// Status timestamp in milliseconds.
    #[serde(rename = "statusTimestamp")]
    pub status_timestamp: u64,
}

/// Represents order information within an order status entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOrderInfo {
    /// Coin symbol.
    pub coin: Ustr,
    /// Order side (buy/sell).
    pub side: HyperliquidSide,
    /// Limit price.
    #[serde(rename = "limitPx")]
    pub limit_px: String,
    /// Order size.
    pub sz: String,
    /// Order ID.
    pub oid: u64,
    /// Order timestamp in milliseconds.
    pub timestamp: u64,
    /// Original order size.
    #[serde(rename = "origSz")]
    pub orig_sz: String,
}

/// ECC signature components for Hyperliquid exchange requests.
#[derive(Debug, Clone, Serialize)]
pub struct HyperliquidSignature {
    /// R component of the signature.
    pub r: String,
    /// S component of the signature.
    pub s: String,
    /// V component (recovery ID) of the signature.
    pub v: u64,
}

impl HyperliquidSignature {
    /// Parse a hex signature string (0x + 64 hex r + 64 hex s + 2 hex v) into components.
    pub fn from_hex(sig_hex: &str) -> Result<Self, String> {
        let sig_hex = sig_hex.strip_prefix("0x").unwrap_or(sig_hex);

        if sig_hex.len() != 130 {
            return Err(format!(
                "Invalid signature length: expected 130 hex chars, got {}",
                sig_hex.len()
            ));
        }

        let r = format!("0x{}", &sig_hex[0..64]);
        let s = format!("0x{}", &sig_hex[64..128]);
        let v = u64::from_str_radix(&sig_hex[128..130], 16)
            .map_err(|e| format!("Failed to parse v component: {}", e))?;

        Ok(Self { r, s, v })
    }
}

/// Represents an exchange action request wrapper for `POST /exchange`.
#[derive(Debug, Clone, Serialize)]
pub struct HyperliquidExchangeRequest<T> {
    /// The action to perform.
    #[serde(rename = "action")]
    pub action: T,
    /// Request nonce for replay protection.
    #[serde(rename = "nonce")]
    pub nonce: u64,
    /// ECC signature over the action.
    #[serde(rename = "signature")]
    pub signature: HyperliquidSignature,
    /// Optional vault address for sub-account trading.
    #[serde(rename = "vaultAddress", skip_serializing_if = "Option::is_none")]
    pub vault_address: Option<String>,
    /// Optional expiration time in milliseconds.
    #[serde(rename = "expiresAfter", skip_serializing_if = "Option::is_none")]
    pub expires_after: Option<u64>,
}

impl<T> HyperliquidExchangeRequest<T>
where
    T: Serialize,
{
    /// Create a new exchange request with the given action.
    pub fn new(action: T, nonce: u64, signature: String) -> Result<Self, String> {
        Ok(Self {
            action,
            nonce,
            signature: HyperliquidSignature::from_hex(&signature)?,
            vault_address: None,
            expires_after: None,
        })
    }

    /// Create a new exchange request with vault address for sub-account trading.
    pub fn with_vault(
        action: T,
        nonce: u64,
        signature: String,
        vault_address: String,
    ) -> Result<Self, String> {
        Ok(Self {
            action,
            nonce,
            signature: HyperliquidSignature::from_hex(&signature)?,
            vault_address: Some(vault_address),
            expires_after: None,
        })
    }

    /// Convert to JSON value for signing purposes.
    pub fn to_sign_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

/// Represents an exchange response wrapper from `POST /exchange`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HyperliquidExchangeResponse {
    /// Successful response with status.
    Status {
        /// Status message.
        status: String,
        /// Response payload.
        response: serde_json::Value,
    },
    /// Error response.
    Error {
        /// Error message.
        error: String,
    },
}

////////////////////////////////////////////////////////////////////////////////
// Conditional Order Models
////////////////////////////////////////////////////////////////////////////////

/// Extended trigger order parameters for advanced conditional orders.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidTriggerOrderParams {
    /// Whether this is a market order when triggered (true) or limit order (false).
    #[serde(rename = "isMarket")]
    pub is_market: bool,
    /// Trigger price.
    #[serde(rename = "triggerPx")]
    pub trigger_px: String,
    /// Take profit or stop loss type.
    pub tpsl: HyperliquidTpSl,
    /// Optional trigger price type (last, mark, oracle). Defaults to mark price if not specified.
    #[serde(rename = "triggerPxType", skip_serializing_if = "Option::is_none")]
    pub trigger_px_type: Option<HyperliquidTriggerPriceType>,
}

/// Trailing stop order parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidTrailingStopParams {
    /// Trailing offset value.
    #[serde(
        rename = "trailingOffset",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub trailing_offset: Decimal,
    /// Trailing offset type (price, percentage, basis_points).
    #[serde(rename = "trailingOffsetType")]
    pub trailing_offset_type: HyperliquidTrailingOffsetType,
    /// Optional activation price - price at which the trailing stop becomes active.
    #[serde(rename = "activationPx", skip_serializing_if = "Option::is_none")]
    pub activation_px: Option<String>,
    /// Take profit or stop loss type.
    pub tpsl: HyperliquidTpSl,
}

/// Request to place a trigger order (stop or take profit).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidPlaceTriggerOrderRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Whether to buy or sell.
    #[serde(rename = "b")]
    pub is_buy: bool,
    /// Order size.
    #[serde(
        rename = "s",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub sz: Decimal,
    /// Limit price (required if is_market is false).
    #[serde(rename = "limitPx", skip_serializing_if = "Option::is_none")]
    pub limit_px: Option<String>,
    /// Trigger order parameters.
    #[serde(flatten)]
    pub trigger_params: HyperliquidTriggerOrderParams,
    /// Whether this is a reduce-only order.
    #[serde(rename = "reduceOnly", skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// Optional client order ID for tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<Cloid>,
}

/// Request to modify an existing trigger order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidModifyTriggerOrderRequest {
    /// Order ID to modify.
    pub oid: OrderId,
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// New trigger price.
    #[serde(rename = "triggerPx")]
    pub trigger_px: String,
    /// New limit price (if applicable).
    #[serde(rename = "limitPx", skip_serializing_if = "Option::is_none")]
    pub limit_px: Option<String>,
    /// New order size (if changing).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::common::parse::serialize_optional_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_optional_decimal_from_str"
    )]
    pub sz: Option<Decimal>,
}

/// Request to cancel a trigger order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidCancelTriggerOrderRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Order ID to cancel.
    pub oid: OrderId,
}

/// Trigger order status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidTriggerOrderStatus {
    /// Order ID.
    pub oid: OrderId,
    /// Order status string.
    pub status: String,
    /// Timestamp when status was updated (milliseconds).
    #[serde(rename = "statusTimestamp")]
    pub status_timestamp: u64,
    /// Trigger order information.
    pub order: HyperliquidTriggerOrderInfo,
}

/// Information about a trigger order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidTriggerOrderInfo {
    /// Asset symbol.
    pub coin: Ustr,
    /// Order side.
    pub side: HyperliquidSide,
    /// Limit price (if limit order).
    #[serde(rename = "limitPx", skip_serializing_if = "Option::is_none")]
    pub limit_px: Option<String>,
    /// Trigger price.
    #[serde(rename = "triggerPx")]
    pub trigger_px: String,
    /// Order size.
    pub sz: String,
    /// Whether this is a market order when triggered.
    #[serde(rename = "isMarket")]
    pub is_market: bool,
    /// Take profit or stop loss type.
    pub tpsl: HyperliquidTpSl,
    /// Order ID.
    pub oid: OrderId,
    /// Order creation timestamp (milliseconds).
    pub timestamp: u64,
    /// Whether the order has been triggered.
    #[serde(default)]
    pub triggered: bool,
    /// Trigger timestamp (milliseconds, if triggered).
    #[serde(rename = "triggerTime", skip_serializing_if = "Option::is_none")]
    pub trigger_time: Option<u64>,
}

/// Bracket order request (entry + TP + SL).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidBracketOrderRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Whether to buy or sell.
    #[serde(rename = "b")]
    pub is_buy: bool,
    /// Entry order size.
    #[serde(
        rename = "s",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub sz: Decimal,
    /// Entry order limit price.
    #[serde(rename = "limitPx")]
    pub limit_px: String,
    /// Take profit trigger price.
    #[serde(rename = "tpTriggerPx")]
    pub tp_trigger_px: String,
    /// Take profit limit price (if limit order).
    #[serde(rename = "tpLimitPx", skip_serializing_if = "Option::is_none")]
    pub tp_limit_px: Option<String>,
    /// Whether TP is market order.
    #[serde(rename = "tpIsMarket", default)]
    pub tp_is_market: bool,
    /// Stop loss trigger price.
    #[serde(rename = "slTriggerPx")]
    pub sl_trigger_px: String,
    /// Stop loss limit price (if limit order).
    #[serde(rename = "slLimitPx", skip_serializing_if = "Option::is_none")]
    pub sl_limit_px: Option<String>,
    /// Whether SL is market order.
    #[serde(rename = "slIsMarket", default)]
    pub sl_is_market: bool,
    /// Optional client order ID for entry order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<Cloid>,
}

/// OCO (One-Cancels-Other) order request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidOcoOrderRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Whether to buy or sell.
    #[serde(rename = "b")]
    pub is_buy: bool,
    /// Order size.
    #[serde(
        rename = "s",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub sz: Decimal,
    /// First order trigger price.
    #[serde(rename = "triggerPx1")]
    pub trigger_px_1: String,
    /// First order limit price (if applicable).
    #[serde(rename = "limitPx1", skip_serializing_if = "Option::is_none")]
    pub limit_px_1: Option<String>,
    /// Whether first order is market.
    #[serde(rename = "isMarket1", default)]
    pub is_market_1: bool,
    /// First order TP/SL type.
    #[serde(rename = "tpsl1")]
    pub tpsl_1: HyperliquidTpSl,
    /// Second order trigger price.
    #[serde(rename = "triggerPx2")]
    pub trigger_px_2: String,
    /// Second order limit price (if applicable).
    #[serde(rename = "limitPx2", skip_serializing_if = "Option::is_none")]
    pub limit_px_2: Option<String>,
    /// Whether second order is market.
    #[serde(rename = "isMarket2", default)]
    pub is_market_2: bool,
    /// Second order TP/SL type.
    #[serde(rename = "tpsl2")]
    pub tpsl_2: HyperliquidTpSl,
    /// Whether orders are reduce-only.
    #[serde(rename = "reduceOnly", skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_meta_deserialization() {
        let json = r#"{"universe": [{"name": "BTC", "szDecimals": 5}]}"#;

        let meta: HyperliquidMeta = serde_json::from_str(json).unwrap();

        assert_eq!(meta.universe.len(), 1);
        assert_eq!(meta.universe[0].name, "BTC");
        assert_eq!(meta.universe[0].sz_decimals, 5);
    }

    #[rstest]
    fn test_l2_book_deserialization() {
        let json = r#"{"coin": "BTC", "levels": [[{"px": "50000", "sz": "1.5"}], [{"px": "50100", "sz": "2.0"}]], "time": 1234567890}"#;

        let book: HyperliquidL2Book = serde_json::from_str(json).unwrap();

        assert_eq!(book.coin, "BTC");
        assert_eq!(book.levels.len(), 2);
        assert_eq!(book.time, 1234567890);
    }

    #[rstest]
    fn test_exchange_response_deserialization() {
        let json = r#"{"status": "ok", "response": {"type": "order"}}"#;

        let response: HyperliquidExchangeResponse = serde_json::from_str(json).unwrap();

        match response {
            HyperliquidExchangeResponse::Status { status, .. } => assert_eq!(status, "ok"),
            _ => panic!("Expected status response"),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Exchange execution endpoint models
////////////////////////////////////////////////////////////////////////////////

/// Custom serde module for handling 128-bit hex client order IDs.
pub mod execution_cloid {
    use std::fmt;

    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    /// A 128-bit client order ID represented as a hex string with `0x` prefix.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct Cloid(pub [u8; 16]);

    impl Cloid {
        /// Creates a new `Cloid` from a hex string.
        ///
        /// # Errors
        ///
        /// Returns an error if the string is not a valid 128-bit hex with `0x` prefix.
        pub fn from_hex<S: AsRef<str>>(s: S) -> Result<Self, String> {
            let hex_str = s.as_ref();
            let without_prefix = hex_str
                .strip_prefix("0x")
                .ok_or("CLOID must start with '0x'")?;

            if without_prefix.len() != 32 {
                return Err("CLOID must be exactly 32 hex characters (128 bits)".to_string());
            }

            let mut bytes = [0u8; 16];
            for i in 0..16 {
                let byte_str = &without_prefix[i * 2..i * 2 + 2];
                bytes[i] = u8::from_str_radix(byte_str, 16)
                    .map_err(|_| "Invalid hex character in CLOID".to_string())?;
            }

            Ok(Self(bytes))
        }

        /// Converts the CLOID to a hex string with `0x` prefix.
        pub fn to_hex(&self) -> String {
            let mut result = String::with_capacity(34);
            result.push_str("0x");
            for byte in &self.0 {
                result.push_str(&format!("{:02x}", byte));
            }
            result
        }
    }

    impl fmt::Display for Cloid {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.to_hex())
        }
    }

    impl Serialize for Cloid {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&self.to_hex())
        }
    }

    impl<'de> Deserialize<'de> for Cloid {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            Self::from_hex(&s).map_err(D::Error::custom)
        }
    }
}

pub use execution_cloid::Cloid;

/// Asset ID type for Hyperliquid.
///
/// For perpetuals, this is the index in `meta.universe`.
/// For spot trading, this is `10000 + index` from `spotMeta.universe`.
pub type AssetId = u32;

/// Order ID assigned by Hyperliquid.
pub type OrderId = u64;

/// Time-in-force for limit orders in exchange endpoint.
///
/// These values must match exactly what Hyperliquid expects for proper serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperliquidExecTif {
    /// Add Liquidity Only (post-only order).
    #[serde(rename = "Alo")]
    Alo,
    /// Immediate or Cancel.
    #[serde(rename = "Ioc")]
    Ioc,
    /// Good Till Canceled.
    #[serde(rename = "Gtc")]
    Gtc,
}

/// Take profit or stop loss side for trigger orders in exchange endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperliquidExecTpSl {
    /// Take profit.
    #[serde(rename = "tp")]
    Tp,
    /// Stop loss.
    #[serde(rename = "sl")]
    Sl,
}

/// Order grouping strategy for linked TP/SL orders in exchange endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HyperliquidExecGrouping {
    /// No grouping semantics.
    #[serde(rename = "na")]
    #[default]
    Na,
    /// Normal TP/SL grouping (linked orders).
    #[serde(rename = "normalTpsl")]
    NormalTpsl,
    /// Position-level TP/SL grouping.
    #[serde(rename = "positionTpsl")]
    PositionTpsl,
}

/// Order kind specification for the `t` field in exchange endpoint order requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HyperliquidExecOrderKind {
    /// Limit order with time-in-force.
    Limit {
        /// Limit order parameters.
        limit: HyperliquidExecLimitParams,
    },
    /// Trigger order (stop/take profit).
    Trigger {
        /// Trigger order parameters.
        trigger: HyperliquidExecTriggerParams,
    },
}

/// Parameters for limit orders in exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidExecLimitParams {
    /// Time-in-force for the limit order.
    pub tif: HyperliquidExecTif,
}

/// Parameters for trigger orders (stop/take profit) in exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidExecTriggerParams {
    /// Whether to use market price when triggered.
    pub is_market: bool,
    /// Trigger price as a string.
    #[serde(
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub trigger_px: Decimal,
    /// Whether this is a take profit or stop loss.
    pub tpsl: HyperliquidExecTpSl,
}

/// Optional builder fee for orders in exchange endpoint.
///
/// The builder fee is specified in tenths of a basis point.
/// For example, `f: 10` represents 1 basis point (0.01%).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HyperliquidExecBuilderFee {
    /// Builder address to receive the fee.
    #[serde(rename = "b")]
    pub address: String,
    /// Fee in tenths of a basis point.
    #[serde(rename = "f")]
    pub fee_tenths_bp: u32,
}

/// Order specification for placing orders via exchange endpoint.
///
/// This struct represents a single order in the exact format expected
/// by the Hyperliquid exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidExecPlaceOrderRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Is buy order (true for buy, false for sell).
    #[serde(rename = "b")]
    pub is_buy: bool,
    /// Price as a string with no trailing zeros.
    #[serde(
        rename = "p",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub price: Decimal,
    /// Size as a string with no trailing zeros.
    #[serde(
        rename = "s",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub size: Decimal,
    /// Reduce-only flag.
    #[serde(rename = "r")]
    pub reduce_only: bool,
    /// Order type (limit or trigger).
    #[serde(rename = "t")]
    pub kind: HyperliquidExecOrderKind,
    /// Optional client order ID (128-bit hex).
    #[serde(rename = "c", skip_serializing_if = "Option::is_none")]
    pub cloid: Option<Cloid>,
}

/// Cancel specification for canceling orders by order ID via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HyperliquidExecCancelOrderRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Order ID to cancel.
    #[serde(rename = "o")]
    pub oid: OrderId,
}

/// Cancel specification for canceling orders by client order ID via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidExecCancelByCloidRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Client order ID to cancel.
    #[serde(rename = "c")]
    pub cloid: Cloid,
}

/// Modify specification for modifying existing orders via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidExecModifyOrderRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Order ID to modify.
    #[serde(rename = "o")]
    pub oid: OrderId,
    /// New price (optional).
    #[serde(
        rename = "p",
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::common::parse::serialize_optional_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_optional_decimal_from_str"
    )]
    pub price: Option<Decimal>,
    /// New size (optional).
    #[serde(
        rename = "s",
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::common::parse::serialize_optional_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_optional_decimal_from_str"
    )]
    pub size: Option<Decimal>,
    /// New reduce-only flag (optional).
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// New order type (optional).
    #[serde(rename = "t", skip_serializing_if = "Option::is_none")]
    pub kind: Option<HyperliquidExecOrderKind>,
}

/// TWAP (Time-Weighted Average Price) order specification for exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidExecTwapRequest {
    /// Asset ID.
    #[serde(rename = "a")]
    pub asset: AssetId,
    /// Is buy order.
    #[serde(rename = "b")]
    pub is_buy: bool,
    /// Total size to execute.
    #[serde(
        rename = "s",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub size: Decimal,
    /// Duration in milliseconds.
    #[serde(rename = "m")]
    pub duration_ms: u64,
}

/// All possible exchange actions for the Hyperliquid `/exchange` endpoint.
///
/// Each variant corresponds to a specific action type that can be performed
/// through the exchange API. The serialization uses the exact action type
/// names expected by Hyperliquid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HyperliquidExecAction {
    /// Place one or more orders.
    #[serde(rename = "order")]
    Order {
        /// List of orders to place.
        orders: Vec<HyperliquidExecPlaceOrderRequest>,
        /// Grouping strategy for TP/SL orders.
        #[serde(default)]
        grouping: HyperliquidExecGrouping,
        /// Optional builder fee.
        #[serde(skip_serializing_if = "Option::is_none")]
        builder: Option<HyperliquidExecBuilderFee>,
    },

    /// Cancel orders by order ID.
    #[serde(rename = "cancel")]
    Cancel {
        /// Orders to cancel.
        cancels: Vec<HyperliquidExecCancelOrderRequest>,
    },

    /// Cancel orders by client order ID.
    #[serde(rename = "cancelByCloid")]
    CancelByCloid {
        /// Orders to cancel by CLOID.
        cancels: Vec<HyperliquidExecCancelByCloidRequest>,
    },

    /// Modify a single order.
    #[serde(rename = "modify")]
    Modify {
        /// Order modification specification.
        #[serde(flatten)]
        modify: HyperliquidExecModifyOrderRequest,
    },

    /// Modify multiple orders atomically.
    #[serde(rename = "batchModify")]
    BatchModify {
        /// Multiple order modifications.
        modifies: Vec<HyperliquidExecModifyOrderRequest>,
    },

    /// Schedule automatic order cancellation (dead man's switch).
    #[serde(rename = "scheduleCancel")]
    ScheduleCancel {
        /// Time in milliseconds when orders should be cancelled.
        /// If None, clears the existing schedule.
        #[serde(skip_serializing_if = "Option::is_none")]
        time: Option<u64>,
    },

    /// Update leverage for a position.
    #[serde(rename = "updateLeverage")]
    UpdateLeverage {
        /// Asset ID.
        #[serde(rename = "a")]
        asset: AssetId,
        /// Whether to use cross margin.
        #[serde(rename = "isCross")]
        is_cross: bool,
        /// Leverage value.
        #[serde(rename = "leverage")]
        leverage: u32,
    },

    /// Update isolated margin for a position.
    #[serde(rename = "updateIsolatedMargin")]
    UpdateIsolatedMargin {
        /// Asset ID.
        #[serde(rename = "a")]
        asset: AssetId,
        /// Margin delta as a string.
        #[serde(
            rename = "delta",
            serialize_with = "crate::common::parse::serialize_decimal_as_str",
            deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
        )]
        delta: Decimal,
    },

    /// Transfer USD between spot and perp accounts.
    #[serde(rename = "usdClassTransfer")]
    UsdClassTransfer {
        /// Source account type.
        from: String,
        /// Destination account type.
        to: String,
        /// Amount to transfer.
        #[serde(
            serialize_with = "crate::common::parse::serialize_decimal_as_str",
            deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
        )]
        amount: Decimal,
    },

    /// Place a TWAP order.
    #[serde(rename = "twapPlace")]
    TwapPlace {
        /// TWAP order specification.
        #[serde(flatten)]
        twap: HyperliquidExecTwapRequest,
    },

    /// Cancel a TWAP order.
    #[serde(rename = "twapCancel")]
    TwapCancel {
        /// Asset ID.
        #[serde(rename = "a")]
        asset: AssetId,
        /// TWAP ID.
        #[serde(rename = "t")]
        twap_id: u64,
    },

    /// No-operation to invalidate pending nonces.
    #[serde(rename = "noop")]
    Noop,
}

/// Exchange request envelope for the `/exchange` endpoint.
///
/// This is the top-level structure sent to Hyperliquid's exchange endpoint.
/// It includes the action to perform along with authentication and metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperliquidExecRequest {
    /// The exchange action to perform.
    pub action: HyperliquidExecAction,
    /// Request nonce for replay protection (milliseconds timestamp recommended).
    pub nonce: u64,
    /// ECC signature over the action and nonce.
    pub signature: String,
    /// Optional vault address for sub-account trading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_address: Option<String>,
    /// Optional expiration time in milliseconds.
    /// Note: Using this field increases rate limit weight by 5x if the request expires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_after: Option<u64>,
}

/// Exchange response envelope from the `/exchange` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidExecResponse {
    /// Response status ("ok" for success).
    pub status: String,
    /// Response payload.
    pub response: HyperliquidExecResponseData,
}

/// Response data containing the actual response payload from exchange endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HyperliquidExecResponseData {
    /// Response for order actions.
    #[serde(rename = "order")]
    Order {
        /// Order response data.
        data: HyperliquidExecOrderResponseData,
    },
    /// Response for cancel actions.
    #[serde(rename = "cancel")]
    Cancel {
        /// Cancel response data.
        data: HyperliquidExecCancelResponseData,
    },
    /// Response for modify actions.
    #[serde(rename = "modify")]
    Modify {
        /// Modify response data.
        data: HyperliquidExecModifyResponseData,
    },
    /// Generic response for other actions.
    #[serde(rename = "default")]
    Default,
    /// Catch-all for unknown response types.
    #[serde(other)]
    Unknown,
}

/// Order response data containing status for each order from exchange endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidExecOrderResponseData {
    /// Status for each order in the request.
    pub statuses: Vec<HyperliquidExecOrderStatus>,
}

/// Cancel response data containing status for each cancellation from exchange endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidExecCancelResponseData {
    /// Status for each cancellation in the request.
    pub statuses: Vec<HyperliquidExecCancelStatus>,
}

/// Modify response data containing status for each modification from exchange endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidExecModifyResponseData {
    /// Status for each modification in the request.
    pub statuses: Vec<HyperliquidExecModifyStatus>,
}

/// Status of an individual order submission via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HyperliquidExecOrderStatus {
    /// Order is resting on the order book.
    Resting {
        /// Resting order information.
        resting: HyperliquidExecRestingInfo,
    },
    /// Order was filled immediately.
    Filled {
        /// Fill information.
        filled: HyperliquidExecFilledInfo,
    },
    /// Order submission failed.
    Error {
        /// Error message.
        error: String,
    },
}

/// Information about a resting order via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HyperliquidExecRestingInfo {
    /// Order ID assigned by Hyperliquid.
    pub oid: OrderId,
}

/// Information about a filled order via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidExecFilledInfo {
    /// Total filled size.
    #[serde(
        rename = "totalSz",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub total_sz: Decimal,
    /// Average fill price.
    #[serde(
        rename = "avgPx",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub avg_px: Decimal,
    /// Order ID.
    pub oid: OrderId,
}

/// Status of an individual order cancellation via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HyperliquidExecCancelStatus {
    /// Cancellation succeeded.
    Success(String), // Usually "success"
    /// Cancellation failed.
    Error {
        /// Error message.
        error: String,
    },
}

/// Status of an individual order modification via exchange endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HyperliquidExecModifyStatus {
    /// Modification succeeded.
    Success(String), // Usually "success"
    /// Modification failed.
    Error {
        /// Error message.
        error: String,
    },
}

/// Complete clearinghouse state response from `POST /info` with `{ "type": "clearinghouseState", "user": "address" }`.
/// This provides account positions, margin information, and balances.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearinghouseState {
    /// List of asset positions (perpetual contracts).
    #[serde(default)]
    pub asset_positions: Vec<AssetPosition>,
    /// Cross margin summary information.
    #[serde(default)]
    pub cross_margin_summary: Option<CrossMarginSummary>,
    /// Time of the state snapshot (milliseconds since epoch).
    #[serde(default)]
    pub time: Option<u64>,
}

/// A single asset position in the clearinghouse state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPosition {
    /// Position information.
    pub position: PositionData,
    /// Type of position (e.g., "oneWay").
    #[serde(rename = "type")]
    pub position_type: String,
}

/// Detailed position data for an asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionData {
    /// Asset symbol/coin (e.g., "BTC").
    pub coin: String,
    /// Cumulative funding (entry price weighted by position size changes).
    #[serde(
        rename = "cumFunding",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub cum_funding: Decimal,
    /// Entry price for the position.
    #[serde(
        rename = "entryPx",
        serialize_with = "crate::common::parse::serialize_optional_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_optional_decimal_from_str",
        default
    )]
    pub entry_px: Option<Decimal>,
    /// Leverage used for the position.
    #[serde(
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub leverage: Decimal,
    /// Liquidation price.
    #[serde(
        rename = "liquidationPx",
        serialize_with = "crate::common::parse::serialize_optional_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_optional_decimal_from_str",
        default
    )]
    pub liquidation_px: Option<Decimal>,
    /// Margin used for this position.
    #[serde(
        rename = "marginUsed",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub margin_used: Decimal,
    /// Maximum trade sizes allowed.
    #[serde(
        rename = "maxTradeSzs",
        serialize_with = "crate::common::parse::serialize_vec_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_vec_decimal_from_str"
    )]
    pub max_trade_szs: Vec<Decimal>,
    /// Position value.
    #[serde(
        rename = "positionValue",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub position_value: Decimal,
    /// Return on equity percentage.
    #[serde(
        rename = "returnOnEquity",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub return_on_equity: Decimal,
    /// Position size (positive for long, negative for short).
    #[serde(
        rename = "szi",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub szi: Decimal,
    /// Unrealized PnL.
    #[serde(
        rename = "unrealizedPnl",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub unrealized_pnl: Decimal,
}

/// Cross margin summary information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossMarginSummary {
    /// Account value in USD.
    #[serde(
        rename = "accountValue",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub account_value: Decimal,
    /// Total notional position value.
    #[serde(
        rename = "totalNtlPos",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub total_ntl_pos: Decimal,
    /// Total raw USD value (collateral).
    #[serde(
        rename = "totalRawUsd",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub total_raw_usd: Decimal,
    /// Total margin used across all positions.
    #[serde(
        rename = "totalMarginUsed",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub total_margin_used: Decimal,
    /// Withdrawable balance.
    #[serde(
        rename = "withdrawable",
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub withdrawable: Decimal,
}
