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

//! Protocol Buffer definitions for dYdX v4.
//!
//! This module re-exports the `dydx-proto` crate which contains pre-compiled
//! Protocol Buffer definitions from the official dYdX v4 chain repository.
//!
//! The proto definitions include:
//! - Cosmos SDK base types and gRPC services
//! - dYdX protocol-specific messages (orders, subaccounts, markets, etc.)
//! - gRPC service clients for querying and broadcasting transactions

// Re-export the entire dydx-proto crate for full access
pub use dydx_proto::*;
