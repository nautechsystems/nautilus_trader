// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
    use std::{sync::OnceLock, time::Duration};

    use ahash::AHashMap;
    use bytes::Bytes;
    use nautilus_common::{
        cache::{Cache, CacheConfig, database::CacheDatabaseAdapter},
        msgbus::database::DatabaseConfig,
        testing::wait_until_async,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_infrastructure::redis::{
        cache::{RedisCacheDatabase, RedisCacheDatabaseAdapter},
        queries::DatabaseQueries,
    };
    use nautilus_model::{
        accounts::AccountAny,
        data::{
            DataType,
            stubs::{ensure_stub_custom_data_registered, stub_custom_data},
        },
        enums::{OrderSide, OrderStatus, OrderType},
        events::{
            AccountState, OrderEventAny, OrderFilled, OrderSnapshot,
            account::stubs::{cash_account_state_multi, cash_account_state_multi_changed_btc},
            position::snapshot::PositionSnapshot,
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, ComponentId, PositionId, StrategyId, TradeId,
            TraderId, VenueOrderId,
        },
        instruments::{
            Instrument, InstrumentAny, SyntheticInstrument, stubs::crypto_perpetual_ethusdt,
        },
        orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        position::Position,
        types::{Currency, Money, Quantity},
    };
    use redis::AsyncCommands;

    fn redis_test_mutex() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    async fn get_redis_cache_adapter()
    -> Result<RedisCacheDatabaseAdapter, Box<dyn std::error::Error>> {
        let mut adapter = connect_redis_cache_adapter().await?;

        // Clean the database at the start of each test
        adapter.database.flushdb().await;

        Ok(adapter)
    }

    // Connects an adapter without flushing; reconnecting under the same trader
    // key sees data written by a previous adapter (restart simulation).
    async fn connect_redis_cache_adapter()
    -> Result<RedisCacheDatabaseAdapter, Box<dyn std::error::Error>> {
        let trader_id = TraderId::from("test-trader");
        let instance_id = UUID4::new();

        // Create a Redis database config
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

        let database = RedisCacheDatabase::new(trader_id, instance_id, config).await?;

        let adapter = RedisCacheDatabaseAdapter { database };

        Ok(adapter)
    }

    #[tokio::test]
    async fn test_delete_order() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        let client_order_id = order.client_order_id();
        let expected_key = format!("{}:orders:{}", adapter.database.trader_key, client_order_id);

        // Set up test data in Redis to verify deletion
        let mut conn = adapter.database.con.clone();
        let _: () = conn.set(&expected_key, "test_data").await.unwrap();

        // Wait for Redis set operation to complete
        let conn_clone = conn.clone();
        let expected_key_clone = expected_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let expected_key = expected_key_clone.clone();
                async move {
                    let exists: bool = conn.exists(&expected_key).await.unwrap();
                    exists
                }
            },
            Duration::from_secs(2),
        )
        .await;

        let exists_before: bool = conn.exists(&expected_key).await.unwrap();
        assert!(exists_before);

        adapter.delete_order(&client_order_id).unwrap();

        // Wait until the order is deleted
        let conn_clone = conn.clone();
        let expected_key_clone = expected_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let expected_key = expected_key_clone.clone();
                async move {
                    let exists: bool = conn.exists(&expected_key).await.unwrap();
                    !exists
                }
            },
            Duration::from_secs(2),
        )
        .await;

        let exists_after: bool = conn.exists(&expected_key).await.unwrap();
        assert!(!exists_after);

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_position() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let position_id = PositionId::new("P-123456");
        let expected_key = format!("{}:positions:{}", adapter.database.trader_key, position_id);

        // Set up test data in Redis to verify deletion
        let mut conn = adapter.database.con.clone();
        let _: () = conn.set(&expected_key, "test_data").await.unwrap();

        // Wait for Redis set operation to complete
        let conn_clone = conn.clone();
        let expected_key_clone = expected_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let expected_key = expected_key_clone.clone();
                async move {
                    let exists: bool = conn.exists(&expected_key).await.unwrap();
                    exists
                }
            },
            Duration::from_secs(2),
        )
        .await;

        let exists_before: bool = conn.exists(&expected_key).await.unwrap();
        assert!(exists_before);

        adapter.delete_position(&position_id).unwrap();

        // Wait until the position is deleted
        let conn_clone = conn.clone();
        let expected_key_clone = expected_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let expected_key = expected_key_clone.clone();
                async move {
                    let exists: bool = conn.exists(&expected_key).await.unwrap();
                    !exists
                }
            },
            Duration::from_secs(2),
        )
        .await;

        let exists_after: bool = conn.exists(&expected_key).await.unwrap();
        assert!(!exists_after);

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_flush_database() {
        let _guard = redis_test_mutex().lock().await;
        let mut adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_operations_are_idempotent() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let order_id = ClientOrderId::new("O-IDEMPOTENT-TEST");
        let position_id = PositionId::new("P-IDEMPOTENT-TEST");

        for _ in 0..3 {
            adapter.delete_order(&order_id).unwrap();
            adapter.delete_position(&position_id).unwrap();
        }

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[expect(
        clippy::too_many_lines,
        reason = "integration test verifies order deletion across every Redis index"
    )]
    #[tokio::test]
    async fn test_delete_order_cleans_up_indexes() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        let client_order_id = order.client_order_id();
        let order_id_str = client_order_id.to_string();
        let trader_key = &adapter.database.trader_key;

        // Set up test data in Redis indexes to verify deletion
        let mut conn = adapter.database.con.clone();

        // Add to various indexes
        let index_keys = [
            format!("{trader_key}:index:order_ids"),
            format!("{trader_key}:index:orders"),
            format!("{trader_key}:index:orders_open"),
            format!("{trader_key}:index:orders_closed"),
            format!("{trader_key}:index:orders_emulated"),
            format!("{trader_key}:index:orders_inflight"),
        ];

        // Add to set-based indexes
        for index_key in &index_keys {
            let _: () = conn.sadd(index_key, &order_id_str).await.unwrap();
        }

        // Add to hash-based indexes
        let hash_keys = [
            format!("{trader_key}:index:order_position"),
            format!("{trader_key}:index:order_client"),
        ];

        for hash_key in &hash_keys {
            let _: () = conn
                .hset(hash_key, &order_id_str, "test_value")
                .await
                .unwrap();
        }

        // Wait for all Redis set/hash operations to complete
        let conn_clone = conn.clone();
        let index_keys_clone = index_keys.clone();
        let hash_keys_clone = hash_keys.clone();
        let order_id_str_clone = order_id_str.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let index_keys = index_keys_clone.clone();
                let hash_keys = hash_keys_clone.clone();
                let order_id_str = order_id_str_clone.clone();
                async move {
                    // Check all set-based indexes
                    for index_key in &index_keys {
                        let exists: bool = conn.sismember(index_key, &order_id_str).await.unwrap();
                        if !exists {
                            return false;
                        }
                    }
                    // Check all hash-based indexes
                    for hash_key in &hash_keys {
                        let exists: bool = conn.hexists(hash_key, &order_id_str).await.unwrap();
                        if !exists {
                            return false;
                        }
                    }
                    true
                }
            },
            Duration::from_secs(2),
        )
        .await;

        // Verify indexes contain the order ID before deletion
        for index_key in &index_keys {
            let exists: bool = conn.sismember(index_key, &order_id_str).await.unwrap();
            assert!(exists, "Order ID should exist in index {index_key}");
        }

        for hash_key in &hash_keys {
            let exists: bool = conn.hexists(hash_key, &order_id_str).await.unwrap();
            assert!(exists, "Order ID should exist in hash {hash_key}");
        }

        // Delete the order
        adapter.delete_order(&client_order_id).unwrap();

        // Wait until all indexes no longer contain the order ID
        let conn_clone = conn.clone();
        let index_keys_clone = index_keys.clone();
        let hash_keys_clone = hash_keys.clone();
        let order_id_str_clone = order_id_str.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let index_keys = index_keys_clone.clone();
                let hash_keys = hash_keys_clone.clone();
                let order_id_str = order_id_str_clone.clone();
                async move {
                    // Check all set-based indexes
                    for index_key in &index_keys {
                        let exists: bool = conn.sismember(index_key, &order_id_str).await.unwrap();
                        if exists {
                            return false;
                        }
                    }
                    // Check all hash-based indexes
                    for hash_key in &hash_keys {
                        let exists: bool = conn.hexists(hash_key, &order_id_str).await.unwrap();
                        if exists {
                            return false;
                        }
                    }
                    true
                }
            },
            Duration::from_secs(2),
        )
        .await;

        // Verify final state
        for index_key in &index_keys {
            let exists: bool = conn.sismember(index_key, &order_id_str).await.unwrap();
            assert!(!exists, "Order ID should be removed from index {index_key}");
        }

        for hash_key in &hash_keys {
            let exists: bool = conn.hexists(hash_key, &order_id_str).await.unwrap();
            assert!(!exists, "Order ID should be removed from hash {hash_key}");
        }

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_position_cleans_up_indexes() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let position_id = PositionId::new("P-INDEX-TEST");
        let position_id_str = position_id.to_string();
        let trader_key = &adapter.database.trader_key;

        // Set up test data in Redis indexes to verify deletion
        let mut conn = adapter.database.con.clone();

        // Add to position indexes
        let index_keys = [
            format!("{trader_key}:index:positions"),
            format!("{trader_key}:index:positions_open"),
            format!("{trader_key}:index:positions_closed"),
        ];

        for index_key in &index_keys {
            let _: () = conn.sadd(index_key, &position_id_str).await.unwrap();
        }

        // Wait for all Redis set operations to complete
        let conn_clone = conn.clone();
        let index_keys_clone = index_keys.clone();
        let position_id_str_clone = position_id_str.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let index_keys = index_keys_clone.clone();
                let position_id_str = position_id_str_clone.clone();
                async move {
                    for index_key in &index_keys {
                        let exists: bool =
                            conn.sismember(index_key, &position_id_str).await.unwrap();

                        if !exists {
                            return false;
                        }
                    }
                    true
                }
            },
            Duration::from_secs(2),
        )
        .await;

        // Verify indexes contain the position ID before deletion
        for index_key in &index_keys {
            let exists: bool = conn.sismember(index_key, &position_id_str).await.unwrap();
            assert!(exists, "Position ID should exist in index {index_key}");
        }

        // Delete the position
        adapter.delete_position(&position_id).unwrap();

        // Wait until all indexes no longer contain the position ID
        let conn_clone = conn.clone();
        let index_keys_clone = index_keys.clone();
        let position_id_str_clone = position_id_str.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let index_keys = index_keys_clone.clone();
                let position_id_str = position_id_str_clone.clone();
                async move {
                    for index_key in &index_keys {
                        let exists: bool =
                            conn.sismember(index_key, &position_id_str).await.unwrap();

                        if exists {
                            return false;
                        }
                    }
                    true
                }
            },
            Duration::from_secs(2),
        )
        .await;

        // Verify final state
        for index_key in &index_keys {
            let exists: bool = conn.sismember(index_key, &position_id_str).await.unwrap();
            assert!(
                !exists,
                "Position ID should be removed from index {index_key}"
            );
        }

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_debug_real_index_deletion() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        let client_order_id = order.client_order_id();
        let order_id_str = client_order_id.to_string();
        let trader_key = &adapter.database.trader_key;

        let mut conn = adapter.database.con.clone();

        // Set up test data exactly like real usage - just one index to test
        let test_index_key = format!("{trader_key}:index:orders");
        let _: () = conn.sadd(&test_index_key, &order_id_str).await.unwrap();

        println!("=== BEFORE DELETION ===");
        println!("Order ID: {order_id_str}");
        println!("Test index key: {test_index_key}");

        // Verify setup
        let exists_before: bool = conn
            .sismember(&test_index_key, &order_id_str)
            .await
            .unwrap();
        println!("Order ID exists in index before deletion: {exists_before}");
        assert!(exists_before);

        // Check all keys that match our pattern
        let all_keys: Vec<String> = conn.keys(format!("{trader_key}:*")).await.unwrap();
        println!("All Redis keys before deletion: {all_keys:?}");

        // Delete the order
        println!("\n=== DELETING ORDER ===");
        adapter.delete_order(&client_order_id).unwrap();

        // Wait for deletion to be processed
        wait_until_async(
            || {
                let mut conn = conn.clone();
                let test_index_key = test_index_key.clone();
                let order_id_str = order_id_str.clone();
                async move {
                    let exists: bool = conn
                        .sismember(&test_index_key, &order_id_str)
                        .await
                        .unwrap_or(true);
                    !exists
                }
            },
            Duration::from_secs(2),
        )
        .await;

        println!("\n=== AFTER DELETION ===");
        let exists_after: bool = conn
            .sismember(&test_index_key, &order_id_str)
            .await
            .unwrap();
        println!("Order ID exists in index after deletion: {exists_after}");

        // Check all keys again
        let all_keys_after: Vec<String> = conn.keys(format!("{trader_key}:*")).await.unwrap();
        println!("All Redis keys after deletion: {all_keys_after:?}");

        // Check what's actually in the index now
        let index_members: Vec<String> = conn.smembers(&test_index_key).await.unwrap();
        println!("Index members after deletion: {index_members:?}");

        // This will fail if index cleanup isn't working
        assert!(!exists_after, "Order ID should be removed from index");

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    /// Tests that the buffer flushes on a timer even when no new messages arrive.
    /// This verifies the fix for issue #3426 where blocking on channel receive
    /// prevented time-based buffer flushing during idle periods.
    #[tokio::test]
    async fn test_buffer_flushes_on_interval_when_idle() {
        let _guard = redis_test_mutex().lock().await;
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
            buffer_interval_ms: Some(50),
            ..Default::default()
        };

        let mut database = RedisCacheDatabase::new(trader_id, instance_id, config)
            .await
            .expect("Failed to create database");
        database.flushdb().await;

        let trader_key = database.trader_key.clone();
        let test_key = "general:test_key";

        let expected_key = format!("{trader_key}:{test_key}");

        database
            .insert(test_key.to_string(), Some(vec![Bytes::from("test_data")]))
            .unwrap();

        // Buffer should flush on timer even with no further messages
        let conn = database.con.clone();
        let expected_key_clone = expected_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn.clone();
                let expected_key = expected_key_clone.clone();
                async move { conn.exists(&expected_key).await.unwrap_or(false) }
            },
            Duration::from_secs(2),
        )
        .await;

        let mut conn = database.con.clone();
        let exists: bool = conn.exists(&expected_key).await.unwrap();

        assert!(
            exists,
            "Data should be flushed to Redis after buffer interval even when idle"
        );
    }

    /// Tests that with `buffer_interval_ms = 0`, data is flushed immediately
    /// without waiting for a timer.
    #[tokio::test]
    async fn test_buffer_flushes_immediately_with_zero_interval() {
        let _guard = redis_test_mutex().lock().await;
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
            buffer_interval_ms: Some(0),
            ..Default::default()
        };

        let mut database = RedisCacheDatabase::new(trader_id, instance_id, config)
            .await
            .expect("Failed to create database");
        database.flushdb().await;

        let trader_key = database.trader_key.clone();
        let test_key = "general:immediate_test";

        let expected_key = format!("{trader_key}:{test_key}");

        database
            .insert(test_key.to_string(), Some(vec![Bytes::from("test_data")]))
            .unwrap();

        // Brief delay for async task processing
        let conn = database.con.clone();
        let expected_key_clone = expected_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn.clone();
                let expected_key = expected_key_clone.clone();
                async move { conn.exists(&expected_key).await.unwrap_or(false) }
            },
            Duration::from_secs(2),
        )
        .await;

        let mut conn = database.con.clone();
        let exists: bool = conn.exists(&expected_key).await.unwrap();

        assert!(
            exists,
            "Data should be flushed immediately with zero buffer interval"
        );
    }

    /// Tests that pending buffered data is drained when close is called.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_buffer_drains_on_close() {
        let _guard = redis_test_mutex().lock().await;
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
            buffer_interval_ms: Some(10000),
            ..Default::default()
        };

        let mut database = RedisCacheDatabase::new(trader_id, instance_id, config)
            .await
            .expect("Failed to create database");
        database.flushdb().await;

        let trader_key = database.trader_key.clone();
        let test_key = "general:close_test";
        let expected_key = format!("{trader_key}:{test_key}");

        database
            .insert(test_key.to_string(), Some(vec![Bytes::from("test_data")]))
            .unwrap();

        // Data should NOT be in Redis yet (buffer interval is 10 seconds)
        let mut conn = database.con.clone();
        let exists_before: bool = conn.exists(&expected_key).await.unwrap();
        assert!(
            !exists_before,
            "Data should be buffered, not yet flushed to Redis"
        );

        database.close();

        let exists_after: bool = conn.exists(&expected_key).await.unwrap();
        assert!(
            exists_after,
            "Data should be flushed to Redis when close is called"
        );
    }

    /// Tests `add_custom_data` and `load_custom_data` roundtrip with filtering by `type_name`
    /// and identifier.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_and_load_custom_data_roundtrip() {
        let _guard = redis_test_mutex().lock().await;
        ensure_stub_custom_data_registered();
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let data = stub_custom_data(1000, 42, None, Some("id1".to_string()));
        let data_type = DataType::new("StubCustomData", None, Some("id1".to_string()));

        adapter
            .add_custom_data(&data)
            .expect("add_custom_data failed");

        let conn = adapter.database.con.clone();
        let custom_pattern = format!("{}:custom:*", adapter.database.trader_key);
        wait_until_async(
            move || {
                let mut conn = conn.clone();
                let custom_pattern = custom_pattern.clone();
                async move {
                    let keys: Vec<String> = conn.keys(custom_pattern).await.unwrap_or_default();
                    keys.len() == 1
                }
            },
            Duration::from_secs(5),
        )
        .await;

        let loaded = adapter
            .load_custom_data(&data_type)
            .expect("load_custom_data failed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], data);

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    /// Tests that `load_custom_data` returns only items matching the requested `DataType`
    /// (identifier filter).
    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_custom_data_filters_by_identifier() {
        let _guard = redis_test_mutex().lock().await;
        ensure_stub_custom_data_registered();
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let data1 = stub_custom_data(2000, 1, None, Some("id1".to_string()));
        let data2 = stub_custom_data(2001, 2, None, Some("id2".to_string()));
        adapter.add_custom_data(&data1).unwrap();
        adapter.add_custom_data(&data2).unwrap();

        let data_type1 = DataType::new("StubCustomData", None, Some("id1".to_string()));
        let data_type2 = DataType::new("StubCustomData", None, Some("id2".to_string()));

        let conn = adapter.database.con.clone();
        let custom_pattern = format!("{}:custom:*", adapter.database.trader_key);
        wait_until_async(
            move || {
                let mut conn = conn.clone();
                let custom_pattern = custom_pattern.clone();
                async move {
                    let keys: Vec<String> = conn.keys(custom_pattern).await.unwrap_or_default();
                    keys.len() == 2
                }
            },
            Duration::from_secs(5),
        )
        .await;

        let loaded_id1 = adapter
            .load_custom_data(&data_type1)
            .expect("load_custom_data failed");
        assert_eq!(loaded_id1.len(), 1);
        assert_eq!(loaded_id1[0].data_type.identifier(), Some("id1"));

        let loaded_id2 = adapter
            .load_custom_data(&data_type2)
            .expect("load_custom_data failed");
        assert_eq!(loaded_id2.len(), 1);
        assert_eq!(loaded_id2[0].data_type.identifier(), Some("id2"));

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_restart_recovery_restores_order_indexes() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let client_id = ClientId::new("BINANCE");

        let mut order_1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-1"))
            .build();
        let order_2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-2"))
            .build();

        let OrderEventAny::Filled(fill) = TestOrderEventStubs::filled(
            &order_1,
            &instrument,
            None,
            Some(PositionId::new("P-1")),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        let mut position = Position::new(&instrument, fill);
        let mut account = AccountAny::from(cash_account_state_multi());

        adapter.add_instrument(&instrument).unwrap();
        adapter.add_order(&order_1, Some(client_id)).unwrap();
        adapter.add_order(&order_2, None).unwrap();
        adapter.add_position(&position).unwrap();
        adapter.add_account(&account).unwrap();
        adapter
            .index_order_position(order_1.client_order_id(), position.id)
            .unwrap();

        let submitted = TestOrderEventStubs::submitted(&order_1, AccountId::new("BINANCE-001"));
        order_1.apply(submitted.clone()).unwrap();
        adapter.update_order(&submitted).unwrap();

        let accepted = TestOrderEventStubs::accepted(
            &order_1,
            AccountId::new("BINANCE-001"),
            VenueOrderId::new("1"),
        );
        order_1.apply(accepted.clone()).unwrap();
        adapter.update_order(&accepted).unwrap();

        account
            .apply(cash_account_state_multi_changed_btc())
            .unwrap();
        adapter.update_account(&account).unwrap();

        let OrderEventAny::Filled(close_fill) = TestOrderEventStubs::filled(
            &order_2,
            &instrument,
            Some(TradeId::new("E-2")),
            Some(position.id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        position.apply(&close_fill);
        adapter.update_position(&position).unwrap();

        let actor_id = ComponentId::new("ACTOR-001");
        assert!(adapter.load_actor(&actor_id).unwrap().is_empty());
        let mut actor_state = AHashMap::new();
        actor_state.insert("A".to_string(), Bytes::from_static(b"1"));
        adapter.update_actor(&actor_id, &actor_state).unwrap();

        let strategy_id = StrategyId::new("S-001");
        assert!(adapter.load_strategy(&strategy_id).unwrap().is_empty());
        let mut strategy_state = AHashMap::new();
        strategy_state.insert("UserState".to_string(), Bytes::from_static(b"1"));
        adapter
            .update_strategy(&strategy_id, &strategy_state)
            .unwrap();

        adapter.snapshot_order_state(&order_1).unwrap();
        adapter
            .snapshot_position_state(
                &position,
                UnixNanos::from(2_000_000_000),
                Some(Money::from("1 USD")),
            )
            .unwrap();
        adapter.heartbeat(UnixNanos::from(1_000_000_000)).unwrap();

        // Wait until the asynchronous writes land in Redis
        wait_until_async(
            || async {
                adapter.load_orders().await.unwrap().len() == 2
                    && adapter.load_positions().await.unwrap().len() == 1
                    && adapter.load_accounts().await.unwrap().len() == 1
                    && adapter
                        .load_order(&order_1.client_order_id())
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded.status() == OrderStatus::Accepted)
                    && adapter
                        .load_position(&position.id)
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded == position)
                    && adapter
                        .load_account(&account.id())
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded == account)
                    && adapter.load_index_order_position().unwrap().len() == 1
                    && adapter.load_index_order_client().unwrap().len() == 1
                    && adapter.load_actor(&actor_id).unwrap() == actor_state
                    && adapter.load_strategy(&strategy_id).unwrap() == strategy_state
            },
            Duration::from_secs(5),
        )
        .await;

        let mut conn = adapter.database.con.clone();
        let encoding = adapter.database.get_encoding();
        let order_snapshot_key = format!(
            "{}:snapshots:orders:{}",
            adapter.database.trader_key,
            order_1.client_order_id()
        );
        let position_snapshot_key = format!(
            "{}:snapshots:positions:{}",
            adapter.database.trader_key, position.id
        );
        let account_key = format!("{}:accounts:{}", adapter.database.trader_key, account.id());
        let order_1_key = format!(
            "{}:orders:{}",
            adapter.database.trader_key,
            order_1.client_order_id()
        );
        let order_2_key = format!(
            "{}:orders:{}",
            adapter.database.trader_key,
            order_2.client_order_id()
        );
        let position_key = format!("{}:positions:{}", adapter.database.trader_key, position.id);
        let heartbeat_key = format!("{}:health:heartbeat", adapter.database.trader_key);

        wait_until_async(
            || {
                let mut conn = adapter.database.con.clone();
                let order_snapshot_key = order_snapshot_key.clone();
                let position_snapshot_key = position_snapshot_key.clone();
                let heartbeat_key = heartbeat_key.clone();

                async move {
                    conn.llen::<_, usize>(&order_snapshot_key)
                        .await
                        .unwrap_or(0)
                        == 1
                        && conn
                            .llen::<_, usize>(&position_snapshot_key)
                            .await
                            .unwrap_or(0)
                            == 1
                        && conn
                            .exists::<_, bool>(&heartbeat_key)
                            .await
                            .unwrap_or(false)
                }
            },
            Duration::from_secs(5),
        )
        .await;

        let account_frames: Vec<Bytes> = conn.lrange(&account_key, 0, -1).await.unwrap();
        let order_1_frames: Vec<Bytes> = conn.lrange(&order_1_key, 0, -1).await.unwrap();
        let order_2_frames: Vec<Bytes> = conn.lrange(&order_2_key, 0, -1).await.unwrap();
        let position_frames: Vec<Bytes> = conn.lrange(&position_key, 0, -1).await.unwrap();
        let order_snapshot_frames: Vec<Bytes> =
            conn.lrange(&order_snapshot_key, 0, -1).await.unwrap();
        let position_snapshot_frames: Vec<Bytes> =
            conn.lrange(&position_snapshot_key, 0, -1).await.unwrap();
        let heartbeat: String = conn.get(&heartbeat_key).await.unwrap();

        let account_events: Vec<AccountState> = account_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let order_1_events: Vec<OrderEventAny> = order_1_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let order_2_events: Vec<OrderEventAny> = order_2_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let position_events: Vec<OrderFilled> = position_frames
            .iter()
            .map(|frame| DatabaseQueries::deserialize_payload(encoding, frame).unwrap())
            .collect();
        let order_snapshot: OrderSnapshot =
            DatabaseQueries::deserialize_payload(encoding, &order_snapshot_frames[0]).unwrap();
        let position_snapshot: PositionSnapshot =
            DatabaseQueries::deserialize_payload(encoding, &position_snapshot_frames[0]).unwrap();

        assert_eq!(account_events, account.events());
        assert_eq!(
            order_1_events,
            order_1.events().into_iter().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            order_2_events,
            order_2.events().into_iter().cloned().collect::<Vec<_>>()
        );
        assert_eq!(position_events, position.events.clone());
        assert_eq!(order_snapshot.client_order_id, order_1.client_order_id());
        assert_eq!(position_snapshot.position_id, position.id);
        assert_eq!(position_snapshot.ts_init, UnixNanos::from(2_000_000_000));
        assert_eq!(heartbeat, "1970-01-01T00:00:01.000000000Z");

        let order_id = order_1.client_order_id().to_string();
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:orders_open", adapter.database.trader_key),
                &order_id,
            )
            .await
            .unwrap(),
            order_1.is_open()
        );
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:orders_closed", adapter.database.trader_key),
                &order_id,
            )
            .await
            .unwrap(),
            order_1.is_closed()
        );
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:orders_inflight", adapter.database.trader_key),
                &order_id,
            )
            .await
            .unwrap(),
            order_1.is_inflight()
        );

        let position_id = position.id.to_string();
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:positions_open", adapter.database.trader_key),
                &position_id,
            )
            .await
            .unwrap(),
            position.is_open()
        );
        assert_eq!(
            conn.sismember::<_, _, bool>(
                format!("{}:index:positions_closed", adapter.database.trader_key),
                &position_id,
            )
            .await
            .unwrap(),
            position.is_closed()
        );

        // Simulate a node restart: fresh adapter over the same trader key, fresh cache
        let restarted_adapter = connect_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");
        let mut cache = Cache::new(None, Some(Box::new(restarted_adapter)));
        cache.cache_all().await.unwrap();
        cache.build_index();

        assert_eq!(
            cache.position_id(&order_1.client_order_id()),
            Some(&position.id)
        );
        assert_eq!(
            cache.client_id(&order_1.client_order_id()),
            Some(&client_id)
        );
        assert!(cache.position_id(&order_2.client_order_id()).is_none());
        assert!(cache.client_id(&order_2.client_order_id()).is_none());
        assert!(cache.order(&order_1.client_order_id()).is_some());
        assert!(cache.position(&position.id).is_some());
        assert_eq!(
            cache
                .order(&order_1.client_order_id())
                .map(|order| order.status()),
            Some(OrderStatus::Accepted)
        );
        assert_eq!(
            cache.account(&account.id()).map(|loaded| loaded.cloned()),
            Some(account)
        );

        adapter.delete_actor(&actor_id).unwrap();
        adapter.delete_strategy(&strategy_id).unwrap();
        wait_until_async(
            || async {
                adapter.load_actor(&actor_id).unwrap().is_empty()
                    && adapter.load_strategy(&strategy_id).unwrap().is_empty()
            },
            Duration::from_secs(5),
        )
        .await;

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_update_order_appends_event_and_reloads_latest_state() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-1"))
            .build();

        adapter.add_order(&order, None).unwrap();

        let submitted = TestOrderEventStubs::submitted(&order, AccountId::new("BINANCE-001"));
        order.apply(submitted.clone()).unwrap();
        adapter.update_order(&submitted).unwrap();

        wait_until_async(
            || async {
                adapter
                    .load_order(&order.client_order_id())
                    .await
                    .unwrap()
                    .is_some_and(|loaded| loaded.status() == OrderStatus::Submitted)
            },
            Duration::from_secs(5),
        )
        .await;

        let loaded = adapter
            .load_order(&order.client_order_id())
            .await
            .unwrap()
            .unwrap();
        let key = format!(
            "{}:orders:{}",
            adapter.database.trader_key,
            order.client_order_id()
        );
        let frames: Vec<Bytes> = adapter
            .database
            .con
            .clone()
            .lrange(key, 0, -1)
            .await
            .unwrap();
        let events: Vec<OrderEventAny> = frames
            .iter()
            .map(|frame| {
                DatabaseQueries::deserialize_payload(adapter.database.get_encoding(), frame)
                    .unwrap()
            })
            .collect();

        assert_eq!(loaded.status(), OrderStatus::Submitted);
        assert_eq!(loaded, order);
        assert_eq!(
            events,
            order.events().into_iter().cloned().collect::<Vec<_>>()
        );

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_order_replaces_event_log_with_initialized_event() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-2"))
            .build();

        adapter.add_order(&order, None).unwrap();

        let submitted = TestOrderEventStubs::submitted(&order, AccountId::new("BINANCE-001"));
        order.apply(submitted.clone()).unwrap();
        adapter.update_order(&submitted).unwrap();

        wait_until_async(
            || async {
                adapter
                    .load_order(&order.client_order_id())
                    .await
                    .unwrap()
                    .is_some_and(|loaded| loaded.status() == OrderStatus::Submitted)
            },
            Duration::from_secs(5),
        )
        .await;

        adapter.add_order(&order, None).unwrap();

        let key = format!(
            "{}:orders:{}",
            adapter.database.trader_key,
            order.client_order_id()
        );
        wait_until_async(
            || {
                let mut conn = adapter.database.con.clone();
                let key = key.clone();
                async move { conn.llen::<_, usize>(&key).await.unwrap_or(0) == 1 }
            },
            Duration::from_secs(5),
        )
        .await;

        let loaded = adapter
            .load_order(&order.client_order_id())
            .await
            .unwrap()
            .unwrap();
        let frames: Vec<Bytes> = adapter
            .database
            .con
            .clone()
            .lrange(key, 0, -1)
            .await
            .unwrap();
        let events: Vec<OrderEventAny> = frames
            .iter()
            .map(|frame| {
                DatabaseQueries::deserialize_payload(adapter.database.get_encoding(), frame)
                    .unwrap()
            })
            .collect();

        assert_eq!(loaded.status(), OrderStatus::Initialized);
        assert_eq!(events.len(), 1);
        assert!(matches!(events.as_slice(), [OrderEventAny::Initialized(_)]));

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_position_replaces_event_log_for_reused_position_id() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        adapter.add_instrument(&instrument).unwrap();

        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-1"))
            .build();
        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-2"))
            .build();
        let reopen_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-3"))
            .build();

        let position_id = PositionId::new("P-NETTING-REUSED");
        let OrderEventAny::Filled(open_fill) = TestOrderEventStubs::filled(
            &open_order,
            &instrument,
            Some(TradeId::new("E-1")),
            Some(position_id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        let mut closed_position = Position::new(&instrument, open_fill);
        adapter.add_position(&closed_position).unwrap();

        let OrderEventAny::Filled(close_fill) = TestOrderEventStubs::filled(
            &close_order,
            &instrument,
            Some(TradeId::new("E-2")),
            Some(position_id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        closed_position.apply(&close_fill);
        adapter.update_position(&closed_position).unwrap();

        let OrderEventAny::Filled(reopen_fill) = TestOrderEventStubs::filled(
            &reopen_order,
            &instrument,
            Some(TradeId::new("E-3")),
            Some(position_id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        let reopened_position = Position::new(&instrument, reopen_fill);
        adapter.add_position(&reopened_position).unwrap();

        let key = format!(
            "{}:positions:{}",
            adapter.database.trader_key, reopened_position.id
        );
        let positions_open_key = format!("{}:index:positions_open", adapter.database.trader_key);
        let positions_closed_key =
            format!("{}:index:positions_closed", adapter.database.trader_key);
        let con = adapter.database.con.clone();
        let encoding = adapter.database.get_encoding();
        let expected_position = reopened_position.clone();
        let wait_key = key.clone();
        let wait_positions_open_key = positions_open_key.clone();
        let wait_positions_closed_key = positions_closed_key.clone();
        let reopen_event_id = reopen_fill.event_id;

        wait_until_async(
            move || {
                let con = con.clone();
                let key = wait_key.clone();
                let positions_open_key = wait_positions_open_key.clone();
                let positions_closed_key = wait_positions_closed_key.clone();
                let expected_position = expected_position.clone();
                async move {
                    let mut conn = con.clone();
                    let frames: Vec<Bytes> = conn.lrange(&key, 0, -1).await.unwrap_or_default();

                    if frames.len() != 1 {
                        return false;
                    }
                    let fill: OrderFilled =
                        DatabaseQueries::deserialize_payload(encoding, &frames[0]).unwrap();
                    fill.event_id == reopen_event_id
                        && conn
                            .sismember::<_, _, bool>(
                                &positions_open_key,
                                expected_position.id.to_string(),
                            )
                            .await
                            .unwrap_or(false)
                        && !conn
                            .sismember::<_, _, bool>(
                                &positions_closed_key,
                                expected_position.id.to_string(),
                            )
                            .await
                            .unwrap_or(true)
                }
            },
            Duration::from_secs(5),
        )
        .await;

        let frames: Vec<Bytes> = adapter
            .database
            .con
            .clone()
            .lrange(&key, 0, -1)
            .await
            .unwrap();
        let fills: Vec<OrderFilled> = frames
            .iter()
            .map(|frame| {
                DatabaseQueries::deserialize_payload(adapter.database.get_encoding(), frame)
                    .unwrap()
            })
            .collect();

        assert_eq!(fills, reopened_position.events.clone());

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_position_duplicate_fill_returns_error() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        adapter.add_instrument(&instrument).unwrap();

        wait_until_async(
            || async {
                adapter
                    .load_instrument(&instrument.id())
                    .await
                    .unwrap()
                    .is_some()
            },
            Duration::from_secs(5),
        )
        .await;

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-1"))
            .build();
        let position_id = PositionId::new("P-DUPLICATE-FILL");
        let OrderEventAny::Filled(fill) = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::new("E-1")),
            Some(position_id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };

        let payload =
            DatabaseQueries::serialize_payload(adapter.database.get_encoding(), &fill).unwrap();
        let key = format!("{}:positions:{position_id}", adapter.database.trader_key);
        let mut conn = adapter.database.con.clone();
        conn.rpush::<_, _, ()>(&key, payload.clone()).await.unwrap();
        conn.rpush::<_, _, ()>(&key, payload).await.unwrap();

        let result = adapter.load_position(&position_id).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("E-1"));

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_update_order_appends_event_when_index_replay_fails() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-000000-001-001-1"))
            .build();
        let key = format!(
            "{}:orders:{}",
            adapter.database.trader_key,
            order.client_order_id()
        );
        let encoding = adapter.database.get_encoding();
        let init_payload =
            DatabaseQueries::serialize_payload(encoding, order.last_event()).unwrap();
        let mut conn = adapter.database.con.clone();
        conn.rpush::<_, _, ()>(&key, init_payload).await.unwrap();
        conn.rpush::<_, _, ()>(&key, vec![0xc1_u8]).await.unwrap();

        let submitted = TestOrderEventStubs::submitted(&order, AccountId::new("BINANCE-001"));
        order.apply(submitted.clone()).unwrap();
        adapter.update_order(&submitted).unwrap();

        wait_until_async(
            || {
                let mut conn = adapter.database.con.clone();
                let key = key.clone();
                async move { conn.llen::<_, usize>(&key).await.unwrap_or(0) == 3 }
            },
            Duration::from_secs(5),
        )
        .await;

        let frames: Vec<Bytes> = adapter
            .database
            .con
            .clone()
            .lrange(&key, 0, -1)
            .await
            .unwrap();
        let appended: OrderEventAny =
            DatabaseQueries::deserialize_payload(encoding, &frames[2]).unwrap();

        assert_eq!(appended, submitted);

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_redis_loads_reject_snapshot_market_data_and_signals() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let client_order_id = ClientOrderId::new("O-19700101-000000-001-001-1");
        let position_id = PositionId::new("P-UNSUPPORTED");

        assert!(adapter.load_order_snapshot(&client_order_id).is_err());
        assert!(adapter.load_position_snapshot(&position_id).is_err());
        assert!(adapter.load_quotes(&instrument.id()).is_err());
        assert!(adapter.load_trades(&instrument.id()).is_err());
        assert!(adapter.load_funding_rates(&instrument.id()).is_err());
        assert!(adapter.load_bars(&instrument.id()).is_err());
        assert!(adapter.load_signals("signals").is_err());

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_reference_data_round_trips() {
        let _guard = redis_test_mutex().lock().await;
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let key = "risk-state".to_string();
        let value = Bytes::from_static(b"enabled");
        let currency = Currency::USD();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let synthetic = SyntheticInstrument::default();

        adapter.add(key.clone(), value.clone()).unwrap();
        adapter.add_currency(&currency).unwrap();
        adapter.add_instrument(&instrument).unwrap();
        adapter.add_synthetic(&synthetic).unwrap();

        wait_until_async(
            || async {
                adapter.load().unwrap().get(&key) == Some(&value)
                    && adapter.load_currency(&currency.code).await.unwrap() == Some(currency)
                    && adapter
                        .load_instrument(&instrument.id())
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded == instrument)
                    && adapter
                        .load_synthetic(&synthetic.id)
                        .await
                        .unwrap()
                        .is_some_and(|loaded| loaded == synthetic)
            },
            Duration::from_secs(5),
        )
        .await;

        let mut adapter = adapter;
        adapter.flush().unwrap();
    }
}
