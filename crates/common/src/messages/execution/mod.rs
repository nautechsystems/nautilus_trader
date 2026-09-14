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

//! Execution specific messages such as order commands.

pub mod cancel;
pub mod modify;
pub mod query;
pub mod report;
pub mod submit;

use std::fmt::Display;

use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, StrategyId},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use strum::Display as StrumDisplay;

pub use self::{
    cancel::{BatchCancelOrders, CancelAllOrders, CancelOrder},
    modify::{BatchModifyOrders, ModifyOrder},
    query::{QueryAccount, QueryOrder},
    report::{
        GenerateExecutionMassStatus, GenerateExecutionMassStatusBuilder, GenerateFillReports,
        GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReportBuilder,
        GenerateOrderStatusReports, GenerateOrderStatusReportsBuilder,
        GeneratePositionStatusReports, GeneratePositionStatusReportsBuilder,
    },
    submit::{SubmitOrder, SubmitOrderList},
};

/// Parameter indicating that a conditional order should close the whole position at trigger time.
pub const PARAMS_CLOSE_POSITION: &str = "close_position";

/// Execution report variants for reconciliation.
#[derive(Clone, Debug)]
pub enum ExecutionReport {
    Order(Box<OrderStatusReport>),
    Fill(Box<FillReport>),
    OrderWithFills(Box<OrderStatusReport>, Vec<FillReport>),
    Position(Box<PositionStatusReport>),
    MassStatus(Box<ExecutionMassStatus>),
}

impl Display for ExecutionReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Order(report) => write!(f, "{report}"),
            Self::Fill(report) => write!(f, "{report}"),
            Self::OrderWithFills(report, fills) => {
                write!(f, "OrderWithFills(order={report}, fills=[")?;
                for (index, fill) in fills.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{fill}")?;
                }
                f.write_str("])")
            }
            Self::Position(report) => write!(f, "{report}"),
            Self::MassStatus(report) => write!(f, "{report}"),
        }
    }
}

/// An execution command sent to an execution client.
///
/// Serializes as the contained command object. Deserialization requires its string `type` field to
/// select the variant.
#[expect(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, StrumDisplay)]
#[serde(untagged)]
pub enum TradingCommand {
    #[strum(transparent)]
    SubmitOrder(SubmitOrder),
    #[strum(transparent)]
    SubmitOrderList(SubmitOrderList),
    #[strum(transparent)]
    ModifyOrder(ModifyOrder),
    #[strum(transparent)]
    ModifyOrders(BatchModifyOrders),
    #[strum(transparent)]
    CancelOrder(CancelOrder),
    #[strum(transparent)]
    CancelOrders(BatchCancelOrders),
    #[strum(transparent)]
    CancelAllOrders(CancelAllOrders),
    #[strum(transparent)]
    QueryOrder(QueryOrder),
    #[strum(transparent)]
    QueryAccount(QueryAccount),
}

#[derive(Deserialize)]
enum TradingCommandType {
    SubmitOrder,
    SubmitOrderList,
    ModifyOrder,
    BatchModifyOrders,
    CancelOrder,
    BatchCancelOrders,
    CancelAllOrders,
    QueryOrder,
    QueryAccount,
}

#[derive(Deserialize)]
struct TradingCommandHeader {
    #[serde(rename = "type")]
    command_type: TradingCommandType,
}

impl<'de> Deserialize<'de> for TradingCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let command_type = TradingCommandHeader::deserialize(&value)
            .map_err(D::Error::custom)?
            .command_type;

        match command_type {
            TradingCommandType::SubmitOrder => serde_json::from_value(value)
                .map(Self::SubmitOrder)
                .map_err(D::Error::custom),
            TradingCommandType::SubmitOrderList => serde_json::from_value(value)
                .map(Self::SubmitOrderList)
                .map_err(D::Error::custom),
            TradingCommandType::ModifyOrder => serde_json::from_value(value)
                .map(Self::ModifyOrder)
                .map_err(D::Error::custom),
            TradingCommandType::BatchModifyOrders => serde_json::from_value(value)
                .map(Self::ModifyOrders)
                .map_err(D::Error::custom),
            TradingCommandType::CancelOrder => serde_json::from_value(value)
                .map(Self::CancelOrder)
                .map_err(D::Error::custom),
            TradingCommandType::BatchCancelOrders => serde_json::from_value(value)
                .map(Self::CancelOrders)
                .map_err(D::Error::custom),
            TradingCommandType::CancelAllOrders => serde_json::from_value(value)
                .map(Self::CancelAllOrders)
                .map_err(D::Error::custom),
            TradingCommandType::QueryOrder => serde_json::from_value(value)
                .map(Self::QueryOrder)
                .map_err(D::Error::custom),
            TradingCommandType::QueryAccount => serde_json::from_value(value)
                .map(Self::QueryAccount)
                .map_err(D::Error::custom),
        }
    }
}

