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

use std::{collections::HashMap, path::PathBuf};

use databento::dbn;
use nautilus_core::{ffi::cvec::CVec, python::to_pyvalue_err};
use nautilus_model::{
    data::{
        bar::Bar, delta::OrderBookDelta, depth::OrderBookDepth10, quote::QuoteTick,
        trade::TradeTick, Data,
    },
    identifiers::{instrument_id::InstrumentId, venue::Venue},
    instruments::InstrumentType,
};
use pyo3::{
    prelude::*,
    types::{PyCapsule, PyList},
};

use crate::databento::{
    loader::DatabentoDataLoader,
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, PublisherId},
};

#[pymethods]
impl DatabentoDataLoader {
    #[new]
    fn py_new(path: Option<String>) -> PyResult<Self> {
        Self::new(path.map(PathBuf::from)).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_publishers")]
    fn py_load_publishers(&mut self, path: String) -> PyResult<()> {
        let path_buf = PathBuf::from(path);
        self.load_publishers(path_buf).map_err(to_pyvalue_err)
    }

    #[must_use]
    #[pyo3(name = "get_publishers")]
    fn py_get_publishers(&self) -> HashMap<u16, DatabentoPublisher> {
        self.get_publishers()
            .iter()
            .map(|(&key, value)| (key, value.clone()))
            .collect::<HashMap<u16, DatabentoPublisher>>()
    }

    #[must_use]
    #[pyo3(name = "get_dataset_for_venue")]
    fn py_get_dataset_for_venue(&self, venue: &Venue) -> Option<String> {
        self.get_dataset_for_venue(venue)
            .map(std::string::ToString::to_string)
    }

    #[must_use]
    #[pyo3(name = "get_venue_for_publisher")]
    fn py_get_venue_for_publisher(&self, publisher_id: PublisherId) -> Option<String> {
        self.get_venue_for_publisher(publisher_id)
            .map(std::string::ToString::to_string)
    }

    #[pyo3(name = "schema_for_file")]
    fn py_schema_for_file(&self, path: String) -> PyResult<Option<String>> {
        self.schema_from_file(PathBuf::from(path))
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_instruments")]
    fn py_load_instruments(&mut self, py: Python, path: String) -> PyResult<PyObject> {
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

    /// Cannot include trades
    #[pyo3(name = "load_order_book_deltas")]
    fn py_load_order_book_deltas(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::MboMsg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((Some(item1), _)) => {
                    if let Data::Delta(delta) = item1 {
                        data.push(delta);
                    }
                }
                Ok((None, _)) => continue,
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_order_book_deltas_as_pycapsule")]
    fn py_load_order_book_deltas_as_pycapsule(
        &self,
        py: Python,
        path: String,
        instrument_id: Option<InstrumentId>,
        include_trades: Option<bool>,
    ) -> PyResult<PyObject> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::MboMsg>(path_buf, instrument_id, include_trades.unwrap_or(false))
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_order_book_depth10")]
    fn py_load_order_book_depth10(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<OrderBookDepth10>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::Mbp10Msg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((Some(item1), _)) => {
                    if let Data::Depth10(depth) = item1 {
                        data.push(depth);
                    }
                }
                Ok((None, _)) => continue,
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_order_book_depth10_as_pycapsule")]
    fn py_load_order_book_depth10_as_pycapsule(
        &self,
        py: Python,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<PyObject> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::Mbp10Msg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_quotes")]
    fn py_load_quotes(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
        include_trades: Option<bool>,
    ) -> PyResult<Vec<QuoteTick>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::Mbp1Msg>(path_buf, instrument_id, include_trades.unwrap_or(false))
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((Some(item1), _)) => {
                    if let Data::Quote(quote) = item1 {
                        data.push(quote);
                    }
                }
                Ok((None, _)) => continue,
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_quotes_as_pycapsule")]
    fn py_load_quotes_as_pycapsule(
        &self,
        py: Python,
        path: String,
        instrument_id: Option<InstrumentId>,
        include_trades: Option<bool>,
    ) -> PyResult<PyObject> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::Mbp1Msg>(path_buf, instrument_id, include_trades.unwrap_or(false))
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_tbbo_trades")]
    fn py_load_tbbo_trades(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<TradeTick>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::TbboMsg>(path_buf, instrument_id, false)
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

    #[pyo3(name = "load_tbbo_trades_as_pycapsule")]
    fn py_load_tbbo_trades_as_pycapsule(
        &self,
        py: Python,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<PyObject> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::TbboMsg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_trades")]
    fn py_load_trades(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<TradeTick>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::TradeMsg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((Some(item1), _)) => {
                    if let Data::Trade(trade) = item1 {
                        data.push(trade);
                    }
                }
                Ok((None, _)) => continue,
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_trades_as_pycapsule")]
    fn py_load_trades_as_pycapsule(
        &self,
        py: Python,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<PyObject> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::TradeMsg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_bars")]
    fn py_load_bars(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<Bar>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::OhlcvMsg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok((Some(item1), _)) => {
                    if let Data::Bar(bar) = item1 {
                        data.push(bar);
                    }
                }
                Ok((None, _)) => continue,
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_bars_as_pycapsule")]
    fn py_load_bars_as_pycapsule(
        &self,
        py: Python,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<PyObject> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_records::<dbn::OhlcvMsg>(path_buf, instrument_id, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_imbalance")]
    fn py_load_imbalance(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<DatabentoImbalance>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_imbalance_records::<dbn::ImbalanceMsg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok(item) => data.push(item),
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }

    #[pyo3(name = "load_statistics")]
    fn py_load_statistics(
        &self,
        path: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<DatabentoStatistics>> {
        let path_buf = PathBuf::from(path);
        let iter = self
            .read_statistics_records::<dbn::StatMsg>(path_buf, instrument_id)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for result in iter {
            match result {
                Ok(item) => data.push(item),
                Err(e) => return Err(to_pyvalue_err(e)),
            }
        }

        Ok(data)
    }
}

pub fn convert_instrument_to_pyobject(
    py: Python,
    instrument: InstrumentType,
) -> PyResult<PyObject> {
    match instrument {
        InstrumentType::Equity(inst) => Ok(inst.into_py(py)),
        InstrumentType::FuturesContract(inst) => Ok(inst.into_py(py)),
        InstrumentType::FuturesSpread(inst) => Ok(inst.into_py(py)),
        InstrumentType::OptionsContract(inst) => Ok(inst.into_py(py)),
        InstrumentType::OptionsSpread(inst) => Ok(inst.into_py(py)),
        _ => Err(to_pyvalue_err("Unsupported instrument type")),
    }
}

fn exhaust_data_iter_to_pycapsule(
    py: Python,
    iter: impl Iterator<Item = anyhow::Result<(Option<Data>, Option<Data>)>>,
) -> anyhow::Result<PyObject> {
    let mut data = Vec::new();
    for result in iter {
        match result {
            Ok((Some(item1), None)) => data.push(item1),
            Ok((None, Some(item2))) => data.push(item2),
            Ok((Some(item1), Some(item2))) => {
                data.push(item1);
                data.push(item2);
            }
            Ok((None, None)) => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    let cvec: CVec = data.into();
    let capsule = PyCapsule::new::<CVec>(py, cvec, None)?;

    Ok(capsule.into_py(py))
}
