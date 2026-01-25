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

//! Factory functions for creating dYdX clients and components.

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
use nautilus_network::retry::RetryConfig;
use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};

use crate::{
    common::{
        consts::DYDX_VENUE,
        credential::{DydxCredential, resolve_wallet_address},
        urls,
    },
    config::{DYDXExecClientConfig, DydxAdapterConfig, DydxDataClientConfig},
    data::DydxDataClient,
    execution::DydxExecutionClient,
    http::client::DydxHttpClient,
    websocket::client::DydxWebSocketClient,
};

impl ClientConfig for DydxDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for DYDXExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating dYdX data clients.
#[derive(Debug)]
pub struct DydxDataClientFactory;

impl DydxDataClientFactory {
    /// Creates a new [`DydxDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DydxDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for DydxDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let dydx_config = config
            .as_any()
            .downcast_ref::<DydxDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for DydxDataClientFactory. Expected DydxDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);

        let http_url = dydx_config
            .base_url_http
            .clone()
            .unwrap_or_else(|| urls::http_base_url(dydx_config.is_testnet).to_string());
        let ws_url = dydx_config
            .base_url_ws
            .clone()
            .unwrap_or_else(|| urls::ws_url(dydx_config.is_testnet).to_string());

        let retry_config = if dydx_config.max_retries.is_some()
            || dydx_config.retry_delay_initial_ms.is_some()
            || dydx_config.retry_delay_max_ms.is_some()
        {
            Some(RetryConfig {
                max_retries: dydx_config.max_retries.unwrap_or(3) as u32,
                initial_delay_ms: dydx_config.retry_delay_initial_ms.unwrap_or(1000),
                max_delay_ms: dydx_config.retry_delay_max_ms.unwrap_or(10000),
                ..Default::default()
            })
        } else {
            None
        };

        let http_client = DydxHttpClient::new(
            Some(http_url),
            dydx_config.http_timeout_secs,
            dydx_config.http_proxy_url.clone(),
            dydx_config.is_testnet,
            retry_config,
        )?;

        let ws_client = DydxWebSocketClient::new_public(ws_url, Some(20));

        let client = DydxDataClient::new(client_id, dydx_config, http_client, ws_client)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "DYDX"
    }

    fn config_type(&self) -> &'static str {
        "DydxDataClientConfig"
    }
}

/// Factory for creating dYdX execution clients.
#[derive(Debug)]
pub struct DydxExecutionClientFactory;

impl DydxExecutionClientFactory {
    /// Creates a new [`DydxExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DydxExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for DydxExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let dydx_config = config
            .as_any()
            .downcast_ref::<DYDXExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for DydxExecutionClientFactory. Expected DYDXExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // dYdX uses netting for perpetual futures
        let oms_type = OmsType::Netting;

        // dYdX is always margin (perpetual futures)
        let account_type = AccountType::Margin;

        let core = ExecutionClientCore::new(
            dydx_config.trader_id,
            ClientId::from(name),
            *DYDX_VENUE,
            oms_type,
            dydx_config.account_id,
            account_type,
            None, // base_currency
            clock,
            cache,
        );

        let adapter_config = DydxAdapterConfig {
            network: dydx_config.network,
            base_url: dydx_config.get_http_url(),
            ws_url: dydx_config.get_ws_url(),
            grpc_url: dydx_config
                .get_grpc_urls()
                .first()
                .cloned()
                .unwrap_or_default(),
            grpc_urls: dydx_config.get_grpc_urls(),
            chain_id: dydx_config.get_chain_id().to_string(),
            timeout_secs: dydx_config.http_timeout_secs.unwrap_or(30),
            wallet_address: dydx_config.wallet_address.clone(),
            subaccount: dydx_config.subaccount_number,
            is_testnet: dydx_config.is_testnet(),
            mnemonic: dydx_config.mnemonic.clone(),
            authenticator_ids: dydx_config.authenticator_ids.clone(),
            max_retries: dydx_config.max_retries.unwrap_or(3),
            retry_delay_initial_ms: dydx_config.retry_delay_initial_ms.unwrap_or(1000),
            retry_delay_max_ms: dydx_config.retry_delay_max_ms.unwrap_or(10000),
        };

