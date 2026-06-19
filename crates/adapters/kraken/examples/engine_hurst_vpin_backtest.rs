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

//! Example backtest of the Hurst/VPIN directional strategy on Kraken Futures
//! `PF_XBTUSD` using historical trades and quotes loaded from Tardis CSV files.
//!
//! Edit the constants below to change the data paths, target instrument, and
//! strategy tuning parameters.
//!
//! Run with: `cargo run -p nautilus-kraken --features examples --example kraken-hurst-vpin-backtest --release`
//!
//! The default data paths point to `/tmp/tardis_kraken/`. The first day of each
//! month is available for free from Tardis without an API key:
//!
//! ```bash
//! curl -L -o PF_XBTUSD_trades.csv.gz \
//!   https://datasets.tardis.dev/v1/cryptofacilities/trades/2024/01/01/PF_XBTUSD.csv.gz
//! curl -L -o PF_XBTUSD_quotes.csv.gz \
//!   https://datasets.tardis.dev/v1/cryptofacilities/quotes/2024/01/01/PF_XBTUSD.csv.gz
//! ```
//!
//! No credentials are required.

// *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
// *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
};
use nautilus_kraken::common::consts::KRAKEN_VENUE;
use nautilus_model::{
    data::{BarType, Data},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_tardis::csv::load::{load_quotes, load_trades};
use nautilus_trading::examples::strategies::{HurstVpinDirectional, HurstVpinDirectionalConfig};
use rust_decimal_macros::dec;

const TRADES_PATH: &str = "/tmp/tardis_kraken/PF_XBTUSD_trades.csv.gz";
const QUOTES_PATH: &str = "/tmp/tardis_kraken/PF_XBTUSD_quotes.csv.gz";

const INSTRUMENT_ID: &str = "PF_XBTUSD.KRAKEN";
const SYMBOL: &str = "PF_XBTUSD";
const BAR_TYPE: &str = "PF_XBTUSD.KRAKEN-2000000-VALUE-LAST-INTERNAL";

const TRADE_SIZE: &str = "0.0100";
const MAX_HOLDING_SECS: u64 = 1800;
const STARTING_BALANCE: &str = "100_000 USD";

fn main() -> anyhow::Result<()> {
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let trades = load_trades(TRADES_PATH, Some(1), Some(4), Some(instrument_id), None)
        .map_err(|e| anyhow::anyhow!("Failed to load trades: {e}"))?;
    let quotes = load_quotes(QUOTES_PATH, Some(1), Some(4), Some(instrument_id), None)
        .map_err(|e| anyhow::anyhow!("Failed to load quotes: {e}"))?;
    println!("Loaded {} trades, {} quotes", trades.len(), quotes.len());

    let instrument = CryptoPerpetual::new(
        instrument_id,
        Symbol::from(SYMBOL),
        Currency::BTC(),
        Currency::USD(),
        Currency::USD(),
        false,
        1,
        4,
        Price::from("0.5"),
        Quantity::from("0.0001"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(dec!(0.02)),
        Some(dec!(0.01)),
        Some(dec!(0.0002)),
        Some(dec!(0.0005)),
        None,
        None,
        0.into(),
        0.into(),
    );

    let mut engine = BacktestEngine::new(BacktestEngineConfig::default())?;

    engine.add_venue(
        SimulatedVenueConfig::builder()
            .venue(*KRAKEN_VENUE)
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from(STARTING_BALANCE)])
            .build(),
    )?;

    engine.add_instrument(&InstrumentAny::CryptoPerpetual(instrument))?;

    let bar_type = BarType::from(BAR_TYPE);
    let config =
        HurstVpinDirectionalConfig::new(instrument_id, bar_type, Quantity::from(TRADE_SIZE))
            .with_max_holding_secs(MAX_HOLDING_SECS);
    engine.add_strategy(HurstVpinDirectional::new(config))?;

    let mut data: Vec<Data> = trades.into_iter().map(Data::Trade).collect();
    data.extend(quotes.into_iter().map(Data::Quote));
    engine.add_data(data, None, true, true)?;

    engine.run(None, None, None, false)?;
    Ok(())
}
