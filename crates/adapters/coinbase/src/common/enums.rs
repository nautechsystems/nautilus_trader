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

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Coinbase environment type.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.coinbase",
        eq,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.coinbase")
)]
pub enum CoinbaseEnvironment {
    /// Production environment.
    #[default]
    Live,
    /// Sandbox/testing environment.
    Sandbox,
}

impl CoinbaseEnvironment {
    /// Returns true if this is the sandbox environment.
    #[must_use]
    pub const fn is_sandbox(self) -> bool {
        matches!(self, Self::Sandbox)
    }
}

/// Coinbase product type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, EnumIter,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseProductType {
    Spot,
    Future,
    #[serde(rename = "UNKNOWN_PRODUCT_TYPE")]
    #[strum(serialize = "UNKNOWN_PRODUCT_TYPE")]
    Unknown,
}

/// Coinbase order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseOrderSide {
    Buy,
    Sell,
    #[serde(rename = "UNKNOWN_ORDER_SIDE")]
    #[strum(serialize = "UNKNOWN_ORDER_SIDE")]
    Unknown,
}

/// Coinbase REST order type values.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseOrderType {
    #[serde(rename = "UNKNOWN_ORDER_TYPE")]
    #[strum(serialize = "UNKNOWN_ORDER_TYPE")]
    Unknown,
    #[serde(alias = "Market")]
    Market,
    #[serde(alias = "Limit")]
    Limit,
    #[serde(alias = "Stop")]
    Stop,
    #[serde(alias = "StopLimit", alias = "Stop Limit")]
    StopLimit,
    #[serde(alias = "Bracket")]
    Bracket,
    Twap,
    #[serde(alias = "Roll Open")]
    RollOpen,
    #[serde(alias = "Roll Close")]
    RollClose,
    Liquidation,
    Scaled,
}

/// Coinbase order status.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseOrderStatus {
    Pending,
    Open,
    Filled,
    Cancelled,
    Expired,
    Failed,
    #[serde(rename = "UNKNOWN_ORDER_STATUS")]
    #[strum(serialize = "UNKNOWN_ORDER_STATUS")]
    Unknown,
    Queued,
    CancelQueued,
    EditQueued,
}

impl CoinbaseOrderStatus {
    /// Returns true when the status represents a terminal lifecycle state
    /// (no further updates expected from the venue).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Expired | Self::Failed
        )
    }
}

/// Coinbase time in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseTimeInForce {
    #[serde(rename = "UNKNOWN_TIME_IN_FORCE")]
    #[strum(serialize = "UNKNOWN_TIME_IN_FORCE")]
    Unknown,
    GoodUntilDateTime,
    GoodUntilCancelled,
    ImmediateOrCancel,
    FillOrKill,
}

/// Coinbase trigger status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseTriggerStatus {
    #[serde(rename = "UNKNOWN_TRIGGER_STATUS")]
    #[strum(serialize = "UNKNOWN_TRIGGER_STATUS")]
    Unknown,
    InvalidOrderType,
    StopPending,
    StopTriggered,
}

/// Coinbase order placement source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseOrderPlacementSource {
    #[serde(rename = "UNKNOWN_PLACEMENT_SOURCE")]
    #[strum(serialize = "UNKNOWN_PLACEMENT_SOURCE")]
    Unknown,
    RetailSimple,
    RetailAdvanced,
}

/// Coinbase order margin type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.coinbase",
        eq,
        eq_int,
        frozen,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.coinbase")
)]
pub enum CoinbaseMarginType {
    #[serde(alias = "Cross")]
    Cross,
    #[serde(alias = "Isolated")]
    Isolated,
}

/// Coinbase product status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CoinbaseProductStatus {
    Online,
    Offline,
    Delisted,
    /// Futures products return an empty status string.
    #[serde(rename = "")]
    #[strum(serialize = "")]
    Unset,
}

/// Coinbase product venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseProductVenue {
    /// Coinbase Exchange (spot).
    Cbe,
    /// Futures Commission Merchant (futures/perpetuals).
    Fcm,
}

/// Coinbase FCM trading session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseFcmTradingSessionState {
    #[serde(rename = "FCM_TRADING_SESSION_STATE_UNDEFINED")]
    #[strum(serialize = "FCM_TRADING_SESSION_STATE_UNDEFINED")]
    Undefined,
    #[serde(rename = "FCM_TRADING_SESSION_STATE_OPEN")]
    #[strum(serialize = "FCM_TRADING_SESSION_STATE_OPEN")]
    Open,
}

