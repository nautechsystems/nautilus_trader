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

use std::{fmt::Debug, time::Duration};

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig, msgbus::MessageBusConfig,
};
use nautilus_core::UUID4;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::identifiers::TraderId;
#[cfg(feature = "streaming")]
pub use nautilus_persistence::config::{DataCatalogConfig, RotationConfig, StreamingConfig};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;

/// Configuration trait for a `NautilusKernel` core system instance.
pub trait NautilusKernelConfig: Debug {
    /// Returns the kernel environment context.
    fn environment(&self) -> Environment;
    /// Returns the trader ID for the node.
    fn trader_id(&self) -> TraderId;
    /// Returns if actor and strategy state should be loaded from the database on start.
    fn load_state(&self) -> bool;
    /// Returns if actor and strategy state should be saved to the database on stop.
    fn save_state(&self) -> bool;
    /// Returns if the system should request shutdown when an error log is emitted.
    ///
    /// Filtered or bypassed error logs still request shutdown.
    fn shutdown_on_error(&self) -> bool;
    /// Returns the logging configuration for the kernel.
    fn logging(&self) -> LoggerConfig;
    /// Returns the unique instance identifier for the kernel.
    fn instance_id(&self) -> Option<UUID4>;
    /// Returns the timeout for all clients to connect and initialize.
    fn timeout_connection(&self) -> Duration;
    /// Returns the timeout for execution state to reconcile.
    fn timeout_reconciliation(&self) -> Duration;
    /// Returns the timeout for portfolio to initialize margins and unrealized pnls.
    fn timeout_portfolio(&self) -> Duration;
    /// Returns the timeout for all engine clients to disconnect.
    fn timeout_disconnection(&self) -> Duration;
    /// Returns the timeout after stopping the node to await residual events before final shutdown.
    fn delay_post_stop(&self) -> Duration;
    /// Returns the timeout to await pending tasks cancellation during shutdown.
    fn timeout_shutdown(&self) -> Duration;
    /// Returns the cache configuration.
    fn cache(&self) -> Option<CacheConfig>;
    /// Returns the message bus configuration.
    fn msgbus(&self) -> Option<MessageBusConfig>;
    /// Returns the data engine configuration.
    fn data_engine(&self) -> Option<DataEngineConfig>;
    /// Returns the risk engine configuration.
    fn risk_engine(&self) -> Option<RiskEngineConfig>;
    /// Returns the execution engine configuration.
    fn exec_engine(&self) -> Option<ExecutionEngineConfig>;
    /// Returns the portfolio configuration.
    fn portfolio(&self) -> Option<PortfolioConfig>;
    /// Returns the configuration for streaming to feather files.
    #[cfg(feature = "streaming")]
    fn streaming(&self) -> Option<StreamingConfig> {
        None
    }
    /// Returns configurations for existing data catalogs.
    #[cfg(feature = "streaming")]
    fn catalogs(&self) -> Vec<DataCatalogConfig> {
        Vec::new()
    }
}

/// Basic implementation of `NautilusKernelConfig` for builder and testing.
#[derive(Debug, Clone, bon::Builder)]
pub struct KernelConfig {
    /// The kernel environment context.
    #[builder(default = Environment::Backtest)]
    pub environment: Environment,
    /// The trader ID for the node (must be a name and ID tag separated by a hyphen).
    #[builder(default)]
    pub trader_id: TraderId,
    /// If actor and strategy state should be loaded from the database on start.
    #[builder(default)]
    pub load_state: bool,
    /// If actor and strategy state should be saved to the database on stop.
    #[builder(default)]
    pub save_state: bool,
    /// If the system should request shutdown when an error log is emitted.
    ///
    /// Filtered or bypassed error logs still request shutdown.
    #[builder(default)]
    pub shutdown_on_error: bool,
    /// The logging configuration for the kernel.
    #[builder(default)]
    pub logging: LoggerConfig,
    /// The unique instance identifier for the kernel
    pub instance_id: Option<UUID4>,
    /// The timeout for all clients to connect and initialize.
    #[builder(default = Duration::from_mins(1))]
    pub timeout_connection: Duration,
    /// The timeout for execution state to reconcile.
    #[builder(default = Duration::from_secs(30))]
    pub timeout_reconciliation: Duration,
    /// The timeout for portfolio to initialize margins and unrealized pnls.
    #[builder(default = Duration::from_secs(10))]
    pub timeout_portfolio: Duration,
    /// The timeout for all engine clients to disconnect.
    #[builder(default = Duration::from_secs(10))]
    pub timeout_disconnection: Duration,
    /// The delay after stopping the node to await residual events before final shutdown.
    #[builder(default = Duration::from_secs(10))]
    pub delay_post_stop: Duration,
    /// The delay to await pending tasks cancellation during shutdown.
    #[builder(default = Duration::from_secs(5))]
    pub timeout_shutdown: Duration,
    /// The cache configuration.
    pub cache: Option<CacheConfig>,
    /// The message bus configuration.
    pub msgbus: Option<MessageBusConfig>,
    /// The data engine configuration.
    pub data_engine: Option<DataEngineConfig>,
    /// The risk engine configuration.
    pub risk_engine: Option<RiskEngineConfig>,
    /// The execution engine configuration.
    pub exec_engine: Option<ExecutionEngineConfig>,
    /// The portfolio configuration.
    pub portfolio: Option<PortfolioConfig>,
    /// The configuration for streaming to feather files.
    #[cfg(feature = "streaming")]
    pub streaming: Option<StreamingConfig>,
    /// Configurations for existing data catalogs.
    #[cfg(feature = "streaming")]
    #[builder(default)]
    pub catalogs: Vec<DataCatalogConfig>,
}

