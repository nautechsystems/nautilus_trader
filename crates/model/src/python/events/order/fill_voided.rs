// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
// -------------------------------------------------------------------------------------------------

use indexmap::IndexMap;
use nautilus_core::{
    UUID4,
    python::{
        IntoPyObjectNautilusExt,
        serialization::{from_dict_pyo3, to_dict_pyo3},
    },
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use ustr::Ustr;

use crate::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::OrderFillVoided,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    orders::str_indexmap_to_ustr,
    types::{Currency, Money, Price, Quantity},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OrderFillVoided {
    /// Records that a cumulative fill quantity no longer has economic effect.
    ///
    /// The correction identity, voided quantity, and commission are cumulative for the referenced
    /// trade. `is_reopened` records positive evidence that the corrected order is executable again
    /// and therefore requires the referenced fill to have been applied locally. Without a local fill,
    /// a non-reopened correction is an authoritative terminal order void.
    #[expect(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (trader_id, strategy_id, instrument_id, client_order_id, venue_order_id, account_id, correction_id, trade_id, voided_qty, order_side, order_type, last_px, currency, liquidity_side, event_id, ts_event, ts_init, reconciliation, is_reopened=false, commission_voided=None, position_id=None, reason=None, info=None))]
    fn py_new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        correction_id: &str,
        trade_id: TradeId,
        voided_qty: Quantity,
        order_side: OrderSide,
        order_type: OrderType,
        last_px: Price,
        currency: Currency,
        liquidity_side: LiquiditySide,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
        reconciliation: bool,
        is_reopened: bool,
        commission_voided: Option<Money>,
        position_id: Option<PositionId>,
        reason: Option<&str>,
        info: Option<IndexMap<String, String>>,
    ) -> Self {
        Self::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id,
            Ustr::from(correction_id),
            trade_id,
            voided_qty,
            commission_voided,
            order_side,
            order_type,
            last_px,
            currency,
            liquidity_side,
            position_id,
            reason.map(Ustr::from),
            info.map(str_indexmap_to_ustr),
            event_id,
            ts_event.into(),
            ts_init.into(),
            reconciliation,
            is_reopened,
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn trader_id(&self) -> TraderId {
        self.trader_id
    }
    #[getter]
    fn strategy_id(&self) -> StrategyId {
        self.strategy_id
    }
    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }
    #[getter]
    fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }
    #[getter]
    fn venue_order_id(&self) -> VenueOrderId {
        self.venue_order_id
    }
    #[getter]
    fn account_id(&self) -> AccountId {
        self.account_id
    }
    #[getter]
    fn correction_id(&self) -> &str {
        self.correction_id.as_str()
    }
    #[getter]
    fn trade_id(&self) -> TradeId {
        self.trade_id
    }
    #[getter]
    fn voided_qty(&self) -> Quantity {
        self.voided_qty
    }
    #[getter]
    fn commission_voided(&self) -> Option<Money> {
        self.commission_voided
    }
    #[getter]
    fn order_side(&self) -> OrderSide {
        self.order_side
    }
    #[getter]
    fn order_type(&self) -> OrderType {
        self.order_type
    }
    #[getter]
    fn last_px(&self) -> Price {
        self.last_px
    }
    #[getter]
    fn currency(&self) -> Currency {
        self.currency
    }
    #[getter]
    fn liquidity_side(&self) -> LiquiditySide {
        self.liquidity_side
    }
    #[getter]
    fn position_id(&self) -> Option<PositionId> {
        self.position_id
    }
    #[getter]
    fn reason(&self) -> Option<&str> {
        self.reason.map(|value| value.as_str())
    }
    #[getter]
    fn event_id(&self) -> UUID4 {
        self.event_id
    }
    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }
    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
    #[getter]
    fn reconciliation(&self) -> bool {
        self.reconciliation
    }
    #[getter]
    fn is_reopened(&self) -> bool {
        self.is_reopened
    }
    #[getter]
    fn causation_id(&self) -> Option<UUID4> {
        self.causation_id
    }

    #[getter]
    fn info(&self) -> Option<IndexMap<&str, &str>> {
        self.info.as_ref().map(|info| {
            info.iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect()
        })
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UUID4;
    use rstest::rstest;

    use super::*;
    use crate::events::order::spec::OrderFillVoidedSpec;

    #[rstest]
    fn test_order_fill_voided_python_dict_round_trip() {
        let mut event = OrderFillVoidedSpec::builder()
            .correction_id(Ustr::from("CORRECTION-PYTHON"))
            .voided_qty(Quantity::from("0.561000"))
            .commission_voided(Money::from("12.20000000 USDT"))
            .position_id(PositionId::from("P-001"))
            .reason(Ustr::from("VENUE_VOID"))
            .info(IndexMap::from([(Ustr::from("source"), Ustr::from("test"))]))
            .reconciliation(true)
            .is_reopened(true)
            .build();
        event.causation_id = Some(UUID4::new());

        Python::initialize();
        Python::attach(|py| {
            let values = event.py_to_dict(py).unwrap();
            let restored = OrderFillVoided::py_from_dict(py, values).unwrap();

            assert_eq!(restored, event);
        });
    }
}
