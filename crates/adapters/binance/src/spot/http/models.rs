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

//! Binance Spot HTTP response models.
//!
//! These models represent Binance venue-specific response types decoded from SBE.

use nautilus_core::{UUID4, nanos::UnixNanos};
use nautilus_model::{
    enums::AccountType,
    events::AccountState,
    identifiers::AccountId,
    types::{AccountBalance, Currency, Money},
};
use rust_decimal::Decimal;

use crate::common::sbe::spot::{
    order_side::OrderSide, order_status::OrderStatus, order_type::OrderType,
    self_trade_prevention_mode::SelfTradePreventionMode, time_in_force::TimeInForce,
};

/// Price/quantity level in an order book.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BinancePriceLevel {
    /// Price mantissa (multiply by 10^exponent to get actual price).
    pub price_mantissa: i64,
    /// Quantity mantissa (multiply by 10^exponent to get actual quantity).
    pub qty_mantissa: i64,
}

/// Binance order book depth response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceDepth {
    /// Last update ID for this depth snapshot.
    pub last_update_id: i64,
    /// Price exponent for all price levels.
    pub price_exponent: i8,
    /// Quantity exponent for all quantity values.
    pub qty_exponent: i8,
    /// Bid price levels (best bid first).
    pub bids: Vec<BinancePriceLevel>,
    /// Ask price levels (best ask first).
    pub asks: Vec<BinancePriceLevel>,
}

/// A single trade from Binance.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceTrade {
    /// Trade ID.
    pub id: i64,
    /// Price mantissa.
    pub price_mantissa: i64,
    /// Quantity mantissa.
    pub qty_mantissa: i64,
    /// Quote quantity mantissa (price * qty).
    pub quote_qty_mantissa: i64,
    /// Trade timestamp in microseconds (SBE precision).
    pub time: i64,
    /// Whether the buyer is the maker.
    pub is_buyer_maker: bool,
    /// Whether this trade is the best price match.
    pub is_best_match: bool,
}

/// Binance trades response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceTrades {
    /// Price exponent for all trades.
    pub price_exponent: i8,
    /// Quantity exponent for all trades.
    pub qty_exponent: i8,
    /// List of trades.
    pub trades: Vec<BinanceTrade>,
}

/// A fill from an order execution.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceOrderFill {
    /// Fill price mantissa.
    pub price_mantissa: i64,
    /// Fill quantity mantissa.
    pub qty_mantissa: i64,
    /// Commission mantissa.
    pub commission_mantissa: i64,
    /// Commission exponent.
    pub commission_exponent: i8,
    /// Commission asset.
    pub commission_asset: String,
    /// Trade ID (if available).
    pub trade_id: Option<i64>,
}

/// New order response (FULL response type).
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceNewOrderResponse {
    /// Price exponent for this response.
    pub price_exponent: i8,
    /// Quantity exponent for this response.
    pub qty_exponent: i8,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    pub order_list_id: Option<i64>,
    /// Transaction time in microseconds.
    pub transact_time: i64,
    /// Order price mantissa.
    pub price_mantissa: i64,
    /// Original order quantity mantissa.
    pub orig_qty_mantissa: i64,
    /// Executed quantity mantissa.
    pub executed_qty_mantissa: i64,
    /// Cumulative quote quantity mantissa.
    pub cummulative_quote_qty_mantissa: i64,
    /// Order status.
    pub status: OrderStatus,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Order type.
    pub order_type: OrderType,
    /// Order side.
    pub side: OrderSide,
    /// Stop price mantissa (for stop orders).
    pub stop_price_mantissa: Option<i64>,
    /// Working time in microseconds.
    pub working_time: Option<i64>,
    /// Self-trade prevention mode.
    pub self_trade_prevention_mode: SelfTradePreventionMode,
    /// Client order ID.
    pub client_order_id: String,
    /// Symbol.
    pub symbol: String,
    /// Order fills.
    pub fills: Vec<BinanceOrderFill>,
}