impl NautilusKernelConfig for KernelConfig {
    fn environment(&self) -> Environment {
        self.environment
    }

    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    fn load_state(&self) -> bool {
        self.load_state
    }

    fn save_state(&self) -> bool {
        self.save_state
    }

    fn shutdown_on_error(&self) -> bool {
        self.shutdown_on_error
    }

    fn logging(&self) -> LoggerConfig {
        self.logging.clone()
    }

    fn instance_id(&self) -> Option<UUID4> {
        self.instance_id
    }

    fn timeout_connection(&self) -> Duration {
        self.timeout_connection
    }

    fn timeout_reconciliation(&self) -> Duration {
        self.timeout_reconciliation
    }

    fn timeout_portfolio(&self) -> Duration {
        self.timeout_portfolio
    }

    fn timeout_disconnection(&self) -> Duration {
        self.timeout_disconnection
    }

    fn delay_post_stop(&self) -> Duration {
        self.delay_post_stop
    }

    fn timeout_shutdown(&self) -> Duration {
        self.timeout_shutdown
    }

    fn cache(&self) -> Option<CacheConfig> {
        self.cache.clone()
    }

    fn msgbus(&self) -> Option<MessageBusConfig> {
        self.msgbus.clone()
    }

    fn data_engine(&self) -> Option<DataEngineConfig> {
        self.data_engine.clone()
    }

    fn risk_engine(&self) -> Option<RiskEngineConfig> {
        self.risk_engine.clone()
    }

    fn exec_engine(&self) -> Option<ExecutionEngineConfig> {
        self.exec_engine.clone()
    }

    fn portfolio(&self) -> Option<PortfolioConfig> {
        self.portfolio
    }

    #[cfg(feature = "streaming")]
    fn streaming(&self) -> Option<StreamingConfig> {
        self.streaming.clone()
    }

    #[cfg(feature = "streaming")]
    fn catalogs(&self) -> Vec<DataCatalogConfig> {
        self.catalogs.clone()
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::config::ConfigError;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_kernel_config_default_connection_timeout() {
        let config = KernelConfig::default();

        assert_eq!(config.timeout_connection, Duration::from_mins(1));
    }

    #[rstest]
    fn test_streaming_config_builder_valid() {
        let config = StreamingConfig::builder()
            .catalog_path("/data/catalog".to_string())
            .fs_protocol("file".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(config.is_ok());
    }

    #[rstest]
    fn test_streaming_config_zero_flush_interval_rejected() {
        let result = StreamingConfig::builder()
            .catalog_path("/data/catalog".to_string())
            .fs_protocol("file".to_string())
            .flush_interval_ms(0)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "flush_interval_ms")
        );
    }

    #[rstest]
    fn test_streaming_config_empty_catalog_path_rejected() {
        let result = StreamingConfig::builder()
            .catalog_path(String::new())
            .fs_protocol("file".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(
            matches!(result, Err(ConfigError::EmptyField { field }) if field == "catalog_path")
        );
    }

    #[rstest]
    fn test_streaming_config_toml_round_trip() {
        let config: StreamingConfig = toml::from_str(
            r#"
catalog_path = "/data/catalog"
fs_protocol = "file"
flush_interval_ms = 1000
replace_existing = false

[rotation_config.size]
max_size = 1048576
"#,
        )
        .unwrap();

        assert_eq!(config.catalog_path, "/data/catalog");
        assert_eq!(config.fs_protocol, "file");
        assert_eq!(config.flush_interval_ms, 1000);
        assert!(!config.replace_existing);
        assert!(matches!(
            config.rotation_config,
            RotationConfig::Size {
                max_size: 1_048_576
            }
        ));
    }

    #[rstest]
    fn test_streaming_config_with_no_rotation_toml() {
        let config: StreamingConfig = toml::from_str(
            r#"
catalog_path = "/data/catalog"
fs_protocol = "file"
flush_interval_ms = 500
replace_existing = true
rotation_config = "no_rotation"
"#,
        )
        .unwrap();

        assert!(matches!(config.rotation_config, RotationConfig::NoRotation));
        assert!(config.replace_existing);
    }
}