impl TradingCommand {
    #[must_use]
    pub const fn client_id(&self) -> Option<ClientId> {
        match self {
            Self::SubmitOrder(command) => command.client_id,
            Self::SubmitOrderList(command) => command.client_id,
            Self::ModifyOrder(command) => command.client_id,
            Self::ModifyOrders(command) => command.client_id,
            Self::CancelOrder(command) => command.client_id,
            Self::CancelOrders(command) => command.client_id,
            Self::CancelAllOrders(command) => command.client_id,
            Self::QueryOrder(command) => command.client_id,
            Self::QueryAccount(command) => command.client_id,
        }
    }

    /// Returns the instrument ID for the command.
    ///
    /// # Panics
    ///
    /// Panics if the command is `QueryAccount` which does not have an instrument ID.
    #[must_use]
    pub const fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::SubmitOrder(command) => command.instrument_id,
            Self::SubmitOrderList(command) => command.instrument_id,
            Self::ModifyOrder(command) => command.instrument_id,
            Self::ModifyOrders(command) => command.instrument_id,
            Self::CancelOrder(command) => command.instrument_id,
            Self::CancelOrders(command) => command.instrument_id,
            Self::CancelAllOrders(command) => command.instrument_id,
            Self::QueryOrder(command) => command.instrument_id,
            Self::QueryAccount(_) => panic!("No instrument ID for command"),
        }
    }

    #[must_use]
    pub const fn ts_init(&self) -> UnixNanos {
        match self {
            Self::SubmitOrder(command) => command.ts_init,
            Self::SubmitOrderList(command) => command.ts_init,
            Self::ModifyOrder(command) => command.ts_init,
            Self::ModifyOrders(command) => command.ts_init,
            Self::CancelOrder(command) => command.ts_init,
            Self::CancelOrders(command) => command.ts_init,
            Self::CancelAllOrders(command) => command.ts_init,
            Self::QueryOrder(command) => command.ts_init,
            Self::QueryAccount(command) => command.ts_init,
        }
    }

    #[must_use]
    pub const fn strategy_id(&self) -> Option<StrategyId> {
        match self {
            Self::SubmitOrder(command) => Some(command.strategy_id),
            Self::SubmitOrderList(command) => Some(command.strategy_id),
            Self::ModifyOrder(command) => Some(command.strategy_id),
            Self::ModifyOrders(command) => Some(command.strategy_id),
            Self::CancelOrder(command) => Some(command.strategy_id),
            Self::CancelOrders(command) => Some(command.strategy_id),
            Self::CancelAllOrders(command) => Some(command.strategy_id),
            Self::QueryOrder(command) => Some(command.strategy_id),
            Self::QueryAccount(_) => None,
        }
    }

    #[must_use]
    pub const fn params(&self) -> Option<&Params> {
        match self {
            Self::SubmitOrder(command) => command.params.as_ref(),
            Self::SubmitOrderList(command) => command.params.as_ref(),
            Self::ModifyOrder(command) => command.params.as_ref(),
            Self::ModifyOrders(command) => command.params.as_ref(),
            Self::CancelOrder(command) => command.params.as_ref(),
            Self::CancelOrders(command) => command.params.as_ref(),
            Self::CancelAllOrders(command) => command.params.as_ref(),
            Self::QueryOrder(command) => command.params.as_ref(),
            Self::QueryAccount(command) => command.params.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
        events::OrderInitialized,
        identifiers::{AccountId, OrderListId, TradeId, TraderId, VenueOrderId},
        orders::OrderList,
        reports::{FillReport, OrderStatusReport},
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn trading_commands() -> Vec<TradingCommand> {
        let trader_id = TraderId::from("TRADER-001");
        let client_id = Some(ClientId::from("EXTERNAL"));
        let strategy_id = StrategyId::from("STRATEGY-001");
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let ts_init = UnixNanos::from(1_000_000_000);
        let order_init = OrderInitialized::default();
        let client_order_id = order_init.client_order_id;
        let submit_order = SubmitOrder::new(
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_init.clone(),
            None,
            None,
            None,
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            ts_init,
            None,
        );
        let order_list = OrderList::new(
            OrderListId::from("OL-001"),
            instrument_id,
            strategy_id,
            vec![client_order_id],
            ts_init,
        );
        let submit_order_list = SubmitOrderList::new(
            trader_id,
            client_id,
            strategy_id,
            order_list,
            vec![order_init],
            None,
            None,
            None,
            UUID4::from("00000000-0000-4000-8000-000000000002"),
            ts_init,
            None,
        );
        let modify_order = ModifyOrder::new(
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            client_order_id,
            None,
            None,
            None,
            None,
            UUID4::from("00000000-0000-4000-8000-000000000003"),
            ts_init,
            None,
            None,
        );
        let cancel_order = CancelOrder::new(
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            client_order_id,
            None,
            UUID4::from("00000000-0000-4000-8000-000000000005"),
            ts_init,
            None,
            None,
        );

        vec![
            TradingCommand::SubmitOrder(submit_order),
            TradingCommand::SubmitOrderList(submit_order_list),
            TradingCommand::ModifyOrder(modify_order.clone()),
            TradingCommand::ModifyOrders(BatchModifyOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                vec![modify_order],
                UUID4::from("00000000-0000-4000-8000-000000000004"),
                ts_init,
                None,
                None,
            )),
            TradingCommand::CancelOrder(cancel_order.clone()),
            TradingCommand::CancelOrders(BatchCancelOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                vec![cancel_order],
                UUID4::from("00000000-0000-4000-8000-000000000006"),
                ts_init,
                None,
                None,
            )),
            TradingCommand::CancelAllOrders(CancelAllOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                None,
                UUID4::from("00000000-0000-4000-8000-000000000007"),
                ts_init,
                None,
                None,
            )),
            TradingCommand::QueryOrder(QueryOrder::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                client_order_id,
                None,
                UUID4::from("00000000-0000-4000-8000-000000000008"),
                ts_init,
                None,
                None,
            )),
            TradingCommand::QueryAccount(QueryAccount::new(
                trader_id,
                client_id,
                AccountId::from("SIM-001"),
                UUID4::from("00000000-0000-4000-8000-000000000009"),
                ts_init,
                None,
                None,
            )),
        ]
    }

    fn order_status_report() -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUD/USD.SIM"),
            None,
            VenueOrderId::from("V-001"),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::PartiallyFilled,
            Quantity::from("2"),
            Quantity::from("1"),
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(3),
            Some(UUID4::from("00000000-0000-4000-8000-000000000010")),
        )
    }

    fn fill_report(trade_id: &str) -> FillReport {
        FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUD/USD.SIM"),
            VenueOrderId::from("V-001"),
            TradeId::from(trade_id),
            OrderSide::Buy,
            Quantity::from("1"),
            Price::from("1.25"),
            Money::new(0.01, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(4),
            UnixNanos::from(5),
            Some(UUID4::from("00000000-0000-4000-8000-000000000011")),
        )
    }

    #[rstest]
    fn trading_command_round_trips_each_variant() {
        for command in trading_commands() {
            let json = serde_json::to_vec(&command).expect("command must serialize as JSON");
            let json_decoded =
                serde_json::from_slice::<TradingCommand>(&json).expect("command JSON must decode");
            let msgpack =
                rmp_serde::to_vec_named(&command).expect("command must serialize as MsgPack");
            let msgpack_decoded = rmp_serde::from_slice::<TradingCommand>(&msgpack)
                .expect("command MsgPack must decode");

            assert_eq!(json_decoded, command);
            assert_eq!(msgpack_decoded, command);
        }
    }

    #[rstest]
    fn trading_command_display_delegates_to_inner() {
        for command in trading_commands() {
            let expected = match &command {
                TradingCommand::SubmitOrder(command) => command.to_string(),
                TradingCommand::SubmitOrderList(command) => command.to_string(),
                TradingCommand::ModifyOrder(command) => command.to_string(),
                TradingCommand::ModifyOrders(command) => command.to_string(),
                TradingCommand::CancelOrder(command) => command.to_string(),
                TradingCommand::CancelOrders(command) => command.to_string(),
                TradingCommand::CancelAllOrders(command) => command.to_string(),
                TradingCommand::QueryOrder(command) => command.to_string(),
                TradingCommand::QueryAccount(command) => command.to_string(),
            };

            assert_eq!(command.to_string(), expected);
        }
    }

    #[rstest]
    fn execution_report_order_with_fills_uses_inner_display() {
        let order_report = order_status_report();
        let fill_report_1 = fill_report("T-001");
        let fill_report_2 = fill_report("T-002");
        let expected = format!(
            "OrderWithFills(order={order_report}, fills=[{fill_report_1}, {fill_report_2}])"
        );
        let report = ExecutionReport::OrderWithFills(
            Box::new(order_report),
            vec![fill_report_1, fill_report_2],
        );

        assert_eq!(report.to_string(), expected);
    }

    #[rstest]
    fn trading_command_rejects_unknown_type() {
        let error = serde_json::from_value::<TradingCommand>(serde_json::json!({
            "type": "UnknownCommand",
        }))
        .expect_err("unknown command type must be rejected");

        assert_eq!(
            error.to_string(),
            "unknown variant `UnknownCommand`, expected one of `SubmitOrder`, `SubmitOrderList`, \
             `ModifyOrder`, `BatchModifyOrders`, `CancelOrder`, `BatchCancelOrders`, \
             `CancelAllOrders`, `QueryOrder`, `QueryAccount`",
        );
    }
}
