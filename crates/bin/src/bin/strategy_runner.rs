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

//! `strategy_runner` - runs any registered strategy selected in `[runner]` of
//! the config file, without recompilation.

use clap::Parser;
use nautilus_bin::cli::Args;
use nautilus_bin::config::Config;
use nautilus_bin::runner;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let args = Args::parse();

    let config = Config::load(args.config_path)?;
    let runner_cfg = config
        .runner
        .clone()
        .expect("config.toml missing [runner] section");

    runner::run(&config, &runner_cfg).await?;

    Ok(())
}
