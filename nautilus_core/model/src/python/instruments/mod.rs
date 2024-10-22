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

//! Instrument definitions the trading domain model.

use nautilus_core::python::to_pyvalue_err;
use pyo3::{IntoPy, PyObject, PyResult, Python};

use crate::instruments::{
    any::InstrumentAny, betting::BettingInstrument, binary_option::BinaryOption,
    crypto_future::CryptoFuture, crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair,
    equity::Equity, futures_contract::FuturesContract, futures_spread::FuturesSpread,
    options_contract::OptionsContract, options_spread::OptionsSpread,
};

pub mod betting;
pub mod binary_option;
pub mod crypto_future;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod futures_spread;
pub mod options_contract;
pub mod options_spread;

pub fn instrument_any_to_pyobject(py: Python, instrument: InstrumentAny) -> PyResult<PyObject> {
    match instrument {
        InstrumentAny::Betting(inst) => Ok(inst.into_py(py)),
        InstrumentAny::BinaryOption(inst) => Ok(inst.into_py(py)),
        InstrumentAny::CryptoFuture(inst) => Ok(inst.into_py(py)),
        InstrumentAny::CryptoPerpetual(inst) => Ok(inst.into_py(py)),
        InstrumentAny::CurrencyPair(inst) => Ok(inst.into_py(py)),
        InstrumentAny::Equity(inst) => Ok(inst.into_py(py)),
        InstrumentAny::FuturesContract(inst) => Ok(inst.into_py(py)),
        InstrumentAny::FuturesSpread(inst) => Ok(inst.into_py(py)),
        InstrumentAny::OptionsContract(inst) => Ok(inst.into_py(py)),
        InstrumentAny::OptionsSpread(inst) => Ok(inst.into_py(py)),
    }
}

pub fn pyobject_to_instrument_any(py: Python, instrument: PyObject) -> PyResult<InstrumentAny> {
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
        stringify!(OptionsContract) => Ok(InstrumentAny::OptionsContract(
            instrument.extract::<OptionsContract>(py)?,
        )),
        stringify!(OptionsSpread) => Ok(InstrumentAny::OptionsSpread(
            instrument.extract::<OptionsSpread>(py)?,
        )),
        _ => Err(to_pyvalue_err(
            "Error in conversion from `PyObject` to `InstrumentAny`",
        )),
    }
}
