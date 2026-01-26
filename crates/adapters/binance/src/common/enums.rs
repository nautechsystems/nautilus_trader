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

//! Binance enumeration types for product types and environments.

use std::fmt::Display;

use nautilus_model::enums::{OrderSide, OrderType, TimeInForce};
use serde::{Deserialize, Serialize};

/// Binance product type identifier.
///
/// Each product type corresponds to a different Binance API domain and
/// has distinct trading rules and instrument specifications.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", eq)
)]
pub enum BinanceProductType {
    /// Spot trading (api.binance.com).
    #[default]
    Spot,
    /// Spot Margin trading (uses Spot API with margin endpoints).
    Margin,
    /// USD-M Futures - linear perpetuals and delivery futures (fapi.binance.com).
    UsdM,
    /// COIN-M Futures - inverse perpetuals and delivery futures (dapi.binance.com).
    CoinM,
    /// European Options (eapi.binance.com).
    Options,
}

impl BinanceProductType {
    /// Returns the string representation used in API requests.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "SPOT",
            Self::Margin => "MARGIN",
            Self::UsdM => "USD_M",
            Self::CoinM => "COIN_M",
            Self::Options => "OPTIONS",
        }
    }

    /// Returns the instrument ID suffix for this product type.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Spot => "-SPOT",
            Self::Margin => "-MARGIN",
            Self::UsdM => "-LINEAR",
            Self::CoinM => "-INVERSE",
            Self::Options => "-OPTION",
        }
    }

    /// Returns true if this is a spot product (Spot or Margin).
    #[must_use]
    pub const fn is_spot(self) -> bool {
        matches!(self, Self::Spot | Self::Margin)
    }

    /// Returns true if this is a futures product (USD-M or COIN-M).
    #[must_use]
    pub const fn is_futures(self) -> bool {
        matches!(self, Self::UsdM | Self::CoinM)
    }

    /// Returns true if this is a linear product (Spot, Margin, or USD-M).
    #[must_use]
    pub const fn is_linear(self) -> bool {
        matches!(self, Self::Spot | Self::Margin | Self::UsdM)
    }

    /// Returns true if this is an inverse product (COIN-M).
    #[must_use]
    pub const fn is_inverse(self) -> bool {
        matches!(self, Self::CoinM)
    }

    /// Returns true if this is an options product.
    #[must_use]
    pub const fn is_options(self) -> bool {
        matches!(self, Self::Options)
    }
}

impl Display for BinanceProductType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Binance environment type.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", eq)
)]
pub enum BinanceEnvironment {
    /// Production/mainnet environment.
    #[default]
    Mainnet,
    /// Testnet environment.
    Testnet,
}

impl BinanceEnvironment {
    /// Returns true if this is the testnet environment.
    #[must_use]
    pub const fn is_testnet(self) -> bool {
        matches!(self, Self::Testnet)
    }
}

/// Order side for Binance orders and trades.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BinanceSide {
    /// Buy side.
    Buy,
    /// Sell side.
    Sell,
}

impl TryFrom<OrderSide> for BinanceSide {
    type Error = anyhow::Error;

    fn try_from(value: OrderSide) -> Result<Self, Self::Error> {
        match value {
            OrderSide::Buy => Ok(Self::Buy),
            OrderSide::Sell => Ok(Self::Sell),
            _ => anyhow::bail!("Unsupported `OrderSide` for Binance: {value:?}"),
        }
    }
}

impl From<BinanceSide> for OrderSide {
    fn from(value: BinanceSide) -> Self {
        match value {
            BinanceSide::Buy => Self::Buy,
            BinanceSide::Sell => Self::Sell,
        }
    }
}

