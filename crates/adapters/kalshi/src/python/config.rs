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

//! Python projection of the Rust Kalshi configs.

use nautilus_common::python::config_error_to_pyvalue_err;
use nautilus_core::string::secret::SecretString;
use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_network::websocket::TransportBackend;
use pyo3::prelude::*;

use crate::{
    common::enums::KalshiEnvironment,
    config::{KalshiDataClientConfig, KalshiExecutionClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KalshiDataClientConfig {
    /// Configuration for the Kalshi data client.
    #[new]
    #[pyo3(signature = (
        api_key_id = None,
        private_key = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
        transport_backend = None,
        book_snapshot_timeout_secs = None,
        instrument_ids = None,
    ))]
    #[expect(
        clippy::too_many_arguments,
        reason = "thin projection of the Rust config fields"
    )]
    fn py_new(
        api_key_id: Option<String>,
        private_key: Option<String>,
        environment: Option<KalshiEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        transport_backend: Option<TransportBackend>,
        book_snapshot_timeout_secs: Option<u64>,
        instrument_ids: Option<Vec<InstrumentId>>,
    ) -> PyResult<Self> {
        let defaults = Self::default();

        let config = Self {
            api_key_id: api_key_id.map(SecretString::from),
            private_key: private_key.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http: base_url_http.map(SecretString::from),
            base_url_ws: base_url_ws.map(SecretString::from),
            proxy_url: proxy_url.map(SecretString::from),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            book_snapshot_timeout_secs: book_snapshot_timeout_secs
                .unwrap_or(defaults.book_snapshot_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            instrument_ids: instrument_ids.unwrap_or(defaults.instrument_ids),
        };

        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    /// Returns whether both explicit credential fields contain non-empty values.
    #[getter(has_credentials)]
    fn py_has_credentials(&self) -> bool {
        Self::has_credentials(self)
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KalshiExecutionClientConfig {
    /// Configuration for the Kalshi execution client.
    #[new]
    #[pyo3(signature = (account_id = None, api_key_id = None, private_key = None, environment = None, base_url_http = None, base_url_ws = None, proxy_url = None, http_timeout_secs = None, ws_timeout_secs = None, exchange_index = None, transport_backend = None, subaccount = None))]
    #[expect(
        clippy::too_many_arguments,
        reason = "thin projection of the Rust config fields"
    )]
    fn py_new(
        account_id: Option<AccountId>,
        api_key_id: Option<String>,
        private_key: Option<String>,
        environment: Option<KalshiEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        exchange_index: Option<u32>,
        transport_backend: Option<TransportBackend>,
        subaccount: Option<u32>,
    ) -> PyResult<Self> {
        let defaults = Self::default();

        let config = Self {
            account_id: account_id.unwrap_or(defaults.account_id),
            subaccount: subaccount.unwrap_or(defaults.subaccount),
            api_key_id: api_key_id.map(SecretString::from),
            private_key: private_key.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http: base_url_http.map(SecretString::from),
            base_url_ws: base_url_ws.map(SecretString::from),
            proxy_url: proxy_url.map(SecretString::from),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            exchange_index: exchange_index.unwrap_or(defaults.exchange_index),
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        };

        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    /// Returns whether both explicit credential fields contain non-empty values.
    #[getter(has_credentials)]
    fn py_has_credentials(&self) -> bool {
        Self::has_credentials(self)
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }
}

#[cfg(test)]
mod tests {
    use pyo3::types::PyDict;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("KalshiDataClientConfig")]
    #[case("KalshiExecutionClientConfig")]
    fn test_python_constructor_defaults_and_secret_representations(#[case] name: &str) {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::new(py, "kalshi").unwrap();
            crate::python::kalshi(&module).unwrap();
            let class = module.getattr(name).unwrap();
            let default = class.call0().unwrap();
            let kwargs = PyDict::new(py);

            let backend = TransportBackend::Sockudo;

            kwargs
                .set_item("api_key_id", "python-distinct-key")
                .unwrap();
            kwargs
                .set_item("private_key", "python-distinct-private")
                .unwrap();
            kwargs
                .set_item("base_url_http", "http://secret-host/path")
                .unwrap();
            kwargs
                .set_item("base_url_ws", "ws://secret-ws/path")
                .unwrap();
            kwargs
                .set_item("proxy_url", "http://user:python-proxy@localhost:12000")
                .unwrap();
            kwargs.set_item("http_timeout_secs", 17).unwrap();
            kwargs.set_item("ws_timeout_secs", 19).unwrap();
            kwargs
                .set_item("environment", KalshiEnvironment::Demo)
                .unwrap();
            kwargs.set_item("transport_backend", backend).unwrap();
            if name == "KalshiDataClientConfig" {
                kwargs.set_item("book_snapshot_timeout_secs", 31).unwrap();
                kwargs
                    .set_item("update_instruments_interval_mins", 23)
                    .unwrap();
                kwargs
                    .set_item(
                        "instrument_ids",
                        vec![InstrumentId::from("KXHIGHNY-26OCT31-T70.KALSHI")],
                    )
                    .unwrap();
            } else {
                kwargs
                    .set_item("account_id", AccountId::from("KALSHI-PYTHON"))
                    .unwrap();
                kwargs.set_item("exchange_index", 3).unwrap();
                kwargs.set_item("subaccount", 7).unwrap();
            }

            let config = class.call((), Some(&kwargs)).unwrap();
            if name == "KalshiDataClientConfig" {
                let actual = config.extract::<KalshiDataClientConfig>().unwrap();

                let expected = KalshiDataClientConfig {
                    api_key_id: Some("python-distinct-key".into()),
                    private_key: Some("python-distinct-private".into()),
                    environment: KalshiEnvironment::Demo,
                    base_url_http: Some("http://secret-host/path".into()),
                    base_url_ws: Some("ws://secret-ws/path".into()),
                    proxy_url: Some("http://user:python-proxy@localhost:12000".into()),
                    http_timeout_secs: 17,
                    ws_timeout_secs: 19,
                    update_instruments_interval_mins: 23,
                    book_snapshot_timeout_secs: 31,
                    transport_backend: backend,
                    instrument_ids: vec![InstrumentId::from("KXHIGHNY-26OCT31-T70.KALSHI")],
                };

                assert_eq!(actual, expected);
                assert_eq!(
                    default.extract::<KalshiDataClientConfig>().unwrap(),
                    KalshiDataClientConfig::default()
                );
            } else {
                let actual = config.extract::<KalshiExecutionClientConfig>().unwrap();

                let expected = KalshiExecutionClientConfig {
                    account_id: AccountId::from("KALSHI-PYTHON"),
                    api_key_id: Some("python-distinct-key".into()),
                    private_key: Some("python-distinct-private".into()),
                    environment: KalshiEnvironment::Demo,
                    base_url_http: Some("http://secret-host/path".into()),
                    base_url_ws: Some("ws://secret-ws/path".into()),
                    proxy_url: Some("http://user:python-proxy@localhost:12000".into()),
                    http_timeout_secs: 17,
                    ws_timeout_secs: 19,
                    exchange_index: 3,
                    transport_backend: backend,
                    subaccount: 7,
                };

                assert_eq!(actual, expected);
                assert_eq!(
                    default.extract::<KalshiExecutionClientConfig>().unwrap(),
                    KalshiExecutionClientConfig::default()
                );
            }

            assert_eq!(
                default
                    .getattr("http_timeout_secs")
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                60
            );
            assert_eq!(
                default
                    .getattr("ws_timeout_secs")
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                30
            );
            assert!(
                !default
                    .getattr("has_credentials")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );
            assert!(
                !default
                    .getattr("has_proxy_url")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );
            assert_eq!(
                config
                    .getattr("http_timeout_secs")
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                17
            );
            assert_eq!(
                config
                    .getattr("ws_timeout_secs")
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                19
            );
            assert!(
                config
                    .getattr("has_credentials")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );
            assert!(
                config
                    .getattr("has_proxy_url")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );

            for output in [
                config.repr().unwrap().to_string(),
                config.str().unwrap().to_string(),
            ] {
                assert_eq!(output.matches("<redacted>").count(), 5);

                for secret in [
                    "python-distinct-key",
                    "python-distinct-private",
                    "secret-host",
                    "secret-ws",
                    "python-proxy",
                ] {
                    assert!(!output.contains(secret));
                }
            }

            for field in [
                "api_key_id",
                "private_key",
                "proxy_url",
                "base_url_http",
                "base_url_ws",
            ] {
                assert!(!config.hasattr(field).unwrap());
            }

            kwargs
                .set_item("transport_backend", TransportBackend::Tungstenite)
                .unwrap();
            let error = class.call((), Some(&kwargs)).unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
            assert!(
                error
                    .to_string()
                    .contains("Kalshi requires the Sockudo WebSocket backend")
            );
            kwargs.set_item("transport_backend", backend).unwrap();

            kwargs.set_item("unknown_field", true).unwrap();
            assert!(
                class
                    .call((), Some(&kwargs))
                    .unwrap_err()
                    .is_instance_of::<pyo3::exceptions::PyTypeError>(py)
            );
            kwargs.del_item("unknown_field").unwrap();
            kwargs.set_item("http_timeout_secs", 0).unwrap();
            assert!(
                class
                    .call((), Some(&kwargs))
                    .unwrap_err()
                    .is_instance_of::<pyo3::exceptions::PyValueError>(py)
            );

            kwargs.set_item("http_timeout_secs", u64::MAX).unwrap();
            let error = class.call((), Some(&kwargs)).unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
            assert_eq!(
                error.value(py).str().unwrap().to_string(),
                "invalid http_timeout_secs: exceeds the supported clock range"
            );

            if name == "KalshiDataClientConfig" {
                kwargs.set_item("http_timeout_secs", 17).unwrap();
                kwargs
                    .set_item(
                        "instrument_ids",
                        vec![InstrumentId::from("BTC-USD.BINANCE")],
                    )
                    .unwrap();
                let error = class.call((), Some(&kwargs)).unwrap_err();
                assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
                assert!(
                    error
                        .to_string()
                        .contains("instrument venue must be KALSHI")
                );
            }

            assert!(!module.hasattr("KalshiExecClientConfig").unwrap());
        });
    }
}
