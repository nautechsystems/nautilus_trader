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

//! Python bindings for Derive configuration.

use nautilus_common::python::config_error_to_pyvalue_err;
use nautilus_core::string::secret::SecretString;
use nautilus_model::identifiers::AccountId;
use nautilus_network::websocket::TransportBackend;
use pyo3::{PyResult, pymethods};
use rust_decimal::Decimal;

use crate::{
    common::enums::DeriveEnvironment,
    config::{DeriveDataClientConfig, DeriveExecutionClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DeriveDataClientConfig {
    /// Configuration for the Derive data client.
    #[new]
    #[pyo3(signature = (
        base_url_rest = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
        currencies = None,
        include_expired = None,
        auto_load_missing_instruments = None,
        transport_backend = None,
        book_snapshot_timeout_secs = None,
        book_stale_threshold_secs = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<DeriveEnvironment>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        currencies: Option<Vec<String>>,
        include_expired: Option<bool>,
        auto_load_missing_instruments: Option<bool>,
        transport_backend: Option<TransportBackend>,
        book_snapshot_timeout_secs: Option<u64>,
        book_stale_threshold_secs: Option<u64>,
    ) -> PyResult<Self> {
        let defaults = Self::default();

        let config = Self {
            base_url_rest,
            base_url_ws,
            proxy_url: proxy_url.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs,
            book_snapshot_timeout_secs: book_snapshot_timeout_secs
                .unwrap_or(defaults.book_snapshot_timeout_secs),
            book_stale_threshold_secs: book_stale_threshold_secs
                .unwrap_or(defaults.book_stale_threshold_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            currencies: currencies.unwrap_or(defaults.currencies),
            include_expired: include_expired.unwrap_or(defaults.include_expired),
            auto_load_missing_instruments: auto_load_missing_instruments
                .unwrap_or(defaults.auto_load_missing_instruments),
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
        stringify!(DeriveDataClientConfig).to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DeriveExecutionClientConfig {
    /// Configuration for the Derive execution client.
    #[new]
    #[pyo3(signature = (
        account_id = None,
        wallet_address = None,
        session_key = None,
        subaccount_id = None,
        base_url_rest = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        ws_timeout_secs = None,
        max_fee_per_contract = None,
        domain_separator = None,
        action_typehash = None,
        trade_module_address = None,
        signature_expiry_secs = None,
        market_order_slippage_bps = None,
        max_matching_requests_per_second = None,
        max_per_instrument_matching_requests_per_second = None,
        transport_backend = None,
        cancel_on_disconnect = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        account_id: Option<AccountId>,
        wallet_address: Option<String>,
        session_key: Option<String>,
        subaccount_id: Option<u64>,
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<DeriveEnvironment>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        ws_timeout_secs: Option<u64>,
        max_fee_per_contract: Option<Decimal>,
        domain_separator: Option<String>,
        action_typehash: Option<String>,
        trade_module_address: Option<String>,
        signature_expiry_secs: Option<u64>,
        market_order_slippage_bps: Option<u32>,
        max_matching_requests_per_second: Option<u32>,
        max_per_instrument_matching_requests_per_second: Option<u32>,
        transport_backend: Option<TransportBackend>,
        cancel_on_disconnect: Option<bool>,
    ) -> Self {
        let defaults = Self::default();

        Self {
            account_id: account_id.unwrap_or(defaults.account_id),
            wallet_address,
            session_key: session_key.map(SecretString::from),
            subaccount_id,
            base_url_rest,
            base_url_ws,
            proxy_url: proxy_url.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            ws_timeout_secs,
            max_fee_per_contract,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            domain_separator,
            action_typehash,
            trade_module_address,
            signature_expiry_secs: signature_expiry_secs.unwrap_or(defaults.signature_expiry_secs),
            market_order_slippage_bps: market_order_slippage_bps
                .unwrap_or(defaults.market_order_slippage_bps),
            max_matching_requests_per_second,
            max_per_instrument_matching_requests_per_second,
            cancel_on_disconnect: cancel_on_disconnect.unwrap_or(defaults.cancel_on_disconnect),
        }
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(DeriveExecutionClientConfig).to_string()
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{prelude::*, types::PyDict};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::default(None, false)]
    #[case::disabled(Some(false), false)]
    #[case::enabled(Some(true), true)]
    fn test_python_cancel_on_disconnect(#[case] enabled: Option<bool>, #[case] expected: bool) {
        Python::initialize();
        Python::attach(|py| {
            let kwargs = PyDict::new(py);
            if let Some(enabled) = enabled {
                kwargs.set_item("cancel_on_disconnect", enabled).unwrap();
            }

            let config = py
                .get_type::<DeriveExecutionClientConfig>()
                .call((), Some(&kwargs))
                .unwrap();
            let getter = config
                .getattr("cancel_on_disconnect")
                .unwrap()
                .extract::<bool>()
                .unwrap();
            let rust_config = config
                .extract::<PyRef<DeriveExecutionClientConfig>>()
                .unwrap();

            assert_eq!(getter, expected);
            assert_eq!(rust_config.cancel_on_disconnect, expected);
        });
    }

    #[rstest]
    fn test_python_currencies_normalized_uniqueness(#[values(false, true)] duplicate: bool) {
        Python::initialize();
        Python::attach(|py| {
            let mut values = vec![" ETH ".to_string(), "BTC".to_string()];
            if duplicate {
                values.push("ETH".to_string());
            }

            let kwargs = PyDict::new(py);
            kwargs.set_item("currencies", &values).unwrap();
            let result = py
                .get_type::<DeriveDataClientConfig>()
                .call((), Some(&kwargs));

            if duplicate {
                let error = result.unwrap_err();
                assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
                assert_eq!(
                    error.value(py).str().unwrap().to_str().unwrap(),
                    "duplicate currencies"
                );
            } else {
                let config = result.unwrap();
                let config = config.extract::<PyRef<DeriveDataClientConfig>>().unwrap();
                assert_eq!(config.currencies, values);
            }
        });
    }
}
