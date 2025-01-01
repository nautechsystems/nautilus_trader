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

use nautilus_infrastructure::sql::pg::{
    connect_pg, drop_postgres, get_postgres_connect_options, init_postgres,
};

use crate::opt::{DatabaseCommand, DatabaseOpt};

pub async fn run_database_command(opt: DatabaseOpt) -> anyhow::Result<()> {
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
            let pg = connect_pg(pg_connect_options.clone().into()).await?;
            log::info!(
                "Connected with Postgres on url: {}",
                pg_connect_options.connection_string()
            );
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
            let pg = connect_pg(pg_connect_options.clone().into()).await?;
            log::info!(
                "Connected with Postgres on url: {}",
                pg_connect_options.connection_string()
            );
            drop_postgres(&pg, pg_connect_options.database).await?;
        }
    }
    Ok(())
}
