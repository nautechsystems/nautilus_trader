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

//! Python projection of prediction-market configuration.

use nautilus_core::string::secret::SecretString;
use nautilus_network::retry::RetryConfig;
use pyo3::prelude::*;

use crate::{
    config::FinFeedDataClientConfig, http::client::FinFeedHttpConfig,
    instruments::FinFeedInstrumentSelection,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl FinFeedInstrumentSelection {
    /// Exact outcome IDs explicitly selected from one FinFeed exchange.
    ///
    /// IDs come from discovery or are supplied directly. Loading does not implicitly include
    /// other outcomes of the same market or event.
    #[new]
    fn py_new(exchange_id: String, market_ids: Vec<String>) -> Self {
        Self {
            exchange_id: exchange_id.into(),
            market_ids: market_ids.into_iter().map(Into::into).collect(),
        }
    }

    /// Provider exchange identifier, such as `POLYMARKET`.
    #[getter]
    fn exchange_id(&self) -> &str {
        &self.exchange_id
    }

    /// Exact outcome IDs, including `_YES` or `_NO`; related outcomes are not loaded implicitly.
    #[getter]
    fn market_ids(&self) -> Vec<String> {
        self.market_ids.iter().map(ToString::to_string).collect()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl FinFeedDataClientConfig {
    /// Explicit startup selections and bounded REST, bootstrap, and polling controls.
    ///
    /// # Bootstrap
    ///
    /// `connect_timeout_secs` bounds the whole bootstrap, including request admission.
    /// Selections and controls are validated when building the client, before network requests.
    ///
    /// # Python Configuration
    ///
    /// Python exposes nested Rust HTTP controls as flat constructor keywords:
    ///
    /// - `http`: `base_url_http`, `proxy_url`, `http_timeout_secs`, `max_concurrent_requests`,
    ///   `attempt_budget`, and `min_request_interval_ms`.
    /// - `http.retry`: `max_retries`, `retry_delay_initial_ms`, `retry_delay_max_ms`,
    ///   `retry_backoff_factor`, `retry_jitter_ms`, `retry_operation_timeout_ms`,
    ///   `retry_immediate_first`, and `retry_max_elapsed_ms`.
    ///
    /// Omitted or `None` options use Rust defaults. Configuration construction rejects unknown
    /// keywords and wrong types. API keys and proxy URLs expose presence indicators instead of
    /// plaintext getters.
    ///
    /// # Credentials
    ///
    /// Creating a config does not read credentials or connect. If `api_key` is absent, the data
    /// client reads `FINFEED_API_KEY` once at client construction.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        base_url_http = None,
        proxy_url = None,
        instruments = None,
        http_timeout_secs = None,
        max_concurrent_requests = None,
        attempt_budget = None,
        min_request_interval_ms = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        retry_backoff_factor = None,
        retry_jitter_ms = None,
        retry_operation_timeout_ms = None,
        retry_immediate_first = None,
        retry_max_elapsed_ms = None,
        poll_interval_ms = None,
        poll_limit = None,
        connect_timeout_secs = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        base_url_http: Option<String>,
        proxy_url: Option<String>,
        instruments: Option<Vec<FinFeedInstrumentSelection>>,
        http_timeout_secs: Option<u64>,
        max_concurrent_requests: Option<usize>,
        attempt_budget: Option<u64>,
        min_request_interval_ms: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        retry_backoff_factor: Option<f64>,
        retry_jitter_ms: Option<u64>,
        retry_operation_timeout_ms: Option<u64>,
        retry_immediate_first: Option<bool>,
        retry_max_elapsed_ms: Option<u64>,
        poll_interval_ms: Option<u64>,
        poll_limit: Option<u32>,
        connect_timeout_secs: Option<u64>,
    ) -> Self {
        let defaults = Self::default();

        Self {
            api_key: api_key.map(SecretString::from),
            http: FinFeedHttpConfig {
                base_url: base_url_http.unwrap_or(defaults.http.base_url),
                proxy_url: proxy_url.map(SecretString::from),
                timeout_secs: http_timeout_secs.unwrap_or(defaults.http.timeout_secs),
                max_concurrent_requests: max_concurrent_requests
                    .unwrap_or(defaults.http.max_concurrent_requests),
                attempt_budget: attempt_budget.unwrap_or(defaults.http.attempt_budget),
                min_request_interval_ms: min_request_interval_ms
                    .unwrap_or(defaults.http.min_request_interval_ms),
                retry: RetryConfig {
                    max_retries: max_retries.unwrap_or(defaults.http.retry.max_retries),
                    initial_delay_ms: retry_delay_initial_ms
                        .unwrap_or(defaults.http.retry.initial_delay_ms),
                    max_delay_ms: retry_delay_max_ms.unwrap_or(defaults.http.retry.max_delay_ms),
                    backoff_factor: retry_backoff_factor
                        .unwrap_or(defaults.http.retry.backoff_factor),
                    jitter_ms: retry_jitter_ms.unwrap_or(defaults.http.retry.jitter_ms),
                    operation_timeout_ms: retry_operation_timeout_ms
                        .or(defaults.http.retry.operation_timeout_ms),
                    immediate_first: retry_immediate_first
                        .unwrap_or(defaults.http.retry.immediate_first),
                    max_elapsed_ms: retry_max_elapsed_ms.or(defaults.http.retry.max_elapsed_ms),
                },
            },
            instruments: instruments.unwrap_or(defaults.instruments),
            poll_interval_ms: poll_interval_ms.unwrap_or(defaults.poll_interval_ms),
            poll_limit: poll_limit.unwrap_or(defaults.poll_limit),
            connect_timeout_secs: connect_timeout_secs.unwrap_or(defaults.connect_timeout_secs),
        }
    }

    /// HTTPS endpoint, or a plaintext loopback test peer, including the version prefix.
    #[getter]
    fn base_url_http(&self) -> &str {
        &self.http.base_url
    }

    /// Exact Polymarket Yes/No outcomes loaded and published before readiness.
    #[getter]
    fn instruments(&self) -> Vec<FinFeedInstrumentSelection> {
        self.instruments.clone()
    }

    /// Transport timeout for one request, in seconds.
    #[getter]
    fn http_timeout_secs(&self) -> u64 {
        self.http.timeout_secs
    }

    /// Maximum managed requests in flight on this instance.
    #[getter]
    fn max_concurrent_requests(&self) -> usize {
        self.http.max_concurrent_requests
    }

    /// Lifetime managed attempt allowance, including failed and canceled attempts after admission.
    #[getter]
    fn attempt_budget(&self) -> u64 {
        self.http.attempt_budget
    }

    /// Minimum spacing between managed attempt starts, in milliseconds.
    #[getter]
    fn min_request_interval_ms(&self) -> u64 {
        self.http.min_request_interval_ms
    }

    /// Maximum number of retry attempts (total attempts = 1 initial + `max_retries`).
    #[getter]
    fn max_retries(&self) -> u32 {
        self.http.retry.max_retries
    }

    /// Initial delay between retries in milliseconds.
    #[getter]
    fn retry_delay_initial_ms(&self) -> u64 {
        self.http.retry.initial_delay_ms
    }

    /// Maximum delay between retries in milliseconds.
    #[getter]
    fn retry_delay_max_ms(&self) -> u64 {
        self.http.retry.max_delay_ms
    }

    /// Backoff multiplier factor.
    #[getter]
    fn retry_backoff_factor(&self) -> f64 {
        self.http.retry.backoff_factor
    }

    /// Maximum jitter in milliseconds to add to delays.
    #[getter]
    fn retry_jitter_ms(&self) -> u64 {
        self.http.retry.jitter_ms
    }

    /// Retry policy with required per-operation and total elapsed deadlines, in milliseconds.
    /// Both include local admission waits; the total deadline also covers retries and backoff.
    /// Deadlines must be positive and at most one day. Python `None` retains defaults;
    /// a Rust `None` deadline fails client validation.
    /// Unspecified retry fields retain FinFeed defaults during deserialization.
    #[getter]
    fn retry_operation_timeout_ms(&self) -> Option<u64> {
        self.http.retry.operation_timeout_ms
    }

    /// Whether the first retry occurs without delay.
    ///
    /// Connection operations typically enable this, while HTTP and order operations typically
    /// retain a delay.
    #[getter]
    fn retry_immediate_first(&self) -> bool {
        self.http.retry.immediate_first
    }

    /// Retry policy with required per-operation and total elapsed deadlines, in milliseconds.
    /// Both include local admission waits; the total deadline also covers retries and backoff.
    /// Deadlines must be positive and at most one day. Python `None` retains defaults;
    /// a Rust `None` deadline fails client validation.
    /// Unspecified retry fields retain FinFeed defaults during deserialization.
    #[getter]
    fn retry_max_elapsed_ms(&self) -> Option<u64> {
        self.http.retry.max_elapsed_ms
    }

    /// Delay after each instrument's poll on the same endpoint completes, in milliseconds, in `[100, 86400000]`.
    #[getter]
    fn poll_interval_ms(&self) -> u64 {
        self.poll_interval_ms
    }

    /// Maximum latest trades and quotes per poll, independently, in `[1, 10000]`.
    /// OHLCV requests add one record slot for a running period, within the endpoint limit.
    #[getter]
    fn poll_limit(&self) -> u32 {
        self.poll_limit
    }

    /// Whole bootstrap deadline in seconds, including request admission.
    #[getter]
    fn connect_timeout_secs(&self) -> u64 {
        self.connect_timeout_secs
    }

    /// Returns whether `api_key` is set.
    #[getter]
    fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Returns whether `proxy_url` is set.
    #[getter]
    fn has_proxy_url(&self) -> bool {
        self.http.proxy_url.is_some()
    }

    /// Returns the constant representation `FinFeedDataClientConfig`.
    fn __repr__(&self) -> &'static str {
        "FinFeedDataClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use pyo3::types::PyDict;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn python_projection_preserves_every_rust_field() {
        Python::initialize();
        Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs.set_item("api_key", "projection-secret").unwrap();
            kwargs
                .set_item("base_url_http", "http://localhost:8123/v1/")
                .unwrap();
            kwargs
                .set_item("proxy_url", "http://user:proxy-secret@localhost:8124")
                .unwrap();
            let selection = Py::new(
                py,
                FinFeedInstrumentSelection::py_new(
                    "POLYMARKET".into(),
                    vec!["first_YES".into(), "second_NO".into()],
                ),
            )
            .unwrap();
            kwargs.set_item("instruments", vec![selection]).unwrap();
            for (name, value) in [
                ("http_timeout_secs", 13),
                ("max_concurrent_requests", 4),
                ("attempt_budget", 17),
                ("min_request_interval_ms", 19),
                ("max_retries", 2),
                ("retry_delay_initial_ms", 29),
                ("retry_delay_max_ms", 53),
                ("retry_jitter_ms", 7),
                ("retry_operation_timeout_ms", 31_000),
                ("retry_max_elapsed_ms", 71_000),
                ("poll_interval_ms", 137),
                ("poll_limit", 23),
                ("connect_timeout_secs", 43),
            ] {
                kwargs.set_item(name, value).unwrap();
            }

            kwargs.set_item("retry_backoff_factor", 1.5).unwrap();
            kwargs.set_item("retry_immediate_first", true).unwrap();
            let object = py
                .get_type::<FinFeedDataClientConfig>()
                .call((), Some(&kwargs))
                .unwrap();
            let config: FinFeedDataClientConfig = object.extract().unwrap();
            assert_eq!(
                config.api_key.as_ref().unwrap().expose_secret(),
                "projection-secret"
            );
            assert_eq!(config.http.base_url, "http://localhost:8123/v1/");
            assert_eq!(
                config.http.proxy_url.as_ref().unwrap().expose_secret(),
                "http://user:proxy-secret@localhost:8124"
            );
            assert_eq!(config.instruments.len(), 1);
            assert_eq!(config.instruments[0].exchange_id, "POLYMARKET");
            assert_eq!(config.instruments[0].market_ids, ["first_YES", "second_NO"]);
            assert_eq!(config.http.timeout_secs, 13);
            assert_eq!(config.http.max_concurrent_requests, 4);
            assert_eq!(config.http.attempt_budget, 17);
            assert_eq!(config.http.min_request_interval_ms, 19);
            assert_eq!(config.http.retry.max_retries, 2);
            assert_eq!(config.http.retry.initial_delay_ms, 29);
            assert_eq!(config.http.retry.max_delay_ms, 53);
            assert_eq!(config.http.retry.backoff_factor, 1.5);
            assert_eq!(config.http.retry.jitter_ms, 7);
            assert_eq!(config.http.retry.operation_timeout_ms, Some(31_000));
            assert!(config.http.retry.immediate_first);
            assert_eq!(config.http.retry.max_elapsed_ms, Some(71_000));
            assert_eq!(config.poll_interval_ms, 137);
            assert_eq!(config.poll_limit, 23);
            assert_eq!(config.connect_timeout_secs, 43);
            let debug = format!("{config:?}");

            for secret in ["projection-secret", "proxy-secret"] {
                assert!(!debug.contains(secret));
            }
        });
    }

    #[rstest]
    fn python_defaults_match_rust() {
        Python::initialize();
        Python::attach(|py| {
            let object = py.get_type::<FinFeedDataClientConfig>().call0().unwrap();
            let config: FinFeedDataClientConfig = object.extract().unwrap();
            assert_eq!(
                serde_json::to_value(config).unwrap(),
                serde_json::to_value(FinFeedDataClientConfig::default()).unwrap()
            );
        });
    }
}
