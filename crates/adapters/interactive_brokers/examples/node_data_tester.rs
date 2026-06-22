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

//! Example demonstrating live data testing with the Interactive Brokers adapter.
//!
//! Run live smoke with:
//! `cargo run --example ib-data-tester --package nautilus-interactive-brokers --features examples`
//!
//! Run embedded config unit tests with:
//! `cargo test --example ib-data-tester --package nautilus-interactive-brokers --features examples`
//!
//! Edit the constants below to change the TWS/Gateway connection, target
//! instrument, market data type, and data spec profile. The adapter connects to
//! a locally running TWS or IB Gateway, so no credential environment variables
//! are required.

use std::{collections::HashSet, time::Duration};

use nautilus_common::{enums::Environment, live::get_runtime};
use nautilus_interactive_brokers::{
    common::consts::{DEFAULT_CLIENT_ID, DEFAULT_HOST, DEFAULT_TWS_PORT, IB},
    config::{
        InteractiveBrokersDataClientConfig, InteractiveBrokersInstrumentProviderConfig,
        MarketDataType,
    },
    factories::InteractiveBrokersDataClientFactory,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::bar::BarType,
    identifiers::{ClientId, InstrumentId, TraderId},
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

// Each variant is exercised by the tests and selected by editing DATA_SPEC_PROFILE,
// but only the default is constructed in a non-test build
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IbDataSpecProfile {
    Supported,
    UnsupportedSurfaces,
    Options,
}

const TRADER_ID: &str = "IB-DATA-TESTER-001";
const NODE_NAME: &str = "IB-DATA-TESTER-001";
const HOST: &str = DEFAULT_HOST;
const PORT: u16 = DEFAULT_TWS_PORT;
const CLIENT_ID: i32 = DEFAULT_CLIENT_ID;
const INSTRUMENT_ID: &str = "AAPL=STK.SMART";
const MARKET_DATA_TYPE: &str = "realtime";
const AUTO_STOP_SECS: u64 = 0;
const DATA_SPEC_PROFILE: IbDataSpecProfile = IbDataSpecProfile::Supported;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let market_data_type = parse_market_data_type(MARKET_DATA_TYPE);
    let bar_type = BarType::from(format!("{instrument_id}-1-MINUTE-LAST-EXTERNAL").as_str());

    let data_config = InteractiveBrokersDataClientConfig {
        host: HOST.to_string(),
        port: PORT,
        client_id: CLIENT_ID,
        market_data_type,
        instrument_provider: instrument_provider_config(instrument_id),
        ..Default::default()
    };

    let mut node = LiveNode::builder(TraderId::from(TRADER_ID), Environment::Live)?
        .with_name(NODE_NAME.to_string())
        .with_delay_post_stop_secs(2)
        .add_data_client(
            None,
            Box::new(InteractiveBrokersDataClientFactory::new()),
            Box::new(data_config),
        )?
        .build()?;

    let tester_config = data_tester_config_for_profile(
        DATA_SPEC_PROFILE,
        ClientId::new(IB),
        instrument_id,
        bar_type,
    );

    node.add_actor(DataTester::new(tester_config))?;
    schedule_auto_stop(&node, AUTO_STOP_SECS);
    node.run().await?;

    Ok(())
}

fn parse_market_data_type(value: &str) -> MarketDataType {
    match value {
        "realtime" => MarketDataType::Realtime,
        "frozen" => MarketDataType::Frozen,
        "delayed" => MarketDataType::Delayed,
        "delayed-frozen" | "delayed_frozen" => MarketDataType::DelayedFrozen,
        value => panic!("invalid NAUTILUS_IB_MARKET_DATA_TYPE={value}"),
    }
}

fn instrument_provider_config(
    instrument_id: InstrumentId,
) -> InteractiveBrokersInstrumentProviderConfig {
    let mut load_ids = HashSet::new();
    load_ids.insert(instrument_id);

    InteractiveBrokersInstrumentProviderConfig {
        load_ids,
        ..Default::default()
    }
}

