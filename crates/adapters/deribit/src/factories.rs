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

//! Factory functions for creating Deribit clients and components.

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
    common::consts::DERIBIT_VENUE,
    config::{DeribitDataClientConfig, DeribitExecClientConfig},
    data::DeribitDataClient,
    execution::DeribitExecutionClient,
};

impl ClientConfig for DeribitDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Deribit data clients.
#[derive(Debug)]
pub struct DeribitDataClientFactory;

impl DeribitDataClientFactory {
    /// Creates a new [`DeribitDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DeribitDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for DeribitDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let deribit_config = config
            .as_any()
            .downcast_ref::<DeribitDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for DeribitDataClientFactory. Expected DeribitDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = DeribitDataClient::new(client_id, deribit_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "DERIBIT"
    }

    fn config_type(&self) -> &'static str {
        "DeribitDataClientConfig"
    }
}

impl ClientConfig for DeribitExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Deribit execution clients.
#[derive(Debug)]
pub struct DeribitExecutionClientFactory;

impl DeribitExecutionClientFactory {
    /// Creates a new [`DeribitExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DeribitExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for DeribitExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let deribit_config = config
            .as_any()
            .downcast_ref::<DeribitExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for DeribitExecutionClientFactory. Expected DeribitExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        // Deribit uses netting (derivatives only, no hedging)
        let oms_type = OmsType::Netting;
        let account_type = AccountType::Margin;

        let client_id = ClientId::from(name);
        let core = ExecutionClientCore::new(
            deribit_config.trader_id,
            client_id,
            *DERIBIT_VENUE,
            oms_type,
            deribit_config.account_id,
            account_type,
            None, // base_currency
            clock,
            cache,
        );

        let client = DeribitExecutionClient::new(core, deribit_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "DERIBIT"
    }

    fn config_type(&self) -> &'static str {
        "DeribitExecClientConfig"
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache, clock::TestClock, live::runner::set_data_event_sender, messages::DataEvent,
    };
    use nautilus_system::factories::{ClientConfig, DataClientFactory};
    use rstest::rstest;

    use super::*;
    use crate::http::models::DeribitInstrumentKind;

    fn setup_test_env() {
        // Initialize data event sender for tests
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    #[rstest]
    fn test_deribit_data_client_factory_creation() {
        let factory = DeribitDataClientFactory::new();
        assert_eq!(factory.name(), "DERIBIT");
        assert_eq!(factory.config_type(), "DeribitDataClientConfig");
    }

    #[rstest]
    fn test_deribit_data_client_factory_default() {
        let factory = DeribitDataClientFactory::new();
        assert_eq!(factory.name(), "DERIBIT");
    }

    #[rstest]
    fn test_deribit_data_client_config_implements_client_config() {
        let config = DeribitDataClientConfig {
            instrument_kinds: vec![DeribitInstrumentKind::Future],
            ..Default::default()
        };

        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<DeribitDataClientConfig>();

        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_deribit_data_client_factory_creates_client() {
        setup_test_env();

        let factory = DeribitDataClientFactory::new();
        let config = DeribitDataClientConfig {
            instrument_kinds: vec![DeribitInstrumentKind::Future],
            use_testnet: true,
            ..Default::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create("DERIBIT-TEST", &config, cache, clock);
        assert!(result.is_ok());

        let client = result.unwrap();
        assert_eq!(client.client_id(), ClientId::from("DERIBIT-TEST"));
    }
}
