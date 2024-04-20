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

use std::{path::Path, str::FromStr};

use sqlx::{
    any::{install_default_drivers, AnyConnectOptions},
    sqlite::SqliteConnectOptions,
    Error, Pool, SqlitePool,
};

#[derive(Clone)]
pub struct Database {
    pub pool: Pool<sqlx::Any>,
}

pub enum DatabaseEngine {
    POSTGRES,
    SQLITE,
}

fn str_to_database_engine(engine_str: &str) -> DatabaseEngine {
    match engine_str {
        "POSTGRES" | "postgres" => DatabaseEngine::POSTGRES,
        "SQLITE" | "sqlite" => DatabaseEngine::SQLITE,
        _ => panic!("Invalid database engine: {engine_str}"),
    }
}

impl Database {
    pub async fn new(engine: Option<DatabaseEngine>, conn_string: Option<&str>) -> Self {
        install_default_drivers();
        let db_options = Self::get_db_options(engine, conn_string);
        let db = sqlx::pool::PoolOptions::new()
            .max_connections(20)
            .connect_with(db_options)
            .await;
        match db {
            Ok(pool) => Self { pool },
            Err(err) => {
                panic!("Failed to connect to database: {err}")
            }
        }
    }

    #[must_use]
    pub fn get_db_options(
        engine: Option<DatabaseEngine>,
        conn_string: Option<&str>,
    ) -> AnyConnectOptions {
        let connection_string = match conn_string {
            Some(conn_string) => Ok(conn_string.to_string()),
            None => std::env::var("DATABASE_URL"),
        };
        let database_engine: DatabaseEngine = match engine {
            Some(engine) => engine,
            None => str_to_database_engine(
                std::env::var("DATABASE_ENGINE")
                    .unwrap_or("SQLITE".to_string())
                    .as_str(),
            ),
        };
        match connection_string {
            Ok(connection_string) => match database_engine {
                DatabaseEngine::POSTGRES => AnyConnectOptions::from_str(connection_string.as_str())
                    .expect("Invalid PostgresSQL connection string"),
                DatabaseEngine::SQLITE => AnyConnectOptions::from_str(connection_string.as_str())
                    .expect("Invalid SQLITE connection string"),
            },
            Err(err) => {
                panic!("Failed to connect to database: {err}")
            }
        }
    }

    pub async fn execute(&self, query_str: &str) -> Result<u64, Error> {
        let result = sqlx::query(query_str).execute(&self.pool).await?;

        Ok(result.rows_affected())
    }

    pub async fn fetch_all<T>(&self, query_str: &str) -> Result<Vec<T>, Error>
    where
        T: for<'r> sqlx::FromRow<'r, sqlx::any::AnyRow> + Unpin,
    {
        let rows = sqlx::query(query_str).fetch_all(&self.pool).await?;

        let mut objects = Vec::new();
        for row in rows {
            let obj = T::from_row(&row)?;
            objects.push(obj);
        }

        Ok(objects)
    }
}

pub async fn init_db_schema(db: &Database, schema_dir: &str) -> anyhow::Result<()> {
    // scan all the files in the current directory
    let mut sql_files =
        std::fs::read_dir(schema_dir)?.collect::<Result<Vec<_>, std::io::Error>>()?;

    for file in &mut sql_files {
        let file_name = file.file_name();
        println!("Executing SQL file: {file_name:?}");
        let file_path = file.path();
        let sql_content = std::fs::read_to_string(file_path.clone())?;
        for sql_statement in sql_content.split(';').filter(|s| !s.trim().is_empty()) {
            db.execute(sql_statement).await.unwrap_or_else(|e| {
                panic!(
                    "Failed to execute SQL statement: {} with reason {}",
                    file_path.display(),
                    e
                )
            });
        }
    }
    Ok(())
}

pub async fn setup_test_database() -> Database {
    // check if test_db.sqlite exists,if not, create it
    let db_path = std::env::var("TEST_DB_PATH").unwrap_or("test_db.sqlite".to_string());
    let db_file_path = Path::new(db_path.as_str());
    let exists = db_file_path.exists();
    if !exists {
        SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(db_file_path)
                .create_if_missing(true),
        )
        .await
        .expect("Failed to create test_db.sqlite");
    }
    Database::new(Some(DatabaseEngine::SQLITE), Some("sqlite:test_db.sqlite")).await
}

#[cfg(test)]
mod tests {

    use sqlx::{FromRow, Row};

    use crate::db::database::{setup_test_database, Database};

    async fn init_item_table(database: &Database) {
        database
            .execute("CREATE TABLE IF NOT EXISTS items (key TEXT PRIMARY KEY, value TEXT)")
            .await
            .expect("Failed to create table item");
    }

    async fn drop_table(database: &Database) {
        database
            .execute("DROP TABLE items")
            .await
            .expect("Failed to drop table items");
    }

    #[tokio::test]
    async fn test_database() {
        let db = setup_test_database().await;
        let rows_affected = db.execute("SELECT 1").await.unwrap();
        // it will not fail and give 0 rows affected
        assert_eq!(rows_affected, 0);
    }

    #[tokio::test]
    async fn test_database_fetch_all() {
        let db = setup_test_database().await;
        struct SimpleValue {
            value: i32,
        }
        impl FromRow<'_, sqlx::any::AnyRow> for SimpleValue {
            fn from_row(row: &sqlx::any::AnyRow) -> Result<Self, sqlx::Error> {
                Ok(Self {
                    value: row.try_get(0)?,
                })
            }
        }
        let result = db.fetch_all::<SimpleValue>("SELECT 3").await.unwrap();
        assert_eq!(result[0].value, 3);
    }

    #[tokio::test]
    async fn test_insert_and_select() {
        let db = setup_test_database().await;
        init_item_table(&db).await;
        // insert some value
        db.execute("INSERT INTO items (key, value) VALUES ('key1', 'value1')")
            .await
            .unwrap();
        // fetch item, impl Data struct
        struct Item {
            key: String,
            value: String,
        }
        impl FromRow<'_, sqlx::any::AnyRow> for Item {
            fn from_row(row: &sqlx::any::AnyRow) -> Result<Self, sqlx::Error> {
                Ok(Self {
                    key: row.try_get(0)?,
                    value: row.try_get(1)?,
                })
            }
        }
        let result = db.fetch_all::<Item>("SELECT * FROM items").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "key1");
        assert_eq!(result[0].value, "value1");
        drop_table(&db).await;
    }
}
