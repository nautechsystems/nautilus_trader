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

//! Python bindings for OKX configuration.

use nautilus_common::python::config_error_to_pyvalue_err;
use nautilus_core::string::secret::SecretString;
use nautilus_model::{enums::OmsType, identifiers::AccountId};
use nautilus_network::websocket::TransportBackend;
use pyo3::prelude::*;

use crate::{
    common::enums::{OKXEnvironment, OKXInstrumentType, OKXMarginMode, OKXRegion, OKXVipLevel},
    config::{OKXDataClientConfig, OKXExecutionClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OKXDataClientConfig {
    /// Configuration for the OKX data client.
    #[new]
    #[pyo3(signature = (
        instrument_types = None,
        environment = None,
        region = None,
        api_key = None,
        api_secret = None,
        api_passphrase = None,
        base_url_http = None,
        base_url_ws_public = None,
        base_url_ws_business = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        update_instruments_interval_mins = None,
        book_stale_check_interval_secs = None,
        book_stale_threshold_secs = None,
        book_snapshot_timeout_secs = None,
        vip_level = None,
        load_spreads = false,
        transport_backend = None,
        instrument_families = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_types: Option<Vec<OKXInstrumentType>>,
        environment: Option<OKXEnvironment>,
        region: Option<OKXRegion>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_public: Option<String>,
        base_url_ws_business: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        book_stale_check_interval_secs: Option<u64>,
        book_stale_threshold_secs: Option<u64>,
        book_snapshot_timeout_secs: Option<u64>,
        vip_level: Option<OKXVipLevel>,
        load_spreads: bool,
        transport_backend: Option<TransportBackend>,
        instrument_families: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let defaults = Self::default();

        let config = Self {
            api_key: api_key.map(SecretString::from),
            api_secret: api_secret.map(SecretString::from),
            api_passphrase: api_passphrase.map(SecretString::from),
            instrument_types: instrument_types.unwrap_or(defaults.instrument_types),
            contract_types: None,
            load_spreads,
            instrument_families,
            base_url_http,
            base_url_ws_public,
            base_url_ws_business,
            proxy_url: proxy_url.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            region: region.unwrap_or(defaults.region),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            book_stale_check_interval_secs: book_stale_check_interval_secs
                .unwrap_or(defaults.book_stale_check_interval_secs),
            book_stale_threshold_secs: book_stale_threshold_secs
                .unwrap_or(defaults.book_stale_threshold_secs),
            book_snapshot_timeout_secs: book_snapshot_timeout_secs
                .unwrap_or(defaults.book_snapshot_timeout_secs),
            vip_level,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        };

        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(OKXDataClientConfig).to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OKXExecutionClientConfig {
    /// Configuration for the OKX execution client.
    #[new]
    #[pyo3(signature = (
        account_id,
        instrument_types = None,
        environment = None,
        region = None,
        api_key = None,
        api_secret = None,
        api_passphrase = None,
        base_url_http = None,
        base_url_ws_private = None,
        base_url_ws_business = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        margin_mode = None,
        load_spreads = false,
        auth_timeout_secs = None,
        transport_backend = None,
        spot_trade_quote_ccy = None,
        oms_type = None,
        max_reconciliation_history_pages = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        account_id: AccountId,
        instrument_types: Option<Vec<OKXInstrumentType>>,
        environment: Option<OKXEnvironment>,
        region: Option<OKXRegion>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_private: Option<String>,
        base_url_ws_business: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        margin_mode: Option<OKXMarginMode>,
        load_spreads: bool,
        auth_timeout_secs: Option<u64>,
        transport_backend: Option<TransportBackend>,
        spot_trade_quote_ccy: Option<String>,
        oms_type: Option<OmsType>,
        max_reconciliation_history_pages: Option<u32>,
    ) -> PyResult<Self> {
        let defaults = Self::default();

        let config = Self {
            account_id,
            oms_type,
            api_key: api_key.map(SecretString::from),
            api_secret: api_secret.map(SecretString::from),
            api_passphrase: api_passphrase.map(SecretString::from),
            instrument_types: instrument_types.unwrap_or(defaults.instrument_types),
            contract_types: None,
            instrument_families: None,
            base_url_http,
            base_url_ws_private,
            base_url_ws_business,
            proxy_url: proxy_url.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            region: region.unwrap_or(defaults.region),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            use_mm_mass_cancel: defaults.use_mm_mass_cancel,
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            margin_mode,
            load_spreads,
            use_spot_margin: defaults.use_spot_margin,
            auth_timeout_secs,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            spot_trade_quote_ccy,
            max_reconciliation_history_pages,
        };

        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(OKXExecutionClientConfig).to_string()
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        Python,
        exceptions::PyValueError,
        types::{PyDict, PyList},
    };
    use rstest::rstest;

    use super::*;

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
                    OKXInstrumentType::Swap,
                    OKXInstrumentType::Spot,
                    OKXInstrumentType::Swap,
                ]
            } else {
                vec![OKXInstrumentType::Swap, OKXInstrumentType::Spot]
            };

            let objects: Vec<_> = values
                .iter()
                .map(|value| Py::new(py, *value).unwrap())
                .collect();
            let list = PyList::new(py, objects).unwrap();
            let kwargs = PyDict::new(py);
            kwargs.set_item("instrument_types", list).unwrap();
            if execution {
                kwargs
                    .set_item(
                        "account_id",
                        Py::new(py, AccountId::from("OKX-001")).unwrap(),
                    )
                    .unwrap();
            }

            let config_type = if execution {
                py.get_type::<OKXExecutionClientConfig>()
            } else {
                py.get_type::<OKXDataClientConfig>()
            };

            let result = config_type.call((), Some(&kwargs));
            if duplicate {
                let error = result.unwrap_err();
                assert!(error.is_instance_of::<PyValueError>(py));
                assert_eq!(
                    error.value(py).str().unwrap().to_str().unwrap(),
                    "duplicate instrument_types"
                );
            } else {
                let config = result.unwrap();
                let actual: Vec<OKXInstrumentType> = config
                    .getattr("instrument_types")
                    .unwrap()
                    .extract()
                    .unwrap();
                assert_eq!(actual, values);
            }
        });
    }

    #[rstest]
    fn test_data_config_py_new() {
        let config = OKXDataClientConfig::py_new(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            true,
            None,
            Some(vec!["BTC-USD".to_string()]),
        )
        .unwrap();

        assert_eq!(
            config.instrument_families,
            Some(vec!["BTC-USD".to_string()]),
        );
        assert!(config.load_spreads);
        assert_eq!(config.book_stale_check_interval_secs, 5);
        assert_eq!(config.book_stale_threshold_secs, 30);
        assert_eq!(config.book_snapshot_timeout_secs, 10);
    }

    #[rstest]
    #[case(None)]
    #[case(Some(0))]
    #[case(Some(2))]
    #[case(Some(51))]
    #[case(Some(u32::MAX))]
    fn test_exec_config_python_history_pages(#[case] pages: Option<u32>) {
        Python::initialize();
        Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs
                .set_item(
                    "account_id",
                    Py::new(py, AccountId::from("OKX-001")).unwrap(),
                )
                .unwrap();

            if let Some(pages) = pages {
                kwargs
                    .set_item("max_reconciliation_history_pages", pages)
                    .unwrap();
            }

            let config = py
                .get_type::<OKXExecutionClientConfig>()
                .call((), Some(&kwargs))
                .unwrap();
            let actual: Option<u32> = config
                .getattr("max_reconciliation_history_pages")
                .unwrap()
                .extract()
                .unwrap();

            assert_eq!(actual, pages);
        });
    }

    #[rstest]
    fn test_exec_config_py_new_load_spreads() {
        let config = OKXExecutionClientConfig::py_new(
            AccountId::from("OKX-001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            true,
            None,
            None,
            Some("USD".to_string()),
            Some(OmsType::Hedging),
            Some(7),
        )
        .unwrap();

        assert!(config.load_spreads);
        assert_eq!(config.auth_timeout_secs, None);
        assert_eq!(config.spot_trade_quote_ccy.as_deref(), Some("USD"));
        assert_eq!(config.oms_type, Some(OmsType::Hedging));
        assert_eq!(config.max_reconciliation_history_pages, Some(7));
    }
}
