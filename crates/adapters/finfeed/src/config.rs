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

//! Configuration for prediction-market requests and polling subscriptions.

use nautilus_core::string::secret::SecretString;
use serde::{Deserialize, Serialize};

use crate::{http::client::FinFeedHttpConfig, instruments::FinFeedInstrumentSelection};

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
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.finfeed", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct FinFeedDataClientConfig {
    /// Explicit key, or `FINFEED_API_KEY` read once at client construction and reused on reconnect.
    pub api_key: Option<SecretString>,
    /// REST transport, pacing, retry, and local attempt controls for each connection generation.
    #[builder(default = FinFeedHttpConfig::default())]
    pub http: FinFeedHttpConfig,
    /// Exact Polymarket Yes/No outcomes loaded and published before readiness.
    #[builder(default = Vec::new())]
    pub instruments: Vec<FinFeedInstrumentSelection>,
    /// Delay after each instrument's poll on the same endpoint completes, in milliseconds, in `[100, 86400000]`.
    #[builder(default = FinFeedDataClientConfig::default().poll_interval_ms)]
    pub poll_interval_ms: u64,
    /// Maximum latest trades and quotes per poll, independently, in `[1, 10000]`.
    /// OHLCV requests add one record slot for a running period, within the endpoint limit.
    #[builder(default = FinFeedDataClientConfig::default().poll_limit)]
    pub poll_limit: u32,
    /// Whole bootstrap deadline in seconds, including request admission.
    #[builder(default = FinFeedDataClientConfig::default().connect_timeout_secs)]
    pub connect_timeout_secs: u64,
}

impl Default for FinFeedDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            http: FinFeedHttpConfig::default(),
            instruments: Vec::new(),
            poll_interval_ms: 10_000,
            poll_limit: 100,
            connect_timeout_secs: 120,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn builder_and_serde_defaults_match() {
        let expected = serde_json::to_value(FinFeedDataClientConfig::default()).unwrap();
        assert_eq!(
            serde_json::to_value(FinFeedDataClientConfig::builder().build()).unwrap(),
            expected
        );
        let empty = serde_json::Value::Object(serde_json::Map::new());
        let decoded: FinFeedDataClientConfig = serde_json::from_value(empty).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }

    #[rstest]
    fn partial_retry_override_preserves_finfeed_defaults() {
        let value = serde_json::json!({"http": {"retry": {"max_retries": 5}}});
        let config: FinFeedDataClientConfig = serde_json::from_value(value).unwrap();

        assert_eq!(
            serde_json::to_value(config.http.retry).unwrap(),
            serde_json::json!({
                "max_retries": 5,
                "initial_delay_ms": 1000,
                "max_delay_ms": 10000,
                "backoff_factor": 2.0,
                "jitter_ms": 0,
                "operation_timeout_ms": 30000,
                "immediate_first": false,
                "max_elapsed_ms": 120000,
            })
        );
    }

    #[rstest]
    fn retry_overrides_preserve_explicit_values_and_nulls() {
        let retry = serde_json::json!({
            "max_retries": 2,
            "initial_delay_ms": 29,
            "max_delay_ms": 53,
            "backoff_factor": 1.5,
            "jitter_ms": 7,
            "operation_timeout_ms": 31000,
            "immediate_first": true,
            "max_elapsed_ms": 71000,
        });
        let value = serde_json::json!({"http": {"retry": retry}});
        let config: FinFeedDataClientConfig = serde_json::from_value(value).unwrap();
        assert_eq!(serde_json::to_value(config.http.retry).unwrap(), retry);

        let value = serde_json::json!({"http": {"retry": {"max_elapsed_ms": null}}});
        let config: FinFeedDataClientConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.http.retry.max_elapsed_ms, None);
        assert_eq!(config.http.retry.jitter_ms, 0);
    }

    #[rstest]
    #[case(serde_json::json!({"unknown": 1}))]
    #[case(serde_json::json!({"http": {"unknown": 2}}))]
    #[case(serde_json::json!({"http": {"retry": {"unknown": 3}}}))]
    #[case(serde_json::json!({"instruments": [{"exchange_id": "POLYMARKET", "market_ids": ["a_YES"], "unknown": 4}]}))]
    fn rejects_unknown_fields(#[case] value: serde_json::Value) {
        let error = serde_json::from_value::<FinFeedDataClientConfig>(value).unwrap_err();
        assert!(error.to_string().contains("unknown field `unknown`"));
    }
}
