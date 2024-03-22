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

use std::{fs, num::NonZeroU64, str::FromStr, sync::Arc};

use databento::{
    dbn::{self, SType},
    historical::timeseries::GetRangeParams,
};
use indexmap::IndexMap;
use nautilus_core::{
    python::to_pyvalue_err,
    time::{get_atomic_clock_realtime, AtomicTime, UnixNanos},
};
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick, Data},
    enums::BarAggregation,
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    types::currency::Currency,
};
use pyo3::{
    exceptions::PyException,
    prelude::*,
    types::{PyDict, PyList},
};
use tokio::sync::Mutex;

use super::loader::convert_instrument_to_pyobject;
use crate::databento::{
    common::get_date_time_range,
    decode::{
        decode_imbalance_msg, decode_instrument_def_msg, decode_record, decode_statistics_msg,
        raw_ptr_to_ustr,
    },
    symbology::{check_consistent_symbology, decode_nautilus_instrument_id, infer_symbology_type},
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, PublisherId},
};

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoHistoricalClient {
    #[pyo3(get)]
    pub key: String,
    clock: &'static AtomicTime,
    inner: Arc<Mutex<databento::HistoricalClient>>,
    publisher_venue_map: Arc<IndexMap<PublisherId, Venue>>,
}

#[pymethods]
impl DatabentoHistoricalClient {
    #[new]
    fn py_new(key: String, publishers_path: &str) -> PyResult<Self> {
        let client = databento::HistoricalClient::builder()
            .key(key.clone())
            .map_err(to_pyvalue_err)?
            .build()
            .map_err(to_pyvalue_err)?;

        let file_content = fs::read_to_string(publishers_path)?;
        let publishers_vec: Vec<DatabentoPublisher> =
            serde_json::from_str(&file_content).map_err(to_pyvalue_err)?;

        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        Ok(Self {
            clock: get_atomic_clock_realtime(),
            inner: Arc::new(Mutex::new(client)),
            publisher_venue_map: Arc::new(publisher_venue_map),
            key,
        })
    }

