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

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::UUID4;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::identifiers::TraderId;
use nautilus_persistence::config::StreamingConfig;
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;

#[derive(Debug, Clone)]
/// Configuration for a `NautilusKernel` core system instance.
pub struct NautilusKernelConfig {
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
    /// The timeout (seconds) for all clients to connect and initialize.
    pub timeout_connection: u32,
    /// The timeout (seconds) for execution state to reconcile.
    pub timeout_reconciliation: u32,
    /// The timeout (seconds) for portfolio to initialize margins and unrealized pnls.
    pub timeout_portfolio: u32,
    /// The timeout (seconds) for all engine clients to disconnect.
    pub timeout_disconnection: u32,
    /// The timeout (seconds) after stopping the node to await residual events before final shutdown.
    pub timeout_post_stop: u32,
    /// The timeout (seconds) to await pending tasks cancellation during shutdown.
    pub timeout_shutdown: u32,
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

impl NautilusKernelConfig {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        environment: Environment,
        trader_id: TraderId,
        load_state: Option<bool>,
        save_state: Option<bool>,
        timeout_connection: Option<u32>,
        timeout_reconciliation: Option<u32>,
        timeout_portfolio: Option<u32>,
        timeout_disconnection: Option<u32>,
        timeout_post_stop: Option<u32>,
        timeout_shutdown: Option<u32>,
        logging: Option<LoggerConfig>,
        instance_id: Option<UUID4>,
        cache: Option<CacheConfig>,
        msgbus: Option<MessageBusConfig>,
        data_engine: Option<DataEngineConfig>,
        risk_engine: Option<RiskEngineConfig>,
        exec_engine: Option<ExecutionEngineConfig>,
        portfolio: Option<PortfolioConfig>,
        streaming: Option<StreamingConfig>,
    ) -> Self {
        Self {
            environment,
            trader_id,
            instance_id,
            cache,
            msgbus,
            data_engine,
            risk_engine,
            exec_engine,
            portfolio,
            streaming,
            load_state: load_state.unwrap_or(true),
            save_state: save_state.unwrap_or(true),
            timeout_connection: timeout_connection.unwrap_or(60),
            timeout_reconciliation: timeout_reconciliation.unwrap_or(30),
            timeout_portfolio: timeout_portfolio.unwrap_or(10),
            timeout_disconnection: timeout_disconnection.unwrap_or(10),
            timeout_post_stop: timeout_post_stop.unwrap_or(10),
            timeout_shutdown: timeout_shutdown.unwrap_or(5),
            logging: logging.unwrap_or_default(),
        }
    }
}

impl Default for NautilusKernelConfig {
    fn default() -> Self {
        Self {
            environment: Environment::Backtest,
            trader_id: TraderId::default(),
            load_state: false,
            save_state: false,
            logging: LoggerConfig::default(),
            instance_id: None,
            timeout_connection: 60,
            timeout_reconciliation: 30,
            timeout_portfolio: 10,
            timeout_disconnection: 10,
            timeout_post_stop: 10,
            timeout_shutdown: 5,
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
