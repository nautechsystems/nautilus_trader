// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use sqlx::{postgres::PgConnectOptions, query, ConnectOptions, PgPool};

use crate::sql::NAUTILUS_TABLES;

#[derive(Debug, Clone)]
pub struct PostgresConnectOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
}

impl PostgresConnectOptions {
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: String,
        database: String,
    ) -> Self {
        Self {
            host,
            port,
            username,
            password,
            database,
        }
    }
}

impl From<PostgresConnectOptions> for PgConnectOptions {
    fn from(opt: PostgresConnectOptions) -> Self {
        PgConnectOptions::new()
            .host(opt.host.as_str())
            .port(opt.port)
            .username(opt.username.as_str())
            .password(opt.password.as_str())
            .database(opt.database.as_str())
            .disable_statement_logging()
    }
}

pub fn get_postgres_connect_options(
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    database: Option<String>,
) -> anyhow::Result<PostgresConnectOptions> {
    let host = match host.or_else(|| std::env::var("POSTGRES_HOST").ok()) {
        Some(host) => host,
        None => anyhow::bail!("No host provided from argument or POSTGRES_HOST env variable"),
    };
    let port = match port.or_else(|| {
        std::env::var("POSTGRES_PORT")
            .map(|port| port.parse::<u16>().unwrap())
            .ok()
    }) {
        Some(port) => port,
        None => anyhow::bail!("No port provided from argument or POSTGRES_PORT env variable"),
    };
    let username = match username.or_else(|| std::env::var("POSTGRES_USERNAME").ok()) {
        Some(username) => username,
        None => {
            anyhow::bail!("No username provided from argument or POSTGRES_USERNAME env variable")
        }
    };
    let database = match database.or_else(|| std::env::var("POSTGRES_DATABASE").ok()) {
        Some(database) => database,
        None => {
            anyhow::bail!("No database provided from argument or POSTGRES_DATABASE env variable")
        }
    };
    let password = match password.or_else(|| std::env::var("POSTGRES_PASSWORD").ok()) {
        Some(password) => password,
        None => {
            anyhow::bail!("No password provided from argument or POSTGRES_PASSWORD env variable")
        }
    };
    Ok(PostgresConnectOptions::new(
        host, port, username, password, database,
    ))
}

pub async fn delete_nautilus_postgres_tables(db: &PgPool) -> anyhow::Result<()> {
    // iterate over NAUTILUS_TABLES and delete all rows
    for table in NAUTILUS_TABLES {
        query(format!("DELETE FROM \"{}\" WHERE true", table).as_str())
            .execute(db)
            .await
            .unwrap_or_else(|_| panic!("Failed to delete table {}", table));
    }
    Ok(())
}

pub async fn connect_pg(options: PgConnectOptions) -> anyhow::Result<PgPool> {
    Ok(PgPool::connect_with(options).await.unwrap())
}
