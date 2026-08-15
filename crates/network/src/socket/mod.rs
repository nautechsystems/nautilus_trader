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

//! Raw TCP clients with suffix framing, optional TLS, heartbeats, and automatic reconnection.
//!
//! # Architecture
//!
//! [`SocketClient`] uses a controller to own connection lifecycle. One reader task splits the byte
//! stream into complete messages for [`TcpMessageHandler`], while one writer task serializes sends
//! from concurrent callers. Optional heartbeat traffic passes through the same writer.
//!
//! # Framing and liveness
//!
//! [`SocketConfig::suffix`] frames both directions. The writer appends it to application and
//! heartbeat messages, and the reader strips it before dispatch. An empty suffix is rejected. A
//! partial frame that exceeds 10 MiB stops the reader and triggers reconnect. An optional idle
//! timeout detects a connection that remains open without delivering bytes.
//!
//! # State reporting and explicit reconnect
//!
//! An optional [`crate::SocketStateSink`] publishes ordered `Connected` and `Disconnected`
//! availability edges for initial connection, transport loss, and recovery. It omits retry attempts
//! and deliberate shutdown. [`SocketReconnectHandle`] lets adapter tasks request transport
//! replacement without owning the client and reports whether each request was accepted, already
//! reconnecting, disconnecting, or closed.
//!
//! # Reconnection and replay
//!
//! Initial connection establishment retries failures with exponential backoff. When a connected
//! transport fails, the controller reconnects with configurable backoff, jitter, timeout,
//! and attempt limits. The writer buffers application messages in FIFO order. A
//! [`SocketReconnectReplay`] can place protocol setup messages before that buffer on the replacement
//! connection, and a post‑reconnection callback runs after the writer, buffer, and reader are ready.
//!
//! # Transport policy
//!
//! Connections support plain TCP or `rustls`, enable `TCP_NODELAY`, and accept either a raw
//! `host:port` address or a URL. A certificate directory can add trusted roots and supply a matching
//! client certificate and key.

pub mod client;
pub mod config;
pub mod types;

pub use client::{SocketClient, SocketReconnectHandle, SocketReconnectReplay};
pub use config::SocketConfig;
pub use types::{TcpMessageHandler, TcpReader, TcpWriter, WriterCommand};
