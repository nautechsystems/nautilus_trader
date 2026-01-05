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

//! WebSocket client for Architect real-time data and execution.
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

pub use data::{
    ArchitectMdWebSocketClient, ArchitectWsClientError, ArchitectWsResult,
    HandlerCommand as DataHandlerCommand,
};
pub use messages::{ArchitectOrdersWsMessage, ArchitectWsError, NautilusWsMessage, OrderMetadata};
pub use orders::{
    ArchitectOrdersWebSocketClient, ArchitectOrdersWsClientError, ArchitectOrdersWsResult,
    HandlerCommand as OrdersHandlerCommand,
};
