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

use nautilus_bin::config::{Config, RecorderTomlConfig};
use nautilus_bin::exchange::Exchange;
use nautilus_bin::strategy::recorder::{config::RecorderConfig, strategy::Recorder};
use nautilus_model::identifiers::{InstrumentId, TraderId};
use nautilus_bin::cli::Args;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let args = Args::parse();

    let cfg: RecorderTomlConfig = Config::load(args.config_path)?.recorder.unwrap();

    let exchange: Exchange = cfg.exchange.parse()?;
    let trader_id = TraderId::from(cfg.trader_id.as_str());

    let mut node = exchange.build_node(trader_id)?;

    let instrument_id: Vec<InstrumentId> = cfg.instrument_id.iter().map(|instrument_id|{
        InstrumentId::from(instrument_id)
    }).collect();

    let config = RecorderConfig::builder()
        // .instrument_id(cfg.instrument_id)
        .path(cfg.path)
        .instrument_id(instrument_id)
        .interval_parquet_dump_seconds(cfg.interval_parquet_dump_seconds)
        .book_depth(cfg.book_depth)
        .build();

    let strategy = Recorder::new(&config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
