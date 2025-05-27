// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::HashMap;

use nautilus_common::enums::Environment;
use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;
use nautilus_system::{
    config::NautilusKernelConfig,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    kernel::NautilusKernel,
};

use crate::config::LiveNodeConfig;

/// High-level abstraction for a live Nautilus system node.
///
/// Provides a simplified interface for running live systems
/// with automatic client management and lifecycle handling.
#[derive(Debug)]
pub struct LiveNode {
    kernel: NautilusKernel,
    is_running: bool,
}

impl LiveNode {
    /// Creates a new [`LiveNodeBuilder`] for fluent configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment is invalid for live trading.
    pub fn builder(
        name: String,
        trader_id: TraderId,
        environment: Environment,
    ) -> anyhow::Result<LiveNodeBuilder> {
        LiveNodeBuilder::new(name, trader_id, environment)
    }

    /// Creates a new [`LiveNode`] directly from a kernel name and optional configuration.
    ///
    /// This is a convenience method for creating a live node with a pre-configured
    /// kernel configuration, bypassing the builder pattern. If no config is provided,
    /// a default configuration will be used.
    ///
    /// # Errors
    ///
    /// Returns an error if kernel construction fails.
    pub fn build(
        name: String,
        kernel_config: Option<NautilusKernelConfig>,
    ) -> anyhow::Result<Self> {
        let config = kernel_config.unwrap_or_default();

        // Validate environment for live trading
        match config.environment {
            Environment::Sandbox | Environment::Live => {}
            Environment::Backtest => {
                anyhow::bail!("LiveNode cannot be used with Backtest environment");
            }
        }

        let kernel = NautilusKernel::new(name, config)?;

        log::info!("LiveNode built successfully with kernel config");

        Ok(Self {
            kernel,
            is_running: false,
        })
    }

    /// Starts the live node.
    ///
    /// # Errors
    ///
    /// Returns an error if startup fails.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.is_running {
            anyhow::bail!("LiveNode is already running");
        }

        log::info!("Starting live node");
        self.kernel.start();
        self.is_running = true;

        log::info!("LiveNode started successfully");
        Ok(())
    }

    /// Stop the live node.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown fails.
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if !self.is_running {
            anyhow::bail!("LiveNode is not running");
        }

        log::info!("Stopping live node");
        self.kernel.stop();
        self.is_running = false;

        log::info!("LiveNode stopped successfully");
        Ok(())
    }

    /// Run the live node with automatic shutdown handling.
    ///
    /// This method will start the node, run indefinitely, and handle
    /// graceful shutdown on interrupt signals.
    ///
    /// # Errors
    ///
    /// Returns an error if the node fails to start or encounters a runtime error.
    pub async fn run_async(&mut self) -> anyhow::Result<()> {
        self.start().await?;

        // Set up signal handling
        let sigint = tokio::signal::ctrl_c();

        tokio::select! {
            _ = sigint => {
                log::info!("Received SIGINT, shutting down...");
            }
        }

        self.stop().await?;
        Ok(())
    }

    /// Checks if the live node is currently running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.is_running
    }

    /// Gets a reference to the underlying kernel.
    #[must_use]
    pub const fn kernel(&self) -> &NautilusKernel {
        &self.kernel
    }

    /// Gets the node's trader ID.
    #[must_use]
    pub const fn trader_id(&self) -> TraderId {
        self.kernel.trader_id()
    }

    /// Gets the node's instance ID.
    #[must_use]
    pub const fn instance_id(&self) -> UUID4 {
        self.kernel.instance_id()
    }

    /// Gets the node's environment.
    #[must_use]
    pub const fn environment(&self) -> Environment {
        self.kernel.environment()
    }
}

/// Builder for constructing a [`LiveNode`] with a fluent API.
///
/// Provides configuration options specific to live nodes,
/// including client factory registration and timeout settings.
#[derive(Debug)]
pub struct LiveNodeBuilder {
    kernel_builder: nautilus_system::builder::NautilusKernelBuilder,
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
    pub fn new(
        name: String,
        trader_id: TraderId,
        environment: Environment,
    ) -> anyhow::Result<Self> {
        match environment {
            Environment::Sandbox | Environment::Live => {}
            Environment::Backtest => {
                anyhow::bail!("LiveNode cannot be used with Backtest environment");
            }
        }

        Ok(Self {
            kernel_builder: NautilusKernel::builder(name, trader_id, environment),
            data_client_factories: HashMap::new(),
            exec_client_factories: HashMap::new(),
            data_client_configs: HashMap::new(),
            exec_client_configs: HashMap::new(),
        })
    }

