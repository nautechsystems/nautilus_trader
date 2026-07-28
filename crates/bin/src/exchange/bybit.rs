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

//! Bybit exchange setup for the GridMarketMaker example.

use log::LevelFilter;
use nautilus_bybit::{
    common::enums::BybitEnvironment,
    config::{BybitDataClientConfig, BybitExecClientConfig},
    factories::{BybitDataClientFactory, BybitExecutionClientFactory},
};
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::{AccountId, TraderId};

pub const ACCOUNT_ID: &str = "BYBIT-001";
pub const NODE_NAME: &str = "BYBIT-GRID-MM-001";

pub fn build_node(trader_id: TraderId) -> Result<LiveNode, Box<dyn std::error::Error>> {
    let bybit_env = BybitEnvironment::Mainnet;
    let environment = Environment::Live;
    let account_id = AccountId::from(ACCOUNT_ID);
    let node_name = NODE_NAME.to_string();

    let data_config = BybitDataClientConfig {
        environment: bybit_env,
        ..Default::default()
    };

    let exec_config = BybitExecClientConfig {
        environment: bybit_env,
        account_id: Some(account_id),
        ..Default::default()
    };

    let data_factory = BybitDataClientFactory::new();
    let exec_factory = BybitExecutionClientFactory::new(trader_id, account_id);

    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(false)
        .with_delay_post_stop_secs(5)
        .build()?;

    Ok(node)
}