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

use derive_builder::Builder;
use regex::Regex;
use sqlx::{ConnectOptions, PgPool, postgres::PgConnectOptions};

#[derive(Debug, Clone, Builder)]
#[builder(default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.infrastructure")
)]
pub struct PostgresConnectOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
}

impl PostgresConnectOptions {
    /// Creates a new [`PostgresConnectOptions`] instance.
    #[must_use]
    pub const fn new(
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

    #[must_use]
    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{username}:{password}@{host}:{port}/{database}",
            username = self.username,
            password = self.password,
            host = self.host,
            port = self.port,
            database = self.database
        )
    }

    #[must_use]
    pub fn default_administrator() -> Self {
        Self::new(
            String::from("localhost"),
            5432,
            String::from("nautilus"),
            String::from("pass"),
            String::from("nautilus"),
        )
    }
}

impl Default for PostgresConnectOptions {
    fn default() -> Self {
        Self::new(
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
        Self::new()
            .host(opt.host.as_str())
            .port(opt.port)
            .username(opt.username.as_str())
            .password(opt.password.as_str())
            .database(opt.database.as_str())
            .disable_statement_logging()
    }
}

/// Constructs `PostgresConnectOptions` by merging provided arguments, environment variables, and defaults.
///
/// # Panics
///
/// Panics if an environment variable for port cannot be parsed into a `u16`.
#[must_use]
pub fn get_postgres_connect_options(
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    database: Option<String>,
) -> PostgresConnectOptions {
    let defaults = PostgresConnectOptions::default_administrator();
    let host = host
        .or_else(|| std::env::var("POSTGRES_HOST").ok())
        .unwrap_or(defaults.host);
    let port = port
        .or_else(|| {
            std::env::var("POSTGRES_PORT")
                .map(|port| port.parse::<u16>().unwrap())
                .ok()
        })
        .unwrap_or(defaults.port);
    let username = username
        .or_else(|| std::env::var("POSTGRES_USERNAME").ok())
        .unwrap_or(defaults.username);
    let database = database
        .or_else(|| std::env::var("POSTGRES_DATABASE").ok())
        .unwrap_or(defaults.database);
    let password = password
        .or_else(|| std::env::var("POSTGRES_PASSWORD").ok())
        .unwrap_or(defaults.password);
    PostgresConnectOptions::new(host, port, username, password, database)
}

/// Connects to a Postgres database with the provided connection `options` returning a connection pool.
///
/// # Errors
///
/// Returns an error if establishing the database connection fails.
pub async fn connect_pg(options: PgConnectOptions) -> anyhow::Result<PgPool> {
    Ok(PgPool::connect_with(options).await?)
}

/// Scans the current working directory for the `nautilus_trader` repository
/// and constructs the path to the SQL schema directory.
///
/// # Errors
///
/// Returns an error if the `SCHEMA_DIR` environment variable is not set and the repository
/// cannot be located in the current directory path.
///
/// # Panics
///
/// Panics if the current working directory cannot be determined or contains invalid UTF-8.
fn get_schema_dir() -> anyhow::Result<String> {
    std::env::var("SCHEMA_DIR").or_else(|_| {
        let nautilus_git_repo_name = "nautilus_trader";
        let binding = std::env::current_dir().unwrap();
        let current_dir = binding.to_str().unwrap();
        match current_dir.find(nautilus_git_repo_name){
            Some(index) => {
                let schema_path = current_dir[0..index + nautilus_git_repo_name.len()].to_string() + "/schema/sql";
                Ok(schema_path)
            }
            None => anyhow::bail!("Could not calculate schema dir from current directory path or SCHEMA_DIR env variable")
        }
    })
}

/// Initializes the Postgres database by creating schema, roles, and executing SQL files from `schema_dir`.
///
/// # Errors
///
/// Returns an error if any SQL execution or file system operation fails.
///
/// # Panics
///
/// Panics if `schema_dir` is missing and cannot be determined or if other unwraps fail.
pub async fn init_postgres(
    pg: &PgPool,
    database: String,
    password: String,
    schema_dir: Option<String>,
) -> anyhow::Result<()> {
    log::info!("Initializing Postgres database with target permissions and schema");

    // Create public schema
    match sqlx::query("CREATE SCHEMA IF NOT EXISTS public;")
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("Schema public created successfully"),
        Err(e) => log::error!("Error creating schema public: {e:?}"),
    }

