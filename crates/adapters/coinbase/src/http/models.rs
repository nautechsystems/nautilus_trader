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

//! HTTP response model types for the Coinbase Advanced Trade REST API.

use rust_decimal::Decimal;
use serde::Deserialize;
use ustr::Ustr;

use crate::common::{
    enums::{
        CoinbaseAccountType, CoinbaseContractExpiryType, CoinbaseFcmTradingSessionClosedReason,
        CoinbaseFcmTradingSessionState, CoinbaseFillTradeType, CoinbaseFuturesAssetType,
        CoinbaseLiquidityIndicator, CoinbaseMarginType, CoinbaseOrderPlacementSource,
        CoinbaseOrderSide, CoinbaseOrderStatus, CoinbaseOrderType, CoinbaseProductStatus,
        CoinbaseProductType, CoinbaseProductVenue, CoinbaseRiskManagedBy, CoinbaseTimeInForce,
        CoinbaseTriggerStatus,
    },
    parse::{
        deserialize_decimal_from_str, deserialize_decimal_or_zero, deserialize_margin_type_or_none,
        deserialize_product_type_or_unknown, deserialize_string_to_u64,
    },
};

/// Response wrapper for listing products.
#[derive(Debug, Clone, Deserialize)]
pub struct ProductsResponse {
    pub products: Vec<Product>,
    pub num_products: Option<i64>,
}

/// Coinbase product (trading pair).
#[derive(Debug, Clone, Deserialize)]
pub struct Product {
    pub product_id: Ustr,
    pub price: String,
    pub price_percentage_change_24h: String,
    pub volume_24h: String,
    pub volume_percentage_change_24h: String,
    pub base_increment: String,
    pub quote_increment: String,
    pub quote_min_size: String,
    pub quote_max_size: String,
    pub base_min_size: String,
    pub base_max_size: String,
    pub base_name: String,
    pub quote_name: String,
    pub watched: bool,
    pub is_disabled: bool,
    pub new: bool,
    pub status: CoinbaseProductStatus,
    pub cancel_only: bool,
    pub limit_only: bool,
    pub post_only: bool,
    pub trading_disabled: bool,
    pub auction_mode: bool,
    #[serde(deserialize_with = "deserialize_product_type_or_unknown")]
    pub product_type: CoinbaseProductType,
    pub quote_currency_id: Ustr,
    pub base_currency_id: Ustr,
    #[serde(default)]
    pub fcm_trading_session_details: Option<FcmTradingSessionDetails>,
    #[serde(default)]
    pub mid_market_price: String,
    #[serde(default)]
    pub alias: Ustr,
    #[serde(default)]
    pub alias_to: Vec<Ustr>,
    #[serde(default)]
    pub base_display_symbol: Ustr,
    #[serde(default)]
    pub quote_display_symbol: Ustr,
    #[serde(default)]
    pub view_only: bool,
    pub price_increment: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub product_venue: Option<CoinbaseProductVenue>,
    #[serde(default)]
    pub approximate_quote_24h_volume: String,
    #[serde(default)]
    pub future_product_details: Option<FutureProductDetails>,
}

/// FCM trading session details for futures products.
#[derive(Debug, Clone, Deserialize)]
pub struct FcmTradingSessionDetails {
    pub is_session_open: bool,
    pub open_time: String,
    pub close_time: String,
    pub session_state: CoinbaseFcmTradingSessionState,
    #[serde(default)]
    pub after_hours_order_entry_disabled: bool,
    pub closed_reason: CoinbaseFcmTradingSessionClosedReason,
    #[serde(default)]
    pub maintenance: Option<MaintenanceWindow>,
}

/// Maintenance window for FCM sessions.
#[derive(Debug, Clone, Deserialize)]
pub struct MaintenanceWindow {
    pub start_time: String,
    pub end_time: String,
}

