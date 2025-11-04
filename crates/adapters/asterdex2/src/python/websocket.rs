use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::common::enums::AsterdexWsChannel;
use crate::websocket::AsterdexWebSocketClient;

fn to_pyerr(e: impl std::fmt::Display) -> PyErr {
    pyo3::exceptions::PyException::new_err(e.to_string())
}

#[pyclass(name = "AsterdexWebSocketClient")]
pub struct PyAsterdexWebSocketClient {
    client: AsterdexWebSocketClient,
}

#[pymethods]
impl PyAsterdexWebSocketClient {
    #[new]
    #[pyo3(signature = (base_url_ws_spot=None, base_url_ws_futures=None))]
    fn py_new(
        base_url_ws_spot: Option<String>,
        base_url_ws_futures: Option<String>,
    ) -> Self {
        let client = AsterdexWebSocketClient::new(base_url_ws_spot, base_url_ws_futures);
        Self { client }
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &self,
        py: Python<'py>,
        is_spot: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            client.connect(is_spot).await.map_err(to_pyerr)?;
            Ok(())
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            client.disconnect().await.map_err(to_pyerr)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_spot_agg_trade")]
    fn py_subscribe_spot_agg_trade<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = AsterdexWsChannel::SpotAggTrade { symbol };
            client.subscribe(channel).await.map_err(to_pyerr)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_futures_agg_trade")]
    fn py_subscribe_futures_agg_trade<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = AsterdexWsChannel::FuturesAggTrade { symbol };
            client.subscribe(channel).await.map_err(to_pyerr)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_spot_depth")]
    fn py_subscribe_spot_depth<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        levels: Option<u16>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = AsterdexWsChannel::SpotDepth { symbol, levels };
            client.subscribe(channel).await.map_err(to_pyerr)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_futures_depth")]
    fn py_subscribe_futures_depth<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        levels: Option<u16>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = AsterdexWsChannel::FuturesDepth { symbol, levels };
            client.subscribe(channel).await.map_err(to_pyerr)?;
            Ok(())
        })
    }

    #[pyo3(name = "receive")]
    fn py_receive<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let message = client.receive().await.map_err(to_pyerr)?;
            Ok(message)
        })
    }

    #[pyo3(name = "is_connected")]
    fn py_is_connected<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let connected = client.is_connected().await;
            Ok(connected)
        })
    }
}
