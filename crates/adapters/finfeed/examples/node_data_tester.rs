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

//! Poll one Polymarket outcome through FinFeed for 60 seconds, then stop the node.
//!
//! Requires `FINFEED_API_KEY` with prediction-market access. Uses at most eight managed GET
//! attempts with ten-second pacing and no application retries. Local attempts are not billing units.
//! Quotes and trades share a poll; L2 book deltas replace the whole book at each accepted snapshot.
//! For longer runs or more outcomes, size the finite allowance and both connection deadlines
//! using `docs/integrations/finfeed.md`; this example keeps its bounded one-outcome defaults.
//!
//! Run from the repository root:
//! `CARGO_BUILD_JOBS=8 cargo run --locked -p nautilus-finfeed --example node_data_tester -- <MARKET_ID>`
//! Pass an active Polymarket outcome ID.

use std::time::Duration;

use nautilus_common::enums::Environment;
use nautilus_finfeed::{
    common::{consts::FINFEED_CLIENT_ID, symbol::to_instrument_id},
    config::FinFeedDataClientConfig,
    factories::FinFeedDataClientFactory,
    http::client::FinFeedHttpConfig,
    instruments::FinFeedInstrumentSelection,
};
use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::TraderId;
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

const RUN_SECONDS: u64 = 60;
const USAGE: &str = "Usage: node_data_tester MARKET_ID (an active Polymarket outcome ID)";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let market_id = match args.as_slice() {
        [help] if matches!(help.as_str(), "-h" | "--help") => {
            println!("{USAGE}");
            return Ok(());
        }
        [market_id] if !market_id.starts_with('-') => market_id,
        _ => anyhow::bail!("{USAGE}"),
    };

    let exchange_id = "POLYMARKET";
    let instrument_id = to_instrument_id(exchange_id, market_id)?;

    let mut config = FinFeedDataClientConfig {
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: exchange_id.into(),
            market_ids: vec![market_id.as_str().into()],
        }],
        http: FinFeedHttpConfig {
            attempt_budget: 8,
            timeout_secs: 10,
            max_concurrent_requests: 1,
            min_request_interval_ms: 10_000,
            ..Default::default()
        },
        poll_interval_ms: 60_000,
        poll_limit: 100,
        connect_timeout_secs: 45,
        ..Default::default()
    };

    config.http.retry.max_retries = 0;
    config.http.retry.operation_timeout_ms = Some(30_000);
    config.http.retry.max_elapsed_ms = Some(120_000);

    let mut node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Live)?
        .with_name("FINFEED-DATA-TESTER")
        .with_load_state(false)
        .with_save_state(false)
        .with_reconciliation(false)
        .with_timeout_connection_secs(60)
        .with_timeout_portfolio_secs(0)
        .with_delay_post_stop_secs(0)
        .add_data_client(
            None,
            Box::new(FinFeedDataClientFactory::new()),
            Box::new(config),
        )?
        .build()?;
    let tester = DataTesterConfig::builder()
        .client_id(*FINFEED_CLIENT_ID)
        .instrument_ids(vec![instrument_id])
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .subscribe_book_deltas(true)
        .stats_interval_secs(0)
        .log_data(true)
        .build()?;
    node.add_actor(DataTester::new(tester))?;

    let handle = node.handle();
    let run = node.run();
    tokio::pin!(run);

    tokio::select! {
        result = &mut run => result?,
        () = tokio::time::sleep(Duration::from_secs(RUN_SECONDS)) => {
            handle.stop();
            run.await?;
        }
    }

    Ok(())
}