/// Future product details.
#[derive(Debug, Clone, Deserialize)]
pub struct FutureProductDetails {
    pub venue: Ustr,
    pub contract_code: Ustr,
    pub contract_expiry: String,
    pub contract_size: String,
    pub contract_root_unit: Ustr,
    pub group_description: String,
    pub contract_expiry_timezone: String,
    pub group_short_description: String,
    pub risk_managed_by: CoinbaseRiskManagedBy,
    pub contract_expiry_type: CoinbaseContractExpiryType,
    #[serde(default)]
    pub perpetual_details: Option<PerpetualDetails>,
    pub contract_display_name: String,
    #[serde(default)]
    pub time_to_expiry_ms: String,
    #[serde(default)]
    pub non_crypto: bool,
    #[serde(default)]
    pub contract_expiry_name: String,
    #[serde(default)]
    pub twenty_four_by_seven: bool,
    #[serde(default)]
    pub open_interest: String,
    #[serde(default)]
    pub funding_rate: String,
    #[serde(default)]
    pub funding_time: Option<String>,
    #[serde(default)]
    pub funding_interval: Option<String>,
    #[serde(default)]
    pub index_price: Option<String>,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub futures_asset_type: Option<CoinbaseFuturesAssetType>,
}

/// Perpetual contract details.
#[derive(Debug, Clone, Deserialize)]
pub struct PerpetualDetails {
    #[serde(default)]
    pub open_interest: String,
    #[serde(default)]
    pub funding_rate: String,
    #[serde(default)]
    pub funding_time: Option<String>,
}

/// Response wrapper for candles.
#[derive(Debug, Clone, Deserialize)]
pub struct CandlesResponse {
    pub candles: Vec<Candle>,
}

/// OHLCV candle data.
#[derive(Debug, Clone, Deserialize)]
pub struct Candle {
    pub start: String,
    pub low: String,
    pub high: String,
    pub open: String,
    pub close: String,
    pub volume: String,
}

/// Response wrapper for ticker/market trades.
#[derive(Debug, Clone, Deserialize)]
pub struct TickerResponse {
    pub trades: Vec<Trade>,
    pub best_bid: String,
    pub best_ask: String,
}

/// A single trade execution.
#[derive(Debug, Clone, Deserialize)]
pub struct Trade {
    pub trade_id: String,
    pub product_id: Ustr,
    pub price: String,
    pub size: String,
    pub time: String,
    pub side: CoinbaseOrderSide,
    #[serde(default)]
    pub bid: String,
    #[serde(default)]
    pub ask: String,
    #[serde(default)]
    pub exchange: String,
}

/// Response wrapper for the product order book.
#[derive(Debug, Clone, Deserialize)]
pub struct ProductBookResponse {
    pub pricebook: PriceBook,
    #[serde(default)]
    pub last: String,
    #[serde(default)]
    pub mid_market: String,
    #[serde(default)]
    pub spread_bps: String,
    #[serde(default)]
    pub spread_absolute: String,
}

/// Order book price levels.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceBook {
    pub product_id: Ustr,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub time: String,
}

/// A single price level in the order book.
#[derive(Debug, Clone, Deserialize)]
pub struct BookLevel {
    pub price: String,
    pub size: String,
}

/// Response wrapper for best bid/ask.
#[derive(Debug, Clone, Deserialize)]
pub struct BestBidAskResponse {
    pub pricebooks: Vec<BestBidAsk>,
}

/// Best bid/ask for a single product.
#[derive(Debug, Clone, Deserialize)]
pub struct BestBidAsk {
    pub product_id: Ustr,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    #[serde(default)]
    pub time: String,
}

/// Response wrapper for listing accounts.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountsResponse {
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub has_next: bool,
    #[serde(default)]
    pub cursor: String,
    #[serde(default)]
    pub size: Option<i64>,
}

/// Coinbase account.
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub name: String,
    pub currency: Ustr,
    pub available_balance: Balance,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub deleted_at: Option<String>,
    #[serde(rename = "type")]
    pub account_type: CoinbaseAccountType,
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub hold: Option<Balance>,
    #[serde(default)]
    pub retail_portfolio_id: String,
}

/// Balance amount.
#[derive(Debug, Clone, Deserialize)]
pub struct Balance {
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub value: Decimal,
    pub currency: Ustr,
}

/// Response for creating an order.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrderResponse {
    pub success: bool,
    #[serde(default)]
    pub failure_reason: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub success_response: Option<OrderSuccessResponse>,
    #[serde(default)]
    pub error_response: Option<OrderErrorResponse>,
}

/// Successful order creation details.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderSuccessResponse {
    pub order_id: String,
    pub product_id: Ustr,
    pub side: CoinbaseOrderSide,
    pub client_order_id: String,
}

