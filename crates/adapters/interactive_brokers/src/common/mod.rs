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

//! Common functionality shared across the Interactive Brokers adapter.
//!
//! This module provides core utilities, constants, and data structures used throughout
//! the Interactive Brokers integration.

pub mod connection;
pub mod consts;
pub mod contracts;
pub mod enums;
pub mod parse;
pub mod shared_client;
pub mod types;

// Re-export commonly used items from parse module
pub use contracts::{
    contract_to_json_value, contract_to_params, parse_contract_from_json,
    parse_contracts_from_json_array,
};
pub use parse::{VENUE_MEMBERS, ib_contract_to_instrument_id_simple, instrument_id_to_ib_contract};
