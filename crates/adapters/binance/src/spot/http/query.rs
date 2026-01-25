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

//! Query parameter builders for Binance Spot HTTP requests.

#[cfg(feature = "python")]
use pyo3::prelude::*;
use serde::Serialize;

use crate::{
    common::enums::{BinanceSelfTradePreventionMode, BinanceSide, BinanceTimeInForce},
    spot::enums::{BinanceCancelReplaceMode, BinanceOrderResponseType, BinanceSpotOrderType},
};

/// Query parameters for the depth endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct DepthParams {
    /// Trading pair symbol (e.g., "BTCUSDT").
    pub symbol: String,
    /// Number of price levels to return (default 100, max 5000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl DepthParams {
    /// Create new depth query params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            limit: None,
        }
    }

    /// Set the limit.
    #[must_use]
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Query parameters for the trades endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct TradesParams {
    /// Trading pair symbol (e.g., "BTCUSDT").
    pub symbol: String,
    /// Number of trades to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl TradesParams {
    /// Create new trades query params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            limit: None,
        }
    }

    /// Set the limit.
    #[must_use]
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Query parameters for new order submission.
#[derive(Debug, Clone, Serialize)]
pub struct NewOrderParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order side (BUY or SELL).
    pub side: BinanceSide,
    /// Order type.
    #[serde(rename = "type")]
    pub order_type: BinanceSpotOrderType,
    /// Time in force.
    #[serde(skip_serializing_if = "Option::is_none", rename = "timeInForce")]
    pub time_in_force: Option<BinanceTimeInForce>,
    /// Order quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<String>,
    /// Quote order quantity (for market orders).
    #[serde(skip_serializing_if = "Option::is_none", rename = "quoteOrderQty")]
    pub quote_order_qty: Option<String>,
    /// Limit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Client order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "newClientOrderId")]
    pub new_client_order_id: Option<String>,
    /// Stop price for stop orders.
    #[serde(skip_serializing_if = "Option::is_none", rename = "stopPrice")]
    pub stop_price: Option<String>,
    /// Trailing delta for trailing stop orders.
    #[serde(skip_serializing_if = "Option::is_none", rename = "trailingDelta")]
    pub trailing_delta: Option<i64>,
    /// Iceberg quantity.
    #[serde(skip_serializing_if = "Option::is_none", rename = "icebergQty")]
    pub iceberg_qty: Option<String>,
    /// Response type (ACK, RESULT, or FULL).
    #[serde(skip_serializing_if = "Option::is_none", rename = "newOrderRespType")]
    pub new_order_resp_type: Option<BinanceOrderResponseType>,
    /// Self-trade prevention mode.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "selfTradePreventionMode"
    )]
    pub self_trade_prevention_mode: Option<BinanceSelfTradePreventionMode>,
    /// Strategy ID for order tracking.
    #[serde(skip_serializing_if = "Option::is_none", rename = "strategyId")]
    pub strategy_id: Option<i64>,
    /// Strategy type for order tracking.
    #[serde(skip_serializing_if = "Option::is_none", rename = "strategyType")]
    pub strategy_type: Option<i64>,
}

