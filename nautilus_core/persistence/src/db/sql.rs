// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
// ------------------------------------------------------------------------------------------------

use nautilus_model::identifiers::trader_id::TraderId;
use sqlx::Error;

use crate::db::{database::Database, schema::GeneralItem};

pub struct SqlCacheDatabase {
    trader_id: TraderId,
    db: Database,
}

impl SqlCacheDatabase {
    pub fn new(trader_id: TraderId, database: Database) -> Self {
        Self {
            trader_id,
            db: database,
        }
    }
    pub fn key_trader(&self) -> String {
        format!("trader-{}", self.trader_id)
    }

    pub fn key_general(&self) -> String {
        format!("{}:general:", self.key_trader())
    }

    pub async fn add(&self, key: String, value: String) -> Result<u64, Error> {
        let query = format!(
            "INSERT INTO general (key, value) VALUES ('{}', '{}') ON CONFLICT (key) DO NOTHING;",
            key, value
        );
        self.db.execute(query.as_str()).await
    }

    pub async fn get(&self, key: String) -> Vec<GeneralItem> {
        let query = format!("SELECT * FROM general WHERE key = '{}'", key);
        self.db
            .fetch_all::<GeneralItem>(query.as_str())
            .await
            .unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::stubs::trader_id;

    use crate::db::{
        database::{init_db_schema, setup_test_database},
        sql::SqlCacheDatabase,
    };

    async fn setup_sql_cache_database() -> SqlCacheDatabase {
        let db = setup_test_database().await;
        let schema_dir = "../../schema/sql";
        init_db_schema(&db,schema_dir).await.expect("Failed to init db schema");
        let trader = trader_id();
        SqlCacheDatabase::new(trader, db)
    }

    #[tokio::test]
    async fn test_keys() {
        let cache = setup_sql_cache_database().await;
        assert_eq!(cache.key_trader(), "trader-TRADER-001");
        assert_eq!(cache.key_general(), "trader-TRADER-001:general:");
    }

    #[tokio::test]
    async fn test_add_get_general() {
        let cache = setup_sql_cache_database().await;
        cache
            .add(String::from("key1"), String::from("value1"))
            .await
            .expect("Failed to add key");
        let value = cache.get(String::from("key1")).await;
        assert_eq!(value.len(), 1);
        let item = value.get(0).unwrap();
        assert_eq!(item.key, "key1");
        assert_eq!(item.value, "value1");
    }
}
