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

//! Common functionality shared across the dYdX adapter.
//!
//! This module provides core utilities, constants, and data structures used throughout
//! the dYdX integration, including:
//!
//! - **Authentication**: Wallet credential storage and message signing for dYdX v4.
//! - **Common enumerations**: dYdX-specific enums mirrored from REST/WebSocket payloads.
//! - **Constants**: Venue identifiers, broker IDs, and scaling factors.
//! - **Parsing utilities**: Helpers for converting dYdX data to Nautilus types.
//! - **URL management**: Environment-aware base URL resolvers for testnet and mainnet.
//! - **Common models**: Shared data structures used across HTTP and WebSocket layers.
//! - **Testing helpers**: Fixtures and utilities for unit tests.

pub mod consts;
pub mod credential;
pub mod enums;
pub mod models;
pub mod parse;
pub mod urls;

#[cfg(test)]
pub(crate) mod testing;
