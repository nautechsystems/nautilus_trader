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

use nautilus_bin::config::Config;
use nautilus_bin::exchange::Exchange;
use nautilus_bin::strategy::grid_mm::{config::GridMarketMakerConfig, strategy::GridMarketMaker};
use nautilus_model::identifiers::{InstrumentId, TraderId};
use nautilus_model::types::Quantity;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let cfg = Config::load()?.grid_mm.expect("config.toml missing [grid_mm] section");

    let exchange: Exchange = cfg.exchange.parse()?;
    let trader_id = TraderId::from(cfg.trader_id.as_str());

    let mut node = exchange.build_node(trader_id)?;
    let instrument_id = InstrumentId::from(cfg.instrument_id);

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
    let strategy = GridMarketMaker::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
