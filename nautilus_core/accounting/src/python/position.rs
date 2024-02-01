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

use crate::position::Position;
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::events::order::filled::OrderFilled;
use nautilus_model::instruments::crypto_future::CryptoFuture;
use nautilus_model::instruments::currency_pair::CurrencyPair;
use nautilus_model::instruments::equity::Equity;
use nautilus_model::instruments::futures_contract::FuturesContract;
use nautilus_model::instruments::options_contract::OptionsContract;
use pyo3::prelude::*;

#[pymethods]
impl Position {
    #[new]
    fn py_new(instrument: PyObject, fill: OrderFilled, py: Python) -> PyResult<Self> {
        // extract instrument from PyObject
        let instrument_type = instrument
            .getattr(py, "instrument_type")?
            .extract::<String>(py)?;
        if instrument_type == "CryptoFuture" {
            let instrument_rust = instrument.extract::<CryptoFuture>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "CurrencyPair" {
            let instrument_rust = instrument.extract::<CurrencyPair>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "Equity" {
            let instrument_rust = instrument.extract::<Equity>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "FuturesContract" {
            let instrument_rust = instrument.extract::<FuturesContract>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else if instrument_type == "OptionsContract" {
            let instrument_rust = instrument.extract::<OptionsContract>(py)?;
            Ok(Self::new(instrument_rust, fill).unwrap())
        } else {
            // throw error unsupported instrument
            Err(to_pyvalue_err("Unsupported instrument type"))
        }
    }
}
