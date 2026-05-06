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
//! Environment variables:
//! - `NAUTILUS_IB_ACCOUNT_ID` is required, for example `U1234567`.
//! - `NAUTILUS_IB_HOST` defaults to `127.0.0.1`.
//! - `NAUTILUS_IB_PORT` defaults to `7497` for paper TWS.
//! - `NAUTILUS_IB_CLIENT_ID` defaults to `1`.
//! - `NAUTILUS_IB_INSTRUMENT_ID` defaults to `AAPL=STK.SMART`.
//! - `NAUTILUS_IB_MARKET_DATA_TYPE` defaults to `realtime`.
//! - `NAUTILUS_IB_ORDER_QTY` defaults to `1`.
//! - `NAUTILUS_IB_AUTO_STOP_SECS` defaults to `0` (run until stopped).
//! - `NAUTILUS_IB_EXEC_SPEC_PROFILE` defaults to `lifecycle`.
//!   Supported values: `lifecycle`, `cancel-modify`, `rejection`, `options`, `unsupported-flags`.

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
use nautilus_live::node::LiveNode;
use nautilus_model::{
    enums::{OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IbExecSpecProfile {
    Lifecycle,
    CancelModify,
    Rejection,
    Options,
    UnsupportedFlags,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = env_string("NAUTILUS_IB_HOST", DEFAULT_HOST);
    let port = env_u16("NAUTILUS_IB_PORT", DEFAULT_TWS_PORT)?;
    let client_id = env_i32("NAUTILUS_IB_CLIENT_ID", DEFAULT_CLIENT_ID)?;
    let account_id_raw = env::var("NAUTILUS_IB_ACCOUNT_ID")?;
    let account_id = account_id_from_ib_account(&account_id_raw);
    let trader_id = TraderId::from("IB-EXEC-TESTER-001");
    let instrument_id =
        InstrumentId::from(env_string("NAUTILUS_IB_INSTRUMENT_ID", "AAPL=STK.SMART"));
    let market_data_type = market_data_type_from_env();
    let order_qty = Quantity::from(env_string("NAUTILUS_IB_ORDER_QTY", "1"));
    let profile = exec_spec_profile_from_env();
    let auto_stop_secs = env_u64("NAUTILUS_IB_AUTO_STOP_SECS", 0)?;

    let data_config = InteractiveBrokersDataClientConfig {
        host: host.clone(),
        port,
        client_id,
        market_data_type,
        instrument_provider: instrument_provider_config(instrument_id),
        ..Default::default()
    };

    let exec_config = InteractiveBrokersExecClientConfig {
        host,
        port,
        client_id,
        account_id: Some(account_id_raw),
        instrument_provider: instrument_provider_config(instrument_id),
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name("IB-EXEC-TESTER-001".to_string())
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

    let tester_config =
        exec_tester_config_for_profile(profile, instrument_id, ClientId::new(IB), order_qty);

    node.add_strategy(ExecTester::new(tester_config))?;
    schedule_auto_stop(&node, auto_stop_secs);
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

fn env_string(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_i32(key: &str, default: i32) -> Result<i32, Box<dyn std::error::Error>> {
    Ok(env::var(key).map_or(Ok(default), |value| value.parse())?)
}

fn env_u16(key: &str, default: u16) -> Result<u16, Box<dyn std::error::Error>> {
    Ok(env::var(key).map_or(Ok(default), |value| value.parse())?)
}

fn env_u64(key: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(env::var(key).map_or(Ok(default), |value| value.parse())?)
}

fn market_data_type_from_env() -> MarketDataType {
    parse_market_data_type(&env_string("NAUTILUS_IB_MARKET_DATA_TYPE", "realtime"))
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

fn exec_spec_profile_from_env() -> IbExecSpecProfile {
    match env_string("NAUTILUS_IB_EXEC_SPEC_PROFILE", "lifecycle").as_str() {
        "lifecycle" => IbExecSpecProfile::Lifecycle,
        "cancel-modify" => IbExecSpecProfile::CancelModify,
        "rejection" => IbExecSpecProfile::Rejection,
        "options" => IbExecSpecProfile::Options,
        "unsupported-flags" => IbExecSpecProfile::UnsupportedFlags,
        value => panic!("invalid NAUTILUS_IB_EXEC_SPEC_PROFILE={value}"),
    }
}

fn exec_tester_config_for_profile(
    profile: IbExecSpecProfile,
    instrument_id: InstrumentId,
    client_id: ClientId,
    order_qty: Quantity,
) -> ExecTesterConfig {
    let builder = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("IB-EXEC-TESTER-001")),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .log_data(false);

    match profile {
        IbExecSpecProfile::Lifecycle => builder
            .open_position_on_start_qty(Decimal::ONE)
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
            .open_position_on_start_qty(Decimal::ONE)
            .enable_limit_buys(false)
            .enable_limit_sells(false)
            .close_positions_on_stop(true)
            .build(),
        IbExecSpecProfile::UnsupportedFlags => builder
            .open_position_on_start_qty(Decimal::ONE)
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
