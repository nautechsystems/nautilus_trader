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

//! Example demonstrating paper trading with Hyperliquid outcome (prediction) markets.
//!
//! This example shows how to combine Hyperliquid live data with sandbox (simulated)
//! execution for paper trading prediction markets.
//!
//! Run with:
//! `cargo run --example hyperliquid-outcome-paper --package nautilus-hyperliquid --features example-outcome-paper`
//!
//! # Overview
//!
//! This demonstrates the recommended architecture for Hyperliquid prediction market
//! paper trading per `t-plan.md` Phase B:
//!
//! - **Data Client**: HyperliquidDataClient (live market data)
//! - **Execution Client**: SandboxExecutionClient (simulated execution)
//! - **Instrument**: BinaryOption (for outcome markets)
//!
//! # Key Features
//!
//! - Subscribes to outcome market quotes (e.g., BTC price predictions)
//! - Uses sandbox matching engine for simulated fills
//! - Tracks positions and PnL in real-time

use log::LevelFilter;
use nautilus_common::{enums::Environment, logging::logger::LoggerConfig};
use nautilus_hyperliquid::{
    HyperliquidDataClientConfig, HyperliquidDataClientFactory,
    common::enums::HyperliquidEnvironment,
    http::{client::HyperliquidHttpClient, parse::HyperliquidMarketType},
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    enums::CurrencyType,
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId, Venue},
    types::{Currency, Money, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use nautilus_sandbox::{SandboxExecutionClientConfig, SandboxExecutionClientFactory};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("PAPER-001");
    let account_id = AccountId::from("PAPER-001");
    let node_name = "HYPERLIQUID-OUTCOME-PAPER-001".to_string();
    let client_id = ClientId::new("SANDBOX");

    // Discover a currently listed outcome market instrument at runtime to
    // avoid hard-coding a potentially stale symbol.
    let discovery_client = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 30, None)?;
    let outcome_defs = discovery_client.request_instrument_defs().await?;
    let outcome_symbol = outcome_defs
        .iter()
        .find(|def| {
            def.market_type == HyperliquidMarketType::Outcome
                && def.symbol.as_str().ends_with("-YES-OUTCOME")
        })
        .or_else(|| {
            outcome_defs
                .iter()
                .find(|def| def.market_type == HyperliquidMarketType::Outcome)
        })
        .ok_or("No outcome market instrument found on Hyperliquid")?;
    let outcome_id = format!("{}.HYPERLIQUID", outcome_symbol.symbol.as_str());
    let outcome_instrument = InstrumentId::from(outcome_id.as_str());

    // Configure Hyperliquid data client (live data)
    let hyperliquid_data_config = HyperliquidDataClientConfig {
        environment: HyperliquidEnvironment::Mainnet,
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    // Configure sandbox execution client (simulated execution)
    let usdh_currency = Currency::try_from_str("USDH").unwrap_or(Currency::new(
        "USDH",
        6,
        0,
        "USDH",
        CurrencyType::Crypto,
    ));

    let sandbox_exec_config = SandboxExecutionClientConfig::builder()
        .trader_id(trader_id)
        .account_id(account_id)
        .venue(Venue::new("HYPERLIQUID"))
        .base_currency(usdh_currency)
        .starting_balances(vec![Money::new(
            10_000.0,
            Currency::new("USDH", 6, 0, "USDH", CurrencyType::Crypto),
        )])
        .build();

    // Create factories
    let data_factory = HyperliquidDataClientFactory::new();
    let exec_factory = SandboxExecutionClientFactory::new();

    // Configure logging
    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    // Build the live node
    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_logging(log_config)
        // Add Hyperliquid data client for live market data
        .add_data_client(
            Some("HYPERLIQUID-DATA".to_string()),
            Box::new(data_factory),
            Box::new(hyperliquid_data_config),
        )?
        // Add sandbox execution client for paper trading
        .add_simulated_exec_client(
            Some("SANDBOX".to_string()),
            Box::new(exec_factory),
            Box::new(sandbox_exec_config),
        )?
        .with_delay_post_stop_secs(5)
        .build()?;

    // Configure execution tester for outcome market
    let order_qty = Quantity::from("100"); // 100 USDH units

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("OUTCOME_PAPER-001")),
            external_order_claims: Some(vec![outcome_instrument]),
            ..Default::default()
        })
        .instrument_id(outcome_instrument)
        .client_id(client_id)
        .order_qty(order_qty)
        .log_data(true)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}