/// Coinbase FCM trading session closed reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseFcmTradingSessionClosedReason {
    #[serde(rename = "FCM_TRADING_SESSION_CLOSED_REASON_UNDEFINED")]
    #[strum(serialize = "FCM_TRADING_SESSION_CLOSED_REASON_UNDEFINED")]
    Undefined,
    #[serde(rename = "FCM_TRADING_SESSION_CLOSED_REASON_EXCHANGE_MAINTENANCE")]
    #[strum(serialize = "FCM_TRADING_SESSION_CLOSED_REASON_EXCHANGE_MAINTENANCE")]
    ExchangeMaintenance,
}

/// Coinbase risk management owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseRiskManagedBy {
    #[serde(rename = "UNKNOWN_RISK_MANAGEMENT_TYPE")]
    #[strum(serialize = "UNKNOWN_RISK_MANAGEMENT_TYPE")]
    Unknown,
    #[serde(rename = "MANAGED_BY_FCM")]
    #[strum(serialize = "MANAGED_BY_FCM")]
    ManagedByFcm,
    #[serde(rename = "MANAGED_BY_VENUE")]
    #[strum(serialize = "MANAGED_BY_VENUE")]
    ManagedByVenue,
}

/// Coinbase account type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseAccountType {
    // Production currently returns the fully qualified wire names
    // (`ACCOUNT_TYPE_CRYPTO`, `ACCOUNT_TYPE_FIAT`); older documented
    // examples use the short forms. Accept both on deserialize / parse
    // but keep the short form as the canonical Display value.
    #[serde(alias = "ACCOUNT_TYPE_CRYPTO")]
    #[strum(to_string = "CRYPTO", serialize = "ACCOUNT_TYPE_CRYPTO")]
    Crypto,
    #[serde(alias = "ACCOUNT_TYPE_FIAT")]
    #[strum(to_string = "FIAT", serialize = "ACCOUNT_TYPE_FIAT")]
    Fiat,
}

/// Coinbase fill trade type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseFillTradeType {
    #[serde(rename = "FILL")]
    #[strum(serialize = "FILL")]
    Fill,
}

/// Coinbase FCM position side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseFcmPositionSide {
    #[serde(rename = "FUTURES_POSITION_SIDE_UNSPECIFIED")]
    #[strum(serialize = "FUTURES_POSITION_SIDE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "LONG")]
    #[strum(serialize = "LONG")]
    Long,
    #[serde(rename = "SHORT")]
    #[strum(serialize = "SHORT")]
    Short,
}

/// Coinbase futures margin window type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseMarginWindowType {
    #[serde(rename = "FCM_MARGIN_WINDOW_TYPE_INTRADAY")]
    #[strum(serialize = "FCM_MARGIN_WINDOW_TYPE_INTRADAY")]
    Intraday,
    #[serde(rename = "FCM_MARGIN_WINDOW_TYPE_OVERNIGHT")]
    #[strum(serialize = "FCM_MARGIN_WINDOW_TYPE_OVERNIGHT")]
    Overnight,
}

/// Coinbase margin level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseMarginLevel {
    #[serde(rename = "MARGIN_LEVEL_TYPE_BASE")]
    #[strum(serialize = "MARGIN_LEVEL_TYPE_BASE")]
    Base,
}

/// Coinbase contract expiry type for futures products.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseContractExpiryType {
    #[serde(
        rename = "UNKNOWN_CONTRACT_EXPIRY_TYPE",
        alias = "UNKNOWN_CONTRACT_EXPIRY"
    )]
    #[strum(
        serialize = "UNKNOWN_CONTRACT_EXPIRY_TYPE",
        serialize = "UNKNOWN_CONTRACT_EXPIRY"
    )]
    Unknown,
    Expiring,
    /// Non-expiring (perpetual)
    #[serde(rename = "PERPETUAL")]
    #[strum(serialize = "PERPETUAL")]
    Perpetual,
}

/// Coinbase futures asset type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseFuturesAssetType {
    #[serde(rename = "FUTURES_ASSET_TYPE_CRYPTO")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_CRYPTO")]
    Crypto,
    #[serde(rename = "FUTURES_ASSET_TYPE_ENERGY")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_ENERGY")]
    Energy,
    #[serde(rename = "FUTURES_ASSET_TYPE_METALS")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_METALS")]
    Metals,
    #[serde(rename = "FUTURES_ASSET_TYPE_STOCKS")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_STOCKS")]
    Stocks,
}

/// Coinbase fill liquidity indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseLiquidityIndicator {
    Maker,
    Taker,
    Unknown,
}

/// Coinbase stop order direction.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum CoinbaseStopDirection {
    #[serde(rename = "STOP_DIRECTION_STOP_DOWN")]
    #[strum(serialize = "STOP_DIRECTION_STOP_DOWN")]
    StopDown,
    #[serde(rename = "STOP_DIRECTION_STOP_UP")]
    #[strum(serialize = "STOP_DIRECTION_STOP_UP")]
    StopUp,
}