/// Order creation error details.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderErrorResponse {
    pub error: String,
    pub message: String,
    pub error_details: String,
    #[serde(default)]
    pub preview_failure_reason: String,
    #[serde(default)]
    pub new_order_failure_reason: String,
}

/// Response for batch cancel.
#[derive(Debug, Clone, Deserialize)]
pub struct CancelOrdersResponse {
    pub results: Vec<CancelResult>,
}

/// Response for editing an order via `/orders/edit`.
#[derive(Debug, Clone, Deserialize)]
pub struct EditOrderResponse {
    pub success: bool,
    #[serde(default)]
    pub errors: Vec<EditOrderError>,
}

/// A single edit error entry returned by `/orders/edit`.
#[derive(Debug, Clone, Deserialize)]
pub struct EditOrderError {
    #[serde(default)]
    pub edit_failure_reason: String,
    #[serde(default)]
    pub preview_failure_reason: String,
}

/// Result for a single order cancellation.
#[derive(Debug, Clone, Deserialize)]
pub struct CancelResult {
    pub success: bool,
    #[serde(default)]
    pub failure_reason: String,
    pub order_id: String,
}

/// Response wrapper for a single order lookup.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderResponse {
    pub order: Order,
}

/// Response wrapper for an orders list query.
#[derive(Debug, Clone, Deserialize)]
pub struct OrdersListResponse {
    pub orders: Vec<Order>,
    #[serde(default)]
    pub sequence: Option<String>,
    #[serde(default)]
    pub has_next: bool,
    #[serde(default)]
    pub cursor: String,
}

/// A historical or open order as returned by `/orders/historical/*`.
///
/// `order_configuration` is kept as a raw JSON value because Coinbase returns
/// a wider set of config shapes on history responses than on submit (bracket
/// orders, TWAP, trigger variants, and new shapes Coinbase may ship without
/// bumping the API version). Consumers that need typed access can try to
/// deserialize the inner value into
/// [`crate::http::query::OrderConfiguration`] and tolerate failures. Keeping
/// the wire shape permissive prevents a single unknown variant from failing
/// the entire batch response.
#[derive(Debug, Clone, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub product_id: Ustr,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub order_configuration: Option<serde_json::Value>,
    pub side: CoinbaseOrderSide,
    #[serde(default)]
    pub client_order_id: String,
    pub status: CoinbaseOrderStatus,
    #[serde(default)]
    pub time_in_force: Option<CoinbaseTimeInForce>,
    #[serde(default)]
    pub created_time: String,
    #[serde(default)]
    pub completion_percentage: String,
    #[serde(default)]
    pub filled_size: String,
    #[serde(default)]
    pub average_filled_price: String,
    // Coinbase returns these as empty strings on terminal or unfilled orders
    // (e.g. cancelled before any partial fill). `deserialize_decimal_or_zero`
    // accepts `""` and `"0"` as `Decimal::ZERO` so a single empty field does
    // not fail the entire historical-order batch.
    #[serde(default, deserialize_with = "deserialize_decimal_or_zero")]
    pub fee: Decimal,
    #[serde(default, deserialize_with = "deserialize_string_to_u64")]
    pub number_of_fills: u64,
    #[serde(default, deserialize_with = "deserialize_decimal_or_zero")]
    pub filled_value: Decimal,
    #[serde(default)]
    pub pending_cancel: bool,
    #[serde(default)]
    pub size_in_quote: bool,
    #[serde(default, deserialize_with = "deserialize_decimal_or_zero")]
    pub total_fees: Decimal,
    #[serde(default)]
    pub size_inclusive_of_fees: bool,
    #[serde(default, deserialize_with = "deserialize_decimal_or_zero")]
    pub total_value_after_fees: Decimal,
    pub trigger_status: CoinbaseTriggerStatus,
    pub order_type: CoinbaseOrderType,
    #[serde(default)]
    pub reject_reason: String,
    #[serde(default)]
    pub settled: bool,
    #[serde(deserialize_with = "deserialize_product_type_or_unknown")]
    pub product_type: CoinbaseProductType,
    #[serde(default)]
    pub reject_message: String,
    #[serde(default)]
    pub cancel_message: String,
    pub order_placement_source: CoinbaseOrderPlacementSource,
    #[serde(default, deserialize_with = "deserialize_decimal_or_zero")]
    pub outstanding_hold_amount: Decimal,
    #[serde(default)]
    pub is_liquidation: bool,
    #[serde(default)]
    pub last_fill_time: Option<String>,
    #[serde(default)]
    pub leverage: String,
    #[serde(default, deserialize_with = "deserialize_margin_type_or_none")]
    pub margin_type: Option<CoinbaseMarginType>,
    #[serde(default)]
    pub retail_portfolio_id: String,
    #[serde(default)]
    pub originating_order_id: String,
    #[serde(default)]
    pub attached_order_id: String,
}

