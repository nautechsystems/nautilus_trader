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

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;
use rust_decimal::Decimal;

use crate::{
    common::enums::{BinanceEnvironment, BinanceMarginType, BinanceProductType},
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceDataClientConfig {
    /// Configuration for Binance data client.
    ///
    /// Ed25519 API keys are required for SBE WebSocket streams.
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        api_key = None,
        api_secret = None,
        instrument_status_poll_secs = None,
    ))]
    fn py_new(
        product_types: Option<Vec<BinanceProductType>>,
        environment: Option<BinanceEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        instrument_status_poll_secs: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            product_types: product_types.unwrap_or(defaults.product_types),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http: base_url_http.or(defaults.base_url_http),
            base_url_ws: base_url_ws.or(defaults.base_url_ws),
            api_key: api_key.or(defaults.api_key),
            api_secret: api_secret.or(defaults.api_secret),
            instrument_status_poll_secs: instrument_status_poll_secs
                .unwrap_or(defaults.instrument_status_poll_secs),
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinanceExecClientConfig {
    /// Configuration for Binance execution client.
    ///
    /// Ed25519 API keys are required for execution clients. Binance deprecated
    /// listenKey-based user data streams in favor of WebSocket API authentication,
    /// which only supports Ed25519.
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        product_types = None,
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        base_url_ws_trading = None,
        use_ws_trading = true,
        use_position_ids = true,
        default_taker_fee = None,
        api_key = None,
        api_secret = None,
        futures_leverages = None,
        futures_margin_types = None,
        treat_expired_as_canceled = false,
        use_trade_lite = false,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        product_types: Option<Vec<BinanceProductType>>,
        environment: Option<BinanceEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        base_url_ws_trading: Option<String>,
        use_ws_trading: bool,
        use_position_ids: bool,
        default_taker_fee: Option<f64>,
        api_key: Option<String>,
        api_secret: Option<String>,
        futures_leverages: Option<HashMap<String, u32>>,
        futures_margin_types: Option<HashMap<String, BinanceMarginType>>,
        treat_expired_as_canceled: bool,
        use_trade_lite: bool,
    ) -> Self {
        let defaults = Self::default();
        Self {
            trader_id,
            account_id,
            product_types: product_types.unwrap_or(defaults.product_types),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http: base_url_http.or(defaults.base_url_http),
            base_url_ws: base_url_ws.or(defaults.base_url_ws),
            base_url_ws_trading: base_url_ws_trading.or(defaults.base_url_ws_trading),
            use_ws_trading,
            use_position_ids,
            default_taker_fee: default_taker_fee
                .map_or_else(|| Ok(defaults.default_taker_fee), Decimal::try_from)
                .unwrap_or(defaults.default_taker_fee),
            api_key: api_key.or(defaults.api_key),
            api_secret: api_secret.or(defaults.api_secret),
            futures_leverages,
            futures_margin_types,
            treat_expired_as_canceled,
            use_trade_lite,
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_data_client_py_new_uses_defaults_for_omitted_fields() {
        let config = BinanceDataClientConfig::py_new(None, None, None, None, None, None, None);
        let defaults = BinanceDataClientConfig::default();

        assert_eq!(config.product_types, defaults.product_types);
        assert_eq!(config.environment, defaults.environment);
        assert_eq!(config.base_url_http, defaults.base_url_http);
        assert_eq!(config.base_url_ws, defaults.base_url_ws);
        assert_eq!(config.api_key, defaults.api_key);
        assert_eq!(config.api_secret, defaults.api_secret);
        assert_eq!(
            config.instrument_status_poll_secs,
            defaults.instrument_status_poll_secs
        );
    }

    #[rstest]
    fn test_data_client_py_new_uses_explicit_overrides() {
        let config = BinanceDataClientConfig::py_new(
            Some(vec![BinanceProductType::UsdM]),
            Some(BinanceEnvironment::Testnet),
            Some("https://http.example".to_string()),
            Some("wss://ws.example".to_string()),
            Some("api-key".to_string()),
            Some("api-secret".to_string()),
            Some(15),
        );

        assert_eq!(config.product_types, vec![BinanceProductType::UsdM]);
        assert_eq!(config.environment, BinanceEnvironment::Testnet);
        assert_eq!(
            config.base_url_http.as_deref(),
            Some("https://http.example")
        );
        assert_eq!(config.base_url_ws.as_deref(), Some("wss://ws.example"));
        assert_eq!(config.api_key.as_deref(), Some("api-key"));
        assert_eq!(config.api_secret.as_deref(), Some("api-secret"));
        assert_eq!(config.instrument_status_poll_secs, 15);
    }

    #[rstest]
    fn test_exec_client_py_new_uses_defaults_for_optional_fields() {
        let trader_id = TraderId::from("TRADER-001");
        let account_id = AccountId::from("BINANCE-001");
        let config = BinanceExecClientConfig::py_new(
            trader_id, account_id, None, None, None, None, None, true, true, None, None, None,
            None, None, false, false,
        );
        let defaults = BinanceExecClientConfig::default();

        assert_eq!(config.trader_id, trader_id);
        assert_eq!(config.account_id, account_id);
        assert_eq!(config.product_types, defaults.product_types);
        assert_eq!(config.environment, defaults.environment);
        assert_eq!(config.base_url_http, defaults.base_url_http);
        assert_eq!(config.base_url_ws, defaults.base_url_ws);
        assert_eq!(config.base_url_ws_trading, defaults.base_url_ws_trading);
        assert_eq!(config.default_taker_fee, defaults.default_taker_fee);
        assert_eq!(config.api_key, defaults.api_key);
        assert_eq!(config.api_secret, defaults.api_secret);
        assert_eq!(config.futures_leverages, defaults.futures_leverages);
        assert_eq!(config.futures_margin_types, defaults.futures_margin_types);
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

        let config = BinanceExecClientConfig::py_new(
            TraderId::from("TRADER-002"),
            AccountId::from("BINANCE-002"),
            Some(vec![BinanceProductType::UsdM]),
            Some(BinanceEnvironment::Demo),
            Some("https://http.example".to_string()),
            Some("wss://stream.example".to_string()),
            Some("wss://trade.example".to_string()),
            false,
            false,
            Some(0.0015),
            Some("api-key".to_string()),
            Some("api-secret".to_string()),
            Some(leverages.clone()),
            Some(margin_types.clone()),
            true,
            true,
        );

        assert_eq!(config.product_types, vec![BinanceProductType::UsdM]);
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
        assert!(!config.use_position_ids);
        assert_eq!(config.default_taker_fee, Decimal::try_from(0.0015).unwrap());
        assert_eq!(config.api_key.as_deref(), Some("api-key"));
        assert_eq!(config.api_secret.as_deref(), Some("api-secret"));
        assert_eq!(config.futures_leverages, Some(leverages));
        assert_eq!(config.futures_margin_types, Some(margin_types));
        assert!(config.treat_expired_as_canceled);
        assert!(config.use_trade_lite);
    }

    #[rstest]
    fn test_exec_client_py_new_uses_default_fee_for_invalid_float() {
        let defaults = BinanceExecClientConfig::default();
        let config = BinanceExecClientConfig::py_new(
            TraderId::from("TRADER-003"),
            AccountId::from("BINANCE-003"),
            None,
            None,
            None,
            None,
            None,
            true,
            true,
            Some(f64::NAN),
            None,
            None,
            None,
            None,
            false,
            false,
        );

        assert_eq!(config.default_taker_fee, defaults.default_taker_fee);
    }
}