/// Coinbase candle granularity for historical data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseGranularity {
    OneMinute,
    FiveMinute,
    FifteenMinute,
    ThirtyMinute,
    OneHour,
    TwoHour,
    SixHour,
    OneDay,
}

/// Coinbase WebSocket channel type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CoinbaseWsChannel {
    Level2,
    MarketTrades,
    Ticker,
    TickerBatch,
    Candles,
    User,
    Heartbeats,
    FuturesBalanceSummary,
    Status,
}

impl CoinbaseWsChannel {
    /// Returns true if this channel requires authentication.
    #[must_use]
    pub fn requires_auth(&self) -> bool {
        matches!(self, Self::User | Self::FuturesBalanceSummary)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(CoinbaseProductType::Spot, "SPOT")]
    #[case(CoinbaseProductType::Future, "FUTURE")]
    fn test_product_type_display(#[case] variant: CoinbaseProductType, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
    }

    #[rstest]
    #[case("BUY", CoinbaseOrderSide::Buy)]
    #[case("SELL", CoinbaseOrderSide::Sell)]
    fn test_order_side_from_str(#[case] input: &str, #[case] expected: CoinbaseOrderSide) {
        assert_eq!(CoinbaseOrderSide::from_str(input).unwrap(), expected);
    }

    #[rstest]
    #[case(CoinbaseOrderStatus::Filled, true)]
    #[case(CoinbaseOrderStatus::Cancelled, true)]
    #[case(CoinbaseOrderStatus::Expired, true)]
    #[case(CoinbaseOrderStatus::Failed, true)]
    #[case(CoinbaseOrderStatus::Open, false)]
    #[case(CoinbaseOrderStatus::Pending, false)]
    #[case(CoinbaseOrderStatus::Queued, false)]
    #[case(CoinbaseOrderStatus::CancelQueued, false)]
    #[case(CoinbaseOrderStatus::EditQueued, false)]
    #[case(CoinbaseOrderStatus::Unknown, false)]
    fn test_order_status_is_terminal(#[case] status: CoinbaseOrderStatus, #[case] expected: bool) {
        assert_eq!(status.is_terminal(), expected);
    }

    #[rstest]
    fn test_ws_channel_requires_auth() {
        assert!(CoinbaseWsChannel::User.requires_auth());
        assert!(CoinbaseWsChannel::FuturesBalanceSummary.requires_auth());
        assert!(!CoinbaseWsChannel::Level2.requires_auth());
        assert!(!CoinbaseWsChannel::MarketTrades.requires_auth());
        assert!(!CoinbaseWsChannel::Ticker.requires_auth());
    }

    #[rstest]
    fn test_order_status_serialization() {
        let status = CoinbaseOrderStatus::Open;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"OPEN\"");

