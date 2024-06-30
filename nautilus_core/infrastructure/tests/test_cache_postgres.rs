// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 2Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_common::cache::{core::CacheConfig, database::CacheDatabaseAdapter, Cache};

#[must_use]
pub fn get_cache(cache_database: Option<Box<dyn CacheDatabaseAdapter>>) -> Cache {
    let cache_config = CacheConfig::default();
    Cache::new(cache_config, cache_database)
}

#[cfg(test)]
#[cfg(target_os = "linux")] // Databases only supported on Linux
mod serial_tests {
    use std::time::Duration;

    use nautilus_common::{cache::database::CacheDatabaseAdapter, testing::wait_until};
    use nautilus_infrastructure::sql::cache_database::get_pg_cache_database;
    use nautilus_model::{
        enums::{CurrencyType, OrderSide},
        identifiers::ClientOrderId,
        instruments::{
            any::InstrumentAny,
            stubs::{crypto_perpetual_ethusdt, currency_pair_ethusdt},
            Instrument,
        },
        orders::stubs::TestOrderStubs,
        types::{currency::Currency, quantity::Quantity},
    };

    use crate::get_cache;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_instruments() {
        let mut database = get_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(database.clone())));
        let eth = Currency::new("ETH", 2, 0, "ETH", CurrencyType::Crypto).unwrap();
        let usdt = Currency::new("USDT", 2, 0, "USDT", CurrencyType::Crypto).unwrap();
        let crypto_perpetual = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        // insert into database and wait
        database.add_currency(&eth).unwrap();
        database.add_currency(&usdt).unwrap();
        database.add_instrument(&crypto_perpetual).unwrap();
        wait_until(
            || {
                let currencies = database.load_currencies().unwrap();
                let instruments = database.load_instruments().unwrap();
                currencies.len() >= 2 && !instruments.is_empty()
            },
            Duration::from_secs(2),
        );
        // load instruments and build indexes
        cache.cache_instruments().unwrap();
        cache.build_index();
        // test
        let cached_instrument_ids = cache.instrument_ids(None);
        assert_eq!(cached_instrument_ids.len(), 1);
        assert_eq!(cached_instrument_ids, vec![&crypto_perpetual.id()]);
        let target_instrument = cache.instrument(&crypto_perpetual.id());
        assert_eq!(target_instrument.unwrap(), &crypto_perpetual);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_orders() {
        let mut database = get_pg_cache_database().await.unwrap();
        let mut cache = get_cache(Some(Box::new(database.clone())));
        let instrument = currency_pair_ethusdt();
        let market_order = TestOrderStubs::market_order(
            instrument.id(),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Some(ClientOrderId::new("O-19700101-0000-001-001-1").unwrap()),
            None,
        );
        // insert into database and wait
        database.add_order(&market_order).unwrap();
        wait_until(
            || {
                let order = database
                    .load_order(&market_order.client_order_id())
                    .unwrap();
                order.is_some()
            },
            Duration::from_secs(2),
        );
        // load orders and build indexes
        cache.cache_orders().unwrap();
        cache.build_index();
        // test
        let cached_order_ids = cache.client_order_ids(None, None, None);
        assert_eq!(cached_order_ids.len(), 1);
        let target_order = cache.order(&market_order.client_order_id());
        assert_eq!(target_order.unwrap(), &market_order);
    }
}
