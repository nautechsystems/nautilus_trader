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

//! Factory for creating FinFeed data clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::DataClient,
    clock::Clock,
    factories::{ClientConfig, DataClientFactory},
    live::runner::try_get_data_event_sender,
};
use nautilus_model::identifiers::ClientId;

use crate::{common::consts::FINFEED, config::FinFeedDataClientConfig, data::FinFeedDataClient};

/// Factory for creating FinFeed data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.finfeed", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct FinFeedDataClientFactory;

impl FinFeedDataClientFactory {
    /// Creates a new [`FinFeedDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for FinFeedDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for FinFeedDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let finfeed_config = config
            .as_any()
            .downcast_ref::<FinFeedDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for FinFeedDataClientFactory. Expected FinFeedDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let sender = try_get_data_event_sender()
            .ok_or_else(|| anyhow::anyhow!("FinFeed data event sender is not initialized"))?;
        let client = FinFeedDataClient::new(client_id, finfeed_config, sender)?;
        Ok(Box::new(client))
    }

    /// Returns the `FINFEED` factory name.
    fn name(&self) -> &'static str {
        FINFEED
    }

    fn config_type(&self) -> &'static str {
        "FinFeedDataClientConfig"
    }
}

impl ClientConfig for FinFeedDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{
        cache::Cache, clock::VirtualClock, live::runner::replace_data_event_sender,
    };
    use nautilus_core::string::secret::SecretString;
    use rstest::rstest;

    use super::*;
    use crate::instruments::FinFeedInstrumentSelection;

    fn create(config: &dyn ClientConfig) -> anyhow::Result<Box<dyn DataClient>> {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        FinFeedDataClientFactory::new().create("FINFEED-TEST", config, cache.into(), clock)
    }

    fn config() -> FinFeedDataClientConfig {
        FinFeedDataClientConfig::builder()
            .api_key(SecretString::from("factory-test-key"))
            .instruments(vec![FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec!["market_YES".into()],
            }])
            .build()
    }

    #[rstest]
    fn creates_named_multivenue_client() {
        let factory = FinFeedDataClientFactory::new();
        let config = config();
        let client = create(&config).unwrap();
        assert_eq!(factory.name(), "FINFEED");
        assert_eq!(factory.config_type(), "FinFeedDataClientConfig");
        assert_eq!(client.client_id(), ClientId::from("FINFEED-TEST"));
        assert_eq!(client.venue(), None);
        assert!(client.is_disconnected());
    }

    #[rstest]
    fn config_downcasts_from_client_config() {
        let config: Box<dyn ClientConfig> = Box::new(config());
        assert_eq!(
            config
                .as_any()
                .downcast_ref::<FinFeedDataClientConfig>()
                .unwrap()
                .instruments[0]
                .market_ids,
            ["market_YES"]
        );
    }

    #[rstest]
    #[case("selection", "instrument selection is empty")]
    #[case("poll_interval", "poll_interval_ms must be in [100, 86400000], was 99")]
    #[case("poll_limit", "poll_limit must be in [1, 10000], was 0")]
    #[case("connect_timeout", "connect_timeout_secs must be in [1, 86400], was 0")]
    #[case(
        "http_timeout",
        "within one day: http_timeout_secs (http.timeout_secs)"
    )]
    #[case("attempt_budget", "within one day: attempt_budget")]
    #[case("concurrency", "within one day: max_concurrent_requests")]
    #[case("request_interval", "within one day: min_request_interval_ms")]
    #[case(
        "retry_timeout",
        "within one day: retry_operation_timeout_ms (http.retry.operation_timeout_ms)"
    )]
    #[case(
        "retry_elapsed",
        "within one day: retry_max_elapsed_ms (http.retry.max_elapsed_ms)"
    )]
    #[case("retry_backoff", "invalid retry backoff")]
    #[case("base_url", "base URL must be HTTPS")]
    #[case("proxy", "invalid proxy URL")]
    fn propagates_client_validation(#[case] field: &str, #[case] expected: &str) {
        let mut config = config();
        match field {
            "selection" => config.instruments.clear(),
            "poll_interval" => config.poll_interval_ms = 99,
            "poll_limit" => config.poll_limit = 0,
            "connect_timeout" => config.connect_timeout_secs = 0,
            "http_timeout" => config.http.timeout_secs = 0,
            "attempt_budget" => config.http.attempt_budget = 0,
            "concurrency" => config.http.max_concurrent_requests = 0,
            "request_interval" => config.http.min_request_interval_ms = 0,
            "retry_timeout" => config.http.retry.operation_timeout_ms = None,
            "retry_elapsed" => config.http.retry.max_elapsed_ms = None,
            "retry_backoff" => config.http.retry.backoff_factor = 0.0,
            "base_url" => config.http.base_url = "http://example.com/".into(),
            "proxy" => config.http.proxy_url = Some(SecretString::from("bad proxy")),
            _ => unreachable!(),
        }

        let error = create(&config).err().unwrap().to_string();
        assert!(error.contains(expected), "{error}");
    }

    #[rstest]
    fn missing_runner_sender_returns_error() {
        let error = std::thread::spawn(|| {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock = Rc::new(RefCell::new(VirtualClock::new()));
            FinFeedDataClientFactory::new()
                .create("FINFEED-TEST", &config(), cache.into(), clock)
                .err()
                .unwrap()
                .to_string()
        })
        .join()
        .unwrap();

        assert_eq!(error, "FinFeed data event sender is not initialized");
    }

    #[rstest]
    fn rejects_wrong_config_type() {
        #[derive(Debug)]
        struct WrongConfig;
        impl ClientConfig for WrongConfig {
            fn as_any(&self) -> &dyn Any {
                self
            }
        }
        let error = create(&WrongConfig).err().unwrap().to_string();
        assert_eq!(
            error,
            "Invalid config type for FinFeedDataClientFactory. Expected FinFeedDataClientConfig, was WrongConfig"
        );
    }
}