impl NewOrderParams {
    /// Create new order params for a limit order.
    #[must_use]
    pub fn limit(
        symbol: impl Into<String>,
        side: BinanceSide,
        quantity: impl Into<String>,
        price: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            side,
            order_type: BinanceSpotOrderType::Limit,
            time_in_force: Some(BinanceTimeInForce::Gtc),
            quantity: Some(quantity.into()),
            quote_order_qty: None,
            price: Some(price.into()),
            new_client_order_id: None,
            stop_price: None,
            trailing_delta: None,
            iceberg_qty: None,
            new_order_resp_type: Some(BinanceOrderResponseType::Full),
            self_trade_prevention_mode: None,
            strategy_id: None,
            strategy_type: None,
        }
    }

    /// Create new order params for a market order.
    #[must_use]
    pub fn market(
        symbol: impl Into<String>,
        side: BinanceSide,
        quantity: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            side,
            order_type: BinanceSpotOrderType::Market,
            time_in_force: None,
            quantity: Some(quantity.into()),
            quote_order_qty: None,
            price: None,
            new_client_order_id: None,
            stop_price: None,
            trailing_delta: None,
            iceberg_qty: None,
            new_order_resp_type: Some(BinanceOrderResponseType::Full),
            self_trade_prevention_mode: None,
            strategy_id: None,
            strategy_type: None,
        }
    }

    /// Set the client order ID.
    #[must_use]
    pub fn with_client_order_id(mut self, id: impl Into<String>) -> Self {
        self.new_client_order_id = Some(id.into());
        self
    }

    /// Set the time in force.
    #[must_use]
    pub fn with_time_in_force(mut self, tif: BinanceTimeInForce) -> Self {
        self.time_in_force = Some(tif);
        self
    }

    /// Set the stop price.
    #[must_use]
    pub fn with_stop_price(mut self, price: impl Into<String>) -> Self {
        self.stop_price = Some(price.into());
        self
    }

    /// Set the self-trade prevention mode.
    #[must_use]
    pub fn with_stp_mode(mut self, mode: BinanceSelfTradePreventionMode) -> Self {
        self.self_trade_prevention_mode = Some(mode);
        self
    }
}

/// Query parameters for canceling an order.
#[derive(Debug, Clone, Serialize)]
pub struct CancelOrderParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order ID to cancel.
    #[serde(skip_serializing_if = "Option::is_none", rename = "orderId")]
    pub order_id: Option<i64>,
    /// Original client order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "origClientOrderId")]
    pub orig_client_order_id: Option<String>,
    /// New client order ID for the cancel request.
    #[serde(skip_serializing_if = "Option::is_none", rename = "newClientOrderId")]
    pub new_client_order_id: Option<String>,
}

impl CancelOrderParams {
    /// Create cancel params by order ID.
    #[must_use]
    pub fn by_order_id(symbol: impl Into<String>, order_id: i64) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: Some(order_id),
            orig_client_order_id: None,
            new_client_order_id: None,
        }
    }

    /// Create cancel params by client order ID.
    #[must_use]
    pub fn by_client_order_id(
        symbol: impl Into<String>,
        client_order_id: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: None,
            orig_client_order_id: Some(client_order_id.into()),
            new_client_order_id: None,
        }
    }
}

/// Query parameters for canceling all open orders on a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct CancelOpenOrdersParams {
    /// Trading pair symbol.
    pub symbol: String,
}

impl CancelOpenOrdersParams {
    /// Create new cancel open orders params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
        }
    }
}

/// Query parameters for cancel and replace order.
#[derive(Debug, Clone, Serialize)]
pub struct CancelReplaceOrderParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order side.
    pub side: BinanceSide,
    /// Order type.
    #[serde(rename = "type")]
    pub order_type: BinanceSpotOrderType,
    /// Cancel/replace mode.
    #[serde(rename = "cancelReplaceMode")]
    pub cancel_replace_mode: BinanceCancelReplaceMode,
    /// Time in force.
    #[serde(skip_serializing_if = "Option::is_none", rename = "timeInForce")]
    pub time_in_force: Option<BinanceTimeInForce>,
    /// Order quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<String>,
    /// Quote order quantity.
    #[serde(skip_serializing_if = "Option::is_none", rename = "quoteOrderQty")]
    pub quote_order_qty: Option<String>,
    /// Limit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Order ID to cancel.
    #[serde(skip_serializing_if = "Option::is_none", rename = "cancelOrderId")]
    pub cancel_order_id: Option<i64>,
    /// Client order ID to cancel.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "cancelOrigClientOrderId"
    )]
    pub cancel_orig_client_order_id: Option<String>,
    /// New client order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "newClientOrderId")]
    pub new_client_order_id: Option<String>,
    /// Stop price.
    #[serde(skip_serializing_if = "Option::is_none", rename = "stopPrice")]
    pub stop_price: Option<String>,
    /// Trailing delta.
    #[serde(skip_serializing_if = "Option::is_none", rename = "trailingDelta")]
    pub trailing_delta: Option<i64>,
    /// Iceberg quantity.
    #[serde(skip_serializing_if = "Option::is_none", rename = "icebergQty")]
    pub iceberg_qty: Option<String>,
    /// Response type.
    #[serde(skip_serializing_if = "Option::is_none", rename = "newOrderRespType")]
    pub new_order_resp_type: Option<BinanceOrderResponseType>,
    /// Self-trade prevention mode.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "selfTradePreventionMode"
    )]
    pub self_trade_prevention_mode: Option<BinanceSelfTradePreventionMode>,
}

