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

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::pymethods;

use crate::{
    common::enums::SignatureType,
    config::{
        PolymarketDataClientConfig, PolymarketExecClientConfig, PolymarketInstrumentProviderConfig,
    },
};

const PY_OPTION_U64_MISSING_SENTINEL: u64 = u64::MAX;

fn resolve_optional_u64_arg(value: Option<u64>, default: Option<u64>) -> Option<u64> {
    match value {
        Some(PY_OPTION_U64_MISSING_SENTINEL) => default,
        other => other,
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PolymarketInstrumentProviderConfig {
    /// Configuration for the Polymarket instrument provider.
    ///
    /// This mirrors the Python adapter's `instrument_config` layering so scoped
    /// market bootstrap can migrate naturally to the Rust/pyO3 live path.
    #[new]
    #[pyo3(signature = (load_all=None, load_ids=None, filters=None, event_slugs=None, market_slugs=None, event_slug_builder=None, log_warnings=None, use_gamma_markets=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        load_all: Option<bool>,
        load_ids: Option<Vec<nautilus_model::identifiers::InstrumentId>>,
        filters: Option<std::collections::HashMap<String, String>>,
        event_slugs: Option<Vec<String>>,
        market_slugs: Option<Vec<String>>,
        event_slug_builder: Option<String>,
        log_warnings: Option<bool>,
        use_gamma_markets: Option<bool>,
    ) -> Self {
        let default = Self::default();
        Self {
            load_all: load_all.unwrap_or(default.load_all),
            load_ids,
            filters,
            event_slugs,
            market_slugs,
            event_slug_builder,
            log_warnings: log_warnings.unwrap_or(default.log_warnings),
            use_gamma_markets: use_gamma_markets.unwrap_or(default.use_gamma_markets),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PolymarketDataClientConfig {
    /// Configuration for the Polymarket data client.
    ///
    /// `filters` and `new_market_filter` hold `Arc<dyn InstrumentFilter>` trait objects
    /// and are skipped during serialization; they default to empty/`None` and must be
    /// installed programmatically after deserialization.
    #[new]
    #[pyo3(signature = (instrument_config=None, base_url_http=None, base_url_ws=None, base_url_gamma=None, base_url_data_api=None, http_timeout_secs=None, ws_timeout_secs=None, ws_max_subscriptions=None, update_instruments_interval_mins=PY_OPTION_U64_MISSING_SENTINEL, subscribe_new_markets=None, auto_load_missing_instruments=None, auto_load_debounce_ms=None, auto_load_max_retries=None, auto_load_retry_delay_initial_secs=None, auto_load_retry_delay_max_secs=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_config: Option<PolymarketInstrumentProviderConfig>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        base_url_gamma: Option<String>,
        base_url_data_api: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        ws_max_subscriptions: Option<usize>,
        update_instruments_interval_mins: Option<u64>,
        subscribe_new_markets: Option<bool>,
        auto_load_missing_instruments: Option<bool>,
        auto_load_debounce_ms: Option<u64>,
        auto_load_max_retries: Option<u32>,
        auto_load_retry_delay_initial_secs: Option<f64>,
        auto_load_retry_delay_max_secs: Option<f64>,
    ) -> Self {
        let default = Self::default();

        Self {
            instrument_config,
            base_url_http,
            base_url_ws,
            base_url_gamma,
            base_url_data_api,
            http_timeout_secs: http_timeout_secs.unwrap_or(default.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(default.ws_timeout_secs),
            ws_max_subscriptions: ws_max_subscriptions.unwrap_or(default.ws_max_subscriptions),
            update_instruments_interval_mins: resolve_optional_u64_arg(
                update_instruments_interval_mins,
                default.update_instruments_interval_mins,
            ),
            subscribe_new_markets: subscribe_new_markets.unwrap_or(default.subscribe_new_markets),
            auto_load_missing_instruments: auto_load_missing_instruments
                .unwrap_or(default.auto_load_missing_instruments),
            auto_load_debounce_ms: auto_load_debounce_ms.unwrap_or(default.auto_load_debounce_ms),
            auto_load_max_retries: auto_load_max_retries.unwrap_or(default.auto_load_max_retries),
            auto_load_retry_delay_initial_secs: auto_load_retry_delay_initial_secs
                .unwrap_or(default.auto_load_retry_delay_initial_secs),
            auto_load_retry_delay_max_secs: auto_load_retry_delay_max_secs
                .unwrap_or(default.auto_load_retry_delay_max_secs),
            filters: Vec::new(),
            new_market_filter: None,
            transport_backend: default.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PolymarketExecClientConfig {
    /// Configuration for the Polymarket execution client.
    ///
    /// `Debug` is implemented manually to redact secrets, so it is not part of the
    /// derive list.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (trader_id=None, account_id=None, private_key=None, api_key=None, api_secret=None, passphrase=None, funder=None, signature_type=None, base_url_http=None, base_url_ws=None, base_url_data_api=None, http_timeout_secs=None, max_retries=None, retry_delay_initial_ms=None, retry_delay_max_ms=None, ack_timeout_secs=None))]
    fn py_new(
        trader_id: Option<String>,
        account_id: Option<String>,
        private_key: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        passphrase: Option<String>,
        funder: Option<String>,
        signature_type: Option<SignatureType>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        base_url_data_api: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        ack_timeout_secs: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            trader_id: trader_id.map_or(default.trader_id, |s| TraderId::from(s.as_str())),
            account_id: account_id.map_or(default.account_id, |s| AccountId::from(s.as_str())),
            private_key,
            api_key,
            api_secret,
            passphrase,
            funder,
            signature_type: signature_type.unwrap_or(default.signature_type),
            base_url_http,
            base_url_ws,
            base_url_data_api,
            http_timeout_secs: http_timeout_secs.unwrap_or(default.http_timeout_secs),
            max_retries: max_retries.unwrap_or(default.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(default.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(default.retry_delay_max_ms),
            ack_timeout_secs: ack_timeout_secs.unwrap_or(default.ack_timeout_secs),
            transport_backend: default.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        Bound, Python,
        types::{PyAnyMethods, PyDict, PyDictMethods, PyModule, PyTuple},
    };

    use super::*;

    fn construct_data_client_config(
        py: Python<'_>,
        args: Option<&Bound<'_, PyTuple>>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PolymarketDataClientConfig {
        let module = PyModule::new(py, "polymarket").expect("module");
        crate::python::polymarket(py, &module).expect("polymarket module");
        let cls = module
            .getattr("PolymarketDataClientConfig")
            .expect("PolymarketDataClientConfig");

        let config = match args {
            Some(args) => cls.call(args, kwargs),
            None => cls.call((), kwargs),
        }
        .expect("construct PolymarketDataClientConfig");

        config
            .extract::<PolymarketDataClientConfig>()
            .expect("extract PolymarketDataClientConfig")
    }

    #[test]
    fn direct_pyo3_constructor_preserves_none_update_interval() {
        Python::initialize();
        Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs
                .set_item("update_instruments_interval_mins", py.None())
                .unwrap();

            let config = construct_data_client_config(py, None, Some(&kwargs));

            assert_eq!(config.update_instruments_interval_mins, None);
        });
    }

    #[test]
    fn direct_pyo3_constructor_uses_default_update_interval_when_omitted() {
        Python::initialize();
        Python::attach(|py| {
            let config = construct_data_client_config(py, None, None);

            assert_eq!(
                config.update_instruments_interval_mins,
                PolymarketDataClientConfig::default().update_instruments_interval_mins,
            );
        });
    }

    #[test]
    fn direct_pyo3_constructor_preserves_none_update_interval_for_positional_args() {
        Python::initialize();
        Python::attach(|py| {
            let args = PyTuple::new(
                py,
                [
                    py.None(),
                    py.None(),
                    py.None(),
                    py.None(),
                    py.None(),
                    py.None(),
                    py.None(),
                    py.None(),
                    py.None(),
                ],
            )
            .expect("args");

            let config = construct_data_client_config(py, Some(&args), None);

            assert_eq!(config.update_instruments_interval_mins, None);
        });
    }
}
