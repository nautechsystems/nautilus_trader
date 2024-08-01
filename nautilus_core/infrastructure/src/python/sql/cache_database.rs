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

use std::collections::HashMap;

use bytes::Bytes;
use nautilus_common::{cache::database::CacheDatabaseAdapter, runtime::get_runtime};
use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    python::{
        account::{convert_account_any_to_pyobject, convert_pyobject_to_account_any},
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::{convert_order_any_to_pyobject, convert_pyobject_to_order_any},
    },
    types::currency::Currency,
};
use pyo3::prelude::*;

use crate::sql::{
    cache_database::PostgresCacheDatabase, pg::delete_nautilus_postgres_tables,
    queries::DatabaseQueries,
};

#[pymethods]
impl PostgresCacheDatabase {
    #[staticmethod]
    #[pyo3(name = "connect")]
    fn py_connect(
        host: Option<String>,
        port: Option<u16>,
        username: Option<String>,
        password: Option<String>,
        database: Option<String>,
    ) -> PyResult<Self> {
        let result = get_runtime().block_on(async {
            PostgresCacheDatabase::connect(host, port, username, password, database).await
        });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load")]
    fn py_load(slf: PyRef<'_, Self>) -> PyResult<HashMap<String, Vec<u8>>> {
        let result = get_runtime().block_on(async { slf.load().await });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_currency")]
    fn py_load_currency(slf: PyRef<'_, Self>, code: &str) -> PyResult<Option<Currency>> {
        let result =
            get_runtime().block_on(async { DatabaseQueries::load_currency(&slf.pool, code).await });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_currencies")]
    fn py_load_currencies(slf: PyRef<'_, Self>) -> PyResult<Vec<Currency>> {
        let result =
            get_runtime().block_on(async { DatabaseQueries::load_currencies(&slf.pool).await });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "add")]
    fn py_add(mut slf: PyRefMut<'_, Self>, key: String, value: Vec<u8>) -> PyResult<()> {
        slf.add(key, Bytes::from(value)).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "add_currency")]
    fn py_add_currency(mut slf: PyRefMut<'_, Self>, currency: Currency) -> PyResult<()> {
        slf.add_currency(&currency).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_instrument")]
    fn py_load_instrument(
        slf: PyRef<'_, Self>,
        instrument_id: InstrumentId,
        py: Python<'_>,
    ) -> PyResult<Option<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_instrument(&slf.pool, &instrument_id)
                .await
                .unwrap();
            match result {
                Some(instrument) => {
                    let py_object = instrument_any_to_pyobject(py, instrument)?;
                    Ok(Some(py_object))
                }
                None => Ok(None),
            }
        })
    }

    #[pyo3(name = "load_instruments")]
    fn py_load_instruments(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Vec<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_instruments(&slf.pool).await.unwrap();
            let mut instruments = Vec::new();
            for instrument in result {
                let py_object = instrument_any_to_pyobject(py, instrument)?;
                instruments.push(py_object);
            }
            Ok(instruments)
        })
    }

    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(
        mut slf: PyRefMut<'_, Self>,
        instrument: PyObject,
        py: Python<'_>,
    ) -> PyResult<()> {
        let instrument_any = pyobject_to_instrument_any(py, instrument)?;
        slf.add_instrument(&instrument_any)
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "add_order")]
    fn py_add_order(mut slf: PyRefMut<'_, Self>, order: PyObject, py: Python<'_>) -> PyResult<()> {
        let order_any = convert_pyobject_to_order_any(py, order)?;
        slf.add_order(&order_any).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "update_order")]
    fn py_update_order(
        mut slf: PyRefMut<'_, Self>,
        order: PyObject,
        py: Python<'_>,
    ) -> PyResult<()> {
        let order_any = convert_pyobject_to_order_any(py, order)?;
        slf.update_order(&order_any).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_order")]
    fn py_load_order(
        slf: PyRef<'_, Self>,
        order_id: ClientOrderId,
        py: Python<'_>,
    ) -> PyResult<Option<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_order(&slf.pool, &order_id)
                .await
                .unwrap();
            match result {
                Some(order) => {
                    let py_object = convert_order_any_to_pyobject(py, order)?;
                    Ok(Some(py_object))
                }
                None => Ok(None),
            }
        })
    }

    #[pyo3(name = "add_account")]
    fn py_add_account(
        mut slf: PyRefMut<'_, Self>,
        account: PyObject,
        py: Python<'_>,
    ) -> PyResult<()> {
        let account_any = convert_pyobject_to_account_any(py, account)?;
        slf.add_account(&account_any).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_account")]
    fn py_load_account(
        slf: PyRef<'_, Self>,
        account_id: AccountId,
        py: Python<'_>,
    ) -> PyResult<Option<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_account(&slf.pool, &account_id)
                .await
                .unwrap();
            match result {
                Some(account) => {
                    let py_object = convert_account_any_to_pyobject(py, account)?;
                    Ok(Some(py_object))
                }
                None => Ok(None),
            }
        })
    }

    #[pyo3(name = "update_account")]
    fn py_update_account(
        mut slf: PyRefMut<'_, Self>,
        order: PyObject,
        py: Python<'_>,
    ) -> PyResult<()> {
        let order_any = convert_pyobject_to_account_any(py, order)?;
        slf.update_account(&order_any).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "add_trade")]
    fn py_add_trade(mut slf: PyRefMut<'_, Self>, trade: PyObject, py: Python<'_>) -> PyResult<()> {
        let trade = trade.extract::<TradeTick>(py)?;
        slf.add_trade(&trade).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_trades")]
    fn py_load_trades(
        slf: PyRef<'_, Self>,
        instrument_id: InstrumentId,
        py: Python<'_>,
    ) -> PyResult<Vec<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_trades(&slf.pool, &instrument_id)
                .await
                .unwrap();
            let mut trades = Vec::new();
            for trade in result {
                let py_object = trade.into_py(py);
                trades.push(py_object);
            }
            Ok(trades)
        })
    }

    #[pyo3(name = "add_quote")]
    fn py_add_quote(mut slf: PyRefMut<'_, Self>, quote: PyObject, py: Python<'_>) -> PyResult<()> {
        let quote = quote.extract::<QuoteTick>(py)?;
        slf.add_quote(&quote).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_quotes")]
    fn py_load_quotes(
        slf: PyRef<'_, Self>,
        instrument_id: InstrumentId,
        py: Python<'_>,
    ) -> PyResult<Vec<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_quotes(&slf.pool, &instrument_id)
                .await
                .unwrap();
            let mut quotes = Vec::new();
            for quote in result {
                let py_object = quote.into_py(py);
                quotes.push(py_object);
            }
            Ok(quotes)
        })
    }

    #[pyo3(name = "add_bar")]
    fn py_add_bar(mut slf: PyRefMut<'_, Self>, bar: PyObject, py: Python<'_>) -> PyResult<()> {
        let bar = bar.extract::<Bar>(py)?;
        slf.add_bar(&bar).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_bars")]
    fn py_load_bars(
        slf: PyRef<'_, Self>,
        instrument_id: InstrumentId,
        py: Python<'_>,
    ) -> PyResult<Vec<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_bars(&slf.pool, &instrument_id)
                .await
                .unwrap();
            let mut bars = Vec::new();
            for bar in result {
                let py_object = bar.into_py(py);
                bars.push(py_object);
            }
            Ok(bars)
        })
    }

    #[pyo3(name = "flush_db")]
    fn py_drop_schema(slf: PyRef<'_, Self>) -> PyResult<()> {
        let result =
            get_runtime().block_on(async { delete_nautilus_postgres_tables(&slf.pool).await });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "truncate")]
    fn py_truncate(slf: PyRef<'_, Self>, table: String) -> PyResult<()> {
        let result =
            get_runtime().block_on(async { DatabaseQueries::truncate(&slf.pool, table).await });
        result.map_err(to_pyruntime_err)
    }
}
