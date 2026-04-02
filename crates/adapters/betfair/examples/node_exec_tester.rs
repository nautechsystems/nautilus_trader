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

//! Example demonstrating live execution testing with the Betfair adapter.
//!
//! Run with: `cargo run -p nautilus-betfair --example betfair-exec-tester`
//!
//! Environment variables:
//! - `BETFAIR_USERNAME`: Your Betfair username
//! - `BETFAIR_PASSWORD`: Your Betfair password
//! - `BETFAIR_APP_KEY`: Your Betfair application key
//! - `BETFAIR_MARKET_ID`: Optional market ID override. Defaults to `1.254209667`
//! - `BETFAIR_INSTRUMENT_ID`: Optional instrument ID override after market preload
//!
//! Market IDs can be found from `https://www.betfair.com.au/exchange/plus/`

use std::sync::Arc;

use nautilus_betfair::{
    config::{BetfairDataConfig, BetfairExecConfig},
    factories::{BetfairDataClientFactory, BetfairExecutionClientFactory},
    http::client::BetfairHttpClient,
    provider::{BetfairInstrumentProvider, NavigationFilter},
};
use nautilus_common::{enums::Environment, providers::InstrumentProvider};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    enums::TimeInForce,
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    instruments::{Instrument, InstrumentAny},
    types::{Currency, Quantity},
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let market_id =
        std::env::var("BETFAIR_MARKET_ID").unwrap_or_else(|_| "1.254209667".to_string());
    let (account_currency, instruments) = load_market_context(&market_id).await?;
    let instrument_id = select_exec_instrument(&instruments)?;
    let instrument_ids = instrument_ids(&instruments);

    println!("Found instruments for market {market_id}: {instrument_ids:?}");
    println!("Using execution instrument: {instrument_id}");

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BETFAIR-001");
    let node_name = "BETFAIR-EXEC-TESTER-001".to_string();
    let client_id = ClientId::new("BETFAIR");

    let data_config = BetfairDataConfig {
        account_currency: account_currency.clone(),
        market_ids: Some(vec![market_id.clone()]),
        stream_conflate_ms: Some(0),
        ..Default::default()
    };

    let exec_config = BetfairExecConfig {
        trader_id,
        account_id,
        account_currency,
        stream_market_ids_filter: Some(vec![market_id.clone()]),
        ignore_external_orders: true,
        reconcile_market_ids_only: true,
        reconcile_market_ids: Some(vec![market_id]),
        ..Default::default()
    };

    let data_factory = BetfairDataClientFactory::new();
    let exec_factory = BetfairExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_delay_post_stop_secs(5)
        .build()?;

    let order_qty = Quantity::from("2.00");

    // Betfair does not expose quote subscriptions or normal market orders.
    // Use a BSP market-on-close order so ExecTester can still submit one order on start.
    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC_TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .subscribe_quotes(false)
        .subscribe_trades(false)
        .enable_limit_buys(false)
        .enable_limit_sells(false)
        .open_position_on_start_qty(order_qty.as_decimal())
        .open_position_time_in_force(TimeInForce::AtTheClose)
        .close_positions_on_stop(false)
        .can_unsubscribe(false)
        .log_data(false)
        .build();

    let tester = ExecTester::new(tester_config);

    node.add_strategy(tester)?;
    node.run().await?;

    Ok(())
}

async fn load_market_context(market_id: &str) -> anyhow::Result<(String, Vec<InstrumentAny>)> {
    let credential = BetfairDataConfig::default().credential()?;
    let http_client = Arc::new(BetfairHttpClient::new(
        credential,
        None,
        None,
        None,
        None,
        Some(5),
        None,
    )?);

    http_client.connect().await?;

    let placeholder_provider = BetfairInstrumentProvider::new(
        Arc::clone(&http_client),
        NavigationFilter::default(),
        Currency::GBP(),
        None,
    );

    let account_currency = placeholder_provider.get_account_currency().await?;
    let mut provider = BetfairInstrumentProvider::new(
        Arc::clone(&http_client),
        NavigationFilter {
            market_ids: Some(vec![market_id.to_string()]),
            ..Default::default()
        },
        account_currency,
        None,
    );

    provider.load_all(None).await?;
    http_client.disconnect().await;

    let instruments: Vec<InstrumentAny> =
        provider.store().list_all().into_iter().cloned().collect();

    if instruments.is_empty() {
        anyhow::bail!(
            "No instruments found for BETFAIR_MARKET_ID={market_id}, find an active market ID \
             from https://www.betfair.com.au/exchange/plus/ and ensure the market is still available"
        );
    }

    Ok((account_currency.code.as_str().to_string(), instruments))
}

fn select_exec_instrument(instruments: &[InstrumentAny]) -> anyhow::Result<InstrumentId> {
    if let Ok(instrument_id) = std::env::var("BETFAIR_INSTRUMENT_ID") {
        let instrument_id = InstrumentId::from(instrument_id.as_str());

        if instruments
            .iter()
            .any(|instrument| instrument.id() == instrument_id)
        {
            return Ok(instrument_id);
        }

        anyhow::bail!("BETFAIR_INSTRUMENT_ID={instrument_id} was not found in the loaded market");
    }

    instruments
        .first()
        .map(InstrumentAny::id)
        .ok_or_else(|| anyhow::anyhow!("No Betfair instruments available for execution testing"))
}

fn instrument_ids(instruments: &[InstrumentAny]) -> Vec<InstrumentId> {
    instruments.iter().map(InstrumentAny::id).collect()
}
