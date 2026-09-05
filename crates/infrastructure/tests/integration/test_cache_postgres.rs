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

#[cfg(all(feature = "postgres", target_os = "linux"))]
use nautilus_common::cache::{Cache, database::CacheDatabaseAdapter};

#[must_use]
#[cfg(all(feature = "postgres", target_os = "linux"))]
fn get_cache(cache_database: Option<Box<dyn CacheDatabaseAdapter>>) -> Cache {
    Cache::new(None, cache_database)
}

#[cfg(test)]
#[cfg(feature = "postgres")]
#[cfg(target_os = "linux")] // Databases only tested and supported on Linux
mod serial_tests {
    use std::time::Duration;

    use ahash::AHashMap;
    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_common::{cache::database::CacheDatabaseAdapter, testing::wait_until_async};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_infrastructure::sql::{
        cache::{PostgresCacheDatabase, get_pg_cache_database},
        queries::DatabaseQueries,
    };
    use nautilus_model::{
        accounts::AccountAny,
        data::InstrumentClose,
        enums::{
            CurrencyType, InstrumentCloseType, LiquiditySide, OrderSide, OrderType, TimeInForce,
        },
        events::{
            OrderEventAny,
            order::spec::{
                OrderAcceptedSpec, OrderCancelRejectedSpec, OrderCanceledSpec, OrderDeniedSpec,
                OrderEmulatedSpec, OrderExpiredSpec, OrderFillVoidedSpec, OrderFilledSpec,
                OrderInitializedSpec, OrderModifyRejectedSpec, OrderPendingCancelSpec,
                OrderPendingUpdateSpec, OrderRejectedSpec, OrderReleasedSpec, OrderSubmittedSpec,
                OrderTriggeredSpec, OrderUpdatedSpec,
            },
        },
        identifiers::{
            AccountId, ActorId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId,
            StrategyId, TradeId, TraderId, VenueOrderId,
        },
        instruments::{
            Instrument, InstrumentAny,
            stubs::{crypto_perpetual_ethusdt, currency_pair_ethusdt},
        },
        orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        position::Position,
        types::{Currency, Money, Price, Quantity},
    };
    use sqlx::PgPool;
    use ustr::Ustr;

    use super::get_cache;

