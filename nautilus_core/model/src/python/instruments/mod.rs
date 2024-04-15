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

use nautilus_core::python::to_pyvalue_err;
use pyo3::{IntoPy, PyObject, PyResult, Python};

use crate::instruments::{
    crypto_future::CryptoFuture, crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair,
    equity::Equity, futures_contract::FuturesContract, futures_spread::FuturesSpread,
    options_contract::OptionsContract, InstrumentAny,
};

pub fn convert_instrument_any_to_pyobject(
    py: Python,
    instrument: InstrumentAny,
) -> PyResult<PyObject> {
    match instrument {
        InstrumentAny::CurrencyPair(inst) => Ok(inst.into_py(py)),
        InstrumentAny::Equity(inst) => Ok(inst.into_py(py)),
        InstrumentAny::FuturesContract(inst) => Ok(inst.into_py(py)),
        InstrumentAny::FuturesSpread(inst) => Ok(inst.into_py(py)),
        InstrumentAny::OptionsContract(inst) => Ok(inst.into_py(py)),
        InstrumentAny::OptionsSpread(inst) => Ok(inst.into_py(py)),
        _ => Err(to_pyvalue_err("Unsupported instrument type")),
    }
}

pub fn convert_pyobject_to_instrument_any(
    py: Python,
    instrument: PyObject,
) -> PyResult<InstrumentAny> {
    let instrument_type = instrument
        .getattr(py, "instrument_type")?
        .extract::<String>(py)?;
    if instrument_type == "CryptoFuture" {
        let crypto_future = instrument.extract::<CryptoFuture>(py)?;
        Ok(InstrumentAny::CryptoFuture(crypto_future))
    } else if instrument_type == "CryptoPerpetual" {
        let crypto_perpetual = instrument.extract::<CryptoPerpetual>(py)?;
        Ok(InstrumentAny::CryptoPerpetual(crypto_perpetual))
    } else if instrument_type == "CurrencyPair" {
        let currency_pair = instrument.extract::<CurrencyPair>(py)?;
        Ok(InstrumentAny::CurrencyPair(currency_pair))
    } else if instrument_type == "Equity" {
        let equity = instrument.extract::<Equity>(py)?;
        Ok(InstrumentAny::Equity(equity))
    } else if instrument_type == "FuturesContract" {
        let futures_contract = instrument.extract::<FuturesContract>(py)?;
        Ok(InstrumentAny::FuturesContract(futures_contract))
    } else if instrument_type == "FuturesSpread" {
        let futures_spread = instrument.extract::<FuturesSpread>(py)?;
        Ok(InstrumentAny::FuturesSpread(futures_spread))
    } else if instrument_type == "OptionsContract" {
        let options_contract = instrument.extract::<OptionsContract>(py)?;
        Ok(InstrumentAny::OptionsContract(options_contract))
    } else if instrument_type == "OptionsSpread" {
        let options_spread = instrument.extract::<CryptoFuture>(py)?;
        Ok(InstrumentAny::CryptoFuture(options_spread))
    } else {
        Err(to_pyvalue_err(
            "Error in conversion from pyobject to instrument type",
        ))
    }
}

pub mod crypto_future;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod futures_spread;
pub mod options_contract;
pub mod options_spread;