/// Query parameters for querying a single order.
#[derive(Debug, Clone, Serialize)]
pub struct QueryOrderParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "orderId")]
    pub order_id: Option<i64>,
    /// Original client order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "origClientOrderId")]
    pub orig_client_order_id: Option<String>,
}

impl QueryOrderParams {
    /// Create query params by order ID.
    #[must_use]
    pub fn by_order_id(symbol: impl Into<String>, order_id: i64) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: Some(order_id),
            orig_client_order_id: None,
        }
    }

    /// Create query params by client order ID.
    #[must_use]
    pub fn by_client_order_id(
        symbol: impl Into<String>,
        client_order_id: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: None,
            orig_client_order_id: Some(client_order_id.into()),
        }
    }
}

/// Query parameters for querying open orders.
#[derive(Debug, Clone, Default, Serialize)]
pub struct OpenOrdersParams {
    /// Trading pair symbol (optional, omit for all symbols).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl OpenOrdersParams {
    /// Create new open orders params for all symbols.
    #[must_use]
    pub fn all() -> Self {
        Self { symbol: None }
    }

    /// Create new open orders params for a specific symbol.
    #[must_use]
    pub fn for_symbol(symbol: impl Into<String>) -> Self {
        Self {
            symbol: Some(symbol.into()),
        }
    }
}

/// Query parameters for querying all orders (includes filled/canceled).
#[derive(Debug, Clone, Serialize)]
pub struct AllOrdersParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Filter by order ID (returns orders >= this ID).
    #[serde(skip_serializing_if = "Option::is_none", rename = "orderId")]
    pub order_id: Option<i64>,
    /// Filter by start time.
    #[serde(skip_serializing_if = "Option::is_none", rename = "startTime")]
    pub start_time: Option<i64>,
    /// Filter by end time.
    #[serde(skip_serializing_if = "Option::is_none", rename = "endTime")]
    pub end_time: Option<i64>,
    /// Maximum number of orders to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl AllOrdersParams {
    /// Create new all orders params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: None,
            start_time: None,
            end_time: None,
            limit: None,
        }
    }

    /// Set the limit.
    #[must_use]
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set the time range.
    #[must_use]
    pub fn with_time_range(mut self, start: i64, end: i64) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }
}

/// Query parameters for new OCO order.
#[derive(Debug, Clone, Serialize)]
pub struct NewOcoOrderParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order side.
    pub side: BinanceSide,
    /// Order quantity.
    pub quantity: String,
    /// Limit price (above-market for sell, below-market for buy).
    pub price: String,
    /// Stop price trigger.
    #[serde(rename = "stopPrice")]
    pub stop_price: String,
    /// Stop limit price (optional, creates stop-limit if provided).
    #[serde(skip_serializing_if = "Option::is_none", rename = "stopLimitPrice")]
    pub stop_limit_price: Option<String>,
    /// Client order ID for the entire list.
    #[serde(skip_serializing_if = "Option::is_none", rename = "listClientOrderId")]
    pub list_client_order_id: Option<String>,
    /// Client order ID for the limit order.
    #[serde(skip_serializing_if = "Option::is_none", rename = "limitClientOrderId")]
    pub limit_client_order_id: Option<String>,
    /// Client order ID for the stop order.
    #[serde(skip_serializing_if = "Option::is_none", rename = "stopClientOrderId")]
    pub stop_client_order_id: Option<String>,
    /// Iceberg quantity for the limit leg.
    #[serde(skip_serializing_if = "Option::is_none", rename = "limitIcebergQty")]
    pub limit_iceberg_qty: Option<String>,
    /// Iceberg quantity for the stop leg.
    #[serde(skip_serializing_if = "Option::is_none", rename = "stopIcebergQty")]
    pub stop_iceberg_qty: Option<String>,
    /// Time in force for the stop-limit leg.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "stopLimitTimeInForce"
    )]
    pub stop_limit_time_in_force: Option<BinanceTimeInForce>,
    /// Response type.
    #[serde(skip_serializing_if = "Option::is_none", rename = "newOrderRespType")]
    pub new_order_resp_type: Option<BinanceOrderResponseType>,
    /// Self-trade prevention mode.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "selfTradePreventionMode"
    )]
    pub self_trade_prevention_mode: Option<BinanceSelfTradePreventionMode>,
}

