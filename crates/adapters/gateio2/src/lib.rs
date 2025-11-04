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

//! Rust integration adapter for the [Gate.io](https://www.gate.io/) cryptocurrency exchange.
//!
//! This crate provides Nautilus Trader integration for the Gate.io exchange,
//! enabling automated trading and market data consumption across spot, futures,
//! and other markets.

pub mod common;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;

// Re-export main types
pub use common::*;
pub use http::*;
pub use websocket::*;
