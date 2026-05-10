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

//! Factory functions for creating Kraken clients and components.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};

use crate::{
    common::{consts::KRAKEN_VENUE, enums::KrakenProductType},
    config::{KrakenDataClientConfig, KrakenExecClientConfig},
    data::{KrakenFuturesDataClient, KrakenSpotDataClient},
    execution::{KrakenFuturesExecutionClient, KrakenSpotExecutionClient},
};

impl ClientConfig for KrakenDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Kraken data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.kraken")
)]
pub struct KrakenDataClientFactory;

impl KrakenDataClientFactory {
    /// Creates a new [`KrakenDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for KrakenDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for KrakenDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let kraken_config = config
            .as_any()
            .downcast_ref::<KrakenDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KrakenDataClientFactory. Expected KrakenDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);

        match kraken_config.product_type {
            KrakenProductType::Spot => {
                let client = KrakenSpotDataClient::new(client_id, kraken_config)?;
                Ok(Box::new(client))
            }
            KrakenProductType::Futures => {
                let client = KrakenFuturesDataClient::new(client_id, kraken_config)?;
                Ok(Box::new(client))
            }
        }
    }

    fn name(&self) -> &'static str {
        "KRAKEN"
    }

    fn config_type(&self) -> &'static str {
        "KrakenDataClientConfig"
    }
}

impl ClientConfig for KrakenExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Kraken execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.kraken")
)]
pub struct KrakenExecutionClientFactory;

impl KrakenExecutionClientFactory {
    /// Creates a new [`KrakenExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for KrakenExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for KrakenExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let kraken_config = config
            .as_any()
            .downcast_ref::<KrakenExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KrakenExecutionClientFactory. Expected KrakenExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let oms_type = OmsType::Netting;
        let account_type = match kraken_config.product_type {
            KrakenProductType::Spot => AccountType::Cash,
            KrakenProductType::Futures => AccountType::Margin,
        };

        let client_id = ClientId::from(name);
        let core = ExecutionClientCore::new(
            kraken_config.trader_id,
            client_id,
            *KRAKEN_VENUE,
            oms_type,
            kraken_config.account_id,
            account_type,
            None, // base_currency
            cache,
        );

        match kraken_config.product_type {
            KrakenProductType::Spot => {
                let client = KrakenSpotExecutionClient::new(core, kraken_config)?;
                Ok(Box::new(client))
            }
            KrakenProductType::Futures => {
                let client = KrakenFuturesExecutionClient::new(core, kraken_config)?;
                Ok(Box::new(client))
            }
        }
    }

    fn name(&self) -> &'static str {
        "KRAKEN"
    }

    fn config_type(&self) -> &'static str {
        "KrakenExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
        live::runner::set_data_event_sender,
        messages::DataEvent,
    };
    use rstest::rstest;

    use super::*;
    use crate::common::enums::KrakenProductType;

    fn setup_test_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    #[rstest]
    fn test_kraken_data_client_factory_creation() {
        let factory = KrakenDataClientFactory::new();
        assert_eq!(factory.name(), "KRAKEN");
        assert_eq!(factory.config_type(), "KrakenDataClientConfig");
    }

    #[rstest]
    fn test_kraken_data_client_factory_default() {
        let factory = KrakenDataClientFactory::new();
        assert_eq!(factory.name(), "KRAKEN");
    }

    #[rstest]
    fn test_kraken_data_client_config_implements_client_config() {
        let config = KrakenDataClientConfig {
            product_type: KrakenProductType::Spot,
            ..Default::default()
        };

        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<KrakenDataClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_kraken_data_client_factory_creates_client() {
        setup_test_env();

        let factory = KrakenDataClientFactory::new();
        let config = KrakenDataClientConfig {
            product_type: KrakenProductType::Spot,
            ..Default::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("KRAKEN-TEST", &config, cache, clock);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("KRAKEN-TEST"));
    }

    #[rstest]
    fn test_kraken_execution_client_factory_creates_spot_client_with_netting_oms() {
        let factory = KrakenExecutionClientFactory::new();
        let config = KrakenExecClientConfig {
            product_type: KrakenProductType::Spot,
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("KRAKEN-TEST", &config, cache);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("KRAKEN-TEST"));
        assert_eq!(client.account_id(), config.account_id);
        assert_eq!(client.oms_type(), OmsType::Netting);
    }

    #[rstest]
    fn test_kraken_execution_client_factory_creates_futures_client_with_netting_oms() {
        let factory = KrakenExecutionClientFactory::new();
        let config = KrakenExecClientConfig {
            product_type: KrakenProductType::Futures,
            ..Default::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create("KRAKEN-TEST", &config, cache);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("KRAKEN-TEST"));
        assert_eq!(client.account_id(), config.account_id);
        assert_eq!(client.oms_type(), OmsType::Netting);
    }
}