/// Response for `GET /api/v3/brokerage/cfm/balance_summary`.
#[derive(Debug, Clone, Deserialize)]
pub struct CfmBalanceSummaryResponse {
    pub balance_summary: CfmBalanceSummary,
}

/// Coinbase FCM (futures) balance summary.
#[derive(Debug, Clone, Deserialize)]
pub struct CfmBalanceSummary {
    pub futures_buying_power: CfmAmount,
    pub total_usd_balance: CfmAmount,
    pub cbi_usd_balance: CfmAmount,
    pub cfm_usd_balance: CfmAmount,
    pub total_open_orders_hold_amount: CfmAmount,
    pub unrealized_pnl: CfmAmount,
    pub daily_realized_pnl: CfmAmount,
    pub initial_margin: CfmAmount,
    pub available_margin: CfmAmount,
    pub liquidation_threshold: CfmAmount,
    pub liquidation_buffer_amount: CfmAmount,
    #[serde(default)]
    pub liquidation_buffer_percentage: String,
    #[serde(default)]
    pub intraday_margin_window_measure: Option<CfmMarginWindowMeasure>,
    #[serde(default)]
    pub overnight_margin_window_measure: Option<CfmMarginWindowMeasure>,
}

/// Monetary value with an explicit currency code.
///
/// REST returns FCM money fields as `{value: "...", currency: "USD"}`; the
/// scalar-only WebSocket form is covered by
/// [`crate::websocket::messages::WsFcmBalanceSummary`].
#[derive(Debug, Clone, Deserialize)]
pub struct CfmAmount {
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub value: Decimal,
    pub currency: Ustr,
}

/// Margin window breakdown inside an FCM balance summary.
#[derive(Debug, Clone, Deserialize)]
pub struct CfmMarginWindowMeasure {
    pub margin_window_type: crate::common::enums::CoinbaseMarginWindowType,
    pub margin_level: crate::common::enums::CoinbaseMarginLevel,
    pub initial_margin: CfmAmount,
    pub maintenance_margin: CfmAmount,
    #[serde(default)]
    pub liquidation_buffer_percentage: String,
    pub total_hold: CfmAmount,
    pub futures_buying_power: CfmAmount,
}

/// Response for `GET /api/v3/brokerage/cfm/positions`.
#[derive(Debug, Clone, Deserialize)]
pub struct CfmPositionsResponse {
    pub positions: Vec<CfmPosition>,
}

/// Response for `GET /api/v3/brokerage/cfm/positions/{product_id}`.
#[derive(Debug, Clone, Deserialize)]
pub struct CfmPositionResponse {
    pub position: CfmPosition,
}

/// Position on the Coinbase FCM venue.
#[derive(Debug, Clone, Deserialize)]
pub struct CfmPosition {
    pub product_id: Ustr,
    #[serde(default)]
    pub expiration_time: String,
    pub side: crate::common::enums::CoinbaseFcmPositionSide,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub number_of_contracts: Decimal,
    pub current_price: CfmAmount,
    pub avg_entry_price: CfmAmount,
    pub unrealized_pnl: CfmAmount,
    pub daily_realized_pnl: CfmAmount,
    #[serde(default)]
    pub total_fees: Option<CfmAmount>,
    #[serde(default)]
    pub contract_size: String,
    #[serde(default)]
    pub entry_vwap: Option<CfmAmount>,
    #[serde(default)]
    pub liquidation_price: Option<CfmAmount>,
    #[serde(default)]
    pub leverage: String,
    #[serde(default)]
    pub im_contribution: Option<CfmAmount>,
    #[serde(default)]
    pub mm_contribution: Option<CfmAmount>,
    #[serde(default)]
    pub position_notional: Option<CfmAmount>,
}

