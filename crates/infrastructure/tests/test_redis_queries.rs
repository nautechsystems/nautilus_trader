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

#[cfg(test)]
#[cfg(feature = "redis")]
#[cfg(target_os = "linux")] // Databases only tested and supported on Linux
mod serial_tests {
    use std::time::Instant;

    use bytes::Bytes;
    use nautilus_common::{
        cache::CacheConfig, enums::SerializationEncoding, msgbus::database::DatabaseConfig,
        testing::wait_until_async,
    };
    use nautilus_core::UUID4;
    use nautilus_infrastructure::redis::{
        cache::RedisCacheDatabase, create_redis_connection, queries::DatabaseQueries,
    };
    use nautilus_model::{identifiers::TraderId, types::Currency};
    use redis::AsyncCommands;
    use ustr::Ustr;

    async fn get_redis_connection() -> redis::aio::ConnectionManager {
        let config = DatabaseConfig {
            database_type: "redis".to_string(),
            host: Some("localhost".to_string()),
            port: Some(6379),
            username: None,
            password: None,
            ssl: false,
            connection_timeout: 20,
            response_timeout: 20,
            number_of_retries: 100,
            exponent_base: 2,
            max_delay: 1000,
            factor: 2,
        };

        create_redis_connection("test", config)
            .await
            .expect("Failed to create Redis connection")
    }

    async fn setup_test_database() -> (RedisCacheDatabase, String) {
        let trader_id = TraderId::from("test-trader");
        let instance_id = UUID4::new();

        let config = CacheConfig {
            database: Some(DatabaseConfig {
                database_type: "redis".to_string(),
                host: Some("localhost".to_string()),
                port: Some(6379),
                username: None,
                password: None,
                ssl: false,
                connection_timeout: 20,
                response_timeout: 20,
                number_of_retries: 100,
                exponent_base: 2,
                max_delay: 1000,
                factor: 2,
            }),
            ..Default::default()
        };

        let mut database = RedisCacheDatabase::new(trader_id, instance_id, config)
            .await
            .expect("Failed to create database");

        // Clean the database at the start
        database.flushdb().await;

        let trader_key = database.trader_key.clone();
        (database, trader_key)
    }

    #[tokio::test]
    async fn test_scan_keys_empty_database() {
        let mut con = get_redis_connection().await;

        // Ensure clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        let pattern = "test:*".to_string();
        let result = DatabaseQueries::scan_keys(&mut con, pattern).await.unwrap();

        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_scan_keys_with_matching_keys() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Add test keys
        let test_keys = vec![
            "test:key1",
            "test:key2",
            "test:key3",
            "other:key1",
            "test:key4",
        ];

        for key in &test_keys {
            let _: () = con.set(*key, "value").await.unwrap();
        }

        let pattern = "test:*".to_string();
        let result = DatabaseQueries::scan_keys(&mut con, pattern).await.unwrap();

        assert_eq!(result.len(), 4);
        for key in result {
            assert!(key.starts_with("test:"));
        }
    }

    #[tokio::test]
    async fn test_scan_keys_handles_large_dataset() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Add many test keys to trigger multiple SCAN iterations
        let num_keys = 10000;
        for i in 0..num_keys {
            let key = format!("test:large:{i}");
            let _: () = con.set(key, "value").await.unwrap();
        }

        let pattern = "test:large:*".to_string();
        let result = DatabaseQueries::scan_keys(&mut con, pattern).await.unwrap();

