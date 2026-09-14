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

//! Asynchronous HTTP requests with rate limiting, connection reuse, and bounded responses.
//!
//! # Architecture
//!
//! [`HttpClient`] applies quota policy before delegating requests to [`InnerHttpClient`]. The inner
//! client owns one reusable Hyper client, preserving its connection pool across requests and clones.
//!
//! # Rate limiting and requests
//!
//! Requests can await default and per-key quotas from one or more shared
//! [`RateLimiter`](crate::ratelimiter::RateLimiter) instances. Sharing a limiter across clients
//! enforces one process-wide budget for scopes such as an IP address or account. The client accepts
//! default and per-request headers, repeated query values, raw bodies, client-level and per-request
//! timeouts, and an optional proxy.
//!
//! HTTP status errors remain [`HttpResponse`] values for adapter-specific handling. The transport
//! retries requests canceled before transmission on reused connections, and allows two retries for
//! remote HTTP/2 `GOAWAY(NO_ERROR)` or `REFUSED_STREAM` errors. Other transport failures and HTTP
//! status codes do not trigger retries. Adapters can apply [`crate::retry::RetryManager`] when the
//! operation and venue error are safe to retry.
//!
//! # Connection and response policy
//!
//! Production clients built through [`HttpClient::builder`] enable `TCP_NODELAY`, pooled idle
//! connections, HTTP/2 keepalive while idle, and adaptive HTTP/2 flow control. Buffered responses
//! retain only configured header fields and reject bodies larger than 100 MiB, including chunked
//! bodies without a declared length. [`HttpClient::get_stream`] consumes bodies incrementally
//! without a total size limit. The redacted request path removes credential-bearing URLs from
//! transport errors and logs.
//!
//! Hyper owns the lifecycle of individual pooled connections, so this client exposes no socket
//! state sink or explicit reconnect operation. Callers observe connection failure through each
//! request result and retain the client to preserve its pool.

pub mod client;
pub mod error;
pub mod types;

#[cfg(not(all(feature = "simulation", madsim)))]
mod connector;
#[cfg(all(feature = "simulation", madsim))]
mod simulation;
#[cfg(not(all(feature = "simulation", madsim)))]
mod transport;

mod stream;

// Re-exports
pub use client::{HttpClient, HttpRedirectPolicy, InnerHttpClient};
pub use error::HttpClientError;
pub use http::{Method, StatusCode, header::USER_AGENT};
pub use stream::HttpResponseStream;
pub use types::{HttpMethod, HttpResponse, HttpStatus};
pub use url::Url;

#[cfg(all(test, not(all(feature = "simulation", madsim))))]
mod tests;
