use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::http::AsterdexHttpClient;

fn to_pyerr(e: impl std::fmt::Display) -> PyErr {
    pyo3::exceptions::PyException::new_err(e.to_string())
}

#[pyclass(name = "AsterdexHttpClient")]
pub struct PyAsterdexHttpClient {
    client: AsterdexHttpClient,
}

#[pymethods]
impl PyAsterdexHttpClient {
    #[new]
    #[pyo3(signature = (base_url_http_spot=None, base_url_http_futures=None, api_key=None, api_secret=None))]
    fn py_new(
        base_url_http_spot: Option<String>,
        base_url_http_futures: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> PyResult<Self> {
        let client = AsterdexHttpClient::new(
            base_url_http_spot,
            base_url_http_futures,
            api_key,
            api_secret,
        )
        .map_err(to_pyerr)?;

        Ok(Self { client })
    }

    #[pyo3(name = "request_spot_exchange_info")]
    fn py_request_spot_exchange_info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let result = client.request_spot_exchange_info().await.map_err(to_pyerr)?;
            Ok(result.to_string())
        })
    }

    #[pyo3(name = "request_futures_exchange_info")]
    fn py_request_futures_exchange_info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let result = client.request_futures_exchange_info().await.map_err(to_pyerr)?;
            Ok(result.to_string())
        })
    }

    #[pyo3(name = "request_spot_order_book")]
    fn py_request_spot_order_book<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let result = client.request_spot_order_book(&symbol, limit).await.map_err(to_pyerr)?;
            Ok(result.to_string())
        })
    }

    #[pyo3(name = "request_futures_order_book")]
    fn py_request_futures_order_book<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let result = client.request_futures_order_book(&symbol, limit).await.map_err(to_pyerr)?;
            Ok(result.to_string())
        })
    }

    #[pyo3(name = "request_spot_account")]
    fn py_request_spot_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let result = client.request_spot_account().await.map_err(to_pyerr)?;
            Ok(result.to_string())
        })
    }

    #[pyo3(name = "request_futures_account")]
    fn py_request_futures_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let result = client.request_futures_account().await.map_err(to_pyerr)?;
            Ok(result.to_string())
        })
    }

    /// Loads all instruments from Asterdex and returns the count.
    ///
    /// Note: Full Python conversion pending - this returns the count of loaded instruments.
    /// Instruments are stored internally and can be accessed via Nautilus providers.
    #[pyo3(name = "load_instruments")]
    fn py_load_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            client
                .load_instruments()
                .await
                .map(|instruments| instruments.len())
                .map_err(to_pyerr)
        })
    }

    /// Returns the count of loaded instruments.
    ///
    /// Note: Full Python conversion pending - this returns the count.
    /// Use Nautilus InstrumentProvider to access instruments in Python.
    #[pyo3(name = "instruments")]
    fn py_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let instruments = client.instruments().await;
            Ok(instruments.len())
        })
    }
}
