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

//! Python bindings from `pyo3`.
//!
//! The Python v2 Polymarket boundary is configuration and factory registration.
//! Provider, data, and execution operations stay in Rust. Add Python here only
//! to expose Rust adapter types or register factories, not to run adapter logic.

#![expect(
    clippy::missing_errors_doc,
    reason = "errors documented on underlying Rust methods"
)]

pub mod config;
pub mod factories;
pub mod sort;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{data::ensure_rust_extractor_registered, identifiers::InstrumentId};
use nautilus_network::websocket::TransportBackend;
use nautilus_system::get_global_pyo3_registry;
use pyo3::{prelude::*, types::PyDict};

use crate::{
    common::consts::POLYMARKET,
    config::{
        PolymarketDataClientConfig, PolymarketExecClientConfig, PolymarketInstrumentProviderConfig,
        PolymarketUpDownEventSlugConfig,
    },
    data_types::{
        PolymarketRtdsCryptoPrice, PolymarketRtdsEquityPrice, register_polymarket_custom_data,
    },
    factories::{PolymarketDataClientFactory, PolymarketExecutionClientFactory},
};

fn getattr_optional<'py>(
    obj: &Bound<'py, PyAny>,
    name: &str,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    if !obj.hasattr(name)? {
        return Ok(None);
    }

    let value = obj.getattr(name)?;
    if value.is_none() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn getattr_optional_option_u64(
    obj: &Bound<'_, PyAny>,
    name: &str,
    default: Option<u64>,
) -> PyResult<Option<u64>> {
    if !obj.hasattr(name)? {
        return Ok(default);
    }

    let value = obj.getattr(name)?;
    if value.is_none() {
        Ok(None)
    } else {
        value.extract::<u64>().map(Some)
    }
}

fn py_scalar_to_string(value: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(v) = value.extract::<bool>() {
        return Ok(v.to_string().to_lowercase());
    }

    if let Ok(v) = value.extract::<i64>() {
        return Ok(v.to_string());
    }

    if let Ok(v) = value.extract::<u64>() {
        return Ok(v.to_string());
    }

    if let Ok(v) = value.extract::<f64>() {
        return Ok(v.to_string());
    }

    if let Ok(v) = value.extract::<String>() {
        return Ok(v);
    }

    value.str()?.extract()
}

fn extract_string_map(
    value: &Bound<'_, PyAny>,
) -> PyResult<std::collections::HashMap<String, String>> {
    let dict = value.cast::<PyDict>()?;
    let mut map = std::collections::HashMap::with_capacity(dict.len());
    for (key, value) in dict.iter() {
        map.insert(key.extract::<String>()?, py_scalar_to_string(&value)?);
    }
    Ok(map)
}

fn extract_event_slug_builder(
    value: &Bound<'_, PyAny>,
) -> PyResult<PolymarketUpDownEventSlugConfig> {
    if let Ok(builder) = value.extract::<PolymarketUpDownEventSlugConfig>() {
        return Ok(builder);
    }

    if value.extract::<String>().is_ok() {
        return Err(to_pyvalue_err(
            "Python callable event_slug_builder is not supported by the Rust Polymarket adapter; \
             pass event_slugs, market_slugs, or PolymarketUpDownEventSlugConfig",
        ));
    }

    Err(to_pyvalue_err(
        "event_slug_builder must be PolymarketUpDownEventSlugConfig",
    ))
}

