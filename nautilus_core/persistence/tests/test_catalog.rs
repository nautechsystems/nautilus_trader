// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::data::tick::{Data, QuoteTick, TradeTick};
use nautilus_persistence::session::{PersistenceCatalog, QueryResult};

// Note: "current_thread" hangs up for some reason
#[tokio::test(flavor = "multi_thread")]
async fn test_quote_ticks() {
    let mut catalog = PersistenceCatalog::new(5000);
    catalog
        .add_file::<QuoteTick>(
            "quote_tick",
            "../../tests/test_data/quote_tick_data.parquet",
        )
        .await
        .unwrap();
    catalog
        .add_file::<QuoteTick>(
            "quote_tick_2",
            "../../tests/test_data/quote_tick_data.parquet",
        )
        .await
        .unwrap();
    let query_result: QueryResult = catalog.to_query_result();
    let ticks: Vec<Data> = query_result.flatten().collect();

    // NOTE: is_sorted_by_key is unstable otherwise use
    // ticks.is_sorted_by_key(|tick| tick.ts_init)
    // https://github.com/rust-lang/rust/issues/53485
    let is_ascending_by_init = |ticks: &Vec<Data>| {
        for i in 1..ticks.len() {
            // previous tick is more recent than current tick
            // this is not ascending order
            if ticks[i - 1].get_ts_init() > ticks[i].get_ts_init() {
                return false;
            }
        }
        true
    };

    match &ticks[0] {
        Data::Trade(_) => assert!(false),
        Data::Quote(q) => assert_eq!("EUR/USD.SIM", q.instrument_id.to_string()),
    }
    assert_eq!(ticks.len(), 19000);
    assert!(is_ascending_by_init(&ticks));
}

// Note: "current_thread" hangs up for some reason
#[tokio::test(flavor = "multi_thread")]
async fn test_data_ticks() {
    let mut catalog = PersistenceCatalog::new(5000);
    catalog
        .add_file::<QuoteTick>(
            "quote_tick",
            "../../tests/test_data/quote_tick_data.parquet",
        )
        .await
        .unwrap();
    catalog
        .add_file::<TradeTick>(
            "quote_tick_2",
            "../../tests/test_data/trade_tick_data.parquet",
        )
        .await
        .unwrap();
    let query_result: QueryResult = catalog.to_query_result();
    let ticks: Vec<Data> = query_result.flatten().collect();

    // NOTE: is_sorted_by_key is unstable otherwise use
    // ticks.is_sorted_by_key(|tick| tick.ts_init)
    // https://github.com/rust-lang/rust/issues/53485
    let is_ascending_by_init = |ticks: &Vec<Data>| {
        for i in 1..ticks.len() {
            // previous tick is more recent than current tick
            // this is not ascending order
            if ticks[i - 1].get_ts_init() > ticks[i].get_ts_init() {
                return false;
            }
        }
        true
    };

    assert_eq!(ticks.len(), 9600);
    assert!(is_ascending_by_init(&ticks));
}
