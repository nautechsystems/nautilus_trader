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

//! Python bindings for Lighter configuration.

use nautilus_core::{python::to_pyvalue_err, string::secret::SecretString};
use nautilus_model::identifiers::{AccountId, Venue};
use nautilus_network::websocket::TransportBackend;
use pyo3::{PyResult, pymethods};

use crate::{
    common::enums::{LighterDeployment, LighterEnvironment},
    config::{
        LighterDataClientConfig, LighterExecutionClientConfig, validate_scheduled_cancel_timeout,
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl LighterDataClientConfig {
    /// Configuration for the Lighter live data client.
    #[new]
    #[pyo3(signature = (
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        account_index = None,
        api_key_index = None,
        private_key = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
        rest_quota_per_min = None,
        transport_backend = None,
        deployment = None,
        venue = None,
        book_snapshot_timeout_secs = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<LighterEnvironment>,
        account_index: Option<u64>,
        api_key_index: Option<u8>,
        private_key: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        rest_quota_per_min: Option<u32>,
        transport_backend: Option<TransportBackend>,
        deployment: Option<LighterDeployment>,
        venue: Option<Venue>,
        book_snapshot_timeout_secs: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            environment: environment.unwrap_or(defaults.environment),
            deployment: deployment.unwrap_or(defaults.deployment),
            venue,
            account_index,
            api_key_index,
            private_key: private_key.map(SecretString::from),
            base_url_http,
            base_url_ws,
            proxy_url: proxy_url.map(SecretString::from),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            book_snapshot_timeout_secs: book_snapshot_timeout_secs
                .unwrap_or(defaults.book_snapshot_timeout_secs),
            rest_quota_per_min,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        }
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(LighterDataClientConfig).to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl LighterExecutionClientConfig {
    /// Configuration for the Lighter live execution client.
    #[new]
    #[pyo3(signature = (
        account_id,
        account_index = None,
        api_key_index = None,
        private_key = None,
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        market_order_slippage_bps = None,
        rest_quota_per_min = None,
        sendtx_quota_per_min = None,
        transport_backend = None,
        deployment = None,
        venue = None,
        use_gtd = true,
        scheduled_cancel_timeout_secs = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        account_id: AccountId,
        account_index: Option<u64>,
        api_key_index: Option<u8>,
        private_key: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<LighterEnvironment>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        market_order_slippage_bps: Option<u32>,
        rest_quota_per_min: Option<u32>,
        sendtx_quota_per_min: Option<u32>,
        transport_backend: Option<TransportBackend>,
        deployment: Option<LighterDeployment>,
        venue: Option<Venue>,
        use_gtd: bool,
        scheduled_cancel_timeout_secs: Option<u64>,
    ) -> PyResult<Self> {
        validate_scheduled_cancel_timeout(scheduled_cancel_timeout_secs).map_err(to_pyvalue_err)?;
        let defaults = Self::default();
        Ok(Self {
            environment: environment.unwrap_or(defaults.environment),
            deployment: deployment.unwrap_or(defaults.deployment),
            venue,
            account_id,
            account_index,
            api_key_index,
            private_key: private_key.map(SecretString::from),
            base_url_http,
            base_url_ws,
            proxy_url: proxy_url.map(SecretString::from),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            market_order_slippage_bps: market_order_slippage_bps
                .unwrap_or(defaults.market_order_slippage_bps),
            rest_quota_per_min,
            sendtx_quota_per_min,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            use_gtd,
            scheduled_cancel_timeout_secs,
        })
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(LighterExecutionClientConfig).to_string()
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        PyRef, Python,
        types::{PyAnyMethods, PyDict, PyDictMethods},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(None, true)]
    #[case(Some(420), true)]
    #[case(Some(1_295_940), true)]
    #[case(Some(419), false)]
    #[case(Some(1_295_941), false)]
    fn test_python_scheduled_cancel(#[case] timeout: Option<u64>, #[case] valid: bool) {
        Python::initialize();
        Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs
                .set_item("account_id", AccountId::from("LIGHTER-123"))
                .unwrap();

            if let Some(timeout) = timeout {
                kwargs
                    .set_item("scheduled_cancel_timeout_secs", timeout)
                    .unwrap();
            }

            let result = py
                .get_type::<LighterExecutionClientConfig>()
                .call((), Some(&kwargs));
            assert_eq!(result.is_ok(), valid);

            if valid {
                let config = result.unwrap();
                assert_eq!(
                    config
                        .getattr("scheduled_cancel_timeout_secs")
                        .unwrap()
                        .extract::<Option<u64>>()
                        .unwrap(),
                    timeout
                );
                assert_eq!(
                    config
                        .extract::<PyRef<LighterExecutionClientConfig>>()
                        .unwrap()
                        .scheduled_cancel_timeout_secs,
                    timeout
                );
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .is_instance_of::<pyo3::exceptions::PyValueError>(py)
                );
            }
        });
    }

    #[rstest]
    fn test_data_config_py_new_book_timeout() {
        let config = LighterDataClientConfig::py_new(
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
            Some(7),
            None,
            None,
            None,
            Some(42),
        );

        assert_eq!(config.rest_quota_per_min, Some(7));
        assert_eq!(config.book_snapshot_timeout_secs, 42);
    }

    #[rstest]
    fn test_data_config_py_new_book_timeout_default() {
        let config = LighterDataClientConfig::py_new(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
            None,
        );

        assert_eq!(config.book_snapshot_timeout_secs, 10);
    }
}
