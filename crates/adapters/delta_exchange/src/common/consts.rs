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

//! Constants for Delta Exchange integration.

/// Delta Exchange production REST API base URL.
pub const DELTA_EXCHANGE_REST_URL: &str = "https://api.delta.exchange";

/// Delta Exchange testnet REST API base URL.
pub const DELTA_EXCHANGE_TESTNET_REST_URL: &str = "https://testnet-api.delta.exchange";

/// Delta Exchange production WebSocket URL.
pub const DELTA_EXCHANGE_WS_URL: &str = "wss://socket.delta.exchange";

/// Delta Exchange testnet WebSocket URL.
pub const DELTA_EXCHANGE_TESTNET_WS_URL: &str = "wss://testnet-socket.delta.exchange";

// API Endpoints
/// Products endpoint for getting tradeable instruments.
pub const PRODUCTS_ENDPOINT: &str = "/v2/products";

/// Assets endpoint for getting supported assets.
pub const ASSETS_ENDPOINT: &str = "/v2/assets";

/// Orders endpoint for order management.
pub const ORDERS_ENDPOINT: &str = "/v2/orders";

/// Positions endpoint for position queries.
pub const POSITIONS_ENDPOINT: &str = "/v2/positions";

/// Margined positions endpoint for detailed position data.
pub const POSITIONS_MARGINED_ENDPOINT: &str = "/v2/positions/margined";

/// Wallet endpoint for account balances.
pub const WALLET_ENDPOINT: &str = "/v2/wallet";

/// Fills endpoint for trade history.
pub const FILLS_ENDPOINT: &str = "/v2/fills";

/// Order book endpoint for market data.
pub const ORDERBOOK_ENDPOINT: &str = "/v2/l2orderbook";

/// Public trades endpoint.
pub const TRADES_ENDPOINT: &str = "/v2/trades";

/// Tickers endpoint for market statistics.
pub const TICKERS_ENDPOINT: &str = "/v2/tickers";

/// Historical candles endpoint.
pub const CANDLES_ENDPOINT: &str = "/v2/history/candles";

/// Order history endpoint.
pub const ORDER_HISTORY_ENDPOINT: &str = "/v2/orders/history";

/// Wallet transactions endpoint.
pub const WALLET_TRANSACTIONS_ENDPOINT: &str = "/v2/wallet/transactions";

// Rate Limiting
/// Maximum requests per second for Delta Exchange API.
pub const MAX_REQUESTS_PER_SECOND: u32 = 100;

/// Maximum WebSocket connections per IP per 5 minutes.
pub const MAX_WS_CONNECTIONS_PER_IP: u32 = 150;

// Pagination
/// Maximum page size for paginated endpoints.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Default page size for paginated endpoints.
pub const DEFAULT_PAGE_SIZE: u32 = 50;

// WebSocket Channels
/// Public WebSocket channels.
pub const WS_PUBLIC_CHANNELS: &[&str] = &[
    "v2_ticker",
    "l1_orderbook",
    "l2_orderbook", 
    "l2_updates",
    "all_trades",
    "mark_price",
    "candlesticks",
    "spot_price",
    "v2/spot_price",
    "spot_30mtwap_price",
    "funding_rate",
    "product_updates",
    "announcements",
];

/// Private WebSocket channels.
pub const WS_PRIVATE_CHANNELS: &[&str] = &[
    "margins",
    "positions",
    "orders", 
    "user_trades",
    "v2/user_trades",
    "portfolio_margins",
    "mmp_trigger",
];

// Product Types
/// Supported product types on Delta Exchange.
pub const PRODUCT_TYPES: &[&str] = &[
    "perpetual_futures",
    "call_options",
    "put_options",
];

// Order Types
/// Supported order types.
pub const ORDER_TYPES: &[&str] = &[
    "limit_order",
    "market_order", 
    "stop_loss_order",
    "take_profit_order",
];

// Order States
/// Possible order states.
pub const ORDER_STATES: &[&str] = &[
    "open",
    "pending",
    "closed", 
    "cancelled",
];

// Time in Force
/// Supported time in force values.
pub const TIME_IN_FORCE: &[&str] = &[
    "gtc", // Good Till Cancel
    "ioc", // Immediate or Cancel
];

// Environment Variables
/// Environment variable for production API key.
pub const DELTA_EXCHANGE_API_KEY: &str = "DELTA_EXCHANGE_API_KEY";

/// Environment variable for production API secret.
pub const DELTA_EXCHANGE_API_SECRET: &str = "DELTA_EXCHANGE_API_SECRET";

/// Environment variable for testnet API key.
pub const DELTA_EXCHANGE_TESTNET_API_KEY: &str = "DELTA_EXCHANGE_TESTNET_API_KEY";

/// Environment variable for testnet API secret.
pub const DELTA_EXCHANGE_TESTNET_API_SECRET: &str = "DELTA_EXCHANGE_TESTNET_API_SECRET";

// HTTP Headers
/// API key header name.
pub const HEADER_API_KEY: &str = "api-key";

/// Signature header name.
pub const HEADER_SIGNATURE: &str = "signature";

/// Timestamp header name.
pub const HEADER_TIMESTAMP: &str = "timestamp";

// Timeouts
/// Default HTTP request timeout in seconds.
pub const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 60;

/// Default WebSocket connection timeout in seconds.
pub const DEFAULT_WS_TIMEOUT_SECS: u64 = 30;

/// Default reconnection delay in seconds.
pub const DEFAULT_RECONNECTION_DELAY_SECS: u64 = 5;

/// Maximum reconnection attempts.
pub const MAX_RECONNECTION_ATTEMPTS: u32 = 10;

// Precision
/// Timestamp precision in microseconds.
pub const TIMESTAMP_PRECISION: u32 = 6;
