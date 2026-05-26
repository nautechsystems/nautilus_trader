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
use pyo3::{prelude::*, pymethods, types::PyDict};

use crate::{
    common::enums::SignatureType,
    config::{
        PolymarketDataClientConfig, PolymarketExecClientConfig, PolymarketInstrumentProviderConfig,
    },
};

fn kwargs_optional<'py>(
    kwargs: Option<&Bound<'py, PyDict>>,
    name: &str,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let Some(kwargs) = kwargs else {
        return Ok(None);
    };

    kwargs.get_item(name)
}

fn kwargs_optional_option_u64(
    kwargs: Option<&Bound<'_, PyDict>>,
    name: &str,
    default: Option<u64>,
) -> PyResult<Option<u64>> {
    let Some(value) = kwargs_optional(kwargs, name)? else {
        return Ok(default);
    };

    if value.is_none() {
        Ok(None)
    } else {
        value.extract::<u64>().map(Some)
    }
}

fn reject_unknown_kwargs(kwargs: Option<&Bound<'_, PyDict>>, allowed: &[&str]) -> PyResult<()> {
    let Some(kwargs) = kwargs else {
        return Ok(());
    };

    for (key, _) in kwargs.iter() {
        let key = key.extract::<String>()?;
        if !allowed.contains(&key.as_str()) {
            return Err(pyo3::exceptions::PyTypeError::new_err(format!(
                "Unexpected keyword argument '{key}'",
            )));
        }
    }

    Ok(())
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PolymarketInstrumentProviderConfig {
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
    #[pyo3(signature = (**kwargs))]
    fn py_new(kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        reject_unknown_kwargs(
            kwargs,
            &[
                "instrument_config",
                "base_url_http",
                "base_url_ws",
                "base_url_gamma",
                "base_url_data_api",
                "http_timeout_secs",
                "ws_timeout_secs",
                "ws_max_subscriptions",
                "update_instruments_interval_mins",
                "subscribe_new_markets",
                "auto_load_missing_instruments",
                "auto_load_debounce_ms",
                "auto_load_max_retries",
                "auto_load_retry_delay_initial_secs",
                "auto_load_retry_delay_max_secs",
            ],
        )?;

        let default = Self::default();
        let instrument_config = match kwargs_optional(kwargs, "instrument_config")? {
            Some(value) => Some(value.extract::<PolymarketInstrumentProviderConfig>()?),
            None => None,
        };

        Ok(Self {
            instrument_config,
            base_url_http: kwargs_optional(kwargs, "base_url_http")?
                .map(|value| value.extract::<String>())
                .transpose()?,
            base_url_ws: kwargs_optional(kwargs, "base_url_ws")?
                .map(|value| value.extract::<String>())
                .transpose()?,
            base_url_gamma: kwargs_optional(kwargs, "base_url_gamma")?
                .map(|value| value.extract::<String>())
                .transpose()?,
            base_url_data_api: kwargs_optional(kwargs, "base_url_data_api")?
                .map(|value| value.extract::<String>())
                .transpose()?,
            http_timeout_secs: kwargs_optional(kwargs, "http_timeout_secs")?
                .map(|value| value.extract::<u64>())
                .transpose()?
                .unwrap_or(default.http_timeout_secs),
            ws_timeout_secs: kwargs_optional(kwargs, "ws_timeout_secs")?
                .map(|value| value.extract::<u64>())
                .transpose()?
                .unwrap_or(default.ws_timeout_secs),
            ws_max_subscriptions: kwargs_optional(kwargs, "ws_max_subscriptions")?
                .map(|value| value.extract::<usize>())
                .transpose()?
                .unwrap_or(default.ws_max_subscriptions),
            update_instruments_interval_mins: kwargs_optional_option_u64(
                kwargs,
                "update_instruments_interval_mins",
                default.update_instruments_interval_mins,
            )?,
            subscribe_new_markets: kwargs_optional(kwargs, "subscribe_new_markets")?
                .map(|value| value.extract::<bool>())
                .transpose()?
                .unwrap_or(default.subscribe_new_markets),
            auto_load_missing_instruments: kwargs_optional(
                kwargs,
                "auto_load_missing_instruments",
            )?
            .map(|value| value.extract::<bool>())
            .transpose()?
            .unwrap_or(default.auto_load_missing_instruments),
            auto_load_debounce_ms: kwargs_optional(kwargs, "auto_load_debounce_ms")?
                .map(|value| value.extract::<u64>())
                .transpose()?
                .unwrap_or(default.auto_load_debounce_ms),
            auto_load_max_retries: kwargs_optional(kwargs, "auto_load_max_retries")?
                .map(|value| value.extract::<u32>())
                .transpose()?
                .unwrap_or(default.auto_load_max_retries),
            auto_load_retry_delay_initial_secs: kwargs_optional(
                kwargs,
                "auto_load_retry_delay_initial_secs",
            )?
            .map(|value| value.extract::<f64>())
            .transpose()?
            .unwrap_or(default.auto_load_retry_delay_initial_secs),
            auto_load_retry_delay_max_secs: kwargs_optional(
                kwargs,
                "auto_load_retry_delay_max_secs",
            )?
            .map(|value| value.extract::<f64>())
            .transpose()?
            .unwrap_or(default.auto_load_retry_delay_max_secs),
            filters: Vec::new(),
            new_market_filter: None,
            transport_backend: default.transport_backend,
        })
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
    use pyo3::{Python, types::PyDict};

    use super::*;

    #[test]
    fn direct_pyo3_constructor_preserves_none_update_interval() {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::new(py, "polymarket").expect("module");
            crate::python::polymarket(py, &module).expect("polymarket module");
            let cls = module
                .getattr("PolymarketDataClientConfig")
                .expect("PolymarketDataClientConfig");

            let kwargs = PyDict::new(py);
            kwargs
                .set_item("update_instruments_interval_mins", py.None())
                .unwrap();

            let config = cls
                .call((), Some(&kwargs))
                .expect("construct PolymarketDataClientConfig")
                .extract::<PolymarketDataClientConfig>()
                .expect("extract PolymarketDataClientConfig");

            assert_eq!(config.update_instruments_interval_mins, None);
        });
    }
}