/// Position side for dual-side position mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", eq)
)]
pub enum BinancePositionSide {
    /// Single position mode (both).
    Both,
    /// Long position.
    Long,
    /// Short position.
    Short,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Margin type applied to a position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinanceMarginType {
    /// Cross margin.
    Cross,
    /// Isolated margin.
    Isolated,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Working type for trigger price evaluation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceWorkingType {
    /// Use the contract price.
    ContractPrice,
    /// Use the mark price.
    MarkPrice,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Order status lifecycle values.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceOrderStatus {
    /// Order accepted and working.
    New,
    /// Partially filled.
    PartiallyFilled,
    /// Fully filled.
    Filled,
    /// Canceled by user or system.
    Canceled,
    /// Pending cancel (not commonly used).
    PendingCancel,
    /// Rejected by exchange.
    Rejected,
    /// Expired.
    Expired,
    /// Expired in match (IOC/FOK not executed).
    ExpiredInMatch,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Futures order type enumeration.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceFuturesOrderType {
    /// Limit order.
    Limit,
    /// Market order.
    Market,
    /// Stop (stop-limit) order.
    Stop,
    /// Stop market order.
    StopMarket,
    /// Take profit (limit) order.
    TakeProfit,
    /// Take profit market order.
    TakeProfitMarket,
    /// Trailing stop market order.
    TrailingStopMarket,
    /// Liquidation order created by exchange.
    Liquidation,
    /// Auto-deleveraging order created by exchange.
    Adl,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

impl From<BinanceFuturesOrderType> for OrderType {
    fn from(value: BinanceFuturesOrderType) -> Self {
        match value {
            BinanceFuturesOrderType::Limit => Self::Limit,
            BinanceFuturesOrderType::Market => Self::Market,
            BinanceFuturesOrderType::Stop => Self::StopLimit,
            BinanceFuturesOrderType::StopMarket => Self::StopMarket,
            BinanceFuturesOrderType::TakeProfit => Self::LimitIfTouched,
            BinanceFuturesOrderType::TakeProfitMarket => Self::MarketIfTouched,
            BinanceFuturesOrderType::TrailingStopMarket => Self::TrailingStopMarket,
            BinanceFuturesOrderType::Liquidation
            | BinanceFuturesOrderType::Adl
            | BinanceFuturesOrderType::Unknown => Self::Market, // Exchange-generated orders
        }
    }
}

/// Time in force options.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BinanceTimeInForce {
    /// Good till canceled.
    Gtc,
    /// Immediate or cancel.
    Ioc,
    /// Fill or kill.
    Fok,
    /// Good till crossing (post-only).
    Gtx,
    /// Good till date.
    Gtd,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

impl TryFrom<TimeInForce> for BinanceTimeInForce {
    type Error = anyhow::Error;

    fn try_from(value: TimeInForce) -> Result<Self, Self::Error> {
        match value {
            TimeInForce::Gtc => Ok(Self::Gtc),
            TimeInForce::Ioc => Ok(Self::Ioc),
            TimeInForce::Fok => Ok(Self::Fok),
            TimeInForce::Gtd => Ok(Self::Gtd),
            _ => anyhow::bail!("Unsupported `TimeInForce` for Binance: {value:?}"),
        }
    }
}

/// Income type for account income history.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceIncomeType {
    /// Internal transfers.
    Transfer,
    /// Welcome bonus.
    WelcomeBonus,
    /// Realized profit and loss.
    RealizedPnl,
    /// Funding fee payments/receipts.
    FundingFee,
    /// Trading commission.
    Commission,
    /// Insurance clear.
    InsuranceClear,
    /// Referral kickback.
    ReferralKickback,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Price match mode for futures maker orders.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinancePriceMatch {
    /// Match opposing side (default).
    Opponent,
    /// Match opposing side with 5 tick offset.
    Opponent5,
    /// Match opposing side with 10 tick offset.
    Opponent10,
    /// Match opposing side with 20 tick offset.
    Opponent20,
    /// Join current queue on same side.
    Queue,
    /// Join queue with 5 tick offset.
    Queue5,
    /// Join queue with 10 tick offset.
    Queue10,
    /// Join queue with 20 tick offset.
    Queue20,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Self-trade prevention mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceSelfTradePreventionMode {
    /// Expire maker orders on self-trade.
    ExpireMaker,
    /// Expire taker orders on self-trade.
    ExpireTaker,
    /// Expire both sides on self-trade.
    ExpireBoth,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Trading status for symbols.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceTradingStatus {
    /// Trading is active.
    Trading,
    /// Pending activation.
    PendingTrading,
    /// Pre-trading session.
    PreTrading,
    /// Post-trading session.
    PostTrading,
    /// End of day.
    EndOfDay,
    /// Trading halted.
    Halt,
    /// Auction match.
    AuctionMatch,
    /// Break period.
    Break,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// Contract status for coin-margined futures.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceContractStatus {
    /// Trading is active.
    Trading,
    /// Pending trading.
    PendingTrading,
    /// Pre-delivering.
    PreDelivering,
    /// Delivering.
    Delivering,
    /// Delivered.
    Delivered,
    /// Pre-delist.
    PreDelisting,
    /// Delisting in progress.
    Delisting,
    /// Contract down.
    Down,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

/// WebSocket stream event types.
///
/// These are the "e" field values in WebSocket JSON messages.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BinanceWsEventType {
    /// Aggregate trade event.
    AggTrade,
    /// Individual trade event.
    Trade,
    /// Book ticker (best bid/ask) event.
    BookTicker,
    /// Depth update (order book delta) event.
    DepthUpdate,
    /// Mark price update event.
    MarkPriceUpdate,
    /// Kline/candlestick event.
    Kline,
    /// Forced liquidation order event.
    ForceOrder,
    /// 24-hour rolling ticker event.
    #[serde(rename = "24hrTicker")]
    Ticker24Hr,
    /// 24-hour rolling mini ticker event.
    #[serde(rename = "24hrMiniTicker")]
    MiniTicker24Hr,

    // User data stream events
    /// Account update (balance and position changes).
    #[serde(rename = "ACCOUNT_UPDATE")]
    AccountUpdate,
    /// Order/trade update event.
    #[serde(rename = "ORDER_TRADE_UPDATE")]
    OrderTradeUpdate,
    /// Margin call warning event.
    #[serde(rename = "MARGIN_CALL")]
    MarginCall,
    /// Account configuration update (leverage change).
    #[serde(rename = "ACCOUNT_CONFIG_UPDATE")]
    AccountConfigUpdate,
    /// Listen key expired event.
    #[serde(rename = "listenKeyExpired")]
    ListenKeyExpired,

    /// Unknown or undocumented event type.
    #[serde(other)]
    Unknown,
}

impl BinanceWsEventType {
    /// Returns the wire format string for this event type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AggTrade => "aggTrade",
            Self::Trade => "trade",
            Self::BookTicker => "bookTicker",
            Self::DepthUpdate => "depthUpdate",
            Self::MarkPriceUpdate => "markPriceUpdate",
            Self::Kline => "kline",
            Self::ForceOrder => "forceOrder",
            Self::Ticker24Hr => "24hrTicker",
            Self::MiniTicker24Hr => "24hrMiniTicker",
            Self::AccountUpdate => "ACCOUNT_UPDATE",
            Self::OrderTradeUpdate => "ORDER_TRADE_UPDATE",
            Self::MarginCall => "MARGIN_CALL",
            Self::AccountConfigUpdate => "ACCOUNT_CONFIG_UPDATE",
            Self::ListenKeyExpired => "listenKeyExpired",
            Self::Unknown => "unknown",
        }
    }
}

impl Display for BinanceWsEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// WebSocket request method (operation type).
///
/// Used for subscription requests on both Spot and Futures WebSocket APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BinanceWsMethod {
    /// Subscribe to streams.
    Subscribe,
    /// Unsubscribe from streams.
    Unsubscribe,
}

/// Filter type identifiers returned in exchange info.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceFilterType {
    /// Price filter.
    PriceFilter,
    /// Percent price filter.
    PercentPrice,
    /// Lot size filter.
    LotSize,
    /// Market lot size filter.
    MarketLotSize,
    /// Notional filter (spot).
    Notional,
    /// Min notional filter (futures).
    MinNotional,
    /// Maximum number of orders filter.
    MaxNumOrders,
    /// Maximum number of algo orders filter.
    MaxNumAlgoOrders,
    /// Unknown or undocumented value.
    #[serde(other)]
    Unknown,
}

impl Display for BinanceEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mainnet => write!(f, "Mainnet"),
            Self::Testnet => write!(f, "Testnet"),
        }
    }
}

