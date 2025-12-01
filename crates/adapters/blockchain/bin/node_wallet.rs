// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, rc::Rc};

use nautilus_blockchain::{
    config::BlockchainExecutionClientConfig, constants::BLOCKCHAIN_VENUE,
    execution::client::BlockchainExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    live::{clock::LiveClock, runtime::get_runtime},
    logging::{init_logging, logger::LoggerConfig, writer::FileWriterConfig},
};
use nautilus_core::UUID4;
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_model::{
    defi::chain::chains,
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let trader_id = TraderId::default();
    let account = AccountId::default();
    let arbitrum = chains::ARBITRUM.clone();
    let ethereum = chains::ETHEREUM.clone();

    // Init logging
    let _log_guard = init_logging(
        trader_id,
        UUID4::new(),
        LoggerConfig::default(),
        FileWriterConfig::default(),
    )?;

    let arbitrum_rpc_url =
        std::env::var("ARBITRUM_RPC_HTTP_URL").expect("ARBITRUM_RPC_HTTP_URL must be set");
    let ethereum_rpc_url =
        std::env::var("ETHEREUM_RPC_HTTP_URL").expect("ETHEREUM_RPC_HTTP_URL must be set");

    let arbitrum_config = BlockchainExecutionClientConfig::new(
        trader_id,
        account,
        arbitrum,
        String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
        Some(vec![
            "0x912CE59144191C1204E64559FE8253a0e49E6548".to_string(),
            "0x40BD670A58238e6E230c430BBb5cE6ec0d40df48".to_string(),
        ]),
        arbitrum_rpc_url,
        None,
    );
    let ethereum_config = BlockchainExecutionClientConfig::new(
        trader_id,
        account,
        ethereum,
        String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
        Some(vec![
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            "0xd5F7838F5C461fefF7FE49ea5ebaF7728bB0ADfa".to_string(),
            "0xB1D1eae60EEA9525032a6DCb4c1CE336a1dE71BE".to_string(),
            "0x4fE83213D56308330EC302a8BD641f1d0113A4Cc".to_string(),
        ]),
        ethereum_rpc_url,
        None,
    );
    let core_execution_client = ExecutionClientCore::new(
        trader_id,
        ClientId::default(),
        *BLOCKCHAIN_VENUE,
        OmsType::Netting,
        account,
        AccountType::Wallet,
        None,
        Rc::new(RefCell::new(LiveClock::new(None))),
        Rc::new(RefCell::new(Cache::default())),
    );

    let mut ethereum_execution_client =
        BlockchainExecutionClient::new(core_execution_client.clone(), ethereum_config)?;
    let mut arbitrum_execution_client =
        BlockchainExecutionClient::new(core_execution_client, arbitrum_config)?;

    get_runtime().block_on(async move {
        ethereum_execution_client.connect().await?;
        arbitrum_execution_client.connect().await?;
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
