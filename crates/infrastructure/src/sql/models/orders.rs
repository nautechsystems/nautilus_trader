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

use std::str::FromStr;

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSnapshot, OrderSubmitted, OrderTriggered, OrderUpdated,
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

use crate::sql::models::enums::TrailingOffsetTypePg;

#[derive(Debug)]
pub struct OrderEventAnyRow(pub OrderEventAny);

#[derive(Debug)]
pub struct OrderAcceptedRow(pub OrderAccepted);

#[derive(Debug)]
pub struct OrderCancelRejectedRow(pub OrderCancelRejected);

#[derive(Debug)]
pub struct OrderCanceledRow(pub OrderCanceled);

#[derive(Debug)]
pub struct OrderDeniedRow(pub OrderDenied);

#[derive(Debug)]
pub struct OrderEmulatedRow(pub OrderEmulated);

#[derive(Debug)]
pub struct OrderExpiredRow(pub OrderExpired);

#[derive(Debug)]
pub struct OrderFilledRow(pub OrderFilled);

#[derive(Debug)]
pub struct OrderFillVoidedRow(pub OrderFillVoided);

#[derive(Debug)]
pub struct OrderInitializedRow(pub OrderInitialized);

#[derive(Debug)]
pub struct OrderModifyRejectedRow(pub OrderModifyRejected);

#[derive(Debug)]
pub struct OrderPendingCancelRow(pub OrderPendingCancel);

#[derive(Debug)]
pub struct OrderPendingUpdateRow(pub OrderPendingUpdate);

#[derive(Debug)]
pub struct OrderRejectedRow(pub OrderRejected);

#[derive(Debug)]
pub struct OrderReleasedRow(pub OrderReleased);

#[derive(Debug)]
pub struct OrderSubmittedRow(pub OrderSubmitted);

#[derive(Debug)]
pub struct OrderTriggeredRow(pub OrderTriggered);

#[derive(Debug)]
pub struct OrderUpdatedRow(pub OrderUpdated);

#[derive(Debug)]
pub struct OrderSnapshotRow(pub OrderSnapshot);

impl<'r> FromRow<'r, PgRow> for OrderEventAnyRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let kind = row.get::<String, _>("kind");
        if kind == "OrderAccepted" {
            let row = OrderAcceptedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Accepted(row.0)))
        } else if kind == "OrderCancelRejected" {
            let row = OrderCancelRejectedRow::from_row(row)?;
            Ok(Self(OrderEventAny::CancelRejected(row.0)))
        } else if kind == "OrderCanceled" {
            let row = OrderCanceledRow::from_row(row)?;
            Ok(Self(OrderEventAny::Canceled(row.0)))
        } else if kind == "OrderDenied" {
            let row = OrderDeniedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Denied(row.0)))
        } else if kind == "OrderEmulated" {
            let row = OrderEmulatedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Emulated(row.0)))
        } else if kind == "OrderExpired" {
            let row = OrderExpiredRow::from_row(row)?;
            Ok(Self(OrderEventAny::Expired(row.0)))
        } else if kind == "OrderFillVoided" {
            let row = OrderFillVoidedRow::from_row(row)?;
            Ok(Self(OrderEventAny::FillVoided(row.0)))
        } else if kind == "OrderFilled" {
            let row = OrderFilledRow::from_row(row)?;
            Ok(Self(OrderEventAny::Filled(row.0)))
        } else if kind == "OrderInitialized" {
            let row = OrderInitializedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Initialized(row.0)))
        } else if kind == "OrderModifyRejected" {
            let row = OrderModifyRejectedRow::from_row(row)?;
            Ok(Self(OrderEventAny::ModifyRejected(row.0)))
        } else if kind == "OrderPendingCancel" {
            let row = OrderPendingCancelRow::from_row(row)?;
            Ok(Self(OrderEventAny::PendingCancel(row.0)))
        } else if kind == "OrderPendingUpdate" {
            let row = OrderPendingUpdateRow::from_row(row)?;
            Ok(Self(OrderEventAny::PendingUpdate(row.0)))
        } else if kind == "OrderRejected" {
            let row = OrderRejectedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Rejected(row.0)))
        } else if kind == "OrderReleased" {
            let row = OrderReleasedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Released(row.0)))
        } else if kind == "OrderSubmitted" {
            let row = OrderSubmittedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Submitted(row.0)))
        } else if kind == "OrderTriggered" {
            let row = OrderTriggeredRow::from_row(row)?;
            Ok(Self(OrderEventAny::Triggered(row.0)))
        } else if kind == "OrderUpdated" {
            let row = OrderUpdatedRow::from_row(row)?;
            Ok(Self(OrderEventAny::Updated(row.0)))
        } else {
            Err(sqlx::Error::Decode(
                format!("Unknown order event kind: {kind} in Postgres transformation").into(),
            ))
        }
    }
}