        assert_eq!(result.len(), num_keys);
    }

    #[tokio::test]
    async fn test_read_bulk_empty_keys() {
        let con = get_redis_connection().await;
        let keys: Vec<String> = vec![];

        let result = DatabaseQueries::read_bulk(&con, &keys).await.unwrap();

        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_read_bulk_single_key() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Set up test data
        let key = "test:bulk:single".to_string();
        let value = b"test_value";
        let _: () = con.set(&key, value).await.unwrap();

        let keys = vec![key];
        let result = DatabaseQueries::read_bulk(&con, &keys).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], Some(Bytes::from(value.to_vec())));
    }

    #[tokio::test]
    async fn test_read_bulk_multiple_keys() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Set up test data
        let keys: Vec<String> = (0..5).map(|i| format!("test:bulk:multi:{i}")).collect();
        let values: Vec<Vec<u8>> = (0..5).map(|i| format!("value_{i}").into_bytes()).collect();

        for (key, value) in keys.iter().zip(values.iter()) {
            let _: () = con.set(key, value).await.unwrap();
        }

        let result = DatabaseQueries::read_bulk(&con, &keys).await.unwrap();

        assert_eq!(result.len(), 5);
        for (i, bytes_opt) in result.iter().enumerate() {
            assert_eq!(*bytes_opt, Some(Bytes::from(values[i].clone())));
        }
    }

    #[tokio::test]
    async fn test_read_bulk_with_missing_keys() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Set up partial data
        let keys: Vec<String> = (0..5).map(|i| format!("test:bulk:missing:{i}")).collect();

        // Only set some keys
        let _: () = con.set(&keys[0], b"value_0").await.unwrap();
        let _: () = con.set(&keys[2], b"value_2").await.unwrap();
        let _: () = con.set(&keys[4], b"value_4").await.unwrap();

        let result = DatabaseQueries::read_bulk(&con, &keys).await.unwrap();

        assert_eq!(result.len(), 5);
        assert!(result[0].is_some());
        assert!(result[1].is_none());
        assert!(result[2].is_some());
        assert!(result[3].is_none());
        assert!(result[4].is_some());
    }

    #[tokio::test]
    async fn test_read_bulk_performance_vs_individual() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Set up test data
        let num_keys = 100;
        let keys: Vec<String> = (0..num_keys).map(|i| format!("test:perf:{i}")).collect();

        for (i, key) in keys.iter().enumerate() {
            let value = format!("value_{i}");
            let _: () = con.set(key, value.as_bytes()).await.unwrap();
        }

        // Measure bulk read time
        let bulk_start = Instant::now();
        let bulk_result = DatabaseQueries::read_bulk(&con, &keys).await.unwrap();
        let bulk_duration = bulk_start.elapsed();

        // Measure individual reads time
        let individual_start = Instant::now();
        let mut individual_results = Vec::new();
        for key in &keys {
            let result: Option<Vec<u8>> = con.get(key).await.unwrap();
            individual_results.push(result.map(Bytes::from));
        }
        let individual_duration = individual_start.elapsed();

        // Verify results are the same
        assert_eq!(bulk_result.len(), individual_results.len());
        for (bulk, individual) in bulk_result.iter().zip(individual_results.iter()) {
            assert_eq!(bulk, individual);
        }

        // Bulk should be significantly faster
        println!("Bulk read time: {bulk_duration:?}");
        println!("Individual read time: {individual_duration:?}");

        // Bulk should be at least 2x faster for 100 keys
        assert!(
            bulk_duration < individual_duration / 2,
            "Bulk reading should be at least 2x faster than individual reads"
        );
    }

    #[tokio::test]
    async fn test_load_currencies_empty() {
        let (database, trader_key) = setup_test_database().await;

        let result = DatabaseQueries::load_currencies(
            &database.con,
            &trader_key,
            SerializationEncoding::MsgPack,
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_load_currencies_with_bulk_loading() {
        let (mut database, trader_key) = setup_test_database().await;

        // Create test currencies
        let currencies = vec![
            Currency::USD(),
            Currency::EUR(),
            Currency::GBP(),
            Currency::JPY(),
            Currency::AUD(),
        ];

        // Store currencies in Redis
        for currency in &currencies {
            let key = format!("currencies:{}", currency.code);
            let payload =
                DatabaseQueries::serialize_payload(SerializationEncoding::MsgPack, &currency)
                    .unwrap();
            database
                .insert(key, Some(vec![Bytes::from(payload)]))
                .unwrap();
        }

        // Wait for all currencies to be written
        wait_until_async(
            || async {
                // Check if we can read back all currencies
                let result = DatabaseQueries::load_currencies(
                    &database.con,
                    &trader_key,
                    SerializationEncoding::MsgPack,
                )
                .await;
                result.is_ok() && result.unwrap().len() == 5
            },
            std::time::Duration::from_secs(2),
        )
        .await;

        // Load currencies using bulk loading
        let result = DatabaseQueries::load_currencies(
            &database.con,
            &trader_key,
            SerializationEncoding::MsgPack,
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 5);
        assert!(result.contains_key(&Ustr::from("USD")));
        assert!(result.contains_key(&Ustr::from("EUR")));
        assert!(result.contains_key(&Ustr::from("GBP")));
        assert!(result.contains_key(&Ustr::from("JPY")));
        assert!(result.contains_key(&Ustr::from("AUD")));
    }

    #[tokio::test]
    async fn test_serialize_deserialize_payload() {
        let currency = Currency::USD();

        // Test JSON encoding
        let json_bytes =
            DatabaseQueries::serialize_payload(SerializationEncoding::Json, &currency).unwrap();

        let deserialized_json: Currency =
            DatabaseQueries::deserialize_payload(SerializationEncoding::Json, &json_bytes).unwrap();

        assert_eq!(currency, deserialized_json);

        // Test MsgPack encoding
        let msgpack_bytes =
            DatabaseQueries::serialize_payload(SerializationEncoding::MsgPack, &currency).unwrap();

        let deserialized_msgpack: Currency =
            DatabaseQueries::deserialize_payload(SerializationEncoding::MsgPack, &msgpack_bytes)
                .unwrap();

        assert_eq!(currency, deserialized_msgpack);
    }

    #[tokio::test]
    async fn test_read_bulk_handles_very_large_keys() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Create large values
        let keys: Vec<String> = (0..10).map(|i| format!("test:large:value:{i}")).collect();
        let large_value = vec![0u8; 1024 * 1024]; // 1MB value

        for key in &keys {
            let _: () = con.set(key, &large_value).await.unwrap();
        }

        let result = DatabaseQueries::read_bulk(&con, &keys).await.unwrap();

        assert_eq!(result.len(), 10);
        for bytes_opt in result {
            assert!(bytes_opt.is_some());
            assert_eq!(bytes_opt.unwrap().len(), 1024 * 1024);
        }
    }

    #[tokio::test]
    async fn test_scan_keys_with_special_characters() {
        let mut con = get_redis_connection().await;

        // Clean state
        let _: () = redis::cmd("FLUSHDB").query_async(&mut con).await.unwrap();

        // Add keys with special characters
        let test_keys = vec![
            "test:key-with-dash",
            "test:key_with_underscore",
            "test:key.with.dots",
            "test:key/with/slashes",
        ];

        for key in &test_keys {
            let _: () = con.set(*key, "value").await.unwrap();
        }

        let pattern = "test:*".to_string();
        let result = DatabaseQueries::scan_keys(&mut con, pattern).await.unwrap();

        assert_eq!(result.len(), 4);
    }

    #[tokio::test]
    async fn test_load_currency_single() {
        let (mut database, trader_key) = setup_test_database().await;

        let currency = Currency::USD();
        let key = format!("currencies:{}", currency.code);
        let payload =
            DatabaseQueries::serialize_payload(SerializationEncoding::MsgPack, &currency).unwrap();

        database
            .insert(key, Some(vec![Bytes::from(payload)]))
            .unwrap();

        // Wait for data to be written
        wait_until_async(
            || async {
                DatabaseQueries::load_currency(
                    &database.con,
                    &trader_key,
                    &Ustr::from("USD"),
                    SerializationEncoding::MsgPack,
                )
                .await
                .unwrap_or(None)
                .is_some()
            },
            std::time::Duration::from_secs(2),
        )
        .await;

        let result = DatabaseQueries::load_currency(
            &database.con,
            &trader_key,
            &Ustr::from("USD"),
            SerializationEncoding::MsgPack,
        )
        .await
        .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap(), currency);
    }

    #[tokio::test]
    async fn test_load_currency_not_found() {
        let (database, trader_key) = setup_test_database().await;

        let result = DatabaseQueries::load_currency(
            &database.con,
            &trader_key,
            &Ustr::from("XYZ"),
            SerializationEncoding::MsgPack,
        )
        .await
        .unwrap();

        assert!(result.is_none());
    }
}
