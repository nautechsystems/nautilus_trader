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

//! Reconnecting WebSocket transport and adapter‑facing connection state.
//!
//! # Operating modes
//!
//! [`WebSocketClient`] supports handler and stream modes. Handler mode owns the reader and replaces
//! it during automatic reconnects. Stream mode returns the reader to the caller and disables
//! automatic reconnects because the client cannot replace caller‑owned state.
//!
//! # Main components
//!
//! - [`client`] manages connection lifecycle, concurrent tasks, sends, heartbeats, and reconnects.
//! - [`auth`] coordinates adapter‑driven authentication and optional replay gating.
//! - [`subscription`] tracks adapter‑driven subscription intent and acknowledgments.
//! - [`config`], [`types`], and [`proxy`] define connection policy and transport boundaries.
//!
//! # Reconnect invariants
//!
//! The writer task serializes sends and is the sole owner of the active sink. Ordinary sends retain
//! FIFO buffering and replay across reconnects. Ownership‑bound sends carry an expected connection
//! epoch and never enter that replay buffer.
//!
//! The initial connection has epoch `0`. The writer advances the epoch when it installs a
//! replacement sink. Epoch‑aware handlers receive that epoch on messages from the replacement
//! reader and on its reconnect notification. Epochs identify transport ownership; they do not
//! order application authentication or subscription recovery.

pub mod auth;
pub mod client;
pub mod config;
pub mod consts;
pub mod proxy;
pub mod subscription;
pub mod types;

// Re-export main types for convenience
pub use auth::AuthTracker;
pub use client::{ReconnectHeaders, WebSocketClient, WebSocketClientInner};
pub use config::{TransportBackend, WebSocketConfig};
pub use consts::{AUTHENTICATION_TIMEOUT_SECS, TEXT_PING, TEXT_PONG};
pub use subscription::{SubscriptionState, split_topic};
pub use types::{
    EpochMessageHandler, MessageHandler, MessageReader, PingHandler, channel_epoch_message_handler,
    channel_message_handler,
};
