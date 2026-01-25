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

//! Sandbox example for Databento live data with CME simulated execution.
//!
//! This example demonstrates paper trading against live CME futures data from Databento
//! using the sandbox execution client for order simulation.
//!
//! Run with: `cargo run --example databento-cme-sandbox --package nautilus-sandbox`
//!
//! Environment variables:
//! - DATABENTO_API_KEY: Your Databento API key

use std::path::PathBuf;

use nautilus_common::enums::Environment;
use nautilus_core::env::get_env_var;
use nautilus_databento::factories::{DatabentoDataClientFactory, DatabentoLiveClientConfig};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId, Venue},
    types::{Currency, Money, Quantity},
};
use nautilus_sandbox::{SandboxExecutionClientConfig, SandboxExecutionClientFactory};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use rust_decimal::Decimal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("SANDBOX-001");
    let node_name = "DATABENTO-CME-SANDBOX".to_string();

    let api_key = get_env_var("DATABENTO_API_KEY").unwrap_or_else(|_| {
        println!("WARNING: DATABENTO_API_KEY not found, using placeholder");
        "db-placeholder-key".to_string()
    });

    let publishers_filepath = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("databento")
        .join("publishers.json");

    if !publishers_filepath.exists() {
        println!(
            "WARNING: Publishers file not found at: {}",
            publishers_filepath.display()
        );
    }

    let databento_config = DatabentoLiveClientConfig::new(
        api_key,
        publishers_filepath,
        true, // use_exchange_as_venue
        true, // bars_timestamp_on_close
    );

    let xcme_venue = Venue::new("XCME");
    let account_id = AccountId::from("XCME-SANDBOX-001");
    let usd = Currency::USD();
    let starting_balance = Money::new(1_000_000.0, usd);

    let sandbox_config = SandboxExecutionClientConfig {
        trader_id,
        account_id,
        venue: xcme_venue,
        starting_balances: vec![starting_balance],
        base_currency: Some(usd),
        oms_type: OmsType::Netting,
        account_type: AccountType::Margin,
        default_leverage: Decimal::ONE,
        leverages: ahash::AHashMap::new(),
        book_type: BookType::L1_MBP,
        frozen_account: false,
        bar_execution: true,
        trade_execution: false,
        reject_stop_orders: true,
        support_gtd_orders: true,
        support_contingent_orders: true,
        use_position_ids: true,
        use_random_ids: false,
        use_reduce_only: true,
    };

    let databento_factory = DatabentoDataClientFactory::new();
    let sandbox_factory = SandboxExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_load_state(false)
        .with_save_state(false)
        .add_data_client(
            None,
            Box::new(databento_factory),
            Box::new(databento_config),
        )?
        .add_exec_client(
            Some("XCME".to_string()),
            Box::new(sandbox_factory),
            Box::new(sandbox_config),
        )?
        .with_delay_post_stop_secs(2)
        .build()?;

    let instrument_id = InstrumentId::from("ESM6.XCME");
    let client_id = ClientId::new("DATABENTO");

    let mut tester_config = ExecTesterConfig::new(
        StrategyId::from("SANDBOX_TESTER-001"),
        instrument_id,
        client_id,
        Quantity::from("1"), // 1 contract
    )
    .with_subscribe_trades(true)
    .with_subscribe_quotes(true)
    .with_log_data(true);

    tester_config.base.use_uuid_client_order_ids = true;

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
