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
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::identifiers::TraderId;
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;

/// Configuration trait for a `NautilusKernel` core system instance.
pub trait NautilusKernelConfig: Debug {
    /// Returns the kernel environment context.
    fn environment(&self) -> Environment;
    /// Returns the trader ID for the node.
    fn trader_id(&self) -> TraderId;
    /// Returns if trading strategy state should be loaded from the database on start.
    fn load_state(&self) -> bool;
    /// Returns if trading strategy state should be saved to the database on stop.
    fn save_state(&self) -> bool;
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
    fn streaming(&self) -> Option<StreamingConfig>;
}

/// Basic implementation of `NautilusKernelConfig` for builder and testing.
#[derive(Debug, Clone)]
pub struct KernelConfig {
    /// The kernel environment context.
    pub environment: Environment,
    /// The trader ID for the node (must be a name and ID tag separated by a hyphen).
    pub trader_id: TraderId,
    /// If trading strategy state should be loaded from the database on start.
    pub load_state: bool,
    /// If trading strategy state should be saved to the database on stop.
    pub save_state: bool,
    /// The logging configuration for the kernel.
    pub logging: LoggerConfig,
    /// The unique instance identifier for the kernel
    pub instance_id: Option<UUID4>,
    /// The timeout for all clients to connect and initialize.
    pub timeout_connection: Duration,
    /// The timeout for execution state to reconcile.
    pub timeout_reconciliation: Duration,
    /// The timeout for portfolio to initialize margins and unrealized pnls.
    pub timeout_portfolio: Duration,
    /// The timeout for all engine clients to disconnect.
    pub timeout_disconnection: Duration,
    /// The delay after stopping the node to await residual events before final shutdown.
    pub delay_post_stop: Duration,
    /// The delay to await pending tasks cancellation during shutdown.
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
    pub streaming: Option<StreamingConfig>,
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
        self.portfolio.clone()
    }

    fn streaming(&self) -> Option<StreamingConfig> {
        self.streaming.clone()
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            environment: Environment::Backtest,
            trader_id: TraderId::default(),
            load_state: false,
            save_state: false,
            logging: LoggerConfig::default(),
            instance_id: None,
            timeout_connection: Duration::from_secs(60),
            timeout_reconciliation: Duration::from_secs(30),
            timeout_portfolio: Duration::from_secs(10),
            timeout_disconnection: Duration::from_secs(10),
            delay_post_stop: Duration::from_secs(10),
            timeout_shutdown: Duration::from_secs(5),
            cache: None,
            msgbus: None,
            data_engine: None,
            risk_engine: None,
            exec_engine: None,
            portfolio: None,
            streaming: None,
        }
    }
}

/// Configuration for file rotation in streaming output.
#[derive(Debug, Clone)]
pub enum RotationConfig {
    /// Rotate based on file size.
    Size {
        /// Maximum buffer size in bytes before rotation.
        max_size: u64,
    },
    /// Rotate based on a time interval.
    Interval {
        /// Interval in nanoseconds.
        interval_ns: u64,
    },
    /// Rotate based on scheduled dates.
    ScheduledDates {
        /// Interval in nanoseconds.
        interval_ns: u64,
        /// Start of the scheduled rotation period.
        schedule_ns: UnixNanos,
    },
    /// No automatic rotation.
    NoRotation,
}

/// Configuration for streaming live or backtest runs to the catalog in feather format.
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// The path to the data catalog.
    pub catalog_path: String,
    /// The `fsspec` filesystem protocol for the catalog.
    pub fs_protocol: String,
    /// The flush interval (milliseconds) for writing chunks.
    pub flush_interval_ms: u64,
    /// If any existing feather files should be replaced.
    pub replace_existing: bool,
    /// Rotation configuration.
    pub rotation_config: RotationConfig,
}

impl StreamingConfig {
    /// Creates a new [`StreamingConfig`] instance.
    #[must_use]
    pub const fn new(
        catalog_path: String,
        fs_protocol: String,
        flush_interval_ms: u64,
        replace_existing: bool,
        rotation_config: RotationConfig,
    ) -> Self {
        Self {
            catalog_path,
            fs_protocol,
            flush_interval_ms,
            replace_existing,
            rotation_config,
        }
    }
}
