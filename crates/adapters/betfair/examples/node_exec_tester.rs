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
//! Run with: `cargo run -p nautilus-betfair --example betfair-exec-tester --features examples`
//!
//! Environment variables:
//! - `BETFAIR_USERNAME`: Your Betfair username
//! - `BETFAIR_PASSWORD`: Your Betfair password
//! - `BETFAIR_APP_KEY`: Your Betfair application key
//! - `BETFAIR_MARKET_ID`: Required active market ID to load and test
//! - `BETFAIR_INSTRUMENT_ID`: Optional instrument ID override after market preload
//!
//! Market IDs can be found from `https://www.betfair.com.au/exchange/plus/`

use std::sync::Arc;

use nautilus_betfair::{
    common::enums::RunnerStatus,
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
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let market_id = std::env::var("BETFAIR_MARKET_ID").expect("BETFAIR_MARKET_ID must be set");
    let (account_currency, instruments, http_client) = load_market_context(&market_id).await?;
    let instrument_id = select_exec_instrument(&http_client, &market_id, &instruments).await?;
    http_client.disconnect().await;
    let instrument_choices = instrument_choices(&instruments);

    println!("Found instruments for market {market_id}:");
    for choice in &instrument_choices {
        println!("  {choice}");
    }
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

async fn load_market_context(
    market_id: &str,
) -> anyhow::Result<(String, Vec<InstrumentAny>, Arc<BetfairHttpClient>)> {
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

    let instruments: Vec<InstrumentAny> =
        provider.store().list_all().into_iter().cloned().collect();

    if instruments.is_empty() {
        anyhow::bail!(
            "No instruments found for BETFAIR_MARKET_ID={market_id}, find an active market ID \
             from https://www.betfair.com.au/exchange/plus/ and confirm the market is still available"
        );
    }

    Ok((
        account_currency.code.as_str().to_string(),
        instruments,
        http_client,
    ))
}

async fn select_exec_instrument(
    http_client: &BetfairHttpClient,
    market_id: &str,
    instruments: &[InstrumentAny],
) -> anyhow::Result<InstrumentId> {
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

    match instruments {
        [] => anyhow::bail!("No Betfair instruments available for execution testing"),
        [instrument] => Ok(instrument.id()),
        _ => match auto_select_active_instrument(http_client, market_id, instruments).await? {
            Some(instrument_id) => Ok(instrument_id),
            None => {
                let available = instrument_choices(instruments).join("\n  ");
                anyhow::bail!(
                    "Could not auto-select an active Betfair runner for market {market_id}.\n\n  \
                     Set BETFAIR_INSTRUMENT_ID to one of:\n  {available}"
                );
            }
        },
    }
}

async fn auto_select_active_instrument(
    http_client: &BetfairHttpClient,
    market_id: &str,
    instruments: &[InstrumentAny],
) -> anyhow::Result<Option<InstrumentId>> {
    let params = ListMarketBookParams {
        market_ids: vec![market_id.to_string()],
    };

    let books: Vec<MarketBook> = http_client
        .send_betting("SportsAPING/v1.0/listMarketBook", &params)
        .await?;

    let Some(book) = books.first() else {
        return Ok(None);
    };

    let mut active_runners: Vec<&RunnerBook> = book
        .runners
        .iter()
        .filter(|runner| runner.status == RunnerStatus::Active)
        .collect();

    active_runners.sort_by(|a, b| {
        b.total_matched
            .partial_cmp(&a.total_matched)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for runner in active_runners {
        let handicap = runner.handicap.unwrap_or(Decimal::ZERO);

        if let Some(instrument_id) = instruments.iter().find_map(|instrument| match instrument {
            InstrumentAny::Betting(betting)
                if betting.selection_id == runner.selection_id
                    && (betting.selection_handicap
                        - handicap.to_string().parse::<f64>().unwrap_or(0.0))
                    .abs()
                        < f64::EPSILON =>
            {
                Some(betting.id)
            }
            _ => None,
        }) {
            return Ok(Some(instrument_id));
        }
    }

    Ok(None)
}

fn instrument_choices(instruments: &[InstrumentAny]) -> Vec<String> {
    let mut choices: Vec<String> = instruments
        .iter()
        .map(|instrument| match instrument {
            InstrumentAny::Betting(betting) => {
                format!("{} ({})", betting.id, betting.selection_name)
            }
            _ => instrument.id().to_string(),
        })
        .collect();
    choices.sort();
    choices
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListMarketBookParams {
    market_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketBook {
    runners: Vec<RunnerBook>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerBook {
    selection_id: u64,
    #[serde(default)]
    handicap: Option<Decimal>,
    status: RunnerStatus,
    #[serde(default)]
    total_matched: f64,
}
