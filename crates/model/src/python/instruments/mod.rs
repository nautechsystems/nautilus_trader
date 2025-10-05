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

//! Instrument definitions the trading domain model.

use nautilus_core::python::to_pyvalue_err;
use pyo3::{IntoPyObjectExt, Py, PyAny, PyResult, Python};

use crate::instruments::{
    BettingInstrument, BinaryOption, CryptoFuture, CryptoPerpetual, CurrencyPair, Equity,
    FuturesContract, FuturesSpread, InstrumentAny, OptionContract, OptionSpread,
    crypto_option::CryptoOption,
};

pub mod betting;
pub mod binary_option;
pub mod crypto_future;
pub mod crypto_option;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod futures_spread;
pub mod option_contract;
pub mod option_spread;

/// Converts an [`InstrumentAny`] into a Python object.
///
/// # Errors
///
/// Returns a `PyErr` if conversion to a Python object fails.
pub fn instrument_any_to_pyobject(py: Python, instrument: InstrumentAny) -> PyResult<Py<PyAny>> {
    match instrument {
        InstrumentAny::Betting(inst) => inst.into_py_any(py),
        InstrumentAny::BinaryOption(inst) => inst.into_py_any(py),
        InstrumentAny::CryptoFuture(inst) => inst.into_py_any(py),
        InstrumentAny::CryptoOption(inst) => inst.into_py_any(py),
        InstrumentAny::CryptoPerpetual(inst) => inst.into_py_any(py),
        InstrumentAny::CurrencyPair(inst) => inst.into_py_any(py),
        InstrumentAny::Equity(inst) => inst.into_py_any(py),
        InstrumentAny::FuturesContract(inst) => inst.into_py_any(py),
        InstrumentAny::FuturesSpread(inst) => inst.into_py_any(py),
        InstrumentAny::OptionContract(inst) => inst.into_py_any(py),
        InstrumentAny::OptionSpread(inst) => inst.into_py_any(py),
    }
}

/// Converts a Python object into an [`InstrumentAny`] enum.
///
/// # Errors
///
/// Returns a `PyErr` if extraction fails or the instrument type is unsupported.
pub fn pyobject_to_instrument_any(py: Python, instrument: Py<PyAny>) -> PyResult<InstrumentAny> {
    match instrument.getattr(py, "type_str")?.extract::<&str>(py)? {
        stringify!(BettingInstrument) => Ok(InstrumentAny::Betting(
            instrument.extract::<BettingInstrument>(py)?,
        )),
        stringify!(BinaryOption) => Ok(InstrumentAny::BinaryOption(
            instrument.extract::<BinaryOption>(py)?,
        )),
        stringify!(CryptoFuture) => Ok(InstrumentAny::CryptoFuture(
            instrument.extract::<CryptoFuture>(py)?,
        )),
        stringify!(CryptoOption) => Ok(InstrumentAny::CryptoOption(
            instrument.extract::<CryptoOption>(py)?,
        )),
        stringify!(CryptoPerpetual) => Ok(InstrumentAny::CryptoPerpetual(
            instrument.extract::<CryptoPerpetual>(py)?,
        )),
        stringify!(CurrencyPair) => Ok(InstrumentAny::CurrencyPair(
            instrument.extract::<CurrencyPair>(py)?,
        )),
        stringify!(Equity) => Ok(InstrumentAny::Equity(instrument.extract::<Equity>(py)?)),
        stringify!(FuturesContract) => Ok(InstrumentAny::FuturesContract(
            instrument.extract::<FuturesContract>(py)?,
        )),
        stringify!(FuturesSpread) => Ok(InstrumentAny::FuturesSpread(
            instrument.extract::<FuturesSpread>(py)?,
        )),
        stringify!(OptionContract) => Ok(InstrumentAny::OptionContract(
            instrument.extract::<OptionContract>(py)?,
        )),
        stringify!(OptionSpread) => Ok(InstrumentAny::OptionSpread(
            instrument.extract::<OptionSpread>(py)?,
        )),
        _ => Err(to_pyvalue_err(
            "Error in conversion from `Py<PyAny>` to `InstrumentAny`",
        )),
    }
}
