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
#[cfg(target_os = "linux")] // Databases only supported on Linux
mod tests {
    use std::time::Duration;

    use nautilus_core::equality::entirely_equal;
    use nautilus_model::{
        enums::{CurrencyType, OrderSide},
        events::order::event::OrderEventAny,
        identifiers::{
            account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
            stubs::account_id, trade_id::TradeId, venue_order_id::VenueOrderId,
        },
        instruments::{
            any::InstrumentAny,
            stubs::{
                crypto_future_btcusdt, crypto_perpetual_ethusdt, currency_pair_ethusdt,
                equity_aapl, futures_contract_es, options_contract_appl,
            },
            Instrument,
        },
        orders::{
            any::OrderAny,
            stubs::{TestOrderEventStubs, TestOrderStubs},
        },
        types::{currency::Currency, price::Price, quantity::Quantity},
    };
    use serial_test::serial;

    use crate::get_pg_cache_database;

    #[tokio::test]
    #[serial]
    async fn test_add_general_object_adds_to_cache() {
        let pg_cache = get_pg_cache_database().await.unwrap();
        let test_id_value = String::from("test_value").into_bytes();
        pg_cache
            .add(String::from("test_id"), test_id_value.clone())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let result = pg_cache.load().await.unwrap();
        assert_eq!(result.keys().len(), 1);
        assert_eq!(
            result.keys().cloned().collect::<Vec<String>>(),
            vec![String::from("test_id")]
        );
        assert_eq!(result.get("test_id").unwrap().to_owned(), test_id_value);
    }

