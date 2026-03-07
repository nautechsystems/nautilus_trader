// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use crate::common::{
    enums::{BitgetEnvironment, BitgetProductType},
    signing::ws_login_sign_base64,
    urls::{get_ws_private_url, get_ws_public_url},
};
use crate::websocket::messages::{
    BitgetWsAccountArg, BitgetWsAccountSubscriptionMessage, BitgetWsArg, BitgetWsLoginArg,
    BitgetWsLoginMessage, BitgetWsSubscriptionMessage,
};
use nautilus_network::websocket::WebSocketConfig;
#[cfg(feature = "python")]
use crate::websocket::parse::{
    parse_public_bars, parse_public_candle, parse_public_funding_rate, parse_public_index_price,
    parse_public_mark_price, parse_public_quote_tick, parse_public_ticker, parse_public_trade_tick,
    parse_public_trades,
};
#[cfg(feature = "python")]
use nautilus_core::UnixNanos;
#[cfg(feature = "python")]
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
#[cfg(feature = "python")]
use nautilus_model::{
    data::Data,
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
#[cfg(feature = "python")]
use pyo3::IntoPyObjectExt;
#[cfg(feature = "python")]
use pyo3::prelude::*;

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget")
)]
pub struct BitgetWebSocketClient {
    pub environment: BitgetEnvironment,
    pub public_url: String,
    pub private_url: String,
}

impl BitgetWebSocketClient {
    const DEFAULT_HEARTBEAT_SECS: u64 = 30;
    const DEFAULT_RECONNECT_TIMEOUT_MS: u64 = 10_000;
    const DEFAULT_RECONNECT_DELAY_INITIAL_MS: u64 = 2_000;
    const DEFAULT_RECONNECT_DELAY_MAX_MS: u64 = 30_000;

    #[must_use]
    pub fn new(environment: BitgetEnvironment) -> Self {
        Self {
            environment,
            public_url: get_ws_public_url(environment).to_string(),
            private_url: get_ws_private_url(environment).to_string(),
        }
    }

    #[must_use]
    fn subscription_message(
        op: &str,
        inst_type: BitgetProductType,
        channel: &str,
        inst_id: &str,
    ) -> String {
        let message = BitgetWsSubscriptionMessage {
            op: op.to_string(),
            args: vec![BitgetWsArg {
                inst_type: inst_type.as_api_str().to_string(),
                channel: channel.to_string(),
                inst_id: inst_id.to_string(),
            }],
        };

        ::serde_json::to_string(&message).expect("bitget subscribe message should serialize")
    }

    #[must_use]
    pub fn subscribe_message(inst_type: BitgetProductType, channel: &str, inst_id: &str) -> String {
        Self::subscription_message("subscribe", inst_type, channel, inst_id)
    }

    #[must_use]
    pub fn unsubscribe_message(inst_type: BitgetProductType, channel: &str, inst_id: &str) -> String {
        Self::subscription_message("unsubscribe", inst_type, channel, inst_id)
    }

    #[must_use]
    pub fn subscribe_ticker_message(inst_type: BitgetProductType, inst_id: &str) -> String {
        Self::subscribe_message(inst_type, "ticker", inst_id)
    }

    #[must_use]
    pub fn unsubscribe_ticker_message(inst_type: BitgetProductType, inst_id: &str) -> String {
        Self::unsubscribe_message(inst_type, "ticker", inst_id)
    }

    #[must_use]
    pub fn subscribe_candle_message(
        inst_type: BitgetProductType,
        interval: &str,
        inst_id: &str,
    ) -> String {
        let channel = if interval.starts_with("candle") {
            interval.to_string()
        } else {
            format!("candle{interval}")
        };
        Self::subscribe_message(inst_type, &channel, inst_id)
    }

    #[must_use]
    pub fn unsubscribe_candle_message(
        inst_type: BitgetProductType,
        interval: &str,
        inst_id: &str,
    ) -> String {
        let channel = if interval.starts_with("candle") {
            interval.to_string()
        } else {
            format!("candle{interval}")
        };
        Self::unsubscribe_message(inst_type, &channel, inst_id)
    }

    #[must_use]
    pub fn subscribe_account_message(inst_type: BitgetProductType, coin: &str) -> String {
        let message = BitgetWsAccountSubscriptionMessage {
            op: "subscribe".to_string(),
            args: vec![BitgetWsAccountArg {
                inst_type: inst_type.as_api_str().to_string(),
                channel: "account".to_string(),
                coin: coin.to_string(),
            }],
        };

        ::serde_json::to_string(&message)
            .expect("bitget account subscribe message should serialize")
    }

