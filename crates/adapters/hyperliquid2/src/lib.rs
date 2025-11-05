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

//! Nautilus Trader integration adapter for Hyperliquid cryptocurrency exchange.
//!
//! This adapter provides connectivity to Hyperliquid's perpetual futures trading platform,
//! supporting both market data and order execution via HTTP REST API and WebSocket streams.
//!
//! # Features
//!
//! - **HTTP Client**: REST API for market data, account info, and order management
//! - **WebSocket Client**: Real-time streaming of trades, order books, and user events
//! - **Ethereum-style Authentication**: Uses ethers crate for wallet-based signing
//! - **Type Conversions**: Parsing of Hyperliquid types to Nautilus instruments
//! - **Python Bindings**: PyO3 integration for use in Nautilus Trader Python ecosystem
//!
//! # Example
//!
//! ```rust,no_run
//! use nautilus_hyperliquid2::http::Hyperliquid2HttpClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create HTTP client
//!     let client = Hyperliquid2HttpClient::new(None, None, false)?;
//!
//!     // Load instruments
//!     let instruments = client.load_instruments().await?;
//!     println!("Loaded {} instruments", instruments.len());
//!
//!     Ok(())
//! }
//! ```

pub mod common;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;

// Re-exports
pub use common::*;
pub use http::Hyperliquid2HttpClient;
pub use websocket::Hyperliquid2WebSocketClient;
