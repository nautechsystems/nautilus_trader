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

//! Builder for constructing [`LiveNode`] instances.

use std::{collections::HashMap, time::Duration};

use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_core::UUID4;
use nautilus_data::client::DataClientAdapter;
use nautilus_model::identifiers::TraderId;
use nautilus_system::{
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    kernel::NautilusKernel,
};

use crate::{
    config::LiveNodeConfig,
    manager::{ExecutionManager, ExecutionManagerConfig},
    node::LiveNode,
    runner::AsyncRunner,
};

/// Builder for constructing a [`LiveNode`] with a fluent API.
///
/// Provides configuration options specific to live nodes,
/// including client factory registration and timeout settings.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", unsendable)
)]
pub struct LiveNodeBuilder {
    name: String,
    config: LiveNodeConfig,
    data_client_factories: HashMap<String, Box<dyn DataClientFactory>>,
    exec_client_factories: HashMap<String, Box<dyn ExecutionClientFactory>>,
    data_client_configs: HashMap<String, Box<dyn ClientConfig>>,
    exec_client_configs: HashMap<String, Box<dyn ClientConfig>>,
}

impl LiveNodeBuilder {
    /// Creates a new [`LiveNodeBuilder`] with required parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if `environment` is invalid (BACKTEST).
    pub fn new(trader_id: TraderId, environment: Environment) -> anyhow::Result<Self> {
        match environment {
            Environment::Sandbox | Environment::Live => {}
            Environment::Backtest => {
                anyhow::bail!("LiveNode cannot be used with Backtest environment");
            }
        }

        let config = LiveNodeConfig {
            environment,
            trader_id,
            ..Default::default()
        };

        Ok(Self {
            name: "LiveNode".to_string(),
            config,
            data_client_factories: HashMap::new(),
            exec_client_factories: HashMap::new(),
            data_client_configs: HashMap::new(),
            exec_client_configs: HashMap::new(),
        })
    }

    /// Returns the name for the node.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the name for the node.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the instance ID for the node.
    #[must_use]
    pub const fn with_instance_id(mut self, instance_id: UUID4) -> Self {
        self.config.instance_id = Some(instance_id);
        self
    }

    /// Configure whether to load state on startup.
    #[must_use]
    pub const fn with_load_state(mut self, load_state: bool) -> Self {
        self.config.load_state = load_state;
        self
    }

    /// Configure whether to save state on shutdown.
    #[must_use]
    pub const fn with_save_state(mut self, save_state: bool) -> Self {
        self.config.save_state = save_state;
        self
    }

    /// Set the connection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_connection(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_connection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the reconciliation timeout in seconds.
    #[must_use]
    pub const fn with_timeout_reconciliation(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_reconciliation = Duration::from_secs(timeout_secs);
        self
    }

    /// Configure whether to run startup reconciliation.
    #[must_use]
    pub fn with_reconciliation(mut self, reconciliation: bool) -> Self {
        self.config.exec_engine.reconciliation = reconciliation;
        self
    }

    /// Set the reconciliation lookback in minutes.
    #[must_use]
    pub fn with_reconciliation_lookback_mins(mut self, mins: u32) -> Self {
        self.config.exec_engine.reconciliation_lookback_mins = Some(mins);
        self
    }

    /// Set the portfolio initialization timeout in seconds.
    #[must_use]
    pub const fn with_timeout_portfolio(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_portfolio = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the disconnection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_disconnection_secs(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_disconnection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the post-stop delay in seconds.
    #[must_use]
    pub const fn with_delay_post_stop_secs(mut self, delay_secs: u64) -> Self {
        self.config.delay_post_stop = Duration::from_secs(delay_secs);
        self
    }

    /// Set the shutdown timeout in seconds.
    #[must_use]
    pub const fn with_delay_shutdown_secs(mut self, delay_secs: u64) -> Self {
        self.config.timeout_shutdown = Duration::from_secs(delay_secs);
        self
    }

    /// Set the logging configuration.
    #[must_use]
    pub fn with_logging(mut self, logging: LoggerConfig) -> Self {
        self.config.logging = logging;
        self
    }

    /// Adds a data client factory with configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_data_client(
        mut self,
        name: Option<String>,
        factory: Box<dyn DataClientFactory>,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| factory.name().to_string());

        if self.data_client_factories.contains_key(&name) {
            anyhow::bail!("Data client '{name}' is already registered");
        }

        self.data_client_factories.insert(name.clone(), factory);
        self.data_client_configs.insert(name, config);
        Ok(self)
    }

    /// Adds an execution client factory with configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_exec_client(
        mut self,
        name: Option<String>,
        factory: Box<dyn ExecutionClientFactory>,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| factory.name().to_string());

        if self.exec_client_factories.contains_key(&name) {
            anyhow::bail!("Execution client '{name}' is already registered");
        }

        self.exec_client_factories.insert(name.clone(), factory);
        self.exec_client_configs.insert(name, config);
        Ok(self)
    }

    /// Build the [`LiveNode`] with the configured settings.
    ///
    /// This will:
    /// 1. Build the underlying kernel.
    /// 2. Create clients using factories.
    /// 3. Register clients with engines.
    ///
    /// # Errors
    ///
    /// Returns an error if node construction fails.
    pub fn build(mut self) -> anyhow::Result<LiveNode> {
        log::info!(
            "Building LiveNode with {} data clients and {} execution clients",
            self.data_client_factories.len(),
            self.exec_client_factories.len()
        );

        let runner = AsyncRunner::new();
        let kernel = NautilusKernel::new(self.name.clone(), self.config.clone())?;

        for (name, factory) in self.data_client_factories {
            if let Some(config) = self.data_client_configs.remove(&name) {
                log::debug!("Creating data client {name}");

                let client =
                    factory.create(&name, config.as_ref(), kernel.cache(), kernel.clock())?;
                let client_id = client.client_id();
                let venue = client.venue();

                let adapter = DataClientAdapter::new(
                    client_id, venue, true, // handles_order_book_deltas
                    true, // handles_order_book_snapshots
                    client,
                );

                kernel
                    .data_engine
                    .borrow_mut()
                    .register_client(adapter, venue);

                log::info!("Registered DataClient-{client_id}");
            } else {
                log::warn!("No config found for data client factory {name}");
            }
        }

        for (name, factory) in self.exec_client_factories {
            if let Some(config) = self.exec_client_configs.remove(&name) {
                log::debug!("Creating execution client {name}");

                let client = factory.create(&name, config.as_ref(), kernel.cache())?;
                let client_id = client.client_id();

                kernel.exec_engine.borrow_mut().register_client(client)?;

                log::info!("Registered ExecutionClient-{client_id}");
            } else {
                log::warn!("No config found for execution client factory {name}");
            }
        }

        let exec_manager_config = ExecutionManagerConfig::from(&self.config.exec_engine)
            .with_trader_id(self.config.trader_id);
        let exec_manager = ExecutionManager::new(
            kernel.clock.clone(),
            kernel.cache.clone(),
            exec_manager_config,
        );

        log::info!("Built successfully");

        Ok(LiveNode::new_from_builder(
            kernel,
            runner,
            self.config,
            exec_manager,
        ))
    }
}