fn extract_provider_config_from_pyobject(
    obj: &Bound<'_, PyAny>,
) -> PyResult<PolymarketInstrumentProviderConfig> {
    if let Ok(config) = obj.extract::<PolymarketInstrumentProviderConfig>() {
        return Ok(config);
    }

    let default = PolymarketInstrumentProviderConfig::default();
    let load_all = getattr_optional(obj, "load_all")?
        .map(|value| value.extract::<bool>())
        .transpose()?
        .unwrap_or(default.load_all);
    let load_ids = getattr_optional(obj, "load_ids")?
        .map(|value| value.extract::<Vec<InstrumentId>>())
        .transpose()?;
    let filters = getattr_optional(obj, "filters")?
        .map(|value| extract_string_map(&value))
        .transpose()?;
    let event_slugs = getattr_optional(obj, "event_slugs")?
        .map(|value| value.extract::<Vec<String>>())
        .transpose()?;
    let market_slugs = getattr_optional(obj, "market_slugs")?
        .map(|value| value.extract::<Vec<String>>())
        .transpose()?;
    let event_slug_builder = getattr_optional(obj, "event_slug_builder")?
        .map(|value| extract_event_slug_builder(&value))
        .transpose()?;
    let log_warnings = getattr_optional(obj, "log_warnings")?
        .map(|value| value.extract::<bool>())
        .transpose()?
        .unwrap_or(default.log_warnings);
    let use_gamma_markets = getattr_optional(obj, "use_gamma_markets")?
        .map(|value| value.extract::<bool>())
        .transpose()?
        .unwrap_or(default.use_gamma_markets);

    Ok(PolymarketInstrumentProviderConfig {
        load_all: load_all || event_slug_builder.is_some(),
        load_ids,
        filters,
        event_slugs,
        market_slugs,
        event_slug_builder,
        log_warnings,
        use_gamma_markets,
    })
}

