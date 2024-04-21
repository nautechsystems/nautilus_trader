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

use log::{error, info};
use nautilus_infrastructure::sql::pg::{connect_pg, get_postgres_connect_options};
use sqlx::PgPool;

use crate::opt::{DatabaseCommand, DatabaseOpt};

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

pub async fn init_postgres(pg: &PgPool, database: String, password: String) -> anyhow::Result<()> {
    info!("Initializing Postgres database with target permissions and schema");
    // create public schema
    match sqlx::query("CREATE SCHEMA IF NOT EXISTS public;")
        .execute(pg)
        .await
    {
        Ok(_) => info!("Schema public created successfully"),
        Err(err) => error!("Error creating schema public: {:?}", err),
    }
    // create role if not exists
    match sqlx::query(format!("CREATE ROLE {} PASSWORD '{}' LOGIN;", database, password).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => info!("Role {} created successfully", database),
        Err(err) => {
            if err.to_string().contains("already exists") {
                info!("Role {} already exists", database);
            } else {
                error!("Error creating role {}: {:?}", database, err);
            }
        }
    }
    // execute all the sql files in schema dir
    let schema_dir = get_schema_dir()?;
    let mut sql_files =
        std::fs::read_dir(schema_dir)?.collect::<Result<Vec<_>, std::io::Error>>()?;
    for file in &mut sql_files {
        let file_name = file.file_name();
        info!("Executing schema file: {:?}", file_name);
        let file_path = file.path();
        let sql_content = std::fs::read_to_string(file_path.clone())?;
        for sql_statement in sql_content.split(';').filter(|s| !s.trim().is_empty()) {
            sqlx::query(sql_statement).execute(pg).await?;
        }
    }
    // grant connect
    match sqlx::query(format!("GRANT CONNECT ON DATABASE {0} TO {0};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => info!("Connect privileges granted to role {}", database),
        Err(err) => error!(
            "Error granting connect privileges to role {}: {:?}",
            database, err
        ),
    }
    // grant all schema privileges to the role
    match sqlx::query(format!("GRANT ALL PRIVILEGES ON SCHEMA public TO {};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => info!("All schema privileges granted to role {}", database),
        Err(err) => error!(
            "Error granting all privileges to role {}: {:?}",
            database, err
        ),
    }
    // grant all table privileges to the role
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
        Ok(_) => info!("All tables privileges granted to role {}", database),
        Err(err) => error!(
            "Error granting all privileges to role {}: {:?}",
            database, err
        ),
    }
    // grant all sequence privileges to the role
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
        Ok(_) => info!("All sequences privileges granted to role {}", database),
        Err(err) => error!(
            "Error granting all privileges to role {}: {:?}",
            database, err
        ),
    }
    // grant all function privileges to the role
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
        Ok(_) => info!("All functions privileges granted to role {}", database),
        Err(err) => error!(
            "Error granting all privileges to role {}: {:?}",
            database, err
        ),
    }

    Ok(())
}

pub async fn drop_postgres(pg: &PgPool, database: String) -> anyhow::Result<()> {
    // execute drop owned
    match sqlx::query(format!("DROP OWNED BY {}", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => info!("Dropped owned objects by role {}", database),
        Err(err) => error!("Error dropping owned by role {}: {:?}", database, err),
    }
    // revoke connect
    match sqlx::query(format!("REVOKE CONNECT ON DATABASE {0} FROM {0};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => info!("Revoked connect privileges from role {}", database),
        Err(err) => error!(
            "Error revoking connect privileges from role {}: {:?}",
            database, err
        ),
    }
    // revoke privileges
    match sqlx::query(format!("REVOKE ALL PRIVILEGES ON DATABASE {0} FROM {0};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => info!("Revoked all privileges from role {}", database),
        Err(err) => error!(
            "Error revoking all privileges from role {}: {:?}",
            database, err
        ),
    }
    // execute drop schema
    match sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(pg)
        .await
    {
        Ok(_) => info!("Dropped schema public"),
        Err(err) => error!("Error dropping schema public: {:?}", err),
    }
    // drop role
    match sqlx::query(format!("DROP ROLE IF EXISTS {};", database).as_str())
        .execute(pg)
        .await
    {
        Ok(_) => info!("Dropped role {}", database),
        Err(err) => error!("Error dropping role {}: {:?}", database, err),
    }
    Ok(())
}

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
            )
            .unwrap();
            let pg = connect_pg(pg_connect_options.clone().into()).await?;
            init_postgres(
                &pg,
                pg_connect_options.database,
                pg_connect_options.password,
            )
            .await?
        }
        DatabaseCommand::Drop(config) => {
            let pg_connect_options = get_postgres_connect_options(
                config.host,
                config.port,
                config.username,
                config.password,
                config.database,
            )
            .unwrap();
            let pg = connect_pg(pg_connect_options.clone().into()).await?;
            drop_postgres(&pg, pg_connect_options.database).await?
        }
    }
    Ok(())
}
