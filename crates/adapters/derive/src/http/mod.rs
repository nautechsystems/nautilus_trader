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

//! HTTP client for the Derive REST API.
//!
//! The wire format mirrors `derive_client`'s upstream Python SDK:
//!
//! - Each method is addressed at `${base_url}/<method-name>` (e.g.
//!   `/public/get_instruments`, `/private/order`).
//! - Request bodies are the raw `params` object; the method is encoded by the
//!   URL path, not by a wrapping envelope.
//! - Successful responses carry `{ "id": <int>, "result": <T> }`; failures
//!   carry `{ "id": <int>, "error": { "code", "message", "data" } }`. The
//!   shared envelope types in [`models`] are reused by the WebSocket layer,
//!   which speaks the full JSON-RPC framing on the wire.
//!
//! Private endpoints inject the EIP-191 session-key auth headers built by
//! [`crate::signing::auth::build_rest_auth_headers`]; public endpoints carry
//! no auth and may be called without credentials.

pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod query;

pub use client::{DeriveCredentials, DeriveHttpClient};
pub use error::{DeriveHttpError, Result};
pub use models::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
