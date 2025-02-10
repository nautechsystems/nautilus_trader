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

//! Identifiers for the trading domain model.

pub mod instrument_id;
pub mod symbol;
pub mod trade_id;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyString, PyTuple},
};

use crate::identifier_for_python;

identifier_for_python!(crate::identifiers::AccountId);
identifier_for_python!(crate::identifiers::ClientId);
identifier_for_python!(crate::identifiers::ClientOrderId);
identifier_for_python!(crate::identifiers::ComponentId);
identifier_for_python!(crate::identifiers::ExecAlgorithmId);
identifier_for_python!(crate::identifiers::OrderListId);
identifier_for_python!(crate::identifiers::PositionId);
identifier_for_python!(crate::identifiers::StrategyId);
identifier_for_python!(crate::identifiers::TraderId);
identifier_for_python!(crate::identifiers::Venue);
identifier_for_python!(crate::identifiers::VenueOrderId);