fn extract_data_config_from_pyobject(
    py: Python<'_>,
    config: &Py<PyAny>,
) -> PyResult<PolymarketDataClientConfig> {
    if let Ok(config) = config.extract::<PolymarketDataClientConfig>(py) {
        return Ok(config);
    }

    let obj = config.bind(py);
    let default = PolymarketDataClientConfig::default();
    let instrument_config = getattr_optional(obj, "instrument_config")?
        .map(|value| extract_provider_config_from_pyobject(&value))
        .transpose()?;
    let base_url_http = getattr_optional(obj, "base_url_http")?
        .map(|value| value.extract::<String>())
        .transpose()?;
    let base_url_ws = getattr_optional(obj, "base_url_ws")?
        .map(|value| value.extract::<String>())
        .transpose()?;
    let base_url_rtds = getattr_optional(obj, "base_url_rtds")?
        .map(|value| value.extract::<String>())
        .transpose()?;
    let base_url_gamma = getattr_optional(obj, "base_url_gamma")?
        .map(|value| value.extract::<String>())
        .transpose()?;
    let base_url_data_api = getattr_optional(obj, "base_url_data_api")?
        .map(|value| value.extract::<String>())
        .transpose()?;
    let http_timeout_secs = getattr_optional(obj, "http_timeout_secs")?
        .map(|value| value.extract::<u64>())
        .transpose()?
        .unwrap_or(default.http_timeout_secs);
    let ws_timeout_secs = getattr_optional(obj, "ws_timeout_secs")?
        .map(|value| value.extract::<u64>())
        .transpose()?
        .unwrap_or(default.ws_timeout_secs);
    let ws_max_subscriptions = getattr_optional(obj, "ws_max_subscriptions")?
        .map(|value| value.extract::<usize>())
        .transpose()?
        .unwrap_or(default.ws_max_subscriptions);
    let update_instruments_interval_mins = getattr_optional_option_u64(
        obj,
        "update_instruments_interval_mins",
        default.update_instruments_interval_mins,
    )?;
    let subscribe_new_markets = getattr_optional(obj, "subscribe_new_markets")?
        .map(|value| value.extract::<bool>())
        .transpose()?
        .unwrap_or(default.subscribe_new_markets);
    let new_market_fetch_max_concurrency =
        getattr_optional(obj, "new_market_fetch_max_concurrency")?
            .map(|value| value.extract::<usize>())
            .transpose()?
            .unwrap_or(default.new_market_fetch_max_concurrency);
    let auto_load_missing_instruments = getattr_optional(obj, "auto_load_missing_instruments")?
        .map(|value| value.extract::<bool>())
        .transpose()?
        .unwrap_or(default.auto_load_missing_instruments);
    let auto_load_debounce_ms = getattr_optional(obj, "auto_load_debounce_ms")?
        .map(|value| value.extract::<u64>())
        .transpose()?
        .unwrap_or(default.auto_load_debounce_ms);
    let auto_load_max_retries = getattr_optional(obj, "auto_load_max_retries")?
        .map(|value| value.extract::<u32>())
        .transpose()?
        .unwrap_or(default.auto_load_max_retries);
    let auto_load_retry_delay_initial_secs =
        getattr_optional(obj, "auto_load_retry_delay_initial_secs")?
            .map(|value| value.extract::<f64>())
            .transpose()?
            .unwrap_or(default.auto_load_retry_delay_initial_secs);
    let auto_load_retry_delay_max_secs = getattr_optional(obj, "auto_load_retry_delay_max_secs")?
        .map(|value| value.extract::<f64>())
        .transpose()?
        .unwrap_or(default.auto_load_retry_delay_max_secs);
    let resolve_poll_enabled = getattr_optional(obj, "resolve_poll_enabled")?
        .map(|value| value.extract::<bool>())
        .transpose()?
        .unwrap_or(default.resolve_poll_enabled);
    let resolve_poll_interval_secs = getattr_optional(obj, "resolve_poll_interval_secs")?
        .map(|value| value.extract::<u64>())
        .transpose()?
        .unwrap_or(default.resolve_poll_interval_secs);
    let resolve_poll_grace_secs = getattr_optional(obj, "resolve_poll_grace_secs")?
        .map(|value| value.extract::<u64>())
        .transpose()?
        .unwrap_or(default.resolve_poll_grace_secs);
    let resolve_poll_max_wait_secs = getattr_optional(obj, "resolve_poll_max_wait_secs")?
        .map(|value| value.extract::<u64>())
        .transpose()?
        .unwrap_or(default.resolve_poll_max_wait_secs);
    let transport_backend = getattr_optional(obj, "transport_backend")?
        .map(|value| value.extract::<TransportBackend>())
        .transpose()?
        .unwrap_or(default.transport_backend);
    Ok(PolymarketDataClientConfig {
        instrument_config,
        base_url_http,
        base_url_ws,
        base_url_rtds,
        base_url_gamma,
        base_url_data_api,
        http_timeout_secs,
        ws_timeout_secs,
        ws_max_subscriptions,
        update_instruments_interval_mins,
        subscribe_new_markets,
        new_market_fetch_max_concurrency,
        auto_load_missing_instruments,
        auto_load_debounce_ms,
        auto_load_max_retries,
        auto_load_retry_delay_initial_secs,
        auto_load_retry_delay_max_secs,
        resolve_poll_enabled,
        resolve_poll_interval_secs,
        resolve_poll_grace_secs,
        resolve_poll_max_wait_secs,
        filters: Vec::new(),
        new_market_filter: None,
        transport_backend,
    })
}

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<PolymarketDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<PolymarketExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match extract_data_config_from_pyobject(py, &config) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<PolymarketExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketExecClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.polymarket`.
#[pymodule]
pub fn polymarket(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::common::enums::SignatureType>()?;
    m.add_class::<PolymarketUpDownEventSlugConfig>()?;
    m.add_class::<PolymarketInstrumentProviderConfig>()?;
    m.add_class::<PolymarketDataClientConfig>()?;
    m.add_class::<PolymarketExecClientConfig>()?;
    m.add_class::<PolymarketDataClientFactory>()?;
    m.add_class::<PolymarketExecutionClientFactory>()?;
    m.add_class::<PolymarketRtdsCryptoPrice>()?;
    m.add_class::<PolymarketRtdsEquityPrice>()?;
    m.add_function(pyo3::wrap_pyfunction!(
        sort::py_polymarket_trade_sort_key,
        m
    )?)?;
    m.add_function(pyo3::wrap_pyfunction!(sort::py_polymarket_trade_id, m)?)?;

    register_polymarket_custom_data();
    let _result = ensure_rust_extractor_registered::<PolymarketRtdsCryptoPrice>();
    let _result = ensure_rust_extractor_registered::<PolymarketRtdsEquityPrice>();

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(POLYMARKET.to_string(), extract_polymarket_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor(POLYMARKET.to_string(), extract_polymarket_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "PolymarketDataClientConfig".to_string(),
        extract_polymarket_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "PolymarketExecClientConfig".to_string(),
        extract_polymarket_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket exec config extractor: {e}"
        )));
    }

    Ok(())
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use std::sync::Arc;

    use nautilus_core::Params;
    use nautilus_model::{
        data::{CustomData, DataType, custom::CustomDataTrait, ensure_rust_extractor_registered},
        types::Price,
    };
    use pyo3::{prelude::*, types::PyDict};
    use rstest::rstest;
    use serde_json::json;

    use super::extract_data_config_from_pyobject;
    use crate::{
        config::PolymarketUpDownEventSlugConfig,
        data_types::{PolymarketRtdsCryptoPrice, register_polymarket_custom_data},
    };

    #[rstest]
    fn extract_data_config_supports_python_style_namespace() {
        Python::initialize();
        Python::attach(|py| {
            let types = py.import("types").expect("types");
            let namespace = types.getattr("SimpleNamespace").expect("SimpleNamespace");
            let event_slug_builder = Py::new(
                py,
                PolymarketUpDownEventSlugConfig {
                    assets: vec!["btc".to_string(), "eth".to_string()],
                    interval_mins: 5,
                    periods: 2,
                    start_offset_periods: 0,
                },
            )
            .expect("event slug builder should convert to Python object");

            let instrument_kwargs = PyDict::new(py);
            instrument_kwargs
                .set_item("event_slug_builder", event_slug_builder)
                .unwrap();
            instrument_kwargs
                .set_item("event_slugs", vec!["event-a", "event-b"])
                .unwrap();
            instrument_kwargs
                .set_item("market_slugs", vec!["market-a"])
                .unwrap();
            instrument_kwargs.set_item("load_all", false).unwrap();
            instrument_kwargs.set_item("log_warnings", false).unwrap();
            let instrument_config = namespace
                .call((), Some(&instrument_kwargs))
                .expect("instrument namespace");

            let config_kwargs = PyDict::new(py);
            config_kwargs
                .set_item("instrument_config", instrument_config)
                .unwrap();
            config_kwargs
                .set_item("update_instruments_interval_mins", 1)
                .unwrap();
            config_kwargs
                .set_item("subscribe_new_markets", false)
                .unwrap();
            config_kwargs
                .set_item("new_market_fetch_max_concurrency", 13)
                .unwrap();
            config_kwargs
                .set_item("base_url_gamma", "https://gamma.example")
                .unwrap();
            config_kwargs
                .set_item("base_url_rtds", "wss://ws-live-data.example")
                .unwrap();
            config_kwargs
                .set_item("base_url_data_api", "https://data.example")
                .unwrap();
            config_kwargs.set_item("ws_timeout_secs", 41).unwrap();
            config_kwargs.set_item("ws_max_subscriptions", 512).unwrap();
            config_kwargs
                .set_item("auto_load_missing_instruments", true)
                .unwrap();
            config_kwargs
                .set_item("auto_load_debounce_ms", 100)
                .unwrap();
            config_kwargs.set_item("auto_load_max_retries", 12).unwrap();
            config_kwargs
                .set_item("auto_load_retry_delay_initial_secs", 5.0)
                .unwrap();
            config_kwargs
                .set_item("auto_load_retry_delay_max_secs", 15.0)
                .unwrap();
            config_kwargs
                .set_item("resolve_poll_enabled", false)
                .unwrap();
            config_kwargs
                .set_item("resolve_poll_interval_secs", 45)
                .unwrap();
            config_kwargs
                .set_item("resolve_poll_grace_secs", 12)
                .unwrap();
            config_kwargs
                .set_item("resolve_poll_max_wait_secs", 2400)
                .unwrap();
            let config_obj = namespace
                .call((), Some(&config_kwargs))
                .expect("config namespace");

            let rust_config = extract_data_config_from_pyobject(py, &config_obj.unbind())
                .expect("extract rust config");
            let instrument_config = rust_config
                .instrument_config
                .expect("instrument_config should be extracted");

            assert!(
                instrument_config.load_all,
                "event_slug_builder should imply scoped load_all bootstrap"
            );
            let event_slug_builder = instrument_config
                .event_slug_builder
                .expect("event_slug_builder should be extracted");
            assert_eq!(
                event_slug_builder.assets,
                ["btc".to_string(), "eth".to_string()]
            );
            assert_eq!(event_slug_builder.interval_mins, 5);
            assert_eq!(event_slug_builder.periods, 2);
            assert_eq!(event_slug_builder.start_offset_periods, 0);
            assert_eq!(
                instrument_config.event_slugs.as_deref(),
                Some(&["event-a".to_string(), "event-b".to_string()][..])
            );
            assert_eq!(
                instrument_config.market_slugs.as_deref(),
                Some(&["market-a".to_string()][..])
            );
            assert!(!instrument_config.log_warnings);
            assert_eq!(rust_config.update_instruments_interval_mins, Some(1));
            assert!(!rust_config.subscribe_new_markets);
            assert_eq!(rust_config.new_market_fetch_max_concurrency, 13);
            assert_eq!(
                rust_config.base_url_gamma.as_deref(),
                Some("https://gamma.example")
            );
            assert_eq!(
                rust_config.base_url_rtds.as_deref(),
                Some("wss://ws-live-data.example")
            );
            assert_eq!(
                rust_config.base_url_data_api.as_deref(),
                Some("https://data.example")
            );
            assert_eq!(rust_config.ws_timeout_secs, 41);
            assert_eq!(rust_config.ws_max_subscriptions, 512);
            assert!(!rust_config.resolve_poll_enabled);
            assert_eq!(rust_config.resolve_poll_interval_secs, 45);
            assert_eq!(rust_config.resolve_poll_grace_secs, 12);
            assert_eq!(rust_config.resolve_poll_max_wait_secs, 2400);
        });
    }

    #[rstest]
    fn extract_data_config_rejects_python_callable_event_slug_builder() {
        Python::initialize();
        Python::attach(|py| {
            let types = py.import("types").expect("types");
            let namespace = types.getattr("SimpleNamespace").expect("SimpleNamespace");

            let instrument_kwargs = PyDict::new(py);
            instrument_kwargs
                .set_item("event_slug_builder", "pkg.module:build_event_slugs")
                .unwrap();
            let instrument_config = namespace
                .call((), Some(&instrument_kwargs))
                .expect("instrument namespace");

            let config_kwargs = PyDict::new(py);
            config_kwargs
                .set_item("instrument_config", instrument_config)
                .unwrap();
            let config_obj = namespace
                .call((), Some(&config_kwargs))
                .expect("config namespace");

            let err = extract_data_config_from_pyobject(py, &config_obj.unbind())
                .expect_err("Python callable event_slug_builder should be rejected");

            assert!(
                err.to_string()
                    .contains("Python callable event_slug_builder is not supported")
            );
        });
    }

    #[rstest]
    fn extract_data_config_preserves_none_update_interval() {
        Python::initialize();
        Python::attach(|py| {
            let types = py.import("types").expect("types");
            let namespace = types.getattr("SimpleNamespace").expect("SimpleNamespace");
            let config_kwargs = PyDict::new(py);
            config_kwargs
                .set_item("update_instruments_interval_mins", py.None())
                .unwrap();
            let config_obj = namespace
                .call((), Some(&config_kwargs))
                .expect("config namespace");

            let rust_config = extract_data_config_from_pyobject(py, &config_obj.unbind())
                .expect("extract rust config");

            assert_eq!(rust_config.update_instruments_interval_mins, None);
        });
    }

    #[rstest]
    fn custom_data_getter_unwraps_rtds_payload_to_python_class() {
        Python::initialize();
        Python::attach(|py| {
            register_polymarket_custom_data();
            let _result = ensure_rust_extractor_registered::<PolymarketRtdsCryptoPrice>();

            let mut metadata = Params::new();
            metadata.insert("symbol".to_string(), json!("btcusdt"));
            let payload = Arc::new(PolymarketRtdsCryptoPrice::new(
                "btcusdt".to_string(),
                Price::from("67234.50"),
                1_753_314_088_395,
                1_753_314_088_421,
                nautilus_core::UnixNanos::from_millis(1_753_314_088_395),
                nautilus_core::UnixNanos::from_millis(1_753_314_088_421),
            ));
            let custom = CustomData::new(
                payload,
                DataType::new(
                    PolymarketRtdsCryptoPrice::type_name_static(),
                    Some(metadata),
                    None,
                ),
            );

            let py_custom = Py::new(py, custom).expect("create Python CustomData");
            let py_payload = py_custom.bind(py).getattr("data").expect("CustomData.data");

            assert_eq!(
                py_payload.get_type().name().expect("type name"),
                "PolymarketRtdsCryptoPrice"
            );
            assert_eq!(
                py_payload
                    .getattr("symbol")
                    .expect("symbol")
                    .extract::<String>()
                    .expect("extract symbol"),
                "btcusdt"
            );
            assert_eq!(
                py_payload
                    .getattr("value")
                    .expect("value")
                    .str()
                    .expect("value str")
                    .to_string(),
                "67234.50"
            );
        });
    }
}
