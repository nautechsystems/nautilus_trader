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

use derive_builder::Builder;
use sqlx::{postgres::PgConnectOptions, query, ConnectOptions, PgPool};

use crate::sql::NAUTILUS_TABLES;

#[derive(Debug, Clone, Builder)]
#[builder(default)]
pub struct PostgresConnectOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
}

impl PostgresConnectOptions {
    /// Creates a new [`PostgresConnectOptions`] instance.
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

impl Default for PostgresConnectOptions {
    fn default() -> Self {
        PostgresConnectOptions::new(
            String::from("localhost"),
            5432,
            String::from("nautilus"),
            String::from("pass"),
            String::from("nautilus"),
        )
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
    // Iterate over NAUTILUS_TABLES and delete all rows
    for table in NAUTILUS_TABLES {
        query(format!("DELETE FROM \"{}\" WHERE true", table).as_str())
            .execute(db)
            .await
            .unwrap_or_else(|err| panic!("Failed to delete table {} because: {}", table, err));
    }
    Ok(())
}

pub async fn connect_pg(options: PgConnectOptions) -> anyhow::Result<PgPool> {
    Ok(PgPool::connect_with(options).await.unwrap())
}

/// Scans current path with keyword nautilus_trader and build schema dir
fn get_schema_dir() -> anyhow::Result<String> {
    std::env::var("SCHEMA_DIR").or_else(|_| {
        let nautilus_git_repo_name = "nautilus_trader";
        let binding = std::env::current_dir().unwrap();
        let current_dir = binding.to_str().unwrap();
        match current_dir.find(nautilus_git_repo_name){
            Some(index) => {
                let schema_path = current_dir[0..index + nautilus_git_repo_name.len()].to_string() + "/schema";
                Ok(schema_path)
            }
            None => anyhow::bail!("Could not calculate schema dir from current directory path or SCHEMA_DIR env variable")
        }
    })
}

pub async fn init_postgres(
    pg: &PgPool,
    database: String,
    password: String,
    schema_dir: Option<String>,
) -> anyhow::Result<()> {
    tracing::info!("Initializing Postgres database with target permissions and schema");

    // Create public schema
    match sqlx::query("CREATE SCHEMA IF NOT EXISTS public;")
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Schema public created successfully"),
        Err(e) => tracing::error!("Error creating schema public: {:?}", e),
    }

    // Create role if not exists
    match sqlx::query(format!("CREATE ROLE {} PASSWORD '{}' LOGIN;", database, password).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Role {} created successfully", database),
        Err(e) => {
            if e.to_string().contains("already exists") {
                tracing::info!("Role {} already exists", database);
            } else {
                tracing::error!("Error creating role {}: {:?}", database, e);
            }
        }
    }

    // Execute all the sql files in schema dir
    let schema_dir = schema_dir.unwrap_or_else(|| get_schema_dir().unwrap());
    let mut sql_files =
        std::fs::read_dir(schema_dir)?.collect::<Result<Vec<_>, std::io::Error>>()?;
    for file in &mut sql_files {
        let file_name = file.file_name();
        tracing::info!("Executing schema file: {:?}", file_name);
        let file_path = file.path();
        let sql_content = std::fs::read_to_string(file_path.clone())?;
        for sql_statement in sql_content.split(';').filter(|s| !s.trim().is_empty()) {
            sqlx::query(sql_statement)
                .execute(pg)
                .await
                .map_err(|err| {
                    if err.to_string().contains("already exists") {
                        tracing::info!("Already exists error on statement, skipping");
                    } else {
                        panic!(
                            "Error executing statement {} with error: {:?}",
                            sql_statement, err
                        )
                    }
                })
                .unwrap();
        }
    }

    // Grant connect
    match sqlx::query(format!("GRANT CONNECT ON DATABASE {0} TO {0};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Connect privileges granted to role {}", database),
        Err(e) => tracing::error!(
            "Error granting connect privileges to role {}: {:?}",
            database,
            e
        ),
    }

    // Grant all schema privileges to the role
    match sqlx::query(format!("GRANT ALL PRIVILEGES ON SCHEMA public TO {};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("All schema privileges granted to role {}", database),
        Err(e) => tracing::error!(
            "Error granting all privileges to role {}: {:?}",
            database,
            e
        ),
    }

    // Grant all table privileges to the role
    match sqlx::query(
        format!(
            "GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO {};",
            database
        )
        .as_str(),
    )
    .execute(pg)
    .await
    {
        Ok(_) => tracing::info!("All tables privileges granted to role {}", database),
        Err(e) => tracing::error!(
            "Error granting all privileges to role {}: {:?}",
            database,
            e
        ),
    }

    // Grant all sequence privileges to the role
    match sqlx::query(
        format!(
            "GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO {};",
            database
        )
        .as_str(),
    )
    .execute(pg)
    .await
    {
        Ok(_) => tracing::info!("All sequences privileges granted to role {}", database),
        Err(e) => tracing::error!(
            "Error granting all privileges to role {}: {:?}",
            database,
            e
        ),
    }

    // Grant all function privileges to the role
    match sqlx::query(
        format!(
            "GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA public TO {};",
            database
        )
        .as_str(),
    )
    .execute(pg)
    .await
    {
        Ok(_) => tracing::info!("All functions privileges granted to role {}", database),
        Err(e) => tracing::error!(
            "Error granting all privileges to role {}: {:?}",
            database,
            e
        ),
    }

    Ok(())
}

pub async fn drop_postgres(pg: &PgPool, database: String) -> anyhow::Result<()> {
    // Execute drop owned
    match sqlx::query(format!("DROP OWNED BY {}", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Dropped owned objects by role {}", database),
        Err(e) => tracing::error!("Error dropping owned by role {}: {:?}", database, e),
    }

    // Revoke connect
    match sqlx::query(format!("REVOKE CONNECT ON DATABASE {0} FROM {0};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Revoked connect privileges from role {}", database),
        Err(e) => tracing::error!(
            "Error revoking connect privileges from role {}: {:?}",
            database,
            e
        ),
    }

    // Revoke privileges
    match sqlx::query(format!("REVOKE ALL PRIVILEGES ON DATABASE {0} FROM {0};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Revoked all privileges from role {}", database),
        Err(e) => tracing::error!(
            "Error revoking all privileges from role {}: {:?}",
            database,
            e
        ),
    }

    // Execute drop schema
    match sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Dropped schema public"),
        Err(e) => tracing::error!("Error dropping schema public: {:?}", e),
    }

    // Drop role
    match sqlx::query(format!("DROP ROLE IF EXISTS {};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => tracing::info!("Dropped role {}", database),
        Err(e) => tracing::error!("Error dropping role {}: {:?}", database, e),
    }
    Ok(())
}
