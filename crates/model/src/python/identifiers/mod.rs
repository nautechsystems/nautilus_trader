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

//! Identifiers for the trading domain model.

pub mod instrument_id;
pub mod option_series_id;
pub mod symbol;
pub mod trade_id;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyString, PyTuple},
};

use crate::{
    identifier_for_python,
    identifiers::{InstrumentId, new_generic_spread_id, parse_generic_spread_id_legs},
};

identifier_for_python!(crate::identifiers::ActorId);
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

/// Creates a generic spread instrument ID from `(instrument_id, ratio)` legs.
///
/// Sorts the legs by symbol and joins them with `GENERIC_SPREAD_ID_SEPARATOR`, formatting a
/// positive ratio as `(ratio)symbol` and a negative ratio as `((ratio))symbol`. For example,
/// `MSFT.NASDAQ` with ratio 1 and `AAPL.NASDAQ` with ratio -2 produce
/// `((2))AAPL___(1)MSFT.NASDAQ`.
///
/// A leg symbol that contains the separator or ends with `_` produces an ID that does not parse
/// back into the same legs.
///
/// # Errors
///
/// Returns an error if:
/// - Fewer than two legs are given.
/// - A ratio is zero or `i64::MIN`.
/// - The legs have different venues.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "new_generic_spread_id")]
#[expect(clippy::needless_pass_by_value)]
pub fn py_new_generic_spread_id(
    instrument_ratios: Vec<(InstrumentId, i64)>,
) -> PyResult<InstrumentId> {
    new_generic_spread_id(&instrument_ratios).map_err(to_pyvalue_err)
}

/// Parses a generic spread instrument ID into `(instrument_id, ratio)` legs.
///
/// Returns the legs in the order they appear in the symbol, with a negative ratio for each
/// `((ratio))symbol` leg.
///
/// # Errors
///
/// Returns an error if `instrument_id` is not in the format `new_generic_spread_id` produces.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "generic_spread_id_to_list")]
pub fn py_generic_spread_id_to_list(
    instrument_id: InstrumentId,
) -> PyResult<Vec<(InstrumentId, i64)>> {
    parse_generic_spread_id_legs(&instrument_id).map_err(to_pyvalue_err)
}
