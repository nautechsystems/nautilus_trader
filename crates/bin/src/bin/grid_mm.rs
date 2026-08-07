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

use nautilus_backtest::{
    config::{
        BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType,
    },
    node::BacktestNode,
};
use nautilus_bin::config::Config;
use nautilus_bin::exchange::Exchange;
use nautilus_bin::strategy::grid_mm::{config::GridMarketMakerConfig, strategy::GridMarketMaker};
use nautilus_common::enums::Environment;
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, InstrumentId, TraderId},
};
use nautilus_model::types::Quantity;

use chrono::{TimeZone, Utc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let cfg = Config::load("config.toml".to_string())?.grid_mm.expect("config.toml missing [grid_mm] section");

    let exchange: Exchange = cfg.exchange.parse()?;
    let trader_id = TraderId::from(cfg.trader_id.as_str());
    let instrument_id = InstrumentId::from(cfg.instrument_id);
    let catalog_path = cfg.path.clone();

    let mut node = exchange.build_node(trader_id)?;

    let config = GridMarketMakerConfig::builder()
        .instrument_id(instrument_id)
        .max_position(Quantity::from(cfg.max_position.as_str()))
        .trade_size(Quantity::from(cfg.trade_size.as_str()))
        .num_levels(cfg.num_levels)
        .grid_step_bps(cfg.grid_step_bps)
        .skew_factor(cfg.skew_factor)
        .requote_threshold_bps(cfg.requote_threshold_bps)
        .maybe_expire_time_secs(cfg.expire_time_secs)
        .on_cancel_resubmit(cfg.on_cancel_resubmit)
        .build();

    let start_date = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).single().unwrap();
    let end_date = Utc.with_ymd_and_hms(2026, 8, 7, 0, 0, 0).single().unwrap();

    match &cfg.execution_environment {
        Environment::Backtest => {
            let venue = BacktestVenueConfig::builder()
                .name("BYBIT")
                .oms_type(OmsType::Hedging)
                .account_type(AccountType::Margin)
                .book_type(BookType::L2_MBP)
                .starting_balances(vec!["1_000 USDT".to_string()])
                .build()?;

            let order_book = BacktestDataConfig::builder()
                .catalog_path(catalog_path.clone())
                .instrument_id(instrument_id)
                .start_time(start_date.into())
                .end_time(end_date.into())
                .data_type(NautilusDataType::OrderBookDelta)
                .optimize_file_loading(true)
                .build()?;

            let trades = BacktestDataConfig::builder()
                .catalog_path(catalog_path.clone())
                .instrument_id(instrument_id)
                .start_time(start_date.into())
                .end_time(end_date.into())
                .data_type(NautilusDataType::TradeTick)
                .optimize_file_loading(true)
                .build()?;

            let run = BacktestRunConfig::builder()
                .id("grid-mm-backtest".to_string())
                .venues(vec![venue])
                .data(vec![order_book, trades])
                .chunk_size(1_000_000)
                .build()?;

            let mut node = BacktestNode::new(vec![run])?;

            node.build()?;
            {
                let engine = node.get_engine_mut("grid-mm-backtest").unwrap();

                let strategy = GridMarketMaker::new(config);

                engine.add_strategy(strategy)?;
            }
            node.run()?;

            let engine = node.get_engine_mut("grid-mm-backtest").unwrap();

            let snapshots = engine
                .kernel()
                .portfolio
                .borrow()
                .snapshots(&AccountId::from("BYBIT-001"));

            log::info!("{snapshots:#?}");
        }
        Environment::Sandbox => todo!(),
        Environment::Live => {
            let strategy = GridMarketMaker::new(config);
            node.add_strategy(strategy)?;
            node.run().await?;
        }
    }

    Ok(())
}