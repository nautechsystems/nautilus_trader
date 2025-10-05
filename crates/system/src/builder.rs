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

use std::time::Duration;

use nautilus_common::{cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig};
use nautilus_core::UUID4;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::identifiers::TraderId;
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;

use crate::{config::KernelConfig, kernel::NautilusKernel};

/// Builder for constructing a [`NautilusKernel`] with a fluent API.
///
/// Provides a convenient way to configure and build a kernel instance with
/// optional components and settings.
#[derive(Debug)]
pub struct NautilusKernelBuilder {
    name: String,
    trader_id: TraderId,
    environment: Environment,
    instance_id: Option<UUID4>,
    load_state: bool,
    save_state: bool,
    logging: Option<LoggerConfig>,
    timeout_connection: Duration,
    timeout_reconciliation: Duration,
    timeout_portfolio: Duration,
    timeout_disconnection: Duration,
    delay_post_stop: Duration,
    timeout_shutdown: Duration,
    cache: Option<CacheConfig>,
    data_engine: Option<DataEngineConfig>,
    risk_engine: Option<RiskEngineConfig>,
    exec_engine: Option<ExecutionEngineConfig>,
    portfolio: Option<PortfolioConfig>,
}

impl NautilusKernelBuilder {
    /// Creates a new [`NautilusKernelBuilder`] with required parameters.
    #[must_use]
    pub const fn new(name: String, trader_id: TraderId, environment: Environment) -> Self {
        Self {
            name,
            trader_id,
            environment,
            instance_id: None,
            load_state: true,
            save_state: true,
            logging: None,
            timeout_connection: Duration::from_secs(60),
            timeout_reconciliation: Duration::from_secs(30),
            timeout_portfolio: Duration::from_secs(10),
            timeout_disconnection: Duration::from_secs(10),
            delay_post_stop: Duration::from_secs(10),
            timeout_shutdown: Duration::from_secs(5),
            cache: None,
            data_engine: None,
            risk_engine: None,
            exec_engine: None,
            portfolio: None,
        }
    }

    /// Set the instance ID for the kernel.
    #[must_use]
    pub const fn with_instance_id(mut self, instance_id: UUID4) -> Self {
        self.instance_id = Some(instance_id);
        self
    }

    /// Configure whether to load state on startup.
    #[must_use]
    pub const fn with_load_state(mut self, load_state: bool) -> Self {
        self.load_state = load_state;
        self
    }

    /// Configure whether to save state on shutdown.
    #[must_use]
    pub const fn with_save_state(mut self, save_state: bool) -> Self {
        self.save_state = save_state;
        self
    }

    /// Set the logging configuration.
    #[must_use]
    pub fn with_logging_config(mut self, config: LoggerConfig) -> Self {
        self.logging = Some(config);
        self
    }

