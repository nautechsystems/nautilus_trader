// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python bindings for Bybit configuration.

use nautilus_common::python::config_error_to_pyvalue_err;
use nautilus_core::{python::to_pyvalue_err, string::secret::SecretString};
use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_network::websocket::TransportBackend;
use pyo3::{PyResult, pymethods};

use crate::{
    common::{
        enums::{BybitEnvironment, BybitMarginMode, BybitProductType},
        parse::parse_smp_type,
    },
    config::{BybitDataClientConfig, BybitExecutionClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitDataClientConfig {
    /// Configuration for the Bybit live data client.
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws_public = None,
        base_url_ws_private = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        heartbeat_interval_secs = None,
        recv_window_ms = None,
        update_instruments_interval_mins = None,
        instrument_status_poll_secs = None,
        transport_backend = None,
        book_snapshot_timeout_secs = None,
        bars_timestamp_on_close = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        product_types: Option<Vec<BybitProductType>>,
        environment: Option<BybitEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_public: Option<String>,
        base_url_ws_private: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        recv_window_ms: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        instrument_status_poll_secs: Option<u64>,
        transport_backend: Option<TransportBackend>,
        book_snapshot_timeout_secs: Option<u64>,
        bars_timestamp_on_close: Option<bool>,
    ) -> PyResult<Self> {
        let defaults = Self::default();

        let config = Self {
            api_key: api_key.map(SecretString::from),
            api_secret: api_secret.map(SecretString::from),
            product_types: product_types.unwrap_or(defaults.product_types),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http,
            base_url_ws_public,
            base_url_ws_private,
            proxy_url: proxy_url.map(SecretString::from),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(defaults.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            update_instruments_interval_mins: update_instruments_interval_mins
                .or(defaults.update_instruments_interval_mins),
            instrument_poll_interval_secs: instrument_status_poll_secs
                .or(defaults.instrument_poll_interval_secs),
            book_snapshot_timeout_secs: book_snapshot_timeout_secs
                .unwrap_or(defaults.book_snapshot_timeout_secs),
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            bars_timestamp_on_close: bars_timestamp_on_close
                .unwrap_or(defaults.bars_timestamp_on_close),
        };

        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    const fn instrument_status_poll_secs(&self) -> Option<u64> {
        self.instrument_poll_interval_secs
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(BybitDataClientConfig).to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitExecutionClientConfig {
    /// Configuration for the Bybit live execution client.
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws_private = None,
        base_url_ws_trade = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        heartbeat_interval_secs = None,
        auth_timeout_secs = None,
        recv_window_ms = None,
        account_id = None,
        use_spot_position_reports = None,
        auto_repay_spot_borrows = None,
        margin_mode = None,
        smp_type = None,
        transport_backend = None,
        use_ws_execution_fast = None,
        load_ids = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        product_types: Option<Vec<BybitProductType>>,
        environment: Option<BybitEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_private: Option<String>,
        base_url_ws_trade: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        auth_timeout_secs: Option<u64>,
        recv_window_ms: Option<u64>,
        account_id: Option<AccountId>,
        use_spot_position_reports: Option<bool>,
        auto_repay_spot_borrows: Option<bool>,
        margin_mode: Option<BybitMarginMode>,
        smp_type: Option<String>,
        transport_backend: Option<TransportBackend>,
        use_ws_execution_fast: Option<bool>,
        load_ids: Option<Vec<InstrumentId>>,
    ) -> PyResult<Self> {
        let smp_type = smp_type
            .map(|value| parse_smp_type(&value))
            .transpose()
            .map_err(to_pyvalue_err)?;

        let defaults = Self::default();

        let config = Self {
            api_key: api_key.map(SecretString::from),
            api_secret: api_secret.map(SecretString::from),
            product_types: product_types.unwrap_or(defaults.product_types),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http,
            base_url_ws_private,
            base_url_ws_trade,
            proxy_url: proxy_url.map(SecretString::from),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(defaults.heartbeat_interval_secs),
            auth_timeout_secs,
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            account_id,
            use_spot_position_reports: use_spot_position_reports
                .unwrap_or(defaults.use_spot_position_reports),
            auto_repay_spot_borrows: auto_repay_spot_borrows
                .unwrap_or(defaults.auto_repay_spot_borrows),
            use_ws_execution_fast: use_ws_execution_fast.unwrap_or(defaults.use_ws_execution_fast),
            futures_leverages: None,
            position_mode: None,
            margin_mode,
            smp_type,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            load_ids,
        };

        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    #[pyo3(name = "smp_type")]
    fn py_smp_type(&self) -> Option<String> {
        self.smp_type.map(|smp_type| smp_type.as_ref().to_string())
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(BybitExecutionClientConfig).to_string()
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        Python,
        exceptions::PyValueError,
        prelude::*,
        types::{PyDict, PyList},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::valid("ETHUSDT-LINEAR.BYBIT", true)]
    #[case::invalid("ETHUSDT-LINEAR.BINANCE", false)]
    #[case::empty_symbol("-LINEAR.BYBIT", false)]
    fn test_python_execution_config_load_ids(#[case] id: &str, #[case] valid: bool) {
        Python::initialize();
        Python::attach(|py| {
            let instrument_id = InstrumentId::from(id);
            let list = PyList::new(py, [Py::new(py, instrument_id).unwrap()]).unwrap();
            let kwargs = PyDict::new(py);
            kwargs.set_item("load_ids", list).unwrap();
            let result = py
                .get_type::<BybitExecutionClientConfig>()
                .call((), Some(&kwargs));

            if valid {
                let actual: Option<Vec<InstrumentId>> = result
                    .unwrap()
                    .getattr("load_ids")
                    .unwrap()
                    .extract()
                    .unwrap();
                assert_eq!(actual, Some(vec![instrument_id]));
            } else {
                let error = result.unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
                assert_eq!(
                    error.value(py).str().unwrap().to_str().unwrap(),
                    format!(
                        "invalid load_ids: {id} is not an uppercase BYBIT instrument in a configured product type"
                    )
                );
            }
        });
    }

    #[rstest]
    #[case::data(false)]
    #[case::execution(true)]
    fn test_python_config_duplicate_types(
        #[case] execution: bool,
        #[values(false, true)] duplicate: bool,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let values = if duplicate {
                vec![
                    BybitProductType::Linear,
                    BybitProductType::Spot,
                    BybitProductType::Linear,
                ]
            } else {
                vec![BybitProductType::Linear, BybitProductType::Spot]
            };

            let objects: Vec<_> = values
                .iter()
                .map(|value| Py::new(py, *value).unwrap())
                .collect();
            let list = PyList::new(py, objects).unwrap();
            let kwargs = PyDict::new(py);
            kwargs.set_item("product_types", list).unwrap();
            if execution {
                kwargs
                    .set_item(
                        "account_id",
                        Py::new(py, AccountId::from("BYBIT-001")).unwrap(),
                    )
                    .unwrap();
            }

            let config_type = if execution {
                py.get_type::<BybitExecutionClientConfig>()
            } else {
                py.get_type::<BybitDataClientConfig>()
            };

            let result = config_type.call((), Some(&kwargs));
            if duplicate {
                let error = result.unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
                assert_eq!(
                    error.value(py).str().unwrap().to_str().unwrap(),
                    "duplicate product_types"
                );
            } else {
                let config = result.unwrap();
                let actual: Vec<BybitProductType> =
                    config.getattr("product_types").unwrap().extract().unwrap();
                assert_eq!(actual, values);
            }
        });
    }

    #[rstest]
    #[case::default(None, false)]
    #[case::enabled(Some(true), true)]
    #[case::disabled(Some(false), false)]
    fn test_python_execution_config_fast_selection(
        #[case] configured: Option<bool>,
        #[case] expected: bool,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let kwargs = PyDict::new(py);
            if let Some(configured) = configured {
                kwargs
                    .set_item("use_ws_execution_fast", configured)
                    .unwrap();
            }

            let config = py
                .get_type::<BybitExecutionClientConfig>()
                .call((), Some(&kwargs))
                .unwrap();
            let actual = config
                .getattr("use_ws_execution_fast")
                .unwrap()
                .extract::<bool>()
                .unwrap();
            assert_eq!(actual, expected);
        });
    }

    #[rstest]
    fn test_python_execution_config_parameter_order() {
        Python::initialize();
        Python::attach(|py| {
            let inspect = py.import("inspect").unwrap();
            let signature = inspect
                .call_method1("signature", (py.get_type::<BybitExecutionClientConfig>(),))
                .unwrap();
            let parameters = signature.getattr("parameters").unwrap();
            let keys = parameters.call_method0("keys").unwrap();
            let names = py
                .import("builtins")
                .unwrap()
                .call_method1("list", (keys,))
                .unwrap()
                .extract::<Vec<String>>()
                .unwrap();

            assert_eq!(
                names,
                [
                    "product_types",
                    "environment",
                    "api_key",
                    "api_secret",
                    "base_url_http",
                    "base_url_ws_private",
                    "base_url_ws_trade",
                    "proxy_url",
                    "http_timeout_secs",
                    "max_retries",
                    "retry_delay_initial_ms",
                    "retry_delay_max_ms",
                    "heartbeat_interval_secs",
                    "auth_timeout_secs",
                    "recv_window_ms",
                    "account_id",
                    "use_spot_position_reports",
                    "auto_repay_spot_borrows",
                    "margin_mode",
                    "smp_type",
                    "transport_backend",
                    "use_ws_execution_fast",
                    "load_ids",
                ],
            );
        });
    }
}