    #[pyo3(name = "get_dataset_range")]
    fn py_get_dataset_range<'py>(&self, py: Python<'py>, dataset: String) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let response = client.metadata().get_dataset_range(&dataset).await;
            match response {
                Ok(res) => Python::with_gil(|py| {
                    let dict = PyDict::new(py);
                    dict.set_item("start_date", res.start_date.to_string())?;
                    dict.set_item("end_date", res.end_date.to_string())?;
                    Ok(dict.to_object(py))
                }),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling response: {e}"
                ))),
            }
        })
    }

    #[pyo3(name = "get_range_instruments")]
    fn py_get_range_instruments<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        symbols: Vec<&str>,
        start: UnixNanos,
        end: Option<UnixNanos>,
        limit: Option<u64>,
    ) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns());
        let time_range = get_date_time_range(start, end).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
            .schema(dbn::Schema::Definition)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let publisher_venue_map = self.publisher_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            decoder.set_upgrade_policy(dbn::VersionUpgradePolicy::Upgrade);

            let mut instruments = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::InstrumentDefMsg>().await {
                let raw_symbol = unsafe { raw_ptr_to_ustr(msg.raw_symbol.as_ptr()).unwrap() };
                let symbol = Symbol { value: raw_symbol };

                let publisher = msg.hd.publisher().expect("Invalid `publisher` for record");
                let venue = publisher_venue_map
                    .get(&msg.hd.publisher_id)
                    .unwrap_or_else(|| panic!("`Venue` not found for `publisher` {publisher}"));
                let instrument_id = InstrumentId::new(symbol, *venue);

                let result = decode_instrument_def_msg(msg, instrument_id, ts_init);
                match result {
                    Ok(instrument) => instruments.push(instrument),
                    Err(e) => eprintln!("{e:?}"),
                };
            }

            Python::with_gil(|py| {
                let py_results: PyResult<Vec<PyObject>> = instruments
                    .into_iter()
                    .map(|result| convert_instrument_to_pyobject(py, result))
                    .collect();

                py_results.map(|objs| PyList::new(py, &objs).to_object(py))
            })
        })
    }

    #[pyo3(name = "get_range_quotes")]
    fn py_get_range_quotes<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        symbols: Vec<&str>,
        start: UnixNanos,
        end: Option<UnixNanos>,
        limit: Option<u64>,
    ) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns());
        let time_range = get_date_time_range(start, end).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
            .schema(dbn::Schema::Mbp1)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = Currency::USD().precision; // TODO: Hard coded for now
        let publisher_venue_map = self.publisher_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut result: Vec<QuoteTick> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::Mbp1Msg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id =
                    decode_nautilus_instrument_id(&record, &metadata, &publisher_venue_map)
                        .map_err(to_pyvalue_err)?;

                let (data, _) = decode_record(
                    &record,
                    instrument_id,
                    price_precision,
                    Some(ts_init),
                    false, // Don't include trades
                )
                .map_err(to_pyvalue_err)?;

                match data {
                    Some(Data::Quote(quote)) => {
                        result.push(quote);
                    }
                    _ => panic!("Invalid data element not `QuoteTick`, was {data:?}"),
                }
            }

            Python::with_gil(|py| Ok(result.into_py(py)))
        })
    }

    #[pyo3(name = "get_range_trades")]
    fn py_get_range_trades<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        symbols: Vec<&str>,
        start: UnixNanos,
        end: Option<UnixNanos>,
        limit: Option<u64>,
    ) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns());
        let time_range = get_date_time_range(start, end).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
            .schema(dbn::Schema::Trades)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = Currency::USD().precision; // TODO: Hard coded for now
        let publisher_venue_map = self.publisher_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut result: Vec<TradeTick> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::TradeMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id =
                    decode_nautilus_instrument_id(&record, &metadata, &publisher_venue_map)
                        .map_err(to_pyvalue_err)?;

                let (data, _) = decode_record(
                    &record,
                    instrument_id,
                    price_precision,
                    Some(ts_init),
                    false, // Not applicable (trade will be decoded regardless)
                )
                .map_err(to_pyvalue_err)?;

                match data {
                    Some(Data::Trade(trade)) => {
                        result.push(trade);
                    }
                    _ => panic!("Invalid data element not `TradeTick`, was {data:?}"),
                }
            }

            Python::with_gil(|py| Ok(result.into_py(py)))
        })
    }

    #[pyo3(name = "get_range_bars")]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_bars<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        symbols: Vec<&str>,
        aggregation: BarAggregation,
        start: UnixNanos,
        end: Option<UnixNanos>,
        limit: Option<u64>,
    ) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let schema = match aggregation {
            BarAggregation::Second => dbn::Schema::Ohlcv1S,
            BarAggregation::Minute => dbn::Schema::Ohlcv1M,
            BarAggregation::Hour => dbn::Schema::Ohlcv1H,
            BarAggregation::Day => dbn::Schema::Ohlcv1D,
            _ => panic!("Invalid `BarAggregation` for request, was {aggregation}"),
        };
        let end = end.unwrap_or(self.clock.get_time_ns());
        let time_range = get_date_time_range(start, end).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
            .schema(schema)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = Currency::USD().precision; // TODO: Hard coded for now
        let publisher_venue_map = self.publisher_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut result: Vec<Bar> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::OhlcvMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id =
                    decode_nautilus_instrument_id(&record, &metadata, &publisher_venue_map)
                        .map_err(to_pyvalue_err)?;

                let (data, _) = decode_record(
                    &record,
                    instrument_id,
                    price_precision,
                    Some(ts_init),
                    false, // Not applicable
                )
                .map_err(to_pyvalue_err)?;

                match data {
                    Some(Data::Bar(bar)) => {
                        result.push(bar);
                    }
                    _ => panic!("Invalid data element not `Bar`, was {data:?}"),
                }
            }

            Python::with_gil(|py| Ok(result.into_py(py)))
        })
    }

    #[pyo3(name = "get_range_imbalance")]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_imbalance<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        symbols: Vec<&str>,
        start: UnixNanos,
        end: Option<UnixNanos>,
        limit: Option<u64>,
    ) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns());
        let time_range = get_date_time_range(start, end).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
            .schema(dbn::Schema::Imbalance)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = Currency::USD().precision; // TODO: Hard coded for now
        let publisher_venue_map = self.publisher_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut result: Vec<DatabentoImbalance> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::ImbalanceMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id =
                    decode_nautilus_instrument_id(&record, &metadata, &publisher_venue_map)
                        .map_err(to_pyvalue_err)?;

                let imbalance = decode_imbalance_msg(msg, instrument_id, price_precision, ts_init)
                    .map_err(to_pyvalue_err)?;

                result.push(imbalance);
            }

            Python::with_gil(|py| Ok(result.into_py(py)))
        })
    }

    #[pyo3(name = "get_range_statistics")]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_statistics<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        symbols: Vec<&str>,
        start: UnixNanos,
        end: Option<UnixNanos>,
        limit: Option<u64>,
    ) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns());
        let time_range = get_date_time_range(start, end).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
            .schema(dbn::Schema::Statistics)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = Currency::USD().precision; // TODO: Hard coded for now
        let publisher_venue_map = self.publisher_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut result: Vec<DatabentoStatistics> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::StatMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id =
                    decode_nautilus_instrument_id(&record, &metadata, &publisher_venue_map)
                        .map_err(to_pyvalue_err)?;

                let statistics =
                    decode_statistics_msg(msg, instrument_id, price_precision, ts_init)
                        .map_err(to_pyvalue_err)?;

                result.push(statistics);
            }

            Python::with_gil(|py| Ok(result.into_py(py)))
        })
    }
}
