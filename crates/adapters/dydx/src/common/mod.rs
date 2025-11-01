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
//! - Common constants for venues and identifiers.
//! - URL management for HTTP and WebSocket endpoints.
//! - Shared data types and models.
//! - Test utilities for development.

pub mod consts;
pub mod types;
pub mod urls;

#[cfg(test)]
pub(crate) mod testing;
