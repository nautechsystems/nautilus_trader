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
    use std::time::Duration;

    use nautilus_common::{
        cache::{CacheConfig, database::CacheDatabaseAdapter},
        enums::SerializationEncoding,
        msgbus::database::DatabaseConfig,
        testing::wait_until_async,
    };
    use nautilus_core::UUID4;
    use nautilus_infrastructure::redis::cache::{RedisCacheDatabase, RedisCacheDatabaseAdapter};
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{ClientOrderId, PositionId, TraderId},
        instruments::stubs::crypto_perpetual_ethusdt,
        orders::{Order, builder::OrderTestBuilder},
        types::Quantity,
    };

    async fn get_redis_cache_adapter()
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

        let mut database = RedisCacheDatabase::new(trader_id, instance_id, config).await?;

        // Clean the database at the start of each test
        database.flushdb().await;

        let adapter = RedisCacheDatabaseAdapter {
            encoding: SerializationEncoding::MsgPack,
            database,
        };

        Ok(adapter)
    }

    #[tokio::test]
    async fn test_delete_order() {
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
        use redis::AsyncCommands;
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
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let position_id = PositionId::new("P-123456");
        let expected_key = format!("{}:positions:{}", adapter.database.trader_key, position_id);

        // Set up test data in Redis to verify deletion
        use redis::AsyncCommands;
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
        let mut adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_operations_are_idempotent() {
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

    #[tokio::test]
    async fn test_delete_order_cleans_up_indexes() {
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
        use redis::AsyncCommands;
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
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let position_id = PositionId::new("P-INDEX-TEST");
        let position_id_str = position_id.to_string();
        let trader_key = &adapter.database.trader_key;

        // Set up test data in Redis indexes to verify deletion
        use redis::AsyncCommands;
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

        use redis::AsyncCommands;
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
}
