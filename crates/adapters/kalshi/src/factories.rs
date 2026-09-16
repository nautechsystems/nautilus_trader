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

//! Factories for creating Kalshi clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{ClientId, TraderId, Venue},
    types::Currency,
};

use crate::{
    common::{
        consts::{KALSHI_CURRENCY, KALSHI_DATA_CLIENT_ID, KALSHI_EXEC_CLIENT_ID, KALSHI_VENUE},
        credential::KalshiCredential,
    },
    config::{KalshiDataClientConfig, KalshiExecClientConfig},
    data::client::KalshiDataClient,
    execution::client::KalshiExecutionClient,
    http::{auth::KalshiAuth, client::KalshiHttpClient},
    providers::KalshiInstrumentProvider,
};

impl ClientConfig for KalshiDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Kalshi data clients.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.kalshi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiDataClientFactory;

impl DataClientFactory for KalshiDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let kalshi_config = config
            .as_any()
            .downcast_ref::<KalshiDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KalshiDataClientFactory, expected KalshiDataClientConfig, was {config:?}",
                )
            })?;

        Ok(Box::new(Self::create_client(name, kalshi_config)?))
    }

    fn name(&self) -> &'static str {
        KALSHI_VENUE
    }

    fn config_type(&self) -> &'static str {
        "KalshiDataClientConfig"
    }
}

impl KalshiDataClientFactory {
    /// Creates a Kalshi data client from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential cannot be resolved or the HTTP client cannot be built.
    pub fn create_client(
        name: &str,
        config: &KalshiDataClientConfig,
    ) -> anyhow::Result<KalshiDataClient> {
        let credential = KalshiCredential::resolve(
            config.api_key_id.clone(),
            config
                .api_key_pem
                .as_ref()
                .map(|pem| pem.expose_secret().to_string()),
        )?;
        let auth = KalshiAuth::new(credential);
        let http_client = KalshiHttpClient::new(
            Some(config.base_url()),
            config.http_timeout_secs,
            config
                .proxy_url
                .as_ref()
                .map(|url| url.expose_secret().to_string()),
            Some(auth),
        )?;
        let provider = KalshiInstrumentProvider::new(
            http_client.clone(),
            config.event_tickers.clone(),
            config.series_ticker.clone(),
        );
        let client_id = ClientId::from(if name == KALSHI_VENUE {
            KALSHI_DATA_CLIENT_ID
        } else {
            name
        });
        let update_interval = config
            .poll_interval_millis
            .map(std::time::Duration::from_millis);

        Ok(KalshiDataClient::new(
            client_id,
            http_client,
            provider,
            update_interval,
        ))
    }
}

impl ClientConfig for KalshiExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Kalshi execution clients.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.kalshi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiExecutionClientFactory;

impl ExecutionClientFactory for KalshiExecutionClientFactory {
    fn create(
        &self,
        trader_id: TraderId,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let kalshi_config = config
            .as_any()
            .downcast_ref::<KalshiExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KalshiExecutionClientFactory, expected KalshiExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        Ok(Box::new(Self::create_client(
            trader_id,
            name,
            &kalshi_config,
            cache,
        )?))
    }

    fn name(&self) -> &'static str {
        KALSHI_VENUE
    }

    fn config_type(&self) -> &'static str {
        "KalshiExecutionClientConfig"
    }
}