    async fn get_test_pg_cache_database() -> anyhow::Result<PostgresCacheDatabase> {
        match tokio::time::timeout(Duration::from_secs(2), get_pg_cache_database()).await {
            Ok(result) => result.map_err(|e| {
                anyhow::anyhow!("A running PostgreSQL service is required for this test: {e}")
            }),
            Err(e) => Err(anyhow::anyhow!(
                "A running PostgreSQL service is required for this test: connection timed out: \
                 {e}"
            )),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_instruments() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_test_pg_cache_database().await.unwrap())));

        let eth = Currency::new("ETH", 2, 0, "ETH", CurrencyType::Crypto);
        let usdt = Currency::new("USDT", 2, 0, "USDT", CurrencyType::Crypto);
        let crypto_perpetual = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());

        // Insert into database and wait
        database.add_currency(&eth).unwrap();
        database.add_currency(&usdt).unwrap();
        database.add_instrument(&crypto_perpetual).unwrap();
        wait_until_async(
            || async {
                let currencies = database.load_currencies().await.unwrap();
                let instruments = database.load_instruments().await.unwrap();
                currencies.len() >= 2 && !instruments.is_empty()
            },
            Duration::from_secs(3),
        )
        .await;

        // Load instruments and build indexes
        cache.cache_instruments().await.unwrap();
        cache.build_index();

        let cached_instrument_ids = cache.instrument_ids(None);
        assert_eq!(cached_instrument_ids.len(), 1);
        assert_eq!(cached_instrument_ids, vec![&crypto_perpetual.id()]);
        let target_instrument = cache.instrument(&crypto_perpetual.id());
        assert_eq!(target_instrument.unwrap(), &crypto_perpetual);

        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_orders() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_test_pg_cache_database().await.unwrap())));

        let instrument = currency_pair_ethusdt();
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-0000-001-001-1"))
            .tags(vec![Ustr::from("tag-1"), Ustr::from("tag-2")])
            .build();

        // Add foreign key dependencies: instrument and currencies
        database
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        database.add_currency(&instrument.quote_currency()).unwrap();
        database
            .add_instrument(&InstrumentAny::CurrencyPair(instrument))
            .unwrap();

        // Insert into database and wait
        database.add_order(&market_order, None).unwrap();
        wait_until_async(
            || async {
                let order = database
                    .load_order(&market_order.client_order_id())
                    .await
                    .unwrap();
                order.is_some()
            },
            Duration::from_secs(3),
        )
        .await;

        // Load orders and build indexes
        cache.cache_orders().await.unwrap();
        cache.build_index();

        let cached_order_ids = cache.client_order_ids(None, None, None, None);
        assert_eq!(cached_order_ids.len(), 1);
        let target_order = cache.order(&market_order.client_order_id());
        assert_eq!(&*target_order.unwrap(), &market_order);

        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_restart_recovery_restores_order_indexes() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_test_pg_cache_database().await.unwrap())));

        let instrument = currency_pair_ethusdt();
        let client_id = ClientId::new("TEST");
        let position_id = PositionId::new("P-19700101-0000-001-001-1");
        let order_1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-0000-001-001-1"))
            .build();
        let order_2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-0000-001-001-2"))
            .build();

        // Add foreign key dependencies: instrument and currencies
        database
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        database.add_currency(&instrument.quote_currency()).unwrap();
        database
            .add_instrument(&InstrumentAny::CurrencyPair(instrument))
            .unwrap();

        // Insert into database and wait
        database.add_order(&order_1, Some(client_id)).unwrap();
        database.add_order(&order_2, None).unwrap();
        database
            .index_order_position(order_1.client_order_id(), position_id)
            .unwrap();
        wait_until_async(
            || async {
                database
                    .load_order(&order_1.client_order_id())
                    .await
                    .unwrap()
                    .is_some()
                    && database
                        .load_order(&order_2.client_order_id())
                        .await
                        .unwrap()
                        .is_some()
                    && !database.load_index_order_position().unwrap().is_empty()
            },
            Duration::from_secs(3),
        )
        .await;

        // Load orders and indexes into a fresh cache (restart simulation)
        cache.cache_orders().await.unwrap();
        cache.build_index();

        assert_eq!(
            cache.position_id(&order_1.client_order_id()),
            Some(&position_id)
        );
        assert_eq!(
            cache.client_id(&order_1.client_order_id()),
            Some(&client_id)
        );
        assert!(cache.position_id(&order_2.client_order_id()).is_none());
        assert!(cache.client_id(&order_2.client_order_id()).is_none());

        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_restart_recovery_restores_positions() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_test_pg_cache_database().await.unwrap())));

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        database
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        database.add_currency(&instrument.quote_currency()).unwrap();
        database.add_instrument(&instrument).unwrap();

        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-CACHE-POSITION-001"))
            .build();
        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-CACHE-POSITION-002"))
            .build();
        let position_id = PositionId::new("P-PG-CACHE-POSITION");

        let OrderEventAny::Filled(open_fill) = TestOrderEventStubs::filled(
            &open_order,
            &instrument,
            Some(TradeId::new("E-PG-CACHE-POSITION-001")),
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
        let mut position = Position::new(&instrument, open_fill);
        database.add_position(&position).unwrap();

        let OrderEventAny::Filled(close_fill) = TestOrderEventStubs::filled(
            &close_order,
            &instrument,
            Some(TradeId::new("E-PG-CACHE-POSITION-002")),
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
        database.update_position(&position).unwrap();

        wait_until_async(
            || async {
                database
                    .load_position(&position.id)
                    .await
                    .unwrap()
                    .is_some_and(|loaded| loaded.events == position.events)
            },
            Duration::from_secs(3),
        )
        .await;

        cache.cache_positions().await.unwrap();
        cache.build_index();

        let cached_position = cache.position(&position.id).unwrap();
        assert_eq!(
            cached_position.events.as_slice(),
            position.events.as_slice()
        );
        assert_eq!(cached_position.quantity, position.quantity);

        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_instrument_close_replacement_survives_restart() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        database.flush().unwrap();
        let instrument_id = InstrumentId::from("BINARY-1.POLYMARKET");
        let close = InstrumentClose::new(
            instrument_id,
            Price::from("1.00000"),
            InstrumentCloseType::ContractExpired,
            UnixNanos::from(10),
            UnixNanos::from(11),
        );
        let replacement = InstrumentClose::new(
            close.instrument_id,
            Price::from("0.00000"),
            InstrumentCloseType::EndOfSession,
            UnixNanos::from(20),
            UnixNanos::from(21),
        );
        let mut cache = get_cache(Some(Box::new(get_test_pg_cache_database().await.unwrap())));

        cache.add_instrument_close(close).unwrap();
        cache.add_instrument_close(replacement).unwrap();
        assert_eq!(
            cache.instrument_close(&close.instrument_id),
            Some(&replacement)
        );
        cache.dispose();

        let mut restarted_cache =
            get_cache(Some(Box::new(get_test_pg_cache_database().await.unwrap())));
        restarted_cache.cache_all().await.unwrap();

        assert_eq!(
            restarted_cache.instrument_close(&close.instrument_id),
            Some(&replacement)
        );

        restarted_cache.dispose();
        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_accounts() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_test_pg_cache_database().await.unwrap())));

        let account = AccountAny::default();
        let last_event = account.last_event().unwrap();
        if let Some(base_currency) = &last_event.base_currency {
            database.add_currency(base_currency).unwrap();
        }

        // Insert into database and wait
        database.add_account(&account).unwrap();
        wait_until_async(
            || async {
                let account = database.load_account(&account.id()).await.unwrap();
                account.is_some()
            },
            Duration::from_secs(3),
        )
        .await;

        // Load accounts and build indexes
        cache.cache_accounts().await.unwrap();
        cache.build_index();

        let cached_accounts = cache.accounts(&account.id());
        assert_eq!(cached_accounts.len(), 1);
        let target_account_for_venue = cache.account_for_venue(&account.id().get_issuer());
        assert_eq!(*target_account_for_venue.unwrap(), account);

        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_all_and_unsupported_loads_return_results() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        database.flush().unwrap();

        let loaded = database.load_all().await.unwrap();
        let synthetic_result = database
            .load_synthetic(&InstrumentId::from("SYNTHETIC.SYNTH"))
            .await;
        let actor_result = database.load_actor(&ActorId::from("ACTOR-001"));
        let strategy_result = database.load_strategy(&StrategyId::from("STRATEGY-001"));
        let state = AHashMap::from([("state".to_string(), Bytes::from_static(b"value"))]);
        let actor_update_result = database.update_actor(&ActorId::from("ACTOR-001"), &state);
        let strategy_update_result =
            database.update_strategy(&StrategyId::from("STRATEGY-001"), &state);

        assert!(loaded.synthetics.is_empty());
        assert!(synthetic_result.is_err());
        assert_eq!(
            actor_result.unwrap_err().to_string(),
            "load_actor not implemented for PostgreSQL cache adapter: ACTOR-001"
        );
        assert_eq!(
            strategy_result.unwrap_err().to_string(),
            "load_strategy not implemented for PostgreSQL cache adapter: STRATEGY-001"
        );
        assert_eq!(
            actor_update_result.unwrap_err().to_string(),
            "update_actor not implemented for PostgreSQL cache adapter: ACTOR-001"
        );
        assert_eq!(
            strategy_update_result.unwrap_err().to_string(),
            "update_strategy not implemented for PostgreSQL cache adapter: STRATEGY-001"
        );

        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_all_registers_persisted_currencies_before_instruments() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        database.flush().unwrap();

        let currency = Currency::new(
            "DBTEST",
            7,
            0,
            "Database test currency",
            CurrencyType::Crypto,
        );
        let mut currency_pair = currency_pair_ethusdt();
        currency_pair.base_currency = currency;
        let instrument = InstrumentAny::CurrencyPair(currency_pair);

        assert_eq!(Currency::try_from_str(currency.code.as_str()), None);

        database.add_currency(&currency).unwrap();
        database.add_currency(&instrument.quote_currency()).unwrap();
        database.add_instrument(&instrument).unwrap();
        database.close().unwrap();

        let mut database = get_test_pg_cache_database().await.unwrap();
        let loaded = database.load_all().await.unwrap();

        let registered_currency = Currency::try_from_str(currency.code.as_str()).unwrap();
        let loaded_currency = *loaded.currencies.get(&currency.code).unwrap();
        let loaded_instrument = loaded.instruments.get(&instrument.id()).unwrap();
        let loaded_base_currency = loaded_instrument.base_currency().unwrap();

        for actual in [registered_currency, loaded_currency, loaded_base_currency] {
            assert_eq!(actual.code, currency.code);
            assert_eq!(actual.precision, currency.precision);
            assert_eq!(actual.iso4217, currency.iso4217);
            assert_eq!(actual.name, currency.name);
            assert_eq!(actual.currency_type, currency.currency_type);
        }

        assert_eq!(
            serde_json::to_string(loaded_instrument).unwrap(),
            serde_json::to_string(&instrument).unwrap()
        );

        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_failed_async_position_load_returns_error() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        database.pool.close().await;

        let result = database.load_positions().await;

        assert!(result.is_err());

        database.close().unwrap();
    }

    // Test inserting and loading OrderCancelRejected events from PostgreSQL.
    //
    // This test verifies that order cancel rejection events can be persisted to and
    // retrieved from the PostgreSQL cache.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_order_cancel_rejected_insert_and_load() {
        let mut db = get_test_pg_cache_database().await.expect("connect db");
        let instrument_id = seed_order_event_dependencies(&db).await;
        let pool = db.pool.clone();

        let client_id_str = UUID4::new().to_string();
        let client_order_id = ClientOrderId::from(client_id_str.as_str());

        let strategy_id = StrategyId::from("S-1");
        let reason = Ustr::from("TEST_REJECT");
        let venue_order_id = Some(VenueOrderId::from("V1"));
        let account_id = Some(AccountId::from("A-1"));

        let event = OrderCancelRejectedSpec::builder()
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .reason(reason)
            .maybe_venue_order_id(venue_order_id)
            .maybe_account_id(account_id)
            .build();

        // Insert into database
        DatabaseQueries::add_order_event(&pool, Box::new(event), None)
            .await
            .unwrap();

        // Load back events
        let events = DatabaseQueries::load_order_events(&pool, &client_order_id)
            .await
            .unwrap();

        delete_order_events(&pool, &client_order_id).await;

        assert_eq!(events.len(), 1);
        match &events[0] {
            OrderEventAny::CancelRejected(e) => {
                assert_eq!(e.client_order_id, client_order_id);
                assert_eq!(e.reason, reason);
            }
            other => panic!("Expected OrderCancelRejected, was {other:?}"),
        }

        db.flush().unwrap();
        db.close().unwrap();
    }

    // Test inserting and loading OrderModifyRejected events from PostgreSQL.
    //
    // This test verifies that order modification rejection events can be persisted to and
    // retrieved from the PostgreSQL cache.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_order_modify_rejected_insert_and_load() {
        let mut db = get_test_pg_cache_database().await.expect("connect db");
        let instrument_id = seed_order_event_dependencies(&db).await;
        let pool = db.pool.clone();

        let client_id_str = UUID4::new().to_string();
        let client_order_id = ClientOrderId::from(client_id_str.as_str());

        let trader_id = TraderId::from("TRADER-002");
        let strategy_id = StrategyId::from("S-2");
        let reason = Ustr::from("TEST_MOD_REJECT");
        let venue_order_id = Some(VenueOrderId::from("V2"));
        let account_id = Some(AccountId::from("A-2"));

        let event = OrderModifyRejectedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .reason(reason)
            .reconciliation(true)
            .maybe_venue_order_id(venue_order_id)
            .maybe_account_id(account_id)
            .build();

        DatabaseQueries::add_order_event(&pool, Box::new(event), None)
            .await
            .unwrap();

        let events = DatabaseQueries::load_order_events(&pool, &client_order_id)
            .await
            .unwrap();

        delete_order_events(&pool, &client_order_id).await;

        assert_eq!(events.len(), 1);
        match &events[0] {
            OrderEventAny::ModifyRejected(e) => {
                assert_eq!(e.client_order_id, client_order_id);
                assert_eq!(e.reason, reason);
            }
            other => panic!("Expected OrderModifyRejected, was {other:?}"),
        }

        db.flush().unwrap();
        db.close().unwrap();
    }

    /// Seeds the foreign-key dependencies that `order_event` rows require.
    async fn seed_order_event_dependencies(database: &PostgresCacheDatabase) -> InstrumentId {
        let instrument = currency_pair_ethusdt();
        let instrument_id = instrument.id();
        database
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        database.add_currency(&instrument.quote_currency()).unwrap();
        database
            .add_instrument(&InstrumentAny::CurrencyPair(instrument))
            .unwrap();

        // Writes go through the database channel, so wait for the foreign-key row to land
        let pool = database.pool.clone();
        wait_until_async(
            || async {
                DatabaseQueries::load_instrument(&pool, &instrument_id)
                    .await
                    .is_ok_and(|instrument| instrument.is_some())
            },
            Duration::from_secs(3),
        )
        .await;

        instrument_id
    }

    /// Removes the event rows for `client_order_id`.
    ///
    /// These tests persist a single event without its `OrderInitialized`, which no order replay
    /// can assemble. Leaving the rows behind would break every later test that loads all orders.
    async fn delete_order_events(pool: &PgPool, client_order_id: &ClientOrderId) {
        sqlx::query(r#"DELETE FROM "order_event" WHERE client_order_id = $1"#)
            .bind(client_order_id.to_string())
            .execute(pool)
            .await
            .unwrap();
    }

    async fn assert_event_round_trip(pool: &PgPool, event: &OrderEventAny) {
        let client_order_id = event.client_order_id();
        DatabaseQueries::add_order_event(pool, event.clone().into_boxed(), None)
            .await
            .unwrap();

        let loaded = DatabaseQueries::load_order_events(pool, &client_order_id)
            .await
            .unwrap();

        delete_order_events(pool, &client_order_id).await;

        assert_eq!(loaded.len(), 1, "expected one event for {client_order_id}");
        assert_eq!(
            loaded[0], *event,
            "round trip changed the event for {client_order_id}"
        );
    }

    /// Every `OrderEventAny` variant survives a Postgres round trip unchanged.
    ///
    /// Regression coverage for the `todo!()` deserializers that panicked the engine on cache load
    /// whenever a persisted order event was one of the ten unimplemented kinds.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_every_order_event_kind_round_trips() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let instrument_id = seed_order_event_dependencies(&database).await;
        let pool = database.pool.clone();

        let trader_id = TraderId::from("TRADER-007");
        let strategy_id = StrategyId::from("S-042");
        let venue_order_id = VenueOrderId::from("V-4242");
        let account_id = AccountId::from("SIM-007");
        let currency = Currency::from("USDT");
        let causation_id = UUID4::new();

        // Distinct, non-default values so a dropped or swapped field fails the equality assert
        let unique =
            |suffix: &str| ClientOrderId::from(format!("O-{}-{suffix}", UUID4::new()).as_str());

        let mut exec_algorithm_params = IndexMap::new();
        exec_algorithm_params.insert(Ustr::from("horizon"), Ustr::from("30"));

        let mut initialized = OrderInitializedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("initialized"))
            .order_side(OrderSide::Buy)
            .order_type(OrderType::Limit)
            .quantity(Quantity::from("3.5"))
            .time_in_force(TimeInForce::Gtc)
            .post_only(true)
            .reduce_only(true)
            .quote_quantity(true)
            .reconciliation(true)
            .price(Price::from("1500.10"))
            .exec_algorithm_id(ExecAlgorithmId::from("TWAP"))
            .exec_algorithm_params(exec_algorithm_params)
            .exec_spawn_id(ClientOrderId::from("O-SPAWN-1"))
            .tags(vec![Ustr::from("tag-1"), Ustr::from("tag-2")])
            .build();
        initialized.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Initialized(initialized)).await;

        let mut canceled = OrderCanceledSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("canceled"))
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .ts_event(UnixNanos::from(111_222_333_444_555_666_u64))
            .ts_init(UnixNanos::from(777_888_999_111_222_333_u64))
            .build();
        canceled.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Canceled(canceled)).await;

        let mut denied = OrderDeniedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("denied"))
            .reason(Ustr::from("RISK_LIMIT"))
            .ts_event(UnixNanos::from(11_u64))
            .ts_init(UnixNanos::from(22_u64))
            .build();
        denied.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Denied(denied)).await;

        let mut emulated = OrderEmulatedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("emulated"))
            .ts_event(UnixNanos::from(33_u64))
            .ts_init(UnixNanos::from(44_u64))
            .build();
        emulated.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Emulated(emulated)).await;

        let mut expired = OrderExpiredSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("expired"))
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .build();
        expired.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Expired(expired)).await;

        let mut pending_cancel = OrderPendingCancelSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("pending-cancel"))
            .account_id(account_id)
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .build();
        pending_cancel.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::PendingCancel(pending_cancel)).await;

        let mut pending_update = OrderPendingUpdateSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("pending-update"))
            .account_id(account_id)
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .build();
        pending_update.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::PendingUpdate(pending_update)).await;

        let mut rejected = OrderRejectedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("rejected"))
            .account_id(account_id)
            .reason(Ustr::from("DUPLICATE_LINK_ID"))
            .reconciliation(true)
            .due_post_only(true)
            .build();
        rejected.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Rejected(rejected)).await;

        let mut released = OrderReleasedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("released"))
            .released_price(Price::from("1234.56"))
            .build();
        released.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Released(released)).await;

        let mut triggered = OrderTriggeredSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("triggered"))
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .build();
        triggered.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Triggered(triggered)).await;

        let mut updated = OrderUpdatedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("updated"))
            .quantity(Quantity::from("7.5"))
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .price(Price::from("1500.10"))
            .trigger_price(Price::from("1499.90"))
            .protection_price(Price::from("1450.00"))
            .is_quote_quantity(true)
            .build();
        updated.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Updated(updated)).await;

        let mut accepted = OrderAcceptedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("accepted"))
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .reconciliation(true)
            .build();
        accepted.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Accepted(accepted)).await;

        let mut submitted = OrderSubmittedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("submitted"))
            .account_id(account_id)
            .build();
        submitted.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Submitted(submitted)).await;

        let mut cancel_rejected = OrderCancelRejectedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("cancel-rejected"))
            .reason(Ustr::from("UNKNOWN_ORDER"))
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .build();
        cancel_rejected.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::CancelRejected(cancel_rejected)).await;

        let mut modify_rejected = OrderModifyRejectedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("modify-rejected"))
            .reason(Ustr::from("PRICE_INVALID"))
            .reconciliation(true)
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .build();
        modify_rejected.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::ModifyRejected(modify_rejected)).await;

        let mut info = IndexMap::new();
        info.insert(Ustr::from("venue_note"), Ustr::from("partial"));

        let mut filled = OrderFilledSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("filled"))
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .trade_id(TradeId::from("T-991"))
            .order_side(OrderSide::Buy)
            .order_type(OrderType::Limit)
            .last_qty(Quantity::from("2.5"))
            .last_px(Price::from("1501.25"))
            .currency(currency)
            .liquidity_side(LiquiditySide::Maker)
            .reconciliation(true)
            .position_id(PositionId::from("P-77"))
            .commission(Money::new(0.15, currency))
            .info(info.clone())
            .build();
        filled.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::Filled(filled)).await;

        let mut fill_voided = OrderFillVoidedSpec::builder()
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(unique("fill-voided"))
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .correction_id(Ustr::from("CORR-1"))
            .trade_id(TradeId::from("T-992"))
            .voided_qty(Quantity::from("1.5"))
            .commission_voided(Money::new(0.05, currency))
            .order_side(OrderSide::Sell)
            .order_type(OrderType::Market)
            .last_px(Price::from("1499.00"))
            .currency(currency)
            .liquidity_side(LiquiditySide::Taker)
            .position_id(PositionId::from("P-78"))
            .reason(Ustr::from("VENUE_CORRECTION"))
            .info(info)
            .reconciliation(true)
            .is_reopened(true)
            .build();
        fill_voided.causation_id = Some(causation_id);
        assert_event_round_trip(&pool, &OrderEventAny::FillVoided(fill_voided)).await;

        database.flush().unwrap();
        database.close().unwrap();
    }

    /// A fill persisted to `position_event` keeps the fields that are not part of the position.
    ///
    /// `position_event` is decoded by the same row mapping as `order_event`, so both tables have to
    /// carry every `OrderFilled` field. The restart-recovery tests use stub fills whose
    /// `reconciliation`, `info`, and `causation_id` are already at their defaults, so only distinct
    /// values here can prove those columns are written and read.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_position_event_round_trip_keeps_non_position_fill_fields() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let instrument_id = seed_order_event_dependencies(&database).await;
        let pool = database.pool.clone();

        let position_id = PositionId::from("P-NON-POSITION-FIELDS");
        let currency = Currency::from("USDT");
        let mut info = IndexMap::new();
        info.insert(Ustr::from("venue_note"), Ustr::from("corrected"));

        let mut fill = OrderFilledSpec::builder()
            .trader_id(TraderId::from("TRADER-009"))
            .strategy_id(StrategyId::from("S-009"))
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-POSITION-FIELDS-1"))
            .venue_order_id(VenueOrderId::from("V-909"))
            .account_id(AccountId::from("SIM-009"))
            .trade_id(TradeId::from("T-909"))
            .order_side(OrderSide::Buy)
            .order_type(OrderType::Market)
            .last_qty(Quantity::from("1.0"))
            .last_px(Price::from("1600.00"))
            .currency(currency)
            .liquidity_side(LiquiditySide::Taker)
            .reconciliation(true)
            .position_id(position_id)
            .commission(Money::new(0.25, currency))
            .info(info)
            .build();
        fill.causation_id = Some(UUID4::new());

        DatabaseQueries::add_position(&pool, position_id, &fill)
            .await
            .unwrap();

        let loaded = DatabaseQueries::load_position_events(&pool, &position_id)
            .await
            .unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].reconciliation, fill.reconciliation);
        assert_eq!(loaded[0].info, fill.info);
        assert_eq!(loaded[0].causation_id, fill.causation_id);
        assert_eq!(loaded[0], fill);

        database.flush().unwrap();
        database.close().unwrap();
    }

    /// An event row written before the new columns existed still decodes.
    ///
    /// `due_post_only` and `is_reopened` were added by `ALTER TABLE`, so a database that predates
    /// them can present NULL. The column default carries the same meaning as a missing value, so
    /// decoding falls back to `false` rather than failing the whole cache load.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_order_event_row_with_null_new_columns_decodes() {
        let mut database = get_test_pg_cache_database().await.unwrap();
        let instrument_id = seed_order_event_dependencies(&database).await;
        let pool = database.pool.clone();

        let client_order_id = ClientOrderId::from("O-LEGACY-NULL-COLUMNS-1");
        delete_order_events(&pool, &client_order_id).await;

        sqlx::query(r#"INSERT INTO "trader" (id) VALUES ($1) ON CONFLICT (id) DO NOTHING"#)
            .bind("TRADER-LEGACY")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            r#"INSERT INTO "order_event"
               (id, kind, trader_id, strategy_id, instrument_id, client_order_id, reason,
                account_id, reconciliation, ts_event, ts_init, due_post_only)
               VALUES ($1, 'OrderRejected', 'TRADER-LEGACY', 'S-LEGACY', $2, $3, 'LEGACY_REASON',
                       'SIM-LEGACY', true, '1', '2', NULL)"#,
        )
        .bind(UUID4::new().to_string())
        .bind(instrument_id.to_string())
        .bind(client_order_id.to_string())
        .execute(&pool)
        .await
        .unwrap();

        let loaded = DatabaseQueries::load_order_events(&pool, &client_order_id)
            .await
            .unwrap();

        delete_order_events(&pool, &client_order_id).await;

        assert_eq!(loaded.len(), 1);
        match &loaded[0] {
            OrderEventAny::Rejected(event) => {
                assert!(!event.due_post_only);
                assert!(event.reconciliation);
                assert_eq!(event.reason, Ustr::from("LEGACY_REASON"));
                assert_eq!(event.causation_id, None);
            }
            other => panic!("Expected OrderRejected, was {other:?}"),
        }

        database.flush().unwrap();
        database.close().unwrap();
    }

    /// Tests that data is flushed immediately with the current hardcoded `buffer_interval=0`.
    /// When `buffer_interval` is exposed via config, this test validates the zero-interval path.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_buffer_flushes_immediately() {
        let mut database = get_test_pg_cache_database().await.unwrap();

        let eth = Currency::new("ETH", 2, 0, "ETH", CurrencyType::Crypto);
        let eth_key = Ustr::from("ETH");

        database.add_currency(&eth).unwrap();

        wait_until_async(
            || async {
                let currencies = database.load_currencies().await.unwrap();
                currencies.contains_key(&eth_key)
            },
            Duration::from_secs(2),
        )
        .await;

        let currencies = database.load_currencies().await.unwrap();
        assert!(
            currencies.contains_key(&eth_key),
            "Currency should be flushed immediately"
        );

        database.flush().unwrap();
        database.close().unwrap();
    }

    /// Tests that pending buffered data is drained when close is called.
    /// With `buffer_interval=0` the buffer is typically empty, but this validates the code path.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_buffer_drains_on_close() {
        let mut database = get_test_pg_cache_database().await.unwrap();

        let usdt = Currency::new("USDT", 2, 0, "USDT", CurrencyType::Crypto);
        let usdt_key = Ustr::from("USDT");

        database.add_currency(&usdt).unwrap();
        database.close().unwrap();

        // Reconnect to verify data was persisted
        let mut database = get_test_pg_cache_database().await.unwrap();
        let currencies = database.load_currencies().await.unwrap();

        assert!(
            currencies.contains_key(&usdt_key),
            "Currency should be persisted after close"
        );

        database.flush().unwrap();
        database.close().unwrap();
    }
}
