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

//! NautilusTrader adapter for Rithmic futures trading.
//!
//! This crate provides connectivity to Rithmic's R | Protocol API™
//! for market data and order execution on futures exchanges.
//!
//! # Architecture
//!
//! The adapter follows NautilusTrader's layered architecture:
//! - **Rust layer**: Performance-critical networking, parsing, and data transformation
//! - **Python layer**: Integration with NautilusTrader's data and execution engines
//!
//! # Modules
//!
//! - [`common`]: Shared utilities, constants, and type converters
//! - [`config`]: Configuration types for data and execution clients
//! - [`data`]: Market data client for streaming quotes and trades
//! - [`execution`]: Execution client for order management
//! - [`instruments`]: Instrument provider for loading contract definitions
//! - [`providers`]: Account and position state providers
//!
//! # Example
//!
//! ```rust,ignore
//! use nautilus_rithmic::{RithmicDataClient, RithmicDataClientConfig};
//!
//! let config = RithmicDataClientConfig::from_env()?;
//! let mut client = RithmicDataClient::new(config);
//! client.connect().await?;
//! ```

pub mod common;
pub mod config;
pub mod data;
pub mod error;
pub mod execution;
pub mod gateway;
pub mod instruments;
pub mod providers;

#[cfg(feature = "python")]
pub mod python;

// Re-exports for convenient access
#[allow(deprecated)]
pub use config::{
    RithmicDataClientConfig, RithmicEnv, RithmicEnvironment, RithmicExecClientConfig,
};
pub use data::{RithmicBarType, RithmicDataClient};
pub use error::{Result, RithmicError, RithmicWsError, WsResult};
pub use execution::RithmicExecutionClient;
pub use gateway::{GatewayConfig, InstrumentInfo, PnlEvent, RithmicGateway};
// Re-export bar types for historical data requests
pub use instruments::RithmicInstrumentProvider;
pub use providers::{RithmicAccountProvider, RithmicPositionProvider};
pub use rithmic_rs::rti::request_time_bar_replay::BarType as TimeBarType;