impl NewOcoOrderParams {
    /// Create new OCO order params.
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        side: BinanceSide,
        quantity: impl Into<String>,
        price: impl Into<String>,
        stop_price: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            side,
            quantity: quantity.into(),
            price: price.into(),
            stop_price: stop_price.into(),
            stop_limit_price: None,
            list_client_order_id: None,
            limit_client_order_id: None,
            stop_client_order_id: None,
            limit_iceberg_qty: None,
            stop_iceberg_qty: None,
            stop_limit_time_in_force: None,
            new_order_resp_type: Some(BinanceOrderResponseType::Full),
            self_trade_prevention_mode: None,
        }
    }

    /// Set stop limit price (makes stop leg a stop-limit order).
    #[must_use]
    pub fn with_stop_limit_price(mut self, price: impl Into<String>) -> Self {
        self.stop_limit_price = Some(price.into());
        self.stop_limit_time_in_force = Some(BinanceTimeInForce::Gtc);
        self
    }
}

/// Query parameters for canceling an order list (OCO).
#[derive(Debug, Clone, Serialize)]
pub struct CancelOrderListParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order list ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "orderListId")]
    pub order_list_id: Option<i64>,
    /// List client order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "listClientOrderId")]
    pub list_client_order_id: Option<String>,
    /// New client order ID for the cancel request.
    #[serde(skip_serializing_if = "Option::is_none", rename = "newClientOrderId")]
    pub new_client_order_id: Option<String>,
}

impl CancelOrderListParams {
    /// Create cancel params by order list ID.
    #[must_use]
    pub fn by_order_list_id(symbol: impl Into<String>, order_list_id: i64) -> Self {
        Self {
            symbol: symbol.into(),
            order_list_id: Some(order_list_id),
            list_client_order_id: None,
            new_client_order_id: None,
        }
    }

    /// Create cancel params by list client order ID.
    #[must_use]
    pub fn by_list_client_order_id(
        symbol: impl Into<String>,
        list_client_order_id: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            order_list_id: None,
            list_client_order_id: Some(list_client_order_id.into()),
            new_client_order_id: None,
        }
    }
}

/// Query parameters for querying an order list (OCO).
#[derive(Debug, Clone, Serialize)]
pub struct QueryOrderListParams {
    /// Order list ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "orderListId")]
    pub order_list_id: Option<i64>,
    /// List client order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "origClientOrderId")]
    pub orig_client_order_id: Option<String>,
}

impl QueryOrderListParams {
    /// Create query params by order list ID.
    #[must_use]
    pub fn by_order_list_id(order_list_id: i64) -> Self {
        Self {
            order_list_id: Some(order_list_id),
            orig_client_order_id: None,
        }
    }

    /// Create query params by list client order ID.
    #[must_use]
    pub fn by_client_order_id(client_order_id: impl Into<String>) -> Self {
        Self {
            order_list_id: None,
            orig_client_order_id: Some(client_order_id.into()),
        }
    }
}

