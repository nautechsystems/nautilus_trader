use nautilus_infrastructure::sql::pg::{
    connect_pg, drop_postgres, get_postgres_connect_options, init_postgres,
};

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
    }
    Ok(())
}
