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

//! Command-line interface and tools for [NautilusTrader](http://nautilustrader.io).
//!
//! The `nautilus-cli` crate provides a comprehensive command-line interface for managing and
//! operating NautilusTrader installations. It includes tools for database management,
//! system configuration, and operational utilities:
//!
//! - Database initialization and management commands.
//! - PostgreSQL schema setup and maintenance.
//! - Configuration validation and setup utilities.
//! - System administration and operational tools.
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
//! highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case:
//!
//! - `defi`: Enables DeFi functionality including blockchain data access and pool analysis.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

#[cfg(feature = "defi")]
mod blockchain;
mod database;
pub mod opt;

#[cfg(feature = "defi")]
use crate::blockchain::run_blockchain_command;
use crate::{
    database::postgres::run_database_command,
    opt::{Commands, NautilusCli},
};

/// Runs the Nautilus CLI based on the provided options.
///
/// # Errors
///
/// Returns an error if execution of the specified command fails.
pub async fn run(opt: NautilusCli) -> anyhow::Result<()> {
    match opt.command {
        Commands::Database(database_opt) => run_database_command(database_opt).await?,
        #[cfg(feature = "defi")]
        Commands::Blockchain(blockchain_opt) => run_blockchain_command(blockchain_opt).await?,
    }
    Ok(())
}
