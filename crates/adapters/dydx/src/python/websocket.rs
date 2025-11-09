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

//! Python bindings for the dYdX WebSocket client.

use nautilus_model::identifiers::{AccountId, InstrumentId};
use pyo3::prelude::*;

use crate::{common::credential::DydxCredential, websocket::client::DydxWebSocketClient};
#[pymethods]
impl DydxWebSocketClient {
    /// Creates a new public WebSocket client for market data.
    #[staticmethod]
    #[pyo3(name = "new_public")]
    fn py_new_public(url: String, heartbeat: Option<u64>) -> Self {
        Self::new_public(url, heartbeat)
    }

    /// Creates a new private WebSocket client for account updates.
    #[staticmethod]
    #[pyo3(name = "new_private")]
    fn py_new_private(
        url: String,
        mnemonic: String,
        account_index: u32,
        authenticator_ids: Vec<u64>,
        account_id: AccountId,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        let credential = DydxCredential::from_mnemonic(&mnemonic, account_index, authenticator_ids)
            .map_err(to_pyvalue_err)?;
        Ok(Self::new_private(url, credential, account_id, heartbeat))
    }

    /// Returns whether the client is currently connected.
    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    /// Sets the account ID for account message parsing.
    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    /// Returns the current account ID if set.
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> Option<AccountId> {
        self.account_id()
    }

    /// Subscribes to public trade updates for a specific instrument.
    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let handler_cmd_tx = self.handler_cmd_tx.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let ticker = ticker_from_instrument_id(&instrument_id);
            let sub = crate::websocket::messages::DydxSubscription {
                op: crate::websocket::enums::DydxWsOperation::Subscribe,
                channel: crate::websocket::enums::DydxWsChannel::Trades,
                id: Some(ticker),
            };
            let payload = serde_json::to_string(&sub).map_err(to_pyvalue_err)?;
            handler_cmd_tx
                .send(crate::websocket::handler::HandlerCommand::SendText(payload))
                .map_err(|e| to_pyvalue_err(anyhow::anyhow!("{}", e)))
        })
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let handler_cmd_tx = self.handler_cmd_tx.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let ticker = ticker_from_instrument_id(&instrument_id);
            let sub = crate::websocket::messages::DydxSubscription {
                op: crate::websocket::enums::DydxWsOperation::Unsubscribe,
                channel: crate::websocket::enums::DydxWsChannel::Trades,
                id: Some(ticker),
            };
            let payload = serde_json::to_string(&sub).map_err(to_pyvalue_err)?;
            handler_cmd_tx
                .send(crate::websocket::handler::HandlerCommand::SendText(payload))
                .map_err(|e| to_pyvalue_err(anyhow::anyhow!("{}", e)))
        })
    }

    /// Subscribes to orderbook updates for a specific instrument.
    #[pyo3(name = "subscribe_orderbook")]
    fn py_subscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_orderbook(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    #[pyo3(name = "unsubscribe_orderbook")]
    fn py_unsubscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orderbook(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to bar updates for a specific instrument.
    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        resolution: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from bar updates for a specific instrument.
    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        resolution: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to all markets updates.
    #[pyo3(name = "subscribe_markets")]
    fn py_subscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_markets().await.map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from all markets updates.
    #[pyo3(name = "unsubscribe_markets")]
    fn py_unsubscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unsubscribe_markets().await.map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to subaccount updates.
    #[pyo3(name = "subscribe_subaccount")]
    fn py_subscribe_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from subaccount updates.
    #[pyo3(name = "unsubscribe_subaccount")]
    fn py_unsubscribe_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to block height updates.
    #[pyo3(name = "subscribe_block_height")]
    fn py_subscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_block_height()
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from block height updates.
    #[pyo3(name = "unsubscribe_block_height")]
    fn py_unsubscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_block_height()
                .await
                .map_err(to_pyvalue_err)
        })
    }
}

fn to_pyvalue_err<E: std::error::Error>(e: E) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(e.to_string())
}
