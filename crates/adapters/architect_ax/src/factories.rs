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
    cache::Cache,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};
use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};

use crate::{
    common::consts::{AX_VENUE, AX_WS_PUBLIC_URL, AX_WS_SANDBOX_PUBLIC_URL},
    config::{AxDataClientConfig, AxExecClientConfig},
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

impl ClientConfig for AxExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating AX Exchange data clients.
#[derive(Debug)]
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
        _cache: Rc<RefCell<Cache>>,
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
            let api_key = ax_config
                .api_key
                .clone()
                .or_else(|| std::env::var("AX_API_KEY").ok())
                .ok_or_else(|| anyhow::anyhow!("AX_API_KEY not configured"))?;

            let api_secret = ax_config
                .api_secret
                .clone()
                .or_else(|| std::env::var("AX_API_SECRET").ok())
                .ok_or_else(|| anyhow::anyhow!("AX_API_SECRET not configured"))?;

            AxHttpClient::with_credentials(
                api_key,
                api_secret,
                Some(ax_config.http_base_url()),
                None, // orders_base_url
                ax_config.http_timeout_secs,
                ax_config.max_retries,
                ax_config.retry_delay_initial_ms,
                ax_config.retry_delay_max_ms,
                ax_config.http_proxy_url.clone(),
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
                ax_config.http_proxy_url.clone(),
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?
        };

        let ws_url = ax_config.base_url_ws_public.clone().unwrap_or_else(|| {
            if ax_config.is_sandbox {
                AX_WS_SANDBOX_PUBLIC_URL.to_string()
            } else {
                AX_WS_PUBLIC_URL.to_string()
            }
        });

        // Token set during connect
        let ws_client =
            AxMdWebSocketClient::without_auth(ws_url, ax_config.heartbeat_interval_secs);

        let client = AxDataClient::new(client_id, ax_config, http_client, ws_client)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "AX"
    }

    fn config_type(&self) -> &'static str {
        "AxDataClientConfig"
    }
}

/// Factory for creating AX Exchange execution clients.
#[derive(Debug)]
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
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let ax_config = config
            .as_any()
            .downcast_ref::<AxExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for AxExecutionClientFactory. Expected AxExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // AX uses netting for perpetual futures
        let oms_type = OmsType::Netting;
        let account_type = AccountType::Margin;

        let core = ExecutionClientCore::new(
            ax_config.trader_id,
            ClientId::from(name),
            *AX_VENUE,
            oms_type,
            ax_config.account_id,
            account_type,
            None, // base_currency
            clock,
            cache,
        );

        let client = AxExecutionClient::new(core, ax_config)?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "AX"
    }

    fn config_type(&self) -> &'static str {
        "AxExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use nautilus_system::factories::ClientConfig;
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
        assert_eq!(factory.name(), "AX");
        assert_eq!(factory.config_type(), "AxDataClientConfig");
    }

    #[rstest]
    fn test_ax_data_client_factory_default() {
        let factory = AxDataClientFactory;
        assert_eq!(factory.name(), "AX");
    }
}