    #[must_use]
    pub fn ping_message() -> &'static str {
        "ping"
    }

    #[must_use]
    pub fn websocket_config(
        &self,
        base_url: Option<String>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> WebSocketConfig {
        WebSocketConfig {
            url: base_url.unwrap_or_else(|| self.public_url.clone()),
            headers: Vec::new(),
            heartbeat: Some(Self::DEFAULT_HEARTBEAT_SECS),
            heartbeat_msg: Some(Self::ping_message().to_string()),
            reconnect_timeout_ms: Some(Self::DEFAULT_RECONNECT_TIMEOUT_MS),
            reconnect_delay_initial_ms: Some(
                retry_delay_initial_ms
                    .unwrap_or(Self::DEFAULT_RECONNECT_DELAY_INITIAL_MS),
            ),
            reconnect_delay_max_ms: Some(retry_delay_max_ms.unwrap_or(Self::DEFAULT_RECONNECT_DELAY_MAX_MS)),
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
        }
    }

    #[must_use]
    pub fn login_message(
        api_key: &str,
        passphrase: &str,
        secret: &str,
        timestamp_ms: i64,
    ) -> String {
        let message = BitgetWsLoginMessage {
            op: "login".to_string(),
            args: vec![BitgetWsLoginArg {
                api_key: api_key.to_string(),
                passphrase: passphrase.to_string(),
                timestamp: timestamp_ms.to_string(),
                sign: ws_login_sign_base64(secret, timestamp_ms),
            }],
        };

        ::serde_json::to_string(&message).expect("bitget login message should serialize")
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BitgetWebSocketClient {
    #[new]
    fn py_new(environment: BitgetEnvironment) -> Self {
        Self::new(environment)
    }

    #[staticmethod]
    #[pyo3(name = "subscribe_message")]
    fn py_subscribe_message(inst_type: BitgetProductType, channel: &str, inst_id: &str) -> String {
        Self::subscribe_message(inst_type, channel, inst_id)
    }

    #[staticmethod]
    #[pyo3(name = "unsubscribe_message")]
    fn py_unsubscribe_message(
        inst_type: BitgetProductType,
        channel: &str,
        inst_id: &str,
    ) -> String {
        Self::unsubscribe_message(inst_type, channel, inst_id)
    }

    #[staticmethod]
    #[pyo3(name = "subscribe_ticker_message")]
    fn py_subscribe_ticker_message(inst_type: BitgetProductType, inst_id: &str) -> String {
        Self::subscribe_ticker_message(inst_type, inst_id)
    }

    #[staticmethod]
    #[pyo3(name = "unsubscribe_ticker_message")]
    fn py_unsubscribe_ticker_message(inst_type: BitgetProductType, inst_id: &str) -> String {
        Self::unsubscribe_ticker_message(inst_type, inst_id)
    }

    #[staticmethod]
    #[pyo3(name = "subscribe_candle_message")]
    fn py_subscribe_candle_message(inst_type: BitgetProductType, interval: &str, inst_id: &str) -> String {
        Self::subscribe_candle_message(inst_type, interval, inst_id)
    }

    #[staticmethod]
    #[pyo3(name = "unsubscribe_candle_message")]
    fn py_unsubscribe_candle_message(
        inst_type: BitgetProductType,
        interval: &str,
        inst_id: &str,
    ) -> String {
        Self::unsubscribe_candle_message(inst_type, interval, inst_id)
    }

    #[staticmethod]
    #[pyo3(name = "subscribe_account_message")]
    fn py_subscribe_account_message(inst_type: BitgetProductType, coin: &str) -> String {
        Self::subscribe_account_message(inst_type, coin)
    }

    #[staticmethod]
    #[pyo3(name = "ping_message")]
    fn py_ping_message() -> &'static str {
        Self::ping_message()
    }

    #[pyo3(name = "public_url")]
    #[must_use]
    fn py_public_url(&self) -> &str {
        &self.public_url
    }

    #[pyo3(name = "websocket_config")]
    #[pyo3(signature = (base_url = None, retry_delay_initial_ms = None, retry_delay_max_ms = None))]
    fn py_websocket_config(
        &self,
        base_url: Option<String>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> WebSocketConfig {
        self.websocket_config(base_url, retry_delay_initial_ms, retry_delay_max_ms)
    }

    #[staticmethod]
    #[pyo3(name = "login_message")]
    fn py_login_message(
        api_key: &str,
        passphrase: &str,
        secret: &str,
        timestamp_ms: i64,
    ) -> String {
        Self::login_message(api_key, passphrase, secret, timestamp_ms)
    }

    #[staticmethod]
    #[pyo3(name = "parse_trade_ticks")]
    #[pyo3(signature = (input, instrument, ts_init = None))]
    fn py_parse_trade_ticks(
        py: Python<'_>,
        input: &str,
        instrument: Py<PyAny>,
        ts_init: Option<u64>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let msg = parse_public_trades(input).map_err(to_pyvalue_err)?;
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let ts_init = nautilus_core::nanos::UnixNanos::from(ts_init.unwrap_or_default());

        msg.data
            .iter()
            .map(|trade| {
                let tick =
                    parse_public_trade_tick(trade, &instrument, ts_init).map_err(to_pyruntime_err)?;
                Ok(data_to_pycapsule(py, Data::Trade(tick)))
            })
            .collect()
    }

    #[staticmethod]
    #[pyo3(name = "parse_quote_ticks")]
    #[pyo3(signature = (input, instrument, ts_init = None))]
    fn py_parse_quote_ticks(
        py: Python<'_>,
        input: &str,
        instrument: Py<PyAny>,
        ts_init: Option<u64>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let msg = parse_public_ticker(input).map_err(to_pyvalue_err)?;
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let ts_init = UnixNanos::from(ts_init.unwrap_or_default());

        msg.data
            .iter()
            .map(|ticker| {
                let tick = parse_public_quote_tick(&instrument, &msg.arg, ticker, ts_init)
                    .map_err(to_pyruntime_err)?;
                Ok(data_to_pycapsule(py, Data::Quote(tick)))
            })
            .collect()
    }

    #[staticmethod]
    #[pyo3(name = "parse_mark_prices")]
    #[pyo3(signature = (input, instrument, ts_init = None))]
    fn py_parse_mark_prices(
        py: Python<'_>,
        input: &str,
        instrument: Py<PyAny>,
        ts_init: Option<u64>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let msg = parse_public_ticker(input).map_err(to_pyvalue_err)?;
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let ts_init = UnixNanos::from(ts_init.unwrap_or_default());

        msg.data
            .iter()
            .map(|ticker| {
                let mark_price =
                    parse_public_mark_price(&instrument, &msg.arg, ticker, ts_init).map_err(to_pyruntime_err)?;
                Ok(data_to_pycapsule(py, Data::MarkPriceUpdate(mark_price)))
            })
            .collect()
    }

    #[staticmethod]
    #[pyo3(name = "parse_index_prices")]
    #[pyo3(signature = (input, instrument, ts_init = None))]
    fn py_parse_index_prices(
        py: Python<'_>,
        input: &str,
        instrument: Py<PyAny>,
        ts_init: Option<u64>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let msg = parse_public_ticker(input).map_err(to_pyvalue_err)?;
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let ts_init = UnixNanos::from(ts_init.unwrap_or_default());

        msg.data
            .iter()
            .map(|ticker| {
                let index_price =
                    parse_public_index_price(&instrument, &msg.arg, ticker, ts_init).map_err(to_pyruntime_err)?;
                Ok(data_to_pycapsule(py, Data::IndexPriceUpdate(index_price)))
            })
            .collect()
    }

    #[staticmethod]
    #[pyo3(name = "parse_funding_rates")]
    #[pyo3(signature = (input, instrument, ts_init = None))]
    fn py_parse_funding_rates(
        py: Python<'_>,
        input: &str,
        instrument: Py<PyAny>,
        ts_init: Option<u64>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let msg = parse_public_ticker(input).map_err(to_pyvalue_err)?;
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let ts_init = UnixNanos::from(ts_init.unwrap_or_default());

        msg.data
            .iter()
            .map(|ticker| {
                let funding_rate = parse_public_funding_rate(
                    &instrument,
                    &msg.arg,
                    ticker,
                    ts_init,
                )
                .map_err(to_pyruntime_err)?;
                funding_rate.into_py_any(py).map_err(to_pyruntime_err)
            })
            .collect()
    }

    #[staticmethod]
    #[pyo3(name = "parse_bars")]
    #[pyo3(signature = (input, instrument, ts_init = None))]
    fn py_parse_bars(
        py: Python<'_>,
        input: &str,
        instrument: Py<PyAny>,
        ts_init: Option<u64>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let msg = parse_public_candle(input).map_err(to_pyvalue_err)?;
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let ts_init = UnixNanos::from(ts_init.unwrap_or_default());
        let bars = parse_public_bars(&msg, &instrument, ts_init).map_err(to_pyruntime_err)?;

        bars.into_iter()
            .map(|bar| Ok(data_to_pycapsule(py, Data::Bar(bar))))
            .collect()
    }
}
