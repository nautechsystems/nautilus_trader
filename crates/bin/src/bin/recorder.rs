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


use nautilus_bin::strategy::recorder::{config::RecorderConfig, strategy::Recorder};
use nautilus_bin::exchange::Exchange;

use nautilus_model::identifiers::TraderId;

const EXCHANGE_STR: &str = "bybit";

const TRADER_ID: &str = "TESTER-001";


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    // tracing_subscriber::fmt::init();
        nautilus_common::logging::ensure_logging_initialized(); 
        
    let exchange: Exchange = EXCHANGE_STR.parse()?;
    let trader_id = TraderId::from(TRADER_ID);

    let (mut node, instrument_id) = exchange.build_node(trader_id)?;

    let config = RecorderConfig::builder()
        .instrument_id(instrument_id)
        .path("data/".into())
        .build();
    
    let strategy = Recorder::new(config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}