impl<'r> FromRow<'r, PgRow> for OrderInitializedRow {
    #[expect(
        clippy::too_many_lines,
        reason = "SQL row mapping mirrors the full order initialized event constructor"
    )]
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
        let activation_price = row
            .try_get::<Option<&str>, _>("activation_price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_price = row
            .try_get::<Option<&str>, _>("trigger_price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_type = row
            .try_get::<Option<&str>, _>("trigger_type")
            .ok()
            .and_then(parse_trigger_type);
        let limit_offset = row
            .try_get::<Option<&str>, _>("limit_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset = row
            .try_get::<Option<&str>, _>("trailing_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset_type = row
            .try_get::<Option<TrailingOffsetTypePg>, _>("trailing_offset_type")
            .ok()
            .flatten()
            .and_then(|value| value.0);
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
            .and_then(parse_trigger_type);
        let trigger_instrument_id = row
            .try_get::<Option<&str>, _>("trigger_instrument_id")
            .ok()
            .and_then(|x| x.map(InstrumentId::from));
        let contingency_type = row
            .try_get::<Option<&str>, _>("contingency_type")
            .ok()
            .and_then(parse_contingency_type);
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
        let tags = tags_from_row(row);
        let mut order_event = OrderInitialized::new_checked(
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
            activation_price,
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
        )
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        order_event.causation_id = causation_id_from_row(row)?;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderAcceptedRow {
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
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderAccepted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderCancelRejectedRow {
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
        let reason = row.try_get::<&str, _>("reason").map(Ustr::from)?;
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderCancelRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderCanceledRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let reason = row.try_get::<Option<&str>, _>("reason")?.map(Ustr::from);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderCanceled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
            reason,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderDeniedRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reason = row.try_get::<&str, _>("reason").map(Ustr::from)?;
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderDenied::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event,
            ts_init,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderEmulatedRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderEmulated::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderExpiredRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderExpired::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderFilledRow {
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
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let info = decode_info(row)?;
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderFilled::new(
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
            reconciliation,
            position_id,
            commission,
            info,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderFillVoidedRow {
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
        let correction_id = row
            .try_get::<Option<&str>, _>("correction_id")?
            .map(Ustr::from)
            .ok_or_else(|| {
                sqlx::Error::Decode(
                    "OrderFillVoided row has no correction_id; it predates the column and \
                     the value cannot be recovered"
                        .into(),
                )
            })?;
        let trade_id = row.try_get::<&str, _>("trade_id").map(TradeId::from)?;
        let voided_qty = row.try_get::<&str, _>("quantity").map(Quantity::from)?;
        let commission_voided = row
            .try_get::<Option<&str>, _>("commission")?
            .map(|x| Money::from_str(x).map_err(|e| sqlx::Error::Decode(e.into())))
            .transpose()?;
        let order_side = OrderSide::from_str(row.try_get::<&str, _>("order_side")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let order_type = OrderType::from_str(row.try_get::<&str, _>("order_type")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let last_px = row.try_get::<&str, _>("last_px").map(Price::from)?;
        let currency = row.try_get::<&str, _>("currency").map(Currency::from)?;
        let liquidity_side = LiquiditySide::from_str(row.try_get::<&str, _>("liquidity_side")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let position_id = row
            .try_get::<Option<&str>, _>("position_id")?
            .map(PositionId::from);
        let reason = row.try_get::<Option<&str>, _>("reason")?.map(Ustr::from);
        let info = decode_info(row)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let is_reopened = row
            .try_get::<Option<bool>, _>("is_reopened")?
            .unwrap_or(false);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderFillVoided::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            correction_id,
            trade_id,
            voided_qty,
            commission_voided,
            order_side,
            order_type,
            last_px,
            currency,
            liquidity_side,
            position_id,
            reason,
            info,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            is_reopened,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderModifyRejectedRow {
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
        let reason = row.try_get::<&str, _>("reason").map(Ustr::from)?;
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderModifyRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderPendingCancelRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderPendingCancel::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderPendingUpdateRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderPendingUpdate::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderRejectedRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let account_id = row.try_get::<&str, _>("account_id").map(AccountId::from)?;
        let reason = row.try_get::<&str, _>("reason").map(Ustr::from)?;
        // Rows written before this column existed decode as NULL; false is its meaning
        let due_post_only = row
            .try_get::<Option<bool>, _>("due_post_only")?
            .unwrap_or(false);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            reason,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            due_post_only,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderReleasedRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let released_price = row
            .try_get::<Option<&str>, _>("released_price")?
            .map(Price::from)
            .ok_or_else(|| {
                sqlx::Error::Decode(
                    "OrderReleased row has no released_price; it predates the column and \
                     the value cannot be recovered"
                        .into(),
                )
            })?;
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderReleased::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            released_price,
            event_id,
            ts_event,
            ts_init,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderSubmittedRow {
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
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderSubmitted::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            event_id,
            ts_event,
            ts_init,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderTriggeredRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderTriggered::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderUpdatedRow {
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
        let event_id = row.try_get::<&str, _>("id").map(UUID4::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let reconciliation = row.try_get::<bool, _>("reconciliation")?;
        let venue_order_id = row
            .try_get::<Option<&str>, _>("venue_order_id")?
            .map(Into::into);
        let account_id = row
            .try_get::<Option<&str>, _>("account_id")?
            .map(Into::into);
        let quantity = row.try_get::<&str, _>("quantity").map(Quantity::from)?;
        let price = row.try_get::<Option<&str>, _>("price")?.map(Price::from);
        let trigger_price = row
            .try_get::<Option<&str>, _>("trigger_price")?
            .map(Price::from);
        let protection_price = row
            .try_get::<Option<&str>, _>("protection_price")?
            .map(Price::from);
        let is_quote_quantity = row.try_get::<bool, _>("quote_quantity")?;
        let causation_id = causation_id_from_row(row)?;
        let mut order_event = OrderUpdated::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            quantity,
            event_id,
            ts_event,
            ts_init,
            reconciliation,
            venue_order_id,
            account_id,
            price,
            trigger_price,
            protection_price,
            is_quote_quantity,
        );
        order_event.causation_id = causation_id;
        Ok(Self(order_event))
    }
}

impl<'r> FromRow<'r, PgRow> for OrderSnapshotRow {
    #[expect(
        clippy::too_many_lines,
        reason = "SQL row mapping mirrors the full order snapshot constructor"
    )]
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
        let activation_price = row
            .try_get::<Option<&str>, _>("activation_price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_price = row
            .try_get::<Option<&str>, _>("trigger_price")
            .ok()
            .and_then(|x| x.map(Price::from));
        let trigger_type = row
            .try_get::<Option<&str>, _>("trigger_type")
            .ok()
            .and_then(parse_trigger_type);
        let limit_offset = row
            .try_get::<Option<&str>, _>("limit_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset = row
            .try_get::<Option<&str>, _>("trailing_offset")
            .ok()
            .and_then(|x| x.and_then(|s| Decimal::from_str(s).ok()));
        let trailing_offset_type = row
            .try_get::<Option<TrailingOffsetTypePg>, _>("trailing_offset_type")
            .ok()
            .flatten()
            .and_then(|value| value.0);
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
        let avg_px = row.try_get::<Option<Decimal>, _>("avg_px").ok().flatten();
        let slippage = row.try_get::<Option<Decimal>, _>("slippage").ok().flatten();
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
            .and_then(parse_trigger_type);
        let trigger_instrument_id = row
            .try_get::<Option<&str>, _>("trigger_instrument_id")
            .ok()
            .and_then(|x| x.map(InstrumentId::from));
        let contingency_type = row
            .try_get::<Option<&str>, _>("contingency_type")
            .ok()
            .and_then(parse_contingency_type);
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
        let tags = tags_from_row(row);
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
            activation_price,
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
            causation_id: None,
        };

        Ok(Self(snapshot))
    }
}

fn causation_id_from_row(row: &PgRow) -> Result<Option<UUID4>, sqlx::Error> {
    row.try_get::<Option<&str>, _>("causation_id")?
        .map(|value| UUID4::from_str(value).map_err(|e| sqlx::Error::Decode(e.into())))
        .transpose()
}

fn decode_info(row: &PgRow) -> Result<Option<IndexMap<Ustr, Ustr>>, sqlx::Error> {
    let value = row.try_get::<Option<serde_json::Value>, _>("info")?;
    let Some(value) = value else {
        return Ok(None);
    };

    let decoded: IndexMap<String, String> =
        serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

    Ok(Some(
        decoded
            .into_iter()
            .map(|(k, v)| (Ustr::from(k.as_str()), Ustr::from(v.as_str())))
            .collect(),
    ))
}

fn tags_from_row(row: &PgRow) -> Option<Vec<Ustr>> {
    row.try_get::<Vec<String>, _>("tags")
        .ok()
        .map(|tags| tags.iter().map(|tag| Ustr::from(tag.as_str())).collect())
}

fn parse_trigger_type(value: Option<&str>) -> Option<TriggerType> {
    value.and_then(|value| {
        if value.eq_ignore_ascii_case("NO_TRIGGER") {
            None
        } else {
            Some(TriggerType::from_str(value).expect("Invalid `TriggerType`"))
        }
    })
}

fn parse_contingency_type(value: Option<&str>) -> Option<ContingencyType> {
    value.and_then(|value| {
        if value.eq_ignore_ascii_case("NO_CONTINGENCY") {
            None
        } else {
            Some(ContingencyType::from_str(value).expect("Invalid `ContingencyType`"))
        }
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(None, None)]
    #[case(Some("NO_TRIGGER"), None)]
    #[case(Some("LAST_PRICE"), Some(TriggerType::LastPrice))]
    fn test_parse_trigger_type_accepts_legacy_absence(
        #[case] value: Option<&str>,
        #[case] expected: Option<TriggerType>,
    ) {
        assert_eq!(parse_trigger_type(value), expected);
    }

    #[rstest]
    #[case(None, None)]
    #[case(Some("NO_CONTINGENCY"), None)]
    #[case(Some("OCO"), Some(ContingencyType::Oco))]
    fn test_parse_contingency_type_accepts_legacy_absence(
        #[case] value: Option<&str>,
        #[case] expected: Option<ContingencyType>,
    ) {
        assert_eq!(parse_contingency_type(value), expected);
    }
}
