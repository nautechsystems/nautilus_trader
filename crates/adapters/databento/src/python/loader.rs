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

use std::{collections::HashMap, path::PathBuf};

use databento::dbn;
use nautilus_core::{
    ffi::cvec::CVec,
    python::{IntoPyObjectNautilusExt, to_pyvalue_err},
};
use nautilus_model::{
    data::{Bar, Data, InstrumentStatus, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick},
    identifiers::{InstrumentId, Venue},
    python::instruments::instrument_any_to_pyobject,
};
use pyo3::{
    prelude::*,
    types::{PyCapsule, PyList},
};
use ustr::Ustr;

use crate::{
    loader::DatabentoDataLoader,
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, PublisherId},
};

#[pymethods]
impl DatabentoDataLoader {
    #[new]
    #[pyo3(signature = (publishers_filepath=None))]
    fn py_new(publishers_filepath: Option<PathBuf>) -> PyResult<Self> {
        Self::new(publishers_filepath).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_publishers")]
    fn py_load_publishers(&mut self, publishers_filepath: PathBuf) -> PyResult<()> {
        self.load_publishers(publishers_filepath)
            .map_err(to_pyvalue_err)
    }

    #[must_use]
    #[pyo3(name = "get_publishers")]
    fn py_get_publishers(&self) -> HashMap<u16, DatabentoPublisher> {
        self.get_publishers()
            .iter()
            .map(|(&key, value)| (key, value.clone()))
            .collect::<HashMap<u16, DatabentoPublisher>>()
    }

    #[pyo3(name = "set_dataset_for_venue")]
    fn py_set_dataset_for_venue(&mut self, dataset: String, venue: Venue) {
        self.set_dataset_for_venue(Ustr::from(&dataset), venue);
    }

    #[must_use]
    #[pyo3(name = "get_dataset_for_venue")]
    fn py_get_dataset_for_venue(&self, venue: &Venue) -> Option<String> {
        self.get_dataset_for_venue(venue).map(ToString::to_string)
    }

    #[must_use]
    #[pyo3(name = "get_venue_for_publisher")]
    fn py_get_venue_for_publisher(&self, publisher_id: PublisherId) -> Option<String> {
        self.get_venue_for_publisher(publisher_id)
            .map(ToString::to_string)
    }

    #[pyo3(name = "schema_for_file")]
    fn py_schema_for_file(&self, filepath: PathBuf) -> PyResult<Option<String>> {
        self.schema_from_file(&filepath).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_instruments")]
    fn py_load_instruments(
        &mut self,
        py: Python,
        filepath: PathBuf,
        use_exchange_as_venue: bool,
    ) -> PyResult<PyObject> {
        let iter = self
            .load_instruments(&filepath, use_exchange_as_venue)
            .map_err(to_pyvalue_err)?;

        let mut data = Vec::new();
        for instrument in iter {
            let py_object = instrument_any_to_pyobject(py, instrument)?;
            data.push(py_object);
        }

        Ok(PyList::new(py, &data)
            .expect("Invalid `ExactSizeIterator`")
            .into())
    }

    // Cannot include trades
    #[pyo3(name = "load_order_book_deltas")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_order_book_deltas(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        self.load_order_book_deltas(&filepath, instrument_id, price_precision)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_order_book_deltas_as_pycapsule")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None, include_trades=None))]
    fn py_load_order_book_deltas_as_pycapsule(
        &self,
        py: Python,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
        include_trades: Option<bool>,
    ) -> PyResult<PyObject> {
        let iter = self
            .read_records::<dbn::MboMsg>(
                &filepath,
                instrument_id,
                price_precision,
                include_trades.unwrap_or(false),
            )
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_order_book_depth10")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_order_book_depth10(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<OrderBookDepth10>> {
        self.load_order_book_depth10(&filepath, instrument_id, price_precision)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_order_book_depth10_as_pycapsule")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_order_book_depth10_as_pycapsule(
        &self,
        py: Python,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<PyObject> {
        let iter = self
            .read_records::<dbn::Mbp10Msg>(&filepath, instrument_id, price_precision, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_quotes")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_quotes(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<QuoteTick>> {
        self.load_quotes(&filepath, instrument_id, price_precision)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_quotes_as_pycapsule")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None, include_trades=None))]
    fn py_load_quotes_as_pycapsule(
        &self,
        py: Python,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
        include_trades: Option<bool>,
    ) -> PyResult<PyObject> {
        let iter = self
            .read_records::<dbn::Mbp1Msg>(
                &filepath,
                instrument_id,
                price_precision,
                include_trades.unwrap_or(false),
            )
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_bbo_quotes")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_bbo_quotes(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<QuoteTick>> {
        self.load_bbo_quotes(&filepath, instrument_id, price_precision)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_bbo_quotes_as_pycapsule")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_bbo_quotes_as_pycapsule(
        &self,
        py: Python,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<PyObject> {
        let iter = self
            .read_records::<dbn::BboMsg>(&filepath, instrument_id, price_precision, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_tbbo_trades")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_tbbo_trades(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<TradeTick>> {
        self.load_tbbo_trades(&filepath, instrument_id, price_precision)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_tbbo_trades_as_pycapsule")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_tbbo_trades_as_pycapsule(
        &self,
        py: Python,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<PyObject> {
        let iter = self
            .read_records::<dbn::TbboMsg>(&filepath, instrument_id, price_precision, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_trades")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_trades(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<TradeTick>> {
        self.load_trades(&filepath, instrument_id, price_precision)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_trades_as_pycapsule")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_trades_as_pycapsule(
        &self,
        py: Python,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<PyObject> {
        let iter = self
            .read_records::<dbn::TradeMsg>(&filepath, instrument_id, price_precision, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_bars")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_bars(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<Bar>> {
        self.load_bars(&filepath, instrument_id, price_precision)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_bars_as_pycapsule")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_bars_as_pycapsule(
        &self,
        py: Python,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<PyObject> {
        let iter = self
            .read_records::<dbn::OhlcvMsg>(&filepath, instrument_id, price_precision, false)
            .map_err(to_pyvalue_err)?;

        exhaust_data_iter_to_pycapsule(py, iter).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "load_status")]
    #[pyo3(signature = (filepath, instrument_id=None))]
    fn py_load_status(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<InstrumentStatus>> {
        let iter = self
            .load_status_records::<dbn::StatusMsg>(&filepath, instrument_id)
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

    #[pyo3(name = "load_imbalance")]
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_imbalance(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<DatabentoImbalance>> {
        let iter = self
            .read_imbalance_records::<dbn::ImbalanceMsg>(&filepath, instrument_id, price_precision)
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
    #[pyo3(signature = (filepath, instrument_id=None, price_precision=None))]
    fn py_load_statistics(
        &self,
        filepath: PathBuf,
        instrument_id: Option<InstrumentId>,
        price_precision: Option<u8>,
    ) -> PyResult<Vec<DatabentoStatistics>> {
        let iter = self
            .read_statistics_records::<dbn::StatMsg>(&filepath, instrument_id, price_precision)
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

    // TODO: Improve error domain. Replace anyhow errors with nautilus
    // errors to unify pyo3 and anyhow errors.
    Ok(capsule.into_py_any_unwrap(py))
}