/// Cancel order response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceCancelOrderResponse {
    /// Price exponent for this response.
    pub price_exponent: i8,
    /// Quantity exponent for this response.
    pub qty_exponent: i8,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    pub order_list_id: Option<i64>,
    /// Transaction time in microseconds.
    pub transact_time: i64,
    /// Order price mantissa.
    pub price_mantissa: i64,
    /// Original order quantity mantissa.
    pub orig_qty_mantissa: i64,
    /// Executed quantity mantissa.
    pub executed_qty_mantissa: i64,
    /// Cumulative quote quantity mantissa.
    pub cummulative_quote_qty_mantissa: i64,
    /// Order status.
    pub status: OrderStatus,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Order type.
    pub order_type: OrderType,
    /// Order side.
    pub side: OrderSide,
    /// Self-trade prevention mode.
    pub self_trade_prevention_mode: SelfTradePreventionMode,
    /// Client order ID.
    pub client_order_id: String,
    /// Original client order ID.
    pub orig_client_order_id: String,
    /// Symbol.
    pub symbol: String,
}

/// Query order response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceOrderResponse {
    /// Price exponent for this response.
    pub price_exponent: i8,
    /// Quantity exponent for this response.
    pub qty_exponent: i8,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    pub order_list_id: Option<i64>,
    /// Order price mantissa.
    pub price_mantissa: i64,
    /// Original order quantity mantissa.
    pub orig_qty_mantissa: i64,
    /// Executed quantity mantissa.
    pub executed_qty_mantissa: i64,
    /// Cumulative quote quantity mantissa.
    pub cummulative_quote_qty_mantissa: i64,
    /// Order status.
    pub status: OrderStatus,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Order type.
    pub order_type: OrderType,
    /// Order side.
    pub side: OrderSide,
    /// Stop price mantissa (for stop orders).
    pub stop_price_mantissa: Option<i64>,
    /// Iceberg quantity mantissa.
    pub iceberg_qty_mantissa: Option<i64>,
    /// Order creation time in microseconds.
    pub time: i64,
    /// Last update time in microseconds.
    pub update_time: i64,
    /// Whether the order is working.
    pub is_working: bool,
    /// Working time in microseconds.
    pub working_time: Option<i64>,
    /// Original quote order quantity mantissa.
    pub orig_quote_order_qty_mantissa: i64,
    /// Self-trade prevention mode.
    pub self_trade_prevention_mode: SelfTradePreventionMode,
    /// Client order ID.
    pub client_order_id: String,
    /// Symbol.
    pub symbol: String,
}

/// Account balance for a single asset.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceBalance {
    /// Asset symbol.
    pub asset: String,
    /// Free (available) balance mantissa.
    pub free_mantissa: i64,
    /// Locked balance mantissa.
    pub locked_mantissa: i64,
    /// Balance exponent.
    pub exponent: i8,
}

/// Account information response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceAccountInfo {
    /// Commission exponent.
    pub commission_exponent: i8,
    /// Maker commission rate mantissa.
    pub maker_commission_mantissa: i64,
    /// Taker commission rate mantissa.
    pub taker_commission_mantissa: i64,
    /// Buyer commission rate mantissa.
    pub buyer_commission_mantissa: i64,
    /// Seller commission rate mantissa.
    pub seller_commission_mantissa: i64,
    /// Whether trading is enabled.
    pub can_trade: bool,
    /// Whether withdrawals are enabled.
    pub can_withdraw: bool,
    /// Whether deposits are enabled.
    pub can_deposit: bool,
    /// Whether the account requires self-trade prevention.
    pub require_self_trade_prevention: bool,
    /// Whether to prevent self-trade by quote order ID.
    pub prevent_sor: bool,
    /// Account update time in microseconds.
    pub update_time: i64,
    /// Account type.
    pub account_type: String,
    /// Account balances.
    pub balances: Vec<BinanceBalance>,
}

