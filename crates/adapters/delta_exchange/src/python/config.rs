// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python configuration bindings for Delta Exchange.

use pyo3::prelude::*;

use crate::{
    common::consts::{
        DEFAULT_HTTP_TIMEOUT_SECS, DEFAULT_RECONNECTION_DELAY_SECS, DEFAULT_WS_TIMEOUT_SECS,
        DELTA_EXCHANGE_REST_URL, DELTA_EXCHANGE_TESTNET_REST_URL, DELTA_EXCHANGE_TESTNET_WS_URL,
        DELTA_EXCHANGE_WS_URL, MAX_RECONNECTION_ATTEMPTS,
    },
    websocket::{
        client::DeltaExchangeWsConfig,
        enums::ReconnectionStrategy,
    },
};

/// Python wrapper for WebSocket configuration.
#[pyclass(name = "DeltaExchangeWsConfig")]
#[derive(Debug, Clone)]
pub struct PyDeltaExchangeWsConfig {
    pub inner: DeltaExchangeWsConfig,
}

#[pymethods]
impl PyDeltaExchangeWsConfig {
    #[new]
    #[pyo3(signature = (
        url=None,
        timeout_secs=None,
        reconnection_strategy=None,
        max_reconnection_attempts=None,
        reconnection_delay_secs=None,
        heartbeat_interval_secs=None,
        auto_reconnect=None,
        max_queue_size=None
    ))]
    fn py_new(
        url: Option<String>,
        timeout_secs: Option<u64>,
        reconnection_strategy: Option<String>,
        max_reconnection_attempts: Option<u32>,
        reconnection_delay_secs: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        auto_reconnect: Option<bool>,
        max_queue_size: Option<usize>,
    ) -> PyResult<Self> {
        let strategy = match reconnection_strategy.as_deref() {
            Some("none") => ReconnectionStrategy::None,
            Some("immediate") => ReconnectionStrategy::Immediate,
            Some("exponential_backoff") => ReconnectionStrategy::ExponentialBackoff,
            Some("fixed_interval") => ReconnectionStrategy::FixedInterval,
            None => ReconnectionStrategy::ExponentialBackoff,
            Some(s) => {
                return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                    format!("Invalid reconnection strategy: {}", s),
                ));
            }
        };

        let config = DeltaExchangeWsConfig {
            url: url.unwrap_or_else(|| DELTA_EXCHANGE_WS_URL.to_string()),
            timeout_secs: timeout_secs.unwrap_or(DEFAULT_WS_TIMEOUT_SECS),
            reconnection_strategy: strategy,
            max_reconnection_attempts: max_reconnection_attempts.unwrap_or(MAX_RECONNECTION_ATTEMPTS),
            reconnection_delay_secs: reconnection_delay_secs.unwrap_or(DEFAULT_RECONNECTION_DELAY_SECS),
            heartbeat_interval_secs,
            auto_reconnect: auto_reconnect.unwrap_or(true),
            max_queue_size: max_queue_size.unwrap_or(10000),
        };

        Ok(Self { inner: config })
    }

    /// Create configuration for production environment.
    #[staticmethod]
    #[pyo3(name = "production")]
    fn py_production() -> Self {
        Self {
            inner: DeltaExchangeWsConfig::default(),
        }
    }

    /// Create configuration for testnet environment.
    #[staticmethod]
    #[pyo3(name = "testnet")]
    fn py_testnet() -> Self {
        Self {
            inner: DeltaExchangeWsConfig::testnet(),
        }
    }

    /// WebSocket URL.
    #[getter]
    #[pyo3(name = "url")]
    fn py_url(&self) -> &str {
        &self.inner.url
    }

    /// Set WebSocket URL.
    #[setter]
    #[pyo3(name = "url")]
    fn py_set_url(&mut self, url: String) {
        self.inner.url = url;
    }

    /// Connection timeout in seconds.
    #[getter]
    #[pyo3(name = "timeout_secs")]
    fn py_timeout_secs(&self) -> u64 {
        self.inner.timeout_secs
    }

    /// Set connection timeout in seconds.
    #[setter]
    #[pyo3(name = "timeout_secs")]
    fn py_set_timeout_secs(&mut self, timeout_secs: u64) {
        self.inner.timeout_secs = timeout_secs;
    }

    /// Reconnection strategy.
    #[getter]
    #[pyo3(name = "reconnection_strategy")]
    fn py_reconnection_strategy(&self) -> String {
        match self.inner.reconnection_strategy {
            ReconnectionStrategy::None => "none".to_string(),
            ReconnectionStrategy::Immediate => "immediate".to_string(),
            ReconnectionStrategy::ExponentialBackoff => "exponential_backoff".to_string(),
            ReconnectionStrategy::FixedInterval => "fixed_interval".to_string(),
        }
    }

    /// Set reconnection strategy.
    #[setter]
    #[pyo3(name = "reconnection_strategy")]
    fn py_set_reconnection_strategy(&mut self, strategy: String) -> PyResult<()> {
        self.inner.reconnection_strategy = match strategy.as_str() {
            "none" => ReconnectionStrategy::None,
            "immediate" => ReconnectionStrategy::Immediate,
            "exponential_backoff" => ReconnectionStrategy::ExponentialBackoff,
            "fixed_interval" => ReconnectionStrategy::FixedInterval,
            _ => {
                return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                    format!("Invalid reconnection strategy: {}", strategy),
                ));
            }
        };
        Ok(())
    }

    /// Maximum reconnection attempts.
    #[getter]
    #[pyo3(name = "max_reconnection_attempts")]
    fn py_max_reconnection_attempts(&self) -> u32 {
        self.inner.max_reconnection_attempts
    }

    /// Set maximum reconnection attempts.
    #[setter]
    #[pyo3(name = "max_reconnection_attempts")]
    fn py_set_max_reconnection_attempts(&mut self, attempts: u32) {
        self.inner.max_reconnection_attempts = attempts;
    }

    /// Reconnection delay in seconds.
    #[getter]
    #[pyo3(name = "reconnection_delay_secs")]
    fn py_reconnection_delay_secs(&self) -> u64 {
        self.inner.reconnection_delay_secs
    }

    /// Set reconnection delay in seconds.
    #[setter]
    #[pyo3(name = "reconnection_delay_secs")]
    fn py_set_reconnection_delay_secs(&mut self, delay_secs: u64) {
        self.inner.reconnection_delay_secs = delay_secs;
    }

    /// Heartbeat interval in seconds.
    #[getter]
    #[pyo3(name = "heartbeat_interval_secs")]
    fn py_heartbeat_interval_secs(&self) -> Option<u64> {
        self.inner.heartbeat_interval_secs
    }

    /// Set heartbeat interval in seconds.
    #[setter]
    #[pyo3(name = "heartbeat_interval_secs")]
    fn py_set_heartbeat_interval_secs(&mut self, interval_secs: Option<u64>) {
        self.inner.heartbeat_interval_secs = interval_secs;
    }

    /// Auto reconnect flag.
    #[getter]
    #[pyo3(name = "auto_reconnect")]
    fn py_auto_reconnect(&self) -> bool {
        self.inner.auto_reconnect
    }

    /// Set auto reconnect flag.
    #[setter]
    #[pyo3(name = "auto_reconnect")]
    fn py_set_auto_reconnect(&mut self, auto_reconnect: bool) {
        self.inner.auto_reconnect = auto_reconnect;
    }

    /// Maximum message queue size.
    #[getter]
    #[pyo3(name = "max_queue_size")]
    fn py_max_queue_size(&self) -> usize {
        self.inner.max_queue_size
    }

    /// Set maximum message queue size.
    #[setter]
    #[pyo3(name = "max_queue_size")]
    fn py_set_max_queue_size(&mut self, max_queue_size: usize) {
        self.inner.max_queue_size = max_queue_size;
    }

    fn __str__(&self) -> String {
        format!(
            "DeltaExchangeWsConfig(url={}, timeout_secs={}, reconnection_strategy={}, auto_reconnect={})",
            self.inner.url,
            self.inner.timeout_secs,
            self.py_reconnection_strategy(),
            self.inner.auto_reconnect
        )
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

/// Python wrapper for HTTP client configuration.
#[pyclass(name = "DeltaExchangeHttpConfig")]
#[derive(Debug, Clone)]
pub struct PyDeltaExchangeHttpConfig {
    pub base_url: String,
    pub timeout_secs: u64,
    pub testnet: bool,
}

#[pymethods]
impl PyDeltaExchangeHttpConfig {
    #[new]
    #[pyo3(signature = (base_url=None, timeout_secs=None, testnet=None))]
    fn py_new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        testnet: Option<bool>,
    ) -> Self {
        let is_testnet = testnet.unwrap_or(false);
        let default_url = if is_testnet {
            DELTA_EXCHANGE_TESTNET_REST_URL
        } else {
            DELTA_EXCHANGE_REST_URL
        };

        Self {
            base_url: base_url.unwrap_or_else(|| default_url.to_string()),
            timeout_secs: timeout_secs.unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS),
            testnet: is_testnet,
        }
    }

    /// Create configuration for production environment.
    #[staticmethod]
    #[pyo3(name = "production")]
    fn py_production() -> Self {
        Self {
            base_url: DELTA_EXCHANGE_REST_URL.to_string(),
            timeout_secs: DEFAULT_HTTP_TIMEOUT_SECS,
            testnet: false,
        }
    }

    /// Create configuration for testnet environment.
    #[staticmethod]
    #[pyo3(name = "testnet")]
    fn py_testnet() -> Self {
        Self {
            base_url: DELTA_EXCHANGE_TESTNET_REST_URL.to_string(),
            timeout_secs: DEFAULT_HTTP_TIMEOUT_SECS,
            testnet: true,
        }
    }

    /// Base URL for REST API.
    #[getter]
    #[pyo3(name = "base_url")]
    fn py_base_url(&self) -> &str {
        &self.base_url
    }

    /// Set base URL for REST API.
    #[setter]
    #[pyo3(name = "base_url")]
    fn py_set_base_url(&mut self, base_url: String) {
        self.base_url = base_url;
    }

    /// HTTP timeout in seconds.
    #[getter]
    #[pyo3(name = "timeout_secs")]
    fn py_timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// Set HTTP timeout in seconds.
    #[setter]
    #[pyo3(name = "timeout_secs")]
    fn py_set_timeout_secs(&mut self, timeout_secs: u64) {
        self.timeout_secs = timeout_secs;
    }

    /// Testnet flag.
    #[getter]
    #[pyo3(name = "testnet")]
    fn py_testnet(&self) -> bool {
        self.testnet
    }

    /// Set testnet flag.
    #[setter]
    #[pyo3(name = "testnet")]
    fn py_set_testnet(&mut self, testnet: bool) {
        self.testnet = testnet;
        // Update base URL based on testnet flag
        self.base_url = if testnet {
            DELTA_EXCHANGE_TESTNET_REST_URL.to_string()
        } else {
            DELTA_EXCHANGE_REST_URL.to_string()
        };
    }

    fn __str__(&self) -> String {
        format!(
            "DeltaExchangeHttpConfig(base_url={}, timeout_secs={}, testnet={})",
            self.base_url, self.timeout_secs, self.testnet
        )
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}
