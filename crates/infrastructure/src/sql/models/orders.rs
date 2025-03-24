// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSnapshot,
        OrderSubmitted, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use sqlx::{FromRow, Row, postgres::PgRow};
use ustr::Ustr;

use crate::sql::models::enums::TrailingOffsetTypeModel;

pub struct OrderEventAnyModel(pub OrderEventAny);
pub struct OrderAcceptedModel(pub OrderAccepted);
pub struct OrderCancelRejectedModel(pub OrderCancelRejected);
pub struct OrderCanceledModel(pub OrderCanceled);
pub struct OrderDeniedModel(pub OrderDenied);
pub struct OrderEmulatedModel(pub OrderEmulated);
pub struct OrderExpiredModel(pub OrderExpired);
pub struct OrderFilledModel(pub OrderFilled);
pub struct OrderInitializedModel(pub OrderInitialized);
pub struct OrderModifyRejectedModel(pub OrderModifyRejected);
pub struct OrderPendingCancelModel(pub OrderPendingCancel);
pub struct OrderPendingUpdateModel(pub OrderPendingUpdate);
pub struct OrderRejectedModel(pub OrderRejected);
pub struct OrderReleasedModel(pub OrderReleased);
pub struct OrderSubmittedModel(pub OrderSubmitted);
pub struct OrderTriggeredModel(pub OrderTriggered);
pub struct OrderUpdatedModel(pub OrderUpdated);
pub struct OrderSnapshotModel(pub OrderSnapshot);

impl<'r> FromRow<'r, PgRow> for OrderEventAnyModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let kind = row.get::<String, _>("kind");
        if kind == "OrderAccepted" {
            let model = OrderAcceptedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Accepted(model.0)))
        } else if kind == "OrderCancelRejected" {
            let model = OrderCancelRejectedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::CancelRejected(model.0)))
        } else if kind == "OrderCanceled" {
            let model = OrderCanceledModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Canceled(model.0)))
        } else if kind == "OrderDenied" {
            let model = OrderDeniedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Denied(model.0)))
        } else if kind == "OrderEmulated" {
            let model = OrderEmulatedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Emulated(model.0)))
        } else if kind == "OrderExpired" {
            let model = OrderExpiredModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Expired(model.0)))
        } else if kind == "OrderFilled" {
            let model = OrderFilledModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Filled(model.0)))
        } else if kind == "OrderInitialized" {
            let model = OrderInitializedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Initialized(model.0)))
        } else if kind == "OrderModifyRejected" {
            let model = OrderModifyRejectedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::ModifyRejected(model.0)))
        } else if kind == "OrderPendingCancel" {
            let model = OrderPendingCancelModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::PendingCancel(model.0)))
        } else if kind == "OrderPendingUpdate" {
            let model = OrderPendingUpdateModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::PendingUpdate(model.0)))
        } else if kind == "OrderRejected" {
            let model = OrderRejectedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Rejected(model.0)))
        } else if kind == "OrderReleased" {
            let model = OrderReleasedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Released(model.0)))
        } else if kind == "OrderSubmitted" {
            let model = OrderSubmittedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Submitted(model.0)))
        } else if kind == "OrderTriggered" {
            let model = OrderTriggeredModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Triggered(model.0)))
        } else if kind == "OrderUpdated" {
            let model = OrderUpdatedModel::from_row(row)?;
            Ok(OrderEventAnyModel(OrderEventAny::Updated(model.0)))
        } else {
            panic!("Unknown order event kind: {kind} in Postgres transformation")
        }
    }
}