impl BinanceAccountInfo {
    /// Converts this Binance account info to a Nautilus [`AccountState`].
    #[must_use]
    pub fn to_account_state(&self, account_id: AccountId, ts_init: UnixNanos) -> AccountState {
        let mut balances = Vec::with_capacity(self.balances.len());

        for asset in &self.balances {
            let currency =
                Currency::get_or_create_crypto_with_context(&asset.asset, Some("spot balance"));

            let exponent = asset.exponent as i32;
            let multiplier = Decimal::new(1, (-exponent) as u32);

            let free = Decimal::new(asset.free_mantissa, 0) * multiplier;
            let locked = Decimal::new(asset.locked_mantissa, 0) * multiplier;
            let total = free + locked;

            let total_money = Money::from_decimal(total, currency)
                .unwrap_or_else(|_| Money::new(total.to_string().parse().unwrap_or(0.0), currency));
            let locked_money = Money::from_decimal(locked, currency).unwrap_or_else(|_| {
                Money::new(locked.to_string().parse().unwrap_or(0.0), currency)
            });
            let free_money = Money::from_decimal(free, currency)
                .unwrap_or_else(|_| Money::new(free.to_string().parse().unwrap_or(0.0), currency));

            let balance = AccountBalance::new(total_money, locked_money, free_money);
            balances.push(balance);
        }

        // Ensure at least one balance exists
        if balances.is_empty() {
            let zero_currency = Currency::USDT();
            let zero_money = Money::new(0.0, zero_currency);
            let zero_balance = AccountBalance::new(zero_money, zero_money, zero_money);
            balances.push(zero_balance);
        }

        let ts_event = UnixNanos::from((self.update_time * 1_000) as u64);

        AccountState::new(
            account_id,
            AccountType::Cash,
            balances,
            vec![], // No margins for spot
            true,   // is_reported
            UUID4::new(),
            ts_event,
            ts_init,
            None, // No base currency for spot
        )
    }
}

/// Price filter from SBE response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinancePriceFilterSbe {
    /// Price exponent for mantissa conversion.
    pub price_exponent: i8,
    /// Minimum price mantissa.
    pub min_price: i64,
    /// Maximum price mantissa.
    pub max_price: i64,
    /// Tick size mantissa.
    pub tick_size: i64,
}

/// Lot size filter from SBE response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceLotSizeFilterSbe {
    /// Quantity exponent for mantissa conversion.
    pub qty_exponent: i8,
    /// Minimum quantity mantissa.
    pub min_qty: i64,
    /// Maximum quantity mantissa.
    pub max_qty: i64,
    /// Step size mantissa.
    pub step_size: i64,
}

/// Symbol filters from SBE response.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BinanceSymbolFiltersSbe {
    /// Price filter (required for trading).
    pub price_filter: Option<BinancePriceFilterSbe>,
    /// Lot size filter (required for trading).
    pub lot_size_filter: Option<BinanceLotSizeFilterSbe>,
}

/// Symbol information from SBE exchange info response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceSymbolSbe {
    /// Symbol name (e.g., "BTCUSDT").
    pub symbol: String,
    /// Base asset (e.g., "BTC").
    pub base_asset: String,
    /// Quote asset (e.g., "USDT").
    pub quote_asset: String,
    /// Base asset precision.
    pub base_asset_precision: u8,
    /// Quote asset precision.
    pub quote_asset_precision: u8,
    /// Symbol status.
    pub status: u8,
    /// Order types bitset.
    pub order_types: u16,
    /// Whether iceberg orders are allowed.
    pub iceberg_allowed: bool,
    /// Whether OCO orders are allowed.
    pub oco_allowed: bool,
    /// Whether OTO orders are allowed.
    pub oto_allowed: bool,
    /// Whether quote order quantity market orders are allowed.
    pub quote_order_qty_market_allowed: bool,
    /// Whether trailing stop is allowed.
    pub allow_trailing_stop: bool,
    /// Whether cancel-replace is allowed.
    pub cancel_replace_allowed: bool,
    /// Whether amend is allowed.
    pub amend_allowed: bool,
    /// Whether spot trading is allowed.
    pub is_spot_trading_allowed: bool,
    /// Whether margin trading is allowed.
    pub is_margin_trading_allowed: bool,
    /// Symbol filters decoded from SBE.
    pub filters: BinanceSymbolFiltersSbe,
    /// Permission sets.
    pub permissions: Vec<Vec<String>>,
}

/// Exchange information from SBE response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceExchangeInfoSbe {
    /// List of symbols.
    pub symbols: Vec<BinanceSymbolSbe>,
}

