// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyString, PyTuple},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ustr::Ustr;

use crate::identifier_for_python;

#[macro_use]
mod macros;

pub mod account_id;
pub mod client_id;
pub mod client_order_id;
pub mod component_id;
pub mod exec_algorithm_id;
pub mod instrument_id;
pub mod order_list_id;
pub mod position_id;
pub mod strategy_id;
pub mod symbol;
pub mod trade_id;
pub mod trader_id;
pub mod venue;
pub mod venue_order_id;

impl_from_str_for_identifier!(account_id::AccountId);
impl_from_str_for_identifier!(client_id::ClientId);
impl_from_str_for_identifier!(client_order_id::ClientOrderId);
impl_from_str_for_identifier!(component_id::ComponentId);
impl_from_str_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_from_str_for_identifier!(order_list_id::OrderListId);
impl_from_str_for_identifier!(position_id::PositionId);
impl_from_str_for_identifier!(strategy_id::StrategyId);
impl_from_str_for_identifier!(symbol::Symbol);
impl_from_str_for_identifier!(trade_id::TradeId);
impl_from_str_for_identifier!(trader_id::TraderId);
impl_from_str_for_identifier!(venue::Venue);
impl_from_str_for_identifier!(venue_order_id::VenueOrderId);

impl_serialization_for_identifier!(account_id::AccountId);
impl_serialization_for_identifier!(client_id::ClientId);
impl_serialization_for_identifier!(client_order_id::ClientOrderId);
impl_serialization_for_identifier!(component_id::ComponentId);
impl_serialization_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_serialization_for_identifier!(order_list_id::OrderListId);
impl_serialization_for_identifier!(position_id::PositionId);
impl_serialization_for_identifier!(strategy_id::StrategyId);
impl_serialization_for_identifier!(symbol::Symbol);
impl_serialization_for_identifier!(trade_id::TradeId);
impl_serialization_for_identifier!(trader_id::TraderId);
impl_serialization_for_identifier!(venue::Venue);
impl_serialization_for_identifier!(venue_order_id::VenueOrderId);

identifier_for_python!(account_id::AccountId);
identifier_for_python!(client_id::ClientId);
identifier_for_python!(client_order_id::ClientOrderId);
identifier_for_python!(component_id::ComponentId);
identifier_for_python!(exec_algorithm_id::ExecAlgorithmId);
identifier_for_python!(order_list_id::OrderListId);
identifier_for_python!(position_id::PositionId);
identifier_for_python!(strategy_id::StrategyId);
identifier_for_python!(symbol::Symbol);
identifier_for_python!(trade_id::TradeId);
identifier_for_python!(trader_id::TraderId);
identifier_for_python!(venue::Venue);
identifier_for_python!(venue_order_id::VenueOrderId);

#[no_mangle]
pub extern "C" fn interned_string_stats() {
    dbg!(ustr::total_allocated());
    dbg!(ustr::total_capacity());

    ustr::string_cache_iter().for_each(|s| println!("{}", s));
}

// #[cfg(test)]
// pub mod stubs {
//     use crate::identifiers::{
//         account_id::stubs::*, client_id::stubs::*, client_order_id::stubs::*,
//         component_id::stubs::*, exec_algorithm_id::stubs::*, instrument_id::stubs::*,
//         order_list_id::stubs::*, position_id::stubs::*, strategy_id::stubs::*, symbol::stubs::*,
//         trade_id::stubs::*, trader_id::stubs::*, venue::stubs::*, venue_order_id::stubs::*,
//     };
// }