    /// Set the instance ID for the node.
    #[must_use]
    pub fn with_instance_id(mut self, instance_id: UUID4) -> Self {
        self.kernel_builder = self.kernel_builder.with_instance_id(instance_id);
        self
    }

    /// Configure whether to load state on startup.
    #[must_use]
    pub fn with_load_state(mut self, load_state: bool) -> Self {
        self.kernel_builder = self.kernel_builder.with_load_state(load_state);
        self
    }

    /// Configure whether to save state on shutdown.
    #[must_use]
    pub fn with_save_state(mut self, save_state: bool) -> Self {
        self.kernel_builder = self.kernel_builder.with_save_state(save_state);
        self
    }

    /// Set the connection timeout in seconds.
    #[must_use]
    pub fn with_timeout_connection(mut self, timeout: u32) -> Self {
        self.kernel_builder = self.kernel_builder.with_timeout_connection(timeout);
        self
    }

    /// Set the reconciliation timeout in seconds.
    #[must_use]
    pub fn with_timeout_reconciliation(mut self, timeout: u32) -> Self {
        self.kernel_builder = self.kernel_builder.with_timeout_reconciliation(timeout);
        self
    }

    /// Set the portfolio initialization timeout in seconds.
    #[must_use]
    pub fn with_timeout_portfolio(mut self, timeout: u32) -> Self {
        self.kernel_builder = self.kernel_builder.with_timeout_portfolio(timeout);
        self
    }

    /// Set the disconnection timeout in seconds.
    #[must_use]
    pub fn with_timeout_disconnection(mut self, timeout: u32) -> Self {
        self.kernel_builder = self.kernel_builder.with_timeout_disconnection(timeout);
        self
    }

    /// Set the post-stop timeout in seconds.
    #[must_use]
    pub fn with_timeout_post_stop(mut self, timeout: u32) -> Self {
        self.kernel_builder = self.kernel_builder.with_timeout_post_stop(timeout);
        self
    }

    /// Set the shutdown timeout in seconds.
    #[must_use]
    pub fn with_timeout_shutdown(mut self, timeout: u32) -> Self {
        self.kernel_builder = self.kernel_builder.with_timeout_shutdown(timeout);
        self
    }