/// Account trade history entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceAccountTrade {
    /// Price exponent.
    pub price_exponent: i8,
    /// Quantity exponent.
    pub qty_exponent: i8,
    /// Commission exponent.
    pub commission_exponent: i8,
    /// Trade ID.
    pub id: i64,
    /// Order ID.
    pub order_id: i64,
    /// Order list ID (for OCO).
    pub order_list_id: Option<i64>,
    /// Trade price mantissa.
    pub price_mantissa: i64,
    /// Trade quantity mantissa.
    pub qty_mantissa: i64,
    /// Quote quantity mantissa.
    pub quote_qty_mantissa: i64,
    /// Commission mantissa.
    pub commission_mantissa: i64,
    /// Trade time in microseconds.
    pub time: i64,
    /// Whether the trade was as buyer.
    pub is_buyer: bool,
    /// Whether the trade was as maker.
    pub is_maker: bool,
    /// Whether this is the best price match.
    pub is_best_match: bool,
    /// Symbol.
    pub symbol: String,
    /// Commission asset.
    pub commission_asset: String,
}

/// Kline (candlestick) data response.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceKlines {
    /// Price exponent for all klines.
    pub price_exponent: i8,
    /// Quantity exponent for all klines.
    pub qty_exponent: i8,
    /// List of klines.
    pub klines: Vec<BinanceKline>,
}

/// Listen key response for user data stream.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyResponse {
    /// The listen key for WebSocket user data stream.
    pub listen_key: String,
}

/// 24-hour ticker statistics response.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticker24hr {
    /// Trading pair symbol.
    pub symbol: String,
    /// Price change in the last 24 hours.
    pub price_change: String,
    /// Price change percentage in the last 24 hours.
    pub price_change_percent: String,
    /// Weighted average price.
    pub weighted_avg_price: String,
    /// Previous close price.
    pub prev_close_price: String,
    /// Last price.
    pub last_price: String,
    /// Last quantity.
    pub last_qty: String,
    /// Best bid price.
    pub bid_price: String,
    /// Best bid quantity.
    pub bid_qty: String,
    /// Best ask price.
    pub ask_price: String,
    /// Best ask quantity.
    pub ask_qty: String,
    /// Open price.
    pub open_price: String,
    /// High price.
    pub high_price: String,
    /// Low price.
    pub low_price: String,
    /// Total traded base asset volume.
    pub volume: String,
    /// Total traded quote asset volume.
    pub quote_volume: String,
    /// Statistics open time in milliseconds.
    pub open_time: i64,
    /// Statistics close time in milliseconds.
    pub close_time: i64,
    /// First trade ID.
    pub first_id: i64,
    /// Last trade ID.
    pub last_id: i64,
    /// Number of trades.
    pub count: i64,
}

/// Symbol price ticker response.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TickerPrice {
    /// Trading pair symbol.
    pub symbol: String,
    /// Latest price.
    pub price: String,
}

/// Book ticker response (best bid/ask).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookTicker {
    /// Trading pair symbol.
    pub symbol: String,
    /// Best bid price.
    pub bid_price: String,
    /// Best bid quantity.
    pub bid_qty: String,
    /// Best ask price.
    pub ask_price: String,
    /// Best ask quantity.
    pub ask_qty: String,
}

/// Average price response.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct AvgPrice {
    /// Average price interval in minutes.
    pub mins: i64,
    /// Average price.
    pub price: String,
    /// Close time in milliseconds.
    #[serde(rename = "closeTime")]
    pub close_time: i64,
}

/// Trade fee information for a symbol.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeFee {
    /// Trading pair symbol.
    pub symbol: String,
    /// Maker commission rate.
    pub maker_commission: String,
    /// Taker commission rate.
    pub taker_commission: String,
}

/// Result of a single order in a batch operation.
///
/// Each item in a batch response can be either a success or an error.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum BatchOrderResult {
    /// Successful order placement.
    Success(Box<BatchOrderSuccess>),
    /// Failed order placement.
    Error(BatchOrderError),
}

