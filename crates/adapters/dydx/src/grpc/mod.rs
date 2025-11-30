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

//! gRPC client implementation for the dYdX v4 protocol.
//!
//! This module provides gRPC client functionality for interacting with the dYdX v4 protocol
//! via the Cosmos SDK. It handles:
//!
//! - Transaction signing and broadcasting using `cosmrs`.
//! - gRPC communication with validator nodes.
//! - Protocol Buffer message encoding/decoding.
//! - Cosmos SDK account management.
//!
//! The client supports dYdX trading operations including:
//!
//! - Order placement, modification, and cancellation.
//! - Transfer operations between subaccounts.
//! - Subaccount management.
//! - Transaction signing with secp256k1 keys.
//!
//! # Architecture
//!
//! dYdX v4 is built on the Cosmos SDK and uses gRPC for all state-changing operations
//! (placing orders, transfers, etc.). The HTTP/REST API (Indexer) is read-only and used
//! for querying market data and historical information.

pub mod builder;
pub mod client;
pub mod order;
pub mod types;
pub mod wallet;

// Re-exports
pub use builder::TxBuilder;
pub use client::{DydxGrpcClient, Height, TxHash};
pub use order::{
    DEFAULT_RUST_CLIENT_METADATA, OrderBuilder, OrderFlags, OrderGoodUntil, OrderMarketParams,
    SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
};
pub use types::ChainId;
pub use wallet::{Account, Subaccount, Wallet};
