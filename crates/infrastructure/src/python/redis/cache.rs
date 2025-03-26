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

use std::collections::HashMap;

use bytes::Bytes;
use nautilus_common::{cache::database::CacheDatabaseAdapter, runtime::get_runtime};
use nautilus_core::{
    UUID4,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId,
    },
    orders::Order,
    position::Position,
    python::{
        account::{account_any_to_pyobject, pyobject_to_account_any},
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::{order_any_to_pyobject, pyobject_to_order_any},
    },
    types::Currency,
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyBytes, PyDict},
};
use ustr::Ustr;

use crate::redis::{cache::RedisCacheDatabase, queries::DatabaseQueries};

#[pymethods]
impl RedisCacheDatabase {
    #[new]
    fn py_new(trader_id: TraderId, instance_id: UUID4, config_json: Vec<u8>) -> PyResult<Self> {
        let config = serde_json::from_slice(&config_json).map_err(to_pyvalue_err)?;
        let result = get_runtime()
            .block_on(async { RedisCacheDatabase::new(trader_id, instance_id, config).await });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close()
    }

    #[pyo3(name = "flushdb")]
    fn py_flushdb(&mut self) {
        get_runtime().block_on(async { self.flushdb().await });
    }

    #[pyo3(name = "keys")]
    fn py_keys(&mut self, pattern: &str) -> PyResult<Vec<String>> {
        let result = get_runtime().block_on(async { self.keys(pattern).await });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_all")]
    fn py_load_all(&mut self) -> PyResult<PyObject> {
        let result = get_runtime().block_on(async {
            DatabaseQueries::load_all(&self.con, self.get_encoding(), self.get_trader_key()).await
        });
        match result {
            Ok(cache_map) => Python::with_gil(|py| {
                let dict = PyDict::new(py);

                // Load currencies
                let currencies_dict = PyDict::new(py);
                for (key, value) in cache_map.currencies {
                    currencies_dict
                        .set_item(key.to_string(), value)
                        .map_err(to_pyvalue_err)?;
                }
                dict.set_item("currencies", currencies_dict)
                    .map_err(to_pyvalue_err)?;

                // Load instruments
                let instruments_dict = PyDict::new(py);
                for (key, value) in cache_map.instruments {
                    let py_object = instrument_any_to_pyobject(py, value)?;
                    instruments_dict
                        .set_item(key, py_object)
                        .map_err(to_pyvalue_err)?;
                }
                dict.set_item("instruments", instruments_dict)
                    .map_err(to_pyvalue_err)?;

                // Load synthetics
                let synthetics_dict = PyDict::new(py);
                for (key, value) in cache_map.synthetics {
                    synthetics_dict
                        .set_item(key, value)
                        .map_err(to_pyvalue_err)?;
                }
                dict.set_item("synthetics", synthetics_dict)
                    .map_err(to_pyvalue_err)?;

                // Load accounts
                let accounts_dict = PyDict::new(py);
                for (key, value) in cache_map.accounts {
                    let py_object = account_any_to_pyobject(py, value)?;
                    accounts_dict
                        .set_item(key, py_object)
                        .map_err(to_pyvalue_err)?;
                }
                dict.set_item("accounts", accounts_dict)
                    .map_err(to_pyvalue_err)?;

                // Load orders
                let orders_dict = PyDict::new(py);
                for (key, value) in cache_map.orders {
                    let py_object = order_any_to_pyobject(py, value)?;
                    orders_dict
                        .set_item(key, py_object)
                        .map_err(to_pyvalue_err)?;
                }
                dict.set_item("orders", orders_dict)
                    .map_err(to_pyvalue_err)?;

                // Load positions
                let positions_dict = PyDict::new(py);
                for (key, value) in cache_map.positions {
                    positions_dict
                        .set_item(key, value)
                        .map_err(to_pyvalue_err)?;
                }
                dict.set_item("positions", positions_dict)
                    .map_err(to_pyvalue_err)?;

                dict.into_py_any(py)
            }),
            Err(e) => Err(to_pyruntime_err(e)),
        }
    }

    #[pyo3(name = "load")]
    fn py_load(&mut self) -> PyResult<HashMap<String, Vec<u8>>> {
        let result: Result<HashMap<String, Vec<u8>>, anyhow::Error> =
            get_runtime().block_on(async {
                let result = self.load().await?;
                Ok(result.into_iter().map(|(k, v)| (k, v.to_vec())).collect())
            });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_currency")]
    fn py_load_currency(&self, code: &str) -> PyResult<Option<Currency>> {
        let result = get_runtime().block_on(async {
            DatabaseQueries::load_currency(
                &self.con,
                self.get_trader_key(),
                &Ustr::from(code),
                self.get_encoding(),
            )
            .await
        });
        result.map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_account")]
    fn py_load_account(&self, py: Python, account_id: AccountId) -> PyResult<Option<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_account(
                &self.con,
                self.get_trader_key(),
                &account_id,
                self.get_encoding(),
            )
            .await;

            match result {
                Ok(Some(account)) => {
                    let py_object = account_any_to_pyobject(py, account)?;
                    Ok(Some(py_object))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(to_pyruntime_err(e)),
            }
        })
    }

    #[pyo3(name = "load_order")]
    fn py_load_order(
        &self,
        py: Python,
        client_order_id: ClientOrderId,
    ) -> PyResult<Option<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_order(
                &self.con,
                self.get_trader_key(),
                &client_order_id,
                self.get_encoding(),
            )
            .await;

            match result {
                Ok(Some(order)) => {
                    let py_object = order_any_to_pyobject(py, order)?;
                    Ok(Some(py_object))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(to_pyruntime_err(e)),
            }
        })
    }

    #[pyo3(name = "load_instrument")]
    fn py_load_instrument(
        &self,
        py: Python,
        instrument_id: InstrumentId,
    ) -> PyResult<Option<PyObject>> {
        get_runtime().block_on(async {
            let result = DatabaseQueries::load_instrument(
                &self.con,
                self.get_trader_key(),
                &instrument_id,
                self.get_encoding(),
            )
            .await;

            match result {
                Ok(Some(instrument)) => {
                    let py_object = instrument_any_to_pyobject(py, instrument)?;
                    Ok(Some(py_object))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(to_pyruntime_err(e)),
            }
        })
    }

    #[pyo3(name = "load_position")]
    fn py_load_position(&self, position_id: PositionId) -> PyResult<Option<Position>> {
        get_runtime()
            .block_on(async {
                DatabaseQueries::load_position(
                    &self.con,
                    self.get_trader_key(),
                    &position_id,
                    self.get_encoding(),
                )
                .await
            })
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "load_strategy")]
    fn py_load_strategy(&self, strategy_id: &str) -> PyResult<HashMap<String, String>> {
        get_runtime().block_on(async {
            DatabaseQueries::load_strategy(
                &self.con,
                self.get_trader_key(),
                &StrategyId::new(strategy_id),
                self.get_encoding(),
            )
            .await
            .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "read")]
    fn py_read(&mut self, py: Python, key: &str) -> PyResult<Vec<PyObject>> {
        let result = get_runtime().block_on(async { self.read(key).await });
        match result {
            Ok(result) => {
                let vec_py_bytes = result
                    .into_iter()
                    .map(|r| PyBytes::new(py, r.as_ref()).into())
                    .collect::<Vec<PyObject>>();
                Ok(vec_py_bytes)
            }
            Err(e) => Err(to_pyruntime_err(e)),
        }
    }

    #[pyo3(name = "insert")]
    fn py_insert(&mut self, key: String, payload: Vec<Vec<u8>>) -> PyResult<()> {
        let payload: Vec<Bytes> = payload.into_iter().map(Bytes::from).collect();
        self.insert(key, Some(payload)).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "update")]
    fn py_update(&mut self, key: String, payload: Vec<Vec<u8>>) -> PyResult<()> {
        let payload: Vec<Bytes> = payload.into_iter().map(Bytes::from).collect();
        self.update(key, Some(payload)).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "update_strategy")]
    fn py_update_strategy(&mut self, strategy: HashMap<String, String>) -> PyResult<()> {
        self.update_strategy(strategy).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "delete")]
    #[pyo3(signature = (key, payload=None))]
    fn py_delete(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> PyResult<()> {
        let payload: Option<Vec<Bytes>> =
            payload.map(|vec| vec.into_iter().map(Bytes::from).collect());
        self.delete(key, payload).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add")]
    fn py_add(&mut self, key: String, value: Vec<u8>) -> PyResult<()> {
        self.add(key, Bytes::from(value)).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_currency")]
    fn py_add_currency(&mut self, currency: Currency) -> PyResult<()> {
        self.add_currency(&currency).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&mut self, py: Python, instrument: PyObject) -> PyResult<()> {
        let instrument_any = pyobject_to_instrument_any(py, instrument)?;
        self.add_instrument(&instrument_any).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_account")]
    fn py_add_account(&mut self, py: Python, account: PyObject) -> PyResult<()> {
        let account_any = pyobject_to_account_any(py, account)?;
        self.add_account(&account_any).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_order")]
    #[pyo3(signature = (order, _position_id=None,client_id=None))]
    fn py_add_order(
        &mut self,
        py: Python,
        order: PyObject,
        _position_id: Option<PositionId>,
        client_id: Option<ClientId>,
    ) -> PyResult<()> {
        let order_any = pyobject_to_order_any(py, order)?;
        self.add_order(&order_any, client_id)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_position")]
    fn py_add_position(&mut self, position: Position) -> PyResult<()> {
        self.add_position(&position).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "update_order")]
    fn py_update_order(&mut self, py: Python, order: PyObject) -> PyResult<()> {
        let order_any = pyobject_to_order_any(py, order)?;
        self.update_order(order_any.last_event())
            .map_err(to_pyvalue_err)
    }
}
