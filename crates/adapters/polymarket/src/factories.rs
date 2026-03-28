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

//! Factory functions for creating Polymarket clients and components.

use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

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
    common::consts::POLYMARKET_VENUE,
    config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
    data::PolymarketDataClient,
    execution::PolymarketExecutionClient,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
    },
    websocket::client::PolymarketWebSocketClient,
};

impl ClientConfig for PolymarketDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Polymarket data clients.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[derive(Debug, Clone)]
pub struct PolymarketDataClientFactory;

impl DataClientFactory for PolymarketDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let polymarket_config = config
            .as_any()
            .downcast_ref::<PolymarketDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for PolymarketDataClientFactory. Expected PolymarketDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);

        let gamma_client = PolymarketGammaHttpClient::new(
            Some(polymarket_config.gamma_url()),
            polymarket_config.http_timeout_secs,
            RetryConfig {
                max_retries: 10,
                initial_delay_ms: 5_000,
                max_delay_ms: 30_000,
                backoff_factor: 1.5,
                jitter_ms: 2_000,
                operation_timeout_ms: Some(30_000),
                immediate_first: true,
                max_elapsed_ms: Some(300_000),
            },
        )?;

        let clob_public_client = PolymarketClobPublicClient::new(
            polymarket_config.base_url_http.clone(),
            polymarket_config.http_timeout_secs,
        )?;

        let data_api_client = PolymarketDataApiHttpClient::new(
            Some(polymarket_config.data_api_url()),
            polymarket_config.http_timeout_secs,
        )?;

        let ws_client = PolymarketWebSocketClient::new_market(
            polymarket_config.base_url_ws.clone(),
            polymarket_config.subscribe_new_markets,
        );

        let mut client = PolymarketDataClient::new(
            client_id,
            polymarket_config.clone(),
            gamma_client,
            clob_public_client,
            data_api_client,
            ws_client,
        );

        for filter in &polymarket_config.filters {
            client.add_instrument_filter(Arc::clone(filter));
        }

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "POLYMARKET"
    }

    fn config_type(&self) -> &'static str {
        "PolymarketDataClientConfig"
    }
}

impl ClientConfig for PolymarketExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Polymarket execution clients.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[derive(Debug, Clone)]
pub struct PolymarketExecutionClientFactory;

impl ExecutionClientFactory for PolymarketExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let polymarket_config = config
            .as_any()
            .downcast_ref::<PolymarketExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for PolymarketExecutionClientFactory. Expected PolymarketExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let oms_type = OmsType::Netting;
        let account_type = AccountType::Cash;

        let client_id = ClientId::from(name);
        let core = ExecutionClientCore::new(
            polymarket_config.trader_id,
            client_id,
            *POLYMARKET_VENUE,
            oms_type,
            polymarket_config.account_id,
            account_type,
            None, // base_currency
            cache,
        );

        let client = PolymarketExecutionClient::new(core, polymarket_config)?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "POLYMARKET"
    }

    fn config_type(&self) -> &'static str {
        "PolymarketExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
    use rstest::rstest;

    use super::*;
    use crate::config::{PolymarketDataClientConfig, PolymarketExecClientConfig};

    #[derive(Debug)]
    struct WrongConfig;

    impl ClientConfig for WrongConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[rstest]
    fn test_polymarket_data_client_factory_creation() {
        let factory = PolymarketDataClientFactory;
        assert_eq!(factory.name(), "POLYMARKET");
        assert_eq!(factory.config_type(), "PolymarketDataClientConfig");
    }

    #[rstest]
    fn test_polymarket_data_client_config_implements_client_config() {
        let config = PolymarketDataClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<PolymarketDataClientConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_polymarket_data_client_factory_rejects_wrong_config_type() {
        let factory = PolymarketDataClientFactory;
        let wrong_config = WrongConfig;
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("POLYMARKET", &wrong_config, cache, clock);
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
    fn test_polymarket_execution_client_factory_creation() {
        let factory = PolymarketExecutionClientFactory;
        assert_eq!(factory.name(), "POLYMARKET");
        assert_eq!(factory.config_type(), "PolymarketExecClientConfig");
    }

    #[rstest]
    fn test_polymarket_exec_client_config_implements_client_config() {
        let config = PolymarketExecClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<PolymarketExecClientConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_polymarket_execution_client_factory_rejects_wrong_config_type() {
        let factory = PolymarketExecutionClientFactory;
        let wrong_config = WrongConfig;
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("POLYMARKET", &wrong_config, cache);
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
