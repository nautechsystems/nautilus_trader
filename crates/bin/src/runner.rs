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

//! Runtime strategy dispatch and shared backtest/live orchestration.

use std::fmt::Debug;

use anyhow::{Context, Result, bail};
use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType},
    node::BacktestNode,
};
use nautilus_common::{actor::DataActorNative, component::Component, enums::Environment};
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, InstrumentId, TraderId},
};
use nautilus_trading::{Strategy, StrategyNative};

use crate::{
    config::{
        Config, GridMarketMakerTomlConfig, MattiasMarketMakerTomlConfig, ObiMomentumTomlConfig,
        RunnerTomlConfig,
    },
    exchange::Exchange,
    strategy::{
        grid_mm::{config::GridMarketMakerConfig, strategy::GridMarketMaker},
        mmm::{config::MattiasMarketMakerConfig, strategy::MattiasMarketMaker},
        obi_momentum::{config::ObiMomentumConfig, strategy::ObiMomentum},
    },
};

fn from_box_err(e: &dyn std::error::Error) -> anyhow::Error {
    anyhow::Error::msg(e.to_string())
}

/// Common accessors shared by all strategy TOML configs.
pub trait StrategyToml {
    fn exchange(&self) -> &str;
    fn trader_id(&self) -> &str;
    fn instrument_id(&self) -> &str;
    fn path(&self) -> &str;
    fn execution_environment(&self) -> Environment;
}

impl StrategyToml for GridMarketMakerTomlConfig {
    fn exchange(&self) -> &str {
        &self.exchange
    }

    fn trader_id(&self) -> &str {
        &self.trader_id
    }

    fn instrument_id(&self) -> &str {
        &self.instrument_id
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn execution_environment(&self) -> Environment {
        self.execution_environment
    }
}

impl StrategyToml for MattiasMarketMakerTomlConfig {
    fn exchange(&self) -> &str {
        &self.exchange
    }

    fn trader_id(&self) -> &str {
        &self.trader_id
    }

    fn instrument_id(&self) -> &str {
        &self.instrument_id
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn execution_environment(&self) -> Environment {
        self.execution_environment
    }
}

impl StrategyToml for ObiMomentumTomlConfig {
    fn exchange(&self) -> &str {
        &self.exchange
    }

    fn trader_id(&self) -> &str {
        &self.trader_id
    }

    fn instrument_id(&self) -> &str {
        &self.instrument_id
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn execution_environment(&self) -> Environment {
        self.execution_environment
    }
}

/// Runs the strategy selected in `[runner]` of the given config.
///
/// The strategy is chosen at runtime by name, so switching strategies
/// only requires editing the config file, no recompilation.
pub async fn run(config: &Config, runner: &RunnerTomlConfig) -> Result<()> {
    match runner.strategy.as_str() {
        "grid_mm" => {
            let toml = config
                .grid_mm
                .as_ref()
                .context("[runner] strategy 'grid_mm' requires a [grid_mm] section")?;
            let strategy = GridMarketMaker::new(GridMarketMakerConfig::try_from(toml)?);
            run_strategy(toml, runner, strategy).await
        }
        "mmm" => {
            let toml = config
                .mmm
                .as_ref()
                .context("[runner] strategy 'mmm' requires a [mmm] section")?;
            let strategy = MattiasMarketMaker::new(&MattiasMarketMakerConfig::try_from(toml)?);
            run_strategy(toml, runner, strategy).await
        }
        "obi_momentum" => {
            let toml = config
                .obi_momentum
                .as_ref()
                .context("[runner] strategy 'obi_momentum' requires a [obi_momentum] section")?;
            let strategy = ObiMomentum::new(ObiMomentumConfig::try_from(toml)?);
            run_strategy(toml, runner, strategy).await
        }
        other => bail!(
            "unknown strategy '{other}'. Registered strategies: 'grid_mm', 'mmm', 'obi_momentum'"
        ),
    }
}

/// Shared orchestration for a concrete strategy across execution environments.
async fn run_strategy<T, C>(toml: &C, runner: &RunnerTomlConfig, strategy: T) -> Result<()>
where
    T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    C: StrategyToml,
{
    let exchange: Exchange = toml
        .exchange()
        .parse()
        .map_err(|e: Box<dyn std::error::Error>| from_box_err(e.as_ref()))?;
    let trader_id = TraderId::from(toml.trader_id());
    let instrument_id = InstrumentId::from(toml.instrument_id());
    let catalog_path = toml.path().to_string();

    match toml.execution_environment() {
        Environment::Backtest => {
            let start_date = runner.start_date.context(
                "[runner] start_date required for backtesting (e.g. \"2026-08-01T00:00:00Z\")",
            )?;
            let end_date = runner.end_date.context(
                "[runner] end_date required for backtesting (e.g. \"2026-08-08T00:00:00Z\")",
            )?;
            let run_id = runner
                .run_id
                .clone()
                .unwrap_or_else(|| format!("{}-backtest", runner.strategy));

            let venue = BacktestVenueConfig::builder()
                .name(runner.venue.clone())
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
                .id(run_id.clone())
                .venues(vec![venue])
                .data(vec![order_book, trades])
                .chunk_size(1_000_000)
                .build()?;

            let mut node = BacktestNode::new(vec![run])?;

            node.build()?;
            {
                let engine = node
                    .get_engine_mut(&run_id)
                    .context("backtest engine not found")?;

                engine.add_strategy(strategy)?;
            }
            node.run()?;

            let engine = node
                .get_engine_mut(&run_id)
                .context("backtest engine not found")?;

            let snapshots = engine
                .kernel()
                .portfolio
                .borrow()
                .snapshots(&AccountId::from(runner.account_id.as_str()));

            log::info!("{snapshots:#?}");
        }
        Environment::Sandbox => todo!(),
        Environment::Live => {
            let mut node = exchange
                .build_node(trader_id)
                .map_err(|e: Box<dyn std::error::Error>| from_box_err(e.as_ref()))?;
            node.add_strategy(strategy)?;
            node.run().await?;
        }
    }

    Ok(())
}