/// Successful order in a batch response.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOrderSuccess {
    /// Trading pair symbol.
    pub symbol: String,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    #[serde(default)]
    pub order_list_id: Option<i64>,
    /// Client order ID.
    pub client_order_id: String,
    /// Transaction time in milliseconds.
    pub transact_time: i64,
    /// Order price.
    pub price: String,
    /// Original order quantity.
    pub orig_qty: String,
    /// Executed quantity.
    pub executed_qty: String,
    /// Cumulative quote quantity.
    #[serde(rename = "cummulativeQuoteQty")]
    pub cummulative_quote_qty: String,
    /// Order status.
    pub status: String,
    /// Time in force.
    pub time_in_force: String,
    /// Order type.
    #[serde(rename = "type")]
    pub order_type: String,
    /// Order side.
    pub side: String,
    /// Working time in milliseconds.
    #[serde(default)]
    pub working_time: Option<i64>,
    /// Self-trade prevention mode.
    #[serde(default)]
    pub self_trade_prevention_mode: Option<String>,
}

/// Error in a batch order response.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct BatchOrderError {
    /// Error code from Binance.
    pub code: i64,
    /// Error message.
    pub msg: String,
}

/// Result of a single cancel in a batch cancel operation.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum BatchCancelResult {
    /// Successful order cancellation.
    Success(Box<BatchCancelSuccess>),
    /// Failed order cancellation.
    Error(BatchOrderError),
}

/// Successful cancel in a batch response.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchCancelSuccess {
    /// Trading pair symbol.
    pub symbol: String,
    /// Original client order ID.
    pub orig_client_order_id: String,
    /// Exchange order ID.
    pub order_id: i64,
    /// Order list ID (for OCO orders).
    #[serde(default)]
    pub order_list_id: Option<i64>,
    /// Client order ID.
    pub client_order_id: String,
    /// Transaction time in milliseconds.
    #[serde(default)]
    pub transact_time: Option<i64>,
    /// Order price.
    pub price: String,
    /// Original order quantity.
    pub orig_qty: String,
    /// Executed quantity.
    pub executed_qty: String,
    /// Cumulative quote quantity.
    #[serde(rename = "cummulativeQuoteQty")]
    pub cummulative_quote_qty: String,
    /// Order status.
    pub status: String,
    /// Time in force.
    pub time_in_force: String,
    /// Order type.
    #[serde(rename = "type")]
    pub order_type: String,
    /// Order side.
    pub side: String,
    /// Self-trade prevention mode.
    #[serde(default)]
    pub self_trade_prevention_mode: Option<String>,
}

