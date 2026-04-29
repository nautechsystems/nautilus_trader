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

//! Transport abstraction layer for WebSocket backends.
//!
//! Defines the backend-agnostic surface that higher layers in `nautilus-network`
//! consume (the reconnecting client, auth tracker, subscription manager, Python
//! bindings, and adapter crates):
//!
//! - [`Message`]: neutral WebSocket message enum.
//! - [`TransportError`]: neutral error type.
//! - [`WsTransport`]: `Stream` plus `Sink` trait for backend implementations.
//!
//! The `tokio-tungstenite` backend is always compiled (its conversions and adapter
//! live in [`tungstenite`]). The `sockudo-ws` backend is gated behind the
//! `transport-sockudo` feature and lives in the `sockudo` submodule; when enabled
//! it can be selected at runtime via `WebSocketConfig.backend`.

pub mod error;
pub mod message;
pub mod stream;
pub mod tungstenite;

#[cfg(feature = "transport-sockudo")]
pub mod sockudo;

pub use error::TransportError;
pub use message::{CloseFrame, Message};
pub use stream::{BoxedWsTransport, WsTransport};