impl KalshiExecutionClientFactory {
    /// Creates a Kalshi execution client from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential cannot be resolved or the HTTP client cannot be built.
    pub fn create_client(
        trader_id: TraderId,
        name: &str,
        config: &KalshiExecClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<KalshiExecutionClient> {
        let credential = KalshiCredential::resolve(
            config.api_key_id.clone(),
            config
                .api_key_pem
                .as_ref()
                .map(|pem| pem.expose_secret().to_string()),
        )?;
        let http_client = KalshiHttpClient::new(
            Some(config.base_url()),
            config.http_timeout_secs,
            config
                .proxy_url
                .as_ref()
                .map(|url| url.expose_secret().to_string()),
            Some(KalshiAuth::new(credential)),
        )?;
        let client_id = ClientId::from(if name == KALSHI_VENUE {
            KALSHI_EXEC_CLIENT_ID
        } else {
            name
        });
        // A Kalshi account holds cash, and every contract it trades settles in USD.
        let core = ExecutionClientCore::new(
            trader_id,
            client_id,
            Venue::from(KALSHI_VENUE),
            OmsType::Netting,
            config.account_id,
            AccountType::Cash,
            Some(Currency::from(KALSHI_CURRENCY)),
            cache,
        );

        Ok(KalshiExecutionClient::new(core, http_client, config))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use nautilus_common::{cache::Cache, clock::TestClock, messages::DataEvent};
    use rstest::rstest;

    use super::*;
    use crate::common::consts::{KALSHI_API_KEY_ID_ENV, KALSHI_API_KEY_PEM_ENV};

    /// A config of a type this factory must refuse.
    #[derive(Debug)]
    struct OtherConfig;

    impl ClientConfig for OtherConfig {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn cache_view() -> CacheView {
        CacheView::new(Rc::new(RefCell::new(Cache::default())))
    }

    fn init_data_sender() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        nautilus_common::live::runner::replace_data_event_sender(tx);
    }

    fn config() -> KalshiDataClientConfig {
        KalshiDataClientConfig::builder()
            .api_key_id("key-id".to_string())
            .api_key_pem("not a pem".into())
            .event_tickers(vec!["KXHIGHNY-25JAN01".to_string()])
            .build()
    }

    #[rstest]
    fn test_factory_reports_the_venue_and_config_type() {
        let factory = KalshiDataClientFactory;

        assert_eq!(factory.name(), KALSHI_VENUE);
        assert_eq!(factory.config_type(), "KalshiDataClientConfig");
    }

    #[rstest]
    fn test_factory_builds_a_client_named_for_its_venue() {
        init_data_sender();
        let client = KalshiDataClientFactory::create_client(KALSHI_VENUE, &config()).unwrap();

        assert_eq!(client.client_id(), ClientId::from(KALSHI_DATA_CLIENT_ID));
        assert_eq!(client.venue(), Some(Venue::from(KALSHI_VENUE)));
    }

    #[rstest]
    fn test_factory_uses_an_explicit_client_name_when_given_one() {
        init_data_sender();
        let client = KalshiDataClientFactory::create_client("KALSHI-CUSTOM", &config()).unwrap();

        assert_eq!(client.client_id(), ClientId::from("KALSHI-CUSTOM"));
    }

    #[rstest]
    fn test_factory_honors_the_configured_poll_interval() {
        init_data_sender();
        let config = KalshiDataClientConfig::builder()
            .api_key_id("key-id".to_string())
            .api_key_pem("not a pem".into())
            .poll_interval_millis(250)
            .build();
        let client = KalshiDataClientFactory::create_client(KALSHI_VENUE, &config).unwrap();

        assert_eq!(
            client.update_interval(),
            std::time::Duration::from_millis(250)
        );
    }

    #[rstest]
    fn test_factory_refuses_a_config_of_another_type() {
        let result = KalshiDataClientFactory.create(
            KALSHI_VENUE,
            &OtherConfig,
            cache_view(),
            Rc::new(RefCell::new(TestClock::new())),
        );
        let message = result.err().map(|e| e.to_string()).unwrap_or_default();

        assert!(message.contains("Invalid config type"), "{message}");
    }

    #[rstest]
    fn test_create_client_requires_a_credential() {
        if std::env::var(KALSHI_API_KEY_ID_ENV).is_ok()
            || std::env::var(KALSHI_API_KEY_PEM_ENV).is_ok()
        {
            return;
        }

        let error =
            KalshiDataClientFactory::create_client(KALSHI_VENUE, &KalshiDataClientConfig::new())
                .unwrap_err();

        assert!(error.to_string().contains(KALSHI_API_KEY_ID_ENV), "{error}");
    }

    fn exec_config() -> KalshiExecClientConfig {
        KalshiExecClientConfig::builder()
            .api_key_id("key-id".to_string())
            .api_key_pem("not a pem".into())
            .build()
    }

    #[rstest]
    fn test_exec_factory_reports_the_venue_and_config_type() {
        let factory = KalshiExecutionClientFactory;

        assert_eq!(factory.name(), KALSHI_VENUE);
        assert_eq!(factory.config_type(), "KalshiExecutionClientConfig");
    }

    #[rstest]
    fn test_exec_factory_builds_a_client_named_for_its_venue() {
        let config = exec_config();
        let client = KalshiExecutionClientFactory
            .create(
                TraderId::from("TESTER-001"),
                KALSHI_VENUE,
                &config,
                cache_view(),
                Rc::new(RefCell::new(TestClock::new())),
            )
            .unwrap();

        assert_eq!(client.client_id(), ClientId::from(KALSHI_EXEC_CLIENT_ID));
        assert_eq!(client.venue(), Venue::from(KALSHI_VENUE));
        assert_eq!(client.account_id(), config.account_id);
        assert_eq!(client.oms_type(), OmsType::Netting);
    }

    #[rstest]
    fn test_exec_factory_uses_an_explicit_client_name_when_given_one() {
        let client = KalshiExecutionClientFactory
            .create(
                TraderId::from("TESTER-001"),
                "KALSHI-CUSTOM",
                &exec_config(),
                cache_view(),
                Rc::new(RefCell::new(TestClock::new())),
            )
            .unwrap();

        assert_eq!(client.client_id(), ClientId::from("KALSHI-CUSTOM"));
    }

    #[rstest]
    fn test_exec_factory_refuses_a_config_of_another_type() {
        let result = KalshiExecutionClientFactory.create(
            TraderId::from("TESTER-001"),
            KALSHI_VENUE,
            &OtherConfig,
            cache_view(),
            Rc::new(RefCell::new(TestClock::new())),
        );
        let message = result.err().map(|e| e.to_string()).unwrap_or_default();

        assert!(message.contains("Invalid config type"), "{message}");
    }

    #[rstest]
    fn test_exec_create_client_honors_the_configured_poll_interval() {
        let config = KalshiExecClientConfig::builder()
            .api_key_id("key-id".to_string())
            .api_key_pem("not a pem".into())
            .poll_interval_millis(250)
            .build();
        let client = KalshiExecutionClientFactory::create_client(
            TraderId::from("TESTER-001"),
            KALSHI_VENUE,
            &config,
            cache_view(),
        )
        .unwrap();

        assert_eq!(
            client.poll_interval(),
            std::time::Duration::from_millis(250)
        );
    }

    #[rstest]
    fn test_exec_create_client_requires_a_credential() {
        if std::env::var(KALSHI_API_KEY_ID_ENV).is_ok()
            || std::env::var(KALSHI_API_KEY_PEM_ENV).is_ok()
        {
            return;
        }

        let error = KalshiExecutionClientFactory::create_client(
            TraderId::from("TESTER-001"),
            KALSHI_VENUE,
            &KalshiExecClientConfig::new(),
            cache_view(),
        )
        .unwrap_err();

        assert!(error.to_string().contains(KALSHI_API_KEY_ID_ENV), "{error}");
    }
}
