// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 2Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_common::cache::{Cache, database::CacheDatabaseAdapter};

#[must_use]
pub fn get_cache(cache_database: Option<Box<dyn CacheDatabaseAdapter>>) -> Cache {
    Cache::new(None, cache_database)
}

#[cfg(test)]
#[cfg(target_os = "linux")] // Databases only supported on Linux

mod serial_tests {
    use std::time::Duration;

    use nautilus_common::{cache::database::CacheDatabaseAdapter, testing::wait_until_async};
    use nautilus_infrastructure::sql::cache::get_pg_cache_database;
    use nautilus_model::{
        accounts::AccountAny,
        enums::{CurrencyType, OrderSide, OrderType},
        identifiers::ClientOrderId,
        instruments::{
            Instrument, InstrumentAny,
            stubs::{crypto_perpetual_ethusdt, currency_pair_ethusdt},
        },
        orders::{Order, builder::OrderTestBuilder},
        types::{Currency, Quantity},
    };

    use crate::get_cache;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_instruments() {
        let mut database = get_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_pg_cache_database().await.unwrap())));

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
            Duration::from_secs(2),
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
        let mut database = get_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_pg_cache_database().await.unwrap())));

        let instrument = currency_pair_ethusdt();
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-19700101-0000-001-001-1"))
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
            Duration::from_secs(2),
        )
        .await;

        // Load orders and build indexes
        cache.cache_orders().await.unwrap();
        cache.build_index();

        let cached_order_ids = cache.client_order_ids(None, None, None);
        assert_eq!(cached_order_ids.len(), 1);
        let target_order = cache.order(&market_order.client_order_id());
        assert_eq!(target_order.unwrap(), &market_order);

        database.flush().unwrap();
        database.close().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_accounts() {
        let mut database = get_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(get_pg_cache_database().await.unwrap())));

        let account = AccountAny::default();
        let last_event = account.last_event().unwrap();
        if last_event.base_currency.is_some() {
            database
                .add_currency(&last_event.base_currency.unwrap())
                .unwrap();
        }

        // Insert into database and wait
        database.add_account(&account).unwrap();
        wait_until_async(
            || async {
                let account = database.load_account(&account.id()).await.unwrap();
                account.is_some()
            },
            Duration::from_secs(2),
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
}