/// Query parameters for querying all order lists (OCOs).
#[derive(Debug, Clone, Default, Serialize)]
pub struct AllOrderListsParams {
    /// Filter by start time.
    #[serde(skip_serializing_if = "Option::is_none", rename = "startTime")]
    pub start_time: Option<i64>,
    /// Filter by end time.
    #[serde(skip_serializing_if = "Option::is_none", rename = "endTime")]
    pub end_time: Option<i64>,
    /// Maximum number of results (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for querying open order lists (OCOs).
#[derive(Debug, Clone, Default, Serialize)]
pub struct OpenOrderListsParams {}

/// Query parameters for account information.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountInfoParams {
    /// Omit zero balances from response.
    #[serde(skip_serializing_if = "Option::is_none", rename = "omitZeroBalances")]
    pub omit_zero_balances: Option<bool>,
}

impl AccountInfoParams {
    /// Create new account info params.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Omit zero balances from response.
    #[must_use]
    pub fn omit_zero_balances(mut self) -> Self {
        self.omit_zero_balances = Some(true);
        self
    }
}

/// Query parameters for account trades.
#[derive(Debug, Clone, Serialize)]
pub struct AccountTradesParams {
    /// Trading pair symbol.
    pub symbol: String,
    /// Filter by order ID.
    #[serde(skip_serializing_if = "Option::is_none", rename = "orderId")]
    pub order_id: Option<i64>,
    /// Filter by start time.
    #[serde(skip_serializing_if = "Option::is_none", rename = "startTime")]
    pub start_time: Option<i64>,
    /// Filter by end time.
    #[serde(skip_serializing_if = "Option::is_none", rename = "endTime")]
    pub end_time: Option<i64>,
    /// Filter by trade ID (returns trades >= this ID).
    #[serde(skip_serializing_if = "Option::is_none", rename = "fromId")]
    pub from_id: Option<i64>,
    /// Maximum number of trades to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl AccountTradesParams {
    /// Create new account trades params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: None,
            start_time: None,
            end_time: None,
            from_id: None,
            limit: None,
        }
    }

    /// Filter by order ID.
    #[must_use]
    pub fn for_order(mut self, order_id: i64) -> Self {
        self.order_id = Some(order_id);
        self
    }

    /// Set the limit.
    #[must_use]
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set the time range.
    #[must_use]
    pub fn with_time_range(mut self, start: i64, end: i64) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }
}

/// Query parameters for klines (candlestick) data.
#[derive(Debug, Clone, Serialize)]
pub struct KlinesParams {
    /// Trading pair symbol (e.g., "BTCUSDT").
    pub symbol: String,
    /// Kline interval (e.g., "1m", "1h", "1d").
    pub interval: String,
    /// Filter by start time (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none", rename = "startTime")]
    pub start_time: Option<i64>,
    /// Filter by end time (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none", rename = "endTime")]
    pub end_time: Option<i64>,
    /// Kline time zone offset (+/- hours, default 0 UTC).
    #[serde(skip_serializing_if = "Option::is_none", rename = "timeZone")]
    pub time_zone: Option<String>,
    /// Maximum number of klines to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for listen key operations (extend/close).
#[derive(Debug, Clone, Serialize)]
pub struct ListenKeyParams {
    /// The listen key to extend or close.
    #[serde(rename = "listenKey")]
    pub listen_key: String,
}

impl ListenKeyParams {
    /// Creates new listen key params.
    #[must_use]
    pub fn new(listen_key: impl Into<String>) -> Self {
        Self {
            listen_key: listen_key.into(),
        }
    }
}

/// Query parameters for ticker endpoints.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TickerParams {
    /// Trading pair symbol (optional, omit for all symbols).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl TickerParams {
    /// Creates ticker params for all symbols.
    #[must_use]
    pub fn all() -> Self {
        Self { symbol: None }
    }

    /// Creates ticker params for a specific symbol.
    #[must_use]
    pub fn for_symbol(symbol: impl Into<String>) -> Self {
        Self {
            symbol: Some(symbol.into()),
        }
    }
}

/// Query parameters for average price endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct AvgPriceParams {
    /// Trading pair symbol (required).
    pub symbol: String,
}