    /// Configure with optional kernel and node configs.
    ///
    /// Both configs are optional and will use defaults if not provided.
    /// Node config settings will be applied on top of any existing builder settings.
    ///
    /// # Errors
    ///
    /// Returns an error if the configurations contain invalid values.
    pub fn with_configs(
        mut self,
        kernel_config: Option<NautilusKernelConfig>,
        node_config: Option<LiveNodeConfig>,
    ) -> anyhow::Result<Self> {
        if let Some(config) = kernel_config {
            // Validate environment compatibility
            match config.environment {
                Environment::Sandbox | Environment::Live => {}
                Environment::Backtest => {
                    anyhow::bail!(
                        "LiveNode cannot be used with Backtest environment from kernel config"
                    );
                }
            }

            // Update kernel builder with config settings
            self.kernel_builder = self
                .kernel_builder
                .with_load_state(config.load_state)
                .with_save_state(config.save_state);

            self.kernel_builder = self
                .kernel_builder
                .with_timeout_connection(config.timeout_connection);
            self.kernel_builder = self
                .kernel_builder
                .with_timeout_reconciliation(config.timeout_reconciliation);
            self.kernel_builder = self
                .kernel_builder
                .with_timeout_portfolio(config.timeout_portfolio);
            self.kernel_builder = self
                .kernel_builder
                .with_timeout_disconnection(config.timeout_disconnection);
            self.kernel_builder = self
                .kernel_builder
                .with_timeout_post_stop(config.timeout_post_stop);
            self.kernel_builder = self
                .kernel_builder
                .with_timeout_shutdown(config.timeout_shutdown);
            if let Some(cache_config) = config.cache {
                self.kernel_builder = self.kernel_builder.with_cache_config(cache_config);
            }
            if let Some(data_engine_config) = config.data_engine {
                self.kernel_builder = self
                    .kernel_builder
                    .with_data_engine_config(data_engine_config);
            }
            if let Some(risk_engine_config) = config.risk_engine {
                self.kernel_builder = self
                    .kernel_builder
                    .with_risk_engine_config(risk_engine_config);
            }
            if let Some(exec_engine_config) = config.exec_engine {
                self.kernel_builder = self
                    .kernel_builder
                    .with_exec_engine_config(exec_engine_config);
            }
            if let Some(portfolio_config) = config.portfolio {
                self.kernel_builder = self.kernel_builder.with_portfolio_config(portfolio_config);
            }
        }

        if let Some(config) = node_config {
            // Validate environment compatibility TODO: Extract this
            match config.environment {
                Environment::Sandbox | Environment::Live => {}
                Environment::Backtest => {
                    anyhow::bail!(
                        "LiveNode cannot be used with Backtest environment from node config"
                    );
                }
            }

            self.kernel_builder = self
                .kernel_builder
                .with_data_engine_config(config.data_engine.into())
                .with_risk_engine_config(config.risk_engine.into())
                .with_exec_engine_config(config.exec_engine.into());

            // Note: data_clients and exec_clients would need to be handled differently
            // since they contain the actual client configurations, not just factory configs
            // TODO: client configs should be added via add_data_client/add_exec_client
        }

        Ok(self)
    }

    /// Adds a data client with both factory and configuration.
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

    /// Adds an execution client with both factory and configuration.
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
    /// 1. Build the underlying kernel
    /// 2. Register all client factories
    /// 3. Create and register all clients
    ///
    /// # Errors
    ///
    /// Returns an error if node construction fails.
    pub fn build(self) -> anyhow::Result<LiveNode> {
        let kernel = self.kernel_builder.build()?;

        // TODO: Register client factories and create clients
        // This would involve:
        // 1. Creating clients using factories and configs
        // 2. Registering clients with the data/execution engines
        // 3. Setting up routing configurations

        log::info!("LiveNode built successfully");

        Ok(LiveNode {
            kernel,
            is_running: false,
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::TraderId;
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_trading_node_builder_creation() {
        let result = LiveNode::builder(
            "TestNode".to_string(),
            TraderId::from("TRADER-001"),
            Environment::Sandbox,
        );

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_trading_node_builder_rejects_backtest() {
        let result = LiveNode::builder(
            "TestNode".to_string(),
            TraderId::from("TRADER-001"),
            Environment::Backtest,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Backtest environment")
        );
    }

    #[rstest]
    fn test_trading_node_builder_fluent_api() {
        let result = LiveNode::builder(
            "TestNode".to_string(),
            TraderId::from("TRADER-001"),
            Environment::Live,
        );

        assert!(result.is_ok());
        let _builder = result
            .unwrap()
            .with_timeout_connection(30)
            .with_load_state(false);

        // Should not panic and methods should chain
    }

    #[rstest]
    fn test_trading_node_build() {
        let builder_result = LiveNode::builder(
            "TestNode".to_string(),
            TraderId::from("TRADER-001"),
            Environment::Sandbox,
        );

        assert!(builder_result.is_ok());
        let build_result = builder_result.unwrap().build();

        assert!(build_result.is_ok());
        let node = build_result.unwrap();
        assert!(!node.is_running());
        assert_eq!(node.environment(), Environment::Sandbox);
    }

    #[rstest]
    fn test_with_configs_rejects_backtest_environment() {
        use nautilus_system::config::NautilusKernelConfig;

        let builder = LiveNode::builder(
            "TestNode".to_string(),
            TraderId::from("TRADER-001"),
            Environment::Sandbox,
        )
        .unwrap();

        // Create a kernel config with backtest environment
        let mut kernel_config = NautilusKernelConfig::default();
        kernel_config.environment = Environment::Backtest;

        let result = builder.with_configs(Some(kernel_config), None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Backtest environment")
        );
    }
}
