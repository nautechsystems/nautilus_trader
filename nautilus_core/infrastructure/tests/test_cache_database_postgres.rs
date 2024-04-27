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

use nautilus_infrastructure::sql::{
    cache_database::PostgresCacheDatabase,
    pg::{connect_pg, delete_nautilus_postgres_tables, PostgresConnectOptions},
};
use sqlx::PgPool;

#[must_use]
pub fn get_test_pg_connect_options(username: &str) -> PostgresConnectOptions {
    PostgresConnectOptions::new(
        "localhost".to_string(),
        5432,
        username.to_string(),
        "pass".to_string(),
        "nautilus".to_string(),
    )
}
pub async fn get_pg(username: &str) -> PgPool {
    let pg_connect_options = get_test_pg_connect_options(username);
    connect_pg(pg_connect_options.into()).await.unwrap()
}

pub async fn initialize() -> anyhow::Result<()> {
    // get pg pool with root postgres user to drop & create schema
    let pg_pool = get_pg("postgres").await;
    delete_nautilus_postgres_tables(&pg_pool).await.unwrap();
    Ok(())
}

pub async fn get_pg_cache_database() -> anyhow::Result<PostgresCacheDatabase> {
    initialize().await.unwrap();
    // run tests as nautilus user
    let connect_options = get_test_pg_connect_options("nautilus");
    Ok(PostgresCacheDatabase::connect(
        Some(connect_options.host),
        Some(connect_options.port),
        Some(connect_options.username),
        Some(connect_options.password),
        Some(connect_options.database),
    )
    .await
    .unwrap())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::get_pg_cache_database;

    #[tokio::test]
    async fn test_load_general_objects_when_nothing_in_cache_returns_empty_hashmap() {
        let pg_cache = get_pg_cache_database().await.unwrap();
        let result = pg_cache.load().await.unwrap();
        println!("1: {result:?}");
        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_add_general_object_adds_to_cache() {
        let pg_cache = get_pg_cache_database().await.unwrap();
        let test_id_value = String::from("test_value").into_bytes();
        pg_cache
            .add(String::from("test_id"), test_id_value.clone())
            .await
            .unwrap();
        // sleep with tokio
        tokio::time::sleep(Duration::from_secs(1)).await;
        let result = pg_cache.load().await.unwrap();
        println!("2: {result:?}");
        assert_eq!(result.keys().len(), 1);
        assert_eq!(
            result.keys().cloned().collect::<Vec<String>>(),
            vec![String::from("test_id")]
        ); // assert_eq!(result.get(&test_id_key).unwrap().to_owned(),&test_id_value.clone());
        assert_eq!(result.get("test_id").unwrap().to_owned(), test_id_value);
    }
}
