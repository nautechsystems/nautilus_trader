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
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    kernel::NautilusKernel,
};
use ustr::Ustr;

/// High-level abstraction for a live trading node.
///
/// Provides a simplified interface for running live trading strategies
/// with automatic client management and lifecycle handling.
#[derive(Debug)]
pub struct TradingNode {
    kernel: NautilusKernel,
    is_running: bool,
}

impl TradingNode {
    /// Creates a new [`TradingNodeBuilder`] for fluent configuration.
    #[must_use]
    pub fn builder(
        name: Ustr,
        trader_id: TraderId,
        environment: Environment,
    ) -> TradingNodeBuilder {
        TradingNodeBuilder::new(name, trader_id, environment)
    }

    /// Starts the trading node.
    ///
    /// # Errors
    ///
    /// Returns an error if startup fails.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.is_running {
            return Err(anyhow::anyhow!("Trading node is already running"));
        }

        log::info!("Starting trading node");
        self.kernel.start();
        self.is_running = true;

        log::info!("Trading node started successfully");
        Ok(())
    }

    /// Stop the trading node.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown fails.
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if !self.is_running {
            return Err(anyhow::anyhow!("Trading node is not running"));
        }

        log::info!("Stopping trading node");
        self.kernel.stop();
        self.is_running = false;

        log::info!("Trading node stopped successfully");
        Ok(())
    }

    /// Run the trading node with automatic shutdown handling.
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

    /// Checks if the trading node is currently running.
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
    pub fn trader_id(&self) -> TraderId {
        self.kernel.trader_id()
    }

    /// Gets the node's instance ID.
    #[must_use]
    pub fn instance_id(&self) -> UUID4 {
        self.kernel.instance_id()
    }

    /// Gets the node's environment.
    #[must_use]
    pub fn environment(&self) -> Environment {
        self.kernel.environment()
    }
}

/// Builder for constructing a [`TradingNode`] with a fluent API.
///
/// Provides configuration options specific to live trading nodes,
/// including client factory registration and timeout settings.
#[derive(Debug)]
pub struct TradingNodeBuilder {
    kernel_builder: nautilus_system::builder::NautilusKernelBuilder,
    data_client_factories: HashMap<String, Box<dyn DataClientFactory>>,
    exec_client_factories: HashMap<String, Box<dyn ExecutionClientFactory>>,
    data_client_configs: HashMap<String, Box<dyn ClientConfig>>,
    exec_client_configs: HashMap<String, Box<dyn ClientConfig>>,
}

impl TradingNodeBuilder {
    /// Creates a new [`TradingNodeBuilder`] with required parameters.
    ///
    /// # Panics
    ///
    /// Panics if `environment` is invalid (BACKTEST).
    #[must_use]
    pub fn new(name: Ustr, trader_id: TraderId, environment: Environment) -> Self {
        match environment {
            Environment::Sandbox | Environment::Live => {}
            Environment::Backtest => {
                panic!("TradingNode cannot be used with Backtest environment");
            }
        }

        Self {
            kernel_builder: NautilusKernel::builder(name, trader_id, environment),
            data_client_factories: HashMap::new(),
            exec_client_factories: HashMap::new(),
            data_client_configs: HashMap::new(),
            exec_client_configs: HashMap::new(),
        }
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

    /// Register a data client factory.
    ///
    /// # Errors
    ///
    /// Returns an error if a factory with the same name is already registered.
    pub fn add_data_client_factory(
        mut self,
        name: String,
        factory: Box<dyn DataClientFactory>,
    ) -> anyhow::Result<Self> {
        if self.data_client_factories.contains_key(&name) {
            return Err(anyhow::anyhow!(
                "Data client factory '{}' is already registered",
                name
            ));
        }

        self.data_client_factories.insert(name, factory);
        Ok(self)
    }

    /// Register an execution client factory.
    ///
    /// # Errors
    ///
    /// Returns an error if a factory with the same name is already registered.
    pub fn add_exec_client_factory(
        mut self,
        name: String,
        factory: Box<dyn ExecutionClientFactory>,
    ) -> anyhow::Result<Self> {
        if self.exec_client_factories.contains_key(&name) {
            return Err(anyhow::anyhow!(
                "Execution client factory '{}' is already registered",
                name
            ));
        }

        self.exec_client_factories.insert(name, factory);
        Ok(self)
    }

    /// Adds a data client configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a configuration with the same name is already added.
    pub fn add_data_client_config(
        mut self,
        name: String,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        if self.data_client_configs.contains_key(&name) {
            return Err(anyhow::anyhow!(
                "Data client configuration '{}' is already added",
                name
            ));
        }

        self.data_client_configs.insert(name, config);
        Ok(self)
    }

    /// Adds an execution client configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a configuration with the same name is already added.
    pub fn add_exec_client_config(
        mut self,
        name: String,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        if self.exec_client_configs.contains_key(&name) {
            return Err(anyhow::anyhow!(
                "Execution client configuration '{}' is already added",
                name
            ));
        }

        self.exec_client_configs.insert(name, config);
        Ok(self)
    }

    /// Build the [`TradingNode`] with the configured settings.
    ///
    /// This will:
    /// 1. Build the underlying kernel
    /// 2. Register all client factories
    /// 3. Create and register all clients
    ///
    /// # Errors
    ///
    /// Returns an error if node construction fails.
    pub fn build(self) -> anyhow::Result<TradingNode> {
        let kernel = self.kernel_builder.build()?;

        // TODO: Register client factories and create clients
        // This would involve:
        // 1. Creating clients using factories and configs
        // 2. Registering clients with the data/execution engines
        // 3. Setting up routing configurations

        log::info!("Trading node built successfully");

        Ok(TradingNode {
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
        let _builder = TradingNode::builder(
            Ustr::from("TestNode"),
            TraderId::from("TRADER-001"),
            Environment::Sandbox,
        );

        // Should not panic
    }

    #[rstest]
    #[should_panic(expected = "TradingNode cannot be used with Backtest environment")]
    fn test_trading_node_builder_rejects_backtest() {
        _ = TradingNode::builder(
            Ustr::from("TestNode"),
            TraderId::from("TRADER-001"),
            Environment::Backtest,
        );

        // Should not panic
    }

    #[rstest]
    fn test_trading_node_builder_fluent_api() {
        let _builder = TradingNode::builder(
            Ustr::from("TestNode"),
            TraderId::from("TRADER-001"),
            Environment::Live,
        )
        .with_timeout_connection(30)
        .with_load_state(false);

        // Should not panic and methods should chain
    }

    #[rstest]
    fn test_trading_node_build() {
        let result = TradingNode::builder(
            Ustr::from("TestNode"),
            TraderId::from("TRADER-001"),
            Environment::Sandbox,
        )
        .build();

        assert!(result.is_ok());
        let node = result.unwrap();
        assert!(!node.is_running());
        assert_eq!(node.environment(), Environment::Sandbox);
    }
}