        let wallet_address = if let Some(addr) =
            resolve_wallet_address(dydx_config.wallet_address.clone(), dydx_config.is_testnet())
        {
            addr
        } else if let Some(credential) = DydxCredential::resolve(
            dydx_config.mnemonic.clone(),
            dydx_config.is_testnet(),
            0,
            dydx_config.authenticator_ids.clone(),
        )? {
            credential.address
        } else {
            anyhow::bail!(
                "No wallet credentials found: set wallet_address/mnemonic in config or use environment variables (DYDX_WALLET_ADDRESS/DYDX_MNEMONIC for mainnet, DYDX_TESTNET_* for testnet)"
            )
        };

        let client = DydxExecutionClient::new(
            core,
            adapter_config,
            wallet_address,
            dydx_config.subaccount_number,
        )?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "DYDX"
    }

    fn config_type(&self) -> &'static str {
        "DYDXExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_model::identifiers::{AccountId, TraderId};
    use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::DydxNetwork,
        config::{DYDXExecClientConfig, DydxDataClientConfig},
    };

    #[rstest]
    fn test_dydx_data_client_factory_creation() {
        let factory = DydxDataClientFactory::new();
        assert_eq!(factory.name(), "DYDX");
        assert_eq!(factory.config_type(), "DydxDataClientConfig");
    }

    #[rstest]
    fn test_dydx_data_client_factory_default() {
        let factory = DydxDataClientFactory;
        assert_eq!(factory.name(), "DYDX");
    }

    #[rstest]
    fn test_dydx_execution_client_factory_creation() {
        let factory = DydxExecutionClientFactory::new();
        assert_eq!(factory.name(), "DYDX");
        assert_eq!(factory.config_type(), "DYDXExecClientConfig");
    }

    #[rstest]
    fn test_dydx_execution_client_factory_default() {
        let factory = DydxExecutionClientFactory;
        assert_eq!(factory.name(), "DYDX");
    }

    #[rstest]
    fn test_dydx_data_client_config_implements_client_config() {
        let config = DydxDataClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config.as_any().downcast_ref::<DydxDataClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_dydx_exec_client_config_implements_client_config() {
        let config = DYDXExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("DYDX-001"),
            network: DydxNetwork::Mainnet,
            grpc_endpoint: None,
            grpc_urls: vec![],
            ws_endpoint: None,
            http_endpoint: None,
            mnemonic: None,
            wallet_address: Some("dydx1abc123".to_string()),
            subaccount_number: 0,
            authenticator_ids: vec![],
            http_timeout_secs: None,
            max_retries: None,
            retry_delay_initial_ms: None,
            retry_delay_max_ms: None,
        };

        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config.as_any().downcast_ref::<DYDXExecClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_dydx_data_client_factory_rejects_wrong_config_type() {
        let factory = DydxDataClientFactory::new();
        let wrong_config = DYDXExecClientConfig {
            trader_id: TraderId::from("TRADER-001"),
            account_id: AccountId::from("DYDX-001"),
            network: DydxNetwork::Mainnet,
            grpc_endpoint: None,
            grpc_urls: vec![],
            ws_endpoint: None,
            http_endpoint: None,
            mnemonic: None,
            wallet_address: None,
            subaccount_number: 0,
            authenticator_ids: vec![],
            http_timeout_secs: None,
            max_retries: None,
            retry_delay_initial_ms: None,
            retry_delay_max_ms: None,
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("DYDX-TEST", &wrong_config, cache, clock);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid config type")
        );
    }

    #[rstest]
    fn test_dydx_execution_client_factory_rejects_wrong_config_type() {
        let factory = DydxExecutionClientFactory::new();
        let wrong_config = DydxDataClientConfig::default();

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("DYDX-TEST", &wrong_config, cache, clock);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid config type")
        );
    }
}