fn schedule_auto_stop(node: &LiveNode, delay_secs: u64) {
    if delay_secs == 0 {
        return;
    }

    let handle = node.handle();

    get_runtime().spawn(async move {
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        handle.stop();
    });
}

fn data_tester_config_for_profile(
    profile: IbDataSpecProfile,
    client_id: ClientId,
    instrument_id: InstrumentId,
    bar_type: BarType,
) -> DataTesterConfig {
    let builder = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(vec![instrument_id])
        .bar_types(vec![bar_type])
        .request_instruments(true);

    match profile {
        IbDataSpecProfile::Supported => builder
            .subscribe_book_deltas(true)
            .subscribe_quotes(true)
            .subscribe_trades(true)
            .subscribe_bars(true)
            .request_quotes(true)
            .request_trades(true)
            .request_bars(true)
            .build()
            .unwrap(),
        IbDataSpecProfile::UnsupportedSurfaces => builder
            .subscribe_book_depth(true)
            .subscribe_instrument_status(true)
            .subscribe_instrument_close(true)
            .request_book_snapshot(true)
            .book_depth(10)
            .stats_interval_secs(0)
            .build()
            .unwrap(),
        IbDataSpecProfile::Options => builder
            .subscribe_quotes(true)
            .subscribe_option_greeks(true)
            .request_quotes(true)
            .stats_interval_secs(0)
            .build()
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("AAPL=STK.SMART")
    }

    fn bar_type() -> BarType {
        BarType::from("AAPL=STK.SMART-1-MINUTE-LAST-EXTERNAL")
    }

    #[rstest::rstest]
    fn test_supported_data_spec_profile_covers_baseline_streams_and_requests() {
        let config = data_tester_config_for_profile(
            IbDataSpecProfile::Supported,
            ClientId::new(IB),
            instrument_id(),
            bar_type(),
        );

        assert!(config.request_instruments);
        assert!(config.subscribe_book_deltas);
        assert!(config.subscribe_quotes);
        assert!(config.subscribe_trades);
        assert!(config.subscribe_bars);
        assert!(config.request_quotes);
        assert!(config.request_trades);
        assert!(config.request_bars);
    }

    #[rstest::rstest]
    fn test_unsupported_data_spec_profile_exercises_missing_surfaces() {
        let config = data_tester_config_for_profile(
            IbDataSpecProfile::UnsupportedSurfaces,
            ClientId::new(IB),
            instrument_id(),
            bar_type(),
        );

        assert!(config.request_instruments);
        assert!(config.subscribe_book_depth);
        assert!(config.subscribe_instrument_status);
        assert!(config.subscribe_instrument_close);
        assert!(config.request_book_snapshot);
        assert_eq!(config.book_depth, Some(10));
    }

    #[rstest::rstest]
    fn test_options_data_spec_profile_exercises_option_greeks_surface() {
        let config = data_tester_config_for_profile(
            IbDataSpecProfile::Options,
            ClientId::new(IB),
            instrument_id(),
            bar_type(),
        );

        assert!(config.request_instruments);
        assert!(config.subscribe_quotes);
        assert!(config.subscribe_option_greeks);
        assert!(config.request_quotes);
    }

    #[rstest::rstest]
    fn test_instrument_provider_config_preloads_test_instrument() {
        let config = instrument_provider_config(instrument_id());

        assert_eq!(config.load_ids.len(), 1);
        assert!(config.load_ids.contains(&instrument_id()));
    }

    #[rstest::rstest]
    #[case("realtime", MarketDataType::Realtime)]
    #[case("frozen", MarketDataType::Frozen)]
    #[case("delayed", MarketDataType::Delayed)]
    #[case("delayed-frozen", MarketDataType::DelayedFrozen)]
    #[case("delayed_frozen", MarketDataType::DelayedFrozen)]
    fn test_parse_market_data_type(#[case] value: &str, #[case] expected: MarketDataType) {
        assert_eq!(parse_market_data_type(value), expected);
    }
}
