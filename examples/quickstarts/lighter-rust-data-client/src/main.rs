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

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_lighter::{
    common::enums::LighterEnvironment, config::LighterDataClientConfig,
    factories::LighterDataClientFactory,
};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{ClientId, InstrumentId, TraderId};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const LIGHTER_ENVIRONMENT: LighterEnvironment = LighterEnvironment::Testnet;

const TRADER_ID: &str = "TESTER-001";
const NODE_NAME: &str = "LIGHTER-DATA-STARTER-001";
const CLIENT_ID: &str = "LIGHTER";
const INSTRUMENT_ID: &str = "BTC-PERP.LIGHTER";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let trader_id = TraderId::from(TRADER_ID);
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);

    let data_config = LighterDataClientConfig::builder()
        .environment(LIGHTER_ENVIRONMENT)
        .build();

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(NODE_NAME.to_string())
        .with_logging(log_config)
        .with_delay_post_stop_secs(2)
        .add_data_client(
            None,
            Box::new(LighterDataClientFactory::new()),
            Box::new(data_config),
        )?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(ClientId::new(CLIENT_ID))
        .instrument_ids(vec![instrument_id])
        .request_instruments(true)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .build()?;

    node.add_actor(DataTester::new(tester_config))?;

    node.run().await?;

    Ok(())
}