    /// Set the connection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_connection(mut self, timeout_secs: u64) -> Self {
        self.timeout_connection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the reconciliation timeout in seconds.
    #[must_use]
    pub const fn with_timeout_reconciliation(mut self, timeout_secs: u64) -> Self {
        self.timeout_reconciliation = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the portfolio initialization timeout in seconds.
    #[must_use]
    pub const fn with_timeout_portfolio(mut self, timeout_secs: u64) -> Self {
        self.timeout_portfolio = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the disconnection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_disconnection(mut self, timeout_secs: u64) -> Self {
        self.timeout_disconnection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the post-stop delay in seconds.
    #[must_use]
    pub const fn with_delay_post_stop(mut self, delay_secs: u64) -> Self {
        self.delay_post_stop = Duration::from_secs(delay_secs);
        self
    }

    /// Set the shutdown timeout in seconds.
    #[must_use]
    pub const fn with_timeout_shutdown(mut self, timeout_secs: u64) -> Self {
        self.timeout_shutdown = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the cache configuration.
    #[must_use]
    pub fn with_cache_config(mut self, config: CacheConfig) -> Self {
        self.cache = Some(config);
        self
    }

    /// Set the data engine configuration.
    #[must_use]
    pub fn with_data_engine_config(mut self, config: DataEngineConfig) -> Self {
        self.data_engine = Some(config);
        self
    }

    /// Set the risk engine configuration.
    #[must_use]
    pub fn with_risk_engine_config(mut self, config: RiskEngineConfig) -> Self {
        self.risk_engine = Some(config);
        self
    }

    /// Set the execution engine configuration.
    #[must_use]
    pub fn with_exec_engine_config(mut self, config: ExecutionEngineConfig) -> Self {
        self.exec_engine = Some(config);
        self
    }

    /// Set the portfolio configuration.
    #[must_use]
    pub const fn with_portfolio_config(mut self, config: PortfolioConfig) -> Self {
        self.portfolio = Some(config);
        self
    }

    /// Build the [`NautilusKernel`] with the configured settings.
    ///
    /// # Errors
    ///
    /// Returns an error if kernel initialization fails.
    pub fn build(self) -> anyhow::Result<NautilusKernel> {
        let config = KernelConfig {
            environment: self.environment,
            trader_id: self.trader_id,
            load_state: self.load_state,
            save_state: self.save_state,
            logging: self.logging.unwrap_or_default(),
            instance_id: self.instance_id,
            timeout_connection: self.timeout_connection,
            timeout_reconciliation: self.timeout_reconciliation,
            timeout_portfolio: self.timeout_portfolio,
            timeout_disconnection: self.timeout_disconnection,
            delay_post_stop: self.delay_post_stop,
            timeout_shutdown: self.timeout_shutdown,
            cache: self.cache,
            msgbus: None, // msgbus config - not exposed in builder yet
            data_engine: self.data_engine,
            risk_engine: self.risk_engine,
            exec_engine: self.exec_engine,
            portfolio: self.portfolio,
            streaming: None, // streaming config - not exposed in builder yet
        };

        NautilusKernel::new(self.name, config)
    }
}

impl Default for NautilusKernelBuilder {
    /// Create a default builder with minimal configuration for testing/development.
    fn default() -> Self {
        Self::new(
            "NautilusKernel".to_string(),
            TraderId::default(),
            Environment::Backtest,
        )
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
    fn test_builder_default() {
        let builder = NautilusKernelBuilder::default();
        assert_eq!(builder.name, "NautilusKernel");
        assert_eq!(builder.environment, Environment::Backtest);
        assert!(builder.load_state);
        assert!(builder.save_state);
    }

    #[rstest]
    fn test_builder_fluent_api() {
        let trader_id = TraderId::from("TRADER-001");
        let instance_id = UUID4::new();

        let builder =
            NautilusKernelBuilder::new("TestKernel".to_string(), trader_id, Environment::Live)
                .with_instance_id(instance_id)
                .with_load_state(false)
                .with_save_state(false)
                .with_timeout_connection(30);

        assert_eq!(builder.name, "TestKernel");
        assert_eq!(builder.trader_id, trader_id);
        assert_eq!(builder.environment, Environment::Live);
        assert_eq!(builder.instance_id, Some(instance_id));
        assert!(!builder.load_state);
        assert!(!builder.save_state);
        assert_eq!(builder.timeout_connection, Duration::from_secs(30));
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_builder_build() {
        let result = NautilusKernelBuilder::default().build();
        assert!(result.is_ok());

        let kernel = result.unwrap();
        assert_eq!(kernel.name(), "NautilusKernel".to_string());
        assert_eq!(kernel.environment(), Environment::Backtest);
    }

    #[rstest]
    fn test_builder_with_configs() {
        let cache_config = CacheConfig::default();
        let data_engine_config = DataEngineConfig::default();

        let builder = NautilusKernelBuilder::default()
            .with_cache_config(cache_config)
            .with_data_engine_config(data_engine_config);

        assert!(builder.cache.is_some());
        assert!(builder.data_engine.is_some());
    }
}