/// Response for listing fills.
#[derive(Debug, Clone, Deserialize)]
pub struct FillsResponse {
    pub fills: Vec<Fill>,
    #[serde(default)]
    pub cursor: String,
}

/// A single fill (trade execution).
#[derive(Debug, Clone, Deserialize)]
pub struct Fill {
    pub entry_id: String,
    pub trade_id: String,
    pub order_id: String,
    pub trade_time: String,
    pub trade_type: CoinbaseFillTradeType,
    pub price: String,
    pub size: String,
    #[serde(default, deserialize_with = "deserialize_decimal_or_zero")]
    pub commission: Decimal,
    pub product_id: Ustr,
    pub sequence_timestamp: String,
    pub liquidity_indicator: CoinbaseLiquidityIndicator,
    pub size_in_quote: bool,
    pub user_id: String,
    pub side: CoinbaseOrderSide,
    #[serde(default)]
    pub retail_portfolio_id: String,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::common::{consts::ORDER_CONFIG_LIMIT_GTC, testing::load_test_fixture};

    #[rstest]
    fn test_deserialize_product() {
        let json = load_test_fixture("http_product.json");
        let product: Product = serde_json::from_str(&json).unwrap();
        assert_eq!(product.product_id, "BTC-USD");
        assert_eq!(product.product_type, CoinbaseProductType::Spot);
        assert_eq!(product.base_currency_id, "BTC");
        assert_eq!(product.quote_currency_id, "USD");
        assert_eq!(product.base_increment, "0.00000001");
        assert_eq!(product.quote_increment, "0.01");
        assert_eq!(product.price_increment, "0.01");
        assert!(!product.is_disabled);
        assert!(!product.trading_disabled);
    }

