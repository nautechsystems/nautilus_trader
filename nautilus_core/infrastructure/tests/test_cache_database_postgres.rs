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

#[cfg(test)]
#[cfg(target_os = "linux")] // Databases only supported on Linux
mod serial_tests {
    use std::{collections::HashSet, time::Duration};

    use bytes::Bytes;
    use indexmap::indexmap;
    use nautilus_common::{
        cache::database::CacheDatabaseAdapter, custom::CustomData, signal::Signal,
        testing::wait_until,
    };
    use nautilus_core::nanos::UnixNanos;
    use nautilus_infrastructure::sql::cache::get_pg_cache_database;
    use nautilus_model::{
        accounts::{any::AccountAny, cash::CashAccount},
        data::{
            stubs::{quote_ethusdt_binance, stub_bar, stub_trade_ethusdt_buyer},
            DataType,
        },
        enums::{CurrencyType, OrderSide, OrderStatus, OrderType},
        events::account::stubs::cash_account_state_million_usd,
        identifiers::{
            stubs::account_id, AccountId, ClientId, ClientOrderId, InstrumentId, TradeId,
            VenueOrderId,
        },
        instruments::{
            any::InstrumentAny,
            stubs::{
                audusd_sim, binary_option, crypto_future_btcusdt, crypto_perpetual_ethusdt,
                currency_pair_ethusdt, equity_aapl, futures_contract_es, options_contract_appl,
            },
            Instrument,
        },
        orders::{builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{currency::Currency, price::Price, quantity::Quantity},
    };
    use serde::Serialize;
    use ustr::Ustr;

    pub fn entirely_equal<T: Serialize>(a: T, b: T) {
        let a_serialized = serde_json::to_string(&a).unwrap();
        let b_serialized = serde_json::to_string(&b).unwrap();

        assert_eq!(a_serialized, b_serialized);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_general_object_adds_to_cache() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        let test_id_value = Bytes::from("test_value");
        pg_cache
            .add(String::from("test_id"), test_id_value.clone())
            .unwrap();
        wait_until(
            || {
                let result = pg_cache.load().unwrap();
                result.keys().len() > 0
            },
            Duration::from_secs(2),
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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_currency_and_instruments() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        // Define currencies
        let btc = Currency::new("BTC", 8, 0, "BTC", CurrencyType::Crypto);
        let eth = Currency::new("ETH", 2, 0, "ETH", CurrencyType::Crypto);
        let gbp = Currency::new("GBP", 2, 0, "GBP", CurrencyType::Fiat);
        let usd = Currency::new("USD", 2, 0, "USD", CurrencyType::Fiat);
        let usdc = Currency::new("USDC", 8, 0, "USDC", CurrencyType::Crypto);
        let usdt = Currency::new("USDT", 2, 0, "USDT", CurrencyType::Crypto);
        // Insert all the currencies
        pg_cache.add_currency(&btc).unwrap();
        pg_cache.add_currency(&eth).unwrap();
        pg_cache.add_currency(&gbp).unwrap();
        pg_cache.add_currency(&usd).unwrap();
        pg_cache.add_currency(&usdc).unwrap();
        pg_cache.add_currency(&usdt).unwrap();
        // Define all the instruments
        let binary_option = binary_option();
        let crypto_future =
            crypto_future_btcusdt(2, 6, Price::from("0.01"), Quantity::from("0.000001"));
        let crypto_perpetual = crypto_perpetual_ethusdt();
        let currency_pair = currency_pair_ethusdt();
        let equity = equity_aapl();
        let futures_contract = futures_contract_es(None, None);
        let options_contract = options_contract_appl();
        // Insert all the instruments
        pg_cache
            .add_instrument(&InstrumentAny::BinaryOption(binary_option))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CryptoFuture(crypto_future))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CryptoPerpetual(crypto_perpetual))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CurrencyPair(currency_pair))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::Equity(equity))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::FuturesContract(futures_contract))
            .unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::OptionsContract(options_contract))
            .unwrap();
        // Wait for the cache to update
        wait_until(
            || {
                let currencies = pg_cache.load_currencies().unwrap();
                let instruments = pg_cache.load_instruments().unwrap();
                currencies.len() >= 6 && instruments.len() >= 7
            },
            Duration::from_secs(2),
        );
        // Check that currency list is correct
        let currencies = pg_cache.load_currencies().unwrap();
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
            pg_cache.load_currency(&Ustr::from("BTC")).unwrap().unwrap(),
            btc
        );
        assert_eq!(
            pg_cache.load_currency(&Ustr::from("ETH")).unwrap().unwrap(),
            eth
        );
        assert_eq!(
            pg_cache.load_currency(&Ustr::from("GBP")).unwrap().unwrap(),
            gbp
        );
        assert_eq!(
            pg_cache.load_currency(&Ustr::from("USD")).unwrap().unwrap(),
            usd
        );
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("USDC"))
                .unwrap()
                .unwrap(),
            usdc
        );
        assert_eq!(
            pg_cache
                .load_currency(&Ustr::from("USDT"))
                .unwrap()
                .unwrap(),
            usdt
        );
        // Check individual instruments
        assert_eq!(
            pg_cache
                .load_instrument(&binary_option.id())
                .unwrap()
                .unwrap(),
            InstrumentAny::BinaryOption(binary_option)
        );
        assert_eq!(
            pg_cache
                .load_instrument(&crypto_future.id())
                .unwrap()
                .unwrap(),
            InstrumentAny::CryptoFuture(crypto_future)
        );
        assert_eq!(
            pg_cache
                .load_instrument(&crypto_perpetual.id())
                .unwrap()
                .unwrap(),
            InstrumentAny::CryptoPerpetual(crypto_perpetual)
        );
        assert_eq!(
            pg_cache
                .load_instrument(&currency_pair.id())
                .unwrap()
                .unwrap(),
            InstrumentAny::CurrencyPair(currency_pair)
        );
        assert_eq!(
            pg_cache.load_instrument(&equity.id()).unwrap().unwrap(),
            InstrumentAny::Equity(equity)
        );
        assert_eq!(
            pg_cache
                .load_instrument(&futures_contract.id())
                .unwrap()
                .unwrap(),
            InstrumentAny::FuturesContract(futures_contract)
        );
        assert_eq!(
            pg_cache
                .load_instrument(&options_contract.id())
                .unwrap()
                .unwrap(),
            InstrumentAny::OptionsContract(options_contract)
        );
        // Check that instrument list is correct
        let instruments = pg_cache.load_instruments().unwrap();
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
                options_contract.id()
            ]
            .into_iter()
            .collect::<HashSet<InstrumentId>>()
        );

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_postgres_cache_database_add_order_and_load_indexes() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        let client_order_id_1 = ClientOrderId::new("O-19700101-000000-001-001-1");
        let client_order_id_2 = ClientOrderId::new("O-19700101-000000-001-001-2");
        let instrument = currency_pair_ethusdt();

        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(client_order_id_1)
            .build();
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .price(Price::from("100.0"))
            .quantity(Quantity::from("1.0"))
            .client_order_id(client_order_id_2)
            .build();

        // add foreign key dependencies: instrument and currencies
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache
            .add_instrument(&InstrumentAny::CurrencyPair(instrument))
            .unwrap();
        // Set client id
        let client_id = ClientId::new("TEST");
        // add orders
        pg_cache.add_order(&market_order, Some(client_id)).unwrap();
        pg_cache.add_order(&limit_order, Some(client_id)).unwrap();
        wait_until(
            || {
                pg_cache
                    .load_order(&market_order.client_order_id())
                    .unwrap()
                    .is_some()
                    && pg_cache
                        .load_order(&limit_order.client_order_id())
                        .unwrap()
                        .is_some()
            },
            Duration::from_secs(2),
        );
        let market_order_result = pg_cache
            .load_order(&market_order.client_order_id())
            .unwrap();
        let limit_order_result = pg_cache.load_order(&limit_order.client_order_id()).unwrap();
        let client_order_ids = pg_cache.load_index_order_client().unwrap();
        entirely_equal(market_order_result.unwrap(), market_order);
        entirely_equal(limit_order_result.unwrap(), limit_order);
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
    async fn test_update_order_for_open_order() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        let client_order_id_1 = ClientOrderId::new("O-19700101-000000-001-002-1");
        let instrument = InstrumentAny::CurrencyPair(currency_pair_ethusdt());
        let account = account_id();
        // add foreign key dependencies: instrument and currencies
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();
        // 1. Create the order
        let mut market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(client_order_id_1)
            .build();
        pg_cache.add_order(&market_order, None).unwrap();
        let submitted = TestOrderEventStubs::order_submitted(&market_order, account);
        market_order.apply(submitted).unwrap();
        pg_cache.update_order(market_order.last_event()).unwrap();

        let accepted =
            TestOrderEventStubs::order_accepted(&market_order, account, VenueOrderId::new("001"));
        market_order.apply(accepted).unwrap();
        pg_cache.update_order(market_order.last_event()).unwrap();

        let filled = TestOrderEventStubs::order_filled(
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
        wait_until(
            || {
                let result = pg_cache
                    .load_order(&market_order.client_order_id())
                    .unwrap();
                result.is_some() && result.unwrap().status() == OrderStatus::Filled
            },
            Duration::from_secs(2),
        );
        // Assert
        let market_order_result = pg_cache
            .load_order(&market_order.client_order_id())
            .unwrap();
        entirely_equal(market_order_result.unwrap(), market_order);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_add_and_update_account() {
        let pg_cache = get_pg_cache_database().await.unwrap();

        let mut account = AccountAny::Cash(CashAccount::new(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
            false,
        ));
        let last_event = account.last_event().unwrap();
        if last_event.base_currency.is_some() {
            pg_cache
                .add_currency(&last_event.base_currency.unwrap())
                .unwrap();
        }
        pg_cache.add_account(&account).unwrap();
        wait_until(
            || pg_cache.load_account(&account.id()).unwrap().is_some(),
            Duration::from_secs(2),
        );
        let account_result = pg_cache.load_account(&account.id()).unwrap();
        entirely_equal(account_result.unwrap(), account.clone());
        // Update the account
        let new_account_state_event =
            cash_account_state_million_usd("1000000 USD", "100000 USD", "900000 USD");
        account.apply(new_account_state_event);
        pg_cache.update_account(&account).unwrap();
        wait_until(
            || {
                let result = pg_cache.load_account(&account.id()).unwrap();
                result.is_some() && result.unwrap().events().len() >= 2
            },
            Duration::from_secs(2),
        );
        let account_result = pg_cache.load_account(&account.id()).unwrap();
        entirely_equal(account_result.unwrap(), account);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_postgres_cache_database_add_trade_tick() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();
        // add target instrument and currencies
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();
        // add trade tick
        let trade_tick = stub_trade_ethusdt_buyer();
        pg_cache.add_trade(&trade_tick).unwrap();
        wait_until(
            || {
                pg_cache
                    .load_instrument(&instrument.id())
                    .unwrap()
                    .is_some()
                    && !pg_cache.load_trades(&instrument.id()).unwrap().is_empty()
            },
            Duration::from_secs(2),
        );
        let trades = pg_cache.load_trades(&instrument.id()).unwrap();
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0], trade_tick);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_postgres_cache_database_add_quote_tick() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        // add target instrument and currencies
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();
        // add quote tick
        let quote_tick = quote_ethusdt_binance();
        pg_cache.add_quote(&quote_tick).unwrap();
        wait_until(
            || {
                pg_cache
                    .load_instrument(&instrument.id())
                    .unwrap()
                    .is_some()
                    && !pg_cache.load_quotes(&instrument.id()).unwrap().is_empty()
            },
            Duration::from_secs(2),
        );
        let quotes = pg_cache.load_quotes(&instrument.id()).unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0], quote_tick);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_postgres_cache_database_add_bar() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        // add target instrument and currencies
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();
        // add bar
        let bar = stub_bar();
        pg_cache.add_bar(&bar).unwrap();
        wait_until(
            || {
                pg_cache
                    .load_instrument(&instrument.id())
                    .unwrap()
                    .is_some()
                    && !pg_cache.load_bars(&instrument.id()).unwrap().is_empty()
            },
            Duration::from_secs(2),
        );
        let bars = pg_cache.load_bars(&instrument.id()).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0], bar);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_truncate() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        // add items in currency and instrument table
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        pg_cache
            .add_currency(&instrument.base_currency().unwrap())
            .unwrap();
        pg_cache.add_currency(&instrument.quote_currency()).unwrap();
        pg_cache.add_instrument(&instrument).unwrap();
        wait_until(
            || {
                pg_cache.load_currencies().unwrap().len() == 2
                    && pg_cache.load_instruments().unwrap().len() == 1
            },
            Duration::from_secs(2),
        );

        // call flush which will truncate all the tables
        pg_cache.flush().unwrap();

        // check if all the tables are empty
        let currencies = pg_cache.load_currencies().unwrap();
        assert_eq!(currencies.len(), 0);
        let instruments = pg_cache.load_instruments().unwrap();
        assert_eq!(instruments.len(), 0);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_postgres_cache_database_add_signal() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        // Add signal
        let name = Ustr::from("SignalExample");
        let value = "0.0".to_string();
        let signal = Signal::new(name, value, UnixNanos::from(1), UnixNanos::from(2));
        pg_cache.add_signal(&signal).unwrap();

        wait_until(
            || pg_cache.load_signals(name.as_str()).unwrap().len() == 1,
            Duration::from_secs(2),
        );

        let signals = pg_cache.load_signals(name.as_str()).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0], signal);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_postgres_cache_database_add_custom_data() {
        let mut pg_cache = get_pg_cache_database().await.unwrap();

        // Add custom data
        let metadata =
            indexmap! {"a".to_string() => "1".to_string(), "b".to_string() => "2".to_string()};
        let data_type = DataType::new("TestData", Some(metadata));
        let json_stub_value = r#"{"a":"1","b":"2"}"#;
        let json_value: serde_json::Value = serde_json::from_str(json_stub_value).unwrap();
        let serialized_bytes = serde_json::to_vec(&json_value).unwrap();

        let data = CustomData::new(
            data_type.clone(),
            Bytes::from(serialized_bytes),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        pg_cache.add_custom_data(&data).unwrap();

        wait_until(
            || pg_cache.load_custom_data(&data_type).unwrap().len() == 1,
            Duration::from_secs(2),
        );

        let datas = pg_cache.load_custom_data(&data_type).unwrap();
        assert_eq!(datas.len(), 1);
        assert_eq!(datas[0], data);

        pg_cache.flush().unwrap();
        pg_cache.close().unwrap();
    }
}
