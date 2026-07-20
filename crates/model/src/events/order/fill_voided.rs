// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use std::fmt::{Debug, Display};

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderType, TimeInForce, TrailingOffsetType,
        TriggerType,
    },
    events::OrderEvent,
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{Currency, Money, Price, Quantity},
};

/// Records that a cumulative fill quantity no longer has economic effect.
///
/// The correction identity, voided quantity, and commission are cumulative for the referenced
/// trade. `is_reopened` records positive evidence that the corrected order is executable again
/// and therefore requires the referenced fill to have been applied locally. Without a local fill,
/// a non-reopened correction is an authoritative terminal order void.
#[repr(C)]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OrderFillVoided {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub correction_id: Ustr,
    pub trade_id: TradeId,
    pub voided_qty: Quantity,
    pub commission_voided: Option<Money>,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub last_px: Price,
    pub currency: Currency,
    pub liquidity_side: LiquiditySide,
    pub position_id: Option<PositionId>,
    pub reason: Option<Ustr>,
    pub info: Option<IndexMap<Ustr, Ustr>>,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
    #[serde(default)]
    pub is_reopened: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl OrderFillVoided {
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        correction_id: Ustr,
        trade_id: TradeId,
        voided_qty: Quantity,
        commission_voided: Option<Money>,
        order_side: OrderSide,
        order_type: OrderType,
        last_px: Price,
        currency: Currency,
        liquidity_side: LiquiditySide,
        position_id: Option<PositionId>,
        reason: Option<Ustr>,
        info: Option<IndexMap<Ustr, Ustr>>,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reconciliation: bool,
        is_reopened: bool,
    ) -> Self {
        Self {
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
            causation_id: None,
        }
    }
}

impl Debug for OrderFillVoided {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, strategy_id={}, instrument_id={}, client_order_id={}, venue_order_id={}, account_id={}, correction_id={}, trade_id={}, voided_qty={}, commission_voided={:?}, is_reopened={}, event_id={}, ts_event={}, ts_init={})",
            stringify!(OrderFillVoided),
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.account_id,
            self.correction_id,
            self.trade_id,
            self.voided_qty,
            self.commission_voided,
            self.is_reopened,
            self.event_id,
            self.ts_event,
            self.ts_init,
        )
    }
}

impl Display for OrderFillVoided {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, client_order_id={}, venue_order_id={}, correction_id={}, trade_id={}, voided_qty={}, is_reopened={}, reason={}, ts_event={})",
            stringify!(OrderFillVoided),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.correction_id,
            self.trade_id,
            self.voided_qty,
            self.is_reopened,
            self.reason.map_or("None", |reason| reason.as_str()),
            self.ts_event,
        )
    }
}

impl OrderEvent for OrderFillVoided {
    fn id(&self) -> UUID4 {
        self.event_id
    }

    fn type_name(&self) -> &'static str {
        stringify!(OrderFillVoided)
    }

    fn order_type(&self) -> Option<OrderType> {
        Some(self.order_type)
    }

    fn order_side(&self) -> Option<OrderSide> {
        Some(self.order_side)
    }

    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    fn strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    fn trade_id(&self) -> Option<TradeId> {
        Some(self.trade_id)
    }

    fn currency(&self) -> Option<Currency> {
        Some(self.currency)
    }

    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    fn reason(&self) -> Option<Ustr> {
        self.reason
    }

    fn quantity(&self) -> Option<Quantity> {
        Some(self.voided_qty)
    }

    fn time_in_force(&self) -> Option<TimeInForce> {
        None
    }

    fn liquidity_side(&self) -> Option<LiquiditySide> {
        Some(self.liquidity_side)
    }

    fn post_only(&self) -> Option<bool> {
        None
    }

    fn reduce_only(&self) -> Option<bool> {
        None
    }

    fn quote_quantity(&self) -> Option<bool> {
        None
    }

    fn reconciliation(&self) -> bool {
        self.reconciliation
    }

    fn price(&self) -> Option<Price> {
        None
    }

    fn last_px(&self) -> Option<Price> {
        Some(self.last_px)
    }

    fn last_qty(&self) -> Option<Quantity> {
        Some(self.voided_qty)
    }

    fn activation_price(&self) -> Option<Price> {
        None
    }

    fn trigger_price(&self) -> Option<Price> {
        None
    }

    fn trigger_type(&self) -> Option<TriggerType> {
        None
    }

    fn limit_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset(&self) -> Option<Decimal> {
        None
    }

    fn trailing_offset_type(&self) -> Option<TrailingOffsetType> {
        None
    }

    fn expire_time(&self) -> Option<UnixNanos> {
        None
    }

    fn display_qty(&self) -> Option<Quantity> {
        None
    }

    fn emulation_trigger(&self) -> Option<TriggerType> {
        None
    }

    fn trigger_instrument_id(&self) -> Option<InstrumentId> {
        None
    }

    fn contingency_type(&self) -> Option<ContingencyType> {
        None
    }

    fn order_list_id(&self) -> Option<OrderListId> {
        None
    }

    fn linked_order_ids(&self) -> Option<Vec<ClientOrderId>> {
        None
    }

    fn parent_order_id(&self) -> Option<ClientOrderId> {
        None
    }

    fn exec_algorithm_id(&self) -> Option<ExecAlgorithmId> {
        None
    }

    fn exec_spawn_id(&self) -> Option<ClientOrderId> {
        None
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        Some(self.venue_order_id)
    }

    fn account_id(&self) -> Option<AccountId> {
        Some(self.account_id)
    }

    fn position_id(&self) -> Option<PositionId> {
        self.position_id
    }

    fn commission(&self) -> Option<Money> {
        self.commission_voided
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}
