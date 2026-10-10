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

//! Example demonstrating live data testing with the CoinAPI adapter.
//!
//! Edit the constants below to change the exchange, target instrument, bar specification, book
//! subscriptions, and historical requests.
//!
//! Run with: `cargo run --example coinapi-data-tester --package nautilus-coinapi --features examples`
//!
//! Requires `COINAPI_API_KEY`. Connecting spends one REST credit on filtered symbol metadata, and
//! each historical request spends credits per 100 returned records within `CREDIT_BUDGET`.

use std::collections::HashMap;

use nautilus_coinapi::{
    common::consts::COINAPI_CLIENT_ID,
    config::{CoinApiDataClientConfig, CoinApiInstrumentSelection},
    factories::CoinApiDataClientFactory,
    http::query::SymbolsQuery,
};
use nautilus_common::enums::Environment;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::bar::BarType,
    identifiers::{InstrumentId, TraderId},
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};
use ustr::Ustr;

const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "COINAPI-TESTER-001";
const EXCHANGE_ID: &str = "COINBASE";
const SYMBOL_ID: &str = "COINBASE_SPOT_BTC_USD";
const INSTRUMENT_ID: &str = "BTC-USD.COINBASE";

// Some captured BTC OHLCV prices use three decimals, finer than CoinAPI's two-decimal metadata
const PRICE_PRECISION: u8 = 3;
const BOOK_DEPTH: usize = 10;
const BAR_SPEC: &str = "1-MINUTE-LAST-EXTERNAL";
const CREDIT_BUDGET: u64 = 10;

// A connection streams one book channel, so enable deltas or depth, not both
const SUBSCRIBE_BOOK_DELTAS: bool = false;
const SUBSCRIBE_BOOK_DEPTH: bool = false;
const MANAGE_BOOK: bool = true;

// An unbounded history request can spend the whole remaining budget and leave none for other
// requests, so enable one request per run
const REQUEST_INSTRUMENTS: bool = false;
const REQUEST_BOOK_SNAPSHOT: bool = false;
const REQUEST_QUOTES: bool = false;
const REQUEST_TRADES: bool = false;
const REQUEST_BARS: bool = true;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from(TRADER_ID);
    let node_name = NODE_NAME.to_string();
    let client_id = *COINAPI_CLIENT_ID;

    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let bar_type = BarType::from(format!("{instrument_id}-{BAR_SPEC}").as_str());

    let coinapi_config = CoinApiDataClientConfig::builder()
        .instruments(vec![CoinApiInstrumentSelection {
            exchange_id: Ustr::from(EXCHANGE_ID),
            query: SymbolsQuery {
                filter_symbol_id: Some(SYMBOL_ID.to_string()),
                ..SymbolsQuery::default()
            },
            instrument_ids: vec![instrument_id],
        }])
        .price_precisions(HashMap::from([(instrument_id, PRICE_PRECISION)]))
        .credit_budget(CREDIT_BUDGET)
        .build(); // Reads 'COINAPI_API_KEY' env var

    let client_factory = CoinApiDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_load_state(false)
        .with_save_state(false)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(coinapi_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(vec![instrument_id])
        .request_instruments(REQUEST_INSTRUMENTS)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .bar_types(vec![bar_type])
        .subscribe_bars(true)
        .subscribe_book_deltas(SUBSCRIBE_BOOK_DELTAS)
        .subscribe_book_depth(SUBSCRIBE_BOOK_DEPTH)
        .manage_book(MANAGE_BOOK)
        .request_book_snapshot(REQUEST_BOOK_SNAPSHOT)
        .book_depth(BOOK_DEPTH)
        .request_quotes(REQUEST_QUOTES)
        .request_trades(REQUEST_TRADES)
        .request_bars(REQUEST_BARS)
        .build()?;

    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}