/// A single kline (candlestick) from Binance.
#[derive(Debug, Clone, PartialEq)]
pub struct BinanceKline {
    /// Kline open time in milliseconds.
    pub open_time: i64,
    /// Open price mantissa.
    pub open_price: i64,
    /// High price mantissa.
    pub high_price: i64,
    /// Low price mantissa.
    pub low_price: i64,
    /// Close price mantissa.
    pub close_price: i64,
    /// Volume (base asset) as 128-bit bytes.
    pub volume: [u8; 16],
    /// Kline close time in milliseconds.
    pub close_time: i64,
    /// Quote volume as 128-bit bytes.
    pub quote_volume: [u8; 16],
    /// Number of trades.
    pub num_trades: i64,
    /// Taker buy base volume as 128-bit bytes.
    pub taker_buy_base_volume: [u8; 16],
    /// Taker buy quote volume as 128-bit bytes.
    pub taker_buy_quote_volume: [u8; 16],
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_listen_key_response_deserialize() {
        let json = r#"{"listenKey": "abc123xyz"}"#;
        let response: ListenKeyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.listen_key, "abc123xyz");
    }

    #[rstest]
    fn test_ticker_price_deserialize() {
        let json = r#"{"symbol": "BTCUSDT", "price": "50000.00"}"#;
        let response: TickerPrice = serde_json::from_str(json).unwrap();
        assert_eq!(response.symbol, "BTCUSDT");
        assert_eq!(response.price, "50000.00");
    }

    #[rstest]
    fn test_book_ticker_deserialize() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "bidPrice": "49999.00",
            "bidQty": "1.5",
            "askPrice": "50001.00",
            "askQty": "2.0"
        }"#;
        let response: BookTicker = serde_json::from_str(json).unwrap();
        assert_eq!(response.symbol, "BTCUSDT");
        assert_eq!(response.bid_price, "49999.00");
        assert_eq!(response.ask_price, "50001.00");
    }

    #[rstest]
    fn test_avg_price_deserialize() {
        let json = r#"{"mins": 5, "price": "50000.00", "closeTime": 1734300000000}"#;
        let response: AvgPrice = serde_json::from_str(json).unwrap();
        assert_eq!(response.mins, 5);
        assert_eq!(response.price, "50000.00");
        assert_eq!(response.close_time, 1734300000000);
    }

    #[rstest]
    fn test_trade_fee_deserialize() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "makerCommission": "0.001",
            "takerCommission": "0.001"
        }"#;
        let response: TradeFee = serde_json::from_str(json).unwrap();
        assert_eq!(response.symbol, "BTCUSDT");
        assert_eq!(response.maker_commission, "0.001");
        assert_eq!(response.taker_commission, "0.001");
    }

    #[rstest]
    fn test_batch_order_result_success() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "orderId": 12345,
            "orderListId": -1,
            "clientOrderId": "my-order-1",
            "transactTime": 1734300000000,
            "price": "50000.00",
            "origQty": "0.1",
            "executedQty": "0.0",
            "cummulativeQuoteQty": "0.0",
            "status": "NEW",
            "timeInForce": "GTC",
            "type": "LIMIT",
            "side": "BUY"
        }"#;
        let result: BatchOrderResult = serde_json::from_str(json).unwrap();
        match result {
            BatchOrderResult::Success(order) => {
                assert_eq!(order.symbol, "BTCUSDT");
                assert_eq!(order.order_id, 12345);
            }
            BatchOrderResult::Error(_) => panic!("Expected Success"),
        }
    }

    #[rstest]
    fn test_batch_order_result_error() {
        let json = r#"{"code": -1013, "msg": "Invalid quantity."}"#;
        let result: BatchOrderResult = serde_json::from_str(json).unwrap();
        match result {
            BatchOrderResult::Success(_) => panic!("Expected Error"),
            BatchOrderResult::Error(error) => {
                assert_eq!(error.code, -1013);
                assert_eq!(error.msg, "Invalid quantity.");
            }
        }
    }

    #[rstest]
    fn test_batch_cancel_result_success() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "orderId": 12345,
            "orderListId": -1,
            "origClientOrderId": "my-order-1",
            "clientOrderId": "cancel-1",
            "transactTime": 1734300000000,
            "price": "50000.00",
            "origQty": "0.1",
            "executedQty": "0.0",
            "cummulativeQuoteQty": "0.0",
            "status": "CANCELED",
            "timeInForce": "GTC",
            "type": "LIMIT",
            "side": "BUY"
        }"#;
        let result: BatchCancelResult = serde_json::from_str(json).unwrap();
        match result {
            BatchCancelResult::Success(cancel) => {
                assert_eq!(cancel.symbol, "BTCUSDT");
                assert_eq!(cancel.order_id, 12345);
            }
            BatchCancelResult::Error(_) => panic!("Expected Success"),
        }
    }

    #[rstest]
    fn test_batch_cancel_result_error() {
        let json = r#"{"code": -2011, "msg": "Unknown order sent."}"#;
        let result: BatchCancelResult = serde_json::from_str(json).unwrap();
        match result {
            BatchCancelResult::Success(_) => panic!("Expected Error"),
            BatchCancelResult::Error(error) => {
                assert_eq!(error.code, -2011);
                assert_eq!(error.msg, "Unknown order sent.");
            }
        }
    }

    #[rstest]
    fn test_ticker_24hr_deserialize() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "priceChange": "100.00",
            "priceChangePercent": "0.2",
            "weightedAvgPrice": "50050.00",
            "prevClosePrice": "49950.00",
            "lastPrice": "50050.00",
            "lastQty": "0.01",
            "bidPrice": "50049.00",
            "bidQty": "1.0",
            "askPrice": "50051.00",
            "askQty": "1.0",
            "openPrice": "49950.00",
            "highPrice": "50200.00",
            "lowPrice": "49800.00",
            "volume": "1000.0",
            "quoteVolume": "50000000.0",
            "openTime": 1734200000000,
            "closeTime": 1734300000000,
            "firstId": 1000,
            "lastId": 2000,
            "count": 1000
        }"#;
        let response: Ticker24hr = serde_json::from_str(json).unwrap();
        assert_eq!(response.symbol, "BTCUSDT");
        assert_eq!(response.last_price, "50050.00");
        assert_eq!(response.count, 1000);
    }
}
