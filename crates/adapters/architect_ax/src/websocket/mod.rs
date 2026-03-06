//! WebSocket client for Ax real-time data and execution.
//!
//! This module provides a two-layer WebSocket client architecture:
//! - Outer client: Orchestrator managing state and subscriptions
//! - Inner handler: I/O boundary running in dedicated Tokio task
//!
//! Features:
//! - Public and private WebSocket streams
//! - Bearer token authentication
//! - Automatic reconnection
//! - Heartbeat/ping-pong
//! - Subscription state management
//! - Message parsing and routing

pub mod data;
pub mod error;
pub mod messages;
pub mod orders;
pub mod parse;

pub use data::{
    AxMdWebSocketClient, AxWsClientError, AxWsResult, HandlerCommand as DataHandlerCommand,
};
pub use messages::{
    AxOrdersWsMessage, AxWsError, NautilusDataWsMessage, NautilusExecWsMessage, OrderMetadata,
};
pub use orders::{
    AxOrdersWebSocketClient, AxOrdersWsClientError, AxOrdersWsResult,
    HandlerCommand as OrdersHandlerCommand,
};
