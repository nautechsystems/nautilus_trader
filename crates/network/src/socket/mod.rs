//! High-performance raw TCP client implementation with TLS capability, automatic reconnection
//! with exponential backoff and state management.

pub mod client;
pub mod config;
pub mod types;

pub use client::SocketClient;
pub use config::SocketConfig;
pub use types::{TcpMessageHandler, TcpReader, TcpWriter, WriterCommand};