impl<'r> FromRow<'r, PgRow> for OrderInitializedModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let client_order_id = row
            .try_get::<&str, _>("client_order_id")
            .map(ClientOrderId::from)?;
        let trader_id = row.try_get::<&str, _>("trader_id").map(TraderId::from)?;
        let strategy_id = row
            .try_get::<&str, _>("strategy_id")
            .map(StrategyId::from)?;
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(InstrumentId::from)?;
        let order_type = row
            .try_get::<&str, _>("order_type")
            .map(|x| OrderType::from_str(x).unwrap())?;
        let order_side = row
            .try_get::<&str, _>("order_side")
            .map(|x| OrderSide::from_str(x).unwrap())?;
        let quantity = row.try_get::<&str, _>("quantity").map(Quantity::from)?;
        let time_in_force = row
            .try_get::<&str, _>("time_in_force")
            .map(|x| TimeInForce::from_str(x).unwrap())?;
        let post_only = row.try_get::<bool, _>("post_only")?;
        let reduce_only = row.try_get::<bool, _>("reduce_only")?;
        let quote_quantity = row.try_get::<bool, _>("quote_quantity")?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;
        let price = row
            .try_get::<Option<&str>, _>("price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_price = row
            .try_get::<Option<&str>, _>("trigger_price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_type = row
            .try_get::<Option<&str>, _>("trigger_type")
            .ok()
            .and_then(|x| x.map(|x| TriggerType::from_str(x).unwrap()));
        let limit_offset = row
            .try_get::<Option<&str>, _>("limit_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset = row
            .try_get::<Option<&str>, _>("trailing_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset_type = row
            .try_get::<Option<TrailingOffsetTypeModel>, _>("trailing_offset_type")
            .ok()
            .and_then(|x| x.map(|x| x.0));
        let expire_time = row
            .try_get::<Option<&str>, _>("expire_time")
            .ok()
            .and_then(|x| x.map(UnixNanos::from));
        let display_qty = row
            .try_get::<Option<&str>, _>("display_qty")
            .ok()
            .and_then(|x| x.map(Quantity::from));
        let emulation_trigger = row
            .try_get::<Option<&str>, _>("emulation_trigger")
            .ok()
            .and_then(|x| x.map(|x| TriggerType::from_str(x).unwrap()));
        let trigger_instrument_id = row
            .try_get::<Option<&str>, _>("trigger_instrument_id")
            .ok()
            .and_then(|x| x.map(InstrumentId::from));
        let contingency_type = row
            .try_get::<Option<&str>, _>("contingency_type")
            .ok()
            .and_then(|x| x.map(|x| ContingencyType::from_str(x).unwrap()));
        let order_list_id = row
            .try_get::<Option<&str>, _>("order_list_id")
            .ok()
            .and_then(|x| x.map(OrderListId::from));
        let linked_order_ids = row
            .try_get::<Vec<String>, _>("linked_order_ids")
            .ok()
            .map(|x| x.iter().map(|x| ClientOrderId::from(x.as_str())).collect());
        let parent_order_id = row
            .try_get::<Option<&str>, _>("parent_order_id")
            .ok()
            .and_then(|x| x.map(ClientOrderId::from));
        let exec_algorithm_id = row
            .try_get::<Option<&str>, _>("exec_algorithm_id")
            .ok()
            .and_then(|x| x.map(ExecAlgorithmId::from));
        let exec_algorithm_params: Option<IndexMap<Ustr, Ustr>> = row
            .try_get::<Option<serde_json::Value>, _>("exec_algorithm_params")
            .ok()
            .and_then(|x| x.map(|x| serde_json::from_value::<IndexMap<String, String>>(x).unwrap()))
            .map(|x| {
                x.into_iter()
                    .map(|(k, v)| (Ustr::from(k.as_str()), Ustr::from(v.as_str())))
                    .collect()
            });
        let exec_spawn_id = row
            .try_get::<Option<&str>, _>("exec_spawn_id")
            .ok()
            .and_then(|x| x.map(ClientOrderId::from));
        let tags: Option<Vec<Ustr>> = row
            .try_get::<Option<serde_json::Value>, _>("tags")
            .ok()
            .and_then(|x| x.map(|x| serde_json::from_value::<Vec<String>>(x).unwrap()))
            .map(|x| x.into_iter().map(|x| Ustr::from(x.as_str())).collect());
        let order_event = OrderInitialized::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            post_only,
            reduce_only,
            quote_quantity,
            reconciliation,
            event_id,
            ts_event,
            ts_init,
            price,
            trigger_price,
            trigger_type,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            expire_time,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
        );
        Ok(OrderInitializedModel(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderAcceptedModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let trader_id = row.try_get::<&str, _>("trader_id").map(TraderId::from)?;
        let strategy_id = row
            .try_get::<&str, _>("strategy_id")
            .map(StrategyId::from)?;
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(InstrumentId::from)?;
        let client_order_id = row
            .try_get::<&str, _>("client_order_id")
            .map(ClientOrderId::from)?;
        let venue_order_id = row
            .try_get::<&str, _>("venue_order_id")
            .map(VenueOrderId::from)?;
        let account_id = row.try_get::<&str, _>("account_id").map(AccountId::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let order_event = OrderAccepted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            false,
        );
        Ok(OrderAcceptedModel(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderCancelRejectedModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderCanceledModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderDeniedModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderEmulatedModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderExpiredModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderFilledModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let trader_id = row.try_get::<&str, _>("trader_id").map(TraderId::from)?;
        let strategy_id = row
            .try_get::<&str, _>("strategy_id")
            .map(StrategyId::from)?;
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(InstrumentId::from)?;
        let client_order_id = row
            .try_get::<&str, _>("client_order_id")
            .map(ClientOrderId::from)?;
        let venue_order_id = row
            .try_get::<&str, _>("venue_order_id")
            .map(VenueOrderId::from)?;
        let account_id = row.try_get::<&str, _>("account_id").map(AccountId::from)?;
        let trade_id = row.try_get::<&str, _>("trade_id").map(TradeId::from)?;
        let order_side = row
            .try_get::<&str, _>("order_side")
            .map(|x| OrderSide::from_str(x).unwrap())?;
        let order_type = row
            .try_get::<&str, _>("order_type")
            .map(|x| OrderType::from_str(x).unwrap())?;
        let last_px = row.try_get::<&str, _>("last_px").map(Price::from)?;
        let last_qty = row.try_get::<&str, _>("last_qty").map(Quantity::from)?;
        let currency = row.try_get::<&str, _>("currency").map(Currency::from)?;
        let liquidity_side = row
            .try_get::<&str, _>("liquidity_side")
            .map(|x| LiquiditySide::from_str(x).unwrap())?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let position_id = row
            .try_get::<Option<&str>, _>("position_id")
            .map(|x| x.map(PositionId::from))?;
        let commission = row
            .try_get::<Option<&str>, _>("commission")
            .map(|x| x.map(|x| Money::from_str(x).unwrap()))?;
        let order_event = OrderFilled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            trade_id,
            order_side,
            order_type,
            last_qty,
            last_px,
            currency,
            liquidity_side,
            event_id,
            ts_event,
            ts_init,
            false,
            position_id,
            commission,
        );
        Ok(OrderFilledModel(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderModifyRejectedModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderPendingCancelModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderPendingUpdateModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderRejectedModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderReleasedModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderSubmittedModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let trader_id = row.try_get::<&str, _>("trader_id").map(TraderId::from)?;
        let strategy_id = row
            .try_get::<&str, _>("strategy_id")
            .map(StrategyId::from)?;
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(InstrumentId::from)?;
        let client_order_id = row
            .try_get::<&str, _>("client_order_id")
            .map(ClientOrderId::from)?;
        let account_id = row.try_get::<&str, _>("account_id").map(AccountId::from)?;
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let order_event = OrderSubmitted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
        );
        Ok(OrderSubmittedModel(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderTriggeredModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderUpdatedModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!()
    }
}

impl<'r> FromRow<'r, PgRow> for OrderSnapshotModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let trader_id = row.try_get::<&str, _>("trader_id").map(TraderId::from)?;
        let strategy_id = row
            .try_get::<&str, _>("strategy_id")
            .map(StrategyId::from)?;
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(InstrumentId::from)?;
        let client_order_id = row
            .try_get::<&str, _>("client_order_id")
            .map(ClientOrderId::from)?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")
            .ok()
            .and_then(|x| x.map(VenueOrderId::from));
        let position_id = row
            .try_get::<Option<&str>, _>("position_id")
            .ok()
            .and_then(|x| x.map(PositionId::from));
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")
            .ok()
            .and_then(|x| x.map(AccountId::from));
        let last_trade_id = row
            .try_get::<Option<&str>, _>("last_trade_id")
            .ok()
            .and_then(|x| x.map(TradeId::from));
        let order_type = row
            .try_get::<&str, _>("order_type")
            .map(|x| OrderType::from_str(x).expect("Invalid `OrderType`"))?;
        let order_side = row
            .try_get::<&str, _>("order_side")
            .map(|x| OrderSide::from_str(x).expect("Invalid `OrderSide`"))?;
        let quantity = row.try_get::<&str, _>("quantity").map(Quantity::from)?;
        let price = row
            .try_get::<Option<&str>, _>("price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_price = row
            .try_get::<Option<&str>, _>("trigger_price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_type = row
            .try_get::<Option<&str>, _>("trigger_type")
            .ok()
            .and_then(|x| x.map(|x| TriggerType::from_str(x).expect("Invalid `TriggerType`")));
        let limit_offset = row
            .try_get::<Option<&str>, _>("limit_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset = row
            .try_get::<Option<&str>, _>("trailing_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset_type = row
            .try_get::<Option<TrailingOffsetTypeModel>, _>("trailing_offset_type")
            .ok()
            .and_then(|x| x.map(|x| x.0));
        let time_in_force = row
            .try_get::<&str, _>("time_in_force")
            .map(|x| TimeInForce::from_str(x).expect("Invalid `TimeInForce`"))?;
        let expire_time = row
            .try_get::<Option<&str>, _>("expire_time")
            .ok()
            .and_then(|x| x.map(UnixNanos::from));
        let filled_qty = row.try_get::<&str, _>("filled_qty").map(Quantity::from)?;
        let liquidity_side = row
            .try_get::<Option<&str>, _>("liquidity_side")
            .ok()
            .and_then(|x| x.map(|x| LiquiditySide::from_str(x).expect("Invalid `LiquiditySide`")));
        let avg_px = row.try_get::<Option<f64>, _>("avg_px").ok().flatten();
        let slippage = row.try_get::<Option<f64>, _>("slippage").ok().flatten();
        let commissions = row
            .try_get::<Option<Vec<String>>, _>("commissions")?
            .map_or_else(Vec::new, |c| {
                c.into_iter().map(|s| Money::from(&s)).collect()
            });
        let status = row
            .try_get::<&str, _>("status")
            .map(|x| OrderStatus::from_str(x).expect("Invalid `OrderStatus`"))?;
        let is_post_only = row.try_get::<bool, _>("is_post_only")?;
        let is_reduce_only = row.try_get::<bool, _>("is_reduce_only")?;
        let is_quote_quantity = row.try_get::<bool, _>("is_quote_quantity")?;
        let display_qty = row
            .try_get::<Option<&str>, _>("display_qty")
            .ok()
            .and_then(|x| x.map(Quantity::from));
        let emulation_trigger = row
            .try_get::<Option<&str>, _>("emulation_trigger")
            .ok()
            .and_then(|x| x.map(|x| TriggerType::from_str(x).expect("Invalid `TriggerType`")));
        let trigger_instrument_id = row
            .try_get::<Option<&str>, _>("trigger_instrument_id")
            .ok()
            .and_then(|x| x.map(InstrumentId::from));
        let contingency_type = row
            .try_get::<Option<&str>, _>("contingency_type")
            .ok()
            .and_then(|x| {
                x.map(|x| ContingencyType::from_str(x).expect("Invalid `ContingencyType`"))
            });
        let order_list_id = row
            .try_get::<Option<&str>, _>("order_list_id")
            .ok()
            .and_then(|x| x.map(OrderListId::from));
        let linked_order_ids = row
            .try_get::<Option<Vec<String>>, _>("linked_order_ids")
            .ok()
            .and_then(|ids| ids.map(|ids| ids.into_iter().map(ClientOrderId::from).collect()));
        let parent_order_id = row
            .try_get::<Option<&str>, _>("parent_order_id")
            .ok()
            .and_then(|x| x.map(ClientOrderId::from));
        let exec_algorithm_id = row
            .try_get::<Option<&str>, _>("exec_algorithm_id")
            .ok()
            .and_then(|x| x.map(ExecAlgorithmId::from));
        let exec_algorithm_params: Option<IndexMap<Ustr, Ustr>> = row
            .try_get::<Option<serde_json::Value>, _>("exec_algorithm_params")
            .ok()
            .and_then(|x| {
                x.map(|x| {
                    serde_json::from_value::<IndexMap<String, String>>(x)
                        .expect("Invalid exec algorithm params")
                })
            })
            .map(|x| {
                x.into_iter()
                    .map(|(k, v)| (Ustr::from(k.as_str()), Ustr::from(v.as_str())))
                    .collect()
            });
        let exec_spawn_id = row
            .try_get::<Option<&str>, _>("exec_spawn_id")
            .ok()
            .and_then(|x| x.map(ClientOrderId::from));
        let tags = row
            .try_get::<Option<serde_json::Value>, _>("tags")
            .ok()
            .flatten()
            .and_then(|tags_value| {
                serde_json::from_value::<Vec<String>>(tags_value)
                    .ok()
                    .map(|vec| {
                        vec.into_iter()
                            .map(|tag| Ustr::from(tag.as_str()))
                            .collect::<Vec<Ustr>>()
                    })
            });
        let init_id = row.try_get::<&str, _>("init_id").map(UUID4::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;
        let ts_last = row.try_get::<String, _>("ts_last").map(UnixNanos::from)?;

        let snapshot = OrderSnapshot {
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            position_id,
            account_id,
            last_trade_id,
            order_type,
            order_side,
            quantity,
            price,
            trigger_price,
            trigger_type,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
            time_in_force,
            expire_time,
            filled_qty,
            liquidity_side,
            avg_px,
            slippage,
            commissions,
            status,
            is_post_only,
            is_reduce_only,
            is_quote_quantity,
            display_qty,
            emulation_trigger,
            trigger_instrument_id,
            contingency_type,
            order_list_id,
            linked_order_ids,
            parent_order_id,
            exec_algorithm_id,
            exec_algorithm_params,
            exec_spawn_id,
            tags,
            init_id,
            ts_init,
            ts_last,
        };

        Ok(OrderSnapshotModel(snapshot))
    }
}