impl AvgPriceParams {
    /// Creates average price params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
        }
    }
}

/// Query parameters for trade fee endpoint.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TradeFeeParams {
    /// Trading pair symbol (optional, omit for all symbols).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl TradeFeeParams {
    /// Creates trade fee params for all symbols.
    #[must_use]
    pub fn all() -> Self {
        Self { symbol: None }
    }

    /// Creates trade fee params for a specific symbol.
    #[must_use]
    pub fn for_symbol(symbol: impl Into<String>) -> Self {
        Self {
            symbol: Some(symbol.into()),
        }
    }
}

/// Single order in a batch order request (JSON format for batchOrders param).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.binance",
        name = "SpotBatchOrderItem",
        get_all
    )
)]
pub struct BatchOrderItem {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order side (BUY or SELL).
    pub side: String,
    /// Order type.
    #[serde(rename = "type")]
    pub order_type: String,
    /// Time in force.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<String>,
    /// Order quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<String>,
    /// Limit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Client order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_client_order_id: Option<String>,
    /// Stop price for stop orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
}

impl BatchOrderItem {
    /// Creates a batch order item from NewOrderParams.
    #[must_use]
    pub fn from_params(params: &NewOrderParams) -> Self {
        Self {
            symbol: params.symbol.clone(),
            side: format!("{:?}", params.side).to_uppercase(),
            order_type: format!("{:?}", params.order_type).to_uppercase(),
            time_in_force: params
                .time_in_force
                .map(|t| format!("{t:?}").to_uppercase()),
            quantity: params.quantity.clone(),
            price: params.price.clone(),
            new_client_order_id: params.new_client_order_id.clone(),
            stop_price: params.stop_price.clone(),
        }
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl BatchOrderItem {
    #[new]
    #[pyo3(signature = (symbol, side, order_type, time_in_force=None, quantity=None, price=None, new_client_order_id=None, stop_price=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        symbol: String,
        side: String,
        order_type: String,
        time_in_force: Option<String>,
        quantity: Option<String>,
        price: Option<String>,
        new_client_order_id: Option<String>,
        stop_price: Option<String>,
    ) -> Self {
        Self {
            symbol,
            side,
            order_type,
            time_in_force,
            quantity,
            price,
            new_client_order_id,
            stop_price,
        }
    }
}

/// Single cancel in a batch cancel request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.binance",
        name = "SpotBatchCancelItem",
        get_all
    )
)]
pub struct BatchCancelItem {
    /// Trading pair symbol.
    pub symbol: String,
    /// Order ID to cancel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    /// Original client order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig_client_order_id: Option<String>,
}

impl BatchCancelItem {
    /// Creates a batch cancel item by order ID.
    #[must_use]
    pub fn by_order_id(symbol: impl Into<String>, order_id: i64) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: Some(order_id),
            orig_client_order_id: None,
        }
    }

    /// Creates a batch cancel item by client order ID.
    #[must_use]
    pub fn by_client_order_id(
        symbol: impl Into<String>,
        client_order_id: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            order_id: None,
            orig_client_order_id: Some(client_order_id.into()),
        }
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl BatchCancelItem {
    #[new]
    #[pyo3(signature = (symbol, order_id=None, orig_client_order_id=None))]
    fn py_new(symbol: String, order_id: Option<i64>, orig_client_order_id: Option<String>) -> Self {
        Self {
            symbol,
            order_id,
            orig_client_order_id,
        }
    }

    /// Creates a batch cancel item by order ID.
    #[staticmethod]
    #[pyo3(name = "by_order_id")]
    fn py_by_order_id(symbol: String, order_id: i64) -> Self {
        Self::by_order_id(symbol, order_id)
    }

    /// Creates a batch cancel item by client order ID.
    #[staticmethod]
    #[pyo3(name = "by_client_order_id")]
    fn py_by_client_order_id(symbol: String, client_order_id: String) -> Self {
        Self::by_client_order_id(symbol, client_order_id)
    }
}