/// Rate limit type for API request quotas.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinanceRateLimitType {
    RequestWeight,
    Orders,
}

/// Rate limit time interval.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinanceRateLimitInterval {
    Second,
    Minute,
    Day,
}

/// Kline (candlestick) interval.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BinanceKlineInterval {
    /// 1 second (only for spot).
    #[serde(rename = "1s")]
    Second1,
    /// 1 minute.
    #[default]
    #[serde(rename = "1m")]
    Minute1,
    /// 3 minutes.
    #[serde(rename = "3m")]
    Minute3,
    /// 5 minutes.
    #[serde(rename = "5m")]
    Minute5,
    /// 15 minutes.
    #[serde(rename = "15m")]
    Minute15,
    /// 30 minutes.
    #[serde(rename = "30m")]
    Minute30,
    /// 1 hour.
    #[serde(rename = "1h")]
    Hour1,
    /// 2 hours.
    #[serde(rename = "2h")]
    Hour2,
    /// 4 hours.
    #[serde(rename = "4h")]
    Hour4,
    /// 6 hours.
    #[serde(rename = "6h")]
    Hour6,
    /// 8 hours.
    #[serde(rename = "8h")]
    Hour8,
    /// 12 hours.
    #[serde(rename = "12h")]
    Hour12,
    /// 1 day.
    #[serde(rename = "1d")]
    Day1,
    /// 3 days.
    #[serde(rename = "3d")]
    Day3,
    /// 1 week.
    #[serde(rename = "1w")]
    Week1,
    /// 1 month.
    #[serde(rename = "1M")]
    Month1,
}

