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

//! Example demonstrating live execution testing with the Interactive Brokers adapter.
//!
//! Run live smoke with:
//! `cargo run --example ib-exec-tester --package nautilus-interactive-brokers --features examples`
//!
//! Run embedded config unit tests with:
//! `cargo test --example ib-exec-tester --package nautilus-interactive-brokers --features examples`
//!
//! Edit the constants below to change the TWS/Gateway connection, target
//! instrument, order size, and exec spec profile.
//!
//! Required environment variable:
//! - `NAUTILUS_IB_ACCOUNT_ID` is your IB account, for example `U1234567`.

use std::{collections::HashSet, env, time::Duration};

use nautilus_common::{enums::Environment, live::get_runtime};
use nautilus_interactive_brokers::{
    common::consts::{DEFAULT_CLIENT_ID, DEFAULT_HOST, DEFAULT_TWS_PORT, IB},
    config::{
        InteractiveBrokersDataClientConfig, InteractiveBrokersExecClientConfig,
        InteractiveBrokersInstrumentProviderConfig, MarketDataType,
    },
    factories::{InteractiveBrokersDataClientFactory, InteractiveBrokersExecutionClientFactory},
};
use nautilus_live::{config::LiveExecEngineConfig, node::LiveNode};
use nautilus_model::{
    enums::{OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

// Each variant is exercised by the tests and selected by editing EXEC_SPEC_PROFILE,
// but only the default is constructed in a non-test build
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IbExecSpecProfile {
    Lifecycle,
    CancelModify,
    Rejection,
    Options,
    UnsupportedFlags,
}

const TRADER_ID: &str = "IB-EXEC-TESTER-001";
const NODE_NAME: &str = "IB-EXEC-TESTER-001";
const STRATEGY_ID: &str = "IB-EXEC-TESTER-001";
const HOST: &str = DEFAULT_HOST;
const PORT: u16 = DEFAULT_TWS_PORT;
const CLIENT_ID: i32 = DEFAULT_CLIENT_ID;
const INSTRUMENT_ID: &str = "AAPL=STK.SMART";
const MARKET_DATA_TYPE: &str = "realtime";
const ORDER_QTY: &str = "1";
const AUTO_STOP_SECS: u64 = 0;
const EXEC_SPEC_PROFILE: IbExecSpecProfile = IbExecSpecProfile::Lifecycle;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let account_id_raw = env::var("NAUTILUS_IB_ACCOUNT_ID")?;
    let account_id = account_id_from_ib_account(&account_id_raw);
    let trader_id = TraderId::from(TRADER_ID);
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let market_data_type = parse_market_data_type(MARKET_DATA_TYPE);
    let order_qty = Quantity::from(ORDER_QTY);

    let data_config = InteractiveBrokersDataClientConfig {
        host: HOST.to_string(),
        port: PORT,
        client_id: CLIENT_ID,
        market_data_type,
        instrument_provider: instrument_provider_config(instrument_id),
        ..Default::default()
    };

    let exec_config = InteractiveBrokersExecClientConfig {
        host: HOST.to_string(),
        port: PORT,
        client_id: CLIENT_ID,
        account_id: Some(account_id_raw),
        instrument_provider: instrument_provider_config(instrument_id),
        ..Default::default()
    };
    let exec_engine_config = LiveExecEngineConfig {
        open_check_interval_secs: Some(10.0),
        position_check_interval_secs: Some(30.0),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(NODE_NAME.to_string())
        .with_exec_engine_config(exec_engine_config)
        .with_delay_post_stop_secs(5)
        .with_reconciliation(true)
        .add_data_client(
            None,
            Box::new(InteractiveBrokersDataClientFactory::new()),
            Box::new(data_config),
        )?
        .add_exec_client(
            None,
            Box::new(InteractiveBrokersExecutionClientFactory::new(
                trader_id, account_id,
            )),
            Box::new(exec_config),
        )?
        .build()?;

    let tester_config = exec_tester_config_for_profile(
        EXEC_SPEC_PROFILE,
        instrument_id,
        ClientId::new(IB),
        order_qty,
    );

    node.add_strategy(ExecTester::new(tester_config))?;
    schedule_auto_stop(&node, AUTO_STOP_SECS);
    node.run().await?;

    Ok(())
}

fn account_id_from_ib_account(account_id: &str) -> AccountId {
    if account_id.contains('-') {
        AccountId::from(account_id)
    } else {
        AccountId::from(format!("{IB}-{account_id}"))
    }
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

fn exec_tester_config_for_profile(
    profile: IbExecSpecProfile,
    instrument_id: InstrumentId,
    client_id: ClientId,
    order_qty: Quantity,
) -> ExecTesterConfig {
    let builder = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from(STRATEGY_ID)),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .log_data(false);

    match profile {
        IbExecSpecProfile::Lifecycle => builder
            .open_position_on_start_qty(order_qty.as_decimal())
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .close_positions_on_stop(true)
            .build(),
        IbExecSpecProfile::CancelModify => builder
            .enable_limit_buys(true)
            .enable_limit_sells(true)
            .modify_orders_to_maintain_tob_offset(true)
            .modify_stop_orders_to_maintain_offset(true)
            .use_individual_cancels_on_stop(true)
            .build(),
        IbExecSpecProfile::Rejection => builder
            .enable_limit_buys(true)
            .enable_limit_sells(true)
            .test_reject_post_only(true)
            .build(),
        IbExecSpecProfile::Options => builder
            .open_position_on_start_qty(order_qty.as_decimal())
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .close_positions_on_stop(true)
            .build(),
        IbExecSpecProfile::UnsupportedFlags => builder
            .open_position_on_start_qty(order_qty.as_decimal())
            .enable_limit_buys(true)
            .enable_limit_sells(false)
            .limit_time_in_force(TimeInForce::Ioc)
            .stop_order_type(OrderType::TrailingStopMarket)
            .test_reject_post_only(true)
            .test_reject_reduce_only(true)
            .use_quote_quantity(true)
            .use_batch_cancel_on_stop(true)
            .build(),
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("AAPL=STK.SMART")
    }

    fn config(profile: IbExecSpecProfile) -> ExecTesterConfig {
        exec_tester_config_for_profile(
            profile,
            instrument_id(),
            ClientId::new(IB),
            Quantity::from("1"),
        )
    }

    #[rstest::rstest]
    fn test_lifecycle_exec_spec_profile_opens_and_closes_position() {
        let config = config(IbExecSpecProfile::Lifecycle);

        assert_eq!(config.open_position_on_start_qty, Some(Decimal::ONE));
        assert!(!config.enable_limit_buys);
        assert!(!config.enable_limit_sells);
        assert!(config.close_positions_on_stop);
    }

    #[rstest::rstest]
    fn test_cancel_modify_exec_spec_profile_enables_amend_and_cancel_paths() {
        let config = config(IbExecSpecProfile::CancelModify);

        assert!(config.enable_limit_buys);
        assert!(config.enable_limit_sells);
        assert!(config.modify_orders_to_maintain_tob_offset);
        assert!(config.modify_stop_orders_to_maintain_offset);
        assert!(config.use_individual_cancels_on_stop);
    }

    #[rstest::rstest]
    fn test_rejection_exec_spec_profile_exercises_post_only_rejection() {
        let config = config(IbExecSpecProfile::Rejection);

        assert!(config.enable_limit_buys);
        assert!(config.enable_limit_sells);
        assert!(config.test_reject_post_only);
    }

    #[rstest::rstest]
    fn test_options_exec_spec_profile_reuses_lifecycle_order_path() {
        let config = config(IbExecSpecProfile::Options);

        assert_eq!(config.open_position_on_start_qty, Some(Decimal::ONE));
        assert!(!config.enable_limit_buys);
        assert!(!config.enable_limit_sells);
        assert!(config.close_positions_on_stop);
    }

    #[rstest::rstest]
    fn test_unsupported_flags_exec_spec_profile_exercises_rejection_and_batch_cancel_flags() {
        let config = config(IbExecSpecProfile::UnsupportedFlags);

        assert_eq!(config.open_position_on_start_qty, Some(Decimal::ONE));
        assert_eq!(config.limit_time_in_force, Some(TimeInForce::Ioc));
        assert_eq!(config.stop_order_type, OrderType::TrailingStopMarket);
        assert!(config.test_reject_post_only);
        assert!(config.test_reject_reduce_only);
        assert!(config.use_quote_quantity);
        assert!(config.use_batch_cancel_on_stop);
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
