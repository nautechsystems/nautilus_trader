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

//! Per-order fill tracking with terminal quantity normalization for the Polymarket adapter.

use ahash::AHashMap;
use indexmap::IndexMap;
use nautilus_common::cache::fifo::FifoCacheMap;
#[cfg(test)]
use nautilus_model::identifiers::InstrumentId;
use nautilus_model::{
    enums::OrderSide,
    events::OrderFilled,
    identifiers::{ClientOrderId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::Quantity,
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use ustr::Ustr;

use super::settlement::SettlementRegistry;
use crate::common::consts::DUST_SNAP_THRESHOLD_DEC;

/// Cumulative fill state for a single venue order.
///
/// A modified order continues on a replacement venue order, so `prior_qty` holds the order
/// quantity carried by earlier venue orders, and the order quantity is `prior_qty` plus
/// `submitted_qty`. `prior_filled` is the part of `prior_qty` they filled, without their
/// non-reopened voided quantity.
#[derive(Debug, Clone, Copy)]
struct OrderFillState {
    submitted_qty: Quantity,
    prior_qty: Quantity,
    prior_filled: Quantity,
    cumulative_filled: Quantity,
    order_side: OrderSide,
}

#[derive(Clone, Debug)]
pub(crate) struct FillCorrectionMetadata {
    pub venue_trade_id: String,
    pub info: Option<IndexMap<Ustr, Ustr>>,
}

#[derive(Clone, Debug)]
pub(crate) struct BufferedFill {
    pub report: FillReport,
    pub correction: Option<FillCorrectionMetadata>,
}

impl BufferedFill {
    /// Returns whether the fill may emit now: a fill without correction metadata always may, and
    /// a trade fill only while the settlement registry permits its application.
    pub(crate) fn claim(&self, settlement: &SettlementRegistry) -> bool {
        self.correction.as_ref().is_none_or(|correction| {
            settlement.claim_buffered_fill(&correction.venue_trade_id, &self.report.trade_id)
        })
    }
}

/// Registration map plus the fill and order-report buffers, all under one mutex.
///
/// Co-locating the buffers with the registration map is what closes the buffer-after-drain race:
/// the WS dispatch's accepted-check and buffer, and the submit path's register and drain, are all
/// single critical sections on this one lock, so a buffer can never slip between a register and the
/// drain that follows it.
#[derive(Debug, Default)]
struct TrackerInner {
    orders: AHashMap<VenueOrderId, OrderFillState>,
    pending_fills: FifoCacheMap<VenueOrderId, Vec<BufferedFill>, 1_000>,
    pending_reports: FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>,
}

/// Tracks per-order fill accumulation, detects dust residuals, and buffers WS messages that arrive
/// before the order is registered.
///
/// Thread-safe: a single internal `Mutex<TrackerInner>` -- safe to share via `Arc` across the WS
/// task and spawned order submission tasks. Because registration and buffering share that lock, the
/// accepted-or-buffer decision and the register-and-drain are mutually atomic.
#[derive(Debug)]
pub(crate) struct OrderFillTrackerMap {
    inner: Mutex<TrackerInner>,
}

impl OrderFillTrackerMap {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(TrackerInner::default()),
        }
    }

    pub(crate) fn restore_order(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        prior_qty: Quantity,
        prior_filled: Quantity,
        filled_qty: Quantity,
        order_side: OrderSide,
    ) {
        let mut state = new_order_state(submitted_qty, prior_qty, prior_filled, order_side);
        state.cumulative_filled = filled_qty;
        self.inner.lock().orders.insert(venue_order_id, state);
    }

    /// Returns true if the order has been registered (accepted).
    pub(crate) fn contains(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner.lock().orders.get(venue_order_id).is_some()
    }

    /// Returns true if the order has received any fills or been removed (settled).
    pub(crate) fn has_fills_or_settled(&self, venue_order_id: &VenueOrderId) -> bool {
        match self.inner.lock().orders.get(venue_order_id) {
            Some(s) => !s.cumulative_filled.is_zero(),
            None => true, // Removed = already settled
        }
    }

    /// Returns the cumulative filled quantity for an order, if tracked.
    pub(crate) fn get_cumulative_filled(&self, venue_order_id: &VenueOrderId) -> Option<Quantity> {
        self.inner
            .lock()
            .orders
            .get(venue_order_id)
            .map(|s| s.cumulative_filled)
    }

    /// Returns `true` if cumulative fills have reached the submitted quantity.
    pub(crate) fn is_fully_filled(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .orders
            .get(venue_order_id)
            .is_some_and(|s| s.cumulative_filled >= s.submitted_qty)
    }

    /// Records a tracked fill, or buffers it until the order is registered, atomically.
    ///
    /// The accepted-check and the buffer insert run under one lock, so the submit path's register
    /// and drain (the same lock) cannot interleave between them. Returns the report to emit when the
    /// order is registered, or `None` when it was buffered.
    pub(crate) fn accept_or_buffer_fill(
        &self,
        venue_order_id: VenueOrderId,
        report: FillReport,
        correction: FillCorrectionMetadata,
    ) -> Option<FillReport> {
        let mut guard = self.inner.lock();
        if guard.orders.get(&venue_order_id).is_some() {
            record_fill_in(&mut guard.orders, &venue_order_id, report.last_qty);
            Some(report)
        } else {
            push_buffered(
                &mut guard.pending_fills,
                venue_order_id,
                BufferedFill {
                    report,
                    correction: Some(correction),
                },
            );
            None
        }
    }

    /// Returns a tracked order report to emit, or buffers it until the order is registered.
    ///
    /// The accepted-check and the buffer insert run under one lock, so the submit path's register
    /// (sequenced before its report drain) cannot leave the report buffered with no later drain.
    /// Returns the report to emit when the order is registered, or `None` when it was buffered.
    pub(crate) fn accept_or_buffer_report(
        &self,
        venue_order_id: VenueOrderId,
        report: OrderStatusReport,
    ) -> Option<OrderStatusReport> {
        let mut guard = self.inner.lock();
        if guard.orders.get(&venue_order_id).is_some() {
            Some(report)
        } else {
            push_buffered(&mut guard.pending_reports, venue_order_id, report);
            None
        }
    }

    /// Registers the order without resetting recovered fills,
    /// then drains its buffered fills under one lock.
    ///
    /// Registration and the drain are a single critical section, so a concurrent
    /// [`Self::accept_or_buffer_fill`] cannot read the order as unregistered and buffer a fill into
    /// the window after this drain.
    pub(crate) fn register_and_take_pending_fills(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        submitted_qty: Quantity,
        prior_qty: Quantity,
        prior_filled: Quantity,
        order_side: OrderSide,
    ) -> Vec<BufferedFill> {
        let mut guard = self.inner.lock();
        guard
            .orders
            .entry(venue_order_id)
            .or_insert_with(|| new_order_state(submitted_qty, prior_qty, prior_filled, order_side));
        take_and_prepare_fills(&mut guard, venue_order_id, client_order_id)
    }

    /// Registers the order and drains its buffered fills only when a fill is already buffered.
    ///
    /// Used by the unknown-submit path, where acceptance is deferred until a buffered fill proves
    /// the venue took the order. Returns `None` (registering nothing) when no fill is buffered.
    pub(crate) fn register_and_take_pending_fills_if_buffered(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
        submitted_qty: Quantity,
        order_side: OrderSide,
    ) -> Option<Vec<BufferedFill>> {
        let mut guard = self.inner.lock();
        if !guard.pending_fills.contains_key(&venue_order_id) {
            return None;
        }

        guard.orders.entry(venue_order_id).or_insert_with(|| {
            let zero = Quantity::zero(submitted_qty.precision);
            new_order_state(submitted_qty, zero, zero, order_side)
        });

        Some(take_and_prepare_fills(
            &mut guard,
            venue_order_id,
            client_order_id,
        ))
    }

    /// Drains and prepares buffered fills for an already-registered order.
    pub(crate) fn take_pending_fills(
        &self,
        venue_order_id: VenueOrderId,
        client_order_id: Option<ClientOrderId>,
    ) -> Vec<BufferedFill> {
        let mut guard = self.inner.lock();
        take_and_prepare_fills(&mut guard, venue_order_id, client_order_id)
    }

    /// Drains buffered order reports for a registered order (raw, for conversion by the caller).
    pub(crate) fn take_pending_reports(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Vec<OrderStatusReport> {
        self.inner
            .lock()
            .pending_reports
            .remove(venue_order_id)
            .unwrap_or_default()
    }

    /// Emits a buffered fill once `authorize` allows it, otherwise suppresses it and rolls back
    /// its tracker quantity.
    ///
    /// The decision, rollback, and overfill bump run under the tracker lock, so they stay
    /// consistent with concurrent fills for the same order.
    pub(crate) fn emit_buffered_fill<A, F>(&self, fill: OrderFilled, authorize: A, emit: F) -> bool
    where
        A: FnOnce() -> bool,
        F: FnOnce(OrderFilled, Option<Quantity>),
    {
        let mut guard = self.inner.lock();

        if !authorize() {
            reverse_fill_in(&mut guard.orders, &fill.venue_order_id, fill.last_qty);
            return false;
        }

        let new_qty = buy_overfill_bump_in(&mut guard.orders, &fill.venue_order_id);
        emit(fill, new_qty);
        true
    }

    pub(crate) fn reverse_fill(&self, venue_order_id: &VenueOrderId, quantity: Quantity) {
        reverse_fill_in(&mut self.inner.lock().orders, venue_order_id, quantity);
    }

    /// Raise the registered quantity to the cumulative BUY fills when they exceed it, returning
    /// the new order quantity to emit via `OrderUpdated` (or `None` when no raise is needed).
    /// The order quantity includes the quantity carried by earlier venue orders of a modified
    /// order.
    ///
    /// A Polymarket BUY is bounded by the USDC it spends (`makerAmount`), so a marketable fill
    /// below the limit price returns more shares than the nominal quantity, and market BUY quote
    /// conversion can leave a few microshares of overfill. The engine rejects a fill past the
    /// order quantity, so the quantity is raised to the actual fill before the `OrderFilled`
    /// applies, and the fill keeps the venue quantity. SELL orders are share-denominated and never
    /// overfill, so they always return `None`.
    ///
    /// Raising `submitted_qty` to exactly the cumulative fill makes the following `OrderFilled`
    /// reach `Filled`. That is correct because an overfill only ever occurs on a marketable taker
    /// BUY, whose fill the venue reports as a single aggregated trade event (one `FillReport` per
    /// taker order): the bumping fill is therefore terminal, with no later fill to strand. Passive
    /// maker BUYs can fill across several events but execute at their own price, so they never
    /// overfill and never reach this raise. A venue that split one marketable BUY across multiple
    /// trade events would close the order on the first crossing fill; this is not Polymarket's
    /// observed behavior and would need a final-fill signal to handle.
    pub(crate) fn buy_overfill_bump(&self, venue_order_id: &VenueOrderId) -> Option<Quantity> {
        let mut guard = self.inner.lock();
        buy_overfill_bump_in(&mut guard.orders, venue_order_id)
    }

    /// Returns the order quantity at the venue-filled quantity when a terminal order has
    /// sub-cent-share leaves.
    ///
    /// The returned quantity is used for an order-only reconciliation update. It is not a fill and
    /// must not change positions, balances, or commissions. It equals the order's filled quantity,
    /// without earlier venue orders' non-reopened voided quantity, so the update closes the order.
    /// The entry is removed on normalization so repeated terminal messages are idempotent.
    pub(crate) fn check_terminal_quantity_normalization(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Option<Quantity> {
        let mut guard = self.inner.lock();
        let s = guard.orders.get(venue_order_id)?;
        if s.cumulative_filled >= s.submitted_qty {
            return None;
        }
        let leaves = s.submitted_qty.as_decimal() - s.cumulative_filled.as_decimal();

        if leaves > Decimal::ZERO && leaves < DUST_SNAP_THRESHOLD_DEC {
            let filled_qty = s.prior_filled + s.cumulative_filled;

            log::debug!(
                "Normalizing terminal order {venue_order_id} quantity from {} to {filled_qty} \
                 (non-economic leaves={leaves})",
                s.prior_qty + s.submitted_qty,
            );
            guard.orders.remove(venue_order_id);
            Some(filled_qty)
        } else {
            if leaves >= DUST_SNAP_THRESHOLD_DEC {
                log::debug!(
                    "Order {venue_order_id} MATCHED with significant residual \
                     {leaves} (filled {}/{})",
                    s.cumulative_filled,
                    s.submitted_qty,
                );
            }
            None
        }
    }

    /// Returns the real unfilled remainder of a terminal IOC order.
    ///
    /// The entry is removed so duplicate `CONFIRMED` trade messages cannot emit repeated
    /// cancellations. The caller must use this only after a taker trade confirms: that proves the
    /// FAK order has finished matching and the venue has killed the returned remainder.
    pub(crate) fn take_terminal_ioc_remainder(
        &self,
        venue_order_id: &VenueOrderId,
    ) -> Option<Quantity> {
        let mut guard = self.inner.lock();
        let state = guard.orders.get(venue_order_id)?;
        if state.cumulative_filled.is_zero() || state.cumulative_filled >= state.submitted_qty {
            return None;
        }

        let remainder = state.submitted_qty - state.cumulative_filled;
        guard.orders.remove(venue_order_id);
        Some(remainder)
    }
}

fn new_order_state(
    submitted_qty: Quantity,
    prior_qty: Quantity,
    prior_filled: Quantity,
    order_side: OrderSide,
) -> OrderFillState {
    OrderFillState {
        submitted_qty,
        prior_qty,
        prior_filled,
        cumulative_filled: Quantity::zero(submitted_qty.precision),
        order_side,
    }
}

fn buy_overfill_bump_in(
    orders: &mut AHashMap<VenueOrderId, OrderFillState>,
    venue_order_id: &VenueOrderId,
) -> Option<Quantity> {
    let state = orders.get_mut(venue_order_id)?;
    if state.order_side != OrderSide::Buy {
        return None;
    }

    if state.cumulative_filled > state.submitted_qty {
        state.submitted_qty = state.cumulative_filled;
        Some(state.prior_qty + state.cumulative_filled)
    } else {
        None
    }
}

/// Drains the buffered fills for `venue_order_id`, stamping the client order ID and recording
/// each one. The caller must hold the lock and have registered the order first.
fn take_and_prepare_fills(
    inner: &mut TrackerInner,
    venue_order_id: VenueOrderId,
    client_order_id: Option<ClientOrderId>,
) -> Vec<BufferedFill> {
    let Some(buffered) = inner.pending_fills.remove(&venue_order_id) else {
        return Vec::new();
    };
    buffered
        .into_iter()
        .map(|mut buffered| {
            buffered.report.client_order_id = client_order_id;
            record_fill_in(&mut inner.orders, &venue_order_id, buffered.report.last_qty);
            buffered
        })
        .collect()
}

fn push_buffered<V>(
    buffer: &mut FifoCacheMap<VenueOrderId, Vec<V>, 1_000>,
    venue_order_id: VenueOrderId,
    value: V,
) {
    if let Some(values) = buffer.get_mut(&venue_order_id) {
        values.push(value);
    } else {
        buffer.insert(venue_order_id, vec![value]);
    }
}

#[cfg(test)]
impl OrderFillTrackerMap {
    /// Registers an order directly, for tests that set up an already-accepted order.
    pub(crate) fn register(
        &self,
        venue_order_id: VenueOrderId,
        submitted_qty: Quantity,
        order_side: OrderSide,
        _instrument_id: InstrumentId,
        _size_precision: u8,
        _price_precision: u8,
    ) {
        let zero = Quantity::zero(submitted_qty.precision);
        self.inner.lock().orders.insert(
            venue_order_id,
            new_order_state(submitted_qty, zero, zero, order_side),
        );
    }

    /// Returns the registered submitted quantity for an order, if tracked.
    pub(crate) fn submitted_qty(&self, venue_order_id: &VenueOrderId) -> Option<Quantity> {
        self.inner
            .lock()
            .orders
            .get(venue_order_id)
            .map(|s| s.submitted_qty)
    }

    /// Returns the quantity earlier venue orders filled, if tracked.
    pub(crate) fn prior_filled(&self, venue_order_id: &VenueOrderId) -> Option<Quantity> {
        self.inner
            .lock()
            .orders
            .get(venue_order_id)
            .map(|s| s.prior_filled)
    }

    /// Records a fill against a registered order, for tests that drive fill accumulation directly.
    pub(crate) fn record_fill(&self, venue_order_id: &VenueOrderId, qty: Quantity) {
        record_fill_in(&mut self.inner.lock().orders, venue_order_id, qty);
    }

    /// Buffers a fill as if it arrived on the WS channel before the order was registered.
    pub(crate) fn buffer_fill_for_test(&self, venue_order_id: VenueOrderId, report: FillReport) {
        push_buffered(
            &mut self.inner.lock().pending_fills,
            venue_order_id,
            BufferedFill {
                report,
                correction: None,
            },
        );
    }

    /// Buffers an order report as if it arrived on the WS channel before the order was registered.
    pub(crate) fn buffer_report_for_test(
        &self,
        venue_order_id: VenueOrderId,
        report: OrderStatusReport,
    ) {
        push_buffered(
            &mut self.inner.lock().pending_reports,
            venue_order_id,
            report,
        );
    }

    /// Returns true if a fill is currently buffered for the order.
    pub(crate) fn has_pending_fill(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner.lock().pending_fills.contains_key(venue_order_id)
    }

    /// Returns the fills currently buffered for the order.
    pub(crate) fn pending_fills_for(&self, venue_order_id: &VenueOrderId) -> Vec<FillReport> {
        self.inner
            .lock()
            .pending_fills
            .get(venue_order_id)
            .map(|fills| fills.iter().map(|fill| fill.report.clone()).collect())
            .unwrap_or_default()
    }

    /// Returns true if an order report is currently buffered for the order.
    pub(crate) fn has_pending_report(&self, venue_order_id: &VenueOrderId) -> bool {
        self.inner
            .lock()
            .pending_reports
            .contains_key(venue_order_id)
    }
}

fn record_fill_in(
    orders: &mut AHashMap<VenueOrderId, OrderFillState>,
    venue_order_id: &VenueOrderId,
    qty: Quantity,
) {
    if let Some(s) = orders.get_mut(venue_order_id) {
        s.cumulative_filled = s.cumulative_filled + qty;
    }
}

fn reverse_fill_in(
    orders: &mut AHashMap<VenueOrderId, OrderFillState>,
    venue_order_id: &VenueOrderId,
    qty: Quantity,
) {
    if let Some(state) = orders.get_mut(venue_order_id) {
        state.cumulative_filled = if qty >= state.cumulative_filled {
            Quantity::zero(state.cumulative_filled.precision)
        } else {
            state.cumulative_filled - qty
        };
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::LiquiditySide,
        identifiers::{AccountId, TradeId},
        types::{Currency, Money, Price},
    };
    use rstest::rstest;

    use super::*;

    fn pusd() -> Currency {
        Currency::pUSD()
    }

    #[rstest]
    fn submit_ack_registration_preserves_recovered_fills() {
        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("V-RECOVERED");
        tracker.register_and_take_pending_fills(
            venue_order_id,
            None,
            Quantity::from("100"),
            Quantity::zero(Quantity::from("100").precision),
            Quantity::zero(Quantity::from("100").precision),
            OrderSide::Buy,
        );
        tracker.record_fill(&venue_order_id, Quantity::from("25"));
        let drained = tracker.register_and_take_pending_fills(
            venue_order_id,
            None,
            Quantity::from("100"),
            Quantity::zero(Quantity::from("100").precision),
            Quantity::zero(Quantity::from("100").precision),
            OrderSide::Buy,
        );
        assert!(drained.is_empty());
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::from("25"))
        );
    }

    #[rstest]
    fn test_register_and_contains() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        assert!(!tracker.contains(&vid));

        tracker.register(
            vid,
            Quantity::from("100"),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );
        assert!(tracker.contains(&vid));
    }

    #[rstest]
    fn test_register_retains_fill_state_after_later_capacity_flood() {
        let tracker = OrderFillTrackerMap::new();
        let retained = VenueOrderId::from("order-retain");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        tracker.register(
            retained,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument_id,
            6,
            2,
        );

        for index in 0..10_000 {
            tracker.register(
                VenueOrderId::from(format!("order-flood-{index}").as_str()),
                Quantity::from("1"),
                OrderSide::Sell,
                instrument_id,
                6,
                2,
            );
        }

        assert!(tracker.contains(&retained));
        assert_eq!(
            tracker.submitted_qty(&retained),
            Some(Quantity::from("100"))
        );
    }

    #[rstest]
    fn test_refused_buffered_fill_is_suppressed_and_rolled_back() {
        use std::cell::Cell;

        use nautilus_model::{
            enums::OrderType,
            identifiers::{StrategyId, TraderId},
        };

        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-failed-before-drain");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let report = FillReport {
            account_id: AccountId::from("POLY-001"),
            instrument_id,
            venue_order_id,
            trade_id: TradeId::from("trade-failed-before-drain"),
            order_side: OrderSide::Buy,
            last_qty: Quantity::new(5.0, 6),
            last_px: Price::new(0.55, 2),
            commission: Money::zero(pusd()),
            liquidity_side: LiquiditySide::Taker,
            avg_px: None,
            report_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            client_order_id: None,
            venue_position_id: None,
        };

        let venue_trade_id = "trade-failed-before-drain-order-failed-before-drain";

        let accepted = tracker.accept_or_buffer_fill(
            venue_order_id,
            report.clone(),
            FillCorrectionMetadata {
                venue_trade_id: venue_trade_id.to_string(),
                info: None,
            },
        );

        let drained = tracker.register_and_take_pending_fills(
            venue_order_id,
            Some(ClientOrderId::from("O-FAILED-BEFORE-DRAIN")),
            Quantity::new(10.0, 6),
            Quantity::zero(Quantity::new(10.0, 6).precision),
            Quantity::zero(Quantity::new(10.0, 6).precision),
            OrderSide::Buy,
        );
        let buffered = &drained[0];
        let fill = OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("O-FAILED-BEFORE-DRAIN"),
            venue_order_id,
            report.account_id,
            report.trade_id,
            report.order_side,
            OrderType::Limit,
            report.last_qty,
            report.last_px,
            pusd(),
            report.liquidity_side,
            UUID4::new(),
            report.ts_event,
            report.ts_init,
            false,
            None,
            Some(report.commission),
            None,
        );
        let was_emitted = Cell::new(false);

        let emitted = tracker.emit_buffered_fill(
            fill,
            || buffered.correction.is_none(),
            |_, _| {
                was_emitted.set(true);
            },
        );

        assert!(accepted.is_none());
        assert_eq!(drained.len(), 1);
        assert!(!emitted);
        assert!(!was_emitted.get());
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6))
        );
    }

    #[rstest]
    fn test_record_fill_accumulates() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(50.0, 6));
        tracker.record_fill(&vid, Quantity::new(49.997714, 6));

        let normalized = tracker.check_terminal_quantity_normalization(&vid);

        assert_eq!(normalized, Some(Quantity::new(99.997714, 6)));
    }

    #[rstest]
    fn test_check_dust_no_residual() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        // Exact fill
        tracker.record_fill(&vid, Quantity::new(100.0, 6));

        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_check_dust_significant_residual() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        // Only half filled, residual = 50 >> 0.01
        tracker.record_fill(&vid, Quantity::new(50.0, 6));

        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_take_terminal_ioc_remainder_is_exact_and_idempotent() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-partial-ioc");
        tracker.register(
            vid,
            Quantity::from("30.000000"),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            3,
        );
        tracker.record_fill(&vid, Quantity::from("20.000000"));

        let remainder = tracker.take_terminal_ioc_remainder(&vid);

        assert_eq!(remainder, Some(Quantity::from("10.000000")));
        assert!(!tracker.contains(&vid));
        assert!(tracker.take_terminal_ioc_remainder(&vid).is_none());
    }

    #[rstest]
    fn test_take_terminal_ioc_remainder_requires_a_partial_fill() {
        let tracker = OrderFillTrackerMap::new();
        let unfilled = VenueOrderId::from("order-unfilled-ioc");
        let filled = VenueOrderId::from("order-filled-ioc");
        for venue_order_id in [unfilled, filled] {
            tracker.register(
                venue_order_id,
                Quantity::from("20.000000"),
                OrderSide::Buy,
                InstrumentId::from("TEST.POLYMARKET"),
                6,
                3,
            );
        }
        tracker.record_fill(&filled, Quantity::from("20.000000"));

        assert!(tracker.take_terminal_ioc_remainder(&unfilled).is_none());
        assert!(tracker.take_terminal_ioc_remainder(&filled).is_none());
        assert!(tracker.contains(&unfilled));
        assert!(tracker.contains(&filled));
    }

    #[rstest]
    fn test_check_dust_unregistered() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("unknown");

        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_dust_settlement_removes_entry() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(99.995, 6));

        let normalized = tracker.check_terminal_quantity_normalization(&vid);
        assert_eq!(normalized, Some(Quantity::new(99.995, 6)));

        // Entry should be removed, second check returns None (no duplicate).
        assert!(!tracker.contains(&vid));
        assert!(
            tracker
                .check_terminal_quantity_normalization(&vid)
                .is_none()
        );
    }

    #[rstest]
    fn test_get_cumulative_filled_no_fills() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        let filled = tracker.get_cumulative_filled(&vid);
        assert_eq!(filled, Some(Quantity::zero(6)));
    }

    #[rstest]
    fn test_get_cumulative_filled_with_fills() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(30.0, 6));
        tracker.record_fill(&vid, Quantity::new(20.0, 6));

        let filled = tracker.get_cumulative_filled(&vid);
        assert_eq!(filled, Some(Quantity::new(50.0, 6)));
    }

    #[rstest]
    fn test_get_cumulative_filled_unregistered() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("unknown");
        assert!(tracker.get_cumulative_filled(&vid).is_none());
    }

    #[rstest]
    fn test_is_fully_filled_unregistered() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("unknown");
        assert!(!tracker.is_fully_filled(&vid));
    }

    #[rstest]
    fn test_is_fully_filled_partial() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(50.0, 6));
        assert!(!tracker.is_fully_filled(&vid));
    }

    #[rstest]
    fn test_is_fully_filled_complete() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(100.0, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::new(60.0, 6));
        tracker.record_fill(&vid, Quantity::new(40.0, 6));
        assert!(tracker.is_fully_filled(&vid));
    }

    fn register_buy(tracker: &OrderFillTrackerMap, vid: VenueOrderId, submitted: f64) {
        tracker.register(
            vid,
            Quantity::new(submitted, 6),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );
    }

    #[rstest]
    fn test_buy_overfill_bump_unregistered_is_none() {
        let tracker = OrderFillTrackerMap::new();
        assert!(
            tracker
                .buy_overfill_bump(&VenueOrderId::from("unknown"))
                .is_none()
        );
    }

    #[rstest]
    fn test_buy_overfill_bump_within_qty_is_none() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 10.0);

        // Exact fill: cumulative equals submitted, no raise.
        tracker.record_fill(&vid, Quantity::new(10.0, 6));
        assert!(tracker.buy_overfill_bump(&vid).is_none());
    }

    #[rstest]
    fn test_buy_overfill_bump_sell_is_none() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::new(10.0, 6),
            OrderSide::Sell,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        // A SELL is share-denominated; even an over-report does not raise the quantity.
        tracker.record_fill(&vid, Quantity::new(12.0, 6));
        assert!(tracker.buy_overfill_bump(&vid).is_none());
    }

    #[rstest]
    fn test_buy_overfill_bump_raises_to_cumulative_and_is_idempotent() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 10.0);

        // Marketable BUY fills below its limit: 12 shares against a nominal 10.
        tracker.record_fill(&vid, Quantity::new(12.0, 6));

        let bumped = tracker.buy_overfill_bump(&vid).expect("expected a raise");
        assert_eq!(bumped, Quantity::new(12.0, 6));
        // Submitted is raised, so a second emit for the same fill does not re-raise.
        assert!(tracker.buy_overfill_bump(&vid).is_none());
        // Leaves are non-negative after the raise, so no spurious dust residual.
        assert!(tracker.is_fully_filled(&vid));
    }

    #[rstest]
    fn test_buy_overfill_bump_tracks_each_crossing_fill() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        register_buy(&tracker, vid, 10.0);

        // First partial stays within the nominal qty: no raise.
        tracker.record_fill(&vid, Quantity::new(6.0, 6));
        assert!(tracker.buy_overfill_bump(&vid).is_none());

        // Second partial crosses the nominal qty: raise to cumulative 14.
        tracker.record_fill(&vid, Quantity::new(8.0, 6));
        assert_eq!(
            tracker.buy_overfill_bump(&vid),
            Some(Quantity::new(14.0, 6))
        );

        // Third partial crosses again: raise to cumulative 20.
        tracker.record_fill(&vid, Quantity::new(6.0, 6));
        assert_eq!(
            tracker.buy_overfill_bump(&vid),
            Some(Quantity::new(20.0, 6))
        );
    }

    // A replacement venue order carries only the leaves of a modified order, so the order-level
    // quantities it returns add the quantity carried by earlier venue orders.
    #[rstest]
    fn test_replacement_order_returns_order_level_qty() {
        let tracker = OrderFillTrackerMap::new();
        let overfilled = VenueOrderId::from("replacement-overfill");
        let underfilled = VenueOrderId::from("replacement-underfill");
        for venue_order_id in [overfilled, underfilled] {
            tracker.register_and_take_pending_fills(
                venue_order_id,
                None,
                Quantity::from("15.000000"),
                Quantity::from("5.000000"),
                Quantity::from("5.000000"),
                OrderSide::Buy,
            );
        }

        tracker.record_fill(&overfilled, Quantity::from("15.000058"));
        tracker.record_fill(&underfilled, Quantity::from("14.995000"));

        let bumped = tracker.buy_overfill_bump(&overfilled);
        let normalized = tracker.check_terminal_quantity_normalization(&underfilled);

        assert_eq!(bumped, Some(Quantity::from("20.000058")));
        assert_eq!(
            tracker.submitted_qty(&overfilled),
            Some(Quantity::from("15.000058"))
        );
        assert_eq!(normalized, Some(Quantity::from("19.995000")));
    }

    // Market BUY quote conversion leaves microshares of overfill, which raise the quantity
    // like any other BUY overfill so the fill keeps the venue quantity.
    #[rstest]
    fn test_buy_overfill_bump_raises_to_dust_overfill() {
        let tracker = OrderFillTrackerMap::new();
        let vid = VenueOrderId::from("order-1");
        tracker.register(
            vid,
            Quantity::from("714.285710"),
            OrderSide::Buy,
            InstrumentId::from("TEST.POLYMARKET"),
            6,
            2,
        );

        tracker.record_fill(&vid, Quantity::from("714.285714"));

        assert_eq!(
            tracker.buy_overfill_bump(&vid),
            Some(Quantity::from("714.285714"))
        );
        assert_eq!(
            tracker.submitted_qty(&vid),
            Some(Quantity::from("714.285714"))
        );
        assert!(tracker.is_fully_filled(&vid));
    }

    #[rstest]
    fn test_drained_buy_dust_overfill_keeps_venue_qty_and_bumps() {
        use std::cell::Cell;

        use nautilus_model::{
            enums::OrderType,
            identifiers::{StrategyId, TraderId},
        };

        let tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("order-buffered-overfill");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");

        let report = FillReport {
            account_id: AccountId::from("POLY-001"),
            instrument_id,
            venue_order_id,
            trade_id: TradeId::from("trade-buffered-overfill"),
            order_side: OrderSide::Buy,
            last_qty: Quantity::from("714.285714"),
            last_px: Price::from("0.014"),
            commission: Money::zero(pusd()),
            liquidity_side: LiquiditySide::Taker,
            avg_px: None,
            report_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
            client_order_id: None,
            venue_position_id: None,
        };

        tracker.buffer_fill_for_test(venue_order_id, report.clone());

        let drained = tracker.register_and_take_pending_fills(
            venue_order_id,
            Some(ClientOrderId::from("O-BUFFERED-OVERFILL")),
            Quantity::from("714.285710"),
            Quantity::zero(Quantity::from("714.285710").precision),
            Quantity::zero(Quantity::from("714.285710").precision),
            OrderSide::Buy,
        );

        let fill = OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("O-BUFFERED-OVERFILL"),
            venue_order_id,
            report.account_id,
            report.trade_id,
            report.order_side,
            OrderType::Market,
            drained[0].report.last_qty,
            report.last_px,
            pusd(),
            report.liquidity_side,
            UUID4::new(),
            report.ts_event,
            report.ts_init,
            false,
            None,
            Some(report.commission),
            None,
        );
        let emitted_qty = Cell::new(None);
        let emitted = tracker.emit_buffered_fill(
            fill,
            || true,
            |filled, new_qty| emitted_qty.set(Some((filled.last_qty, new_qty))),
        );

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].report.last_qty, Quantity::from("714.285714"));
        assert!(emitted);
        assert_eq!(
            emitted_qty.get(),
            Some((
                Quantity::from("714.285714"),
                Some(Quantity::from("714.285714"))
            ))
        );
        assert_eq!(
            tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::from("714.285714"))
        );
    }
}
