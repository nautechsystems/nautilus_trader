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

use std::{any::Any, path::PathBuf};

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{
        bar::Bar, delta::OrderBookDelta, depth::OrderBookDepth10, quote::QuoteTick,
        trade::TradeTick, Data,
    },
    identifiers::instrument_id::InstrumentId,
    instruments::{
        equity::Equity, futures_contract::FuturesContract, options_contract::OptionsContract,
        Instrument,
    },
};
use pyo3::{prelude::*, types::PyList};

use crate::databento::loader::DatabentoDataLoader;

#[pymethods]
impl DatabentoDataLoader {
    #[new]
    pub fn py_new(path: Option<String>) -> PyResult<Self> {
        Self::new(path.map(PathBuf::from)).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "schema_for_file")]
    pub fn py_schema_for_file(&self, path: String) -> PyResult<Option<dbn::Schema>> {
        self.schema_from_file(PathBuf::from(path))
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_instruments")]
    pub fn py_load_instruments(&self, py: Python, path: String) -> PyResult<PyObject> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_definition_records(path_buf)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok(instrument) => {
                    let py_object = convert_instrument_to_pyobject(py, instrument)?;
                    data.push(py_object);
                }
                Err(e) => {
                    eprintln!("{e}");
                }
            }
        }

        Ok(PyList::new(py, &data).into())
    }

    #[pyo3(name = "load_order_book_deltas")]
    pub fn py_load_order_book_deltas(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::MboMsg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((item1, _)) => {
                    if let Data::Delta(delta) = item1 {
                        data.push(delta);
                    }
                }
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_order_book_depth10")]
    pub fn py_load_order_book_depth10(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<OrderBookDepth10>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::Mbp10Msg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((item1, _)) => {
                    if let Data::Depth10(depth) = item1 {
                        data.push(depth);
                    }
                }
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_quote_ticks")]
    pub fn py_load_quote_ticks(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<QuoteTick>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::Mbp1Msg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((item1, _)) => {
                    if let Data::Quote(quote) = item1 {
                        data.push(quote);
                    }
                }
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_tbbo_trade_ticks")]
    pub fn py_load_tbbo_trade_ticks(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<TradeTick>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::Mbp1Msg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((_, maybe_item2)) => {
                    if let Some(Data::Trade(trade)) = maybe_item2 {
                        data.push(trade);
                    }
                }
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_trade_ticks")]
    pub fn py_load_trade_ticks(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<TradeTick>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::TradeMsg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((item1, _)) => {
                    if let Data::Trade(trade) = item1 {
                        data.push(trade);
                    }
                }
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_bars")]
    pub fn py_load_bars(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<Bar>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::OhlcvMsg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((item1, _)) => {
                    if let Data::Bar(bar) = item1 {
                        data.push(bar);
                    }
                }
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }
}

fn convert_instrument_to_pyobject(
    py: Python,
    instrument: Box<dyn Instrument + 'static>,
) -> PyResult<PyObject> {
    let any_ref: &dyn Any = instrument.as_any();
    if let Some(equity) = any_ref.downcast_ref::<Equity>() {
        return Ok(equity.into_py(py));
    }
    if let Some(future) = any_ref.downcast_ref::<FuturesContract>() {
        return Ok(future.into_py(py));
    }
    if let Some(option) = any_ref.downcast_ref::<OptionsContract>() {
        return Ok(option.into_py(py));
    }

    Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
        "Unknown instrument type",
    ))
}
