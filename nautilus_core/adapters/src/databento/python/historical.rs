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

use std::{num::NonZeroU64, sync::Arc};

use databento::{self, historical::timeseries::GetRangeParams};
use dbn;
use nautilus_core::python::to_pyvalue_err;
use pyo3::{exceptions::PyException, prelude::*, types::PyDict};
use time::macros::datetime;
use tokio::sync::Mutex;

#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoHistoricalClient {
    inner: Arc<Mutex<databento::HistoricalClient>>,
}

#[pymethods]
impl DatabentoHistoricalClient {
    #[new]
    pub fn py_new(key: String) -> PyResult<Self> {
        let client = databento::HistoricalClient::builder()
            .key(key)
            .map_err(to_pyvalue_err)?
            .build()
            .map_err(to_pyvalue_err)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(client)),
        })
    }

    #[pyo3(name = "get_dataset_range")]
    fn py_get_dataset_range<'py>(&self, py: Python<'py>, dataset: String) -> PyResult<&'py PyAny> {
        let client = self.inner.clone();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = client.lock().await;
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

    #[pyo3(name = "get_range")]
    fn py_get_range(
        &self,
        _py: Python<'_>,
        dataset: String,
        symbols: String,
        schema: dbn::Schema,
        _limit: Option<usize>,
        // ) -> PyResult<&'py PyAny> {
    ) {
        let _client = self.inner.clone();

        let _params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range((
                datetime!(2022-06-06 00:00 UTC),
                datetime!(2022-06-10 12:10 UTC),
            ))
            .symbols(symbols)
            .schema(schema)
            .limit(NonZeroU64::new(1))
            .build();

        // WIP
        // pyo3_asyncio::tokio::future_into_py(py, async move {
        //     let mut client = client.lock().await;
        //     let decoder = client
        //         .timeseries()
        //         .get_range(&params)
        //         .await
        //         .map_err(to_pyvalue_err)?;
        //
        //     let mut result: Vec<&dbn::TradeMsg> = Vec::new();
        //     for rec in decoder
        //         .decode_record::<dbn::TradeMsg>()
        //         .await
        //         .map_err(to_pyvalue_err)?
        //     {
        //         // TODO: Parse event record to a Nautilus data object
        //         result.push(rec);
        //     }
        //
        //     let output = Vec::new();
        //     let py_list = output.into_py(py);
        //     Ok(py_list)
        // });
    }
}
