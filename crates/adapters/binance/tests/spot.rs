//! Integration tests for Binance Spot adapter.

#[path = "spot/execution.rs"]
mod execution;
#[path = "spot/http.rs"]
mod http;
#[path = "spot/websocket_streams.rs"]
mod websocket_streams;
#[path = "spot/websocket_trading.rs"]
mod websocket_trading;