        let deserialized: CoinbaseOrderStatus = serde_json::from_str("\"CANCELLED\"").unwrap();
        assert_eq!(deserialized, CoinbaseOrderStatus::Cancelled);
    }

    #[rstest]
    fn test_screaming_snake_case_multi_word() {
        let json = serde_json::to_string(&CoinbaseOrderType::StopLimit).unwrap();
        assert_eq!(json, "\"STOP_LIMIT\"");

        let json = serde_json::to_string(&CoinbaseOrderStatus::CancelQueued).unwrap();
        assert_eq!(json, "\"CANCEL_QUEUED\"");

        let json = serde_json::to_string(&CoinbaseTimeInForce::GoodUntilDateTime).unwrap();
        assert_eq!(json, "\"GOOD_UNTIL_DATE_TIME\"");

        let json = serde_json::to_string(&CoinbaseGranularity::FifteenMinute).unwrap();
        assert_eq!(json, "\"FIFTEEN_MINUTE\"");
    }

    #[rstest]
    fn test_order_type_accepts_title_case_aliases() {
        let order_type: CoinbaseOrderType = serde_json::from_str("\"Limit\"").unwrap();
        assert_eq!(order_type, CoinbaseOrderType::Limit);

        let order_type: CoinbaseOrderType = serde_json::from_str("\"StopLimit\"").unwrap();
        assert_eq!(order_type, CoinbaseOrderType::StopLimit);

        let order_type: CoinbaseOrderType = serde_json::from_str("\"Stop Limit\"").unwrap();
        assert_eq!(order_type, CoinbaseOrderType::StopLimit);

        let order_type = CoinbaseOrderType::from_str("STOP_LIMIT").unwrap();
        assert_eq!(order_type, CoinbaseOrderType::StopLimit);

        let order_type = CoinbaseOrderType::from_str("TWAP").unwrap();
        assert_eq!(order_type, CoinbaseOrderType::Twap);

        let order_type = CoinbaseOrderType::from_str("LIQUIDATION").unwrap();
        assert_eq!(order_type, CoinbaseOrderType::Liquidation);
    }

    #[rstest]
    fn test_account_type_accepts_current_wire_values() {
        let account_type: CoinbaseAccountType = serde_json::from_str("\"FIAT\"").unwrap();
        assert_eq!(account_type, CoinbaseAccountType::Fiat);

        let account_type = CoinbaseAccountType::from_str("CRYPTO").unwrap();
        assert_eq!(account_type, CoinbaseAccountType::Crypto);
    }

    // Production `/accounts` currently returns the fully qualified wire
    // names (`ACCOUNT_TYPE_CRYPTO` / `ACCOUNT_TYPE_FIAT`). Both shapes
    // must parse so account-state bootstrap doesn't fail with
    // "unknown variant".
    #[rstest]
    fn test_account_type_accepts_qualified_wire_values() {
        let account_type: CoinbaseAccountType =
            serde_json::from_str("\"ACCOUNT_TYPE_CRYPTO\"").unwrap();
        assert_eq!(account_type, CoinbaseAccountType::Crypto);

        let account_type: CoinbaseAccountType =
            serde_json::from_str("\"ACCOUNT_TYPE_FIAT\"").unwrap();
        assert_eq!(account_type, CoinbaseAccountType::Fiat);

        let account_type = CoinbaseAccountType::from_str("ACCOUNT_TYPE_CRYPTO").unwrap();
        assert_eq!(account_type, CoinbaseAccountType::Crypto);
    }

    // Display must keep emitting the short form regardless of input
    // wire shape; the qualified form is an input-only alias.
    #[rstest]
    fn test_account_type_display_uses_short_form() {
        assert_eq!(CoinbaseAccountType::Crypto.to_string(), "CRYPTO");
        assert_eq!(CoinbaseAccountType::Fiat.to_string(), "FIAT");
        assert_eq!(
            serde_json::to_string(&CoinbaseAccountType::Crypto).unwrap(),
            "\"CRYPTO\""
        );
    }

    #[rstest]
    fn test_margin_type_accepts_request_and_ws_spellings() {
        let margin_type: CoinbaseMarginType = serde_json::from_str("\"CROSS\"").unwrap();
        assert_eq!(margin_type, CoinbaseMarginType::Cross);

        let margin_type: CoinbaseMarginType = serde_json::from_str("\"Cross\"").unwrap();
        assert_eq!(margin_type, CoinbaseMarginType::Cross);
    }

    #[rstest]
    fn test_contract_expiry_type_accepts_websocket_alias() {
        let expiry_type: CoinbaseContractExpiryType =
            serde_json::from_str("\"UNKNOWN_CONTRACT_EXPIRY\"").unwrap();
        assert_eq!(expiry_type, CoinbaseContractExpiryType::Unknown);
    }

    #[rstest]
    fn test_ws_channel_snake_case() {
        let json = serde_json::to_string(&CoinbaseWsChannel::Level2).unwrap();
        assert_eq!(json, "\"level2\"");

        let json = serde_json::to_string(&CoinbaseWsChannel::MarketTrades).unwrap();
        assert_eq!(json, "\"market_trades\"");

        let json = serde_json::to_string(&CoinbaseWsChannel::FuturesBalanceSummary).unwrap();
        assert_eq!(json, "\"futures_balance_summary\"");
    }

    #[rstest]
    fn test_unknown_variants_have_qualified_names() {
        let json = serde_json::to_string(&CoinbaseProductType::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_PRODUCT_TYPE\"");

        let json = serde_json::to_string(&CoinbaseOrderSide::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_ORDER_SIDE\"");

        let json = serde_json::to_string(&CoinbaseOrderType::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_ORDER_TYPE\"");

        let json = serde_json::to_string(&CoinbaseOrderStatus::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_ORDER_STATUS\"");

        let json = serde_json::to_string(&CoinbaseTimeInForce::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_TIME_IN_FORCE\"");

        let json = serde_json::to_string(&CoinbaseTriggerStatus::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_TRIGGER_STATUS\"");

        let json = serde_json::to_string(&CoinbaseOrderPlacementSource::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_PLACEMENT_SOURCE\"");

        let json = serde_json::to_string(&CoinbaseContractExpiryType::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_CONTRACT_EXPIRY_TYPE\"");

        let json = serde_json::to_string(&CoinbaseRiskManagedBy::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_RISK_MANAGEMENT_TYPE\"");
    }
}
