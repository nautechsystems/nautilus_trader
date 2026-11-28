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

use clap::Parser;
use nautilus_persistence::backend::migration::parse_storage_option;

/// Command-line interface for NautilusTrader.
#[derive(Debug, Parser)]
#[clap(version, about, author)]
pub struct NautilusCli {
    #[clap(subcommand)]
    pub(crate) command: Commands,
}

/// Available top-level commands for the NautilusTrader CLI.
#[derive(Parser, Debug)]
pub enum Commands {
    Database(DatabaseOpt),
    Catalog(CatalogOpt),
}

/// Database management options and subcommands.
#[derive(Parser, Debug)]
#[command(about = "Postgres database operations", long_about = None)]
pub struct DatabaseOpt {
    #[clap(subcommand)]
    pub(crate) command: DatabaseCommand,
}

/// Configuration parameters for database connection and operations.
#[derive(Parser, Debug, Clone)]
pub struct DatabaseConfig {
    /// Hostname or IP address of the database server.
    #[arg(long)]
    pub(crate) host: Option<String>,
    /// Port number of the database server.
    #[arg(long)]
    pub(crate) port: Option<u16>,
    /// Username for connecting to the database.
    #[arg(long)]
    pub(crate) username: Option<String>,
    /// Name of the database.
    #[arg(long)]
    pub(crate) database: Option<String>,
    /// Password for connecting to the database.
    #[arg(long)]
    pub(crate) password: Option<String>,
    /// Directory path to the schema files.
    #[arg(long)]
    pub(crate) schema: Option<String>,
}

/// Available database management commands.
#[derive(Parser, Debug, Clone)]
#[command(about = "Postgres database operations", long_about = None)]
pub enum DatabaseCommand {
    /// Initializes a new Postgres database with the latest schema.
    Init(DatabaseConfig),
    /// Drops roles, privileges and deletes all data from the database.
    Drop(DatabaseConfig),
    /// Assigns an account's events that have no trader to a trader.
    AssignAccount(AssignAccountConfig),
}

/// Configuration for assigning an account's events that have no trader.
///
/// Account events written before trader-scoped persistence carry no trader, and no trader-scoped
/// cache loads them until they are assigned.
#[derive(Parser, Debug, Clone)]
pub struct AssignAccountConfig {
    /// Account ID whose events to assign.
    #[arg(long)]
    pub(crate) account_id: String,
    /// Trader ID to assign the events to.
    #[arg(long)]
    pub(crate) trader_id: String,
    /// Database configuration options.
    #[clap(flatten)]
    pub(crate) database: DatabaseConfig,
}

/// Catalog management commands.
#[derive(Debug, Parser)]
pub struct CatalogOpt {
    #[clap(subcommand)]
    pub(crate) command: CatalogCommand,
}

/// Operations on persisted catalogs.
#[derive(Debug, Parser)]
pub enum CatalogCommand {
    MigrateParquet(CatalogMigrationOpt),
}

/// Convert a Parquet catalog to the current Arrow storage format.
#[derive(Debug, Parser)]
pub struct CatalogMigrationOpt {
    /// Source catalog path or object-store URI.
    pub(crate) source: String,
    /// Empty destination catalog path or object-store URI.
    pub(crate) destination: String,
    /// Validate source schemas without creating the destination.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Source object-store option in key=value form. Can be repeated.
    #[arg(long = "source-option", value_parser = parse_storage_option)]
    pub(crate) source_options: Vec<(String, String)>,
    /// Destination object-store option in key=value form. Can be repeated.
    #[arg(long = "target-option", value_parser = parse_storage_option)]
    pub(crate) target_options: Vec<(String, String)>,
}

#[cfg(test)]
mod database_tests {
    use clap::Parser;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn assign_account_cli_parses_ids_and_connection_options() {
        let cli = NautilusCli::try_parse_from([
            "nautilus",
            "database",
            "assign-account",
            "--account-id",
            "HYPERLIQUID-001",
            "--trader-id",
            "TRADER-001",
            "--host",
            "localhost",
            "--port",
            "5433",
        ])
        .unwrap();

        let Commands::Database(DatabaseOpt {
            command: DatabaseCommand::AssignAccount(config),
        }) = cli.command
        else {
            panic!("Expected assign-account database command");
        };

        assert_eq!(config.account_id, "HYPERLIQUID-001");
        assert_eq!(config.trader_id, "TRADER-001");
        assert_eq!(config.database.host.as_deref(), Some("localhost"));
        assert_eq!(config.database.port, Some(5433));
    }

    #[rstest]
    fn assign_account_cli_requires_account_and_trader() {
        let result = NautilusCli::try_parse_from([
            "nautilus",
            "database",
            "assign-account",
            "--trader-id",
            "T-1",
        ]);

        assert!(result.is_err());
    }
}