    #[rstest]
    fn test_deserialize_products_list() {
        let json = load_test_fixture("http_products.json");
        let response: ProductsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.products.len(), 2);
        assert_eq!(response.products[0].product_id, "BTC-USD");
        assert_eq!(response.products[1].product_id, "BTC-USDC");
    }

    #[rstest]
    fn test_deserialize_products_future() {
        let json = load_test_fixture("http_products_future.json");
        let response: ProductsResponse = serde_json::from_str(&json).unwrap();
        assert!(!response.products.is_empty());
        assert_eq!(
            response.products[0].product_type,
            CoinbaseProductType::Future
        );
        assert!(response.products[0].fcm_trading_session_details.is_some());
    }

    #[rstest]
    fn test_deserialize_candles() {
        let json = load_test_fixture("http_candles.json");
        let response: CandlesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.candles.len(), 2);

        let candle = &response.candles[0];
        assert!(!candle.start.is_empty());
        assert!(!candle.open.is_empty());
        assert!(!candle.high.is_empty());
        assert!(!candle.low.is_empty());
        assert!(!candle.close.is_empty());
        assert!(!candle.volume.is_empty());
    }

    #[rstest]
    fn test_deserialize_ticker() {
        let json = load_test_fixture("http_ticker.json");
        let response: TickerResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.trades.len(), 3);
        assert!(!response.best_bid.is_empty());
        assert!(!response.best_ask.is_empty());

        let trade = &response.trades[0];
        assert_eq!(trade.product_id, "BTC-USD");
        assert!(!trade.price.is_empty());
        assert!(!trade.size.is_empty());
        assert!(!trade.time.is_empty());
        assert!(trade.side == CoinbaseOrderSide::Buy || trade.side == CoinbaseOrderSide::Sell);
    }

    #[rstest]
    fn test_deserialize_product_book() {
        let json = load_test_fixture("http_product_book.json");
        let response: ProductBookResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.pricebook.product_id, "BTC-USD");
        assert!(!response.pricebook.bids.is_empty());
        assert!(!response.pricebook.asks.is_empty());
        assert!(!response.pricebook.time.is_empty());
        assert!(!response.spread_absolute.is_empty());

        let bid = &response.pricebook.bids[0];
        assert!(!bid.price.is_empty());
        assert!(!bid.size.is_empty());
    }

    #[rstest]
    fn test_product_spot_fields() {
        let json = load_test_fixture("http_product.json");
        let product: Product = serde_json::from_str(&json).unwrap();

        // Verify precision-relevant fields
        assert_eq!(product.base_min_size, "0.00000001");
        assert_eq!(product.base_max_size, "3400");
        assert_eq!(product.quote_min_size, "1");
        assert_eq!(product.quote_max_size, "150000000");
        assert_eq!(product.product_venue, Some(CoinbaseProductVenue::Cbe));
    }

    #[rstest]
    fn test_deserialize_order() {
        let json = load_test_fixture("http_order.json");
        let response: OrderResponse = serde_json::from_str(&json).unwrap();
        let order = response.order;

        assert_eq!(order.order_id, "0000-000000-000000");
        assert_eq!(order.product_id, "BTC-USD");
        assert_eq!(order.side, CoinbaseOrderSide::Buy);
        assert_eq!(order.status, CoinbaseOrderStatus::Open);
        assert_eq!(order.client_order_id, "11111-000000-000000");
        assert_eq!(
            order.time_in_force,
            Some(CoinbaseTimeInForce::GoodUntilCancelled)
        );
        assert_eq!(order.order_type, CoinbaseOrderType::Limit);
        assert_eq!(
            order.trigger_status,
            CoinbaseTriggerStatus::InvalidOrderType
        );
        assert_eq!(
            order.order_placement_source,
            CoinbaseOrderPlacementSource::RetailAdvanced
        );
        assert_eq!(order.margin_type, Some(CoinbaseMarginType::Cross));
        assert_eq!(order.filled_size, "0.001");
        assert_eq!(order.average_filled_price, "50");
        assert_eq!(order.fee, Decimal::ZERO);
        assert_eq!(order.number_of_fills, 2);
        assert_eq!(order.filled_value, Decimal::from_str("10000").unwrap());
        assert_eq!(order.total_fees, Decimal::from_str("5.00").unwrap());
        assert_eq!(
            order.total_value_after_fees,
            Decimal::from_str("10000").unwrap()
        );
        assert_eq!(
            order.outstanding_hold_amount,
            Decimal::from_str("1.00").unwrap()
        );
        assert_eq!(
            order.last_fill_time.as_deref(),
            Some("2021-05-31T10:30:00Z")
        );
        // History configs are kept as raw JSON so unknown Coinbase variants
        // don't fail the whole batch; verify the shape by key lookup.
        let config = order
            .order_configuration
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("order_configuration should be a JSON object");
        assert!(config.contains_key(ORDER_CONFIG_LIMIT_GTC));
    }

    #[rstest]
    fn test_deserialize_orders_list() {
        let json = load_test_fixture("http_orders_list.json");
        let response: OrdersListResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.orders.len(), 2);
        assert!(!response.has_next);

        let open_order = &response.orders[0];
        assert_eq!(open_order.status, CoinbaseOrderStatus::Open);
        assert_eq!(open_order.side, CoinbaseOrderSide::Buy);
        assert_eq!(open_order.order_type, CoinbaseOrderType::Limit);
        assert_eq!(
            open_order.trigger_status,
            CoinbaseTriggerStatus::InvalidOrderType
        );
        assert_eq!(open_order.margin_type, None);
        assert_eq!(open_order.number_of_fills, 0);
        assert_eq!(open_order.total_fees, Decimal::ZERO);

        let filled_order = &response.orders[1];
        assert_eq!(filled_order.status, CoinbaseOrderStatus::Filled);
        assert_eq!(filled_order.side, CoinbaseOrderSide::Sell);
        assert_eq!(filled_order.order_type, CoinbaseOrderType::Market);
        assert_eq!(filled_order.margin_type, None);
        assert!(filled_order.size_in_quote);
        assert_eq!(
            filled_order.time_in_force,
            Some(CoinbaseTimeInForce::ImmediateOrCancel)
        );
    }

    #[rstest]
    fn test_deserialize_fills() {
        let json = load_test_fixture("http_fills.json");
        let response: FillsResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.fills.len(), 2);

        let maker_fill = &response.fills[0];
        assert_eq!(maker_fill.trade_id, "1111-11111-111111");
        assert_eq!(maker_fill.order_id, "0000-000000-000000");
        assert_eq!(maker_fill.product_id, "BTC-USD");
        assert_eq!(maker_fill.price, "45123.45");
        assert_eq!(maker_fill.size, "0.005");
        assert_eq!(maker_fill.trade_type, CoinbaseFillTradeType::Fill);
        assert_eq!(maker_fill.commission, Decimal::from_str("1.14").unwrap());
        assert_eq!(maker_fill.side, CoinbaseOrderSide::Buy);
        assert_eq!(
            maker_fill.liquidity_indicator,
            CoinbaseLiquidityIndicator::Maker
        );

        let taker_fill = &response.fills[1];
        assert_eq!(
            taker_fill.liquidity_indicator,
            CoinbaseLiquidityIndicator::Taker
        );
    }

    #[rstest]
    fn test_deserialize_accounts() {
        let json = load_test_fixture("http_accounts.json");
        let response: AccountsResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.accounts.len(), 2);
        assert!(!response.has_next);

        let btc_account = &response.accounts[0];
        assert_eq!(btc_account.currency, "BTC");
        assert_eq!(
            btc_account.available_balance.value,
            Decimal::from_str("1.23456789").unwrap()
        );
        assert_eq!(btc_account.available_balance.currency, "BTC");
        assert_eq!(btc_account.account_type, CoinbaseAccountType::Crypto);
        assert!(btc_account.default);
        assert_eq!(
            btc_account.hold.as_ref().map(|b| b.value),
            Some(Decimal::from_str("0.00500000").unwrap())
        );

        let usd_account = &response.accounts[1];
        assert_eq!(usd_account.currency, "USD");
        assert_eq!(
            usd_account.available_balance.value,
            Decimal::from_str("10000.50").unwrap()
        );
        assert_eq!(usd_account.account_type, CoinbaseAccountType::Fiat);
    }

    #[rstest]
    fn test_future_product_fields() {
        let json = load_test_fixture("http_products_future.json");
        let response: ProductsResponse = serde_json::from_str(&json).unwrap();
        let product = &response.products[0];

        assert_eq!(product.product_type, CoinbaseProductType::Future);
        assert_eq!(product.product_venue, Some(CoinbaseProductVenue::Fcm));
        assert!(product.future_product_details.is_some());

        let session = product.fcm_trading_session_details.as_ref().unwrap();
        assert_eq!(session.session_state, CoinbaseFcmTradingSessionState::Open);
        assert_eq!(
            session.closed_reason,
            CoinbaseFcmTradingSessionClosedReason::ExchangeMaintenance
        );

        let details = product.future_product_details.as_ref().unwrap();
        assert!(!details.contract_code.is_empty());
        assert!(!details.contract_size.is_empty());
        assert_eq!(details.risk_managed_by, CoinbaseRiskManagedBy::ManagedByFcm);
    }

    #[rstest]
    fn test_deserialize_edit_order_response_success() {
        let json = r#"{"success": true, "errors": []}"#;
        let resp: EditOrderResponse = serde_json::from_str(json).unwrap();
        assert!(resp.success);
        assert!(resp.errors.is_empty());
    }

    #[rstest]
    fn test_deserialize_edit_order_response_failure() {
        let json = r#"{
            "success": false,
            "errors": [
                {
                    "edit_failure_reason": "ORDER_NOT_FOUND",
                    "preview_failure_reason": ""
                }
            ]
        }"#;
        let resp: EditOrderResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.success);
        assert_eq!(resp.errors.len(), 1);
        assert_eq!(resp.errors[0].edit_failure_reason, "ORDER_NOT_FOUND");
        assert_eq!(resp.errors[0].preview_failure_reason, "");
    }

    #[rstest]
    fn test_deserialize_edit_order_response_preview_failure() {
        let json = r#"{
            "success": false,
            "errors": [
                {
                    "edit_failure_reason": "",
                    "preview_failure_reason": "PREVIEW_INSUFFICIENT_FUNDS"
                }
            ]
        }"#;
        let resp: EditOrderResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.success);
        assert_eq!(
            resp.errors[0].preview_failure_reason,
            "PREVIEW_INSUFFICIENT_FUNDS"
        );
    }

    #[rstest]
    fn test_deserialize_edit_order_response_omitted_errors_defaults_empty() {
        let json = r#"{"success": true}"#;
        let resp: EditOrderResponse = serde_json::from_str(json).unwrap();
        assert!(resp.success);
        assert!(resp.errors.is_empty());
    }
}
