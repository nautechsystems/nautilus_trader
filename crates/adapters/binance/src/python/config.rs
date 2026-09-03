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

//! Python bindings for Binance configuration.

use std::collections::HashMap;

use nautilus_core::{python::to_pyvalue_err, string::secret::SecretString};
use nautilus_model::{enums::OmsType, identifiers::AccountId, types::Currency};
use nautilus_network::websocket::TransportBackend;
use pyo3::{
    prelude::*,
    types::{PyDict, PyDictMethods},
};
use rust_decimal::Decimal;

use crate::{
    common::enums::{BinanceEnvironment, BinanceMarginType, BinanceProductType},
    config::{
        BinanceDataClientConfig, BinanceExecutionClientConfig, BinanceInstrumentProviderConfig,
        BinanceSpotMarketDataMode,
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceInstrumentProviderConfig {
    /// Configuration for Binance instrument loading.
    #[new]
    #[pyo3(signature = (
        load_all = true,
        load_ids = None,
        filters = None,
        filter_callable = None,
        log_warnings = true,
        query_commission_rates = false,
    ))]
    fn py_new(
        load_all: bool,
        load_ids: Option<Vec<String>>,
        filters: Option<HashMap<String, Py<PyAny>>>,
        filter_callable: Option<String>,
        log_warnings: bool,
        query_commission_rates: bool,
    ) -> PyResult<Self> {
        let filters = filters
            .map(nautilus_live::python::config::coerce_json_config)
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            load_all,
            load_ids,
            filters,
            filter_callable,
            log_warnings,
            query_commission_rates,
        })
    }

    fn __repr__(&self) -> String {
        stringify!(BinanceInstrumentProviderConfig).to_string()
    }

    #[getter]
    fn load_all(&self) -> bool {
        self.load_all
    }

    #[getter]
    fn load_ids(&self) -> Option<Vec<String>> {
        self.load_ids.clone()
    }

    #[getter]
    fn filters(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.filters {
            dict.set_item(
                key,
                nautilus_live::python::config::json_value_to_py(py, value)?,
            )?;
        }
        Ok(dict.into_any().unbind())
    }

    #[getter]
    fn filter_callable(&self) -> Option<String> {
        self.filter_callable.clone()
    }

    #[getter]
    fn log_warnings(&self) -> bool {
        self.log_warnings
    }

    #[getter]
    fn query_commission_rates(&self) -> bool {
        self.query_commission_rates
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceDataClientConfig {
    /// Configuration for Binance data client.
    ///
    /// Ed25519 API keys are required for SBE WebSocket streams.
    #[new]
    #[pyo3(signature = (
        product_type = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        api_key = None,
        api_secret = None,
        spot_market_data_mode = None,
        instrument_provider = None,
        instrument_refresh_interval_secs = None,
        instrument_status_poll_secs = None,
        proxy_url = None,
        recv_window_ms = None,
        us = false,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        product_type: Option<BinanceProductType>,
        environment: Option<BinanceEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        spot_market_data_mode: Option<BinanceSpotMarketDataMode>,
        instrument_provider: Option<BinanceInstrumentProviderConfig>,
        instrument_refresh_interval_secs: Option<u64>,
        instrument_status_poll_secs: Option<u64>,
        proxy_url: Option<String>,
        recv_window_ms: Option<u64>,
        us: bool,
        transport_backend: Option<TransportBackend>,
    ) -> PyResult<Self> {
        let defaults = Self::default();
        let config = Self {
            product_type: product_type.unwrap_or(defaults.product_type),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http: base_url_http.or(defaults.base_url_http),
            base_url_ws: base_url_ws.or(defaults.base_url_ws),
            api_key: api_key.map(SecretString::from).or(defaults.api_key),
            api_secret: api_secret.map(SecretString::from).or(defaults.api_secret),
            spot_market_data_mode: spot_market_data_mode.unwrap_or(defaults.spot_market_data_mode),
            instrument_provider: instrument_provider.unwrap_or(defaults.instrument_provider),
            instrument_refresh_interval_secs: instrument_refresh_interval_secs
                .unwrap_or(defaults.instrument_refresh_interval_secs),
            instrument_status_poll_secs: instrument_status_poll_secs
                .unwrap_or(defaults.instrument_status_poll_secs),
            proxy_url: proxy_url.map(SecretString::from).or(defaults.proxy_url),
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            us,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        };
        config.validate().map_err(to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(BinanceDataClientConfig).to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceExecutionClientConfig {
    /// Configuration for Binance execution client.
    ///
    /// Global execution uses WebSocket API authentication with Ed25519 credentials.
    /// Binance US uses HMAC-signed HTTP requests and listen-key user data streams.
    #[new]
    #[pyo3(signature = (
        account_id,
        product_type = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        base_url_ws_trading = None,
        use_ws_trading = true,
        ws_trading_setup_timeout_ms = None,
        instrument_provider = None,
        instrument_refresh_interval_secs = None,
        use_gtd = true,
        use_position_ids = true,
        oms_type = None,
        default_taker_fee = None,
        proxy_url = None,
        recv_window_ms = None,
        us = false,
        api_key = None,
        api_secret = None,
        futures_leverages = None,
        futures_margin_types = None,
        treat_expired_as_canceled = false,
        use_trade_lite = false,
        bnfcr_currency = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        account_id: AccountId,
        product_type: Option<BinanceProductType>,
        environment: Option<BinanceEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        base_url_ws_trading: Option<String>,
        use_ws_trading: bool,
        ws_trading_setup_timeout_ms: Option<u64>,
        instrument_provider: Option<BinanceInstrumentProviderConfig>,
        instrument_refresh_interval_secs: Option<u64>,
        use_gtd: bool,
        use_position_ids: bool,
        oms_type: Option<OmsType>,
        default_taker_fee: Option<f64>,
        proxy_url: Option<String>,
        recv_window_ms: Option<u64>,
        us: bool,
        api_key: Option<String>,
        api_secret: Option<String>,
        futures_leverages: Option<HashMap<String, u32>>,
        futures_margin_types: Option<HashMap<String, BinanceMarginType>>,
        treat_expired_as_canceled: bool,
        use_trade_lite: bool,
        bnfcr_currency: Option<Currency>,
        transport_backend: Option<TransportBackend>,
    ) -> PyResult<Self> {
        let defaults = Self::default();
        let config = Self {
            account_id,
            product_type: product_type.unwrap_or(defaults.product_type),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http: base_url_http.or(defaults.base_url_http),
            base_url_ws: base_url_ws.or(defaults.base_url_ws),
            base_url_ws_trading: base_url_ws_trading.or(defaults.base_url_ws_trading),
            use_ws_trading,
            ws_trading_setup_timeout_ms: ws_trading_setup_timeout_ms
                .unwrap_or(defaults.ws_trading_setup_timeout_ms),
            instrument_provider: instrument_provider.unwrap_or(defaults.instrument_provider),
            instrument_refresh_interval_secs: instrument_refresh_interval_secs
                .unwrap_or(defaults.instrument_refresh_interval_secs),
            use_gtd,
            use_position_ids,
            oms_type,
            default_taker_fee: default_taker_fee
                .map_or_else(|| Ok(defaults.default_taker_fee), Decimal::try_from)
                .unwrap_or(defaults.default_taker_fee),
            proxy_url: proxy_url.map(SecretString::from).or(defaults.proxy_url),
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            us,
            api_key: api_key.map(SecretString::from).or(defaults.api_key),
            api_secret: api_secret.map(SecretString::from).or(defaults.api_secret),
            futures_leverages,
            futures_margin_types,
            bnfcr_currency: bnfcr_currency.unwrap_or(defaults.bnfcr_currency),
            treat_expired_as_canceled,
            use_trade_lite,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        };
        config.validate().map_err(to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(BinanceExecutionClientConfig).to_string()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_data_client_py_new_uses_defaults_for_omitted_fields() {
        let config = BinanceDataClientConfig::py_new(
            None, None, None, None, None, None, None, None, None, None, None, None, false, None,
        )
        .unwrap();
        let defaults = BinanceDataClientConfig::default();

        assert_eq!(config.product_type, defaults.product_type);
        assert_eq!(config.environment, defaults.environment);
        assert_eq!(config.base_url_http, defaults.base_url_http);
        assert_eq!(config.base_url_ws, defaults.base_url_ws);
        assert_eq!(config.api_key, defaults.api_key);
        assert_eq!(config.api_secret, defaults.api_secret);
        assert_eq!(config.spot_market_data_mode, defaults.spot_market_data_mode);
        assert_eq!(config.instrument_provider, defaults.instrument_provider);
        assert_eq!(
            config.instrument_refresh_interval_secs,
            defaults.instrument_refresh_interval_secs
        );
        assert_eq!(
            config.instrument_status_poll_secs,
            defaults.instrument_status_poll_secs
        );
        assert_eq!(config.proxy_url, defaults.proxy_url);
        assert_eq!(config.recv_window_ms, defaults.recv_window_ms);
        assert!(!config.us);
    }

    #[rstest]
    fn test_data_client_py_new_uses_explicit_overrides() {
        let config = BinanceDataClientConfig::py_new(
            Some(BinanceProductType::UsdM),
            Some(BinanceEnvironment::Testnet),
            Some("https://http.example".to_string()),
            Some("wss://ws.example".to_string()),
            Some("api-key".to_string()),
            Some("api-secret".to_string()),
            Some(BinanceSpotMarketDataMode::Json),
            None,
            Some(30),
            Some(15),
            Some("http://proxy.example:8080".to_string()),
            Some(45_000),
            false,
            None,
        )
        .unwrap();

        assert_eq!(config.product_type, BinanceProductType::UsdM);
        assert_eq!(config.environment, BinanceEnvironment::Testnet);
        assert_eq!(
            config.base_url_http.as_deref(),
            Some("https://http.example")
        );
        assert_eq!(config.base_url_ws.as_deref(), Some("wss://ws.example"));
        assert_eq!(
            config.api_key.as_ref().map(SecretString::expose_secret),
            Some("api-key"),
        );
        assert_eq!(
            config.api_secret.as_ref().map(SecretString::expose_secret),
            Some("api-secret"),
        );
        assert_eq!(
            config.spot_market_data_mode,
            BinanceSpotMarketDataMode::Json
        );
        assert_eq!(config.instrument_refresh_interval_secs, 30);
        assert_eq!(config.instrument_status_poll_secs, 15);
        assert_eq!(
            config.proxy_url.as_ref().map(SecretString::expose_secret),
            Some("http://proxy.example:8080")
        );
        assert_eq!(config.recv_window_ms, 45_000);
    }

    #[rstest]
    fn test_exec_client_py_new_uses_defaults_for_optional_fields() {
        let account_id = AccountId::from("BINANCE-001");
        let config = BinanceExecutionClientConfig::py_new(
            account_id, None, None, None, None, None, true, None, None, None, true, true, None,
            None, None, None, false, None, None, None, None, false, false, None, None,
        )
        .unwrap();
        let defaults = BinanceExecutionClientConfig::default();

        assert_eq!(config.account_id, account_id);
        assert_eq!(config.product_type, defaults.product_type);
        assert_eq!(config.environment, defaults.environment);
        assert_eq!(config.base_url_http, defaults.base_url_http);
        assert_eq!(config.base_url_ws, defaults.base_url_ws);
        assert_eq!(config.base_url_ws_trading, defaults.base_url_ws_trading);
        assert!(config.use_ws_trading);
        assert_eq!(config.ws_trading_setup_timeout_ms, 10_000);
        assert_eq!(config.instrument_provider, defaults.instrument_provider);
        assert_eq!(
            config.instrument_refresh_interval_secs,
            defaults.instrument_refresh_interval_secs
        );
        assert!(config.use_gtd);
        assert_eq!(config.oms_type, defaults.oms_type);
        assert_eq!(config.default_taker_fee, defaults.default_taker_fee);
        assert_eq!(config.proxy_url, defaults.proxy_url);
        assert_eq!(config.recv_window_ms, defaults.recv_window_ms);
        assert!(!config.us);
        assert_eq!(config.api_key, defaults.api_key);
        assert_eq!(config.api_secret, defaults.api_secret);
        assert_eq!(config.futures_leverages, defaults.futures_leverages);
        assert_eq!(config.futures_margin_types, defaults.futures_margin_types);
        assert_eq!(config.bnfcr_currency, defaults.bnfcr_currency);
        assert_eq!(config.bnfcr_currency, Currency::USDT());
        assert_eq!(
            config.treat_expired_as_canceled,
            defaults.treat_expired_as_canceled
        );
    }

    #[rstest]
    fn test_exec_client_py_new_preserves_explicit_overrides() {
        use std::collections::HashMap;

        use crate::common::enums::BinanceMarginType;

        let leverages = HashMap::from([("BTCUSDT".to_string(), 20)]);
        let margin_types = HashMap::from([("BTCUSDT".to_string(), BinanceMarginType::Cross)]);

        let config = BinanceExecutionClientConfig::py_new(
            AccountId::from("BINANCE-002"),
            Some(BinanceProductType::UsdM),
            Some(BinanceEnvironment::Demo),
            Some("https://http.example".to_string()),
            Some("wss://stream.example".to_string()),
            Some("wss://trade.example".to_string()),
            false,
            Some(250),
            None,
            Some(45),
            false,
            false,
            Some(OmsType::Hedging),
            Some(0.0015),
            Some("http://proxy.example:8080".to_string()),
            Some(60_000),
            false,
            Some("api-key".to_string()),
            Some("api-secret".to_string()),
            Some(leverages.clone()),
            Some(margin_types.clone()),
            true,
            true,
            Some(Currency::USDC()),
            None,
        )
        .unwrap();

        assert_eq!(config.product_type, BinanceProductType::UsdM);
        assert_eq!(config.environment, BinanceEnvironment::Demo);
        assert_eq!(
            config.base_url_http.as_deref(),
            Some("https://http.example")
        );
        assert_eq!(config.base_url_ws.as_deref(), Some("wss://stream.example"));
        assert_eq!(
            config.base_url_ws_trading.as_deref(),
            Some("wss://trade.example")
        );
        assert!(!config.use_ws_trading);
        assert_eq!(config.ws_trading_setup_timeout_ms, 250);
        assert_eq!(config.instrument_refresh_interval_secs, 45);
        assert!(!config.use_gtd);
        assert!(!config.use_position_ids);
        assert_eq!(config.oms_type, Some(OmsType::Hedging));
        assert_eq!(config.default_taker_fee, Decimal::try_from(0.0015).unwrap());
        assert_eq!(
            config.proxy_url.as_ref().map(SecretString::expose_secret),
            Some("http://proxy.example:8080")
        );
        assert_eq!(config.recv_window_ms, 60_000);
        assert_eq!(
            config.api_key.as_ref().map(SecretString::expose_secret),
            Some("api-key"),
        );
        assert_eq!(
            config.api_secret.as_ref().map(SecretString::expose_secret),
            Some("api-secret"),
        );
        assert_eq!(config.futures_leverages, Some(leverages));
        assert_eq!(config.futures_margin_types, Some(margin_types));
        assert_eq!(config.bnfcr_currency, Currency::USDC());
        assert!(config.treat_expired_as_canceled);
        assert!(config.use_trade_lite);
    }

    #[rstest]
    fn test_exec_client_py_new_uses_default_fee_for_invalid_float() {
        let defaults = BinanceExecutionClientConfig::default();
        let config = BinanceExecutionClientConfig::py_new(
            AccountId::from("BINANCE-003"),
            None,
            None,
            None,
            None,
            None,
            true,
            None,
            None,
            None,
            true,
            true,
            None,
            Some(f64::NAN),
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.default_taker_fee, defaults.default_taker_fee);
    }

    #[rstest]
    fn test_instrument_provider_py_new_preserves_filters() {
        Python::initialize();
        Python::attach(|py| {
            let symbols = vec!["BTCUSDT", "ETHUSDT"].into_pyobject(py).unwrap();
            let filters = HashMap::from([("symbols".to_string(), symbols.into_any().unbind())]);

            let config = BinanceInstrumentProviderConfig::py_new(
                false,
                Some(vec!["BTCUSDT.BINANCE".to_string()]),
                Some(filters),
                None,
                false,
                true,
            )
            .unwrap();

            assert!(!config.load_all);
            assert_eq!(config.load_ids, Some(vec!["BTCUSDT.BINANCE".to_string()]));
            assert_eq!(
                config.filters["symbols"],
                serde_json::json!(["BTCUSDT", "ETHUSDT"])
            );
            assert!(!config.log_warnings);
            assert!(config.query_commission_rates);
        });
    }
}
