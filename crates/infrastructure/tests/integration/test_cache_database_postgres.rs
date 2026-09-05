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
#[cfg(feature = "postgres")]
#[cfg(target_os = "linux")] // Databases only tested and supported on Linux
mod serial_tests {
    use std::{collections::HashSet, str::FromStr, time::Duration};

    use bytes::Bytes;
    use indexmap::indexmap;
    use nautilus_common::{
        cache::{Cache, database::CacheDatabaseAdapter},
        signal::Signal,
        testing::{wait_until, wait_until_async},
    };
    use nautilus_core::{Params, UnixNanos};
    use nautilus_infrastructure::sql::{
        cache::{PostgresCacheDatabase, get_pg_cache_database},
        pg::{connect_pg, get_postgres_connect_options, init_postgres},
        queries::DatabaseQueries,
    };
    use nautilus_model::{
        accounts::{AccountAny, CashAccount},
        data::{
            CustomData, DataType,
            stubs::{quote_ethusdt_binance, stub_bar, stub_trade_ethusdt_buy},
        },
        enums::{CurrencyType, OrderSide, OrderStatus, OrderType},
        events::{
            OrderEventAny, OrderFilled, OrderSnapshot,
            account::stubs::{
                cash_account_state_million_usd, wallet_account_state, wallet_account_state_changed,
            },
            order::spec::OrderFillVoidedSpec,
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId,
            stubs::account_id,
        },
        instruments::{
            Instrument, InstrumentAny,
            stubs::{
                audusd_sim, binary_option, crypto_future_btcusdt, crypto_perpetual_ethusdt,
                currency_pair_ethusdt, equity_aapl, futures_contract_es, option_contract_appl,
            },
        },
        orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        position::Position,
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };
    use nautilus_persistence::test_data::RustTestCustomData;
    use nautilus_serialization::ensure_custom_data_registered;
    use rust_decimal::Decimal;
    use serde::Serialize;
    use sqlx::{AssertSqlSafe, PgPool, postgres::PgConnectOptions};
    use ustr::Ustr;

    pub(crate) fn assert_entirely_equal<T: Serialize>(a: T, b: T) {
        let a_serialized = serde_json::to_string(&a).unwrap();
        let b_serialized = serde_json::to_string(&b).unwrap();

        assert_eq!(a_serialized, b_serialized);
    }

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

