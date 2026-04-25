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
//! Phase 1 ships with a single backend, `tokio-tungstenite`; conversions between
//! its native types and the neutral types live in [`tungstenite`]. Phase 2 will
//! add feature-gated backend selection (`sockudo-ws` and an in-house crate)
//! once the `WebSocketClient` is migrated onto the [`WsTransport`] trait. At
//! that point each backend module (including [`tungstenite`]) will be gated on
//! a `transport-*` feature so consumers compile only the backend they select.

pub mod error;
pub mod message;
pub mod stream;
pub mod tungstenite;

pub use error::TransportError;
pub use message::{CloseFrame, Message};
pub use stream::{BoxedWsTransport, WsTransport};
