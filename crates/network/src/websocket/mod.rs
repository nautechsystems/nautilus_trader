//! WebSocket client implementation with automatic reconnection and subscription tracking.

pub mod auth;
pub mod client;
pub mod config;
pub mod consts;
pub mod subscription;
pub mod types;

// Re-export main types for convenience
pub use auth::AuthTracker;
pub use client::{WebSocketClient, WebSocketClientInner};
pub use config::WebSocketConfig;
pub use consts::{AUTHENTICATION_TIMEOUT_SECS, TEXT_PING, TEXT_PONG};
pub use subscription::{SubscriptionState, split_topic};
pub use types::{MessageHandler, MessageReader, PingHandler, channel_message_handler};
