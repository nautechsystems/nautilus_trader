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

use nautilus_infrastructure::sql::{
    pg::{connect_pg, drop_postgres, get_postgres_connect_options, init_postgres},
    queries::DatabaseQueries,
};
use nautilus_model::identifiers::{AccountId, TraderId};

use crate::opt::{DatabaseCommand, DatabaseOpt};

/// Executes database management commands for PostgreSQL operations.
///
/// This function handles database initialization, schema setup, and database
/// dropping operations based on the provided command options.
///
/// # Errors
///
/// Returns an error if:
/// - Database connection fails
/// - Schema initialization fails
/// - Database dropping operation fails
/// - Any PostgreSQL operation encounters an error
pub(crate) async fn run_database_command(opt: DatabaseOpt) -> anyhow::Result<()> {
    let command = opt.command.clone();

    match command {
        DatabaseCommand::Init(config) => {
            let pg_connect_options = get_postgres_connect_options(
                config.host,
                config.port,
                config.username,
                config.password,
                config.database,
            );
            log::info!(
                "Connecting to Postgres at {}",
                pg_connect_options.connection_string_masked()
            );

            let pg = connect_pg(pg_connect_options.clone().into()).await?;
            log::info!("Connected");

            init_postgres(
                &pg,
                pg_connect_options.database,
                pg_connect_options.password,
                config.schema,
            )
            .await?;
        }
        DatabaseCommand::Drop(config) => {
            let pg_connect_options = get_postgres_connect_options(
                config.host,
                config.port,
                config.username,
                config.password,
                config.database,
            );
            log::info!(
                "Connecting to Postgres at {}",
                pg_connect_options.connection_string_masked()
            );

            let pg = connect_pg(pg_connect_options.clone().into()).await?;
            log::info!("Connected");

            drop_postgres(&pg, pg_connect_options.database).await?;
        }
        DatabaseCommand::AssignAccount(config) => {
            let account_id = AccountId::new_checked(&config.account_id)?;
            let trader_id = TraderId::new_checked(&config.trader_id)?;
            let database = config.database;
            let pg_connect_options = get_postgres_connect_options(
                database.host,
                database.port,
                database.username,
                database.password,
                database.database,
            );
            log::info!(
                "Connecting to Postgres at {}",
                pg_connect_options.connection_string_masked()
            );

            let pg = connect_pg(pg_connect_options.into()).await?;
            log::info!("Connected");

            let assigned =
                DatabaseQueries::assign_account_trader(&pg, &account_id, &trader_id).await?;
            log::info!("Assigned {assigned} account events of {account_id} to {trader_id}");
        }
    }
    Ok(())
}
