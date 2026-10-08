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
//! Build the node and tester configuration with:
//! `cargo run --example ib-exec-tester --package nautilus-interactive-brokers --features examples`
//!
//! Set `NAUTILUS_IB_RUN=1` to connect. Set `NAUTILUS_IB_LIVE_ORDERS=1` as a second opt-in to
//! submit orders. Select an order profile with `NAUTILUS_IB_EXEC_PROFILE`.
//!
//! Run embedded config unit tests with:
//! `cargo test --example ib-exec-tester --package nautilus-interactive-brokers --features examples`
//!
//! Edit the constants below to change the TWS/Gateway connection and order size. The tester
//! trades the live quarterly ES contract so it works outside stock market hours.
//!
//! Required environment variable:
//! - `NAUTILUS_IB_ACCOUNT_ID` is your IB account, for example `U1234567`

use std::{collections::HashSet, env, time::Duration};

use nautilus_common::{enums::Environment, live::get_runtime};
use nautilus_interactive_brokers::{
    common::consts::{DEFAULT_CLIENT_ID, DEFAULT_HOST, DEFAULT_TWS_PORT, IB},
    config::{
        InteractiveBrokersDataClientConfig, InteractiveBrokersExecutionClientConfig,
        InteractiveBrokersInstrumentProviderConfig, MarketDataType,
    },
    factories::{InteractiveBrokersDataClientFactory, InteractiveBrokersExecutionClientFactory},
};
use nautilus_live::{
    config::{LiveExecutionEngineConfig, RoutingConfig},
    node::LiveNode,
};
use nautilus_model::{
    enums::{OrderType, TimeInForce},
    identifiers::{ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;

#[path = "contracts/active_future.rs"]
mod active_future;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IbExecutionSpecProfile {
    Lifecycle,
    CancelModify,
    Rejection,
    Options,
    Stop,
    StopLimit,
    Trailing,
    Bracket,
    UnsupportedFlags,
}

// WARNING: With `DRY_RUN = false`, this tester submits orders to the configured
// environment and may use real funds. Set `DRY_RUN = true` to connect without
// submitting orders or sending shutdown cancel/close commands.
const DRY_RUN: bool = false;
const TRADER_ID: &str = "IB-EXEC-TESTER-001";
const NODE_NAME: &str = "IB-EXEC-TESTER-001";
const STRATEGY_ID: &str = "IB-EXEC-TESTER-001";
const HOST: &str = DEFAULT_HOST;
const PORT: u16 = DEFAULT_TWS_PORT;
const CLIENT_ID: i32 = DEFAULT_CLIENT_ID;
// Delayed data streams without a real-time subscription; override with
// `NAUTILUS_IB_MARKET_DATA_TYPE` when the account is entitled to real-time data.
const MARKET_DATA_TYPE: &str = "delayed";
const ORDER_QTY: &str = "1";
const AUTO_STOP_SECS: u64 = 0;
const EXEC_SPEC_PROFILE: &str = "lifecycle";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = env_enabled("NAUTILUS_IB_RUN");
    let live_orders = env_enabled("NAUTILUS_IB_LIVE_ORDERS");
    validate_live_opt_ins(run, live_orders)?;

    let account_id_raw = match env::var("NAUTILUS_IB_ACCOUNT_ID") {
        Ok(value) => value,
        Err(e) if run => return Err(e.into()),
        Err(_) => "U1234567".to_string(),
    };
    let trader_id = TraderId::from(TRADER_ID);
    let instrument_id = active_future::es_future_instrument_id();
    let market_data_type = parse_market_data_type(
        &env::var("NAUTILUS_IB_MARKET_DATA_TYPE").unwrap_or_else(|_| MARKET_DATA_TYPE.to_string()),
    );
    let order_qty = Quantity::from(ORDER_QTY);
    let profile = parse_exec_spec_profile(
        &env::var("NAUTILUS_IB_EXEC_PROFILE").unwrap_or_else(|_| EXEC_SPEC_PROFILE.to_string()),
    );

    let routing = RoutingConfig::builder().default(true).build();

    let data_config = InteractiveBrokersDataClientConfig {
        host: HOST.to_string(),
        port: PORT,
        client_id: CLIENT_ID,
        market_data_type,
        instrument_provider: instrument_provider_config(instrument_id),
        ..Default::default()
    };

    let exec_config = InteractiveBrokersExecutionClientConfig {
        host: HOST.to_string(),
        port: PORT,
        client_id: CLIENT_ID,
        account_id: Some(account_id_raw),
        instrument_provider: instrument_provider_config(instrument_id),
        ..Default::default()
    };
    let exec_engine_config = LiveExecutionEngineConfig {
        open_check_interval_secs: Some(10.0),
        position_check_interval_secs: Some(30.0),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(NODE_NAME.to_string())
        .with_exec_engine_config(exec_engine_config)
        .with_delay_post_stop_secs(5)
        .with_reconciliation(true)
        .add_data_client_with_routing(
            None,
            Box::new(InteractiveBrokersDataClientFactory::new()),
            Box::new(data_config),
            routing.clone(),
        )?
        .add_exec_client_with_routing(
            None,
            Box::new(InteractiveBrokersExecutionClientFactory::new()),
            Box::new(exec_config),
            routing,
        )?
        .build()?;

    let tester_config = exec_tester_config_for_profile(
        profile,
        instrument_id,
        ClientId::new(IB),
        order_qty,
        live_orders,
    );

    node.add_strategy(ExecTester::new(tester_config))?;
    if !run {
        println!("Built Interactive Brokers exec tester node. Set NAUTILUS_IB_RUN=1 to connect.");
        return Ok(());
    }

    schedule_auto_stop(&node, AUTO_STOP_SECS);
    node.run().await?;

    Ok(())
}

fn env_enabled(name: &str) -> bool {
    env::var(name).is_ok_and(|value| value == "1")
}

fn validate_live_opt_ins(run: bool, live_orders: bool) -> Result<(), std::io::Error> {
    if live_orders && !run {
        return Err(std::io::Error::other(
            "NAUTILUS_IB_LIVE_ORDERS=1 requires NAUTILUS_IB_RUN=1",
        ));
    }

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

fn parse_exec_spec_profile(value: &str) -> IbExecutionSpecProfile {
    match value {
        "lifecycle" => IbExecutionSpecProfile::Lifecycle,
        "cancel-modify" | "cancel_modify" => IbExecutionSpecProfile::CancelModify,
        "rejection" => IbExecutionSpecProfile::Rejection,
        "options" => IbExecutionSpecProfile::Options,
        "stop" => IbExecutionSpecProfile::Stop,
        "stop-limit" | "stop_limit" => IbExecutionSpecProfile::StopLimit,
        "trailing" => IbExecutionSpecProfile::Trailing,
        "bracket" => IbExecutionSpecProfile::Bracket,
        "unsupported-flags" | "unsupported_flags" => IbExecutionSpecProfile::UnsupportedFlags,
        value => panic!("invalid NAUTILUS_IB_EXEC_PROFILE={value}"),
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
    profile: IbExecutionSpecProfile,
    instrument_id: InstrumentId,
    client_id: ClientId,
    order_qty: Quantity,
    live_orders: bool,
) -> ExecTesterConfig {
    let builder = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from(STRATEGY_ID)),
            use_uuid_client_order_ids: true,
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .dry_run(DRY_RUN || !live_orders)
        .log_data(false);

    match profile {
        IbExecutionSpecProfile::Lifecycle => builder
            .open_position_on_start_qty(order_qty.as_decimal())
            .open_position_on_first_quote(true)
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .build()
            .unwrap(),
        IbExecutionSpecProfile::CancelModify => builder
            .enable_limit_buys(true)
            .enable_limit_sells(true)
            .modify_orders_to_maintain_tob_offset(true)
            .modify_stop_orders_to_maintain_offset(true)
            .use_individual_cancels_on_stop(true)
            .build()
            .unwrap(),
        IbExecutionSpecProfile::Rejection => builder
            .enable_limit_buys(true)
            .enable_limit_sells(true)
            .test_reject_post_only(true)
            .build()
            .unwrap(),
        IbExecutionSpecProfile::Options => builder
            .open_position_on_start_qty(order_qty.as_decimal())
            .open_position_on_first_quote(true)
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .build()
            .unwrap(),
        IbExecutionSpecProfile::Stop => builder
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .enable_stop_buys(true)
            .enable_stop_sells(true)
            .stop_order_type(OrderType::StopMarket)
            .modify_stop_orders_to_maintain_offset(true)
            .build()
            .unwrap(),
        IbExecutionSpecProfile::StopLimit => builder
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .enable_stop_buys(true)
            .enable_stop_sells(true)
            .stop_order_type(OrderType::StopLimit)
            .stop_limit_offset_ticks(25)
            .build()
            .unwrap(),
        IbExecutionSpecProfile::Trailing => builder
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .enable_stop_buys(true)
            .enable_stop_sells(true)
            .stop_order_type(OrderType::TrailingStopMarket)
            .trailing_offset(rust_decimal::Decimal::new(25, 0))
            .build()
            .unwrap(),
        IbExecutionSpecProfile::Bracket => builder
            .enable_limit_buys(true)
            .enable_limit_sells(true)
            .enable_brackets(true)
            .bracket_entry_order_type(OrderType::Limit)
            .bracket_offset_ticks(500)
            .build()
            .unwrap(),
        IbExecutionSpecProfile::UnsupportedFlags => builder
            .open_position_on_start_qty(order_qty.as_decimal())
            .enable_limit_buys(true)
            .enable_limit_sells(false)
            .limit_time_in_force(TimeInForce::Ioc)
            .stop_order_type(OrderType::TrailingStopMarket)
            .test_reject_post_only(true)
            .test_reject_reduce_only(true)
            .use_quote_quantity(true)
            .use_batch_cancel_on_stop(true)
            .build()
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("AAPL=STK.SMART")
    }

    fn config(profile: IbExecutionSpecProfile) -> ExecTesterConfig {
        exec_tester_config_for_profile(
            profile,
            instrument_id(),
            ClientId::new(IB),
            Quantity::from("1"),
            true,
        )
    }

    #[rstest::rstest]
    fn test_default_connection_opt_in_keeps_tester_in_dry_run() {
        let config = exec_tester_config_for_profile(
            IbExecutionSpecProfile::Lifecycle,
            instrument_id(),
            ClientId::new(IB),
            Quantity::from("1"),
            false,
        );

        assert!(config.dry_run);
        assert!(validate_live_opt_ins(true, false).is_ok());
        assert_eq!(
            validate_live_opt_ins(false, true).unwrap_err().to_string(),
            "NAUTILUS_IB_LIVE_ORDERS=1 requires NAUTILUS_IB_RUN=1",
        );
    }

    #[rstest::rstest]
    fn test_lifecycle_exec_spec_profile_opens_and_closes_position() {
        let config = config(IbExecutionSpecProfile::Lifecycle);

        assert_eq!(config.open_position_on_start_qty, Some(Decimal::ONE));
        assert!(config.open_position_on_first_quote);
        assert!(!config.enable_limit_buys);
        assert!(!config.enable_limit_sells);
        assert!(config.close_positions_on_stop);
    }

    #[rstest::rstest]
    fn test_cancel_modify_exec_spec_profile_enables_amend_and_cancel_paths() {
        let config = config(IbExecutionSpecProfile::CancelModify);

        assert!(config.enable_limit_buys);
        assert!(config.enable_limit_sells);
        assert!(config.modify_orders_to_maintain_tob_offset);
        assert!(config.modify_stop_orders_to_maintain_offset);
        assert!(config.use_individual_cancels_on_stop);
    }

    #[rstest::rstest]
    fn test_rejection_exec_spec_profile_exercises_post_only_rejection() {
        let config = config(IbExecutionSpecProfile::Rejection);

        assert!(config.enable_limit_buys);
        assert!(config.enable_limit_sells);
        assert!(config.test_reject_post_only);
    }

    #[rstest::rstest]
    fn test_options_exec_spec_profile_reuses_lifecycle_order_path() {
        let config = config(IbExecutionSpecProfile::Options);

        assert_eq!(config.open_position_on_start_qty, Some(Decimal::ONE));
        assert!(config.open_position_on_first_quote);
        assert!(!config.enable_limit_buys);
        assert!(!config.enable_limit_sells);
        assert!(config.close_positions_on_stop);
    }

    #[rstest::rstest]
    fn test_stop_exec_spec_profile_enables_modifiable_stop_orders() {
        let config = config(IbExecutionSpecProfile::Stop);

        assert!(!config.enable_limit_buys);
        assert!(!config.enable_limit_sells);
        assert!(config.enable_stop_buys);
        assert!(config.enable_stop_sells);
        assert_eq!(config.stop_order_type, OrderType::StopMarket);
        assert!(config.modify_stop_orders_to_maintain_offset);
    }

    #[rstest::rstest]
    fn test_stop_limit_exec_spec_profile_enables_stop_limit_orders() {
        let config = config(IbExecutionSpecProfile::StopLimit);

        assert!(config.enable_stop_buys);
        assert!(config.enable_stop_sells);
        assert_eq!(config.stop_order_type, OrderType::StopLimit);
        assert_eq!(config.stop_limit_offset_ticks, Some(25));
    }

    #[rstest::rstest]
    fn test_trailing_exec_spec_profile_enables_trailing_stop_orders() {
        let config = config(IbExecutionSpecProfile::Trailing);

        assert!(config.enable_stop_buys);
        assert!(config.enable_stop_sells);
        assert_eq!(config.stop_order_type, OrderType::TrailingStopMarket);
        assert_eq!(config.trailing_offset, Some(Decimal::new(25, 0)));
    }

    #[rstest::rstest]
    fn test_bracket_exec_spec_profile_enables_bracket_orders() {
        let config = config(IbExecutionSpecProfile::Bracket);

        assert!(config.enable_limit_buys);
        assert!(config.enable_limit_sells);
        assert!(config.enable_brackets);
        assert_eq!(config.bracket_entry_order_type, OrderType::Limit);
        assert_eq!(config.bracket_offset_ticks, 500);
    }

    #[rstest::rstest]
    fn test_unsupported_flags_exec_spec_profile_exercises_rejection_and_batch_cancel_flags() {
        let config = config(IbExecutionSpecProfile::UnsupportedFlags);

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
