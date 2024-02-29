// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
use ustr::Ustr;

use crate::identifier_for_python;

pub mod instrument_id;
pub mod trade_id;

identifier_for_python!(crate::identifiers::account_id::AccountId);
identifier_for_python!(crate::identifiers::client_id::ClientId);
identifier_for_python!(crate::identifiers::client_order_id::ClientOrderId);
identifier_for_python!(crate::identifiers::component_id::ComponentId);
identifier_for_python!(crate::identifiers::exec_algorithm_id::ExecAlgorithmId);
identifier_for_python!(crate::identifiers::order_list_id::OrderListId);
identifier_for_python!(crate::identifiers::position_id::PositionId);
identifier_for_python!(crate::identifiers::strategy_id::StrategyId);
identifier_for_python!(crate::identifiers::symbol::Symbol);
identifier_for_python!(crate::identifiers::trader_id::TraderId);
identifier_for_python!(crate::identifiers::venue::Venue);
identifier_for_python!(crate::identifiers::venue_order_id::VenueOrderId);