impl BinanceKlineInterval {
    /// Returns the string representation used by Binance API.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Second1 => "1s",
            Self::Minute1 => "1m",
            Self::Minute3 => "3m",
            Self::Minute5 => "5m",
            Self::Minute15 => "15m",
            Self::Minute30 => "30m",
            Self::Hour1 => "1h",
            Self::Hour2 => "2h",
            Self::Hour4 => "4h",
            Self::Hour6 => "6h",
            Self::Hour8 => "8h",
            Self::Hour12 => "12h",
            Self::Day1 => "1d",
            Self::Day3 => "3d",
            Self::Week1 => "1w",
            Self::Month1 => "1M",
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_product_type_as_str() {
        assert_eq!(BinanceProductType::Spot.as_str(), "SPOT");
        assert_eq!(BinanceProductType::Margin.as_str(), "MARGIN");
        assert_eq!(BinanceProductType::UsdM.as_str(), "USD_M");
        assert_eq!(BinanceProductType::CoinM.as_str(), "COIN_M");
        assert_eq!(BinanceProductType::Options.as_str(), "OPTIONS");
    }

    #[rstest]
    fn test_product_type_suffix() {
        assert_eq!(BinanceProductType::Spot.suffix(), "-SPOT");
        assert_eq!(BinanceProductType::Margin.suffix(), "-MARGIN");
        assert_eq!(BinanceProductType::UsdM.suffix(), "-LINEAR");
        assert_eq!(BinanceProductType::CoinM.suffix(), "-INVERSE");
        assert_eq!(BinanceProductType::Options.suffix(), "-OPTION");
    }

    #[rstest]
    fn test_product_type_predicates() {
        assert!(BinanceProductType::Spot.is_spot());
        assert!(BinanceProductType::Margin.is_spot());
        assert!(!BinanceProductType::UsdM.is_spot());

        assert!(BinanceProductType::UsdM.is_futures());
        assert!(BinanceProductType::CoinM.is_futures());
        assert!(!BinanceProductType::Spot.is_futures());

        assert!(BinanceProductType::CoinM.is_inverse());
        assert!(!BinanceProductType::UsdM.is_inverse());

        assert!(BinanceProductType::Options.is_options());
        assert!(!BinanceProductType::Spot.is_options());
    }
}
