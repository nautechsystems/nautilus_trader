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

//! Unified error handling for [NautilusTrader](https://nautilustrader.io).
//!
//! Inspired by [OpenDAL's error handling practices](https://xuanwo.io/en-us/reports/2022-46/),
//! this crate provides a single structured error type that enables:
//!
//! - **Programmatic matching** via [`ErrorKind`] — callers can decide how to handle errors.
//! - **Retryability** via [`ErrorStatus`] — callers know whether to retry.
//! - **Structured context** — key-value pairs for debugging and telemetry.
//! - **Operation tracking** — which API call triggered the error.
//! - **Source preservation** — the underlying cause via `anyhow::Error`.
//!
//! # Design principles
//!
//! - Every function should return `Result<T, NautilusError>`.
//! - External library errors are wrapped with `.set_source(err)`.
//! - The same error is handled only once; subsequent layers only append context.
//! - `Display` shows a compact single-line message; `Debug` shows the full chain.

mod error;
mod kind;
mod status;

pub use error::NautilusError;
pub use kind::ErrorKind;
pub use status::ErrorStatus;

/// Convenience type alias for `Result<T, NautilusError>`.
pub type Result<T> = std::result::Result<T, NautilusError>;