    #[tokio::test]
    #[serial]
    async fn test_add_currency_and_instruments() {
        // 1. first define and add currencies as they are contain foreign keys for instruments
        let pg_cache = get_pg_cache_database().await.unwrap();
        // Define currencies
        let btc = Currency::new("BTC", 8, 0, "BTC", CurrencyType::Crypto).unwrap();
        let eth = Currency::new("ETH", 2, 0, "ETH", CurrencyType::Crypto).unwrap();
        let usd = Currency::new("USD", 2, 0, "USD", CurrencyType::Fiat).unwrap();
        let usdt = Currency::new("USDT", 2, 0, "USDT", CurrencyType::Crypto).unwrap();
        // Insert all the currencies
        pg_cache.add_currency(btc).await.unwrap();
        pg_cache.add_currency(eth).await.unwrap();
        pg_cache.add_currency(usd).await.unwrap();
        pg_cache.add_currency(usdt).await.unwrap();
        // Define all the instruments
        let crypto_future =
            crypto_future_btcusdt(2, 6, Price::from("0.01"), Quantity::from("0.000001"));
        let crypto_perpetual = crypto_perpetual_ethusdt();
        let currency_pair = currency_pair_ethusdt();
        let equity = equity_aapl();
        let futures_contract = futures_contract_es();
        let options_contract = options_contract_appl();
        // Insert all the instruments
        pg_cache
            .add_instrument(InstrumentAny::CryptoFuture(crypto_future))
            .await
            .unwrap();
        pg_cache
            .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual))
            .await
            .unwrap();
        pg_cache
            .add_instrument(InstrumentAny::CurrencyPair(currency_pair))
            .await
            .unwrap();
        pg_cache
            .add_instrument(InstrumentAny::Equity(equity))
            .await
            .unwrap();
        pg_cache
            .add_instrument(InstrumentAny::FuturesContract(futures_contract))
            .await
            .unwrap();
        pg_cache
            .add_instrument(InstrumentAny::OptionsContract(options_contract))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        // Check that currency list is correct
        let currencies = pg_cache.load_currencies().await.unwrap();
        assert_eq!(currencies.len(), 4);
        assert_eq!(
            currencies
                .into_iter()
                .map(|c| c.code.to_string())
                .collect::<Vec<String>>(),
            vec![
                String::from("BTC"),
                String::from("ETH"),
                String::from("USD"),
                String::from("USDT")
            ]
        );
        // Check individual currencies
        assert_eq!(pg_cache.load_currency("BTC").await.unwrap().unwrap(), btc);
        assert_eq!(pg_cache.load_currency("ETH").await.unwrap().unwrap(), eth);
        assert_eq!(pg_cache.load_currency("USDT").await.unwrap().unwrap(), usdt);
        // Check individual instruments
        assert_eq!(
            pg_cache
                .load_instrument(crypto_future.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::CryptoFuture(crypto_future)
        );
        assert_eq!(
            pg_cache
                .load_instrument(crypto_perpetual.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::CryptoPerpetual(crypto_perpetual)
        );
        assert_eq!(
            pg_cache
                .load_instrument(currency_pair.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::CurrencyPair(currency_pair)
        );
        assert_eq!(
            pg_cache
                .load_instrument(equity.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::Equity(equity)
        );
        assert_eq!(
            pg_cache
                .load_instrument(futures_contract.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::FuturesContract(futures_contract)
        );
        assert_eq!(
            pg_cache
                .load_instrument(options_contract.id())
                .await
                .unwrap()
                .unwrap(),
            InstrumentAny::OptionsContract(options_contract)
        );
        // Check that instrument list is correct
        let instruments = pg_cache.load_instruments().await.unwrap();
        assert_eq!(instruments.len(), 6);
        assert_eq!(
            instruments
                .into_iter()
                .map(|i| i.id())
                .collect::<Vec<InstrumentId>>(),
            vec![
                crypto_future.id(),
                crypto_perpetual.id(),
                currency_pair.id(),
                equity.id(),
                futures_contract.id(),
                options_contract.id()
            ]
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_add_order() {
        let client_order_id_1 = ClientOrderId::new("O-19700101-0000-000-001-1").unwrap();
        let client_order_id_2 = ClientOrderId::new("O-19700101-0000-000-001-2").unwrap();
        let instrument = currency_pair_ethusdt();
        let pg_cache = get_pg_cache_database().await.unwrap();
        let market_order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Some(client_order_id_1),
            None,
        );
        let limit_order = TestOrderStubs::limit_order(
            instrument.id(),
            OrderSide::Sell,
            Price::from("100.0"),
            Quantity::from("1.0"),
            Some(client_order_id_2),
            None,
        );
        pg_cache
            .add_order(OrderAny::Market(market_order.clone()))
            .await
            .unwrap();
        pg_cache
            .add_order(OrderAny::Limit(limit_order.clone()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let market_order_result = pg_cache
            .load_order(&market_order.client_order_id)
            .await
            .unwrap();
        let limit_order_result = pg_cache
            .load_order(&limit_order.client_order_id)
            .await
            .unwrap();
        entirely_equal(market_order_result.unwrap(), OrderAny::Market(market_order));
        entirely_equal(limit_order_result.unwrap(), OrderAny::Limit(limit_order));
    }

    #[tokio::test]
    #[serial]
    async fn test_update_order_for_open_order() {
        let client_order_id_1 = ClientOrderId::new("O-19700101-0000-000-002-1").unwrap();
        let instrument = currency_pair_ethusdt();
        let account = account_id();
        let pg_cache = get_pg_cache_database().await.unwrap();
        // Add the target currency of order
        pg_cache
            .add_currency(instrument.quote_currency)
            .await
            .unwrap();
        // 1. create the order
        let mut market_order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Some(client_order_id_1),
            None,
        );
        pg_cache
            .add_order(OrderAny::Market(market_order.clone()))
            .await
            .unwrap();
        let submitted_event = TestOrderEventStubs::order_submitted(&market_order, account).unwrap();
        market_order
            .apply(OrderEventAny::Submitted(submitted_event))
            .unwrap();
        pg_cache
            .update_order(OrderAny::Market(market_order.clone()))
            .await
            .unwrap();
        let accepted_event = TestOrderEventStubs::order_accepted(
            &market_order,
            account,
            VenueOrderId::new("001").unwrap(),
        )
        .unwrap();
        market_order
            .apply(OrderEventAny::Accepted(accepted_event))
            .unwrap();
        pg_cache
            .update_order(OrderAny::Market(market_order.clone()))
            .await
            .unwrap();
        let order_filled = TestOrderEventStubs::order_filled(
            &market_order,
            &instrument,
            None,
            Some(TradeId::new("T-19700101-0000-000-001-1").unwrap()),
            None,
            Some(Price::from("100.0")),
            Some(Quantity::from("1.0")),
            None,
            None,
            Some(AccountId::new("SIM-001").unwrap()),
        )
        .unwrap();
        market_order
            .apply(OrderEventAny::Filled(order_filled))
            .unwrap();
        pg_cache
            .update_order(OrderAny::Market(market_order.clone()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        // assert
        let market_order_result = pg_cache
            .load_order(&market_order.client_order_id)
            .await
            .unwrap();
        entirely_equal(market_order_result.unwrap(), OrderAny::Market(market_order));
    }
}
