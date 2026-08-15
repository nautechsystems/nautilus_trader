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

//! WebSocket transport with runtime backend selection and adapter‑facing lifecycle policy.
//!
//! # Architecture
//!
//! [`WebSocketClient`] coordinates a controller, one serialized writer, an optional heartbeat, and
//! either a managed reader or a caller‑owned stream. Application text and binary sends can await
//! default or keyed quotas from a shared [`RateLimiter`](crate::ratelimiter::RateLimiter).
//!
//! [`client`] manages connection lifecycle and concurrent tasks. [`auth`] coordinates
//! adapter‑driven authentication and optional replay gating, while [`subscription`] records
//! adapter‑driven subscription intent and acknowledgments. [`config`], [`types`], and [`proxy`]
//! define connection policy and transport boundaries.
//!
//! # Operating modes
//!
//! [`WebSocketClient`] supports handler and stream modes. Handler mode owns the reader and replaces
//! it during automatic reconnects. Stream mode returns the reader to the caller and disables
//! automatic reconnects because the client cannot replace caller‑owned state.
//!
//! # Liveness
//!
//! A configured heartbeat sends either a protocol Ping or a text message at a fixed interval; it
//! does not imply a response timeout. Handler mode can separately reconnect when no frame arrives
//! before a heartbeat timeout or when no text or binary application data arrives before the idle
//! timeout. Ping and Pong reset the frame timeout but not the application‑data idle timeout.
//!
//! # State reporting and explicit reconnect
//!
//! An optional [`crate::SocketStateSink`] publishes ordered `Connected` and `Disconnected`
//! availability edges for initial connection, transport loss, and recovery. It omits retry attempts
//! and deliberate shutdown. [`WebSocketReconnectHandle`] lets adapter tasks request transport
//! replacement without owning the client. Handler mode accepts an active request, invalidates
//! registered authentication state, and reports the loss before reconnecting; stream mode returns
//! [`crate::mode::ReconnectRequestOutcome::Unsupported`].
//!
//! # Reconnection and sends
//!
//! The writer task serializes sends and is the sole owner of the active sink. Ordinary sends retain
//! FIFO buffering and replay across reconnects. Ownership‑bound sends carry an expected connection
//! epoch and never enter that replay buffer.
//!
//! The initial connection has epoch `0`. The writer advances the epoch when it installs a
//! replacement sink. Epoch‑aware handlers receive that epoch on messages from the replacement
//! reader and on its reconnect notification. Epochs identify transport ownership; they do not
//! order application authentication or subscription recovery.
//!
//! # Transport backends
//!
//! The backend‑neutral [`Message`](crate::transport::Message) and
//! [`TransportError`](crate::transport::TransportError) types keep lifecycle code independent of
//! the concrete library. [`tokio-tungstenite`](https://github.com/snapview/tokio-tungstenite) is
//! always available. [`sockudo-ws`](https://github.com/sockudo/sockudo-ws) is enabled and selected
//! by default through the `transport-sockudo` feature. Both accept custom upgrade headers. Proxy
//! connections use Tungstenite; selecting Sockudo with a proxy falls back to that backend.

pub mod auth;
pub mod client;
pub mod config;
pub mod consts;
pub mod proxy;
pub mod subscription;
pub mod types;

// Re-export main types for convenience
pub use auth::AuthTracker;
pub use client::{
    ReconnectHeaders, WebSocketClient, WebSocketClientInner, WebSocketReconnectHandle,
};
pub use config::{TransportBackend, WebSocketConfig};
pub use consts::{AUTHENTICATION_TIMEOUT_SECS, TEXT_PING, TEXT_PONG};
pub use subscription::{SubscriptionState, split_topic};
pub use types::{
    EpochMessageHandler, EpochPingHandler, MessageHandler, MessageReader, PingHandler,
    channel_epoch_message_handler, channel_message_handler,
};