    // Create role if not exists
    match sqlx::query(format!("CREATE ROLE {database} PASSWORD '{password}' LOGIN;").as_str())
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("Role {database} created successfully"),
        Err(e) => {
            if e.to_string().contains("already exists") {
                log::info!("Role {database} already exists");
            } else {
                log::error!("Error creating role {database}: {e:?}");
            }
        }
    }

    // Execute all the sql files in schema dir
    let schema_dir = schema_dir.unwrap_or_else(|| get_schema_dir().unwrap());
    let sql_files = vec!["types.sql", "functions.sql", "partitions.sql", "tables.sql"];
    let plpgsql_regex =
        Regex::new(r"\$\$ LANGUAGE plpgsql(?:[ \t\r\n]+SECURITY[ \t\r\n]+DEFINER)?;")?;
    for file_name in &sql_files {
        log::info!("Executing schema file: {file_name:?}");
        let file_path = format!("{}/{}", schema_dir, file_name);
        let sql_content = std::fs::read_to_string(&file_path)?;
        let sql_statements: Vec<String> = match *file_name {
            "functions.sql" | "partitions.sql" => {
                let mut statements = Vec::new();
                let mut last_end = 0;

                for mat in plpgsql_regex.find_iter(&sql_content) {
                    let statement = sql_content[last_end..mat.end()].to_string();
                    if !statement.trim().is_empty() {
                        statements.push(statement);
                    }
                    last_end = mat.end();
                }
                statements
            }
            _ => sql_content
                .split(';')
                .filter(|s| !s.trim().is_empty())
                .map(|s| format!("{s};"))
                .collect(),
        };

        for sql_statement in sql_statements {
            sqlx::query(&sql_statement)
                .execute(pg)
                .await
                .map_err(|e| {
                    if e.to_string().contains("already exists") {
                        log::info!("Already exists error on statement, skipping");
                    } else {
                        panic!("Error executing statement {sql_statement} with error: {e:?}")
                    }
                })
                .unwrap();
        }
    }

    // Grant connect
    match sqlx::query(format!("GRANT CONNECT ON DATABASE {database} TO {database};").as_str())
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("Connect privileges granted to role {database}"),
        Err(e) => log::error!("Error granting connect privileges to role {database}: {e:?}"),
    }

    // Grant all schema privileges to the role
    match sqlx::query(format!("GRANT ALL PRIVILEGES ON SCHEMA public TO {database};").as_str())
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("All schema privileges granted to role {database}"),
        Err(e) => log::error!("Error granting all privileges to role {database}: {e:?}"),
    }

    // Grant all table privileges to the role
    match sqlx::query(
        format!("GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO {database};").as_str(),
    )
    .execute(pg)
    .await
    {
        Ok(_) => log::info!("All tables privileges granted to role {database}"),
        Err(e) => log::error!("Error granting all privileges to role {database}: {e:?}"),
    }

    // Grant all sequence privileges to the role
    match sqlx::query(
        format!("GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO {database};").as_str(),
    )
    .execute(pg)
    .await
    {
        Ok(_) => log::info!("All sequences privileges granted to role {database}"),
        Err(e) => log::error!("Error granting all privileges to role {database}: {e:?}"),
    }

    // Grant all function privileges to the role
    match sqlx::query(
        format!("GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA public TO {database};").as_str(),
    )
    .execute(pg)
    .await
    {
        Ok(_) => log::info!("All functions privileges granted to role {database}"),
        Err(e) => log::error!("Error granting all privileges to role {database}: {e:?}"),
    }

    Ok(())
}

/// Drops the Postgres database with the given name using the provided connection pool.
///
/// # Errors
///
/// Returns an error if the DROP DATABASE command fails.
pub async fn drop_postgres(pg: &PgPool, database: String) -> anyhow::Result<()> {
    // Execute drop owned
    match sqlx::query(format!("DROP OWNED BY {database}").as_str())
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("Dropped owned objects by role {database}"),
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("2BP01") || err_msg.contains("required by the database system") {
                log::warn!("Skipping system-required objects for role {database}");
            } else {
                log::error!("Error dropping owned by role {database}: {e:?}");
            }
        }
    }

    // Revoke connect
    match sqlx::query(format!("REVOKE CONNECT ON DATABASE {database} FROM {database};").as_str())
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("Revoked connect privileges from role {database}"),
        Err(e) => log::error!("Error revoking connect privileges from role {database}: {e:?}"),
    }

    // Revoke privileges
    match sqlx::query(
        format!("REVOKE ALL PRIVILEGES ON DATABASE {database} FROM {database};").as_str(),
    )
    .execute(pg)
    .await
    {
        Ok(_) => log::info!("Revoked all privileges from role {database}"),
        Err(e) => log::error!("Error revoking all privileges from role {database}: {e:?}"),
    }

    // Execute drop schema
    match sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("Dropped schema public"),
        Err(e) => log::error!("Error dropping schema public: {e:?}"),
    }

    // Drop role
    match sqlx::query(format!("DROP ROLE IF EXISTS {database};").as_str())
        .execute(pg)
        .await
    {
        Ok(_) => log::info!("Dropped role {database}"),
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("55006") || err_msg.contains("current user cannot be dropped") {
                log::warn!("Cannot drop currently connected role {database}");
            } else {
                log::error!("Error dropping role {database}: {e:?}");
            }
        }
    }
    Ok(())
}