    async fn connect_test_pg(options: PgConnectOptions) -> anyhow::Result<PgPool> {
        match tokio::time::timeout(Duration::from_secs(2), connect_pg(options)).await {
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
    async fn test_add_general_object_adds_to_cache() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let test_id_value = Bytes::from("test_value");
        pg_cache
            .add(String::from("test_id"), test_id_value.clone())
            .unwrap();
        wait_until(
            || {
                let result = pg_cache.load().unwrap();
                result.keys().len() > 0
            },
            Duration::from_secs(5),
        );
        let result = pg_cache.load().unwrap();
        assert_eq!(result.keys().len(), 1);
        assert_eq!(
            result.keys().cloned().collect::<Vec<String>>(),
            vec![String::from("test_id")]
        );
        assert_eq!(result.get("test_id").unwrap().to_owned(), test_id_value);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[expect(
        clippy::similar_names,
        reason = "USDC and USDT are distinct currency symbols in this integration test"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "integration test inserts all supported instrument variants"
    )]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_currency_and_instruments() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        // Define currencies
        let btc = Currency::new("BTC", 8, 0, "BTC", CurrencyType::Crypto);
        let eth = Currency::new("ETH", 2, 0, "ETH", CurrencyType::Crypto);
        let gbp = Currency::new("GBP", 2, 0, "GBP", CurrencyType::Fiat);
        let usd = Currency::new("USD", 2, 0, "USD", CurrencyType::Fiat);
        let usdc = Currency::new("USDC", 8, 0, "USDC", CurrencyType::Crypto);
        let usdt = Currency::new("USDT", 2, 0, "USDT", CurrencyType::Crypto);

        // Insert all currencies
        pg_cache.add_currency(&btc).unwrap();
        pg_cache.add_currency(&eth).unwrap();
        pg_cache.add_currency(&gbp).unwrap();
        pg_cache.add_currency(&usd).unwrap();
        pg_cache.add_currency(&usdc).unwrap();
        pg_cache.add_currency(&usdt).unwrap();

        // Define all instruments
        let binary_option = binary_option();
        let crypto_future =
            crypto_future_btcusdt(2, 6, Price::from("0.01"), Quantity::from("0.000001"));
        let crypto_perpetual = crypto_perpetual_ethusdt();
        let currency_pair = currency_pair_ethusdt();
        let equity = equity_aapl();
        let futures_contract = futures_contract_es(None, None);
        let option_contract = option_contract_appl();

        // Insert all instruments
        pg_cache
            .add_instrument(&InstrumentAny::BinaryOption(binary_option.clone()))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CryptoFuture(crypto_future.clone()))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CryptoPerpetual(crypto_perpetual.clone()))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CurrencyPair(currency_pair.clone()))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::Equity(equity.clone()))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::FuturesContract(futures_contract.clone()))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::OptionContract(option_contract.clone()))
            .unwrap();

        // Wait for cache to update
        wait_until_async(
            || async {
                let currencies = pg_cache.load_currencies().await.unwrap();
                let instruments = pg_cache.load_instruments().await.unwrap();
                currencies.len() >= 6 && instruments.len() >= 7
            },
            Duration::from_secs(5),
        )
        .await;

        // Check currency list is correct
        let currencies = pg_cache.load_currencies().await.unwrap();
        assert_eq!(currencies.len(), 6);
        assert_eq!(
            currencies
                .into_values()
                .map(|c| c.code.to_string())
                .collect::<HashSet<String>>(),
            vec![
                String::from("BTC"),
                String::from("ETH"),
                String::from("GBP"),
                String::from("USD"),
                String::from("USDC"),
                String::from("USDT")
            ]
            .into_iter()
            .collect::<HashSet<String>>()
        );

        // Check individual currencies
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("BTC"))
                .await
                .unwrap()
                .unwrap(),
            btc
        );
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("ETH"))
                .await
                .unwrap()
                .unwrap(),
            eth
        );
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("GBP"))
                .await
                .unwrap()
                .unwrap(),
            gbp
        );
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("USD"))
                .await
                .unwrap()
                .unwrap(),
            usd
        );
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("USDC"))
                .await
                .unwrap()
                .unwrap(),
            usdc
        );
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("USDT"))
                .await
                .unwrap()
                .unwrap(),
            usdt
        );

        // Check individual instruments
        assert_eq!(
            pg_cache
                .load_instrument(&binary_option.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::BinaryOption(binary_option.clone())
        );
        assert_eq!(
            pg_cache
                .load_instrument(&crypto_future.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::CryptoFuture(crypto_future.clone())
        );
        assert_eq!(
            pg_cache
                .load_instrument(&crypto_perpetual.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::CryptoPerpetual(crypto_perpetual.clone())
        );
        assert_eq!(
            pg_cache
                .load_instrument(&currency_pair.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::CurrencyPair(currency_pair.clone())
        );
        assert_eq!(
            pg_cache
                .load_instrument(&equity.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::Equity(equity.clone())
        );
        assert_eq!(
            pg_cache
                .load_instrument(&futures_contract.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::FuturesContract(futures_contract.clone())
        );
        assert_eq!(
            pg_cache
                .load_instrument(&option_contract.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::OptionContract(option_contract.clone())
        );

        // Check instrument list is correct
        let instruments = pg_cache.load_instruments().await.unwrap();
        assert_eq!(instruments.len(), 7);
        assert_eq!(
            instruments.into_keys().collect::<HashSet<InstrumentId>>(),
            vec![
                binary_option.id(),
                crypto_future.id(),
                crypto_perpetual.id(),
                currency_pair.id(),
                equity.id(),
                futures_contract.id(),
                option_contract.id()
            ]
            .into_iter()
            .collect::<HashSet<InstrumentId>>()
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_truncate() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        // Add items in currency and instrument table
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();
        wait_until_async(
            || async {
                pg_cache.load_currencies().await.unwrap().len() == 2
                    && pg_cache.load_instruments().await.unwrap().len() == 1
            },
            Duration::from_secs(5),
        )
        .await;

        // Call flush which will truncate all the tables
        pg_cache.flush().unwrap();

        // Check if all the tables are empty
        let currencies = pg_cache.load_currencies().await.unwrap();
        assert_eq!(currencies.len(), 0);
        let instruments = pg_cache.load_instruments().await.unwrap();
        assert_eq!(instruments.len(), 0);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_order_and_load_indexes() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let client_order_id_1 = ClientOrderId::new("O-19700101-000000-001-001-1");
        let client_order_id_2 = ClientOrderId::new("O-19700101-000000-001-001-2");
        let instrument = currency_pair_ethusdt();

        let market_order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(client_order_id_1)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .exec_algorithm_params(indexmap! { Ustr::from("speed") => Ustr::from("fast") })
            .tags(vec![Ustr::from("tag-1"), Ustr::from("tag-2")])
            .build();
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id_2)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .price(Price::from("100.0"))
            .quantity(Quantity::from("1.0"))
            .build();

        // Add foreign key dependencies: instrument and currencies
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CurrencyPair(instrument))
            .unwrap();

        // Set client id
        let client_id = ClientId::new("TEST");

        // Add orders
        pg_cache.add_order(&market_order, Some(client_id)).unwrap();
        pg_cache.add_order(&limit_order, Some(client_id)).unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_order(&market_order.client_order_id())
                    .await
                    .unwrap()
                    .is_some()
                    && pg_cache
                        .load_order(&limit_order.client_order_id())
                        .await
                        .unwrap()
                        .is_some()
            },
            Duration::from_secs(5),
        )
        .await;
        let market_order_result = pg_cache
            .load_order(&market_order.client_order_id())
            .await
            .unwrap();
        let limit_order_result = pg_cache
            .load_order(&limit_order.client_order_id())
            .await
            .unwrap();
        let client_order_ids = pg_cache.load_index_order_client().unwrap();
        assert_entirely_equal(market_order_result.unwrap(), market_order);
        assert_entirely_equal(limit_order_result.unwrap(), limit_order);

        // Check event client order ids
        assert_eq!(client_order_ids.len(), 2);
        assert_eq!(
            client_order_ids
                .keys()
                .copied()
                .collect::<HashSet<ClientOrderId>>(),
            vec![client_order_id_1, client_order_id_2]
                .into_iter()
                .collect::<HashSet<ClientOrderId>>()
        );
        assert_eq!(
            client_order_ids
                .values()
                .copied()
                .collect::<HashSet<ClientId>>(),
            vec![client_id].into_iter().collect::<HashSet<ClientId>>()
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_index_order_clients_batch_survives_restart() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();
        let instrument = currency_pair_ethusdt();
        let client_a = ClientId::new("CLIENT-A");
        let client_b = ClientId::new("CLIENT-B");
        let order_1 = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-PG-ORIGIN-001"))
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();
        let order_2 = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-PG-ORIGIN-002"))
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .build();
        let claims = [
            (order_1.client_order_id(), client_a),
            (order_2.client_order_id(), client_b),
        ];

        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CurrencyPair(instrument))
            .unwrap();
        pg_cache.add_order(&order_1, None).unwrap();
        pg_cache.add_order(&order_2, None).unwrap();

        wait_until_async(
            || async {
                pg_cache
                    .load_order(&order_1.client_order_id())
                    .await
                    .unwrap()
                    .is_some()
                    && pg_cache
                        .load_order(&order_2.client_order_id())
                        .await
                        .unwrap()
                        .is_some()
            },
            Duration::from_secs(5),
        )
        .await;

        let missing_order_id = ClientOrderId::new("O-PG-ORIGIN-MISSING");
        let error = DatabaseQueries::index_order_clients(
            &pg_cache.pool,
            &[
                (order_1.client_order_id(), client_a),
                (missing_order_id, client_b),
            ],
        )
        .await
        .unwrap_err();
        let index_after_rollback = pg_cache.load_index_order_client().unwrap();

        assert!(error.to_string().contains("No persisted order events"));
        assert!(!index_after_rollback.contains_key(&order_1.client_order_id()));
        assert!(!index_after_rollback.contains_key(&missing_order_id));

        pg_cache.index_order_clients(&claims).unwrap();
        wait_until_async(
            || async {
                let index = pg_cache.load_index_order_client().unwrap();
                index.get(&order_1.client_order_id()) == Some(&client_a)
                    && index.get(&order_2.client_order_id()) == Some(&client_b)
            },
            Duration::from_secs(5),
        )
        .await;

        let restarted_adapter = get_test_pg_cache_database().await.unwrap();
        let mut cache = Cache::new(None, Some(Box::new(restarted_adapter)));
        cache.cache_orders().await.unwrap();
        cache.build_index();

        assert!(cache.order(&order_1.client_order_id()).is_some());
        assert!(cache.order(&order_2.client_order_id()).is_some());
        assert_eq!(cache.client_id(&order_1.client_order_id()), Some(&client_a));
        assert_eq!(cache.client_id(&order_2.client_order_id()), Some(&client_b));

        cache.dispose();
        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_index_order_clients_conflict_rolls_back_batch() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();
        let instrument = currency_pair_ethusdt();
        let existing_client = ClientId::new("CLIENT-EXISTING");
        let conflicting_client = ClientId::new("CLIENT-CONFLICTING");
        let unclaimed_order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-PG-ORIGIN-ROLLBACK-001"))
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();
        let claimed_order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-PG-ORIGIN-ROLLBACK-002"))
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .build();

        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CurrencyPair(instrument))
            .unwrap();
        pg_cache.add_order(&unclaimed_order, None).unwrap();
        pg_cache
            .add_order(&claimed_order, Some(existing_client))
            .unwrap();

        wait_until_async(
            || async {
                let index = pg_cache.load_index_order_client().unwrap();
                pg_cache
                    .load_order(&unclaimed_order.client_order_id())
                    .await
                    .unwrap()
                    .is_some()
                    && index.get(&claimed_order.client_order_id()) == Some(&existing_client)
            },
            Duration::from_secs(5),
        )
        .await;

        let error = DatabaseQueries::index_order_clients(
            &pg_cache.pool,
            &[
                (unclaimed_order.client_order_id(), conflicting_client),
                (claimed_order.client_order_id(), conflicting_client),
            ],
        )
        .await
        .unwrap_err();
        let index_after_rollback = pg_cache.load_index_order_client().unwrap();

        assert!(
            error
                .to_string()
                .contains("already claimed by execution client")
        );
        assert!(!index_after_rollback.contains_key(&unclaimed_order.client_order_id()));
        assert_eq!(
            index_after_rollback.get(&claimed_order.client_order_id()),
            Some(&existing_client)
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_index_order_position_round_trip() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let client_order_id = ClientOrderId::new("O-19700101-000000-001-001-1");
        let position_id_1 = PositionId::new("P-19700101-000000-001-001-1");
        let position_id_2 = PositionId::new("P-19700101-000000-001-001-2");

        pg_cache
            .index_order_position(client_order_id, position_id_1)
            .unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_index_order_position()
                    .unwrap()
                    .get(&client_order_id)
                    == Some(&position_id_1)
            },
            Duration::from_secs(5),
        )
        .await;

        // Re-indexing the same order updates the mapping
        pg_cache
            .index_order_position(client_order_id, position_id_2)
            .unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_index_order_position()
                    .unwrap()
                    .get(&client_order_id)
                    == Some(&position_id_2)
            },
            Duration::from_secs(5),
        )
        .await;

        let index = pg_cache.load_index_order_position().unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index.get(&client_order_id), Some(&position_id_2));

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_and_update_position_round_trip() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-POSITION-001"))
            .build();
        let increase_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-POSITION-002"))
            .build();
        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("2.0"))
            .client_order_id(ClientOrderId::new("O-PG-POSITION-003"))
            .build();
        let position_id = PositionId::new("P-PG-POSITION-ROUND-TRIP");

        let OrderEventAny::Filled(open_fill) = TestOrderEventStubs::filled(
            &open_order,
            &instrument,
            Some(TradeId::new("E-PG-POSITION-001")),
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
        pg_cache.add_position(&position).unwrap();

        let OrderEventAny::Filled(increase_fill) = TestOrderEventStubs::filled(
            &increase_order,
            &instrument,
            Some(TradeId::new("E-PG-POSITION-002")),
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
        position.apply(&increase_fill);
        pg_cache.update_position(&position).unwrap();

        let OrderEventAny::Filled(close_fill) = TestOrderEventStubs::filled(
            &close_order,
            &instrument,
            Some(TradeId::new("E-PG-POSITION-003")),
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
        pg_cache.update_position(&position).unwrap();

        wait_until_async(
            || async {
                pg_cache
                    .load_position(&position.id)
                    .await
                    .unwrap()
                    .is_some_and(|loaded| loaded.events == position.events)
                    && pg_cache.load_positions().await.unwrap().len() == 1
                    && DatabaseQueries::load_position_events(&pg_cache.pool, &position.id)
                        .await
                        .unwrap()
                        .len()
                        == 3
            },
            Duration::from_secs(5),
        )
        .await;

        let loaded = pg_cache.load_position(&position.id).await.unwrap().unwrap();
        let events = DatabaseQueries::load_position_events(&pg_cache.pool, &position.id)
            .await
            .unwrap();

        assert_entirely_equal(loaded, position.clone());
        assert_eq!(events, position.events.clone());

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_position_replaces_event_log_for_reused_position_id() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-NETTING-001"))
            .build();
        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-NETTING-002"))
            .build();
        let reopen_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-NETTING-003"))
            .build();
        let position_id = PositionId::new("P-PG-NETTING-REUSED");

        let OrderEventAny::Filled(open_fill) = TestOrderEventStubs::filled(
            &open_order,
            &instrument,
            Some(TradeId::new("E-PG-NETTING-001")),
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
        pg_cache.add_position(&closed_position).unwrap();

        let OrderEventAny::Filled(close_fill) = TestOrderEventStubs::filled(
            &close_order,
            &instrument,
            Some(TradeId::new("E-PG-NETTING-002")),
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
        pg_cache.update_position(&closed_position).unwrap();

        let OrderEventAny::Filled(reopen_fill) = TestOrderEventStubs::filled(
            &reopen_order,
            &instrument,
            Some(TradeId::new("E-PG-NETTING-003")),
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
        let reopened_position = Position::new(&instrument, reopen_fill.clone());
        pg_cache.add_position(&reopened_position).unwrap();

        wait_until_async(
            || async {
                let events =
                    DatabaseQueries::load_position_events(&pg_cache.pool, &reopened_position.id)
                        .await
                        .unwrap();
                events.len() == 1 && events[0].event_id == reopen_fill.event_id
            },
            Duration::from_secs(5),
        )
        .await;

        let events = DatabaseQueries::load_position_events(&pg_cache.pool, &reopened_position.id)
            .await
            .unwrap();

        assert_eq!(events, reopened_position.events.clone());

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_position_duplicate_fill_returns_error() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        wait_until_async(
            || async {
                pg_cache
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
            .client_order_id(ClientOrderId::new("O-PG-DUPLICATE-FILL"))
            .build();
        let position_id = PositionId::new("P-PG-DUPLICATE-FILL");
        let OrderEventAny::Filled(fill) = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::new("E-PG-DUPLICATE-FILL")),
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

        DatabaseQueries::add_position_event(&pg_cache.pool, &fill)
            .await
            .unwrap();
        DatabaseQueries::add_position_event(&pg_cache.pool, &fill)
            .await
            .unwrap();

        let events: Vec<OrderFilled> =
            DatabaseQueries::load_position_events(&pg_cache.pool, &position_id)
                .await
                .unwrap();
        let result = pg_cache.load_position(&position_id).await;

        assert_eq!(events.len(), 2);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("E-PG-DUPLICATE-FILL")
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_position_event_without_position_id_returns_error() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-MISSING-POSITION-ID"))
            .build();
        let OrderEventAny::Filled(mut fill) = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::new("E-PG-MISSING-POSITION-ID")),
            Some(PositionId::new("P-PG-MISSING-POSITION-ID")),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        fill.position_id = None;

        let result = DatabaseQueries::add_position_event(&pg_cache.pool, &fill).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no position_id"));

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_positions_skips_duplicate_fill_position() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        wait_until_async(
            || async {
                pg_cache
                    .load_instrument(&instrument.id())
                    .await
                    .unwrap()
                    .is_some()
            },
            Duration::from_secs(5),
        )
        .await;

        let good_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-GOOD-POSITION"))
            .build();
        let corrupt_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-PG-CORRUPT-POSITION"))
            .build();
        let good_position_id = PositionId::new("P-PG-GOOD-POSITION");
        let corrupt_position_id = PositionId::new("P-PG-CORRUPT-POSITION");

        let OrderEventAny::Filled(good_fill) = TestOrderEventStubs::filled(
            &good_order,
            &instrument,
            Some(TradeId::new("E-PG-GOOD-POSITION")),
            Some(good_position_id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };
        let good_position = Position::new(&instrument, good_fill.clone());

        let OrderEventAny::Filled(corrupt_fill) = TestOrderEventStubs::filled(
            &corrupt_order,
            &instrument,
            Some(TradeId::new("E-PG-CORRUPT-POSITION")),
            Some(corrupt_position_id),
            None,
            None,
            None,
            None,
            None,
            None,
        ) else {
            unreachable!();
        };

        DatabaseQueries::add_position_event(&pg_cache.pool, &good_fill)
            .await
            .unwrap();
        DatabaseQueries::add_position_event(&pg_cache.pool, &corrupt_fill)
            .await
            .unwrap();
        DatabaseQueries::add_position_event(&pg_cache.pool, &corrupt_fill)
            .await
            .unwrap();

        let positions = DatabaseQueries::load_positions(&pg_cache.pool)
            .await
            .unwrap();

        assert_eq!(positions.len(), 1);
        assert_eq!(
            positions[0].events.as_slice(),
            good_position.events.as_slice()
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_update_order_for_open_order() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let client_order_id_1 = ClientOrderId::new("O-19700101-000000-001-002-1");
        let instrument = InstrumentAny::CurrencyPair(currency_pair_ethusdt());
        let account = account_id();

        // Add foreign key dependencies: instrument and currencies
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        let mut market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(client_order_id_1)
            .build();

        pg_cache.add_order(&market_order, None).unwrap();

        let submitted = TestOrderEventStubs::submitted(&market_order, account);
        market_order.apply(submitted).unwrap();

        pg_cache.update_order(market_order.last_event()).unwrap();

        let accepted =
            TestOrderEventStubs::accepted(&market_order, account, VenueOrderId::new("001"));
        market_order.apply(accepted).unwrap();

        pg_cache.update_order(market_order.last_event()).unwrap();

        let filled = TestOrderEventStubs::filled(
            &market_order,
            &instrument,
            Some(TradeId::new("T-19700101-000000-001-001-1")),
            None,
            Some(Price::from("100.0")),
            Some(Quantity::from("1.0")),
            None,
            None,
            None,
            Some(AccountId::new("SIM-001")),
        );
        market_order.apply(filled).unwrap();

        pg_cache.update_order(market_order.last_event()).unwrap();
        wait_until_async(
            || async {
                let result = pg_cache
                    .load_order(&market_order.client_order_id())
                    .await
                    .unwrap();
                result.is_some() && result.unwrap().status() == OrderStatus::Filled
            },
            Duration::from_secs(5),
        )
        .await;

        let market_order_result = pg_cache
            .load_order(&market_order.client_order_id())
            .await
            .unwrap();
        assert_entirely_equal(market_order_result.unwrap(), market_order);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_and_update_account() {
        let pg_cache = get_test_pg_cache_database().await.unwrap();

        let mut account = AccountAny::Cash(CashAccount::new(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
            false,
            false,
        ));
        let last_event = account.last_event().unwrap();
        if let Some(base_currency) = &last_event.base_currency {
            pg_cache.add_currency(base_currency).unwrap();
        }
        pg_cache.add_account(&account).unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_account(&account.id())
                    .await
                    .unwrap()
                    .is_some()
            },
            Duration::from_secs(5),
        )
        .await;
        let account_result = pg_cache.load_account(&account.id()).await.unwrap();
        assert_entirely_equal(account_result.unwrap(), account.clone());

        // Update account
        let new_account_state_event =
            cash_account_state_million_usd("1000000 USD", "100000 USD", "900000 USD");
        account.apply(new_account_state_event).unwrap();
        pg_cache.update_account(&account).unwrap();
        wait_until_async(
            || async {
                let result = pg_cache.load_account(&account.id()).await.unwrap();
                result.is_some() && result.unwrap().events().len() >= 2
            },
            Duration::from_secs(5),
        )
        .await;
        let account_result = pg_cache.load_account(&account.id()).await.unwrap();
        assert_entirely_equal(account_result.unwrap(), account);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_and_update_wallet_account() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        // Distinct account ID and flush: these tests share one database
        pg_cache.flush().unwrap();

        let mut init_state = wallet_account_state();
        init_state.account_id = AccountId::from("WALLET-001");
        let token = Currency::new(
            "ENG729P",
            6,
            0,
            "Postgres wallet token",
            CurrencyType::Crypto,
        );
        let total = Money::from_mantissa_exponent(123_456_789, -6, token);
        init_state.balances[1] = AccountBalance::new(total, Money::zero(token), total);
        assert!(Currency::try_from_str("ENG729P").is_none());
        let mut account = AccountAny::try_from_state(init_state).unwrap();
        pg_cache.add_account(&account).unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_account(&account.id())
                    .await
                    .unwrap()
                    .is_some()
            },
            Duration::from_secs(5),
        )
        .await;
        let account_result = pg_cache.load_account(&account.id()).await.unwrap();
        let loaded = account_result.unwrap();
        assert!(matches!(loaded, AccountAny::Wallet(_)));
        assert_entirely_equal(loaded, account.clone());

        // Update account
        let mut changed_state = wallet_account_state_changed();
        changed_state.account_id = AccountId::from("WALLET-001");
        account.apply(changed_state).unwrap();
        pg_cache.update_account(&account).unwrap();
        wait_until_async(
            || async {
                let result = pg_cache.load_account(&account.id()).await.unwrap();
                result.is_some() && result.unwrap().events().len() >= 2
            },
            Duration::from_secs(5),
        )
        .await;
        let account_result = pg_cache.load_account(&account.id()).await.unwrap();
        let loaded = account_result.unwrap();
        assert!(matches!(loaded, AccountAny::Wallet(_)));
        assert_entirely_equal(loaded, account);
        assert!(Currency::try_from_str("ENG729P").is_none());

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_wallet_account_rejects_malformed_balance() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();
        pg_cache.flush().unwrap();

        let mut event = wallet_account_state();
        event.account_id = AccountId::from("WALLET-MALFORMED-001");
        DatabaseQueries::add_account(&pg_cache.pool, false, event)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE account_event SET balances = '[{\"currency\":\"USD\"}]'::jsonb \
             WHERE account_id = 'WALLET-MALFORMED-001'",
        )
        .execute(&pg_cache.pool)
        .await
        .unwrap();

        let error = DatabaseQueries::load_account_events(
            &pg_cache.pool,
            &AccountId::from("WALLET-MALFORMED-001"),
        )
        .await
        .unwrap_err();

        assert!(
            error.to_string().contains("missing field `total`"),
            "was: {error}"
        );
        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_update_account_without_existing_event_returns_error() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();
        let event = cash_account_state_million_usd("1000000 USD", "100000 USD", "900000 USD");

        let result = DatabaseQueries::add_account(&pg_cache.pool, true, event).await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Account event does not exist")
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_quote() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        // Add target instrument and currencies
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        // Add quote
        let quote = quote_ethusdt_binance();
        pg_cache.add_quote(&quote).unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_instrument(&instrument.id())
                    .await
                    .unwrap()
                    .is_some()
                    && !pg_cache.load_quotes(&instrument.id()).unwrap().is_empty()
            },
            Duration::from_secs(5),
        )
        .await;
        let quotes = pg_cache.load_quotes(&instrument.id()).unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0], quote);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_trade() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        // Add target instrument and currencies
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        // Add trade
        let trade = stub_trade_ethusdt_buy();
        pg_cache.add_trade(&trade).unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_instrument(&instrument.id())
                    .await
                    .unwrap()
                    .is_some()
                    && !pg_cache.load_trades(&instrument.id()).unwrap().is_empty()
            },
            Duration::from_secs(5),
        )
        .await;
        let trades = pg_cache.load_trades(&instrument.id()).unwrap();
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0], trade);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_bar() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        // Add target instrument and currencies
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        // Add bar
        let bar = stub_bar();
        pg_cache.add_bar(&bar).unwrap();
        wait_until_async(
            || async {
                pg_cache
                    .load_instrument(&instrument.id())
                    .await
                    .unwrap()
                    .is_some()
                    && !pg_cache.load_bars(&instrument.id()).unwrap().is_empty()
            },
            Duration::from_secs(5),
        )
        .await;
        let bars = pg_cache.load_bars(&instrument.id()).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0], bar);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_signal() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        // Add signal
        let name = Ustr::from("SignalExample");
        let value = "0.0".to_string();
        let signal = Signal::new(name, value, UnixNanos::from(1), UnixNanos::from(2));
        pg_cache.add_signal(&signal).unwrap();

        wait_until(
            || pg_cache.load_signals(name.as_str()).unwrap().len() == 1,
            Duration::from_secs(5),
        );

        let signals = pg_cache.load_signals(name.as_str()).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0], signal);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_custom_data() {
        ensure_custom_data_registered::<RustTestCustomData>();

        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let instrument_id = InstrumentId::from("RUST.TEST");
        let metadata = indexmap! {
            "a".to_string() => serde_json::Value::String("1".to_string()),
            "b".to_string() => serde_json::Value::String("2".to_string()),
        };
        let params = Params::from_index_map(metadata);
        let data_type = DataType::new(
            "RustTestCustomData",
            Some(params),
            Some("RUST.TEST".to_string()),
        );
        let inner = RustTestCustomData {
            instrument_id,
            value: 42.0,
            flag: true,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };
        let data = CustomData::new(std::sync::Arc::new(inner), data_type.clone());

        pg_cache.add_custom_data(&data).unwrap();

        wait_until(
            || pg_cache.load_custom_data(&data_type).unwrap().len() == 1,
            Duration::from_secs(5),
        );

        let datas = pg_cache.load_custom_data(&data_type).unwrap();
        assert_eq!(datas.len(), 1);
        assert_eq!(datas[0].data_type.type_name(), "RustTestCustomData");
        assert_eq!(datas[0].data_type.identifier(), Some("RUST.TEST"));
        // Full CustomData wrapper roundtrip: loaded value must equal original
        assert_eq!(
            datas[0], data,
            "CustomData roundtrip through Postgres must preserve equality"
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_snapshot_order_state() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let client_order_id = ClientOrderId::new("O-19700101-000000-001-002-1");
        let instrument = InstrumentAny::CurrencyPair(currency_pair_ethusdt());

        // Add foreign key dependencies: instrument and currencies
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        let order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(client_order_id)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .tags(vec![Ustr::from("tag-1"), Ustr::from("tag-2")])
            .build();
        let expected = OrderSnapshot::from(order.clone());

        pg_cache.snapshot_order_state(&order).unwrap();

        wait_until(
            || {
                pg_cache
                    .load_order_snapshot(&client_order_id)
                    .unwrap()
                    .is_some()
            },
            Duration::from_secs(5),
        );

        let loaded = pg_cache
            .load_order_snapshot(&client_order_id)
            .unwrap()
            .unwrap();

        assert_entirely_equal(loaded, expected);
        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_order_snapshot_keeps_exact_avg_px_and_slippage() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let client_order_id = ClientOrderId::new("O-19700101-000000-001-002-2");
        let instrument = InstrumentAny::CurrencyPair(currency_pair_ethusdt());

        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        let order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(client_order_id)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        // Full 28-place scale, which the previous `double precision` column could not hold
        let mut snapshot: OrderSnapshot = order.into();
        snapshot.avg_px = Some(Decimal::from_str("1.6666666666666666666666666667").unwrap());
        snapshot.slippage = Some(Decimal::from_str("0.0000000000000000000000000001").unwrap());

        pg_cache.add_order_snapshot(&snapshot).unwrap();

        wait_until(
            || {
                pg_cache
                    .load_order_snapshot(&client_order_id)
                    .unwrap()
                    .is_some()
            },
            Duration::from_secs(5),
        );

        let loaded = pg_cache
            .load_order_snapshot(&client_order_id)
            .unwrap()
            .unwrap();

        assert_eq!(loaded.avg_px, snapshot.avg_px);
        assert_eq!(loaded.slippage, snapshot.slippage);
        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_init_postgres_skips_existing_objects_on_re_run() {
        // `types.sql` has no `CREATE TYPE IF NOT EXISTS`, so a re-run against an initialized
        // database always raises "already exists" on the first statement. The loader must skip it
        // and carry on; it previously mapped that branch to `Err(())` and unwrapped, aborting init.
        let options = get_postgres_connect_options(None, None, None, None, None);
        let pg = connect_test_pg(options.clone().into()).await.unwrap();
        let schema_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/sql").to_string();

        let result = init_postgres(&pg, options.database, options.password, Some(schema_dir)).await;

        assert!(
            result.is_ok(),
            "re-running init must succeed, was {result:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_postgres_application_role_owns_schema_objects() {
        let options = get_postgres_connect_options(None, None, None, None, None);
        let expected_owner = options.database.clone();
        let pg = connect_test_pg(options.clone().into()).await.unwrap();
        let schema_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/sql").to_string();

        init_postgres(&pg, options.database, options.password, Some(schema_dir))
            .await
            .unwrap();

        let owners: Vec<(String, String)> = sqlx::query_as(
            "SELECT 'database', pg_get_userbyid(datdba)
             FROM pg_database
             WHERE datname = current_database()
             UNION ALL
             SELECT 'domain', pg_get_userbyid(t.typowner)
             FROM pg_type t
             JOIN pg_namespace n ON n.oid = t.typnamespace
             WHERE n.nspname = 'public' AND t.typname = 'i256'
             UNION ALL
             SELECT 'function', pg_get_userbyid(p.proowner)
             FROM pg_proc p
             JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE n.nspname = 'public' AND p.proname = 'get_all_tables'
             UNION ALL
             SELECT 'schema', pg_get_userbyid(nspowner)
             FROM pg_namespace
             WHERE nspname = 'public'
             UNION ALL
             SELECT 'table', pg_get_userbyid(c.relowner)
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'public' AND c.relname = 'order'
             UNION ALL
             SELECT 'type', pg_get_userbyid(t.typowner)
             FROM pg_type t
             JOIN pg_namespace n ON n.oid = t.typnamespace
             WHERE n.nspname = 'public' AND t.typname = 'account_type'
             ORDER BY 1",
        )
        .fetch_all(&pg)
        .await
        .unwrap();

        assert_eq!(
            owners,
            vec![
                (String::from("database"), expected_owner.clone()),
                (String::from("domain"), expected_owner.clone()),
                (String::from("function"), expected_owner.clone()),
                (String::from("schema"), expected_owner.clone()),
                (String::from("table"), expected_owner.clone()),
                (String::from("type"), expected_owner),
            ]
        );
    }

    // Extracts the guarded order-column migration from the real schema file, so the test runs the
    // shipped SQL rather than a copy of it.
    fn order_numeric_migration_sql() -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/sql/tables.sql");
        let schema = std::fs::read_to_string(path).unwrap();
        let start = schema
            .find("DO $$")
            .expect("no DO block in tables.sql; the order-column migration moved");
        let end = schema[start..]
            .find("END $$;")
            .expect("unterminated DO block in tables.sql")
            + start
            + "END $$;".len();
        let block = schema[start..end].to_string();
        assert!(
            block.contains("avg_px TYPE NUMERIC") && block.contains("slippage TYPE NUMERIC"),
            "first DO block in tables.sql is no longer the order-column migration"
        );
        block
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_order_column_migration_converts_legacy_floats_without_rounding() {
        // A direct `double precision::numeric` cast rounds to 15 significant digits, so these
        // would land as 1.23456789012346 and 1.66666666666667. The shipped migration casts through
        // `text` instead, which takes float8's shortest round-trip output.
        const LEGACY_AVG_PX: &str = "1.2345678901234567";
        const LEGACY_SLIPPAGE: &str = "1.6666666666666667";

        let options = get_postgres_connect_options(None, None, None, None, None);
        let pg = connect_test_pg(options.into()).await.unwrap();

        // The whole exercise runs inside a transaction that is always rolled back: PostgreSQL DDL
        // is transactional, so neither the column downgrade nor the probe row can outlive the
        // test, even on panic.
        let mut tx = pg.begin().await.unwrap();

        sqlx::query(
            r#"ALTER TABLE "order"
               ALTER COLUMN avg_px TYPE DOUBLE PRECISION USING avg_px::float8,
               ALTER COLUMN slippage TYPE DOUBLE PRECISION USING slippage::float8"#,
        )
        .execute(&mut *tx)
        .await
        .unwrap();

        sqlx::query(
            r#"INSERT INTO "order" (id, strategy_id, client_order_id, order_type, order_side,
               quantity, time_in_force, status, avg_px, slippage, init_id, ts_init, ts_last)
               VALUES ('O-LEGACY-FLOAT', 'S-1', 'O-LEGACY-FLOAT', 'MARKET', 'BUY', '1', 'GTC',
               'FILLED', $1, $2, 'i', '0', '0')"#,
        )
        .bind(LEGACY_AVG_PX.parse::<f64>().unwrap())
        .bind(LEGACY_SLIPPAGE.parse::<f64>().unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();

        sqlx::query(AssertSqlSafe(order_numeric_migration_sql()))
            .execute(&mut *tx)
            .await
            .unwrap();

        let (avg_px, slippage): (Decimal, Decimal) =
            sqlx::query_as(r#"SELECT avg_px, slippage FROM "order" WHERE id = 'O-LEGACY-FLOAT'"#)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let types: Vec<String> = sqlx::query_scalar(
            "SELECT data_type FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = 'order'
               AND column_name IN ('avg_px', 'slippage')
             ORDER BY column_name",
        )
        .fetch_all(&mut *tx)
        .await
        .unwrap();

        tx.rollback().await.unwrap();

        assert_eq!(types, vec!["numeric".to_string(), "numeric".to_string()]);
        assert_eq!(avg_px, Decimal::from_str(LEGACY_AVG_PX).unwrap());
        assert_eq!(slippage, Decimal::from_str(LEGACY_SLIPPAGE).unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_snapshot_position_state_replays_later_fill_after_restart() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let instrument = InstrumentAny::CurrencyPair(currency_pair_ethusdt());
        let position_id = PositionId::new("P-PG-ROUTINE-SNAPSHOT");
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        let opening_order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-PG-ROUTINE-SNAPSHOT-1"))
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();
        let OrderEventAny::Filled(opening_fill) = TestOrderEventStubs::filled(
            &opening_order,
            &instrument,
            Some(TradeId::new("T-PG-ROUTINE-SNAPSHOT-1")),
            Some(position_id),
            Some(Price::from("100.0")),
            Some(Quantity::from("1.0")),
            None,
            None,
            None,
            Some(AccountId::new("SIM-001")),
        ) else {
            unreachable!();
        };
        let mut position = Position::new(&instrument, opening_fill);

        pg_cache.add_position(&position).unwrap();
        pg_cache
            .snapshot_position_state(&position, UnixNanos::from(1_000_000_000), None)
            .unwrap();

        let next_order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-PG-ROUTINE-SNAPSHOT-2"))
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.5"))
            .build();
        let OrderEventAny::Filled(next_fill) = TestOrderEventStubs::filled(
            &next_order,
            &instrument,
            Some(TradeId::new("T-PG-ROUTINE-SNAPSHOT-2")),
            Some(position_id),
            Some(Price::from("101.0")),
            Some(Quantity::from("0.5")),
            None,
            None,
            None,
            Some(AccountId::new("SIM-001")),
        ) else {
            unreachable!();
        };
        position.apply(&next_fill);
        pg_cache.update_position(&position).unwrap();
        pg_cache.close().unwrap();

        let mut restarted = get_test_pg_cache_database().await.unwrap();
        let snapshot = restarted
            .load_position_snapshot(&position_id)
            .unwrap()
            .expect("routine position snapshot should survive restart");
        let loaded = restarted
            .load_position(&position_id)
            .await
            .unwrap()
            .expect("position should replay fills newer than its routine snapshot");

        assert_eq!(snapshot.quantity, Quantity::from("1.0"));
        assert!(snapshot.replay_state.is_none());
        assert_eq!(loaded.quantity, Quantity::from("1.5"));
        assert_eq!(loaded.events.len(), 2);
        assert_entirely_equal(&loaded, &position);

        restarted.flush().unwrap();
        restarted.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_snapshot_position_state_survives_restart_with_fill_void() {
        let mut pg_cache = get_test_pg_cache_database().await.unwrap();

        let client_order_id = ClientOrderId::new("O-PG-POSITION-SNAPSHOT");
        let instrument = InstrumentAny::CurrencyPair(currency_pair_ethusdt());

        // Add foreign key dependencies: instrument and currencies
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();

        let order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(client_order_id)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        let OrderEventAny::Filled(fill) = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::new("T-PG-POSITION-SNAPSHOT")),
            Some(PositionId::new("P-PG-POSITION-SNAPSHOT")),
            Some(Price::from("100.0")),
            Some(Quantity::from("1.0")),
            None,
            None,
            None,
            Some(AccountId::new("SIM-001")),
        ) else {
            unreachable!();
        };
        let mut position = Position::new(&instrument, fill.clone());
        let voided_qty = Quantity::from("0.4");
        let fill_void = OrderFillVoidedSpec::builder()
            .trader_id(fill.trader_id)
            .strategy_id(fill.strategy_id)
            .instrument_id(fill.instrument_id)
            .client_order_id(fill.client_order_id)
            .venue_order_id(fill.venue_order_id)
            .account_id(fill.account_id)
            .trade_id(fill.trade_id)
            .voided_qty(voided_qty)
            .order_side(fill.order_side)
            .order_type(fill.order_type)
            .last_px(fill.last_px)
            .currency(fill.currency)
            .liquidity_side(fill.liquidity_side)
            .position_id(position.id)
            .build();
        position
            .apply_fill_void(fill_void, voided_qty, None)
            .unwrap();

        let ts_snapshot = UnixNanos::from(2_000_000_000);
        let unrealized_pnl = Money::from("12.34 USDT");
        pg_cache.add_position(&position).unwrap();
        pg_cache
            .snapshot_position_state(&position, ts_snapshot, Some(unrealized_pnl))
            .unwrap();
        pg_cache.close().unwrap();

        let mut restarted = get_test_pg_cache_database().await.unwrap();
        let snapshot = restarted
            .load_position_snapshot(&position.id)
            .unwrap()
            .expect("position snapshot should survive restart");
        let loaded = restarted
            .load_position(&position.id)
            .await
            .unwrap()
            .expect("position should load from the persisted snapshot");

        assert_eq!(snapshot.position_id, position.id);
        assert_eq!(snapshot.ts_init, ts_snapshot);
        assert_eq!(snapshot.unrealized_pnl, Some(unrealized_pnl));
        assert!(snapshot.replay_state.is_some());
        assert_eq!(loaded.quantity, Quantity::from("0.6"));
        assert_eq!(loaded.fill_voids.len(), 1);
        assert_entirely_equal(&loaded, &position);

        restarted.flush().unwrap();
        restarted.close().unwrap();
    }
}
