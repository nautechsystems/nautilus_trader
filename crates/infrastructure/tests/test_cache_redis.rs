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
        identifiers::{AccountId, ClientOrderId, PositionId, TraderId},
        instruments::stubs::crypto_perpetual_ethusdt,
        orders::{Order, builder::OrderTestBuilder},
        types::Quantity,
    };

    async fn get_redis_cache_adapter()
    -> Result<RedisCacheDatabaseAdapter, Box<dyn std::error::Error>> {
        let trader_id = TraderId::from("test-trader");
        let instance_id = UUID4::new();

        // Create a Redis database config
        let mut config = CacheConfig::default();
        config.database = Some(DatabaseConfig {
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
        });

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
    async fn test_delete_account_event() {
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let account_id = AccountId::new("ACCOUNT-001");
        let event_id = "event-123";

        adapter.delete_account_event(&account_id, event_id).unwrap();

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_account_event_from_list() {
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        // Use a unique account ID to avoid interference from other tests
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let account_id = AccountId::new(format!("ACCOUNT-LIST-{timestamp}"));
        let event_id_to_delete = "event-456";
        let event_id_to_keep = "event-789";
        let trader_key = &adapter.database.trader_key;

        // Create test account event data (simulate serialized account events)
        let test_data_with_target = format!(
            r#"{{"event_id":"{event_id_to_delete}","account_id":"{account_id}","data":"test1"}}"#
        );
        let test_data_without_target = format!(
            r#"{{"event_id":"{event_id_to_keep}","account_id":"{account_id}","data":"test2"}}"#
        );
        let another_test_data_with_target = format!(
            r#"{{"event_id":"{event_id_to_delete}","account_id":"{account_id}","data":"test3"}}"#
        );

        use redis::AsyncCommands;
        let mut conn = adapter.database.con.clone();
        let list_key = format!("{trader_key}:accounts:{account_id}");

        // Ensure the list is clean before starting
        let _: () = conn.del(&list_key).await.unwrap();

        // Add test data to the Redis list
        let _: () = conn.rpush(&list_key, &test_data_with_target).await.unwrap();
        let _: () = conn
            .rpush(&list_key, &test_data_without_target)
            .await
            .unwrap();
        let _: () = conn
            .rpush(&list_key, &another_test_data_with_target)
            .await
            .unwrap();

        // Wait for Redis list operations to complete
        let conn_clone = conn.clone();
        let list_key_clone = list_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let list_key = list_key_clone.clone();
                async move {
                    let count: i32 = conn.llen(&list_key).await.unwrap();
                    count == 3
                }
            },
            Duration::from_secs(2),
        )
        .await;

        // Verify initial state - should have 3 items
        let initial_count: i32 = conn.llen(&list_key).await.unwrap();
        assert_eq!(initial_count, 3);

        // Get initial list contents
        let initial_items: Vec<String> = conn.lrange(&list_key, 0, -1).await.unwrap();
        assert_eq!(initial_items.len(), 3);
        assert!(
            initial_items
                .iter()
                .any(|item| item.contains(event_id_to_delete))
        );
        assert!(
            initial_items
                .iter()
                .any(|item| item.contains(event_id_to_keep))
        );

        // Delete account events with the target event_id
        adapter
            .delete_account_event(&account_id, event_id_to_delete)
            .unwrap();

        // Wait until the account event is deleted
        let conn_clone = conn.clone();
        let list_key_clone = list_key.clone();
        wait_until_async(
            move || {
                let mut conn = conn_clone.clone();
                let list_key = list_key_clone.clone();
                async move {
                    let count: i32 = conn.llen(&list_key).await.unwrap();
                    count == 1 // Should have 1 item remaining after deletion
                }
            },
            Duration::from_secs(2),
        )
        .await;

        // Verify the list now only contains items without the target event_id
        let final_count: i32 = conn.llen(&list_key).await.unwrap();
        assert_eq!(
            final_count, 1,
            "Should have 1 item remaining after deletion"
        );

        let final_items: Vec<String> = conn.lrange(&list_key, 0, -1).await.unwrap();
        assert_eq!(final_items.len(), 1);

        // The remaining item should be the one with event_id_to_keep
        assert!(final_items[0].contains(event_id_to_keep));
        assert!(!final_items[0].contains(event_id_to_delete));

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_nonexistent_order() {
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let fake_order_id = ClientOrderId::new("O-nonexistent");
        adapter.delete_order(&fake_order_id).unwrap();

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_nonexistent_position() {
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let fake_position_id = PositionId::new("P-nonexistent");
        adapter.delete_position(&fake_position_id).unwrap();

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_nonexistent_account_event() {
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let fake_account_id = AccountId::new("ACCOUNT-nonexistent");
        adapter
            .delete_account_event(&fake_account_id, "fake-event")
            .unwrap();

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
        let account_id = AccountId::new("ACCOUNT-IDEMPOTENT-TEST");

        for _ in 0..3 {
            adapter.delete_order(&order_id).unwrap();
            adapter.delete_position(&position_id).unwrap();
            adapter
                .delete_account_event(&account_id, "test-event")
                .unwrap();
        }

        // Final cleanup
        let mut adapter = adapter;
        adapter.flush().unwrap();
    }

    #[tokio::test]
    async fn test_delete_account_event_functionality() {
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        let account_id = AccountId::new("ACCOUNT-TEST");
        let event_id = "event-123";

        // First verify that the delete command can be sent without error
        adapter.delete_account_event(&account_id, event_id).unwrap();

        // Note: This now uses DeleteFromList operation to target the account list:
        // "trader-{id}:accounts:ACCOUNT-TEST"
        // The implementation is now fully functional using a Lua script.

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
    async fn test_delete_account_event_edge_cases() {
        let adapter = get_redis_cache_adapter()
            .await
            .expect("Failed to create adapter");

        // Use unique account IDs to avoid interference
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let account_id1 = AccountId::new(format!("ACCOUNT-EDGE-1-{timestamp}"));
        let account_id2 = AccountId::new(format!("ACCOUNT-EDGE-2-{timestamp}"));

        use redis::AsyncCommands;
        let mut conn = adapter.database.con.clone();

        // Test 1: Delete from non-existent list (should not error)
        adapter
            .delete_account_event(&account_id1, "nonexistent-event")
            .unwrap();

        // Wait a moment for the operation to complete
        wait_until_async(
            || async { true }, // No-op wait since this should complete immediately
            Duration::from_millis(100),
        )
        .await;

        // Verify no list was created
        let list_key1 = format!("{}:accounts:{}", adapter.database.trader_key, account_id1);
        let list_exists: bool = conn.exists(&list_key1).await.unwrap();
        assert!(
            !list_exists,
            "No list should be created for non-existent deletion"
        );

        // Test 2: Delete non-existent event ID from populated list
        let list_key2 = format!("{}:accounts:{}", adapter.database.trader_key, account_id2);

        // Ensure clean start
        let _: () = conn.del(&list_key2).await.unwrap();

        // Add some test data
        let test_data = r#"{"event_id":"real-event","data":"test"}"#;
        let _: () = conn.rpush(&list_key2, test_data).await.unwrap();

        // Verify we have 1 item
        let initial_count: i32 = conn.llen(&list_key2).await.unwrap();
        assert_eq!(initial_count, 1);

        // Try to delete non-existent event
        adapter
            .delete_account_event(&account_id2, "does-not-exist")
            .unwrap();

        // Wait for the operation to complete (should be a no-op)
        wait_until_async(
            || async { true }, // No-op wait since this should complete immediately
            Duration::from_millis(100),
        )
        .await;

        // Should still have 1 item (nothing should be deleted)
        let final_count: i32 = conn.llen(&list_key2).await.unwrap();
        assert_eq!(final_count, 1);

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

        // Give some time for async operations
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

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
