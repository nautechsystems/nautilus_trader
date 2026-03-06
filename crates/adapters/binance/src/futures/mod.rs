//! Binance Futures adapter components.
//!
//! This module provides HTTP and WebSocket clients for Binance Futures:
//!
//! - **USD-M Futures** (`fapi.binance.com`) - USDT-margined perpetual contracts
//! - **COIN-M Futures** (`dapi.binance.com`) - Coin-margined perpetual contracts
//!
//! ## WebSocket Streams
//!
//! Unlike Spot which uses SBE binary encoding, Futures uses standard JSON WebSocket streams:
//!
//! - `<symbol>@trade` - Real-time trade data
//! - `<symbol>@depth` - Order book updates (diff)
//! - `<symbol>@depth@100ms` - Order book updates (100ms frequency)
//! - `<symbol>@markPrice` - Mark price updates
//! - `<symbol>@kline_<interval>` - Kline/candlestick updates
//!
//! ## Authentication
//!
//! - Public streams: No authentication required
//! - User data streams: Requires listen key (obtained via REST API)

pub mod data;
pub mod execution;
pub mod http;
pub mod websocket;

pub use data::BinanceFuturesDataClient;
pub use execution::BinanceFuturesExecutionClient;
pub use http::client::BinanceFuturesHttpClient;
pub use websocket::client::BinanceFuturesWebSocketClient;
