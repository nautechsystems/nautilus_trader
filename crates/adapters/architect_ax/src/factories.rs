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

//! Factory functions for creating AX Exchange clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    live::runner::try_get_exec_event_sender,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{ClientId, TraderId},
};

use crate::{
    common::{
        consts::{AX, AX_VENUE},
        credential::Credential,
    },
    config::{AxDataClientConfig, AxExecutionClientConfig},
    data::AxDataClient,
    execution::AxExecutionClient,
    http::client::AxHttpClient,
    websocket::data::AxMdWebSocketClient,
};

impl ClientConfig for AxDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for AxExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating AX Exchange data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.architect_ax", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.architect_ax")
)]
pub struct AxDataClientFactory;

impl AxDataClientFactory {
    /// Creates a new [`AxDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for AxDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for AxDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let ax_config = config
            .as_any()
            .downcast_ref::<AxDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for AxDataClientFactory. Expected AxDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);

        let http_client = if ax_config.has_api_credentials() {
            let credential = Credential::resolve(
                ax_config.api_key.clone().map(|value| value.into_inner()),
                ax_config.api_secret.clone().map(|value| value.into_inner()),
            )
            .ok_or_else(|| anyhow::anyhow!("API credentials not configured"))?;

            AxHttpClient::with_credentials(
                credential.api_key().to_string(),
                credential.api_secret().to_string(),
                Some(ax_config.http_base_url()),
                None, // orders_base_url
                ax_config.http_timeout_secs,
                ax_config.max_retries,
                ax_config.retry_delay_initial_ms,
                ax_config.retry_delay_max_ms,
                ax_config
                    .proxy_url
                    .as_ref()
                    .map(|url| url.expose_secret().to_owned()),
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?
        } else {
            AxHttpClient::new(
                Some(ax_config.http_base_url()),
                None, // orders_base_url
                ax_config.http_timeout_secs,
                ax_config.max_retries,
                ax_config.retry_delay_initial_ms,
                ax_config.retry_delay_max_ms,
                ax_config
                    .proxy_url
                    .as_ref()
                    .map(|url| url.expose_secret().to_owned()),
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?
        };

        let ws_url = ax_config.ws_public_url();

        // Token set during connect
        let ws_client = AxMdWebSocketClient::without_auth(
            ws_url,
            ax_config.heartbeat_interval_secs,
            ax_config.transport_backend,
            ax_config
                .proxy_url
                .as_ref()
                .map(|url| url.expose_secret().to_owned()),
        );

        let client = AxDataClient::new(client_id, ax_config, http_client, ws_client)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        AX
    }

    fn config_type(&self) -> &'static str {
        "AxDataClientConfig"
    }
}

/// Factory for creating AX Exchange execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.architect_ax", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.architect_ax")
)]
pub struct AxExecutionClientFactory;

impl AxExecutionClientFactory {
    /// Creates a new [`AxExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for AxExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for AxExecutionClientFactory {
    fn create(
        &self,
        trader_id: TraderId,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let ax_config = config
            .as_any()
            .downcast_ref::<AxExecutionClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for AxExecutionClientFactory. Expected AxExecutionClientConfig, was {config:?}",
                )
            })?
            .clone();

        // AX uses netting for perpetual futures
        let oms_type = OmsType::Netting;
        let account_type = AccountType::Margin;

        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::from(name),
            *AX_VENUE,
            oms_type,
            ax_config.account_id,
            account_type,
            None, // base_currency
            cache,
        );

        let mut client = AxExecutionClient::new(core, ax_config)?;

        if let Some(sender) = try_get_exec_event_sender() {
            client.set_event_sender(sender);
        }

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        AX
    }

    fn config_type(&self) -> &'static str {
        "AxExecutionClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{
        cache::Cache, clock::VirtualClock, factories::ClientConfig,
        live::runner::replace_exec_event_sender, messages::ExecutionEvent,
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::AccountId,
        types::{AccountBalance, Money},
    };
    use rstest::rstest;

    use super::*;
    use crate::config::AxDataClientConfig;

    #[rstest]
    fn test_ax_data_client_config_implements_client_config() {
        let config = AxDataClientConfig::default();

        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config.as_any().downcast_ref::<AxDataClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_ax_data_client_factory_creation() {
        let factory = AxDataClientFactory::new();
        assert_eq!(factory.name(), AX);
        assert_eq!(factory.config_type(), "AxDataClientConfig");
    }

    #[rstest]
    fn test_ax_data_client_factory_default() {
        let factory = AxDataClientFactory;
        assert_eq!(factory.name(), AX);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_ax_execution_client_factory_sender(#[case] sender_bound: bool) {
        std::thread::spawn(move || {
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            if sender_bound {
                replace_exec_event_sender(sender.clone());
            }

            let account_id = AccountId::from("AX-FACTORY-002");
            let config = AxExecutionClientConfig {
                account_id,
                api_key: Some("test_api_key".into()),
                api_secret: Some("test_api_secret".into()),
                ..Default::default()
            };
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock = Rc::new(RefCell::new(VirtualClock::new()));
            let mut client = AxExecutionClientFactory::new()
                .create(
                    TraderId::from("TESTER-002"),
                    "AX",
                    &config,
                    cache.into(),
                    clock,
                )
                .unwrap();
            let balances = vec![AccountBalance::new(
                Money::from("1234.50 USD"),
                Money::from("234.25 USD"),
                Money::from("1000.25 USD"),
            )];
            let ts_event = UnixNanos::from(123_456);

            if !sender_bound {
                assert!(try_get_exec_event_sender().is_none());
                replace_exec_event_sender(sender);
                client.start().unwrap();
            }

            client
                .generate_account_state(balances.clone(), vec![], true, ts_event, None)
                .unwrap();

            let ExecutionEvent::Account(state) = receiver.try_recv().unwrap() else {
                panic!("Expected account state");
            };
            assert_eq!(state.account_id, account_id);
            assert_eq!(state.account_type, AccountType::Margin);
            assert_eq!(state.base_currency, None);
            assert_eq!(state.balances, balances);
            assert_eq!(state.margins, vec![]);
            assert!(state.is_reported);
            assert_eq!(state.ts_event, ts_event);
            assert_eq!(state.info, None);
            assert_eq!(
                receiver.try_recv().unwrap_err(),
                tokio::sync::mpsc::error::TryRecvError::Empty
            );
        })
        .join()
        .unwrap();
    }
}
