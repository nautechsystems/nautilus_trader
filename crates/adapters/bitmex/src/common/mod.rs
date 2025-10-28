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

//! Common types and utilities shared across the BitMEX adapter.
//!
//! This module provides reusable components that are used by both the HTTP and WebSocket
//! clients, including:
//! - Constants for BitMEX URLs and venue identifier.
//! - Credential management for API authentication.
//! - Enumerations for order types, sides, and statuses.
//! - Parsing utilities for currency codes and other data transformations.

pub mod consts;
pub mod credential;
pub mod enums;
pub mod parse;
pub mod urls;

#[cfg(test)]
pub(crate) mod testing;
