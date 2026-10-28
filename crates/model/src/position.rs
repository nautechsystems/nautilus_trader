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

//! Market positions, replay history, and checked financial arithmetic.
//!
//! [`Position`] tracks open and closed positions from fills, adjustments, and instrument settlement.
//! It records quantity, side, average prices, commissions, and realized PnL.
//!
//! # Replay
//!
//! The replay journal records fills, adjustments, and instrument close events. Retained history
//! supports price restatements and cumulative fill corrections across close and reopen cycles.
//!
//! # Checked Arithmetic
//!
//! Financial calculations use checked domain types and [`Decimal`]. The shared arithmetic also
//! supports account projections and splitting fills for position reversals.
//!
//! - Input conversions reject precision loss and overflow.
//! - Derived products, cost sums, division, and proportional intermediates round at Decimal precision.
//!   Cost allocations retain a representable remainder.
//! - Arithmetic rejects a nonzero operand or result that would disappear at that precision.
//!   PnL sums may absorb a disappearing residual smaller than one native settlement [`Money`] unit.
//! - Derived [`Money`] values round midpoint to even at their native fixed-point scale.

use std::{
    fmt::Display,
    hash::{Hash, Hasher},
};

use ahash::{AHashMap, AHashSet};
use indexmap::IndexMap;
use nautilus_core::{
    DurationNanos, UUID4, UnixNanos,
    correctness::{
        CorrectnessError, CorrectnessResult, CorrectnessResultExt, FAILED, check_equal,
        check_predicate_true,
    },
};
use ruint::aliases::{U256, U512};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    data::InstrumentClose,
    enums::{
        InstrumentClass, InstrumentCloseType, OrderSide, PositionAdjustmentType, PositionSide,
    },
    events::{OrderFillVoided, OrderFilled, PositionAdjusted},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId, TraderId,
        Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    types::{
        Currency, Money, Price, Quantity,
        fixed::{FIXED_PRECISION, FIXED_SCALAR_RAW, checked_mul_div_raw, raw_scales_match},
        quantity::QuantityRaw,
    },
};

/// An open or closed position in a market.
///
/// # Identity and Replay
///
/// The venue may assign the position ID; the strategy's Order Management System (OMS) settings
/// control system-generated IDs. Retained replay events and cumulative fill corrections preserve
/// derived state across close and reopen cycles.
///
/// # Accounting
///
/// Financial calculations use checked domain and [`Decimal`] arithmetic. Decimal has a 96-bit
/// coefficient and a maximum scale of 28. Input conversions reject precision loss and overflow;
/// derived products, cost sums, division, and proportional intermediates use its finite precision.
///
/// - Cost accumulators preserve the cost basis without reusing rounded average prices.
/// - Partial reductions retain unallocated cost; the final reduction consumes it.
/// - A sub-Money PnL remainder preserves fractional amounts between updates.
///
/// [`Money`] results round midpoint to even at their native fixed-point scale. `realized_return`
/// is a floating-point analytics projection and does not feed accounting.
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct Position {
    pub events: Vec<OrderFilled>,
    pub adjustments: Vec<PositionAdjusted>,
    #[serde(default)]
    pub replay_events: Vec<PositionReplayEvent>,
    #[serde(default)]
    pub fill_voids: Vec<PositionFillVoid>,
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub id: PositionId,
    pub account_id: AccountId,
    pub opening_order_id: ClientOrderId,
    pub closing_order_id: Option<ClientOrderId>,
    pub entry: OrderSide,
    pub side: PositionSide,
    pub signed_qty: Decimal,
    pub quantity: Quantity,
    pub peak_qty: Quantity,
    pub price_precision: u8,
    pub size_precision: u8,
    pub multiplier: Quantity,
    pub is_inverse: bool,
    pub is_currency_pair: bool,
    pub instrument_class: InstrumentClass,
    pub base_currency: Option<Currency>,
    pub quote_currency: Currency,
    pub settlement_currency: Currency,
    pub ts_init: UnixNanos,
    pub ts_opened: UnixNanos,
    pub ts_last: UnixNanos,
    pub ts_closed: Option<UnixNanos>,
    pub duration_ns: DurationNanos,
    pub avg_px_open: Decimal,
    pub avg_px_close: Option<Decimal>,
    pub realized_return: f64,
    pub realized_pnl: Option<Money>,
    #[serde(with = "nautilus_core::serialization::sorted_hashset")]
    pub trade_ids: AHashSet<TradeId>,
    /// Total buy quantity in the current position episode.
    pub buy_qty: Quantity,
    /// Total sell quantity in the current position episode.
    pub sell_qty: Quantity,
    pub commissions: IndexMap<Currency, Money>,
    pub cost_open: Decimal,
    pub cost_close: Decimal,
    pub qty_closed: Decimal,
    pub realized_pnl_remainder: Decimal,
}

#[expect(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PositionReplayEvent {
    Filled(OrderFilled),
    Adjusted(PositionAdjusted),
    InstrumentClosed(InstrumentClose),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionFillVoid {
    pub event: OrderFillVoided,
    pub voided_qty: Quantity,
    pub commission_voided: Option<Money>,
}

impl Position {
    /// Creates a new [`Position`] instance.
    ///
    /// # Panics
    ///
    /// Panics if [`Position::new_checked`] returns an error.
    #[must_use]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "constructor takes the opening fill by value as the position's seed event"
    )]
    pub fn new(instrument: &InstrumentAny, fill: OrderFilled) -> Self {
        Self::new_checked(instrument, fill).expect_display(FAILED)
    }

    /// Creates a new [`Position`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument ID does not match the fill, the fill has no position
    /// ID, or its accounting values cannot be represented without input precision loss.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "constructor takes the opening fill by value as the position's seed event"
    )]
    pub fn new_checked(instrument: &InstrumentAny, fill: OrderFilled) -> CorrectnessResult<Self> {
        Self::check_fill_instrument(instrument.id(), "instrument.id()", &fill)?;
        let position_id = Self::fill_position_id(&fill)?;

        let mut item = Self {
            events: Vec::<OrderFilled>::new(),
            adjustments: Vec::<PositionAdjusted>::new(),
            replay_events: Vec::new(),
            fill_voids: Vec::new(),
            trade_ids: AHashSet::<TradeId>::new(),
            buy_qty: Quantity::zero(instrument.size_precision()),
            sell_qty: Quantity::zero(instrument.size_precision()),
            commissions: IndexMap::<Currency, Money>::new(),
            trader_id: fill.trader_id,
            strategy_id: fill.strategy_id,
            instrument_id: fill.instrument_id,
            id: position_id,
            account_id: fill.account_id,
            opening_order_id: fill.client_order_id,
            closing_order_id: None,
            entry: fill.order_side,
            side: PositionSide::Flat,
            signed_qty: Decimal::ZERO,
            quantity: fill.last_qty,
            peak_qty: fill.last_qty,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
            multiplier: instrument.multiplier(),
            is_inverse: instrument.is_inverse(),
            is_currency_pair: matches!(
                instrument,
                InstrumentAny::CurrencyPair(_) | InstrumentAny::TokenizedAsset(_)
            ),
            instrument_class: instrument.instrument_class(),
            base_currency: instrument.base_currency(),
            quote_currency: instrument.quote_currency(),
            settlement_currency: instrument.cost_currency(),
            ts_init: fill.ts_init,
            ts_opened: fill.ts_event,
            ts_last: fill.ts_event,
            ts_closed: None,
            duration_ns: DurationNanos::default(),
            avg_px_open: price(fill.last_px)?,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            cost_open: Decimal::ZERO,
            cost_close: Decimal::ZERO,
            qty_closed: Decimal::ZERO,
            realized_pnl_remainder: Decimal::ZERO,
        };

        item.check_accounting()?;
        item.apply_fill(&fill, true)?;
        Ok(item)
    }

    /// Returns a copy without stored events, adjustments, replay events, fill voids, or trade IDs.
    ///
    /// # Warning
    ///
    /// Use this copy only as transient read state. Applying events or caching this copy can bypass
    /// replay and duplicate-fill checks and discard position history.
    #[must_use]
    pub fn clone_without_events(&self) -> Self {
        Self {
            events: Vec::new(),
            adjustments: Vec::new(),
            replay_events: Vec::new(),
            fill_voids: Vec::new(),
            trader_id: self.trader_id,
            strategy_id: self.strategy_id,
            instrument_id: self.instrument_id,
            id: self.id,
            account_id: self.account_id,
            opening_order_id: self.opening_order_id,
            closing_order_id: self.closing_order_id,
            entry: self.entry,
            side: self.side,
            signed_qty: self.signed_qty,
            quantity: self.quantity,
            peak_qty: self.peak_qty,
            price_precision: self.price_precision,
            size_precision: self.size_precision,
            multiplier: self.multiplier,
            is_inverse: self.is_inverse,
            is_currency_pair: self.is_currency_pair,
            instrument_class: self.instrument_class,
            base_currency: self.base_currency,
            quote_currency: self.quote_currency,
            settlement_currency: self.settlement_currency,
            ts_init: self.ts_init,
            ts_opened: self.ts_opened,
            ts_last: self.ts_last,
            ts_closed: self.ts_closed,
            duration_ns: self.duration_ns,
            avg_px_open: self.avg_px_open,
            avg_px_close: self.avg_px_close,
            realized_return: self.realized_return,
            realized_pnl: self.realized_pnl,
            trade_ids: AHashSet::new(),
            buy_qty: self.buy_qty,
            sell_qty: self.sell_qty,
            commissions: self.commissions.clone(),
            cost_open: self.cost_open,
            cost_close: self.cost_close,
            qty_closed: self.qty_closed,
            realized_pnl_remainder: self.realized_pnl_remainder,
        }
    }

    /// Returns a snapshot copy without durable replay or fill-void history.
    #[must_use]
    pub fn clone_for_snapshot(&self) -> Self {
        let mut snapshot = self.clone_without_events();
        snapshot.events.clone_from(&self.events);
        snapshot.adjustments.clone_from(&self.adjustments);
        snapshot.trade_ids.clone_from(&self.trade_ids);
        snapshot
    }

    /// Moves durable replay state from `prior` and keeps this position's current replay events.
    pub fn transfer_replay_state_from(&mut self, prior: &mut Self) {
        let current_replay = std::mem::take(&mut self.replay_events);
        self.replay_events = std::mem::take(&mut prior.replay_events);
        self.fill_voids = std::mem::take(&mut prior.fill_voids);
        self.replay_events.extend(current_replay);
    }

    /// Purges all order fill events for the given client order ID and recalculates derived state.
    ///
    /// # Warning
    ///
    /// This operation recalculates the entire position from scratch after removing the specified
    /// order's fills. This is an expensive operation and should be used sparingly.
    ///
    /// # Errors
    ///
    /// Returns an error if the remaining history cannot be represented. An error leaves the
    /// position unchanged.
    pub fn purge_events_for_order(
        &mut self,
        client_order_id: ClientOrderId,
    ) -> CorrectnessResult<()> {
        let mut next = self.clone();
        next.purge_order(client_order_id)?;
        *self = next;
        Ok(())
    }

    fn purge_order(&mut self, client_order_id: ClientOrderId) -> CorrectnessResult<()> {
        self.replay_events.retain(|event| {
            !matches!(event, PositionReplayEvent::Filled(fill) if fill.client_order_id == client_order_id)
        });
        self.fill_voids
            .retain(|record| record.event.client_order_id != client_order_id);

        let filtered_events: Vec<OrderFilled> = self
            .events
            .iter()
            .filter(|e| e.client_order_id != client_order_id)
            .cloned()
            .collect();

        // Preserve non-commission adjustments (funding, manual adjustments, etc.)
        // Commission adjustments will be automatically re-created when fills are replayed
        let preserved_adjustments: Vec<PositionAdjusted> = self
            .adjustments
            .iter()
            .filter(|adj| {
                // Keep all non-commission adjustments (funding, manual, etc.)
                // Commission adjustments will be re-created during fill replay
                adj.adjustment_type != PositionAdjustmentType::Commission
            })
            .copied()
            .collect();

        // If no events remain, log warning - position should be closed/removed instead
        if filtered_events.is_empty() {
            log::warn!(
                "Position {} has no fills remaining after purging order {}; consider closing the position instead",
                self.id,
                client_order_id
            );
            self.events.clear();
            self.trade_ids.clear();
            self.adjustments.clear();
            self.buy_qty = Quantity::zero(self.size_precision);
            self.sell_qty = Quantity::zero(self.size_precision);
            self.commissions.clear();
            self.signed_qty = Decimal::ZERO;
            self.quantity = Quantity::zero(self.size_precision);
            self.side = PositionSide::Flat;
            self.avg_px_close = None;
            self.realized_pnl = None;
            self.realized_return = 0.0;
            self.ts_opened = UnixNanos::default();
            self.ts_last = UnixNanos::default();
            self.ts_closed = Some(UnixNanos::default());
            self.duration_ns = DurationNanos::default();
            self.cost_open = Decimal::ZERO;
            self.cost_close = Decimal::ZERO;
            self.qty_closed = Decimal::ZERO;
            self.realized_pnl_remainder = Decimal::ZERO;
            return Ok(());
        }

        // Recalculate position from scratch
        let position_id = self.id;
        self.reset_derived_state();

        // Use the first remaining event to set opening state
        let first_event = &filtered_events[0];
        self.entry = first_event.order_side;
        self.opening_order_id = first_event.client_order_id;
        self.ts_opened = first_event.ts_event;
        self.ts_init = first_event.ts_init;
        self.closing_order_id = None;
        self.ts_closed = None;
        self.duration_ns = DurationNanos::default();

        let restatement_indexes = Self::price_restatement_indexes(&self.replay_events);

        let start = self.replay_events.iter().position(|event| {
            matches!(
                event,
                PositionReplayEvent::Filled(fill) if fill.event_id == first_event.event_id
            )
        });

        let remaining_orders: AHashSet<_> = filtered_events
            .iter()
            .map(|event| event.client_order_id)
            .collect();
        let effective_by_id: AHashMap<_, _> = filtered_events
            .iter()
            .map(|event| (event.event_id, event))
            .collect();

        if let Some(start) = start {
            let replay = self.replay_events.clone();
            for (index, event) in replay.iter().enumerate().skip(start) {
                let PositionReplayEvent::Filled(fill) = event else {
                    continue;
                };

                if !remaining_orders.contains(&fill.client_order_id) {
                    continue;
                }

                if restatement_indexes.contains(&index) {
                    self.apply_price_restatement(fill, false)?;
                } else if let Some(effective) = effective_by_id.get(&fill.event_id) {
                    self.apply_fill(effective, false)?;
                }
            }
        } else {
            for event in &filtered_events {
                self.apply_fill(event, false)?;
            }
        }

        // Reapply preserved adjustments to maintain full state
        for adjustment in preserved_adjustments {
            self.apply_adjustment_state(adjustment, false)?;
        }

        log::info!(
            "Purged fills for order {} from position {}; recalculated state: qty={}, signed_qty={}, side={:?}",
            client_order_id,
            position_id,
            self.quantity,
            self.signed_qty,
            self.side
        );
        Ok(())
    }

    /// Applies an `OrderFilled` event to this position.
    ///
    /// A fill with the same `trade_id`, the same `client_order_id`, and a new price restates
    /// the open average when that trade is an opening fill in the current cycle. Quantity is
    /// unchanged. The original fill stays in `events`, so a void still matches its `last_px`.
    /// Applying the same `event_id` again does nothing. A different order that reuses the trade
    /// ID is rejected. A closing fill does not change the open average, and restating an
    /// opening fill does not change realized PnL for quantity already closed.
    ///
    /// # Panics
    ///
    /// Panics if the fill is rejected as a duplicate trade or fails accounting validation.
    pub fn apply(&mut self, fill: &OrderFilled) {
        self.apply_fill(fill, true).expect_display(FAILED);
    }

    /// Applies an `OrderFilled` event to this position with correctness checking.
    ///
    /// A fill with the same `trade_id`, the same `client_order_id`, and a new price restates
    /// the open average when that trade is an opening fill in the current cycle. Quantity is
    /// unchanged. An error leaves the position unchanged. Applying the same `event_id` again
    /// does nothing. A closing fill does not change the open average.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The fill instrument or position identity does not match this position.
    /// - The fill has no position ID.
    /// - An accounting input or intermediate exceeds the supported Decimal or domain bounds.
    /// - `fill.trade_id` is already in `trade_ids` and this fill is not a restatement of that
    ///   order.
    pub fn try_apply(&mut self, fill: &OrderFilled) -> CorrectnessResult<()> {
        Self::check_fill_instrument(self.instrument_id, "self.instrument_id", fill)?;
        let position_id = Self::fill_position_id(fill)?;
        check_equal(&self.id, &position_id, "self.id", "fill.position_id")?;
        self.apply_fill(fill, true)
    }

    fn replay_has_event(&self, event_id: UUID4) -> bool {
        self.replay_events.iter().any(|event| {
            matches!(
                event,
                PositionReplayEvent::Filled(existing) if existing.event_id == event_id
            )
        })
    }

    fn check_fill_instrument(
        instrument_id: InstrumentId,
        instrument_param: &str,
        fill: &OrderFilled,
    ) -> CorrectnessResult<()> {
        check_equal(
            &instrument_id,
            &fill.instrument_id,
            instrument_param,
            "fill.instrument_id",
        )
    }

    fn fill_position_id(fill: &OrderFilled) -> CorrectnessResult<PositionId> {
        fill.position_id
            .ok_or_else(|| CorrectnessError::PredicateViolation {
                message: "`fill.position_id` was None".to_string(),
            })
    }

    fn check_fill_accounting(&self, fill: &OrderFilled) -> CorrectnessResult<(Decimal, Decimal)> {
        let last_px = price(fill.last_px)?;
        let last_qty = qty(fill.last_qty)?;
        if let Some(commission) = fill.commission {
            amount(commission)?;
        }

        Ok((last_px, last_qty))
    }

    /// Checks that stored domain inputs fit the exact Decimal accounting bounds.
    ///
    /// # Errors
    ///
    /// Returns an error if a stored price, quantity, or monetary input loses precision or overflows.
    pub fn check_accounting(&self) -> CorrectnessResult<()> {
        for quantity in [
            self.quantity,
            self.peak_qty,
            self.buy_qty,
            self.sell_qty,
            self.multiplier,
        ] {
            qty(quantity)?;
        }

        for amount in self.commissions.values().copied().chain(self.realized_pnl) {
            self::amount(amount)?;
        }

        for fill in &self.events {
            self.check_fill_accounting(fill)?;
        }

        for event in &self.replay_events {
            match event {
                PositionReplayEvent::Filled(fill) => {
                    self.check_fill_accounting(fill)?;
                }
                PositionReplayEvent::InstrumentClosed(close) => {
                    price(close.close_price)?;
                }
                PositionReplayEvent::Adjusted(_) => {}
            }
        }

        Ok(())
    }

    fn apply_fill(&mut self, fill: &OrderFilled, record_replay: bool) -> CorrectnessResult<()> {
        if record_replay
            && self.trade_ids.contains(&fill.trade_id)
            && self.replay_has_event(fill.event_id)
        {
            return Ok(());
        }

        let (last_px, last_qty) = self.check_fill_accounting(fill)?;

        if self.is_price_restatement(fill) {
            let mut next = self.clone();
            next.apply_price_restatement(fill, record_replay)?;
            *self = next;
            return Ok(());
        }

        if record_replay
            && (self.side == PositionSide::Flat || !self.trade_ids.contains(&fill.trade_id))
            && self.is_duplicate_replay_fill(fill)
        {
            log::warn!(
                "Ignoring historical duplicate fill {} for position {}; durable replay already contains this trade",
                fill.trade_id,
                self.id,
            );
            return Ok(());
        }

        if fill.ts_event < self.ts_opened {
            log::warn!(
                "Fill ts_event {} for {} is before position ts_opened {}",
                fill.ts_event,
                self.id,
                self.ts_opened,
            );
        }

        let resets = self.side == PositionSide::Flat;
        if record_replay && !resets {
            check_predicate_true(
                !self.trade_ids.contains(&fill.trade_id),
                "`fill.trade_id` already contained in `trade_ids`",
            )?;
        }

        let mut next = self.prepare_fill(fill, last_px, last_qty)?;
        if !resets {
            next.events = std::mem::take(&mut self.events);
            next.trade_ids = std::mem::take(&mut self.trade_ids);
            let mut adjustments = std::mem::take(&mut self.adjustments);
            adjustments.append(&mut next.adjustments);
            next.adjustments = adjustments;
        }

        next.replay_events = std::mem::take(&mut self.replay_events);
        next.fill_voids = std::mem::take(&mut self.fill_voids);
        next.events.push(fill.clone());
        next.trade_ids.insert(fill.trade_id);
        if record_replay {
            next.replay_events
                .push(PositionReplayEvent::Filled(fill.clone()));
        }

        next.debug_assert_invariants();
        *self = next;
        Ok(())
    }

    fn prepare_fill(
        &self,
        fill: &OrderFilled,
        last_px: Decimal,
        last_qty: Decimal,
    ) -> CorrectnessResult<Self> {
        let mut next = self.clone_without_events();
        if self.side == PositionSide::Flat {
            next.reset_cycle(fill)?;
        }

        if let Some(commission) = fill.commission {
            let total = match next.commissions.get(&commission.currency) {
                Some(total) => total
                    .checked_add(commission)
                    .ok_or_else(|| invalid("position commission overflow"))?,
                None => commission,
            };

            next.commissions.insert(commission.currency, total);
        }

        next.handle_order_fill(fill, last_px, last_qty)?;
        if next.is_currency_pair
            && let Some(commission) = fill.commission
            && next.base_currency == Some(commission.currency)
        {
            next.apply_base_commission_adjustment(fill, commission)?;
        }

        next.quantity = quantity(next.signed_qty.abs(), next.size_precision)?;
        next.peak_qty = next.peak_qty.max(next.quantity);
        if next.quantity.is_zero() {
            next.side = PositionSide::Flat;
            next.signed_qty = Decimal::ZERO;
            next.closing_order_id = Some(fill.client_order_id);
            next.ts_closed = Some(fill.ts_event);
            next.duration_ns = fill.ts_event.saturating_duration_since(next.ts_opened);
        } else if next.signed_qty > Decimal::ZERO {
            next.entry = OrderSide::Buy;
            next.side = PositionSide::Long;
        } else {
            next.entry = OrderSide::Sell;
            next.side = PositionSide::Short;
        }

        next.ts_last = fill.ts_event;
        Ok(next)
    }

    fn reset_cycle(&mut self, fill: &OrderFilled) -> CorrectnessResult<()> {
        self.events.clear();
        self.trade_ids.clear();
        self.adjustments.clear();
        self.buy_qty = Quantity::zero(self.size_precision);
        self.sell_qty = Quantity::zero(self.size_precision);
        self.commissions.clear();
        self.opening_order_id = fill.client_order_id;
        self.closing_order_id = None;
        self.peak_qty = Quantity::zero(self.size_precision);
        self.ts_init = fill.ts_init;
        self.ts_opened = fill.ts_event;
        self.ts_closed = None;
        self.duration_ns = DurationNanos::default();
        self.avg_px_open = price(fill.last_px)?;
        self.avg_px_close = None;
        self.realized_return = 0.0;
        self.realized_pnl = None;
        self.cost_open = Decimal::ZERO;
        self.cost_close = Decimal::ZERO;
        self.qty_closed = Decimal::ZERO;
        self.realized_pnl_remainder = Decimal::ZERO;
        Ok(())
    }

    /// Returns whether durable replay history contains `trade_id`.
    #[must_use]
    pub fn has_replay_trade_id(&self, trade_id: TradeId) -> bool {
        self.replay_events.iter().any(
            |event| matches!(event, PositionReplayEvent::Filled(fill) if fill.trade_id == trade_id),
        )
    }

    fn is_duplicate_replay_fill(&self, fill: &OrderFilled) -> bool {
        let continues_latest_fill = fill.causation_id.is_some_and(|source_id| {
            self.events.last().is_some_and(|latest| {
                latest.trade_id == fill.trade_id && latest.event_id == source_id
            })
        });

        if self.trade_ids.contains(&fill.trade_id) {
            return !continues_latest_fill
                || self.replay_events.iter().any(|event| {
                    matches!(
                        event,
                        PositionReplayEvent::Filled(replayed)
                            if replayed.trade_id == fill.trade_id
                                && replayed.causation_id == fill.causation_id
                    )
                });
        }

        let replay_starts_current_cycle = self.replay_events.is_empty()
            || matches!(
                (self.replay_events.first(), self.events.first()),
                (
                    Some(PositionReplayEvent::Filled(replayed)),
                    Some(current),
                ) if replayed.event_id == current.event_id
            );
        let corrected_trade = self
            .fill_voids
            .iter()
            .any(|record| record.event.trade_id == fill.trade_id);
        let current_cycle_only = replay_starts_current_cycle && !corrected_trade;
        if current_cycle_only {
            return false;
        }

        self.has_replay_trade_id(fill.trade_id)
    }

    /// Returns whether the fill can restate an opening trade's price in the current episode.
    ///
    /// The order, trade, side, and quantity must match the recorded opening fill.
    #[must_use]
    pub fn accepts_price_restatement(&self, fill: &OrderFilled) -> bool {
        if !self.is_open()
            || !self.trade_ids.contains(&fill.trade_id)
            || fill.order_side != self.entry
        {
            return false;
        }

        let Some(existing) = self.events.iter().find(|event| {
            event.trade_id == fill.trade_id && event.client_order_id == fill.client_order_id
        }) else {
            return false;
        };

        if existing.last_qty != fill.last_qty {
            return false;
        }

        let mut latest = None;

        for event in &self.replay_events {
            let PositionReplayEvent::Filled(existing) = event else {
                continue;
            };

            if existing.trade_id == fill.trade_id
                && existing.client_order_id == fill.client_order_id
            {
                latest = Some(existing.last_px);
            }
        }

        latest.is_some_and(|price| price != fill.last_px)
    }

    fn is_price_restatement(&self, fill: &OrderFilled) -> bool {
        if self.side == PositionSide::Flat
            || self.quantity.is_zero()
            || !self.trade_ids.contains(&fill.trade_id)
        {
            return false;
        }

        let mut latest = None;

        for event in &self.replay_events {
            let PositionReplayEvent::Filled(existing) = event else {
                continue;
            };

            if existing.trade_id != fill.trade_id
                || existing.client_order_id != fill.client_order_id
            {
                continue;
            }

            if existing.event_id == fill.event_id {
                return latest.is_some_and(|price| price != fill.last_px);
            }

            latest = Some(existing.last_px);
        }

        latest.is_some_and(|price| price != fill.last_px)
    }

    fn apply_price_restatement(
        &mut self,
        fill: &OrderFilled,
        record_replay: bool,
    ) -> CorrectnessResult<()> {
        if self.side != PositionSide::Flat && !self.quantity.is_zero() {
            self.restate_open_average(fill)?;
        }

        self.ts_last = fill.ts_event;

        if record_replay {
            self.replay_events
                .push(PositionReplayEvent::Filled(fill.clone()));
        }

        Ok(())
    }

    fn has_replay_price_restatement(&self) -> bool {
        let mut seen = AHashMap::new();

        for event in &self.replay_events {
            let PositionReplayEvent::Filled(fill) = event else {
                continue;
            };

            if let Some(price) = seen.get(&fill.trade_id) {
                if *price != fill.last_px {
                    return true;
                }
            } else {
                seen.insert(fill.trade_id, fill.last_px);
            }
        }

        false
    }

    fn restate_open_average(&mut self, fill: &OrderFilled) -> CorrectnessResult<()> {
        if !self.trade_ids.contains(&fill.trade_id)
            || fill.order_side != self.entry
            || !self.extends_current_cycle(fill)?
        {
            return Ok(());
        }

        let new_px = price(fill.last_px)?;

        let Some(previous_px) = self.accounting_px_before(fill)? else {
            return Ok(());
        };

        let open_qty = qty(self.quantity)?;
        let trade_qty = self.cycle_open_qty_for_trade(fill.trade_id)?.min(open_qty);
        if open_qty.is_zero() || trade_qty.is_zero() {
            return Ok(());
        }

        let delta = mul_rounded(sub(new_px, previous_px)?, trade_qty)?;
        self.cost_open = add_rounded(self.cost_open, delta)?;
        self.avg_px_open = div(self.cost_open, open_qty)?;
        Ok(())
    }

    fn extends_current_cycle(&self, fill: &OrderFilled) -> CorrectnessResult<bool> {
        let long = self.entry == OrderSide::Buy;
        let mut signed = Decimal::ZERO;

        for event in &self.events {
            let qty = qty(event.last_qty)?;
            let delta = if event.order_side == OrderSide::Buy {
                qty
            } else {
                -qty
            };

            let after = add(signed, delta)?;
            if event.trade_id == fill.trade_id && event.client_order_id == fill.client_order_id {
                return Ok((signed.is_zero() || (signed > Decimal::ZERO) == long)
                    && ((after > Decimal::ZERO) == long));
            }

            signed = after;
        }

        Ok(false)
    }

    fn cycle_open_qty_for_trade(&self, trade_id: TradeId) -> CorrectnessResult<Decimal> {
        let long = self.entry == OrderSide::Buy;
        let mut remaining: Vec<(TradeId, Decimal)> = Vec::new();
        let mut signed = Decimal::ZERO;

        for fill in &self.events {
            let qty = qty(fill.last_qty)?;
            let delta = if fill.order_side == OrderSide::Buy {
                qty
            } else {
                -qty
            };

            let after = add(signed, delta)?;
            if !signed.is_zero() && (after > Decimal::ZERO) != (signed > Decimal::ZERO) {
                remaining.clear();
                if !after.is_zero() && (after > Decimal::ZERO) == long {
                    remaining.push((fill.trade_id, after.abs()));
                }
            } else if fill.order_side == self.entry {
                remaining.push((fill.trade_id, qty));
            } else {
                Self::reduce_open_lots(&mut remaining, qty)?;
            }

            signed = after;
        }

        remaining
            .into_iter()
            .filter(|(id, _)| *id == trade_id)
            .try_fold(Decimal::ZERO, |total, (_, qty)| add_rounded(total, qty))
    }

    fn reduce_open_lots(
        remaining: &mut [(TradeId, Decimal)],
        qty: Decimal,
    ) -> CorrectnessResult<()> {
        let open_qty = remaining
            .iter()
            .try_fold(Decimal::ZERO, |total, (_, qty)| add_rounded(total, *qty))?;

        if !open_qty.is_zero() {
            let keep = div(sub(open_qty, qty.min(open_qty))?, open_qty)?;
            for (_, qty) in remaining {
                *qty = mul_rounded(*qty, keep)?;
            }
        }

        Ok(())
    }

    fn accounting_px_before(&self, fill: &OrderFilled) -> CorrectnessResult<Option<Decimal>> {
        let mut price = None;

        for event in &self.replay_events {
            let PositionReplayEvent::Filled(existing) = event else {
                continue;
            };

            if existing.trade_id != fill.trade_id {
                continue;
            }

            if existing.event_id == fill.event_id {
                break;
            }

            price = Some(self::price(existing.last_px)?);
        }

        Ok(price)
    }

    fn voidable_replay_fill<'a>(
        restatement_indexes: &AHashSet<usize>,
        index: usize,
        replay_event: &'a PositionReplayEvent,
    ) -> Option<&'a OrderFilled> {
        let PositionReplayEvent::Filled(fill) = replay_event else {
            return None;
        };

        if restatement_indexes.contains(&index) {
            return None;
        }

        Some(fill)
    }

    fn price_restatement_indexes(replay_events: &[PositionReplayEvent]) -> AHashSet<usize> {
        let mut latest_px = AHashMap::new();
        let mut indexes = AHashSet::new();

        for (index, event) in replay_events.iter().enumerate() {
            let PositionReplayEvent::Filled(fill) = event else {
                continue;
            };

            let key = (fill.trade_id, fill.client_order_id);
            if let Some(price) = latest_px.get(&key)
                && *price != fill.last_px
            {
                indexes.insert(index);
            }

            latest_px.insert(key, fill.last_px);
        }

        indexes
    }

    fn handle_order_fill(
        &mut self,
        fill: &OrderFilled,
        last_px: Decimal,
        last_qty: Decimal,
    ) -> CorrectnessResult<()> {
        check_predicate_true(last_qty > Decimal::ZERO, "fill quantity must be positive")?;
        let prior_qty = self.signed_qty.abs();
        let increases = self.signed_qty.is_zero()
            || (self.signed_qty > Decimal::ZERO) == (fill.order_side == OrderSide::Buy);
        let mut pnl = Decimal::ZERO;

        if increases {
            self.cost_open = add_rounded(self.cost_open, mul_rounded(last_px, last_qty)?)?;
            self.avg_px_open = div(self.cost_open, add(prior_qty, last_qty)?)?;
        } else {
            let closing_qty = last_qty.min(prior_qty);

            let allocated_cost = if closing_qty == prior_qty {
                self.cost_open
            } else {
                proportion(self.cost_open, closing_qty, prior_qty)?
            };

            pnl = self.pnl_from_cost(
                last_px,
                closing_qty,
                allocated_cost,
                prior_qty,
                self.cost_open,
            )?;
            self.cost_open = sub(self.cost_open, allocated_cost)?;
            self.cost_close = add_rounded(self.cost_close, mul_rounded(last_px, closing_qty)?)?;
            self.qty_closed = add(self.qty_closed, closing_qty)?;
            self.avg_px_close = Some(div(self.cost_close, self.qty_closed)?);
            self.realized_return = self.calculate_return(
                self.avg_px_open,
                self.avg_px_close.expect("closing average is set"),
            );

            if last_qty > prior_qty {
                let opening_qty = sub(last_qty, prior_qty)?;
                self.cost_open = mul_rounded(last_px, opening_qty)?;
                self.avg_px_open = last_px;
                self.avg_px_close = None;
                self.realized_return = 0.0;
                self.cost_close = Decimal::ZERO;
                self.qty_closed = Decimal::ZERO;
                self.buy_qty = Quantity::zero(self.size_precision);
                self.sell_qty = Quantity::zero(self.size_precision);
            }
        }

        let commission = fill
            .commission
            .filter(|commission| commission.currency == self.settlement_currency)
            .map(amount)
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        pnl = add_pnl(pnl, -commission, self.settlement_currency)?;
        self.accumulate_pnl(pnl)?;

        let signed = if fill.order_side == OrderSide::Buy {
            last_qty
        } else {
            -last_qty
        };

        self.signed_qty = add(self.signed_qty, signed)?;

        let qty = if !increases && last_qty > prior_qty {
            quantity(sub(last_qty, prior_qty)?, self.size_precision)?
        } else {
            fill.last_qty
        };

        let cumulative = if fill.order_side == OrderSide::Buy {
            &mut self.buy_qty
        } else {
            &mut self.sell_qty
        };

        *cumulative = quantity_add(*cumulative, qty, self.size_precision)?;

        Ok(())
    }

    fn accumulate_pnl(&mut self, change: Decimal) -> CorrectnessResult<()> {
        let prior = self.realized_decimal()?;
        let total = add_pnl(prior, change, self.settlement_currency)?;
        let rounded = money(total, self.settlement_currency)?;
        self.realized_pnl_remainder = sub(total, amount(rounded)?)?;
        self.realized_pnl = Some(rounded);
        Ok(())
    }

    fn realized_decimal(&self) -> CorrectnessResult<Decimal> {
        let prior = match self.realized_pnl {
            Some(pnl) => {
                check_equal(
                    &pnl.currency,
                    &self.settlement_currency,
                    "realized_pnl.currency",
                    "settlement_currency",
                )?;
                amount(pnl)?
            }
            None => Decimal::ZERO,
        };

        add(prior, self.realized_pnl_remainder)
    }

    fn pnl_from_cost(
        &self,
        last_px: Decimal,
        qty: Decimal,
        cost: Decimal,
        prior_qty: Decimal,
        basis_cost: Decimal,
    ) -> CorrectnessResult<Decimal> {
        let difference = if self
            .instrument_class
            .divides_notional_by_price(self.is_inverse)
        {
            check_predicate_true(
                self.base_currency.is_some(),
                "inverse position requires base currency",
            )?;
            check_predicate_true(
                last_px > Decimal::ZERO && basis_cost > Decimal::ZERO,
                "inverse prices must be positive",
            )?;
            let entry = div(prior_qty, basis_cost)?;
            let exit = div(Decimal::ONE, last_px)?;
            mul_rounded(qty, add_rounded(entry, -exit)?)?
        } else {
            add_rounded(mul_rounded(last_px, qty)?, -cost)?
        };

        let difference = if self.signed_qty < Decimal::ZERO {
            -difference
        } else {
            difference
        };

        if self.multiplier.precision <= FIXED_PRECISION && self.multiplier.raw == FIXED_SCALAR_RAW {
            return Ok(difference);
        }

        mul_rounded(difference, self::qty(self.multiplier)?)
    }

    /// Applies a position adjustment event.
    ///
    /// This method handles adjustments to position quantity or realized PnL that occur
    /// outside of normal order fills, such as:
    /// - Commission adjustments in base currency (crypto spot markets).
    /// - Funding payments (perpetual futures).
    ///
    /// The adjustment event is stored in the position's adjustment history for full audit trail.
    ///
    /// # Errors
    ///
    /// Returns an error if currencies differ, arithmetic overflows, or a quantity change
    /// cannot be represented.
    /// An error leaves the position unchanged.
    pub fn apply_adjustment(&mut self, adjustment: PositionAdjusted) -> CorrectnessResult<()> {
        self.apply_adjustment_state(adjustment, true)
    }

    /// Reverts the most recent adjustment applied with [`Self::apply_adjustment`].
    ///
    /// `prior` is a snapshot of this position taken before that adjustment was applied, for
    /// example with [`Self::clone_without_events`]; the state an adjustment changes is restored
    /// from it, and the adjustment is removed from the position history.
    ///
    /// An adjustment can leave every field the snapshot is checked on unchanged, for example a
    /// zero value funding payment, so the snapshot must also not be older than the latest
    /// adjustment that stays in the history. This assumes adjustments are applied in timestamp
    /// order, as the backtest does; a stale snapshot behind out of order adjustments can pass.
    ///
    /// # Errors
    ///
    /// Returns an error if `prior` is a snapshot of a different position, size precision or
    /// settlement currency, the last replay event is not the latest adjustment (for example a
    /// fill was applied after it), `prior` predates the adjustment before the latest one, its
    /// realized PnL cannot take the adjustment's PnL change (another currency, or out of
    /// `Money` bounds), or applying that adjustment to `prior` does not reproduce the current
    /// state. An error leaves the position unchanged.
    pub fn revert_last_adjustment(&mut self, prior: &Self) -> CorrectnessResult<()> {
        check_equal(&self.id, &prior.id, "self.id", "prior.id")?;
        check_equal(
            &self.size_precision,
            &prior.size_precision,
            "self.size_precision",
            "prior.size_precision",
        )?;
        check_equal(
            &self.settlement_currency,
            &prior.settlement_currency,
            "self.settlement_currency",
            "prior.settlement_currency",
        )?;

        let adjustment = match (self.replay_events.last(), self.adjustments.last()) {
            (Some(PositionReplayEvent::Adjusted(replayed)), Some(latest)) if replayed == latest => {
                *latest
            }
            _ => {
                return Err(CorrectnessError::PredicateViolation {
                    message: format!(
                        "cannot revert adjustment for position {}: the last replay event is not its latest adjustment",
                        self.id
                    ),
                });
            }
        };

        if let Some(earlier) = self.adjustments.iter().rev().nth(1)
            && prior.ts_last < earlier.ts_event
        {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "cannot revert adjustment {} for position {}: `prior` at {} predates adjustment {} at {}",
                    adjustment.event_id, self.id, prior.ts_last, earlier.event_id, earlier.ts_event
                ),
            });
        }

        // Checked here because adding to a realized PnL in another currency panics
        if let (Some(realized_pnl), Some(pnl_change)) = (prior.realized_pnl, adjustment.pnl_change)
            && (realized_pnl.currency != pnl_change.currency
                || realized_pnl.checked_add(pnl_change).is_none())
        {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "cannot revert adjustment {} for position {}: `prior` realized PnL {realized_pnl} cannot take the PnL change {pnl_change}",
                    adjustment.event_id, self.id
                ),
            });
        }

        let mut expected = prior.clone_without_events();
        expected.apply_adjustment_state(adjustment, false)?;

        if expected.signed_qty != self.signed_qty
            || expected.quantity != self.quantity
            || expected.peak_qty != self.peak_qty
            || expected.side != self.side
            || expected.cost_open != self.cost_open
            || expected.realized_pnl_remainder != self.realized_pnl_remainder
            || expected.realized_pnl != self.realized_pnl
            || expected.ts_last != self.ts_last
        {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "cannot revert adjustment {} for position {}: `prior` is not the state before it",
                    adjustment.event_id, self.id
                ),
            });
        }

        self.adjustments.pop();
        self.replay_events.pop();
        self.signed_qty = prior.signed_qty;
        self.quantity = prior.quantity;
        self.peak_qty = prior.peak_qty;
        self.side = prior.side;
        self.realized_pnl = prior.realized_pnl;
        self.cost_open = prior.cost_open;
        self.realized_pnl_remainder = prior.realized_pnl_remainder;
        self.ts_last = prior.ts_last;

        self.debug_assert_invariants();
        Ok(())
    }

    fn apply_adjustment_state(
        &mut self,
        adjustment: PositionAdjusted,
        record_replay: bool,
    ) -> CorrectnessResult<()> {
        let signed_qty = add(
            self.signed_qty,
            adjustment.quantity_change.unwrap_or(Decimal::ZERO),
        )?;
        let quantity = quantity(signed_qty.abs(), self.size_precision)?;

        let cost_open = if adjustment.quantity_change.is_some() {
            if self.signed_qty.is_zero() {
                mul_rounded(self.avg_px_open, signed_qty.abs())?
            } else {
                proportion(self.cost_open, signed_qty.abs(), self.signed_qty.abs())?
            }
        } else {
            self.cost_open
        };

        let (realized_pnl, remainder) = if let Some(change) = adjustment.pnl_change {
            check_equal(
                &change.currency,
                &self.settlement_currency,
                "adjustment.currency",
                "settlement_currency",
            )?;
            let prior = self.realized_decimal()?;
            let total = add_pnl(prior, amount(change)?, self.settlement_currency)?;
            let rounded = money(total, self.settlement_currency)?;
            (Some(rounded), sub(total, amount(rounded)?)?)
        } else {
            (self.realized_pnl, self.realized_pnl_remainder)
        };

        self.signed_qty = signed_qty;
        self.quantity = quantity;
        self.peak_qty = self.peak_qty.max(quantity);
        self.cost_open = cost_open;
        self.realized_pnl = realized_pnl;
        self.realized_pnl_remainder = remainder;

        self.side = if signed_qty.is_zero() {
            PositionSide::Flat
        } else if signed_qty > Decimal::ZERO {
            PositionSide::Long
        } else {
            PositionSide::Short
        };

        self.adjustments.push(adjustment);

        if record_replay {
            self.replay_events
                .push(PositionReplayEvent::Adjusted(adjustment));
        }

        self.ts_last = adjustment.ts_event;
        self.debug_assert_invariants();
        Ok(())
    }

    /// Settles the open position at the payout price of an authoritative contract expiration.
    ///
    /// The settlement closes the current cycle at `close.close_price` and books its realized PnL
    /// without an order fill. Fill quantities and commissions stay as filled. The close is
    /// recorded in the replay history, so [`Self::is_settled`] holds until a later fill reopens
    /// the position.
    ///
    /// # Errors
    ///
    /// Returns an error if the close targets another instrument, is not a contract expiration,
    /// the position is not open, or the settlement PnL cannot be represented. An error leaves the
    /// position unchanged.
    pub fn apply_instrument_close(&mut self, close: InstrumentClose) -> anyhow::Result<()> {
        anyhow::ensure!(
            close.instrument_id == self.instrument_id,
            "instrument close {} does not match position instrument {}",
            close.instrument_id,
            self.instrument_id,
        );
        anyhow::ensure!(
            close.close_type == InstrumentCloseType::ContractExpired,
            "instrument close for {} is not a contract expiration",
            close.instrument_id,
        );
        anyhow::ensure!(
            self.is_open(),
            "cannot settle position {} which is not open",
            self.id,
        );

        self.apply_instrument_close_state(close, true)
    }

    fn apply_instrument_close_state(
        &mut self,
        close: InstrumentClose,
        record_replay: bool,
    ) -> anyhow::Result<()> {
        if self.side == PositionSide::Flat {
            return Ok(());
        }

        let last_px = price(close.close_price)?;
        let qty = self.signed_qty.abs();
        let cost_close = add_rounded(self.cost_close, mul_rounded(last_px, qty)?)?;
        let qty_closed = add(self.qty_closed, qty)?;
        let avg_px_close = div(cost_close, qty_closed)?;
        let realized_return = self.calculate_return(self.avg_px_open, avg_px_close);
        let pnl = self.pnl_from_cost(last_px, qty, self.cost_open, qty, self.cost_open)?;
        self.accumulate_pnl(pnl)?;

        if record_replay {
            self.replay_events
                .push(PositionReplayEvent::InstrumentClosed(close));
        }

        self.avg_px_close = Some(avg_px_close);
        self.realized_return = realized_return;
        self.cost_open = Decimal::ZERO;
        self.cost_close = cost_close;
        self.qty_closed = qty_closed;
        self.side = PositionSide::Flat;
        self.signed_qty = Decimal::ZERO;
        self.quantity = Quantity::zero(self.size_precision);
        self.closing_order_id = None;
        self.ts_last = close.ts_event;
        self.ts_closed = Some(close.ts_event);
        self.duration_ns = close.ts_event.saturating_duration_since(self.ts_opened);
        self.debug_assert_invariants();
        Ok(())
    }

    fn debug_assert_invariants(&self) {
        debug_assert!(
            match self.side {
                PositionSide::Long => self.signed_qty > Decimal::ZERO,
                PositionSide::Short => self.signed_qty < Decimal::ZERO,
                PositionSide::Flat => self.signed_qty == Decimal::ZERO,
            },
            "Invariant: position side must match signed_qty sign (side={:?}, signed_qty={})",
            self.side,
            self.signed_qty,
        );
        debug_assert!(
            self.peak_qty >= self.quantity,
            "Invariant: peak_qty must not be less than current quantity (peak={}, quantity={})",
            self.peak_qty,
            self.quantity,
        );
    }

    /// Applies a cumulative fill correction allocated to this position and rebuilds derived state.
    ///
    /// Returns the realized PnL of the cycles the rebuild closed before the current one, which
    /// [`Self::realized_pnl`] no longer holds because reopening from flat resets it. A caller
    /// archiving closed cycles needs this to keep their PnL once the correction has moved the
    /// cycle boundaries its existing archive describes. `None` when the corrected history never
    /// goes flat, so the current cycle covers all of it.
    ///
    /// # Errors
    ///
    /// Returns an error if the allocation is stale, duplicated, exceeds known fragments, or has a
    /// commission currency that differs from those fragments.
    pub fn apply_fill_void(
        &mut self,
        event: OrderFillVoided,
        voided_qty: Quantity,
        commission_voided: Option<Money>,
    ) -> anyhow::Result<Option<Money>> {
        let fragments = self.fill_fragments(event.client_order_id, event.trade_id);

        qty(voided_qty)?;

        let fragment_qty = fragments
            .iter()
            .try_fold(Quantity::zero(self.size_precision), |total, fill| {
                quantity_add(total, fill.last_qty, self.size_precision)
            })?;

        anyhow::ensure!(
            !voided_qty.is_zero() && voided_qty <= fragment_qty,
            "position fill void exceeds known fragments for {}",
            event.trade_id,
        );

        if let Some(commission_voided) = commission_voided {
            for commission in fragments.iter().filter_map(|fill| fill.commission) {
                anyhow::ensure!(
                    commission.currency == commission_voided.currency,
                    "position commission currency differs for fill {}",
                    event.trade_id,
                );
            }
        }

        if let Some(previous) = self.fill_voids.iter().rev().find(|record| {
            record.event.client_order_id == event.client_order_id
                && record.event.trade_id == event.trade_id
        }) {
            anyhow::ensure!(
                voided_qty >= previous.voided_qty,
                "stale position fill void for {}",
                event.trade_id,
            );
            anyhow::ensure!(
                voided_qty != previous.voided_qty
                    || commission_voided != previous.commission_voided,
                "duplicate position fill void for {}",
                event.trade_id,
            );
        }

        let mut next = self.clone();
        next.fill_voids.push(PositionFillVoid {
            event,
            voided_qty,
            commission_voided,
        });

        let closed = next.rebuild_from_replay()?;
        *self = next;
        Ok(closed)
    }

    /// Returns durable fill fragments matching an order trade in local application order.
    #[must_use]
    pub fn fill_fragments(
        &self,
        client_order_id: ClientOrderId,
        trade_id: TradeId,
    ) -> Vec<&OrderFilled> {
        let restatement_indexes = Self::price_restatement_indexes(&self.replay_events);
        self.replay_events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match event {
                PositionReplayEvent::Filled(fill)
                    if fill.client_order_id == client_order_id
                        && fill.trade_id == trade_id
                        && !restatement_indexes.contains(&index) =>
                {
                    Some(fill)
                }
                _ => None,
            })
            .collect()
    }

    // The banked total assumes `replay_events` spans every cycle this position archived, since
    // settling replaces all of its frames with one worth that total. Bounding the log has to
    // preserve it at trim time; `Cache::settle_position_snapshots` documents the two ways.
    fn rebuild_from_replay(&mut self) -> CorrectnessResult<Option<Money>> {
        let replay_events = self.replay_events.clone();
        let restatement_indexes = Self::price_restatement_indexes(&replay_events);
        let mut quantity_removed = AHashMap::<usize, Quantity>::new();
        let mut commission_removed = AHashMap::<usize, Money>::new();

        for correction in self.latest_fill_voids() {
            let mut remaining_qty = correction.voided_qty;
            let mut remaining_commission = correction.commission_voided;

            for (index, replay_event) in replay_events.iter().enumerate().rev() {
                let Some(fill) =
                    Self::voidable_replay_fill(&restatement_indexes, index, replay_event)
                else {
                    continue;
                };

                if fill.client_order_id != correction.event.client_order_id
                    || fill.trade_id != correction.event.trade_id
                {
                    continue;
                }

                if !remaining_qty.is_zero() {
                    let removed = remaining_qty.min(fill.last_qty);
                    quantity_removed.insert(index, removed);
                    remaining_qty = quantity_sub(remaining_qty, removed, remaining_qty.precision)?;
                }

                if let (Some(remaining), Some(commission)) = (remaining_commission, fill.commission)
                {
                    let magnitude = remaining.abs().min(commission.abs());

                    let removed = if remaining.is_negative() {
                        -magnitude
                    } else {
                        magnitude
                    };

                    commission_removed.insert(index, removed);
                    let next = remaining - removed;
                    remaining_commission = (!next.is_zero()).then_some(next);
                }
            }
        }

        self.reset_derived_state();

        let mut closed_cycles_pnl: Option<Money> = None;

        for (index, replay_event) in replay_events.iter().enumerate() {
            match replay_event {
                PositionReplayEvent::Filled(fill) => {
                    if restatement_indexes.contains(&index) {
                        self.apply_price_restatement(fill, false)?;
                        continue;
                    }

                    let removed = quantity_removed
                        .get(&index)
                        .copied()
                        .unwrap_or_else(|| Quantity::zero(fill.last_qty.precision));
                    let effective_qty =
                        quantity_sub(fill.last_qty, removed, fill.last_qty.precision)?;

                    let effective_commission =
                        match (fill.commission, commission_removed.get(&index).copied()) {
                            (Some(commission), Some(removed)) => Some(commission - removed),
                            (commission, None) => commission,
                            (None, Some(_)) => None,
                        };

                    if effective_qty.is_zero() {
                        if let Some(commission) =
                            effective_commission.filter(|commission| !commission.is_zero())
                            && let Some(realized_pnl) =
                                self.apply_surviving_fill_commission(fill, commission)?
                        {
                            closed_cycles_pnl =
                                Some(closed_cycles_pnl.map_or(Ok(realized_pnl), |total| {
                                    total
                                        .checked_add(realized_pnl)
                                        .ok_or_else(|| invalid("closed cycle PnL overflow"))
                                })?);
                        }
                        continue;
                    }

                    // `apply_fill` clears realized PnL when it reopens from flat, so bank the
                    // closing cycle's total before it goes
                    if self.side == PositionSide::Flat
                        && let Some(realized_pnl) = self.realized_pnl
                    {
                        closed_cycles_pnl =
                            Some(closed_cycles_pnl.map_or(Ok(realized_pnl), |total| {
                                total
                                    .checked_add(realized_pnl)
                                    .ok_or_else(|| invalid("closed cycle PnL overflow"))
                            })?);
                    }

                    let mut effective = fill.clone();
                    effective.last_qty = effective_qty;
                    effective.commission = effective_commission;
                    self.apply_fill(&effective, false)?;
                }
                PositionReplayEvent::Adjusted(adjustment) => {
                    self.apply_adjustment_state(*adjustment, false)?;
                }
                PositionReplayEvent::InstrumentClosed(close) => {
                    self.apply_instrument_close_state(*close, false)
                        .map_err(|e| invalid(&e.to_string()))?;
                }
            }
        }

        Ok(closed_cycles_pnl)
    }

    fn apply_surviving_fill_commission(
        &mut self,
        fill: &OrderFilled,
        commission: Money,
    ) -> CorrectnessResult<Option<Money>> {
        let is_base_commission =
            self.is_currency_pair && self.base_currency == Some(commission.currency);
        let reopens =
            self.side == PositionSide::Flat && is_base_commission && !amount(commission)?.is_zero();
        let closed_cycles_pnl_previous = reopens.then_some(self.realized_pnl).flatten();

        if reopens {
            self.reset_cycle(fill)?;
        }

        self.commissions
            .entry(commission.currency)
            .or_insert(Money::zero(commission.currency));
        let total = self.commissions[&commission.currency]
            .checked_add(commission)
            .ok_or_else(|| invalid("position commission overflow"))?;
        self.commissions.insert(commission.currency, total);
        if commission.currency == self.settlement_currency {
            self.accumulate_pnl(-amount(commission)?)?;
        }

        if is_base_commission {
            let previous_side = self.side;
            self.apply_base_commission_adjustment(fill, commission)?;
            self.finalize_surviving_base_commission(fill, previous_side)?;
        } else {
            self.ts_last = fill.ts_event;
        }

        Ok(closed_cycles_pnl_previous)
    }

    fn finalize_surviving_base_commission(
        &mut self,
        fill: &OrderFilled,
        previous_side: PositionSide,
    ) -> CorrectnessResult<()> {
        if self.side == PositionSide::Flat {
            if previous_side != PositionSide::Flat {
                self.closing_order_id = Some(fill.client_order_id);
                self.ts_closed = Some(fill.ts_event);
                self.duration_ns = fill.ts_event.saturating_duration_since(self.ts_opened);
            }
        } else {
            self.entry = match self.side {
                PositionSide::Long => OrderSide::Buy,
                PositionSide::Short => OrderSide::Sell,
                PositionSide::Flat => unreachable!(),
            };

            if previous_side != PositionSide::Flat && previous_side != self.side {
                self.avg_px_open = price(fill.last_px)?;
                self.cost_open = mul_rounded(self.avg_px_open, self.signed_qty.abs())?;
            }
        }

        Ok(())
    }

    fn apply_base_commission_adjustment(
        &mut self,
        fill: &OrderFilled,
        commission: Money,
    ) -> CorrectnessResult<()> {
        let mut adjustment_id = fill.event_id.as_bytes();
        adjustment_id[15] ^= 0x01;

        let adjustment = PositionAdjusted::new(
            self.trader_id,
            self.strategy_id,
            self.instrument_id,
            self.id,
            self.account_id,
            PositionAdjustmentType::Commission,
            Some(-amount(commission)?),
            None,
            Some(fill.client_order_id.inner()),
            UUID4::from_bytes(adjustment_id),
            fill.ts_event,
            fill.ts_init,
        );
        self.apply_adjustment_state(adjustment, false)
    }

    fn latest_fill_voids(&self) -> Vec<&PositionFillVoid> {
        let mut latest = IndexMap::<(ClientOrderId, TradeId), &PositionFillVoid>::new();
        for correction in &self.fill_voids {
            latest.insert(
                (correction.event.client_order_id, correction.event.trade_id),
                correction,
            );
        }
        latest.into_values().collect()
    }

    fn reset_derived_state(&mut self) {
        self.events.clear();
        self.adjustments.clear();
        self.trade_ids.clear();
        self.buy_qty = Quantity::zero(self.size_precision);
        self.sell_qty = Quantity::zero(self.size_precision);
        self.commissions.clear();
        self.signed_qty = Decimal::ZERO;
        self.quantity = Quantity::zero(self.size_precision);
        self.peak_qty = Quantity::zero(self.size_precision);
        self.side = PositionSide::Flat;
        self.closing_order_id = None;
        self.ts_opened = UnixNanos::default();
        self.ts_last = UnixNanos::default();
        self.ts_closed = Some(UnixNanos::default());
        self.duration_ns = DurationNanos::default();
        self.cost_open = Decimal::ZERO;
        self.cost_close = Decimal::ZERO;
        self.qty_closed = Decimal::ZERO;
        self.realized_pnl_remainder = Decimal::ZERO;
        self.avg_px_open = Decimal::ZERO;
        self.avg_px_close = None;
        self.realized_pnl = None;
        self.realized_return = 0.0;
    }

    fn calculate_return(&self, open: Decimal, close: Decimal) -> f64 {
        if open.is_zero() {
            return 0.0;
        }

        let open = open.as_f64();
        let close = close.as_f64();
        let points = if self.side == PositionSide::Short {
            open - close
        } else {
            close - open
        };

        points / open
    }

    /// Returns the realized PnL increment for reducing this position at `last`.
    ///
    /// Carries the position's existing fractional PnL into the native Money rounding.
    ///
    /// # Errors
    ///
    /// Returns an error if inverse prices are invalid, precision is unsupported, or arithmetic overflows.
    pub fn try_realized_pnl(&self, last: Price, quantity: Quantity) -> CorrectnessResult<Money> {
        let qty = qty(quantity)?.min(self.signed_qty.abs());
        if qty.is_zero() {
            return Ok(Money::zero(self.settlement_currency));
        }

        let cost = if qty == self.signed_qty.abs() {
            self.cost_open
        } else {
            proportion(self.cost_open, qty, self.signed_qty.abs())?
        };

        let pnl = self.pnl_from_cost(
            price(last)?,
            qty,
            cost,
            self.signed_qty.abs(),
            self.cost_open,
        )?;
        let prior = self.realized_decimal()?;
        let total = add_pnl(prior, pnl, self.settlement_currency)?;
        let rounded = money(total, self.settlement_currency)?;
        rounded
            .checked_sub(
                self.realized_pnl
                    .unwrap_or(Money::zero(self.settlement_currency)),
            )
            .ok_or_else(|| invalid("position PnL increment overflow"))
    }

    /// Calculates PnL using Decimal prices and exact quantity.
    ///
    /// # Errors
    ///
    /// Returns an error if inverse prices are invalid, arithmetic overflows, or precision is unsupported.
    pub fn try_calculate_pnl(
        &self,
        avg_px_open: Decimal,
        avg_px_close: Decimal,
        quantity: Quantity,
    ) -> CorrectnessResult<Money> {
        let qty = qty(quantity)?.min(self.signed_qty.abs());
        if qty.is_zero() {
            return Ok(Money::zero(self.settlement_currency));
        }

        let cost = mul_rounded(avg_px_open, qty)?;
        let basis_cost = mul_rounded(avg_px_open, self.signed_qty.abs())?;
        let pnl = self.pnl_from_cost(avg_px_close, qty, cost, self.signed_qty.abs(), basis_cost)?;
        money(pnl, self.settlement_currency)
    }

    /// Calculates PnL using Decimal prices and exact quantity.
    ///
    /// # Errors
    ///
    /// Returns an error if [`Self::try_calculate_pnl`] fails.
    pub fn calculate_pnl(
        &self,
        avg_px_open: Decimal,
        avg_px_close: Decimal,
        quantity: Quantity,
    ) -> CorrectnessResult<Money> {
        self.try_calculate_pnl(avg_px_open, avg_px_close, quantity)
    }

    /// Returns total PnL at the given price.
    ///
    /// # Errors
    ///
    /// Returns an error if the PnL cannot be calculated or represented.
    pub fn try_total_pnl(&self, last: Price) -> CorrectnessResult<Money> {
        let unrealized = self.unrealized_decimal(last)?;
        let realized = self.realized_decimal()?;
        let total = add_pnl(realized, unrealized, self.settlement_currency)?;
        money(total, self.settlement_currency)
    }

    /// Returns total PnL at the given price.
    ///
    /// # Errors
    ///
    /// Returns an error if [`Self::try_total_pnl`] fails.
    pub fn total_pnl(&self, last: Price) -> CorrectnessResult<Money> {
        self.try_total_pnl(last)
    }

    /// Returns unrealized PnL at the given price.
    ///
    /// # Errors
    ///
    /// Returns an error if the PnL cannot be calculated or represented.
    pub fn try_unrealized_pnl(&self, last: Price) -> CorrectnessResult<Money> {
        money(self.unrealized_decimal(last)?, self.settlement_currency)
    }

    fn unrealized_decimal(&self, last: Price) -> CorrectnessResult<Decimal> {
        if self.side == PositionSide::Flat {
            return Ok(Decimal::ZERO);
        }

        self.pnl_from_cost(
            price(last)?,
            self.signed_qty.abs(),
            self.cost_open,
            self.signed_qty.abs(),
            self.cost_open,
        )
    }

    /// Returns unrealized PnL at the given price.
    ///
    /// # Errors
    ///
    /// Returns an error if [`Self::try_unrealized_pnl`] fails.
    pub fn unrealized_pnl(&self, last: Price) -> CorrectnessResult<Money> {
        self.try_unrealized_pnl(last)
    }

    /// Returns the order side required to close this position.
    #[must_use]
    pub fn closing_order_side(&self) -> Option<OrderSide> {
        match self.side {
            PositionSide::Long => Some(OrderSide::Sell),
            PositionSide::Short => Some(OrderSide::Buy),
            PositionSide::Flat => None,
        }
    }

    /// Returns whether the given order side is opposite to the position entry side.
    #[must_use]
    pub fn is_opposite_side(&self, side: OrderSide) -> bool {
        self.entry != side
    }

    /// Returns the instrument symbol.
    #[must_use]
    pub fn symbol(&self) -> Symbol {
        self.instrument_id.symbol
    }

    /// Returns the trading venue.
    #[must_use]
    pub fn venue(&self) -> Venue {
        self.instrument_id.venue
    }

    /// Returns the count of order fill events applied to this position.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Returns unique client order IDs from all fill events, sorted.
    #[must_use]
    pub fn client_order_ids(&self) -> Vec<ClientOrderId> {
        // First to hash set to remove duplicate, then again iter to vector
        let mut result = self
            .events
            .iter()
            .map(|event| event.client_order_id)
            .collect::<AHashSet<ClientOrderId>>()
            .into_iter()
            .collect::<Vec<ClientOrderId>>();
        result.sort_unstable();
        result
    }

    /// Returns unique venue order IDs from all fill events, sorted.
    #[must_use]
    pub fn venue_order_ids(&self) -> Vec<VenueOrderId> {
        // First to hash set to remove duplicate, then again iter to vector
        let mut result = self
            .events
            .iter()
            .map(|event| event.venue_order_id)
            .collect::<AHashSet<VenueOrderId>>()
            .into_iter()
            .collect::<Vec<VenueOrderId>>();
        result.sort_unstable();
        result
    }

    /// Returns unique trade IDs from all fill events, sorted.
    #[must_use]
    pub fn trade_ids(&self) -> Vec<TradeId> {
        let mut result = self
            .events
            .iter()
            .map(|event| event.trade_id)
            .collect::<AHashSet<TradeId>>()
            .into_iter()
            .collect::<Vec<TradeId>>();
        result.sort_unstable();
        result
    }

    /// Calculates the notional value based on the last price.
    ///
    /// # Errors
    ///
    /// Returns an error if this is an inverse position without a base currency, the price is not
    /// positive for a non-premium inverse position, or the result cannot be represented as
    /// [`Money`].
    pub fn try_notional_value(&self, last: Price) -> anyhow::Result<Money> {
        let currency = if self.is_inverse {
            self.base_currency.ok_or_else(|| {
                anyhow::anyhow!(
                    "inverse position {} has no base currency",
                    self.instrument_id
                )
            })?
        } else {
            self.settlement_currency
        };

        notional(
            self.quantity,
            last,
            self.multiplier,
            self.instrument_class
                .divides_notional_by_price(self.is_inverse),
            currency,
        )
        .map_err(Into::into)
    }

    /// Calculates the notional value based on the last price.
    ///
    /// # Panics
    ///
    /// Panics if [`Position::try_notional_value`] returns an error.
    #[must_use]
    pub fn notional_value(&self, last: Price) -> Money {
        self.try_notional_value(last)
            .expect("invalid notional value")
    }

    /// Returns the last `OrderFilled` event for the position (if any after purging).
    #[must_use]
    pub fn last_event(&self) -> Option<OrderFilled> {
        self.events.last().cloned()
    }

    /// Returns the last `TradeId` for the position (if any after purging).
    #[must_use]
    pub fn last_trade_id(&self) -> Option<TradeId> {
        self.events.last().map(|e| e.trade_id)
    }

    /// Returns whether the position is long (positive quantity).
    #[must_use]
    pub fn is_long(&self) -> bool {
        self.side == PositionSide::Long
    }

    /// Returns whether the position is short (negative quantity).
    #[must_use]
    pub fn is_short(&self) -> bool {
        self.side == PositionSide::Short
    }

    /// Returns whether the position is currently open (has quantity and no close timestamp).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.side != PositionSide::Flat && self.ts_closed.is_none()
    }

    /// Returns whether the position is closed (flat with a close timestamp).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.side == PositionSide::Flat && self.ts_closed.is_some()
    }

    /// Returns whether an authoritative instrument close settled the current cycle.
    ///
    /// Reads the replay history, so it is false for a copy made by
    /// [`Self::clone_without_events`] or [`Self::clone_for_snapshot`].
    #[must_use]
    pub fn is_settled(&self) -> bool {
        matches!(
            self.replay_events.last(),
            Some(PositionReplayEvent::InstrumentClosed(_))
        )
    }

    /// Returns whether replaying the position's fills alone cannot rebuild its state.
    ///
    /// Fill voids, settlements, and price restatements change state without a new
    /// quantity-bearing fill, so persistence must store the complete replay state.
    /// Retained virtual position cycles also require the complete replay state.
    #[must_use]
    pub fn requires_replay_state(&self) -> bool {
        // A price restatement is stored only in replay, so equal lengths cannot contain one
        !self.fill_voids.is_empty()
            || self.is_settled()
            || (self.replay_events.len() > self.events.len() && self.has_replay_price_restatement())
            || (self.id.is_virtual() && self.replay_events.len() > self.events.len())
    }

    /// Returns the signed quantity as a `Decimal`.
    ///
    /// Uses the authoritative Decimal signed quantity, as the `quantity`
    /// field may have reduced precision based on the instrument's `size_precision`.
    #[must_use]
    pub fn signed_decimal_qty(&self) -> Decimal {
        self.signed_qty
    }

    /// Returns the cumulative commissions for the position as a vector.
    #[must_use]
    pub fn commissions(&self) -> Vec<Money> {
        self.commissions.values().copied().collect()
    }
}

impl PartialEq<Self> for Position {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Position {}

impl Hash for Position {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quantity_str = if self.quantity == Quantity::zero(self.size_precision) {
            String::new()
        } else {
            self.quantity.to_formatted_string() + " "
        };

        write!(
            f,
            "Position({} {}{}, id={})",
            self.side, quantity_str, self.instrument_id, self.id
        )
    }
}

fn invalid(message: &str) -> CorrectnessError {
    CorrectnessError::PredicateViolation {
        message: message.to_owned(),
    }
}

pub(crate) fn from_raw(mut raw: i128, mut scale: u32) -> CorrectnessResult<Decimal> {
    if let Ok(value) = Decimal::try_from_i128_with_scale(raw, scale) {
        return Ok(value.normalize());
    }

    while scale > 0 && raw % 10 == 0 {
        raw /= 10;
        scale -= 1;
    }

    Decimal::try_from_i128_with_scale(raw, scale)
        .map_err(|_| invalid("position input exceeds exact Decimal representation"))
}

#[allow(
    clippy::useless_conversion,
    reason = "raw integer widths vary between standard and high precision"
)]
pub(crate) fn price(value: Price) -> CorrectnessResult<Decimal> {
    from_raw(
        i128::from(value.raw),
        u32::from(FIXED_PRECISION.max(value.precision)),
    )
}

#[allow(
    clippy::unnecessary_fallible_conversions,
    reason = "high precision quantities use u128 and require a checked signed conversion"
)]
pub(crate) fn qty(value: Quantity) -> CorrectnessResult<Decimal> {
    let raw = i128::try_from(value.raw)
        .map_err(|_| invalid("position quantity exceeds Decimal bounds"))?;
    from_raw(raw, u32::from(FIXED_PRECISION.max(value.precision)))
}

#[allow(
    clippy::useless_conversion,
    reason = "raw integer widths vary between standard and high precision"
)]
pub(crate) fn amount(value: Money) -> CorrectnessResult<Decimal> {
    from_raw(
        i128::from(value.raw),
        u32::from(FIXED_PRECISION.max(value.currency.precision)),
    )
}

pub(crate) fn quantity(value: Decimal, precision: u8) -> CorrectnessResult<Quantity> {
    let precision = precision.max(value.normalize().scale() as u8);
    let quantity = Quantity::from_decimal_dp(value, precision)?;
    if qty(quantity)? != value {
        return Err(invalid("position quantity loses instrument precision"));
    }

    Ok(quantity)
}

pub(crate) fn quantity_add(
    lhs: Quantity,
    rhs: Quantity,
    precision: u8,
) -> CorrectnessResult<Quantity> {
    if let Some(sum) = lhs.checked_add(rhs)
        && let Some(quantity) = quantity_native(sum, precision)
    {
        return Ok(quantity);
    }

    quantity(add(qty(lhs)?, qty(rhs)?)?, precision)
}

#[allow(clippy::useless_conversion, reason = "QuantityRaw is u64 or u128")]
pub(crate) fn quantity_sub(
    lhs: Quantity,
    rhs: Quantity,
    precision: u8,
) -> CorrectnessResult<Quantity> {
    if u128::from(lhs.raw) <= Decimal::MAX.mantissa() as u128
        && let Some(difference) = lhs.checked_sub(rhs)
        && let Some(quantity) = quantity_native(difference, precision)
    {
        return Ok(quantity);
    }

    quantity(sub(qty(lhs)?, qty(rhs)?)?, precision)
}

#[allow(clippy::useless_conversion, reason = "QuantityRaw is u64 or u128")]
fn quantity_native(value: Quantity, precision: u8) -> Option<Quantity> {
    if !raw_scales_match(value.precision, precision)
        || u128::from(value.raw) > Decimal::MAX.mantissa() as u128
    {
        return None;
    }

    let step = QuantityRaw::pow(
        10,
        u32::from(FIXED_PRECISION.max(value.precision) - precision),
    );

    if !value.raw.is_multiple_of(step) {
        return None;
    }

    Quantity::from_raw_checked(value.raw, precision).ok()
}

pub(crate) fn proportion(
    value: Decimal,
    numerator: Decimal,
    denominator: Decimal,
) -> CorrectnessResult<Decimal> {
    if denominator.is_zero() {
        return Err(invalid("position proportion has zero denominator"));
    }

    if value.is_zero() || numerator.is_zero() {
        return Ok(Decimal::ZERO);
    }

    if let Ok(product) = mul(value, numerator)
        && let Ok(result) = div(product, denominator)
        && sub(value, result).is_ok()
    {
        return Ok(result);
    }

    let raw = U512::from(value.mantissa().unsigned_abs())
        * U512::from(numerator.mantissa().unsigned_abs());
    let divisor = U512::from(denominator.mantissa().unsigned_abs());
    let negative =
        value.is_sign_negative() ^ numerator.is_sign_negative() ^ denominator.is_sign_negative();
    for scale in (0..=Decimal::MAX_SCALE).rev() {
        let exponent = scale as i32 + denominator.scale() as i32
            - value.scale() as i32
            - numerator.scale() as i32;
        let ten = U512::from(10_u8);
        let (dividend, divisor) = if exponent >= 0 {
            (raw * ten.pow(U512::from(exponent as u32)), divisor)
        } else {
            (raw, divisor * ten.pow(U512::from(exponent.unsigned_abs())))
        };

        let Some(result) = decimal_quotient(dividend, divisor, scale, negative) else {
            continue;
        };

        if result.is_zero() {
            return Err(invalid("position proportional allocation underflow"));
        }

        if sub(value, result).is_ok() {
            return Ok(result);
        }
    }

    Err(invalid("position proportional allocation overflow"))
}

fn decimal_quotient(dividend: U512, divisor: U512, scale: u32, negative: bool) -> Option<Decimal> {
    let mut quotient = dividend / divisor;
    let doubled = (dividend % divisor) * U512::from(2_u8);
    if doubled > divisor || (doubled == divisor && quotient % U512::from(2_u8) != U512::ZERO) {
        quotient += U512::ONE;
    }

    let mantissa = u128::try_from(quotient).ok()?;
    let mantissa = i128::try_from(mantissa).ok()?;
    from_raw(if negative { -mantissa } else { mantissa }, scale).ok()
}

pub(crate) fn mul_rounded(lhs: Decimal, rhs: Decimal) -> CorrectnessResult<Decimal> {
    let value = lhs
        .checked_mul(rhs)
        .ok_or_else(|| invalid("position proportional arithmetic overflow"))?;

    if !lhs.is_zero() && !rhs.is_zero() && value.is_zero() {
        return Err(invalid("position proportional arithmetic underflow"));
    }

    Ok(value.normalize())
}

pub(crate) fn add(lhs: Decimal, rhs: Decimal) -> CorrectnessResult<Decimal> {
    let scale = lhs.scale().max(rhs.scale());
    let left_scale = 10_i128.pow(scale - lhs.scale());
    let right_scale = 10_i128.pow(scale - rhs.scale());
    if let Some(raw) = lhs.mantissa().checked_mul(left_scale).and_then(|left| {
        rhs.mantissa()
            .checked_mul(right_scale)
            .and_then(|right| left.checked_add(right))
    }) {
        return from_raw(raw, scale)
            .map_err(|e| invalid(&format!("position exact sum {lhs} + {rhs}: {e}")));
    }

    let left = U256::from(lhs.mantissa().unsigned_abs()) * U256::from(left_scale as u128);
    let right = U256::from(rhs.mantissa().unsigned_abs()) * U256::from(right_scale as u128);
    let (raw, negative) = if lhs.is_sign_negative() == rhs.is_sign_negative() {
        (left + right, lhs.is_sign_negative())
    } else if left >= right {
        (left - right, lhs.is_sign_negative())
    } else {
        (right - left, rhs.is_sign_negative())
    };

    from_wide(raw, scale, negative)
}

pub(crate) fn add_rounded(lhs: Decimal, rhs: Decimal) -> CorrectnessResult<Decimal> {
    let result = lhs
        .checked_add(rhs)
        .ok_or_else(|| invalid("position Decimal addition overflow"))?;

    if (!rhs.is_zero() && result == lhs) || (!lhs.is_zero() && result == rhs) {
        return Err(invalid(
            "position Decimal addition loses the entire increment",
        ));
    }

    Ok(result)
}

fn add_pnl(lhs: Decimal, rhs: Decimal, currency: Currency) -> CorrectnessResult<Decimal> {
    let result = lhs
        .checked_add(rhs)
        .ok_or_else(|| invalid("position Decimal addition overflow"))?;

    let lost = if result == lhs {
        rhs
    } else if result == rhs {
        lhs
    } else {
        Decimal::ZERO
    };

    if !lost.is_zero() && lost.abs() >= amount(Money::from_raw_checked(1, currency)?)? {
        return Err(invalid(
            "position PnL addition loses at least one native Money unit",
        ));
    }

    Ok(result)
}

pub(crate) fn sub(lhs: Decimal, rhs: Decimal) -> CorrectnessResult<Decimal> {
    add(lhs, -rhs)
}

pub(crate) fn mul(lhs: Decimal, rhs: Decimal) -> CorrectnessResult<Decimal> {
    let scale = lhs.scale() + rhs.scale();
    if let Some(raw) = lhs.mantissa().checked_mul(rhs.mantissa()) {
        return from_raw(raw, scale);
    }

    let raw = U256::from(lhs.mantissa().unsigned_abs()) * U256::from(rhs.mantissa().unsigned_abs());
    from_wide(raw, scale, lhs.is_sign_negative() != rhs.is_sign_negative())
}

fn from_wide(mut raw: U256, mut scale: u32, negative: bool) -> CorrectnessResult<Decimal> {
    let ten = U256::from(10_u8);
    while scale > 0 && raw % ten == U256::ZERO {
        raw /= ten;
        scale -= 1;
    }

    let raw: u128 = raw
        .try_into()
        .map_err(|_| invalid("position Decimal arithmetic overflow"))?;
    let signed =
        i128::try_from(raw).map_err(|_| invalid("position Decimal arithmetic overflow"))?;
    from_raw(if negative { -signed } else { signed }, scale)
}

pub(crate) fn div(lhs: Decimal, rhs: Decimal) -> CorrectnessResult<Decimal> {
    let value = lhs
        .checked_div(rhs)
        .ok_or_else(|| invalid("position Decimal division overflow or zero divisor"))?;

    if !lhs.is_zero() && value.is_zero() {
        return Err(invalid("position Decimal division underflow"));
    }

    Ok(value)
}

pub(crate) fn money(amount: Decimal, currency: Currency) -> CorrectnessResult<Money> {
    Money::from_decimal_native(amount, currency)
}

#[allow(
    clippy::useless_conversion,
    reason = "the raw width differs when high-precision is disabled"
)]
pub(crate) fn money_from_qty(value: Quantity, currency: Currency) -> CorrectnessResult<Money> {
    if raw_scales_match(value.precision, currency.precision)
        && u128::from(value.raw) <= Decimal::MAX.mantissa() as u128
        && let Ok(raw) = value.raw.try_into()
    {
        return Money::from_raw_checked(raw, currency);
    }

    money(qty(value)?, currency)
}

pub(crate) fn notional(
    quantity: Quantity,
    last: Price,
    multiplier: Quantity,
    divides_by_price: bool,
    currency: Currency,
) -> CorrectnessResult<Money> {
    let product = mul_rounded(qty(quantity)?, qty(multiplier)?)?;
    let price = price(last)?;
    let amount = if divides_by_price {
        if price <= Decimal::ZERO {
            return Err(invalid(
                "price must be positive for inverse notional valuation",
            ));
        }

        div(product, price)?
    } else {
        mul_rounded(product, price)?
    };

    money(amount, currency)
}

#[allow(
    clippy::useless_conversion,
    clippy::unnecessary_fallible_conversions,
    reason = "raw integer widths vary between standard and high precision"
)]
pub(crate) fn split_commission_qty(
    commission: Money,
    closing_qty: Quantity,
    total_qty: Quantity,
) -> CorrectnessResult<(Money, Money)> {
    if closing_qty.non_zero()
        && closing_qty < total_qty
        && raw_scales_match(closing_qty.precision, total_qty.precision)
        && u128::from(total_qty.raw) <= Decimal::MAX.mantissa() as u128
        && let Some(magnitude) = checked_mul_div_raw(
            commission.raw.unsigned_abs(),
            closing_qty.raw,
            total_qty.raw,
        )
    {
        let raw = i128::try_from(magnitude)
            .map_err(|_| invalid("commission split exceeds raw bounds"))?;
        return commission_parts(
            commission,
            if commission.is_negative() { -raw } else { raw },
        );
    }

    split_commission(commission, qty(closing_qty)?, qty(total_qty)?)
}

#[allow(
    clippy::useless_conversion,
    reason = "raw integer widths vary between standard and high precision"
)]
pub(crate) fn split_commission(
    commission: Money,
    closing_qty: Decimal,
    total_qty: Decimal,
) -> CorrectnessResult<(Money, Money)> {
    if closing_qty <= Decimal::ZERO || closing_qty >= total_qty {
        return Err(invalid(
            "commission split requires a positive closing quantity below the total",
        ));
    }

    let raw = i128::from(commission.raw);
    let mut numerator =
        U512::from(raw.unsigned_abs()) * U512::from(closing_qty.mantissa().unsigned_abs());
    let mut denominator = U512::from(total_qty.mantissa().unsigned_abs());
    let ten = U512::from(10_u8);
    if total_qty.scale() >= closing_qty.scale() {
        numerator *= ten.pow(U512::from(total_qty.scale() - closing_qty.scale()));
    } else {
        denominator *= ten.pow(U512::from(closing_qty.scale() - total_qty.scale()));
    }

    let magnitude: u128 = (numerator / denominator)
        .try_into()
        .map_err(|_| invalid("commission split exceeds raw bounds"))?;
    let magnitude =
        i128::try_from(magnitude).map_err(|_| invalid("commission split exceeds raw bounds"))?;
    commission_parts(commission, if raw < 0 { -magnitude } else { magnitude })
}

#[allow(clippy::useless_conversion, reason = "MoneyRaw is i64 or i128")]
fn commission_parts(commission: Money, closing_raw: i128) -> CorrectnessResult<(Money, Money)> {
    let closing_raw = closing_raw
        .try_into()
        .map_err(|_| invalid("commission split exceeds native Money bounds"))?;
    let closing = Money::from_raw(closing_raw, commission.currency);
    let opening = commission
        .checked_sub(closing)
        .ok_or_else(|| invalid("position commission allocation overflow"))?;
    Ok((closing, opening))
}

/// Replays position legs onto a hypothetical NETTING position in `ts_opened`
/// order, returning `(net_signed_qty, net_avg_px_open)`.
///
/// Each leg is `(signed_qty, avg_px_open, ts_opened_ns)`. Rules follow
/// [`Position::apply`]:
/// - Same-side legs produce a quantity-weighted average open price.
/// - Opposite-side legs partial-close at the existing average.
/// - A leg that crosses zero makes the residual take that leg's price.
///
/// Zero-quantity legs are skipped. Sort is stable on `ts_opened`; the caller
/// orders ties (e.g. by `position_id`).
#[must_use]
pub fn fold_net_position(legs: &[(Decimal, Decimal, u64)]) -> (Decimal, Decimal) {
    let mut sorted: Vec<&(Decimal, Decimal, u64)> =
        legs.iter().filter(|(qty, _, _)| !qty.is_zero()).collect();
    sorted.sort_by_key(|(_, _, ts_opened)| *ts_opened);

    let mut net_signed_qty = Decimal::ZERO;
    let mut net_avg_px = Decimal::ZERO;

    for &(p_qty, p_px, _) in sorted {
        if net_signed_qty.is_zero() {
            net_signed_qty = p_qty;
            net_avg_px = p_px;
            continue;
        }

        let same_side = net_signed_qty.is_sign_negative() == p_qty.is_sign_negative();
        let new_net = net_signed_qty + p_qty;
        if same_side {
            let total_abs = net_signed_qty.abs() + p_qty.abs();
            net_avg_px = (net_signed_qty.abs() * net_avg_px + p_qty.abs() * p_px) / total_abs;
            net_signed_qty = new_net;
        } else if new_net.is_zero()
            || new_net.is_sign_negative() == net_signed_qty.is_sign_negative()
        {
            net_signed_qty = new_net;
            if new_net.is_zero() {
                net_avg_px = Decimal::ZERO;
            }
        } else {
            net_signed_qty = new_net;
            net_avg_px = p_px;
        }
    }

    (net_signed_qty, net_avg_px)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use ahash::AHashSet;
    use nautilus_core::{DurationNanos, UUID4, UnixNanos, correctness::CorrectnessError};
    use proptest::prelude::*;
    use rstest::rstest;
    use rust_decimal::{Decimal, prelude::ToPrimitive};
    use rust_decimal_macros::dec;

    use super::*;
    #[cfg(feature = "defi")]
    use crate::enums::CurrencyType;
    use crate::{
        data::InstrumentClose,
        enums::{InstrumentCloseType, OrderSide, OrderType, PositionAdjustmentType, PositionSide},
        events::{
            OrderEventAny, OrderFillVoided, OrderFilled, PositionAdjusted, PositionSnapshot,
            order::spec::{OrderFillVoidedSpec, OrderFilledSpec},
        },
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, VenueOrderId,
            stubs::uuid4,
        },
        instruments::{
            BinaryOption, CryptoFuture, CryptoOption, CryptoPerpetual, CurrencyPair, Instrument,
            InstrumentAny, stubs::*,
        },
        orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        stubs::*,
        types::{
            Currency, Money, Price, Quantity,
            money::{MONEY_RAW_MAX, MoneyRaw},
        },
    };

    fn native_money(value: Decimal, currency: Currency) -> Money {
        let value = value.round_dp_with_strategy(
            u32::from(crate::types::fixed::FIXED_PRECISION),
            rust_decimal::RoundingStrategy::MidpointNearestEven,
        );
        Money::from_decimal_exact(value, currency).unwrap()
    }

    #[rstest]
    fn test_decimal_intermediate_price_restatement_after_third_reduction(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-THIRD-RESTATE"))
            .last_qty(Quantity::from(100_000))
            .last_px(Price::from("1.00001"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, opening.clone());
        let mut addition = opening.clone();
        addition.trade_id = TradeId::from("T-ADD");
        addition.event_id = UUID4::new();
        addition.last_qty = Quantity::from(200_000);
        addition.last_px = Price::from("1.00002");
        position.try_apply(&addition).unwrap();
        let mut reduction = opening.clone();
        reduction.trade_id = TradeId::from("T-REDUCE");
        reduction.event_id = UUID4::new();
        reduction.order_side = OrderSide::Sell;
        reduction.last_px = Price::from("1.00004");
        position.try_apply(&reduction).unwrap();
        let realized_before = position.realized_pnl;
        let mut restatement = opening;
        restatement.event_id = UUID4::new();
        restatement.last_px = Price::from("1.00003");

        position.try_apply(&restatement).unwrap();

        assert_eq!(position.quantity, Quantity::from(200_000));
        assert_eq!(position.signed_qty, dec!(200000));
        assert_eq!(position.cost_open, dec!(200004.66666666666666666666666));
        assert_eq!(position.avg_px_open, dec!(1.0000233333333333333333333333));
        assert_eq!(position.realized_pnl, realized_before);
        assert_eq!(position.events.len(), 3);
        assert_eq!(position.replay_events.len(), 4);
    }

    #[rstest]
    #[case(OrderSide::Buy, dec!(1))]
    #[case(OrderSide::Sell, dec!(-1))]
    fn test_decimal_intermediate_inverse_large_move(
        btcusd_bybit: CryptoPerpetual,
        #[case] side: OrderSide,
        #[case] sign: Decimal,
    ) {
        let instrument = InstrumentAny::CryptoPerpetual(btcusd_bybit);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-INVERSE-MOVE"))
            .order_side(side)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("0.0625"))
            .commission(Money::zero(Currency::BTC()))
            .build();
        let mut position = Position::new(&instrument, opening.clone());
        let mut settled = position.clone();
        let mark = Price::from("0.15");
        let expected = native_money(sign * dec!(28) / dec!(3), Currency::BTC());
        let unrealized = position.try_unrealized_pnl(mark).unwrap();
        let realized = position.try_realized_pnl(mark, position.quantity).unwrap();
        let mut closing = opening;
        closing.trade_id = TradeId::from("T-CLOSE");
        closing.event_id = UUID4::new();

        closing.order_side = if side == OrderSide::Buy {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };

        closing.last_px = mark;
        position.try_apply(&closing).unwrap();
        let close = contract_expired(&instrument, "0.15");
        settled.apply_instrument_close(close).unwrap();

        assert_eq!(unrealized, expected);
        assert_eq!(realized, expected);
        assert_eq!(position.realized_pnl, Some(expected));
        assert_eq!(settled.realized_pnl, Some(expected));
        assert_eq!(position.quantity, Quantity::from(0));
        assert_eq!(settled.quantity, Quantity::from(0));
        assert_eq!(position.side, PositionSide::Flat);
        assert!(settled.is_settled());
    }

    #[rstest]
    fn test_position_accepts_pnl_after_break_even_residual(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-BREAK-EVEN"))
            .last_qty(Quantity::from(5))
            .last_px(Price::from("1.00000"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, opening.clone());

        for (trade_id, side, qty, px) in [
            ("T-ADD", OrderSide::Buy, 10, "0.60000"),
            ("T-REDUCE-1", OrderSide::Sell, 1, "0.70000"),
            ("T-REDUCE-2", OrderSide::Sell, 5, "0.74000"),
        ] {
            let fill = OrderFilled {
                trade_id: TradeId::from(trade_id),
                order_side: side,
                last_qty: Quantity::from(qty),
                last_px: Price::from(px),
                event_id: UUID4::new(),
                ..opening.clone()
            };

            position.try_apply(&fill).unwrap();
        }

        assert_eq!(position.realized_pnl, Some(Money::zero(Currency::USD())));
        assert_eq!(
            position.realized_pnl_remainder,
            dec!(0.0000000000000000000000000002)
        );

        let closing = OrderFilled {
            trade_id: TradeId::from("T-CLOSE"),
            order_side: OrderSide::Sell,
            last_qty: Quantity::from(9),
            last_px: Price::from("2.00000"),
            event_id: UUID4::new(),
            ..opening
        };

        let realized = position.try_realized_pnl(closing.last_px, closing.last_qty);
        let total = position.try_total_pnl(closing.last_px);
        let expected = Money::from("11.40 USD");
        let mut adjusted = position.clone();

        let adjustment = PositionAdjusted::new(
            position.trader_id,
            position.strategy_id,
            position.instrument_id,
            position.id,
            position.account_id,
            PositionAdjustmentType::Funding,
            None,
            Some(expected),
            None,
            UUID4::new(),
            position.ts_last,
            position.ts_last,
        );
        let adjustment = adjusted
            .apply_adjustment(adjustment)
            .map(|()| adjusted.realized_pnl.unwrap());
        let closed = position
            .try_apply(&closing)
            .map(|()| position.realized_pnl.unwrap());
        let results: Vec<_> = [realized, total, adjustment, closed]
            .into_iter()
            .map(|result| result.map_err(|e| e.to_string()))
            .collect();

        assert_eq!(results, vec![Ok(expected); 4]);
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.quantity, Quantity::from(0));
        assert_eq!(position.cost_open, Decimal::ZERO);
        assert_eq!(position.realized_pnl_remainder, Decimal::ZERO);
        assert_eq!(adjusted.quantity, Quantity::from(9));
        assert_eq!(adjusted.realized_pnl_remainder, Decimal::ZERO);
    }

    #[rstest]
    #[case::without_fee("0 USD", "10.60 USD")]
    #[case::with_fee("0.01 USD", "10.59 USD")]
    fn test_position_accepts_residual_pnl_on_close(
        audusd_sim: CurrencyPair,
        #[case] commission: &str,
        #[case] expected: &str,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-RESIDUAL-CLOSE"))
            .last_qty(Quantity::from(5))
            .last_px(Price::from("1.00000"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, opening.clone());

        for (trade_id, side, qty, px) in [
            ("T-ADD", OrderSide::Buy, 10, "0.60000"),
            ("T-REDUCE-1", OrderSide::Sell, 1, "10.00000"),
            ("T-REDUCE-2", OrderSide::Sell, 5, "1.00000"),
            ("T-ADD-2", OrderSide::Buy, 1, "0.60000"),
        ] {
            let fill = OrderFilled {
                trade_id: TradeId::from(trade_id),
                order_side: side,
                last_qty: Quantity::from(qty),
                last_px: Price::from(px),
                event_id: UUID4::new(),
                ..opening.clone()
            };

            position.try_apply(&fill).unwrap();
        }

        let closing = OrderFilled {
            trade_id: TradeId::from("T-CLOSE"),
            order_side: OrderSide::Sell,
            last_qty: Quantity::from(10),
            last_px: Price::from("0.72000"),
            commission: Some(Money::from(commission)),
            event_id: UUID4::new(),
            ..opening
        };

        let realized = position.try_realized_pnl(closing.last_px, closing.last_qty);
        let total = position.try_total_pnl(closing.last_px);
        let closed = position.try_apply(&closing);

        assert_eq!(realized.unwrap(), Money::zero(Currency::USD()));
        assert_eq!(total.unwrap(), Money::from("10.60 USD"));
        closed.unwrap();
        assert_eq!(position.realized_pnl, Some(Money::from(expected)));
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.quantity, Quantity::from(0));
        assert_eq!(position.cost_open, Decimal::ZERO);
        assert_eq!(position.realized_pnl_remainder, Decimal::ZERO);
    }

    #[rstest]
    fn test_decimal_intermediate_restatement_after_two_reductions(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-REDUCED-LOTS"))
            .last_qty(Quantity::from("2.00"))
            .last_px(Price::from("12.00"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, opening.clone());
        for (id, side, qty, price) in [
            ("T-ADD", OrderSide::Buy, "100.00", "10.00"),
            ("T-REDUCE-1", OrderSide::Sell, "3.00", "15.00"),
            ("T-REDUCE-2", OrderSide::Sell, "1.00", "15.00"),
        ] {
            let mut fill = opening.clone();
            fill.trade_id = TradeId::from(id);
            fill.event_id = UUID4::new();
            fill.order_side = side;
            fill.last_qty = Quantity::from(qty);
            fill.last_px = Price::from(price);
            position.try_apply(&fill).unwrap();
        }

        let realized = position.realized_pnl;
        let mut restatement = opening;
        restatement.event_id = UUID4::new();
        restatement.last_px = Price::from("14.00");

        position.try_apply(&restatement).unwrap();

        assert_eq!(position.quantity, Quantity::from(98));
        assert_eq!(position.cost_open, dec!(987.686274509803921568627451));
        assert_eq!(position.avg_px_open, dec!(10.078431372549019607843137255));
        assert_eq!(position.realized_pnl, realized);
        assert_eq!(position.events.len(), 4);
        assert_eq!(position.replay_events.len(), 5);
    }

    #[rstest]
    fn test_decimal_intermediate_adjustment_on_flat_position(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-ADJUST-REOPEN"))
            .last_qty(Quantity::from(100_000))
            .last_px(Price::from("1.00001"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, opening);
        let mut addition = position.events[0].clone();
        addition.trade_id = TradeId::from("T-ADD-REOPEN");
        addition.event_id = UUID4::new();
        addition.last_qty = Quantity::from(23_457);
        addition.last_px = Price::from("1.00004");
        position.try_apply(&addition).unwrap();
        let qty = position.quantity;
        let average = position.avg_px_open;
        let mut closing = addition;
        closing.trade_id = TradeId::from("T-CLOSE-REOPEN");
        closing.event_id = UUID4::new();
        closing.order_side = OrderSide::Sell;
        closing.last_qty = qty;
        position.try_apply(&closing).unwrap();

        let adjustment = PositionAdjusted::new(
            position.trader_id,
            position.strategy_id,
            position.instrument_id,
            position.id,
            position.account_id,
            PositionAdjustmentType::Commission,
            Some(qty.as_decimal()),
            None,
            None,
            UUID4::new(),
            position.ts_last,
            position.ts_last,
        );

        position.apply_adjustment(adjustment).unwrap();

        assert_eq!(position.quantity, qty);
        assert_eq!(position.signed_qty, dec!(123457));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.avg_px_open, average);
        assert_eq!(position.cost_open, dec!(123458.93828));
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_position_accepts_rounded_dex_fill_cost(
        audusd_sim: CurrencyPair,
        #[case] side: OrderSide,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-DEX-COST"))
            .order_side(side)
            .last_qty(Quantity::from("0.0010000004030790"))
            .last_px(Price::from("1891.3472376376468003"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new_checked(&instrument, opening.clone()).unwrap();
        let expected_cost = dec!(1.8913480000000000000450346181);
        let notional = position.try_notional_value(opening.last_px).unwrap();
        let opening_cost = position.cost_open;
        let mut addition = opening.clone();
        addition.trade_id = TradeId::from("T-DEX-ADD");
        addition.event_id = UUID4::new();
        position.try_apply(&addition).unwrap();
        let added_cost = position.cost_open;
        let mut settled = position.clone();
        let mut closing = opening;
        closing.trade_id = TradeId::from("T-DEX-REDUCE");
        closing.event_id = UUID4::new();

        closing.order_side = if side == OrderSide::Buy {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };

        position.try_apply(&closing).unwrap();
        let remaining_cost = position.cost_open;
        closing.trade_id = TradeId::from("T-DEX-CLOSE");
        closing.event_id = UUID4::new();
        position.try_apply(&closing).unwrap();
        let close = contract_expired(&instrument, "1891.3472376376468003");
        settled.apply_instrument_close(close).unwrap();

        assert_eq!(opening_cost, expected_cost);
        assert_eq!(notional.as_decimal(), dec!(1.891348));
        assert_eq!(added_cost, dec!(3.7826960000000000000900692362));
        assert_eq!(remaining_cost, expected_cost);
        assert_eq!(position.cost_open, Decimal::ZERO);
        assert_eq!(position.cost_close, added_cost);
        assert_eq!(position.signed_qty, Decimal::ZERO);
        assert_eq!(position.realized_pnl, Some(Money::zero(Currency::USD())));
        assert_eq!(position.realized_pnl_remainder, Decimal::ZERO);
        assert_eq!(settled.cost_open, Decimal::ZERO);
        assert_eq!(settled.cost_close, added_cost);
        assert_eq!(settled.signed_qty, Decimal::ZERO);
        assert_eq!(settled.realized_pnl, position.realized_pnl);
        assert_eq!(settled.realized_pnl_remainder, Decimal::ZERO);
    }

    #[rstest]
    fn test_decimal_intermediate_pnl_accepts_repeating_average(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-REPEATING-PNL"))
            .last_qty(Quantity::from(100_000))
            .last_px(Price::from("1.00001"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, opening.clone());
        let mut addition = opening;
        addition.trade_id = TradeId::from("T-ADD");
        addition.event_id = UUID4::new();
        addition.last_qty = Quantity::from(23_457);
        addition.last_px = Price::from("1.00004");
        position.try_apply(&addition).unwrap();

        let pnl = position.calculate_pnl(position.avg_px_open, dec!(1.00005), position.quantity);
        let unchanged = position.calculate_pnl(
            position.avg_px_open,
            position.avg_px_open,
            position.quantity,
        );

        assert_eq!(position.avg_px_open, dec!(1.0000157000413099297731193857));
        assert_eq!(
            pnl.unwrap(),
            Money::from_decimal_exact(dec!(4.23457), Currency::USD()).unwrap()
        );
        assert_eq!(unchanged.unwrap(), Money::zero(Currency::USD()));
    }

    #[rstest]
    #[case(OrderSide::Buy, "0.1234", "0.1235")]
    #[case(OrderSide::Sell, "0.1235", "0.1234")]
    fn test_position_preserves_micro_fees_and_profit(
        audusd_sim: CurrencyPair,
        #[case] side: OrderSide,
        #[case] opening_px: &str,
        #[case] closing_px: &str,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-MICRO"))
            .order_side(side)
            .last_qty(Quantity::from("0.01"))
            .last_px(Price::from(opening_px))
            .commission(Money::from_decimal_exact(dec!(0.000001), Currency::USD()).unwrap())
            .build();
        let mut position = Position::new_checked(&instrument, opening.clone()).unwrap();
        assert_eq!(position.realized_pnl.unwrap().as_decimal(), dec!(-0.000001));

        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position.id)
            .client_order_id(ClientOrderId::from("O-CLOSE-MICRO"))
            .trade_id(TradeId::from("T-CLOSE-MICRO"))
            .order_side(if side == OrderSide::Buy {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            })
            .last_qty(Quantity::from("0.01"))
            .last_px(Price::from(closing_px))
            .commission(Money::zero(Currency::USD()))
            .build();

        let mut zero_fee = opening;
        zero_fee.commission = Some(Money::zero(Currency::USD()));
        let mut without_fee = Position::new_checked(&instrument, zero_fee).unwrap();
        without_fee.try_apply(&closing).unwrap();
        position.try_apply(&closing).unwrap();
        assert_eq!(
            without_fee.realized_pnl.unwrap().as_decimal(),
            dec!(0.000001)
        );
        assert_eq!(position.realized_pnl.unwrap().as_decimal(), Decimal::ZERO);
        assert_eq!(position.signed_qty, Decimal::ZERO);
        assert_eq!(
            position.commissions(),
            vec![Money::from_decimal_exact(dec!(0.000001), Currency::USD()).unwrap()]
        );
    }

    #[rstest]
    fn test_position_commission_overflow_preserves_all_state(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-OVERFLOW"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("1"))
            .commission(Money::from_raw(
                crate::types::money::MONEY_RAW_MAX,
                Currency::USD(),
            ))
            .build();
        let mut position = Position::new_checked(&instrument, opening.clone()).unwrap();
        let before = serde_json::to_value(&position).unwrap();
        let mut next = opening;
        next.event_id = UUID4::new();
        next.trade_id = TradeId::from("T-OVERFLOW");
        next.last_qty = Quantity::from(2);
        next.last_px = Price::from("3");
        next.commission = Some(Money::from_decimal_exact(dec!(0.000001), Currency::USD()).unwrap());
        let result = position.try_apply(&next);
        assert_eq!(
            result.unwrap_err().to_string(),
            "position commission overflow"
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), before);
    }

    #[rstest]
    fn test_position_restoration_preserves_basis_and_fractional_pnl(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let mut fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-RESTORE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(3))
            .last_px(Price::from("0.1"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new_checked(&instrument, fill.clone()).unwrap();
        fill.event_id = UUID4::new();
        fill.trade_id = TradeId::from("T-ADD");
        fill.last_qty = Quantity::from(4);
        fill.last_px = Price::from("0.2");
        position.try_apply(&fill).unwrap();
        fill.event_id = UUID4::new();
        fill.trade_id = TradeId::from("T-REDUCE");
        fill.order_side = OrderSide::Sell;
        fill.last_qty = Quantity::from(1);
        fill.last_px = Price::from("0.3");
        position.try_apply(&fill).unwrap();
        let encoded = serde_json::to_string(&position).unwrap();
        let mut restored: Position = serde_json::from_str(&encoded).unwrap();
        fill.event_id = UUID4::new();
        fill.trade_id = TradeId::from("T-CLOSE");
        fill.last_qty = Quantity::from(6);
        position.try_apply(&fill).unwrap();
        restored.try_apply(&fill).unwrap();
        assert_eq!(position.realized_pnl.unwrap().as_decimal(), dec!(1));
        assert_eq!(position.cost_open, Decimal::ZERO);
        assert_eq!(position.realized_pnl_remainder, Decimal::ZERO);
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(position).unwrap()
        );
    }

    #[rstest]
    fn test_same_trade_id_new_price_restates_position_average(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-RESTATE");
        let trade_id = TradeId::from("T-1");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&restatement);

        assert_eq!(position.quantity.as_decimal(), Decimal::from(2));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.avg_px_open, Price::from("9.47").as_decimal());
        assert_eq!(position.events[0].last_px, Price::from("12"));

        let voided = OrderFillVoidedSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .voided_qty(Quantity::from(1))
            .last_px(Price::from("12"))
            .order_side(OrderSide::Buy)
            .position_id(position_id)
            .build();
        position
            .apply_fill_void(voided, Quantity::from(1), None)
            .unwrap();

        assert_eq!(position.quantity.as_decimal(), Decimal::from(1));
        assert_eq!(position.avg_px_open, Price::from("9.47").as_decimal());
        assert_eq!(position.events[0].last_px, Price::from("12"));
        assert!(position.requires_replay_state());
    }

    #[rstest]
    fn test_price_restatement_of_closing_fill_keeps_open_average(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-CLOSE-RESTATE");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-OPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("15"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("12.75"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&closing);
        position.apply(&restatement);

        assert_eq!(position.quantity.as_decimal(), Decimal::from(1));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.avg_px_open, Price::from("12").as_decimal());
        assert_eq!(position.avg_px_close, Some(Price::from("15").as_decimal()));
        assert_eq!(
            position.realized_pnl.map(|pnl| pnl.as_decimal()),
            Some(dec!(3))
        );
        assert_eq!(position.events[1].last_px, Price::from("15"));
    }

    #[rstest]
    fn test_restatement_of_reversal_fill_keeps_flipped_average(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-REVERSAL-RESTATE");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-OPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let reversal = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-REVERSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(3))
            .last_px(Price::from("10"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-REVERSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(3))
            .last_px(Price::from("9"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&reversal);
        position.apply(&restatement);

        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.quantity.as_decimal(), Decimal::from(1));
        assert_eq!(position.avg_px_open, Price::from("10").as_decimal());
    }

    #[rstest]
    fn test_restatement_share_excludes_reversal_closing_quantity(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-REVERSAL-SHARE");
        let trade_id = TradeId::from("T-RESTATE");

        let mut position = Position::new(
            &instrument,
            OrderFilledSpec::builder()
                .instrument_id(instrument.id())
                .trade_id(TradeId::from("T-OPEN"))
                .order_side(OrderSide::Buy)
                .last_qty(Quantity::from(2))
                .last_px(Price::from("12"))
                .position_id(position_id)
                .build(),
        );

        for (trade, side, qty, px) in [
            ("T-REVERSE", OrderSide::Sell, 3, "10"),
            (trade_id.as_str(), OrderSide::Sell, 1, "8"),
            ("T-CLOSE", OrderSide::Buy, 1, "9"),
        ] {
            position.apply(
                &OrderFilledSpec::builder()
                    .instrument_id(instrument.id())
                    .trade_id(TradeId::from(trade))
                    .order_side(side)
                    .last_qty(Quantity::from(qty))
                    .last_px(Price::from(px))
                    .position_id(position_id)
                    .event_id(UUID4::new())
                    .build(),
            );
        }

        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.quantity.as_decimal(), Decimal::from(1));
        assert_eq!(position.avg_px_open, Price::from("9").as_decimal());

        position.apply(
            &OrderFilledSpec::builder()
                .instrument_id(instrument.id())
                .trade_id(trade_id)
                .order_side(OrderSide::Sell)
                .last_qty(Quantity::from(1))
                .last_px(Price::from("6"))
                .position_id(position_id)
                .event_id(UUID4::new())
                .build(),
        );

        assert_eq!(position.avg_px_open, Price::from("8").as_decimal());
    }

    #[rstest]
    fn test_price_restatement_uses_surviving_quantity(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-SURVIVING");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let later = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("10"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        let voided = OrderFillVoidedSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .voided_qty(Quantity::from(1))
            .last_px(Price::from("12"))
            .order_side(OrderSide::Buy)
            .position_id(position_id)
            .build();
        position
            .apply_fill_void(voided, Quantity::from(1), None)
            .unwrap();
        position.apply(&later);
        position.apply(&restatement);

        let expected = (dec!(11) * dec!(2) - dec!(12) + dec!(9.47)) / dec!(2);
        assert_eq!(position.quantity.as_decimal(), Decimal::from(2));
        assert_eq!(position.avg_px_open, expected);
    }

    #[rstest]
    fn test_reapplying_same_price_restatement_is_idempotent(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-IDEMPOTENT");
        let trade_id = TradeId::from("T-1");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&restatement);
        let replay_len = position.replay_events.len();
        position.apply(&restatement);

        assert_eq!(position.avg_px_open, Price::from("9.47").as_decimal());
        assert_eq!(position.replay_events.len(), replay_len);
    }

    #[rstest]
    fn test_purge_keeps_price_restatement_on_surviving_order(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-PURGE-RESTATE");
        let opening = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-KEEP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-KEEP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let other = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-DROP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-DROP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("10"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let restatement = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-KEEP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-KEEP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&other);
        position.apply(&restatement);
        position
            .purge_events_for_order(ClientOrderId::from("O-DROP"))
            .unwrap();

        assert_eq!(position.quantity.as_decimal(), Decimal::from(2));
        assert_eq!(position.avg_px_open, Price::from("9.47").as_decimal());
    }

    #[rstest]
    fn test_price_restatement_excludes_closed_quantity(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-CLOSED-QTY");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("15"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let later = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("10"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&closing);
        position.apply(&later);
        position.apply(&restatement);

        let expected = (dec!(11) * dec!(2) - dec!(12) + dec!(9.47)) / dec!(2);
        assert_eq!(position.quantity.as_decimal(), Decimal::from(2));
        assert_eq!(position.avg_px_open, expected);
    }

    #[rstest]
    fn test_price_restatement_after_close_across_two_lots_uses_average_cost(
        audusd_sim: CurrencyPair,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-AVG-COST");
        let first = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let second = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("10"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("15"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, first);
        position.apply(&second);
        position.apply(&closing);
        position.apply(&restatement);

        let expected = (dec!(11) * dec!(2) - dec!(12) + dec!(9.47)) / dec!(2);
        assert_eq!(position.quantity.as_decimal(), Decimal::from(2));
        assert_eq!(position.avg_px_open, expected);
    }

    #[rstest]
    fn test_purge_applies_restatement_before_later_close(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-PURGE-ORDER");
        let opening = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-KEEP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-KEEP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let restatement = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-KEEP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-KEEP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let closing = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-CLOSE"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("15"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let other = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-DROP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-DROP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("10"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&restatement);
        position.apply(&closing);
        let pnl_before = position.realized_pnl;
        let avg_before = position.avg_px_open;
        position.apply(&other);
        position
            .purge_events_for_order(ClientOrderId::from("O-DROP"))
            .unwrap();

        assert_eq!(position.realized_pnl, pnl_before);
        assert_eq!(position.avg_px_open, avg_before);
        assert_eq!(position.quantity.as_decimal(), Decimal::from(1));
    }

    #[rstest]
    fn test_full_void_after_restatement_does_not_reopen(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-RESTATE");
        let trade_id = TradeId::from("T-1");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let restatement = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&restatement);
        let voided = OrderFillVoidedSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .voided_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .order_side(OrderSide::Buy)
            .position_id(position_id)
            .build();
        position
            .apply_fill_void(voided, Quantity::from(2), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Flat);
        assert!(position.quantity.is_zero());
    }

    #[rstest]
    fn test_return_to_original_price_then_full_void_stays_flat(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-PRICE-ROUNDTRIP");
        let trade_id = TradeId::from("T-1");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let restated = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("9.47"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let restored = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        position.apply(&restated);
        position.apply(&restored);
        let voided = OrderFillVoidedSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(trade_id)
            .voided_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .order_side(OrderSide::Buy)
            .position_id(position_id)
            .build();
        position
            .apply_fill_void(voided, Quantity::from(2), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Flat);
        assert!(position.quantity.is_zero());
    }

    #[rstest]
    fn test_purge_keeps_voided_quantity_on_surviving_order(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-PURGE-VOID");
        let opening = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-KEEP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-KEEP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("12"))
            .position_id(position_id)
            .build();
        let other = OrderFilledSpec::builder()
            .client_order_id(ClientOrderId::from("O-DROP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-DROP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("10"))
            .position_id(position_id)
            .event_id(UUID4::new())
            .build();
        let mut position = Position::new(&instrument, opening);
        let voided = OrderFillVoidedSpec::builder()
            .client_order_id(ClientOrderId::from("O-KEEP"))
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-KEEP"))
            .voided_qty(Quantity::from(1))
            .last_px(Price::from("12"))
            .order_side(OrderSide::Buy)
            .position_id(position_id)
            .build();
        position
            .apply_fill_void(voided, Quantity::from(1), None)
            .unwrap();
        position.apply(&other);
        position
            .purge_events_for_order(ClientOrderId::from("O-DROP"))
            .unwrap();

        assert_eq!(position.quantity.as_decimal(), Decimal::from(1));
    }

    #[rstest]
    fn test_position_long_display(stub_position_long: Position) {
        let display = format!("{stub_position_long}");
        assert_eq!(display, "Position(LONG 1 AUD/USD.SIM, id=1)");
    }

    #[rstest]
    fn test_position_short_display(stub_position_short: Position) {
        let display = format!("{stub_position_short}");
        assert_eq!(display, "Position(SHORT 1 AUD/USD.SIM, id=1)");
    }

    #[rstest]
    #[case::open(false)]
    #[case::closed(true)]
    fn test_clone_without_events_preserves_current_state(
        mut stub_position_long: Position,
        #[case] close: bool,
    ) {
        let adjustment = PositionAdjusted::new(
            stub_position_long.trader_id,
            stub_position_long.strategy_id,
            stub_position_long.instrument_id,
            stub_position_long.id,
            stub_position_long.account_id,
            PositionAdjustmentType::Funding,
            None,
            Some(Money::from_decimal(dec!(1.25), stub_position_long.settlement_currency).unwrap()),
            Some("clone-test".into()),
            uuid4(),
            UnixNanos::from(2),
            UnixNanos::from(2),
        );
        stub_position_long.apply_adjustment(adjustment).unwrap();

        if close {
            let closing_fill = OrderFilledSpec::builder()
                .trader_id(stub_position_long.trader_id)
                .strategy_id(stub_position_long.strategy_id)
                .instrument_id(stub_position_long.instrument_id)
                .client_order_id(ClientOrderId::from("CLONE-CLOSE"))
                .venue_order_id(VenueOrderId::from("CLONE-CLOSE"))
                .account_id(stub_position_long.account_id)
                .trade_id(TradeId::from("CLONE-CLOSE"))
                .order_side(OrderSide::Sell)
                .order_type(OrderType::Market)
                .last_qty(stub_position_long.quantity)
                .last_px(Price::from("1.0012"))
                .currency(stub_position_long.settlement_currency)
                .position_id(stub_position_long.id)
                .ts_event(UnixNanos::from(3))
                .ts_init(UnixNanos::from(3))
                .build();
            stub_position_long.apply(&closing_fill);
        }

        let source_fill = stub_position_long.events[0].clone();
        let fill_voided = matching_fill_void(&source_fill, source_fill.last_qty, None);
        stub_position_long.fill_voids.push(PositionFillVoid {
            event: fill_voided,
            voided_qty: source_fill.last_qty,
            commission_voided: source_fill.commission,
        });

        let cloned = stub_position_long.clone_without_events();
        let mut expected = stub_position_long.clone();
        expected.events.clear();
        expected.adjustments.clear();
        expected.replay_events.clear();
        expected.fill_voids.clear();
        expected.trade_ids.clear();

        assert!(!stub_position_long.events.is_empty());
        assert!(!stub_position_long.adjustments.is_empty());
        assert!(!stub_position_long.replay_events.is_empty());
        assert!(!stub_position_long.fill_voids.is_empty());
        assert!(!stub_position_long.trade_ids.is_empty());
        assert!(cloned.events.is_empty());
        assert!(cloned.adjustments.is_empty());
        assert!(cloned.replay_events.is_empty());
        assert!(cloned.fill_voids.is_empty());
        assert!(cloned.trade_ids.is_empty());
        assert_eq!(
            serde_json::to_value(cloned).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[rstest]
    fn test_clone_for_snapshot_matches_clone_then_clear(mut stub_position_long: Position) {
        let source_fill = stub_position_long.events[0].clone();
        stub_position_long.fill_voids.push(PositionFillVoid {
            event: matching_fill_void(&source_fill, source_fill.last_qty, None),
            voided_qty: source_fill.last_qty,
            commission_voided: source_fill.commission,
        });
        let mut expected = stub_position_long.clone();
        expected.replay_events.clear();
        expected.fill_voids.clear();

        let snapshot = stub_position_long.clone_for_snapshot();

        assert!(snapshot.replay_events.is_empty());
        assert!(snapshot.fill_voids.is_empty());
        assert_eq!(
            serde_json::to_vec(&snapshot).unwrap(),
            serde_json::to_vec(&expected).unwrap(),
        );
    }

    #[rstest]
    fn test_new_checked_rejects_missing_position_id(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .build();

        let error = Position::new_checked(&instrument, fill).unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::PredicateViolation {
                message: "`fill.position_id` was None".to_string(),
            }
        );
    }

    #[rstest]
    fn test_new_checked_rejects_instrument_mismatch(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let fill = OrderFilledSpec::builder()
            .instrument_id(InstrumentId::from("GBP/USD.SIM"))
            .position_id(PositionId::from("P-1"))
            .build();

        let error = Position::new_checked(&instrument, fill).unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::EqualityMismatch {
                lhs_param: "instrument.id()".to_string(),
                rhs_param: "fill.instrument_id".to_string(),
                lhs: "AUD/USD.SIM".to_string(),
                rhs: "GBP/USD.SIM".to_string(),
                type_name: "value",
            }
        );
    }

    #[rstest]
    #[case::instrument_mismatch(
        "GBP/USD.SIM",
        Some("P-1"),
        "'self.instrument_id' value of AUD/USD.SIM was not equal to 'fill.instrument_id' value of GBP/USD.SIM"
    )]
    #[case::missing_position_id("AUD/USD.SIM", None, "`fill.position_id` was None")]
    #[case::position_mismatch(
        "AUD/USD.SIM",
        Some("P-2"),
        "'self.id' value of P-1 was not equal to 'fill.position_id' value of P-2"
    )]
    fn test_try_apply_rejects_invalid_fill_identity_without_mutation(
        #[case] fill_instrument_id: &str,
        #[case] fill_position_id: Option<&str>,
        #[case] expected_error: &str,
        audusd_sim: CurrencyPair,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-1");
        let fill_open = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .position_id(position_id)
            .build();
        let mut fill_invalid = OrderFilledSpec::builder()
            .instrument_id(InstrumentId::from(fill_instrument_id))
            .trade_id(TradeId::from("T-2"))
            .build();
        fill_invalid.position_id = fill_position_id.map(PositionId::from);
        let mut position = Position::new(&instrument, fill_open);
        let state_before = serde_json::to_value(&position).unwrap();

        let error = position.try_apply(&fill_invalid).unwrap_err();

        assert_eq!(error.to_string(), expected_error);
        assert_eq!(serde_json::to_value(&position).unwrap(), state_before);
    }

    #[rstest]
    fn test_try_apply_rejects_duplicate_trade_without_mutation(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-1");
        let fill_open = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .position_id(position_id)
            .build();
        let fill_duplicate = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-1"))
            .position_id(position_id)
            .build();
        let mut position = Position::new(&instrument, fill_open);
        let state_before = serde_json::to_value(&position).unwrap();

        let error = position.try_apply(&fill_duplicate).unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::PredicateViolation {
                message: "`fill.trade_id` already contained in `trade_ids`".to_string(),
            }
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), state_before);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[case::price_17("1.00000000000000001", "100000", None)]
    #[case::price_18("1.000000000000000001", "100000", None)]
    #[case::quantity_18("1.00000", "1.000000000000000001", None)]
    #[case::settlement_commission_18("1.00000", "100000", Some(("USD", 18)))]
    #[case::base_commission_18("1.00000", "100000", Some(("AUD", 18)))]
    fn test_new_checked_preserves_high_precision_fill_values(
        #[case] last_px: &str,
        #[case] last_qty: &str,
        #[case] commission: Option<(&str, u8)>,
        audusd_sim: CurrencyPair,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let mut fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .last_px(Price::from(last_px))
            .last_qty(Quantity::from(last_qty))
            .position_id(PositionId::from("P-1"))
            .build();
        fill.commission = commission.map(|(code, precision)| {
            let currency = Currency::new(code, precision, 0, code, CurrencyType::Fiat);
            Money::from_decimal(dec!(1), currency).unwrap()
        });

        let position = Position::new_checked(&instrument, fill.clone()).unwrap();

        let expected_qty = if commission.is_some_and(|(code, _)| code == "AUD") {
            dec!(99999)
        } else {
            last_qty.parse().unwrap()
        };

        assert_eq!(position.avg_px_open, last_px.parse::<Decimal>().unwrap());
        assert_eq!(position.signed_qty, expected_qty);
        assert_eq!(position.quantity.as_decimal(), expected_qty);
        assert_eq!(
            position.commissions(),
            fill.commission.into_iter().collect::<Vec<_>>()
        );
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[case::size_precision(18, 0, 2)]
    #[case::multiplier(0, 18, 2)]
    #[case::settlement_currency(0, 0, 18)]
    fn test_new_checked_accepts_high_precision_instrument(
        #[case] size_precision: u8,
        #[case] multiplier_precision: u8,
        #[case] quote_precision: u8,
        mut audusd_sim: CurrencyPair,
    ) {
        audusd_sim.size_precision = size_precision;
        audusd_sim.multiplier = Quantity::from_decimal_dp(dec!(1), multiplier_precision).unwrap();
        audusd_sim.quote_currency =
            Currency::new("USD", quote_precision, 840, "US dollar", CurrencyType::Fiat);
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-1"))
            .build();

        let position = Position::new_checked(&instrument, fill).unwrap();

        assert_eq!(position.size_precision, size_precision);
        assert_eq!(position.multiplier.precision, multiplier_precision);
        assert_eq!(position.settlement_currency.precision, quote_precision);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_new_checked_preserves_sixteen_digit_inputs(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let usd = Currency::new("USD", 16, 840, "US dollar", CurrencyType::Fiat);
        let mut fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .last_px(Price::from("1.5000000000000000"))
            .last_qty(Quantity::from("2.0000000000000000"))
            .position_id(PositionId::from("P-1"))
            .build();
        fill.commission = Some(Money::from_decimal(dec!(0.25), usd).unwrap());

        let position = Position::new_checked(&instrument, fill).unwrap();

        assert_eq!(position.avg_px_open, dec!(1.5));
        assert_eq!(position.signed_qty, dec!(2.0));
        assert_eq!(position.quantity, Quantity::from("2.0000000000000000"));
        assert_eq!(position.realized_pnl, Some(Money::from("-0.25 USD")));
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_new_checked_preserves_eighteen_digit_foreign_commission(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let wei = Currency::new("WEI", 18, 0, "Wei", CurrencyType::Crypto);
        let commission = Money::from_decimal(dec!(0.000000000000000001), wei).unwrap();
        let mut fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-1"))
            .build();
        fill.commission = Some(commission);

        let position = Position::new_checked(&instrument, fill).unwrap();

        assert_eq!(position.commissions(), vec![commission]);
        assert_eq!(position.realized_pnl, Some(Money::from("0 USD")));
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[case::price("price")]
    #[case::foreign_commission("foreign_commission")]
    fn test_try_apply_rejects_unrepresentable_input_without_mutation(
        audusd_sim: CurrencyPair,
        #[case] input: &str,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-1");
        let fill_open = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-1"))
            .position_id(position_id)
            .build();
        let mut fill_invalid = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .trade_id(TradeId::from("T-2"))
            .position_id(position_id)
            .build();

        match input {
            "price" => fill_invalid.last_px = Price::from("100000000000.000000000000000001"),
            "foreign_commission" => {
                fill_invalid.commission = Some(Money::from_raw(
                    100_000_000_000_000_000_000_000_000_001,
                    Currency::EUR(),
                ));
            }
            _ => unreachable!(),
        }

        assert_eq!(
            Position::new_checked(&instrument, fill_invalid.clone())
                .unwrap_err()
                .to_string(),
            "position input exceeds exact Decimal representation",
        );
        let mut position = Position::new(&instrument, fill_open);
        let state_before = serde_json::to_value(&position).unwrap();

        let error = position.try_apply(&fill_invalid).unwrap_err();

        assert_eq!(
            error.to_string(),
            "position input exceeds exact Decimal representation"
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), state_before);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[case::unchanged(|_: &mut Position| {})]
    #[case::buy_qty(
        |position: &mut Position| position.buy_qty = Quantity::from("1.000000000000000001")
    )]
    #[case::sell_qty(
        |position: &mut Position| position.sell_qty = Quantity::from("1.000000000000000001")
    )]
    #[case::event_fill(
        |position: &mut Position| position.events[0].last_px = Price::from("1.000000000000000001")
    )]
    #[case::replay_fill(
        |position: &mut Position| {
            let mut fill = position.events[0].clone();
            fill.last_qty = Quantity::from("1.00000000000000001");
            position.replay_events.push(PositionReplayEvent::Filled(fill));
        }
    )]
    #[case::replay_close(
        |position: &mut Position| {
            let close = InstrumentClose::new(
                position.instrument_id,
                Price::from("1.000000000000000001"),
                InstrumentCloseType::ContractExpired,
                UnixNanos::default(),
                UnixNanos::default(),
            );
            position.replay_events.push(PositionReplayEvent::InstrumentClosed(close));
        }
    )]
    #[case::realized_pnl(
        |position: &mut Position| {
            let usd = Currency::new("USD", 18, 840, "US dollar", CurrencyType::Fiat);
            position.realized_pnl = Some(Money::from_decimal(dec!(1), usd).unwrap());
        }
    )]
    fn test_check_accounting_accepts_high_precision_stored_values(
        #[case] mutate: fn(&mut Position),
        mut stub_position_long: Position,
    ) {
        mutate(&mut stub_position_long);

        let result = stub_position_long.check_accounting();

        assert_eq!(result, Ok(()));
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_try_pnl_preserves_eighteen_digit_inputs(stub_position_long: Position) {
        let last = Price::from("1.000000000000000001");
        let quantity = Quantity::from("1.000000000000000001");

        let unrealized = stub_position_long.try_unrealized_pnl(last).unwrap();
        let total = stub_position_long.try_total_pnl(last).unwrap();
        let calculated = stub_position_long
            .try_calculate_pnl(dec!(1.0), dec!(1.0), quantity)
            .unwrap();

        assert_eq!(unrealized, Money::from_str_exact("-0.0002 USD").unwrap());
        assert_eq!(total, Money::from_str_exact("-2.0002 USD").unwrap());
        assert_eq!(calculated, Money::zero(Currency::USD()));
    }

    #[rstest]
    #[should_panic(expected = "`fill.trade_id` already contained in `trade_ids`")]
    fn test_two_trades_with_same_trade_id_error(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::from("O-1"))
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::from("O-2"))
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &audusd_sim,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            None,
            None,
        );
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("1.00002")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&audusd_sim, fill1.into());
        position.apply(&fill2.into());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_historical_duplicate_trade_id_does_not_poison_fill_void_replay(
        #[case] causal_duplicate: bool,
        audusd_sim: CurrencyPair,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-DUP");
        let fill_open = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill_close = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00010"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let mut fill_duplicate = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00020"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(3))
            .build();

        if causal_duplicate {
            fill_duplicate.causation_id = Some(fill_open.event_id);
        }
        let fill_reopen = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-3"))
            .trade_id(TradeId::from("T-3"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(5))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(4))
            .build();
        let mut fill_duplicate_open = fill_duplicate.clone();
        fill_duplicate_open.event_id = uuid4();
        fill_duplicate_open.client_order_id = ClientOrderId::from("O-4");
        fill_duplicate_open.ts_event = UnixNanos::from(5);
        let fill_voided = matching_fill_void(&fill_close, Quantity::from(10), None);
        let mut position = Position::new(&instrument, fill_open.clone());
        position.try_apply(&fill_close).unwrap();

        position.try_apply(&fill_duplicate).unwrap();

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.quantity, Quantity::from(0));
        assert_eq!(position.events, vec![fill_open.clone(), fill_close.clone()]);
        assert_eq!(position.replay_events.len(), 2);
        assert_eq!(position.trade_ids.len(), 2);
        assert!(position.trade_ids.contains(&TradeId::from("T-1")));
        assert!(position.trade_ids.contains(&TradeId::from("T-2")));

        position.try_apply(&fill_reopen).unwrap();
        position.try_apply(&fill_duplicate_open).unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(5));
        assert_eq!(position.opening_order_id, ClientOrderId::from("O-3"));
        assert_eq!(position.events, vec![fill_reopen.clone()]);
        assert_eq!(position.replay_events.len(), 3);
        assert_eq!(position.trade_ids.len(), 1);
        assert!(position.trade_ids.contains(&TradeId::from("T-3")));

        position
            .apply_fill_void(fill_voided, Quantity::from(10), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(15));
        assert_eq!(position.opening_order_id, ClientOrderId::from("O-1"));
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.avg_px_open, dec!(1.0));
        assert_eq!(position.buy_qty, Quantity::from(15));
        assert_eq!(position.sell_qty, Quantity::from(0));
        assert_eq!(
            position.events,
            vec![fill_open.clone(), fill_reopen.clone()]
        );
        assert_eq!(position.replay_events.len(), 3);
        assert_eq!(position.fill_voids.len(), 1);
        assert_eq!(position.trade_ids.len(), 2);
        assert!(position.trade_ids.contains(&TradeId::from("T-1")));
        assert!(position.trade_ids.contains(&TradeId::from("T-3")));

        let mut fill_close_duplicate = fill_close;
        fill_close_duplicate.event_id = uuid4();
        fill_close_duplicate.ts_event = UnixNanos::from(6);
        position.try_apply(&fill_close_duplicate).unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(15));
        assert_eq!(position.events, vec![fill_open, fill_reopen]);
        assert_eq!(position.replay_events.len(), 3);
    }

    #[rstest]
    fn test_replay_fill_with_foreign_causation_stays_duplicate(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-CAUSATION");
        let fill_open = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill_close = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let mut position = Position::new(&instrument, fill_open.clone());
        position.apply(&fill_close);

        let mut replayed = fill_open.clone();
        replayed.event_id = uuid4();
        replayed.ts_event = UnixNanos::from(3);
        replayed.causation_id = Some(fill_close.event_id);

        position.apply(&replayed);

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.quantity, Quantity::from(0));
        assert_eq!(position.events, vec![fill_open, fill_close.clone()]);
    }

    #[rstest]
    fn test_position_applies_fills_with_negative_prices(audusd_sim: CurrencyPair) {
        // Options and spreads can trade at negative prices; position average
        // price updates must not panic when the stored average or incoming
        // fill price is below zero.
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("-5.00000")),
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        let fill2 = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("-7.00000")),
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&audusd_sim, fill1.into());
        position.apply(&fill2.into());

        assert_eq!(position.quantity, Quantity::from(100_000));
        assert_eq!(position.signed_qty, dec!(100_000.0));
        assert_eq!(position.side, PositionSide::Long);
        // Weighted avg_px_open: (50_000 * -5 + 50_000 * -7) / 100_000 = -6.0
        assert_eq!(position.avg_px_open, dec!(-6.0));
    }

    #[rstest]
    fn test_position_filled_with_buy_order(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            None,
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            None,
            None,
        );
        let last_price = Price::from_str("1.0005").unwrap();
        let position = Position::new(&audusd_sim, fill.into());
        assert_eq!(position.symbol(), audusd_sim.id().symbol);
        assert_eq!(position.venue(), audusd_sim.id().venue);
        assert_eq!(position.closing_order_side(), Some(OrderSide::Sell));
        assert!(!position.is_opposite_side(OrderSide::Buy));
        assert_eq!(position, position); // equality operator test
        assert!(position.closing_order_id.is_none());
        assert_eq!(position.quantity, Quantity::from(100_000));
        assert_eq!(position.peak_qty, Quantity::from(100_000));
        assert_eq!(position.size_precision, 0);
        assert_eq!(position.signed_qty, dec!(100_000.0));
        assert_eq!(position.entry, OrderSide::Buy);
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.ts_opened.as_u64(), 0);
        assert_eq!(position.duration_ns, DurationNanos::default());
        assert_eq!(position.avg_px_open, dec!(1.00001));
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.id, PositionId::new("1"));
        assert_eq!(position.events.len(), 1);
        assert!(position.is_long());
        assert!(!position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(position.realized_pnl, Some(Money::from("-2.0 USD")));
        assert_eq!(
            position.unrealized_pnl(last_price).unwrap(),
            Money::from("49.0 USD")
        );
        assert_eq!(
            position.total_pnl(last_price).unwrap(),
            Money::from("47.0 USD")
        );
        assert_eq!(position.commissions(), vec![Money::from("2.0 USD")]);
        assert_eq!(
            format!("{position}"),
            "Position(LONG 100_000 AUD/USD.SIM, id=1)"
        );
    }

    #[rstest]
    fn test_position_filled_with_sell_order(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            None,
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            None,
            None,
        );
        let last_price = Price::from_str("1.00050").unwrap();
        let position = Position::new(&audusd_sim, fill.into());
        assert_eq!(position.symbol(), audusd_sim.id().symbol);
        assert_eq!(position.venue(), audusd_sim.id().venue);
        assert_eq!(position.closing_order_side(), Some(OrderSide::Buy));
        assert!(!position.is_opposite_side(OrderSide::Sell));
        assert_eq!(position, position); // Equality operator test
        assert!(position.closing_order_id.is_none());
        assert_eq!(position.quantity, Quantity::from(100_000));
        assert_eq!(position.peak_qty, Quantity::from(100_000));
        assert_eq!(position.signed_qty, dec!(-100_000.0));
        assert_eq!(position.entry, OrderSide::Sell);
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.ts_opened.as_u64(), 0);
        assert_eq!(position.avg_px_open, dec!(1.00001));
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.id, PositionId::new("1"));
        assert_eq!(position.events.len(), 1);
        assert!(!position.is_long());
        assert!(position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(position.realized_pnl, Some(Money::from("-2.0 USD")));
        assert_eq!(
            position.unrealized_pnl(last_price).unwrap(),
            Money::from("-49.0 USD")
        );
        assert_eq!(
            position.total_pnl(last_price).unwrap(),
            Money::from("-51.0 USD")
        );
        assert_eq!(position.commissions(), vec![Money::from("2.0 USD")]);
        assert_eq!(
            format!("{position}"),
            "Position(SHORT 100_000 AUD/USD.SIM, id=1)"
        );
    }

    #[rstest]
    fn test_position_partial_fills_with_buy_order(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            None,
            None,
            Some(Price::from("1.00001")),
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        let last_price = Price::from_str("1.00048").unwrap();
        let position = Position::new(&audusd_sim, fill.into());
        assert_eq!(position.quantity, Quantity::from(50_000));
        assert_eq!(position.peak_qty, Quantity::from(50_000));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.signed_qty, dec!(50000.0));
        assert_eq!(position.avg_px_open, dec!(1.00001));
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.ts_opened.as_u64(), 0);
        assert!(position.is_long());
        assert!(!position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(position.realized_pnl, Some(Money::from("-2.0 USD")));
        assert_eq!(
            position.unrealized_pnl(last_price).unwrap(),
            Money::from("23.5 USD")
        );
        assert_eq!(
            position.total_pnl(last_price).unwrap(),
            Money::from("21.5 USD")
        );
        assert_eq!(position.commissions(), vec![Money::from("2.0 USD")]);
        assert_eq!(
            format!("{position}"),
            "Position(LONG 50_000 AUD/USD.SIM, id=1)"
        );
    }

    #[rstest]
    fn test_position_partial_fills_with_two_sell_orders(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("1.00001")),
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        let fill2 = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("1.00002")),
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        let last_price = Price::from_str("1.0005").unwrap();
        let mut position = Position::new(&audusd_sim, fill1.into());
        position.apply(&fill2.into());

        assert_eq!(position.quantity, Quantity::from(100_000));
        assert_eq!(position.peak_qty, Quantity::from(100_000));
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.signed_qty, dec!(-100_000.0));
        assert_eq!(position.avg_px_open, dec!(1.000_015));
        assert_eq!(position.event_count(), 2);
        assert_eq!(position.ts_opened, 0);
        assert!(position.is_short());
        assert!(!position.is_long());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(position.realized_pnl, Some(Money::from("-4.0 USD")));
        assert_eq!(
            position.unrealized_pnl(last_price).unwrap(),
            Money::from("-48.5 USD")
        );
        assert_eq!(
            position.total_pnl(last_price).unwrap(),
            Money::from("-52.5 USD")
        );
        assert_eq!(position.commissions(), vec![Money::from("4.0 USD")]);
    }

    #[rstest]
    pub fn test_position_filled_with_buy_order_then_sell_order(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(150_000))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-1")),
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            Some(UnixNanos::from(1_000_000_000)),
            None,
        );
        let mut position = Position::new(&audusd_sim, fill.into());

        let fill2 = OrderFilledSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(StrategyId::new("S-001"))
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from("2"))
            .account_id(order.account_id().unwrap_or(AccountId::new("SIM-001")))
            .trade_id(TradeId::new("2"))
            .order_side(OrderSide::Sell)
            .last_qty(order.quantity())
            .last_px(Price::from("1.00011"))
            .currency(audusd_sim.quote_currency())
            .ts_event(2_000_000_000.into())
            .position_id(PositionId::new("T1"))
            .commission(Money::from("0.0 USD"))
            .build();
        position.apply(&fill2);
        let last = Price::from_str("1.0005").unwrap();

        assert!(position.is_opposite_side(fill2.order_side));
        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision())
        );
        assert_eq!(position.size_precision, 0);
        assert_eq!(position.signed_qty, dec!(0.0));
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 1_000_000_000);
        assert_eq!(position.ts_closed, Some(UnixNanos::from(2_000_000_000)));
        assert_eq!(position.duration_ns, DurationNanos::from_secs(1));
        assert_eq!(position.avg_px_open, dec!(1.00001));
        assert_eq!(position.avg_px_close, Some(dec!(1.00011)));
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert!(!position.is_open());
        assert!(position.is_closed());
        assert_eq!(position.realized_return, (1.00011_f64 - 1.00001) / 1.00001);
        assert_eq!(position.realized_pnl, Some(Money::from("13.0 USD")));
        assert_eq!(position.unrealized_pnl(last).unwrap(), Money::from("0 USD"));
        assert_eq!(position.commissions(), vec![Money::from("2 USD")]);
        assert_eq!(position.total_pnl(last).unwrap(), Money::from("13 USD"));
        assert_eq!(format!("{position}"), "Position(FLAT AUD/USD.SIM, id=P-1)");
    }

    #[rstest]
    pub fn test_position_filled_with_sell_order_then_buy_order(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &audusd_sim,
            None,
            Some(PositionId::new("P-19700101-000000-001-001-1")),
            Some(Price::from("1.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&audusd_sim, fill1.into());
        // create closing from order from different venue but same strategy
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-19700101-000000-001-001-1")),
            Some(Price::from("1.00001")),
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        let fill3 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            Some(TradeId::new("2")),
            Some(PositionId::new("P-19700101-000000-001-001-1")),
            Some(Price::from("1.00003")),
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        let last = Price::from("1.0005");
        position.apply(&fill2.into());
        position.apply(&fill3.into());

        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision())
        );
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.avg_px_open, dec!(1.0));
        assert_eq!(position.events.len(), 3);
        assert_eq!(position.ts_closed, Some(UnixNanos::default()));
        assert_eq!(position.avg_px_close, Some(dec!(1.00002)));
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert!(!position.is_open());
        assert!(position.is_closed());
        assert_eq!(position.commissions(), vec![Money::from("6.0 USD")]);
        assert_eq!(position.unrealized_pnl(last).unwrap(), Money::from("0 USD"));
        assert_eq!(position.realized_pnl, Some(Money::from("-8.0 USD")));
        assert_eq!(position.total_pnl(last).unwrap(), Money::from("-8.0 USD"));
        assert_eq!(
            format!("{position}"),
            "Position(FLAT AUD/USD.SIM, id=P-19700101-000000-001-001-1)"
        );
    }

    #[rstest]
    fn test_position_filled_with_no_change(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &audusd_sim,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-19700101-000000-001-001-1")),
            Some(Price::from("1.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&audusd_sim, fill1.into());
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            Some(TradeId::new("2")),
            Some(PositionId::new("P-19700101-000000-001-001-1")),
            Some(Price::from("1.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let last = Price::from("1.0005");
        position.apply(&fill2.into());

        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision())
        );
        assert_eq!(position.closing_order_side(), None);
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.avg_px_open, dec!(1.0));
        assert_eq!(position.events.len(), 2);
        // assert_eq!(position.trade_ids, vec![fill1.trade_id, fill2.trade_id]);  // TODO
        assert_eq!(position.ts_closed, Some(UnixNanos::default()));
        assert_eq!(position.avg_px_close, Some(dec!(1.0)));
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert!(!position.is_open());
        assert!(position.is_closed());
        assert_eq!(position.commissions(), vec![Money::from("4.0 USD")]);
        assert_eq!(position.unrealized_pnl(last).unwrap(), Money::from("0 USD"));
        assert_eq!(position.realized_pnl, Some(Money::from("-4.0 USD")));
        assert_eq!(position.total_pnl(last).unwrap(), Money::from("-4.0 USD"));
        assert_eq!(
            format!("{position}"),
            "Position(FLAT AUD/USD.SIM, id=P-19700101-000000-001-001-1)"
        );
    }

    #[rstest]
    fn test_position_long_with_multiple_filled_orders(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(200_000))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &audusd_sim,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("1.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            Some(TradeId::new("2")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            None,
            None,
        );
        let fill3 = TestOrderEventStubs::filled(
            &order3,
            &audusd_sim,
            Some(TradeId::new("3")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("1.0001")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&audusd_sim, fill1.into());
        let last = Price::from("1.0005");
        position.apply(&fill2.into());
        position.apply(&fill3.into());

        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.price_precision())
        );
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, 0);
        assert_eq!(position.avg_px_open, dec!(1.000_005));
        assert_eq!(position.events.len(), 3);
        // assert_eq!(
        //     position.trade_ids,
        //     vec![fill1.trade_id, fill2.trade_id, fill3.trade_id]
        // );
        assert_eq!(position.ts_closed, Some(UnixNanos::default()));
        assert_eq!(position.avg_px_close, Some(dec!(1.0001)));
        assert!(position.is_closed());
        assert!(!position.is_open());
        assert!(!position.is_long());
        assert!(!position.is_short());
        assert_eq!(position.commissions(), vec![Money::from("6.0 USD")]);
        assert_eq!(position.realized_pnl, Some(Money::from("13.0 USD")));
        assert_eq!(position.unrealized_pnl(last).unwrap(), Money::from("0 USD"));
        assert_eq!(position.total_pnl(last).unwrap(), Money::from("13 USD"));
        assert_eq!(
            format!("{position}"),
            "Position(FLAT AUD/USD.SIM, id=P-123456)"
        );
    }

    #[rstest]
    fn test_pnl_calculation_from_trading_technologies_example(currency_pair_ethusdt: CurrencyPair) {
        let ethusdt = InstrumentAny::CurrencyPair(currency_pair_ethusdt);
        let quantity1 = Quantity::from(12);
        let price1 = Price::from("100.0");
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusdt.id())
            .side(OrderSide::Buy)
            .quantity(quantity1)
            .build();
        let commission1 =
            calculate_commission(&ethusdt, order1.quantity(), price1, None, dec!(0.0001));
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &ethusdt,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-123456")),
            Some(price1),
            None,
            None,
            Some(commission1),
            None,
            None,
        );
        let mut position = Position::new(&ethusdt, fill1.into());
        let quantity2 = Quantity::from(17);
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusdt.id())
            .side(OrderSide::Buy)
            .quantity(quantity2)
            .build();
        let price2 = Price::from("99.0");
        let commission2 =
            calculate_commission(&ethusdt, order2.quantity(), price2, None, dec!(0.0001));
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &ethusdt,
            Some(TradeId::new("2")),
            Some(PositionId::new("P-123456")),
            Some(price2),
            None,
            None,
            Some(commission2),
            None,
            None,
        );
        position.apply(&fill2.into());
        assert_eq!(position.quantity, Quantity::from(29));
        assert_eq!(position.realized_pnl, Some(Money::from("-0.28830000 USDT")));
        assert_eq!(position.avg_px_open, (dec!(2883) / dec!(29)));
        let quantity3 = Quantity::from(9);
        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusdt.id())
            .side(OrderSide::Sell)
            .quantity(quantity3)
            .build();
        let price3 = Price::from("101.0");
        let commission3 =
            calculate_commission(&ethusdt, order3.quantity(), price3, None, dec!(0.0001));
        let fill3 = TestOrderEventStubs::filled(
            &order3,
            &ethusdt,
            Some(TradeId::new("3")),
            Some(PositionId::new("P-123456")),
            Some(price3),
            None,
            None,
            Some(commission3),
            None,
            None,
        );
        position.apply(&fill3.into());
        assert_eq!(position.quantity, Quantity::from(20));
        assert_eq!(
            position.realized_pnl,
            Some(native_money(dec!(251877) / dec!(18125), Currency::USDT()))
        );
        assert_eq!(position.avg_px_open, (dec!(2883) / dec!(29)));
        let quantity4 = Quantity::from("4");
        let price4 = Price::from("105.0");
        let order4 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusdt.id())
            .side(OrderSide::Sell)
            .quantity(quantity4)
            .build();
        let commission4 =
            calculate_commission(&ethusdt, order4.quantity(), price4, None, dec!(0.0001));
        let fill4 = TestOrderEventStubs::filled(
            &order4,
            &ethusdt,
            Some(TradeId::new("4")),
            Some(PositionId::new("P-123456")),
            Some(price4),
            None,
            None,
            Some(commission4),
            None,
            None,
        );
        position.apply(&fill4.into());
        assert_eq!(position.quantity, Quantity::from("16"));
        assert_eq!(
            position.realized_pnl,
            Some(native_money(dec!(2624463) / dec!(72500), Currency::USDT()))
        );
        assert_eq!(position.avg_px_open, (dec!(2883) / dec!(29)));
        let quantity5 = Quantity::from("3");
        let price5 = Price::from("103.0");
        let order5 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusdt.id())
            .side(OrderSide::Buy)
            .quantity(quantity5)
            .build();
        let commission5 =
            calculate_commission(&ethusdt, order5.quantity(), price5, None, dec!(0.0001));
        let fill5 = TestOrderEventStubs::filled(
            &order5,
            &ethusdt,
            Some(TradeId::new("5")),
            Some(PositionId::new("P-123456")),
            Some(price5),
            None,
            None,
            Some(commission5),
            None,
            None,
        );
        position.apply(&fill5.into());
        assert_eq!(position.quantity, Quantity::from("19"));
        assert_eq!(
            position.realized_pnl,
            Some(native_money(
                dec!(10488891) / dec!(290000),
                Currency::USDT()
            ))
        );
        assert_eq!(position.avg_px_open, dec!(99.98003629764065335753176043));
        assert_eq!(
            format!("{position}"),
            "Position(LONG 19.00000 ETHUSDT.BINANCE, id=P-123456)"
        );
    }

    #[rstest]
    fn test_position_closed_and_reopened(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let quantity1 = Quantity::from(150_000);
        let price1 = Price::from("1.00001");
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(quantity1)
            .build();
        let commission1 = calculate_commission(&audusd_sim, quantity1, price1, None, dec!(0.00002));
        let fill1 = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("5")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("1.00001")),
            None,
            None,
            Some(commission1),
            Some(UnixNanos::from(1_000_000_000)),
            None,
        );
        let mut position = Position::new(&audusd_sim, fill1.into());

        let fill2 = OrderFilledSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from("2"))
            .account_id(order.account_id().unwrap_or(AccountId::new("SIM-001")))
            .trade_id(TradeId::from("2"))
            .order_side(OrderSide::Sell)
            .last_qty(order.quantity())
            .last_px(Price::from("1.00011"))
            .currency(audusd_sim.quote_currency())
            .ts_event(UnixNanos::from(2_000_000_000))
            .position_id(PositionId::from("P-123456"))
            .commission(Money::from("0 USD"))
            .build();

        position.apply(&fill2);

        let fill3 = OrderFilledSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from("2"))
            .account_id(order.account_id().unwrap_or(AccountId::new("SIM-001")))
            .trade_id(TradeId::from("3"))
            .last_qty(order.quantity())
            .last_px(Price::from("1.00012"))
            .currency(audusd_sim.quote_currency())
            .ts_event(UnixNanos::from(3_000_000_000))
            .position_id(PositionId::from("P-123456"))
            .commission(Money::from("0 USD"))
            .build();

        position.apply(&fill3);

        let last = Price::from("1.0003");
        assert!(position.is_opposite_side(fill2.order_side));
        assert_eq!(position.quantity, Quantity::from(150_000));
        assert_eq!(position.peak_qty, Quantity::from(150_000));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.opening_order_id, fill3.client_order_id);
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.ts_opened, 3_000_000_000);
        assert_eq!(position.duration_ns, DurationNanos::default());
        assert_eq!(position.avg_px_open, dec!(1.00012));
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.ts_closed, None);
        assert_eq!(position.avg_px_close, None);
        assert!(position.is_long());
        assert!(!position.is_short());
        assert!(position.is_open());
        assert!(!position.is_closed());
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(position.realized_pnl, Some(Money::from("0 USD")));
        assert_eq!(
            position.unrealized_pnl(last).unwrap(),
            Money::from("27 USD")
        );
        assert_eq!(position.total_pnl(last).unwrap(), Money::from("27 USD"));
        assert_eq!(position.commissions(), vec![Money::from("0 USD")]);
        assert_eq!(
            format!("{position}"),
            "Position(LONG 150_000 AUD/USD.SIM, id=P-123456)"
        );
    }

    #[rstest]
    #[case::zero(Quantity::from(0))]
    #[case::exceeds_fragments(Quantity::from(11))]
    fn test_apply_fill_void_rejects_invalid_allocation_without_mutation(
        #[case] voided_qty: Quantity,
        audusd_sim: CurrencyPair,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-INVALID");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-VOID-INVALID"))
            .trade_id(TradeId::from("T-VOID-INVALID"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let fill_voided = matching_fill_void(&fill, voided_qty, None);
        let mut position = Position::new(&instrument, fill);
        let state_before = serde_json::to_value(&position).unwrap();

        let error = position
            .apply_fill_void(fill_voided, voided_qty, None)
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "position fill void exceeds known fragments for T-VOID-INVALID"
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), state_before);
    }

    #[rstest]
    #[case::stale(
        Quantity::from(4),
        Money::from("0.40 USD"),
        "stale position fill void for T-VOID-CUMULATIVE"
    )]
    #[case::duplicate(
        Quantity::from(5),
        Money::from("0.50 USD"),
        "duplicate position fill void for T-VOID-CUMULATIVE"
    )]
    fn test_apply_fill_void_rejects_invalid_cumulative_update_without_mutation(
        #[case] voided_qty: Quantity,
        #[case] commission_voided: Money,
        #[case] expected_error: &str,
        audusd_sim: CurrencyPair,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-CUMULATIVE");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-VOID-CUMULATIVE"))
            .trade_id(TradeId::from("T-VOID-CUMULATIVE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .commission(Money::from("1.00 USD"))
            .build();
        let fill_voided =
            matching_fill_void(&fill, Quantity::from(5), Some(Money::from("0.50 USD")));
        let mut position = Position::new(&instrument, fill);
        position
            .apply_fill_void(
                fill_voided.clone(),
                Quantity::from(5),
                Some(Money::from("0.50 USD")),
            )
            .unwrap();
        let state_before = serde_json::to_value(&position).unwrap();

        let error = position
            .apply_fill_void(fill_voided, voided_qty, Some(commission_voided))
            .unwrap_err();

        assert_eq!(error.to_string(), expected_error);
        assert_eq!(serde_json::to_value(&position).unwrap(), state_before);
    }

    #[rstest]
    fn test_apply_fill_void_rejects_currency_mismatch_without_mutation(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-VOID-CURRENCY"))
            .trade_id(TradeId::from("T-VOID-CURRENCY"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(PositionId::from("P-VOID-CURRENCY"))
            .commission(Money::from("1.00 USD"))
            .build();
        let commission_voided = Some(Money::from("0.50 EUR"));
        let voided_qty = Quantity::from(5);
        let fill_voided = matching_fill_void(&fill, voided_qty, commission_voided);

        let mut position = Position::new(&instrument, fill);
        let before = serde_json::to_value(&position).unwrap();
        let error = position
            .apply_fill_void(fill_voided, voided_qty, commission_voided)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "position commission currency differs for fill T-VOID-CURRENCY"
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), before);
    }

    #[rstest]
    fn test_apply_fill_void_uses_latest_cumulative_commission(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-LATEST");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-VOID-LATEST"))
            .trade_id(TradeId::from("T-VOID-LATEST"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .commission(Money::from("1.00 USD"))
            .build();
        let fill_voided =
            matching_fill_void(&fill, Quantity::from(4), Some(Money::from("0.40 USD")));
        let mut position = Position::new(&instrument, fill);
        position
            .apply_fill_void(
                fill_voided.clone(),
                Quantity::from(4),
                Some(Money::from("0.40 USD")),
            )
            .unwrap();

        position
            .apply_fill_void(fill_voided, Quantity::from(7), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(3));
        assert_eq!(position.commissions(), vec![Money::from("1.00 USD")]);
        assert_eq!(position.realized_pnl, Some(Money::from("-1.00 USD")));
        assert_eq!(position.fill_voids.len(), 2);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[case::native_instrument(0, "1.000000000000000001", "0.5", "0.500000000000000001")]
    #[case::wei_instrument(18, "1", "0.500000000000000001", "0.499999999999999999")]
    fn test_fill_void_accepts_mixed_qty_scales(
        mut audusd_sim: CurrencyPair,
        #[case] precision: u8,
        #[case] qty: &str,
        #[case] voided: &str,
        #[case] remaining: &str,
    ) {
        audusd_sim.size_precision = precision;
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .last_qty(Quantity::from(qty))
            .last_px(Price::from("1"))
            .position_id(PositionId::from("P-MIXED-VOID"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, fill.clone());
        let voided_qty = Quantity::from(voided);
        let first = matching_fill_void(&fill, voided_qty, None);
        position.apply_fill_void(first, voided_qty, None).unwrap();

        assert_eq!(position.quantity, Quantity::from(remaining));
        assert_eq!(position.signed_qty, remaining.parse::<Decimal>().unwrap());
        assert_eq!(position.cost_open, remaining.parse::<Decimal>().unwrap());
        assert_eq!(position.realized_pnl, Some(Money::zero(Currency::USD())));

        let final_void = matching_fill_void(&fill, fill.last_qty, None);
        position
            .apply_fill_void(final_void, fill.last_qty, None)
            .unwrap();

        assert_eq!(position.quantity.as_decimal(), Decimal::ZERO);
        assert_eq!(position.signed_qty, Decimal::ZERO);
        assert_eq!(position.cost_open, Decimal::ZERO);
        assert_eq!(position.realized_pnl_remainder, Decimal::ZERO);
        assert_eq!(position.fill_voids.len(), 2);
    }

    #[rstest]
    fn test_fill_void_replays_across_position_close_and_reopen(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-REPLAY");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-OPEN"))
            .trade_id(TradeId::from("T-OPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .commission(Money::from("1.00 USD"))
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CLOSE"))
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.10000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .commission(Money::from("1.00 USD"))
            .ts_event(UnixNanos::from(2))
            .build();
        let fill3 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-REOPEN"))
            .trade_id(TradeId::from("T-REOPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(5))
            .last_px(Price::from("1.20000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .commission(Money::from("1.00 USD"))
            .ts_event(UnixNanos::from(3))
            .build();
        let fill_voided =
            matching_fill_void(&fill2, Quantity::from(5), Some(Money::from("0.50 USD")));
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);
        position.apply(&fill3);

        position
            .apply_fill_void(
                fill_voided,
                Quantity::from(5),
                Some(Money::from("0.50 USD")),
            )
            .unwrap();
        let encoded = serde_json::to_string(&position).unwrap();
        let restored: Position = serde_json::from_str(&encoded).unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(10));
        assert_eq!(position.opening_order_id, ClientOrderId::from("O-OPEN"));
        assert_eq!(position.buy_qty, Quantity::from(15));
        assert_eq!(position.sell_qty, Quantity::from(5));
        assert_eq!(position.commissions(), vec![Money::from("2.50 USD")]);
        assert_eq!(position.replay_events.len(), 3);
        assert_eq!(position.fill_voids.len(), 1);
        assert_eq!(restored.quantity, position.quantity);
        assert_eq!(restored.opening_order_id, position.opening_order_id);
        assert_eq!(restored.commissions(), position.commissions());
        assert_eq!(restored.replay_events.len(), position.replay_events.len());
        assert_eq!(restored.fill_voids.len(), position.fill_voids.len());
    }

    #[rstest]
    fn test_fill_void_replays_manual_adjustment(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-ADJUSTMENT");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-VOID-ADJUSTMENT"))
            .trade_id(TradeId::from("T-VOID-ADJUSTMENT"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill_voided = matching_fill_void(&fill, Quantity::from(2), None);
        let adjustment = PositionAdjusted::new(
            fill.trader_id,
            fill.strategy_id,
            fill.instrument_id,
            position_id,
            fill.account_id,
            PositionAdjustmentType::Funding,
            None,
            Some(Money::from("5.00 USD")),
            Some("funding".into()),
            uuid4(),
            UnixNanos::from(2),
            UnixNanos::from(2),
        );
        let mut position = Position::new(&instrument, fill);
        position.apply_adjustment(adjustment).unwrap();

        position
            .apply_fill_void(fill_voided, Quantity::from(2), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(8));
        assert_eq!(position.realized_pnl, Some(Money::from("5.00 USD")));
        assert_eq!(position.adjustments, vec![adjustment]);
        assert_eq!(position.replay_events.len(), 2);
        assert_eq!(position.fill_voids.len(), 1);
        assert_eq!(position.ts_last, UnixNanos::from(2));
    }

    fn binary_fill(
        instrument: &InstrumentAny,
        trade_id: &str,
        last_qty: &str,
        last_px: &str,
        ts_event: u64,
    ) -> OrderFilled {
        OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(format!("O-{trade_id}").as_str()))
            .venue_order_id(VenueOrderId::from(format!("V-{trade_id}").as_str()))
            .account_id(AccountId::from("POLYMARKET-001"))
            .trade_id(TradeId::from(trade_id))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(last_qty))
            .last_px(Price::from(last_px))
            .currency(Currency::USDC())
            .commission(Money::from("0.10 USDC"))
            .position_id(PositionId::from("P-BINARY"))
            .ts_event(UnixNanos::from(ts_event))
            .ts_init(UnixNanos::from(ts_event))
            .build()
    }

    fn contract_expired(instrument: &InstrumentAny, close_price: &str) -> InstrumentClose {
        InstrumentClose::new(
            instrument.id(),
            Price::from(close_price),
            InstrumentCloseType::ContractExpired,
            UnixNanos::from(300),
            UnixNanos::from(301),
        )
    }

    #[rstest]
    #[case::winner("1.000", dec!(5.90), 1.5)]
    #[case::loser("0.000", dec!(-4.10), -1.0)]
    fn test_apply_instrument_close_settles_without_fill(
        binary_option: BinaryOption,
        #[case] close_price: &str,
        #[case] expected_realized_pnl: Decimal,
        #[case] expected_return: f64,
    ) {
        let instrument = InstrumentAny::BinaryOption(binary_option);
        let fill = binary_fill(&instrument, "T-OPEN", "10.00", "0.400", 100);
        let mut position = Position::new(&instrument, fill.clone());
        let close = contract_expired(&instrument, close_price);
        let requires_replay_state_before = position.requires_replay_state();

        position.apply_instrument_close(close).unwrap();
        let snapshot = PositionSnapshot::from_replay_state(&position, None);
        let restored: Position = serde_json::from_value(snapshot.replay_state.unwrap()).unwrap();

        assert!(!requires_replay_state_before);
        assert!(position.requires_replay_state());
        assert!(position.is_closed());
        assert!(position.is_settled());
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.signed_qty, dec!(0.0));
        assert_eq!(position.quantity, Quantity::from("0.00"));
        assert_eq!(position.peak_qty, Quantity::from("10.00"));
        assert_eq!(position.buy_qty, Quantity::from("10.00"));
        assert_eq!(position.sell_qty, Quantity::from("0.00"));
        assert_eq!(position.opening_order_id, fill.client_order_id);
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.events, vec![fill]);
        assert_eq!(position.commissions(), vec![Money::from("0.10 USDC")]);
        assert_eq!(position.avg_px_open, dec!(0.4));
        assert_eq!(
            position.avg_px_close,
            Some(Price::from(close_price).as_decimal())
        );
        assert!((position.realized_return - expected_return).abs() < 1e-12);
        assert_eq!(
            position.realized_pnl.unwrap().as_decimal(),
            expected_realized_pnl
        );
        assert_eq!(position.ts_last, UnixNanos::from(300));
        assert_eq!(position.ts_closed, Some(UnixNanos::from(300)));
        assert_eq!(position.duration_ns, DurationNanos::new(200));
        assert!(matches!(
            position.replay_events.last(),
            Some(PositionReplayEvent::InstrumentClosed(event)) if *event == close
        ));
        assert!(restored.is_settled());
        assert!(restored.is_closed());
        assert_eq!(restored.realized_pnl, position.realized_pnl);
        assert_eq!(restored.avg_px_close, position.avg_px_close);
        assert_eq!(restored.ts_closed, position.ts_closed);
    }

    #[rstest]
    fn test_apply_instrument_close_after_partial_close(binary_option: BinaryOption) {
        let instrument = InstrumentAny::BinaryOption(binary_option);
        let opening_fill = binary_fill(&instrument, "T-OPEN", "10.00", "0.400", 100);
        let mut reducing_fill = binary_fill(&instrument, "T-REDUCE", "4.00", "0.600", 200);
        reducing_fill.order_side = OrderSide::Sell;
        let mut position = Position::new(&instrument, opening_fill);
        position.apply(&reducing_fill);

        position
            .apply_instrument_close(contract_expired(&instrument, "1.000"))
            .unwrap();

        assert!(position.is_closed());
        assert_eq!(position.quantity, Quantity::from("0.00"));
        assert_eq!(position.buy_qty, Quantity::from("10.00"));
        assert_eq!(position.sell_qty, Quantity::from("4.00"));
        assert!((position.avg_px_close.unwrap() - dec!(0.84)).abs() < dec!(1e-12));
        assert!((position.realized_return - 1.1).abs() < 1e-12);
        assert_eq!(position.realized_pnl.unwrap().as_decimal(), dec!(4.20));
        assert_eq!(position.closing_order_id, None);
    }

    #[rstest]
    #[case::fills_only(false, false, false)]
    #[case::fill_voided(true, false, true)]
    #[case::settled(false, true, true)]
    fn test_requires_replay_state(
        binary_option: BinaryOption,
        #[case] void_fill: bool,
        #[case] settle: bool,
        #[case] expected: bool,
    ) {
        let instrument = InstrumentAny::BinaryOption(binary_option);
        let fill = binary_fill(&instrument, "T-OPEN", "10.00", "0.400", 100);
        let mut position = Position::new(&instrument, fill.clone());

        if void_fill {
            position
                .apply_fill_void(
                    matching_fill_void(&fill, Quantity::from("2.00"), None),
                    Quantity::from("2.00"),
                    None,
                )
                .unwrap();
        }

        if settle {
            position
                .apply_instrument_close(contract_expired(&instrument, "1.000"))
                .unwrap();
        }

        assert_eq!(position.requires_replay_state(), expected);
    }

    #[rstest]
    #[case::other_instrument(Some("OTHER.POLYMARKET"), InstrumentCloseType::ContractExpired, false)]
    #[case::end_of_session(None, InstrumentCloseType::EndOfSession, false)]
    #[case::not_open(None, InstrumentCloseType::ContractExpired, true)]
    fn test_apply_instrument_close_rejects_without_mutation(
        binary_option: BinaryOption,
        #[case] instrument_id: Option<&str>,
        #[case] close_type: InstrumentCloseType,
        #[case] settle_first: bool,
    ) {
        let instrument = InstrumentAny::BinaryOption(binary_option);
        let fill = binary_fill(&instrument, "T-OPEN", "10.00", "0.400", 100);
        let mut position = Position::new(&instrument, fill);

        if settle_first {
            position
                .apply_instrument_close(contract_expired(&instrument, "1.000"))
                .unwrap();
        }

        let before = serde_json::to_value(&position).unwrap();

        let close = InstrumentClose::new(
            instrument_id.map_or_else(|| instrument.id(), InstrumentId::from),
            Price::from("1.000"),
            close_type,
            UnixNanos::from(400),
            UnixNanos::from(401),
        );

        let result = position.apply_instrument_close(close);

        assert!(result.is_err());
        assert_eq!(serde_json::to_value(&position).unwrap(), before);
    }

    #[rstest]
    fn test_fill_void_replays_instrument_close(binary_option: BinaryOption) {
        let instrument = InstrumentAny::BinaryOption(binary_option);
        let opening_fill = binary_fill(&instrument, "T-OPEN", "10.00", "0.400", 100);
        let adding_fill = binary_fill(&instrument, "T-ADD", "5.00", "0.500", 200);
        let fill_voided = matching_fill_void(&adding_fill, Quantity::from("5.00"), None);
        let mut position = Position::new(&instrument, opening_fill);
        position.apply(&adding_fill);
        let close = contract_expired(&instrument, "1.000");
        position.apply_instrument_close(close).unwrap();

        let closed_cycles_pnl = position
            .apply_fill_void(fill_voided, Quantity::from("5.00"), None)
            .unwrap();

        assert_eq!(closed_cycles_pnl, None);
        assert!(position.is_closed());
        assert!(position.is_settled());
        assert_eq!(position.quantity, Quantity::from("0.00"));
        assert_eq!(position.peak_qty, Quantity::from("10.00"));
        assert_eq!(position.buy_qty, Quantity::from("10.00"));
        assert_eq!(position.realized_pnl.unwrap().as_decimal(), dec!(5.80));
        assert_eq!(position.ts_closed, Some(UnixNanos::from(300)));
        assert_eq!(position.fill_voids.len(), 1);
        assert_eq!(position.replay_events.len(), 3);
    }

    #[rstest]
    fn test_fill_void_returns_all_closed_cycle_pnl(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-CYCLES");
        let fill = |client_order_id: &str,
                    trade_id: &str,
                    order_side: OrderSide,
                    quantity: u32,
                    price: &str,
                    ts_event: u64| {
            OrderFilledSpec::builder()
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::from(client_order_id))
                .trade_id(TradeId::from(trade_id))
                .order_side(order_side)
                .last_qty(Quantity::from(quantity))
                .last_px(Price::from(price))
                .currency(Currency::USD())
                .position_id(position_id)
                .ts_event(UnixNanos::from(ts_event))
                .build()
        };
        let fills = [
            fill("O-OPEN-1", "T-OPEN-1", OrderSide::Buy, 10, "1.0", 1),
            fill("O-CLOSE-1", "T-CLOSE-1", OrderSide::Sell, 10, "2.0", 2),
            fill("O-OPEN-2", "T-OPEN-2", OrderSide::Buy, 10, "3.0", 3),
            fill("O-CLOSE-2", "T-CLOSE-2", OrderSide::Sell, 10, "5.0", 4),
            fill("O-CURRENT", "T-CURRENT", OrderSide::Buy, 5, "6.0", 5),
        ];
        let current = fills.last().unwrap();
        let fill_voided = matching_fill_void(current, Quantity::from(1), None);
        let mut position = Position::new(&instrument, fills[0].clone());
        for fill in &fills[1..] {
            position.apply(fill);
        }

        let closed_cycles_pnl = position
            .apply_fill_void(fill_voided, Quantity::from(1), None)
            .unwrap();

        assert_eq!(closed_cycles_pnl, Some(Money::from("30.00 USD")));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(4));
        assert_eq!(position.avg_px_open, dec!(6.0));
        assert_eq!(position.realized_pnl, Some(Money::from("0.00 USD")));
        assert_eq!(position.events.len(), 1);
        assert_eq!(position.replay_events.len(), 5);
        assert_eq!(position.fill_voids.len(), 1);
    }

    #[rstest]
    fn test_full_fill_void_preserves_unvoided_commission(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-FEE-VOID");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-FEE"))
            .trade_id(TradeId::from("T-FEE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .commission(Money::from("1.00 USD"))
            .build();
        let fill_voided = OrderFillVoidedSpec::builder()
            .instrument_id(fill.instrument_id)
            .client_order_id(fill.client_order_id)
            .venue_order_id(fill.venue_order_id)
            .account_id(fill.account_id)
            .trade_id(fill.trade_id)
            .voided_qty(fill.last_qty)
            .order_side(fill.order_side)
            .order_type(fill.order_type)
            .last_px(fill.last_px)
            .currency(fill.currency)
            .liquidity_side(fill.liquidity_side)
            .build();
        let mut position = Position::new(&instrument, fill);

        position
            .apply_fill_void(fill_voided, Quantity::from(10), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.quantity, Quantity::from(0));
        assert_eq!(position.commissions(), vec![Money::from("1.00 USD")]);
        assert_eq!(position.realized_pnl, Some(Money::from("-1.00 USD")));
        assert!(position.events.is_empty());
    }

    #[rstest]
    fn test_full_fill_void_preserves_unvoided_base_commission() {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let position_id = PositionId::from("P-BASE-FEE-VOID");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-BASE-FEE"))
            .trade_id(TradeId::from("T-BASE-FEE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from("1.000000"))
            .last_px(Price::from("50000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .commission(Money::from("0.00100000 BTC"))
            .ts_event(UnixNanos::from(2_000))
            .ts_init(UnixNanos::from(1_900))
            .build();
        let fill_voided = matching_fill_void(&fill, fill.last_qty, None);
        let mut position = Position::new(&instrument, fill);

        let closed_cycles_pnl = position
            .apply_fill_void(fill_voided, Quantity::from("1.000000"), None)
            .unwrap();

        assert_eq!(closed_cycles_pnl, None);
        assert_eq!(position.entry, OrderSide::Sell);
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.signed_decimal_qty(), dec!(-0.001));
        assert_eq!(position.quantity.as_decimal(), dec!(0.001));
        assert_eq!(position.buy_qty.as_decimal(), Decimal::ZERO);
        assert_eq!(position.sell_qty.as_decimal(), Decimal::ZERO);
        assert_eq!(position.commissions(), vec![Money::from("0.00100000 BTC")]);
        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(
            position.adjustments[0].adjustment_type,
            PositionAdjustmentType::Commission
        );
        assert_eq!(position.adjustments[0].quantity_change, Some(dec!(-0.001)));
        assert_eq!(position.opening_order_id, ClientOrderId::from("O-BASE-FEE"));
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.ts_init, UnixNanos::from(1_900));
        assert_eq!(position.ts_opened, UnixNanos::from(2_000));
        assert_eq!(position.ts_last, UnixNanos::from(2_000));
        assert_eq!(position.ts_closed, None);
        assert_eq!(position.duration_ns, DurationNanos::default());
        assert_eq!(position.avg_px_open, dec!(50_000.0));
        assert_eq!(position.avg_px_close, None);
        assert_eq!(position.realized_pnl, None);
        assert!(position.events.is_empty());
        assert!(position.is_open());
        assert!(!position.is_closed());
    }

    #[rstest]
    fn test_surviving_base_commission_can_close_position() {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let position_id = PositionId::from("P-BASE-FEE-CLOSE");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-BASE-OPEN"))
            .trade_id(TradeId::from("T-BASE-OPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from("0.001000"))
            .last_px(Price::from("50000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1_000))
            .ts_init(UnixNanos::from(900))
            .build();
        let fee_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-BASE-FEE"))
            .trade_id(TradeId::from("T-BASE-FEE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from("1.000000"))
            .last_px(Price::from("51000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .commission(Money::from("0.00100000 BTC"))
            .ts_event(UnixNanos::from(2_000))
            .ts_init(UnixNanos::from(1_900))
            .build();
        let fill_voided = matching_fill_void(&fee_fill, fee_fill.last_qty, None);
        let mut position = Position::new(&instrument, opening);
        position.apply(&fee_fill);

        let closed_cycles_pnl = position
            .apply_fill_void(fill_voided, Quantity::from("1.000000"), None)
            .unwrap();

        assert_eq!(closed_cycles_pnl, None);
        assert_eq!(position.entry, OrderSide::Buy);
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.signed_decimal_qty(), Decimal::ZERO);
        assert_eq!(position.quantity.as_decimal(), Decimal::ZERO);
        assert_eq!(position.buy_qty.as_decimal(), dec!(0.001));
        assert_eq!(position.sell_qty.as_decimal(), Decimal::ZERO);
        assert_eq!(position.commissions(), vec![Money::from("0.00100000 BTC")]);
        assert_eq!(position.events.len(), 1);
        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(
            position.opening_order_id,
            ClientOrderId::from("O-BASE-OPEN")
        );
        assert_eq!(
            position.closing_order_id,
            Some(ClientOrderId::from("O-BASE-FEE"))
        );
        assert_eq!(position.ts_init, UnixNanos::from(900));
        assert_eq!(position.ts_opened, UnixNanos::from(1_000));
        assert_eq!(position.ts_last, UnixNanos::from(2_000));
        assert_eq!(position.ts_closed, Some(UnixNanos::from(2_000)));
        assert_eq!(position.duration_ns, DurationNanos::new(1_000));
        assert_eq!(position.avg_px_open, dec!(50_000.0));
        assert_eq!(position.avg_px_close, None);
        assert_eq!(position.realized_pnl, Some(Money::from("0.00 USDT")));
        assert!(!position.is_open());
        assert!(position.is_closed());
    }

    #[rstest]
    fn test_surviving_base_commission_reopen_returns_previous_cycle_pnl() {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let position_id = PositionId::from("P-BASE-FEE-REOPEN");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-OPEN"))
            .trade_id(TradeId::from("T-OPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from("1.000000"))
            .last_px(Price::from("50000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1_000))
            .build();
        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CLOSE"))
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from("1.000000"))
            .last_px(Price::from("51000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2_000))
            .build();
        let reopening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-REOPEN"))
            .trade_id(TradeId::from("T-REOPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from("1.000000"))
            .last_px(Price::from("52000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .commission(Money::from("0.00100000 BTC"))
            .ts_event(UnixNanos::from(3_000))
            .ts_init(UnixNanos::from(2_900))
            .build();
        let fill_voided = matching_fill_void(&reopening, reopening.last_qty, None);
        let mut position = Position::new(&instrument, opening);
        position.apply(&closing);
        position.apply(&reopening);

        let closed_cycles_pnl = position
            .apply_fill_void(fill_voided, Quantity::from("1.000000"), None)
            .unwrap();

        assert_eq!(closed_cycles_pnl, Some(Money::from("1000.00 USDT")));
        assert_eq!(position.entry, OrderSide::Sell);
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.signed_decimal_qty(), dec!(-0.001));
        assert_eq!(position.quantity.as_decimal(), dec!(0.001));
        assert_eq!(position.buy_qty.as_decimal(), Decimal::ZERO);
        assert_eq!(position.sell_qty.as_decimal(), Decimal::ZERO);
        assert_eq!(position.commissions(), vec![Money::from("0.00100000 BTC")]);
        assert!(position.events.is_empty());
        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(position.opening_order_id, ClientOrderId::from("O-REOPEN"));
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.ts_init, UnixNanos::from(2_900));
        assert_eq!(position.ts_opened, UnixNanos::from(3_000));
        assert_eq!(position.ts_last, UnixNanos::from(3_000));
        assert_eq!(position.ts_closed, None);
        assert_eq!(position.duration_ns, DurationNanos::default());
        assert_eq!(position.avg_px_open, dec!(52_000.0));
        assert_eq!(position.avg_px_close, None);
        assert_eq!(position.realized_pnl, None);
        assert!(position.is_open());
        assert!(!position.is_closed());
    }

    #[rstest]
    fn test_surviving_base_commission_can_flip_position() {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let position_id = PositionId::from("P-BASE-FEE-FLIP");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-BASE-OPEN"))
            .trade_id(TradeId::from("T-BASE-OPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from("0.000500"))
            .last_px(Price::from("50000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1_000))
            .ts_init(UnixNanos::from(900))
            .build();
        let fee_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-BASE-FEE"))
            .trade_id(TradeId::from("T-BASE-FEE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from("1.000000"))
            .last_px(Price::from("52000.00"))
            .currency(Currency::USDT())
            .position_id(position_id)
            .commission(Money::from("0.00100000 BTC"))
            .ts_event(UnixNanos::from(2_000))
            .ts_init(UnixNanos::from(1_900))
            .build();
        let fill_voided = matching_fill_void(&fee_fill, fee_fill.last_qty, None);
        let mut position = Position::new(&instrument, opening);
        position.apply(&fee_fill);

        let closed_cycles_pnl = position
            .apply_fill_void(fill_voided, Quantity::from("1.000000"), None)
            .unwrap();

        assert_eq!(closed_cycles_pnl, None);
        assert_eq!(position.entry, OrderSide::Sell);
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.signed_decimal_qty(), dec!(-0.0005));
        assert_eq!(position.quantity.as_decimal(), dec!(0.0005));
        assert_eq!(position.buy_qty.as_decimal(), dec!(0.0005));
        assert_eq!(position.sell_qty.as_decimal(), Decimal::ZERO);
        assert_eq!(position.commissions(), vec![Money::from("0.00100000 BTC")]);
        assert_eq!(position.events.len(), 1);
        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(
            position.opening_order_id,
            ClientOrderId::from("O-BASE-OPEN")
        );
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.ts_init, UnixNanos::from(900));
        assert_eq!(position.ts_opened, UnixNanos::from(1_000));
        assert_eq!(position.ts_last, UnixNanos::from(2_000));
        assert_eq!(position.ts_closed, None);
        assert_eq!(position.duration_ns, DurationNanos::default());
        assert_eq!(position.avg_px_open, dec!(52_000.0));
        assert_eq!(position.avg_px_close, None);
        assert_eq!(position.realized_pnl, Some(Money::from("0.00 USDT")));
        assert!(position.is_open());
        assert!(!position.is_closed());
    }

    #[rstest]
    fn test_fill_void_replays_netting_flip_fragments_with_one_trade_id(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-FLIP-VOID");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-OPEN"))
            .trade_id(TradeId::from("T-OPEN"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-FLIP"))
            .trade_id(TradeId::from("T-FLIP"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.10000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let mut reopening = closing.clone();
        reopening.last_qty = Quantity::from(5);
        reopening.event_id = uuid4();
        reopening.causation_id = Some(closing.event_id);
        let fill_voided = matching_fill_void(&closing, Quantity::from(12), None);
        let mut position = Position::new(&instrument, opening);
        position.apply(&closing);
        assert!(!position.is_duplicate_replay_fill(&reopening));
        position.apply(&reopening);

        position
            .apply_fill_void(fill_voided, Quantity::from(12), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(7));
        assert_eq!(position.buy_qty, Quantity::from(10));
        assert_eq!(position.sell_qty, Quantity::from(3));
        assert_eq!(position.replay_events.len(), 3);
        assert!(position.is_duplicate_replay_fill(&reopening));
    }

    #[rstest]
    fn test_fill_void_replays_split_fragments_in_one_corrected_cycle(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-FLIP-CYCLE-VOID");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-SELL-1"))
            .trade_id(TradeId::from("T-SELL-1"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(17))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let second_sell = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-SELL-2"))
            .trade_id(TradeId::from("T-SELL-2"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(17))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-FLIP"))
            .trade_id(TradeId::from("T-FLIP"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(34))
            .last_px(Price::from("1.10000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let mut reopening = closing.clone();
        reopening.last_qty = Quantity::from(591);
        reopening.event_id = uuid4();
        reopening.causation_id = Some(closing.event_id);
        let fill_voided = matching_fill_void(&second_sell, Quantity::from(2), None);
        let mut position = Position::new(&instrument, opening);
        position.apply(&second_sell);
        position.apply(&closing);
        position.apply(&reopening);

        position
            .apply_fill_void(fill_voided, Quantity::from(2), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(593));
        // The reversal starts a new episode with only the opening residual
        assert_eq!(position.buy_qty, Quantity::from(593));
        assert_eq!(position.sell_qty, Quantity::from(0));
        assert_eq!(position.events.len(), 4);
        assert_eq!(position.replay_events.len(), 4);
        assert_eq!(position.fill_voids.len(), 1);
        assert_eq!(position.trade_ids.len(), 3);
        assert!(position.trade_ids.contains(&TradeId::from("T-FLIP")));
    }

    #[rstest]
    fn test_fill_void_replays_partially_voided_reversal(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-REVERSAL-PARTIAL-VOID");
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-OPEN"))
            .trade_id(TradeId::from("T-OPEN"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let partial_close = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CLOSE"))
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(4))
            .last_px(Price::from("0.90000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let reversal = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-REVERSE"))
            .trade_id(TradeId::from("T-REVERSE"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(11))
            .last_px(Price::from("1.10000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .build();
        let fill_voided = matching_fill_void(&reversal, Quantity::from(2), None);
        let mut position = Position::new(&instrument, opening);
        position.apply(&partial_close);
        position.apply(&reversal);

        position
            .apply_fill_void(fill_voided, Quantity::from(2), None)
            .unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(3));
        assert_eq!(position.avg_px_open, dec!(1.1));
        assert_eq!(position.avg_px_close, None);
        assert_eq!(position.realized_return, 0.0);
        assert_eq!(position.buy_qty, Quantity::from(3));
        assert_eq!(position.sell_qty, Quantity::from(0));
        assert_eq!(position.realized_pnl, Some(Money::from("-0.20 USD")));
        assert_eq!(position.fill_voids.len(), 1);
    }

    #[rstest]
    fn test_position_realized_pnl_with_interleaved_order_sides(
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(12))
            .build();
        let commission1 = calculate_commission(
            &btcusdt,
            order1.quantity(),
            Price::from("10000.0"),
            None,
            dec!(0.001),
        );
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &btcusdt,
            Some(TradeId::from("1")),
            Some(PositionId::from("P-19700101-000000-001-001-1")),
            Some(Price::from("10000.0")),
            None,
            None,
            Some(commission1),
            None,
            None,
        );
        let mut position = Position::new(&btcusdt, fill1.into());
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(17))
            .build();
        let commission2 = calculate_commission(
            &btcusdt,
            order2.quantity(),
            Price::from("9999.0"),
            None,
            dec!(0.001),
        );
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &btcusdt,
            Some(TradeId::from("2")),
            Some(PositionId::from("P-19700101-000000-001-001-1")),
            Some(Price::from("9999.0")),
            None,
            None,
            Some(commission2),
            None,
            None,
        );
        position.apply(&fill2.into());
        assert_eq!(position.quantity, Quantity::from(29));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from("-289.98300000 USDT"))
        );
        assert_eq!(position.avg_px_open, (dec!(289983) / dec!(29)));
        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(9))
            .build();
        let commission3 = calculate_commission(
            &btcusdt,
            order3.quantity(),
            Price::from("10001.0"),
            None,
            dec!(0.001),
        );
        let fill3 = TestOrderEventStubs::filled(
            &order3,
            &btcusdt,
            Some(TradeId::from("3")),
            Some(PositionId::from("P-19700101-000000-001-001-1")),
            Some(Price::from("10001.0")),
            None,
            None,
            Some(commission3),
            None,
            None,
        );
        position.apply(&fill3.into());
        assert_eq!(position.quantity, Quantity::from(20));
        assert_eq!(
            position.realized_pnl,
            Some(native_money(dec!(-1325721) / dec!(3625), Currency::USDT()))
        );
        assert_eq!(position.avg_px_open, (dec!(289983) / dec!(29)));
        let order4 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(3))
            .build();
        let commission4 = calculate_commission(
            &btcusdt,
            order4.quantity(),
            Price::from("10003.0"),
            None,
            dec!(0.001),
        );
        let fill4 = TestOrderEventStubs::filled(
            &order4,
            &btcusdt,
            Some(TradeId::from("4")),
            Some(PositionId::from("P-19700101-000000-001-001-1")),
            Some(Price::from("10003.0")),
            None,
            None,
            Some(commission4),
            None,
            None,
        );
        position.apply(&fill4.into());
        assert_eq!(position.quantity, Quantity::from(23));
        assert_eq!(
            position.realized_pnl,
            Some(native_money(
                dec!(-11476029) / dec!(29000),
                Currency::USDT()
            ))
        );
        assert_eq!(position.avg_px_open, (dec!(6669921) / dec!(667)));
        let order5 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(4))
            .build();
        let commission5 = calculate_commission(
            &btcusdt,
            order5.quantity(),
            Price::from("10005.0"),
            None,
            dec!(0.001),
        );
        let fill5 = TestOrderEventStubs::filled(
            &order5,
            &btcusdt,
            Some(TradeId::from("5")),
            Some(PositionId::from("P-19700101-000000-001-001-1")),
            Some(Price::from("10005.0")),
            None,
            None,
            Some(commission5),
            None,
            None,
        );
        position.apply(&fill5.into());
        assert_eq!(position.quantity, Quantity::from(19));
        assert_eq!(
            position.realized_pnl,
            Some(native_money(
                dec!(-276986007) / dec!(667000),
                Currency::USDT()
            ))
        );
        assert_eq!(position.avg_px_open, (dec!(6669921) / dec!(667)));
        assert_eq!(
            format!("{position}"),
            "Position(LONG 19.000000 BTCUSDT.BINANCE, id=P-19700101-000000-001-001-1)"
        );
    }

    #[rstest]
    fn test_calculate_pnl_when_given_position_side_flat_returns_zero(
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(12))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusdt,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let position = Position::new(&btcusdt, fill.into());
        let result = position
            .calculate_pnl(dec!(10500.0), dec!(10500.0), Quantity::from("100000.0"))
            .unwrap();
        assert_eq!(result, Money::from("0 USDT"));
    }

    #[rstest]
    fn test_calculate_pnl_for_long_position_win(currency_pair_btcusdt: CurrencyPair) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(12))
            .build();
        let commission = calculate_commission(
            &btcusdt,
            order.quantity(),
            Price::from("10500.0"),
            None,
            dec!(0.001),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusdt,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&btcusdt, fill.into());
        let pnl = position
            .calculate_pnl(dec!(10500.0), dec!(10510.0), Quantity::from("12.0"))
            .unwrap();
        assert_eq!(pnl, Money::from("120 USDT"));
        assert_eq!(position.realized_pnl, Some(Money::from("-126 USDT")));
        assert_eq!(
            position.unrealized_pnl(Price::from("10510.0")).unwrap(),
            Money::from("120.0 USDT")
        );
        assert_eq!(
            position.total_pnl(Price::from("10510.0")).unwrap(),
            Money::from("-6 USDT")
        );
        assert_eq!(position.commissions(), vec![Money::from("126.0 USDT")]);
    }

    #[rstest]
    fn test_calculate_pnl_for_long_position_loss(currency_pair_btcusdt: CurrencyPair) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(12))
            .build();
        let commission = calculate_commission(
            &btcusdt,
            order.quantity(),
            Price::from("10500.0"),
            None,
            dec!(0.001),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusdt,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&btcusdt, fill.into());
        let pnl = position
            .calculate_pnl(dec!(10500.0), dec!(10480.5), Quantity::from("10.0"))
            .unwrap();
        assert_eq!(pnl, Money::from("-195 USDT"));
        assert_eq!(position.realized_pnl, Some(Money::from("-126 USDT")));
        assert_eq!(
            position.unrealized_pnl(Price::from("10480.50")).unwrap(),
            Money::from("-234.0 USDT")
        );
        assert_eq!(
            position.total_pnl(Price::from("10480.50")).unwrap(),
            Money::from("-360 USDT")
        );
        assert_eq!(position.commissions(), vec![Money::from("126.0 USDT")]);
    }

    #[rstest]
    fn test_calculate_pnl_for_short_position_winning(currency_pair_btcusdt: CurrencyPair) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.15"))
            .build();
        let commission = calculate_commission(
            &btcusdt,
            order.quantity(),
            Price::from("10500.0"),
            None,
            dec!(0.001),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusdt,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&btcusdt, fill.into());
        let pnl = position
            .calculate_pnl(dec!(10500.0), dec!(10390.0), Quantity::from("10.15"))
            .unwrap();
        assert_eq!(pnl, Money::from("1116.5 USDT"));
        assert_eq!(
            position.unrealized_pnl(Price::from("10390.0")).unwrap(),
            Money::from("1116.5 USDT")
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-106.575 USDT")));
        assert_eq!(position.commissions(), vec![Money::from("106.575 USDT")]);
        assert_eq!(
            position.notional_value(Price::from("10390.0")),
            Money::from("105458.5 USDT")
        );
    }

    #[rstest]
    fn test_calculate_pnl_for_short_position_loss(currency_pair_btcusdt: CurrencyPair) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.0"))
            .build();
        let commission = calculate_commission(
            &btcusdt,
            order.quantity(),
            Price::from("10500.0"),
            None,
            dec!(0.001),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusdt,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10500.0")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&btcusdt, fill.into());
        let pnl = position
            .calculate_pnl(dec!(10500.0), dec!(10670.5), Quantity::from("10.0"))
            .unwrap();
        assert_eq!(pnl, Money::from("-1705 USDT"));
        assert_eq!(
            position.unrealized_pnl(Price::from("10670.5")).unwrap(),
            Money::from("-1705 USDT")
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-105 USDT")));
        assert_eq!(position.commissions(), vec![Money::from("105 USDT")]);
        assert_eq!(
            position.notional_value(Price::from("10670.5")),
            Money::from("106705 USDT")
        );
    }

    #[rstest]
    fn test_calculate_pnl_for_inverse1(btcusd_bybit: CryptoPerpetual) {
        let btcusd_bybit = InstrumentAny::CryptoPerpetual(btcusd_bybit);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusd_bybit.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("100000"))
            .build();
        let commission = calculate_commission(
            &btcusd_bybit,
            order.quantity(),
            Price::from("10000.0"),
            None,
            dec!(0.00075),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusd_bybit,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("10000.0")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&btcusd_bybit, fill.into());
        let pnl = position
            .calculate_pnl(dec!(10000.0), dec!(11000.0), Quantity::from("100000.0"))
            .unwrap();
        assert_eq!(pnl, native_money(dec!(-10) / dec!(11), Currency::BTC()));
        assert_eq!(
            position.unrealized_pnl(Price::from("11000.0")).unwrap(),
            native_money(dec!(-10) / dec!(11), Currency::BTC())
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-0.00750000 BTC")));
        assert_eq!(
            position.notional_value(Price::from("11000.0")),
            native_money(dec!(100) / dec!(11), Currency::BTC())
        );
    }

    #[rstest]
    #[case("0.030", "0.001 BTC", "0.003 BTC", "0.00098 BTC")]
    #[case("0.000", "-0.002 BTC", "0 BTC", "-0.00202 BTC")]
    fn test_calculate_pnl_for_inverse_option_values_premium_in_base(
        mut crypto_option_btc_deribit: CryptoOption,
        #[case] close_px: &str,
        #[case] expected_unrealized_pnl: &str,
        #[case] expected_notional: &str,
        #[case] expected_realized_pnl: &str,
    ) {
        crypto_option_btc_deribit.is_inverse = true;
        crypto_option_btc_deribit.multiplier = Quantity::from("0.01");
        let option = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(option.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .build();
        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(option.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.0"))
            .build();
        let open_fill = TestOrderEventStubs::filled(
            &open_order,
            &option,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("0.020")),
            None,
            None,
            Some(Money::from("0.00001 BTC")),
            None,
            None,
        );
        let close_fill = TestOrderEventStubs::filled(
            &close_order,
            &option,
            Some(TradeId::new("2")),
            None,
            Some(Price::from(close_px)),
            None,
            None,
            Some(Money::from("0.00001 BTC")),
            None,
            None,
        );
        let mut position = Position::new(&option, open_fill.into());
        let unrealized_pnl = position.unrealized_pnl(Price::from(close_px)).unwrap();
        let notional = position.notional_value(Price::from(close_px));

        position.apply(&close_fill.into());

        assert_eq!(unrealized_pnl, Money::from(expected_unrealized_pnl));
        assert_eq!(notional, Money::from(expected_notional));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from(expected_realized_pnl))
        );
    }

    #[rstest]
    fn test_calculate_pnl_scales_by_contract_multiplier(
        mut btcusd_bybit: CryptoPerpetual,
        mut crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        btcusd_bybit.multiplier = Quantity::from(10);
        let btcusd_bybit = InstrumentAny::CryptoPerpetual(btcusd_bybit);
        let inverse_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusd_bybit.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("100000"))
            .build();
        let inverse_fill = TestOrderEventStubs::filled(
            &inverse_order,
            &btcusd_bybit,
            None,
            Some(PositionId::from("P-MULTIPLIER-INVERSE")),
            Some(Price::from("10000.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let inverse_position = Position::new(&btcusd_bybit, inverse_fill.into());

        assert_eq!(
            inverse_position
                .calculate_pnl(dec!(10000.0), dec!(11000.0), Quantity::from("100000.0"))
                .unwrap(),
            native_money(dec!(-100) / dec!(11), Currency::BTC())
        );

        crypto_perpetual_ethusdt.multiplier = Quantity::from(10);
        let ethusdt = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        let linear_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.000"))
            .build();
        let linear_fill = TestOrderEventStubs::filled(
            &linear_order,
            &ethusdt,
            None,
            Some(PositionId::from("P-MULTIPLIER-LINEAR")),
            Some(Price::from("1000.00")),
            None,
            None,
            None,
            None,
            None,
        );
        let linear_position = Position::new(&ethusdt, linear_fill.into());

        assert_eq!(
            linear_position
                .calculate_pnl(dec!(1000.0), dec!(1100.0), Quantity::from("2.000"))
                .unwrap(),
            Money::new(2000.0, linear_position.settlement_currency)
        );
    }

    #[rstest]
    fn test_try_notional_value_for_inverse_zero_price_returns_error(btcusd_bybit: CryptoPerpetual) {
        let btcusd_bybit = InstrumentAny::CryptoPerpetual(btcusd_bybit);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusd_bybit.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("100000"))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusd_bybit,
            None,
            Some(PositionId::from("P-ZERO-PRICE")),
            Some(Price::from("10000.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&btcusd_bybit, fill.into());

        let result = position.try_notional_value(Price::new(0.0, 1));

        assert_eq!(
            result.unwrap_err().to_string(),
            "price must be positive for inverse notional valuation"
        );
        assert!(
            position
                .try_calculate_pnl(dec!(10_000.0), dec!(0.0), position.quantity)
                .is_err()
        );
        assert!(position.try_unrealized_pnl(Price::new(0.0, 1)).is_err());
        assert!(position.try_total_pnl(Price::new(0.0, 1)).is_err());
        assert!(position.try_unrealized_pnl(Price::new(-1.0, 1)).is_err());
        assert!(
            position
                .calculate_pnl(dec!(10_000.0), dec!(0.0), position.quantity)
                .is_err()
        );
        assert!(position.unrealized_pnl(Price::new(0.0, 1)).is_err());
        assert!(position.total_pnl(Price::new(0.0, 1)).is_err());

        position.base_currency = None;
        let result = position.try_notional_value(Price::from("10000.0"));

        assert_eq!(
            result.unwrap_err().to_string(),
            "inverse position BTCUSD.BYBIT has no base currency"
        );
        assert!(position.try_unrealized_pnl(Price::from("10000.0")).is_err());
    }

    #[rstest]
    fn test_calculate_avg_px_zero_quantity_paths(stub_position_long: Position) {
        let mut position = stub_position_long;
        let mut fill = position.events[0].clone();
        position.signed_qty = Decimal::ZERO;
        position.quantity = Quantity::zero(position.size_precision);
        position.cost_open = Decimal::ZERO;
        fill.last_qty = Quantity::zero(position.size_precision);
        assert!(
            position
                .handle_order_fill(&fill, fill.last_px.as_decimal(), Decimal::ZERO)
                .is_err()
        );
        fill.last_qty = Quantity::from(5);
        fill.last_px = Price::from("2");
        position.handle_order_fill(&fill, dec!(2), dec!(5)).unwrap();
        assert_eq!(position.avg_px_open, dec!(2));
        assert_eq!(position.cost_open, dec!(10));
    }

    #[rstest]
    fn test_calculate_return_with_zero_open_price(stub_position_long: Position) {
        assert_eq!(
            stub_position_long.calculate_return(Decimal::ZERO, dec!(1.5)),
            0.0
        );
        assert_eq!(stub_position_long.calculate_return(dec!(2), dec!(3)), 0.5);
    }

    #[rstest]
    fn test_calculate_points_inverse_rejects_non_positive_prices(btcusd_bybit: CryptoPerpetual) {
        let btcusd_bybit = InstrumentAny::CryptoPerpetual(btcusd_bybit);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusd_bybit.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("100000"))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusd_bybit,
            None,
            Some(PositionId::from("P-INVERSE-GUARD")),
            Some(Price::from("10000.0")),
            None,
            None,
            None,
            None,
            None,
        );
        let position = Position::new(&btcusd_bybit, fill.into());

        for (open, close) in [
            (dec!(-1), dec!(10000)),
            (dec!(10000), dec!(-1)),
            (Decimal::ZERO, dec!(10000)),
            (dec!(10000), Decimal::ZERO),
        ] {
            assert!(
                position
                    .try_calculate_pnl(open, close, position.quantity)
                    .is_err()
            );
        }

        assert!(
            position
                .try_calculate_pnl(dec!(0.0000000000000001), dec!(10000), position.quantity)
                .is_err()
        );
    }

    #[rstest]
    fn test_calculate_pnl_for_inverse2(ethusd_bybit: CryptoPerpetual) {
        let ethusd_bybit = InstrumentAny::CryptoPerpetual(ethusd_bybit);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusd_bybit.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("100000"))
            .build();
        let commission = calculate_commission(
            &ethusd_bybit,
            order.quantity(),
            Price::from("375.95"),
            None,
            dec!(0.00075),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &ethusd_bybit,
            None,
            Some(PositionId::from("P-123456")),
            Some(Price::from("375.95")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&ethusd_bybit, fill.into());

        assert_eq!(
            position.unrealized_pnl(Price::from("370.00")).unwrap(),
            native_money(dec!(1190000) / dec!(278203), Currency::ETH())
        );
        assert_eq!(
            position.notional_value(Price::from("370.00")),
            native_money(dec!(10000) / dec!(37), Currency::ETH())
        );
    }

    #[rstest]
    fn test_notional_value_for_quanto_uses_settlement_currency(ethbtc_quanto: CryptoFuture) {
        let instrument = InstrumentAny::CryptoFuture(ethbtc_quanto);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("5"))
            .build();
        let price = Price::from("0.03600");
        let fill = TestOrderEventStubs::filled(
            &order,
            &instrument,
            None,
            Some(PositionId::from("P-QUANTO-NOTIONAL")),
            Some(price),
            None,
            None,
            None,
            None,
            None,
        );
        let position = Position::new(&instrument, fill.into());
        let position_notional = position.notional_value(price);
        let instrument_notional =
            instrument.calculate_notional_value(position.quantity, price, None);

        assert_eq!(position_notional, instrument_notional);
        assert_eq!(position_notional, Money::from("0.18 USDT"));
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_long(currency_pair_btcusdt: CurrencyPair) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.000000"))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.000000"))
            .build();
        let commission1 = calculate_commission(
            &btcusdt,
            order1.quantity(),
            Price::from("10500.0"),
            None,
            dec!(0.001),
        );
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &btcusdt,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("10500.00")),
            None,
            None,
            Some(commission1),
            None,
            None,
        );
        let commission2 = calculate_commission(
            &btcusdt,
            order2.quantity(),
            Price::from("10500.0"),
            None,
            dec!(0.001),
        );
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &btcusdt,
            Some(TradeId::new("2")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("10500.00")),
            None,
            None,
            Some(commission2),
            None,
            None,
        );
        let mut position = Position::new(&btcusdt, fill1.into());
        position.apply(&fill2.into());
        let pnl = position.unrealized_pnl(Price::from("11505.60")).unwrap();
        assert_eq!(pnl, Money::from("4022.40000000 USDT"));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from("-42.00000000 USDT"))
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from("42.00000000 USDT")]
        );
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_short(currency_pair_btcusdt: CurrencyPair) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("5.912000"))
            .build();
        let commission = calculate_commission(
            &btcusdt,
            order.quantity(),
            Price::from("10505.60"),
            None,
            dec!(0.001),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusdt,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("10505.60")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&btcusdt, fill.into());
        let pnl = position.unrealized_pnl(Price::from("10407.15")).unwrap();
        assert_eq!(pnl, Money::from("582.03640000 USDT"));
        assert_eq!(
            position.realized_pnl,
            Some(Money::from("-62.10910720 USDT"))
        );
        assert_eq!(
            position.commissions(),
            vec![Money::from("62.10910720 USDT")]
        );
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_long_inverse(btcusd_bybit: CryptoPerpetual) {
        let btcusd_bybit = InstrumentAny::CryptoPerpetual(btcusd_bybit);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusd_bybit.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();
        let commission = calculate_commission(
            &btcusd_bybit,
            order.quantity(),
            Price::from("10500.0"),
            None,
            dec!(0.00075),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusd_bybit,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("10500.00")),
            None,
            None,
            Some(commission),
            None,
            None,
        );

        let position = Position::new(&btcusd_bybit, fill.into());
        let pnl = position.unrealized_pnl(Price::from("11505.60")).unwrap();
        assert_eq!(
            pnl,
            native_money(dec!(41900) / dec!(50337), Currency::BTC())
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-0.00714286 BTC")));
        assert_eq!(position.commissions(), vec![Money::from("0.00714286 BTC")]);
    }

    #[rstest]
    fn test_calculate_unrealized_pnl_for_short_inverse(btcusd_bybit: CryptoPerpetual) {
        let btcusd_bybit = InstrumentAny::CryptoPerpetual(btcusd_bybit);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btcusd_bybit.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1250000"))
            .build();
        let commission = calculate_commission(
            &btcusd_bybit,
            order.quantity(),
            Price::from("15500.00"),
            None,
            dec!(0.00075),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &btcusd_bybit,
            Some(TradeId::new("1")),
            Some(PositionId::new("P-123456")),
            Some(Price::from("15500.00")),
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&btcusd_bybit, fill.into());
        let pnl = position.unrealized_pnl(Price::from("12506.65")).unwrap();

        assert_eq!(
            pnl,
            native_money(dec!(149667500) / dec!(7754123), Currency::BTC())
        );
        assert_eq!(position.realized_pnl, Some(Money::from("-0.06048387 BTC")));
        assert_eq!(position.commissions(), vec![Money::from("0.06048387 BTC")]);
    }

    #[rstest]
    #[case(OrderSide::Buy, 25, 25.0)]
    #[case(OrderSide::Sell,25,-25.0)]
    fn test_signed_qty_decimal_qty_for_equity(
        #[case] order_side: OrderSide,
        #[case] quantity: i64,
        #[case] expected: f64,
        audusd_sim: CurrencyPair,
    ) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(order_side)
            .quantity(Quantity::from(quantity))
            .build();

        let commission = calculate_commission(
            &audusd_sim,
            order.quantity(),
            Price::from("1.0"),
            None,
            dec!(0.00002),
        );
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            None,
            Some(PositionId::from("P-123456")),
            None,
            None,
            None,
            Some(commission),
            None,
            None,
        );
        let position = Position::new(&audusd_sim, fill.into());
        assert_eq!(position.signed_qty, Decimal::try_from(expected).unwrap());
    }

    #[rstest]
    fn test_position_with_commission_none(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let fill = OrderFilledSpec::builder()
            .position_id(PositionId::from("1"))
            .build();

        let position = Position::new(&audusd_sim, fill);
        assert_eq!(position.realized_pnl, Some(Money::from("0 USD")));
    }

    #[rstest]
    fn test_position_with_commission_zero(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let fill = OrderFilledSpec::builder()
            .position_id(PositionId::from("1"))
            .commission(Money::from("0 USD"))
            .build();

        let position = Position::new(&audusd_sim, fill);
        assert_eq!(position.realized_pnl, Some(Money::from("0 USD")));
    }

    #[rstest]
    fn test_cache_purge_order_events() {
        let audusd_sim = audusd_sim();
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-1"))
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(50_000))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-2"))
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(50_000))
            .build();

        let position_id = PositionId::new("P-123456");

        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &audusd_sim,
            Some(TradeId::new("1")),
            Some(position_id),
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            None,
            None,
        );

        let mut position = Position::new(&audusd_sim, fill1.into());

        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            Some(TradeId::new("2")),
            Some(position_id),
            Some(Price::from("1.00002")),
            None,
            None,
            None,
            None,
            None,
        );

        position.apply(&fill2.into());
        position
            .purge_events_for_order(order1.client_order_id())
            .unwrap();

        assert_eq!(position.events.len(), 1);
        assert_eq!(position.trade_ids.len(), 1);
        assert_eq!(position.events[0].client_order_id, order2.client_order_id());
        assert!(position.trade_ids.contains(&TradeId::new("2")));
    }

    #[rstest]
    fn test_purge_all_events_returns_none_for_last_event_and_trade_id() {
        let audusd_sim = audusd_sim();
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

        let order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::new("O-1"))
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let position_id = PositionId::new("P-123456");
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("1")),
            Some(position_id),
            Some(Price::from("1.00050")),
            None,
            None,
            None,
            Some(UnixNanos::from(1_000_000_000)), // Explicit non-zero timestamp
            None,
        );

        let mut position = Position::new(&audusd_sim, fill.into());

        assert_eq!(position.events.len(), 1);
        assert!(position.last_event().is_some());
        assert!(position.last_trade_id().is_some());

        // Store original timestamps (should be non-zero)
        let original_ts_opened = position.ts_opened;
        let original_ts_last = position.ts_last;
        assert_ne!(original_ts_opened, UnixNanos::default());
        assert_ne!(original_ts_last, UnixNanos::default());

        position
            .purge_events_for_order(order.client_order_id())
            .unwrap();

        assert_eq!(position.events.len(), 0);
        assert_eq!(position.trade_ids.len(), 0);
        assert!(position.last_event().is_none());
        assert!(position.last_trade_id().is_none());

        // Verify timestamps are zeroed - empty shell has no meaningful history
        // ts_closed is set to Some(0) so position reports as closed and is eligible for purge
        assert_eq!(position.ts_opened, UnixNanos::default());
        assert_eq!(position.ts_last, UnixNanos::default());
        assert_eq!(position.ts_closed, Some(UnixNanos::default()));
        assert_eq!(position.duration_ns, DurationNanos::default());

        // Verify empty shell reports as closed (this was the bug we fixed!)
        // is_closed() must return true so cache purge logic recognizes empty shells
        assert!(position.is_closed());
        assert!(!position.is_open());
        assert_eq!(position.side, PositionSide::Flat);
    }

    #[rstest]
    fn test_revive_from_empty_shell(audusd_sim: CurrencyPair) {
        // Test adding a fill to an empty shell position
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

        // Create and then purge position to get empty shell
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &audusd_sim,
            None,
            Some(PositionId::new("P-1")),
            Some(Price::from("1.00000")),
            None,
            None,
            None,
            Some(UnixNanos::from(1_000_000_000)),
            None,
        );

        let mut position = Position::new(&audusd_sim, fill1.into());
        position
            .purge_events_for_order(order1.client_order_id())
            .unwrap();

        // Verify it's an empty shell
        assert!(position.is_closed());
        assert_eq!(position.ts_closed, Some(UnixNanos::default()));
        assert_eq!(position.event_count(), 0);

        // Add new fill to revive the position
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(50_000))
            .build();

        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            None,
            Some(PositionId::new("P-1")),
            Some(Price::from("1.00020")),
            None,
            None,
            None,
            Some(UnixNanos::from(3_000_000_000)),
            None,
        );

        let fill2_typed: OrderFilled = fill2.clone().into();
        position.apply(&fill2_typed);

        // Position should be alive with new timestamps
        assert!(position.is_long());
        assert!(!position.is_closed());
        assert!(position.ts_closed.is_none());
        assert_eq!(position.ts_opened, fill2.ts_event());
        assert_eq!(position.ts_last, fill2.ts_event());
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.quantity, Quantity::from(50_000));
    }

    #[rstest]
    fn test_empty_shell_position_invariants(audusd_sim: CurrencyPair) {
        // Property-based test: Any position with event_count == 0 must satisfy invariants
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            None,
            Some(PositionId::new("P-1")),
            Some(Price::from("1.00000")),
            None,
            None,
            None,
            Some(UnixNanos::from(1_000_000_000)),
            None,
        );

        let mut position = Position::new(&audusd_sim, fill.into());
        position
            .purge_events_for_order(order.client_order_id())
            .unwrap();

        // INVARIANTS: When event_count == 0, the following MUST be true
        assert_eq!(
            position.event_count(),
            0,
            "Precondition: event_count must be 0"
        );

        // Invariant 1: Position must report as closed
        assert!(
            position.is_closed(),
            "INV1: Empty shell must report is_closed() == true"
        );
        assert!(
            !position.is_open(),
            "INV1: Empty shell must report is_open() == false"
        );

        // Invariant 2: Position must be FLAT
        assert_eq!(
            position.side,
            PositionSide::Flat,
            "INV2: Empty shell must be FLAT"
        );

        // Invariant 3: ts_closed must be Some (not None)
        assert!(
            position.ts_closed.is_some(),
            "INV3: Empty shell must have ts_closed.is_some()"
        );
        assert_eq!(
            position.ts_closed,
            Some(UnixNanos::default()),
            "INV3: Empty shell ts_closed must be 0"
        );

        // Invariant 4: All lifecycle timestamps must be zeroed
        assert_eq!(
            position.ts_opened,
            UnixNanos::default(),
            "INV4: Empty shell ts_opened must be 0"
        );
        assert_eq!(
            position.ts_last,
            UnixNanos::default(),
            "INV4: Empty shell ts_last must be 0"
        );
        assert_eq!(
            position.duration_ns,
            DurationNanos::default(),
            "INV4: Empty shell duration_ns must be 0"
        );

        // Invariant 5: Quantity must be zero
        assert_eq!(
            position.quantity,
            Quantity::zero(audusd_sim.size_precision()),
            "INV5: Empty shell quantity must be 0"
        );

        // Invariant 6: No events or trade IDs
        assert!(
            position.events.is_empty(),
            "INV6: Empty shell must have no events"
        );
        assert!(
            position.trade_ids.is_empty(),
            "INV6: Empty shell must have no trade IDs"
        );
        assert!(
            position.last_event().is_none(),
            "INV6: Empty shell must have no last event"
        );
        assert!(
            position.last_trade_id().is_none(),
            "INV6: Empty shell must have no last trade ID"
        );
    }

    #[rstest]
    fn test_position_pnl_precision_with_very_small_amounts(audusd_sim: CurrencyPair) {
        // Tests behavior with very small commission amounts
        // NOTE: Amounts below f64 epsilon (~1e-15) may be lost to precision
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .build();

        // Test with a commission that won't be lost to Money precision (0.01 USD)
        let small_commission = Money::new(0.01, Currency::USD());
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            None,
            None,
            Some(Price::from("1.00001")),
            Some(Quantity::from(100)),
            None,
            Some(small_commission),
            None,
            None,
        );

        let position = Position::new(&audusd_sim, fill.into());

        // Commission is recorded and preserved in f64 arithmetic
        assert_eq!(position.commissions().len(), 1);
        let recorded_commission = position.commissions()[0];
        assert!(
            recorded_commission.as_f64() > 0.0,
            "Commission of 0.01 should be preserved"
        );

        // Realized PnL should include commission (negative)
        let realized = position.realized_pnl.unwrap().as_f64();
        assert!(
            realized < 0.0,
            "Realized PnL should be negative due to commission"
        );
    }

    #[rstest]
    fn test_position_pnl_precision_with_high_precision_instrument() {
        // Tests precision with high-precision crypto instrument
        use crate::instruments::stubs::crypto_perpetual_ethusdt;
        let ethusdt = crypto_perpetual_ethusdt();
        let ethusdt = InstrumentAny::CryptoPerpetual(ethusdt);

        // Check instrument precision
        let size_precision = ethusdt.size_precision();

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(ethusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.123456789"))
            .build();

        let fill = TestOrderEventStubs::filled(
            &order,
            &ethusdt,
            None,
            None,
            Some(Price::from("2345.123456789")),
            Some(Quantity::from("1.123456789")),
            None,
            Some(Money::from("0.1 USDT")),
            None,
            None,
        );

        let position = Position::new(&ethusdt, fill.into());

        // Verify high-precision price is preserved in f64 (within tolerance)
        let avg_px = position.avg_px_open;
        assert_eq!(avg_px, dec!(2345.123456789));

        assert_eq!(position.size_precision, size_precision);
        assert_eq!(position.quantity.precision, 9);
        assert_eq!(position.quantity.as_decimal(), dec!(1.123456789));
        assert_eq!(position.signed_qty, dec!(1.123456789));
    }

    #[rstest]
    fn test_position_pnl_accumulation_across_many_fills(audusd_sim: CurrencyPair) {
        // Tests precision drift across 100 fills
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(1000))
            .build();

        let initial_fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("1.00000")),
            Some(Quantity::from(10)),
            None,
            Some(Money::from("0.01 USD")),
            None,
            None,
        );

        let mut position = Position::new(&audusd_sim, initial_fill.into());

        // Apply 99 more fills with varying prices
        for i in 2..=100 {
            let price_offset = f64::from(i) * 0.00001;
            let fill = TestOrderEventStubs::filled(
                &order,
                &audusd_sim,
                Some(TradeId::new(i.to_string())),
                None,
                Some(Price::from(&format!("{:.5}", 1.0 + price_offset))),
                Some(Quantity::from(10)),
                None,
                Some(Money::from("0.01 USD")),
                None,
                None,
            );
            position.apply(&fill.into());
        }

        // Verify we accumulated 100 fills
        assert_eq!(position.events.len(), 100);
        assert_eq!(position.quantity, Quantity::from(1000));

        // Verify commissions accumulated (should be 100 * 0.01 = 1.0 USD)
        let total_commission: f64 = position.commissions().iter().map(Money::as_f64).sum();
        assert!(
            (total_commission - 1.0).abs() < 1e-10,
            "Commission accumulation should be accurate: expected 1.0, was {total_commission}"
        );

        // Verify average price is reasonable (should be around 1.0005)
        let avg_px = position.avg_px_open;
        assert!(
            avg_px > dec!(1.0) && avg_px < dec!(1.001),
            "Average price should be reasonable: got {avg_px}"
        );
    }

    #[rstest]
    fn test_position_pnl_with_extreme_price_values(audusd_sim: CurrencyPair) {
        // Tests position handling with very large and very small prices
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

        // Test with very small price
        let order_small = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let fill_small = TestOrderEventStubs::filled(
            &order_small,
            &audusd_sim,
            None,
            None,
            Some(Price::from("0.00001")),
            Some(Quantity::from(100_000)),
            None,
            None,
            None,
            None,
        );

        let position_small = Position::new(&audusd_sim, fill_small.into());
        assert_eq!(position_small.avg_px_open, dec!(0.00001));

        // Verify notional calculation doesn't underflow
        let last_price_small = Price::from("0.00002");
        let unrealized = position_small.unrealized_pnl(last_price_small).unwrap();
        assert!(
            unrealized.as_f64() > 0.0,
            "Unrealized PnL should be positive when price doubles"
        );

        // Test with very large price
        let order_large = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .build();

        let fill_large = TestOrderEventStubs::filled(
            &order_large,
            &audusd_sim,
            None,
            None,
            Some(Price::from("99999.99999")),
            Some(Quantity::from(100)),
            None,
            None,
            None,
            None,
        );

        let position_large = Position::new(&audusd_sim, fill_large.into());
        assert!(
            (position_large.avg_px_open - dec!(99999.99999)).abs() < dec!(1e-6),
            "Large price should be preserved within f64 tolerance"
        );
    }

    #[rstest]
    fn test_position_pnl_roundtrip_precision(audusd_sim: CurrencyPair) {
        // Tests that opening and closing a position preserves precision
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let buy_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let sell_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();

        // Open at precise price
        let open_fill = TestOrderEventStubs::filled(
            &buy_order,
            &audusd_sim,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("1.123456")),
            None,
            None,
            Some(Money::from("0.50 USD")),
            None,
            None,
        );

        let mut position = Position::new(&audusd_sim, open_fill.into());

        // Close at same price (no profit/loss except commission)
        let close_fill = TestOrderEventStubs::filled(
            &sell_order,
            &audusd_sim,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("1.123456")),
            None,
            None,
            Some(Money::from("0.50 USD")),
            None,
            None,
        );

        position.apply(&close_fill.into());

        // Position should be flat
        assert!(position.is_closed());

        // Realized PnL should be exactly -1.0 USD (two commissions of 0.50)
        let realized = position.realized_pnl.unwrap().as_f64();
        assert!(
            (realized - (-1.0)).abs() < 1e-10,
            "Realized PnL should be exactly -1.0 USD (commissions), was {realized}"
        );
    }

    #[rstest]
    fn test_position_commission_in_base_currency_buy() {
        // Test that commission in base currency reduces position quantity on buy (SPOT only)
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        // Buy 1.0 BTC with 0.001 BTC commission
        let fill = match TestOrderEventStubs::filled(
            &order,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        ) {
            OrderEventAny::Filled(fill) => fill,
            _ => unreachable!(),
        };

        let position = Position::new(&btc_usdt, fill.clone());
        let replayed_position = Position::new(&btc_usdt, fill);

        // Position quantity should be 1.0 - 0.001 = 0.999 BTC
        assert!(
            (position.quantity.as_f64() - 0.999).abs() < 1e-9,
            "Position quantity should be 0.999 BTC (1.0 - 0.001 commission), was {}",
            position.quantity.as_f64()
        );

        // Signed qty should also be 0.999
        assert!(
            (position.signed_qty - dec!(0.999)).abs() < dec!(1e-9),
            "Signed qty should be 0.999, was {}",
            position.signed_qty
        );

        // Verify PositionAdjusted event was created
        assert_eq!(
            position.adjustments.len(),
            1,
            "Should have 1 adjustment event"
        );
        let adjustment = &position.adjustments[0];
        assert_eq!(
            adjustment.adjustment_type,
            PositionAdjustmentType::Commission
        );
        assert_eq!(
            adjustment.quantity_change,
            Some(rust_decimal_macros::dec!(-0.001))
        );
        assert_eq!(adjustment.pnl_change, None);
        assert_eq!(
            adjustment.event_id,
            replayed_position.adjustments[0].event_id
        );
    }

    #[rstest]
    fn test_tokenized_asset_base_commission_buy_then_close_net_holding() {
        let instrument = InstrumentAny::TokenizedAsset(tokenized_asset_aaplx());
        let base_currency = instrument.base_currency().unwrap();
        let buy = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-BUY"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0000"))
            .build();
        let OrderEventAny::Filled(buy_fill) = TestOrderEventStubs::filled(
            &buy,
            &instrument,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("200.00")),
            Some(Quantity::from("1.0000")),
            None,
            Some(Money::new(0.001, base_currency)),
            None,
            None,
        ) else {
            unreachable!()
        };

        let mut position = Position::new(&instrument, buy_fill);

        assert_eq!(position.quantity, Quantity::from("0.9990"));
        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(
            position.adjustments[0].adjustment_type,
            PositionAdjustmentType::Commission
        );
        assert_eq!(
            position.adjustments[0].quantity_change,
            Some(rust_decimal_macros::dec!(-0.001))
        );

        let sell = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-SELL"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.9990"))
            .build();
        let OrderEventAny::Filled(sell_fill) = TestOrderEventStubs::filled(
            &sell,
            &instrument,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("210.00")),
            Some(Quantity::from("0.9990")),
            None,
            Some(Money::zero(instrument.quote_currency())),
            None,
            None,
        ) else {
            unreachable!()
        };

        position.apply(&sell_fill);

        assert_eq!(position.side, PositionSide::Flat);
        assert!(position.quantity.is_zero());
        assert!(position.is_closed());
    }

    #[rstest]
    fn test_base_commission_adjustment_id_flips_low_bit_of_fill_id(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let base_currency = instrument.base_currency().unwrap();
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .commission(Money::new(2.0, base_currency))
            .position_id(PositionId::from("P-COMMISSION"))
            .event_id(UUID4::from_str("a1b2c3d4-e5f6-4789-8abc-def012345679").unwrap())
            .ts_event(UnixNanos::from(1))
            .build();

        let position = Position::new(&instrument, fill);

        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(
            position.adjustments[0].event_id,
            UUID4::from_str("a1b2c3d4-e5f6-4789-8abc-def012345678").unwrap()
        );
    }

    #[rstest]
    fn test_position_commission_in_base_currency_sell() {
        // Test that commission in base currency increases short position on sell
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.0"))
            .build();

        // Sell 1.0 BTC with 0.001 BTC commission
        let fill = TestOrderEventStubs::filled(
            &order,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let position = Position::new(&btc_usdt, fill.into());

        // Position quantity should be 1.0 + 0.001 = 1.001 BTC
        // (you sold 1.0 and paid 0.001 commission, so total short exposure is 1.001)
        assert!(
            (position.quantity.as_f64() - 1.001).abs() < 1e-9,
            "Position quantity should be 1.001 BTC (1.0 + 0.001 commission), was {}",
            position.quantity.as_f64()
        );

        // Signed qty should be -1.001 (short position)
        assert!(
            (position.signed_qty - (dec!(-1.001))).abs() < dec!(1e-9),
            "Signed qty should be -1.001, was {}",
            position.signed_qty
        );

        // Verify PositionAdjusted event was created
        assert_eq!(
            position.adjustments.len(),
            1,
            "Should have 1 adjustment event"
        );
        let adjustment = &position.adjustments[0];
        assert_eq!(
            adjustment.adjustment_type,
            PositionAdjustmentType::Commission
        );
        // For sell, commission increases the short (negative adjustment)
        assert_eq!(
            adjustment.quantity_change,
            Some(rust_decimal_macros::dec!(-0.001))
        );
        assert_eq!(adjustment.pnl_change, None);
    }

    #[rstest]
    fn test_position_commission_in_quote_currency_no_adjustment() {
        // Test that commission in quote currency does NOT reduce position quantity
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        // Buy 1.0 BTC with 50 USDT commission (in quote currency)
        let fill = TestOrderEventStubs::filled(
            &order,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(50.0, Currency::USD())),
            None,
            None,
        );

        let position = Position::new(&btc_usdt, fill.into());

        // Position quantity should be exactly 1.0 BTC (no adjustment)
        assert!(
            (position.quantity.as_f64() - 1.0).abs() < 1e-9,
            "Position quantity should be 1.0 BTC (no adjustment for quote currency commission), was {}",
            position.quantity.as_f64()
        );

        // Verify NO PositionAdjusted event was created (commission in quote currency)
        assert_eq!(
            position.adjustments.len(),
            0,
            "Should have no adjustment events for quote currency commission"
        );
    }

    #[rstest]
    fn test_position_reset_clears_adjustments() {
        // Test that closing and reopening a position clears adjustment history
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        // Open long position with commission adjustment
        let buy_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        let buy_fill = TestOrderEventStubs::filled(
            &buy_order,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&btc_usdt, buy_fill.into());
        assert_eq!(position.adjustments.len(), 1, "Should have 1 adjustment");

        // Close the position (sell the actual quantity, use quote currency commission to avoid complexity)
        let sell_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.999"))
            .build();

        let sell_fill = TestOrderEventStubs::filled(
            &sell_order,
            &btc_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("51000.0")),
            Some(Quantity::from("0.999")),
            None,
            Some(Money::new(50.0, Currency::USD())), // Quote currency commission - no adjustment
            None,
            None,
        );

        position.apply(&sell_fill.into());
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(
            position.adjustments.len(),
            1,
            "Should still have 1 adjustment (no new one from quote commission)"
        );

        // Reopen the position - adjustments should be cleared
        let buy_order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.0"))
            .build();

        let buy_fill2 = TestOrderEventStubs::filled(
            &buy_order2,
            &btc_usdt,
            Some(TradeId::new("3")),
            None,
            Some(Price::from("52000.0")),
            Some(Quantity::from("2.0")),
            None,
            Some(Money::new(0.002, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        position.apply(&buy_fill2.into());

        // Verify adjustments were cleared and only new adjustment exists
        assert_eq!(
            position.adjustments.len(),
            1,
            "Adjustments should be cleared on position reset, only new adjustment"
        );
        assert_eq!(
            position.adjustments[0].quantity_change,
            Some(rust_decimal_macros::dec!(-0.002)),
            "New adjustment should be for the new fill"
        );
        assert_eq!(position.events.len(), 1, "Events should also be reset");
    }

    #[rstest]
    fn test_id_accessors_deduplicate_and_sort(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-IDS");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .venue_order_id(VenueOrderId::from("V-2"))
            .trade_id(TradeId::from("T-3"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .venue_order_id(VenueOrderId::from("V-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let fill3 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .venue_order_id(VenueOrderId::from("V-1"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(3))
            .build();
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);
        position.apply(&fill3);

        assert_eq!(
            position.client_order_ids(),
            vec![ClientOrderId::from("O-1"), ClientOrderId::from("O-2")]
        );
        assert_eq!(
            position.venue_order_ids(),
            vec![VenueOrderId::from("V-1"), VenueOrderId::from("V-2")]
        );
        assert_eq!(
            position.trade_ids(),
            vec![
                TradeId::from("T-1"),
                TradeId::from("T-2"),
                TradeId::from("T-3")
            ]
        );
    }

    #[rstest]
    fn test_is_open_and_is_closed_require_both_conditions(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-PREDICATES");
        let fill_open = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill_close = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let mut position = Position::new(&instrument, fill_open);
        position.apply(&fill_close);

        assert!(position.is_closed());
        assert!(!position.is_open());

        position
            .apply_adjustment(PositionAdjusted::new(
                position.trader_id,
                position.strategy_id,
                position.instrument_id,
                position.id,
                position.account_id,
                PositionAdjustmentType::Funding,
                Some(dec!(5)),
                None,
                None,
                uuid4(),
                UnixNanos::from(3),
                UnixNanos::from(3),
            ))
            .unwrap();

        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.ts_closed, Some(UnixNanos::from(2)));
        assert!(!position.is_open());
        assert!(!position.is_closed());
    }

    #[rstest]
    fn test_voided_fill_rebate_leaves_no_residual_commission(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-REBATE");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let rebate = Money::from("-2.00 USD");
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .commission(rebate)
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let void2 = matching_fill_void(&fill2, Quantity::from(10), Some(rebate));
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);

        position
            .apply_fill_void(void2, Quantity::from(10), Some(rebate))
            .unwrap();

        assert_eq!(position.quantity, Quantity::from(10));
        assert!(position.commissions().is_empty());
        assert_eq!(position.realized_pnl, Some(Money::from("0.00 USD")));
    }

    #[rstest]
    fn test_voided_fill_commission_accumulates_into_realized_pnl(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-SURVIVING-QUOTE");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .commission(Money::from("2.00 USD"))
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("2.00000"))
            .currency(Currency::USD())
            .commission(Money::from("1.00 USD"))
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let void2 = matching_fill_void(&fill2, Quantity::from(10), None);
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);

        position
            .apply_fill_void(void2, Quantity::from(10), None)
            .unwrap();

        assert_eq!(position.quantity, Quantity::from(10));
        assert_eq!(position.commissions(), vec![Money::from("3.00 USD")]);
        assert_eq!(position.realized_pnl, Some(Money::from("-3.00 USD")));
    }

    #[rstest]
    fn test_voided_fill_base_commission_preserves_avg_px_open(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let base_currency = instrument.base_currency().unwrap();
        let position_id = PositionId::from("P-SURVIVING-BASE");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .commission(Money::new(2.0, base_currency))
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("2.00000"))
            .currency(Currency::USD())
            .commission(Money::new(1.0, base_currency))
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let void2 = matching_fill_void(&fill2, Quantity::from(10), None);
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);

        position
            .apply_fill_void(void2, Quantity::from(10), None)
            .unwrap();

        assert_eq!(position.quantity, Quantity::from(7));
        assert_eq!(position.avg_px_open, dec!(1.0));
        assert_eq!(position.commissions(), vec![Money::new(3.0, base_currency)]);
    }

    #[rstest]
    fn test_fill_fragments_requires_matching_order_and_trade(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-FRAGMENTS");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);

        let fragments = position.fill_fragments(ClientOrderId::from("O-1"), TradeId::from("T-1"));

        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].trade_id, TradeId::from("T-1"));
        assert!(
            position
                .fill_fragments(ClientOrderId::from("O-2"), TradeId::from("T-1"))
                .is_empty()
        );
    }

    #[rstest]
    fn test_apply_fill_void_scopes_staleness_check_to_same_trade(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-TRADES");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let void1 = matching_fill_void(&fill1, Quantity::from(6), None);
        let void2 = matching_fill_void(&fill2, Quantity::from(2), None);
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);
        position
            .apply_fill_void(void1, Quantity::from(6), None)
            .unwrap();

        position
            .apply_fill_void(void2, Quantity::from(2), None)
            .unwrap();

        assert_eq!(position.quantity, Quantity::from(12));
        assert_eq!(position.fill_voids.len(), 2);
    }

    #[rstest]
    fn test_apply_fill_void_removes_quantity_from_voided_trade_only(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-VOID-ATTRIBUTION");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("2.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let void1 = matching_fill_void(&fill1, Quantity::from(4), None);
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);

        position
            .apply_fill_void(void1, Quantity::from(4), None)
            .unwrap();

        assert_eq!(position.quantity, Quantity::from(16));
        assert_eq!(position.avg_px_open, dec!(1.625));
    }

    #[rstest]
    fn test_purge_events_for_order_retains_other_order_fill_voids(audusd_sim: CurrencyPair) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim);
        let position_id = PositionId::from("P-PURGE-VOID");
        let fill1 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-1"))
            .trade_id(TradeId::from("T-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(1))
            .build();
        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-2"))
            .trade_id(TradeId::from("T-2"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(10))
            .last_px(Price::from("1.00000"))
            .currency(Currency::USD())
            .position_id(position_id)
            .ts_event(UnixNanos::from(2))
            .build();
        let void1 = matching_fill_void(&fill1, Quantity::from(2), None);
        let void2 = matching_fill_void(&fill2, Quantity::from(3), None);
        let mut position = Position::new(&instrument, fill1);
        position.apply(&fill2);
        position
            .apply_fill_void(void1, Quantity::from(2), None)
            .unwrap();
        position
            .apply_fill_void(void2, Quantity::from(3), None)
            .unwrap();

        assert_eq!(position.fill_voids.len(), 2);

        position
            .purge_events_for_order(ClientOrderId::from("O-1"))
            .unwrap();

        assert_eq!(position.fill_voids.len(), 1);
        assert_eq!(
            position.fill_voids[0].event.client_order_id,
            ClientOrderId::from("O-2")
        );
        assert_eq!(position.fill_voids[0].voided_qty, Quantity::from(3));
    }

    #[rstest]
    fn test_purge_events_for_order_clears_adjustments_when_flat() {
        // Test that purging all fills clears adjustment history
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        let fill = TestOrderEventStubs::filled(
            &order,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&btc_usdt, fill.into());
        assert_eq!(position.adjustments.len(), 1, "Should have 1 adjustment");
        assert_eq!(position.events.len(), 1);

        // Purge the only fill - should go to flat and clear everything
        position
            .purge_events_for_order(order.client_order_id())
            .unwrap();

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.events.len(), 0, "Events should be cleared");
        assert_eq!(
            position.adjustments.len(),
            0,
            "Adjustments should be cleared when position goes flat"
        );
        assert_eq!(position.quantity, Quantity::zero(btc_usdt.size_precision()));
    }

    #[rstest]
    fn test_purge_events_for_order_clears_adjustments_on_rebuild() {
        // Test that rebuilding position from remaining fills clears and recreates adjustments
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        // First fill with adjustment
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-001"))
            .build();

        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&btc_usdt, fill1.into());
        assert_eq!(position.adjustments.len(), 1);

        // Second fill with different order and adjustment
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.0"))
            .client_order_id(ClientOrderId::new("O-002"))
            .build();

        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &btc_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("51000.0")),
            Some(Quantity::from("2.0")),
            None,
            Some(Money::new(0.002, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        position.apply(&fill2.into());
        assert_eq!(position.adjustments.len(), 2, "Should have 2 adjustments");
        assert_eq!(position.events.len(), 2);

        // Purge first order - should rebuild from remaining fill
        position
            .purge_events_for_order(order1.client_order_id())
            .unwrap();

        assert_eq!(position.events.len(), 1, "Should have 1 remaining event");
        assert_eq!(
            position.adjustments.len(),
            1,
            "Should have only the adjustment from remaining fill"
        );
        assert_eq!(
            position.adjustments[0].quantity_change,
            Some(rust_decimal_macros::dec!(-0.002)),
            "Should be the adjustment from order2"
        );
        assert!(
            (position.quantity.as_f64() - 1.998).abs() < 1e-9,
            "Quantity should be 2.0 - 0.002 commission"
        );
    }

    fn btcusdt_fill(trade_id: &str, client_order_id: &str, quantity: &str) -> OrderFilled {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(quantity))
            .client_order_id(ClientOrderId::new(client_order_id))
            .build();

        TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::new(trade_id)),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from(quantity)),
            None,
            Some(Money::new(1.0, instrument.quote_currency())),
            None,
            None,
        )
        .into()
    }

    fn funding_payment(position: &Position, pnl: f64) -> PositionAdjusted {
        PositionAdjusted::new(
            position.trader_id,
            position.strategy_id,
            position.instrument_id,
            position.id,
            position.account_id,
            PositionAdjustmentType::Funding,
            None,
            Some(Money::new(pnl, position.settlement_currency)),
            None,
            uuid4(),
            UnixNanos::from(5),
            UnixNanos::from(5),
        )
    }

    fn btcusdt_position() -> Position {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        Position::new(&instrument, btcusdt_fill("1", "O-001", "1.0"))
    }

    #[rstest]
    fn test_revert_last_adjustment_restores_prior_state() {
        let mut position = btcusdt_position();
        let before = serde_json::to_value(&position).unwrap();
        let prior = position.clone_without_events();
        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();

        position.revert_last_adjustment(&prior).unwrap();

        assert_eq!(serde_json::to_value(&position).unwrap(), before);
    }

    fn assert_reverts_to_snapshot(mut position: Position, payment: PositionAdjusted) {
        let before = serde_json::to_value(&position).unwrap();
        let prior = position.clone_without_events();
        position.apply_adjustment(payment).unwrap();

        position.revert_last_adjustment(&prior).unwrap();

        assert_eq!(serde_json::to_value(&position).unwrap(), before);
    }

    #[rstest]
    fn test_revert_last_adjustment_after_earlier_adjustment() {
        let mut position = btcusdt_position();
        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();
        let mut payment = funding_payment(&position, -5.0);
        payment.ts_event = UnixNanos::from(6);

        assert_reverts_to_snapshot(position, payment);
    }

    #[rstest]
    fn test_revert_last_adjustment_after_zero_value_adjustment_at_same_time() {
        let mut position = btcusdt_position();
        let prior = position.clone_without_events();
        let mut zero_payment = funding_payment(&position, 0.0);
        zero_payment.ts_event = position.ts_last;
        position.apply_adjustment(zero_payment).unwrap();
        let before = serde_json::to_value(&position).unwrap();
        position
            .apply_adjustment(funding_payment(&position, -5.0))
            .unwrap();

        // The zero value payment at the snapshot's time leaves the snapshot's state unchanged
        position.revert_last_adjustment(&prior).unwrap();

        assert_eq!(serde_json::to_value(&position).unwrap(), before);
    }

    #[rstest]
    #[case::partial("0.2", None)]
    #[case::full("0.5", Some(1.0))]
    fn test_revert_last_adjustment_after_voided_fill(
        #[case] voided_qty: &str,
        #[case] commission_voided: Option<f64>,
    ) {
        let mut position = btcusdt_position();
        let mut fill = btcusdt_fill("2", "O-002", "0.5");
        fill.ts_event = UnixNanos::from(3);
        position.apply(&fill);
        let commission_voided =
            commission_voided.map(|amount| Money::new(amount, position.settlement_currency));
        position
            .apply_fill_void(
                matching_fill_void(&fill, Quantity::from(voided_qty), commission_voided),
                Quantity::from(voided_qty),
                commission_voided,
            )
            .unwrap();
        let payment = funding_payment(&position, -5.0);

        assert_reverts_to_snapshot(position, payment);
    }

    #[rstest]
    fn test_revert_last_adjustment_after_purge() {
        let mut position = btcusdt_position();
        let mut fill = btcusdt_fill("2", "O-002", "0.5");
        fill.ts_event = UnixNanos::from(3);
        position.apply(&fill);
        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();
        position
            .purge_events_for_order(ClientOrderId::new("O-002"))
            .unwrap();
        let mut payment = funding_payment(&position, -5.0);
        payment.ts_event = UnixNanos::from(6);

        assert_reverts_to_snapshot(position, payment);
    }

    #[rstest]
    fn test_revert_last_adjustment_after_reopening() {
        let mut position = btcusdt_position();
        let mut close = btcusdt_fill("2", "O-002", "1.0");
        close.order_side = OrderSide::Sell;
        close.ts_event = UnixNanos::from(2);
        position.apply(&close);
        let mut reopen = btcusdt_fill("3", "O-003", "0.5");
        reopen.ts_event = UnixNanos::from(3);
        position.apply(&reopen);
        let payment = funding_payment(&position, -5.0);

        assert_reverts_to_snapshot(position, payment);
    }

    #[rstest]
    fn test_revert_last_adjustment_rejects_snapshot_of_another_position() {
        let mut position = btcusdt_position();
        let mut prior = position.clone_without_events();
        prior.id = PositionId::new("P-OTHER");
        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(error.to_string().contains("prior.id"), "{error}");
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    fn test_revert_last_adjustment_rejects_after_later_fill() {
        let mut position = btcusdt_position();
        let prior = position.clone_without_events();
        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();
        position.apply(&btcusdt_fill("2", "O-002", "0.5"));
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(
            error.to_string().contains("not its latest adjustment"),
            "{error}"
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    fn test_revert_last_adjustment_rejects_stale_snapshot() {
        let mut position = btcusdt_position();
        let prior = position.clone_without_events();
        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();
        position
            .apply_adjustment(funding_payment(&position, -5.0))
            .unwrap();
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(error.to_string().contains("predates adjustment"), "{error}");
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    fn test_revert_last_adjustment_rejects_snapshot_before_zero_value_adjustment() {
        let mut position = btcusdt_position();
        let prior = position.clone_without_events();
        position
            .apply_adjustment(funding_payment(&position, 0.0))
            .unwrap();
        let mut later_payment = funding_payment(&position, -5.0);
        later_payment.ts_event = UnixNanos::from(6);
        later_payment.ts_init = UnixNanos::from(6);
        position.apply_adjustment(later_payment).unwrap();
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(error.to_string().contains("predates adjustment"), "{error}");
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    fn test_revert_last_adjustment_rejects_snapshot_before_zero_value_adjustment_and_voided_fill() {
        let mut position = btcusdt_position();
        let prior = position.clone_without_events();
        position
            .apply_adjustment(funding_payment(&position, 0.0))
            .unwrap();
        let mut fill = btcusdt_fill("2", "O-002", "0.5");
        fill.ts_event = UnixNanos::from(6);
        position.apply(&fill);
        let commission = Money::new(1.0, position.settlement_currency);
        position
            .apply_fill_void(
                matching_fill_void(&fill, Quantity::from("0.5"), Some(commission)),
                Quantity::from("0.5"),
                Some(commission),
            )
            .unwrap();
        let mut later_payment = funding_payment(&position, -5.0);
        later_payment.ts_event = UnixNanos::from(7);
        position.apply_adjustment(later_payment).unwrap();
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(error.to_string().contains("predates adjustment"), "{error}");
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    fn test_revert_last_adjustment_rejects_snapshot_with_pnl_at_money_bounds() {
        let mut position = btcusdt_position();
        let mut prior = position.clone_without_events();
        prior.realized_pnl = Some(Money::from_raw(MONEY_RAW_MAX, position.settlement_currency));
        position
            .apply_adjustment(funding_payment(&position, 10.0))
            .unwrap();
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(
            error.to_string().contains("cannot take the PnL change"),
            "{error}"
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    #[case::size_precision("prior.size_precision")]
    #[case::settlement_currency("prior.settlement_currency")]
    fn test_revert_last_adjustment_rejects_snapshot_of_other_instrument_config(
        #[case] field: &str,
    ) {
        let mut position = btcusdt_position();
        let mut prior = position.clone_without_events();
        match field {
            "prior.size_precision" => prior.size_precision += 1,
            _ => prior.settlement_currency = Currency::USD(),
        }

        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(error.to_string().contains(field), "{error}");
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    fn test_revert_last_adjustment_rejects_snapshot_with_other_pnl_currency() {
        let mut position = btcusdt_position();
        let mut prior = position.clone_without_events();
        prior.realized_pnl = Some(Money::new(1.0, Currency::USD()));
        position
            .apply_adjustment(funding_payment(&position, -10.0))
            .unwrap();
        let after = serde_json::to_value(&position).unwrap();

        let error = position.revert_last_adjustment(&prior).unwrap_err();

        assert!(
            error.to_string().contains("cannot take the PnL change"),
            "{error}"
        );
        assert_eq!(serde_json::to_value(&position).unwrap(), after);
    }

    #[rstest]
    fn test_purge_events_preserves_manual_adjustments() {
        // Test that manual adjustments (e.g., funding payments) are preserved when purging unrelated fills
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        // First fill
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .client_order_id(ClientOrderId::new("O-001"))
            .build();

        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&btc_usdt, fill1.into());
        assert_eq!(position.adjustments.len(), 1);

        // Apply a manual funding payment adjustment (no reason field)
        let funding_adjustment = PositionAdjusted::new(
            position.trader_id,
            position.strategy_id,
            position.instrument_id,
            position.id,
            position.account_id,
            PositionAdjustmentType::Funding,
            None,
            Some(Money::new(10.0, btc_usdt.quote_currency())),
            None, // No reason - this is a manual adjustment
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        position.apply_adjustment(funding_adjustment).unwrap();
        assert_eq!(position.adjustments.len(), 2);

        // Second fill with different order
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.0"))
            .client_order_id(ClientOrderId::new("O-002"))
            .build();

        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &btc_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("51000.0")),
            Some(Quantity::from("2.0")),
            None,
            Some(Money::new(0.002, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        position.apply(&fill2.into());
        assert_eq!(
            position.adjustments.len(),
            3,
            "Should have 3 adjustments: 2 commissions + 1 funding"
        );

        // Purge first order - manual funding adjustment should be preserved
        position
            .purge_events_for_order(order1.client_order_id())
            .unwrap();

        assert_eq!(position.events.len(), 1, "Should have 1 remaining event");
        assert_eq!(
            position.adjustments.len(),
            2,
            "Should have funding adjustment + commission from remaining fill"
        );

        // Verify funding adjustment is preserved
        let has_funding = position.adjustments.iter().any(|adj| {
            adj.adjustment_type == PositionAdjustmentType::Funding
                && adj.pnl_change == Some(Money::new(10.0, btc_usdt.quote_currency()))
        });
        assert!(has_funding, "Funding adjustment should be preserved");

        // Verify realized_pnl includes the funding payment
        // Note: Commission is in BTC (base currency), so it doesn't directly affect USDT realized_pnl
        assert_eq!(
            position.realized_pnl,
            Some(Money::new(10.0, btc_usdt.quote_currency())),
            "Realized PnL should be the funding payment only (commission is in BTC, not USDT)"
        );
    }

    #[rstest]
    fn test_position_commission_affects_buy_and_sell_qty() {
        // Test that commission in base currency affects both buy_qty and sell_qty tracking
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let buy_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        // Buy 1.0 BTC with 0.001 BTC commission
        let fill = TestOrderEventStubs::filled(
            &buy_order,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let position = Position::new(&btc_usdt, fill.into());

        // buy_qty tracks order fills (1.0 BTC), adjustments tracked separately
        assert!(
            (position.buy_qty.as_f64() - 1.0).abs() < 1e-9,
            "buy_qty should be 1.0 (order fill amount), was {}",
            position.buy_qty.as_f64()
        );

        // Position quantity reflects both order fill and commission adjustment
        assert!(
            (position.quantity.as_f64() - 0.999).abs() < 1e-9,
            "position.quantity should be 0.999 (1.0 - 0.001 commission), was {}",
            position.quantity.as_f64()
        );

        // Adjustment event tracks the commission
        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(
            position.adjustments[0].quantity_change,
            Some(rust_decimal_macros::dec!(-0.001))
        );
    }

    #[rstest]
    fn test_position_perpetual_commission_no_adjustment() {
        // Test that perpetuals/futures do NOT adjust quantity for base currency commission
        let eth_perp = crypto_perpetual_ethusdt();
        let eth_perp = InstrumentAny::CryptoPerpetual(eth_perp);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_perp.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        // Buy 1.0 ETH-PERP contracts with 0.001 ETH commission
        let fill = TestOrderEventStubs::filled(
            &order,
            &eth_perp,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("3000.0")),
            Some(Quantity::from("1.0")),
            None,
            Some(Money::new(0.001, eth_perp.base_currency().unwrap())),
            None,
            None,
        );

        let position = Position::new(&eth_perp, fill.into());

        // Position quantity should be exactly 1.0 (NO adjustment for derivatives)
        assert!(
            (position.quantity.as_f64() - 1.0).abs() < 1e-9,
            "Perpetual position should be 1.0 contracts (no adjustment), was {}",
            position.quantity.as_f64()
        );

        // Signed qty should also be 1.0
        assert!(
            (position.signed_qty - dec!(1.0)).abs() < dec!(1e-9),
            "Signed qty should be 1.0, was {}",
            position.signed_qty
        );
    }

    #[rstest]
    fn test_signed_decimal_qty_long(stub_position_long: Position) {
        let signed_qty = stub_position_long.signed_decimal_qty();
        assert!(signed_qty > Decimal::ZERO);
        assert_eq!(signed_qty, stub_position_long.signed_qty);
    }

    #[rstest]
    fn test_signed_decimal_qty_short(stub_position_short: Position) {
        let signed_qty = stub_position_short.signed_decimal_qty();
        assert!(signed_qty < Decimal::ZERO);
        assert_eq!(signed_qty, stub_position_short.signed_qty);
    }

    #[rstest]
    fn test_signed_decimal_qty_flat(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&audusd_sim, fill.into());

        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();
        let close_fill = TestOrderEventStubs::filled(
            &close_order,
            &audusd_sim,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("1.00002")),
            None,
            None,
            None,
            None,
            None,
        );
        position.apply(&close_fill.into());

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.signed_decimal_qty(), Decimal::ZERO);
    }

    #[rstest]
    fn test_position_flat_with_floating_point_precision_edge_case() {
        // This test verifies that when signed_qty has accumulated floating-point
        // errors (tiny non-zero value) but quantity rounds to zero, the position
        // correctly becomes FLAT with signed_qty normalized to 0.0
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.123456789"))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.00")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&btc_usdt, fill1.into());

        assert_eq!(position.side, PositionSide::Long);
        assert!(position.quantity.is_positive());

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.123456789"))
            .build();
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &btc_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("50000.00")),
            None,
            None,
            None,
            None,
            None,
        );
        position.apply(&fill2.into());

        assert_eq!(
            position.side,
            PositionSide::Flat,
            "Position should be FLAT, not {:?}",
            position.side
        );
        assert!(
            position.quantity.is_zero(),
            "Quantity should be zero, was {}",
            position.quantity
        );
        assert_eq!(
            position.signed_qty,
            dec!(0.0),
            "signed_qty should be normalized to 0.0, was {}",
            position.signed_qty
        );
        assert!(position.is_closed());
    }

    #[rstest]
    #[case(OrderSide::Buy, OrderSide::Sell, "162.50", "176.50", 171.5)]
    #[case(OrderSide::Sell, OrderSide::Buy, "140.00", "126.00", 131.0)]
    fn test_position_exact_close_after_partial_fills_preserves_open_average(
        #[case] entry: OrderSide,
        #[case] exit: OrderSide,
        #[case] first_close_px: &str,
        #[case] final_close_px: &str,
        #[case] expected_avg_close: f64,
    ) {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let position_id = PositionId::new("P-PARTIAL-CLOSE");
        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new("O-OPEN"))
            .side(entry)
            .quantity(Quantity::from("0.7"))
            .build();
        let open_fill = TestOrderEventStubs::filled(
            &open_order,
            &instrument,
            Some(TradeId::new("T-OPEN")),
            Some(position_id),
            Some(Price::from("151.25")),
            None,
            None,
            Some(Money::from("0 USDT")),
            Some(UnixNanos::from(1_000)),
            None,
        );
        let mut position = Position::new(&instrument, open_fill.into());

        for (client_order_id, trade_id, quantity, price, ts_event) in [
            ("O-CLOSE-1", "T-CLOSE-1", "0.25", first_close_px, 1_100),
            ("O-CLOSE-2", "T-CLOSE-2", "0.45", final_close_px, 1_250),
        ] {
            let close_order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::new(client_order_id))
                .side(exit)
                .quantity(Quantity::from(quantity))
                .build();
            let close_fill = TestOrderEventStubs::filled(
                &close_order,
                &instrument,
                Some(TradeId::new(trade_id)),
                Some(position_id),
                Some(Price::from(price)),
                None,
                None,
                Some(Money::from("0 USDT")),
                Some(UnixNanos::from(ts_event)),
                None,
            );
            position.apply(&close_fill.into());
        }

        assert_eq!(position.entry, entry);
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.signed_qty, dec!(0.0));
        assert_eq!(position.quantity, Quantity::zero(6));
        assert_eq!(position.peak_qty, Quantity::from("0.7"));
        assert_eq!(position.buy_qty, Quantity::from("0.7"));
        assert_eq!(position.sell_qty, Quantity::from("0.7"));
        assert_eq!(position.avg_px_open, dec!(151.25));
        assert_eq!(
            position.avg_px_close,
            Some(Decimal::try_from(expected_avg_close).unwrap())
        );
        assert_eq!(position.realized_return, 0.133_884_297_520_661_17);
        assert_eq!(position.realized_pnl, Some(Money::from("14.17500000 USDT")));
        assert_eq!(position.commissions(), vec![Money::from("0 USDT")]);
        assert_eq!(position.opening_order_id, ClientOrderId::new("O-OPEN"));
        assert_eq!(
            position.closing_order_id,
            Some(ClientOrderId::new("O-CLOSE-2"))
        );
        assert_eq!(position.ts_opened, UnixNanos::from(1_000));
        assert_eq!(position.ts_last, UnixNanos::from(1_250));
        assert_eq!(position.ts_closed, Some(UnixNanos::from(1_250)));
        assert_eq!(position.duration_ns, DurationNanos::new(250));
        assert_eq!(position.event_count(), 3);
        assert!(position.is_closed());
    }

    #[rstest]
    #[case(
        OrderSide::Buy,
        OrderSide::Sell,
        "140.00",
        "126.00",
        PositionSide::Short,
        -0.000_001
    )]
    #[case(
        OrderSide::Sell,
        OrderSide::Buy,
        "162.50",
        "176.50",
        PositionSide::Long,
        0.000_001
    )]
    fn test_position_true_reversal_uses_fill_price(
        #[case] entry: OrderSide,
        #[case] exit: OrderSide,
        #[case] first_close_px: &str,
        #[case] reversal_px: &str,
        #[case] expected_side: PositionSide,
        #[case] expected_signed_qty: f64,
    ) {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let position_id = PositionId::new("P-REVERSAL");
        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new("O-REVERSAL-OPEN"))
            .side(entry)
            .quantity(Quantity::from("0.7"))
            .build();
        let open_fill = TestOrderEventStubs::filled(
            &open_order,
            &instrument,
            Some(TradeId::new("T-REVERSAL-OPEN")),
            Some(position_id),
            Some(Price::from("151.25")),
            None,
            None,
            Some(Money::from("0 USDT")),
            Some(UnixNanos::from(2_000)),
            None,
        );
        let mut position = Position::new(&instrument, open_fill.into());

        let close_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new("O-REVERSAL-CLOSE"))
            .side(exit)
            .quantity(Quantity::from("0.25"))
            .build();
        let close_fill = TestOrderEventStubs::filled(
            &close_order,
            &instrument,
            Some(TradeId::new("T-REVERSAL-CLOSE")),
            Some(position_id),
            Some(Price::from(first_close_px)),
            None,
            None,
            Some(Money::from("0 USDT")),
            Some(UnixNanos::from(2_050)),
            None,
        );
        position.apply(&close_fill.into());

        let reversal_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new("O-REVERSAL"))
            .side(exit)
            .quantity(Quantity::from("0.450001"))
            .build();
        let reversal_fill = TestOrderEventStubs::filled(
            &reversal_order,
            &instrument,
            Some(TradeId::new("T-REVERSAL")),
            Some(position_id),
            Some(Price::from(reversal_px)),
            None,
            None,
            Some(Money::from("0 USDT")),
            Some(UnixNanos::from(2_100)),
            None,
        );
        position.apply(&reversal_fill.into());

        assert_eq!(position.entry, exit);
        assert_eq!(position.side, expected_side);
        assert_eq!(
            position.signed_qty,
            Decimal::try_from(expected_signed_qty).unwrap()
        );
        assert_eq!(position.quantity, Quantity::from("0.000001"));
        assert_eq!(position.peak_qty, Quantity::from("0.7"));
        assert_eq!(position.avg_px_open, Price::from(reversal_px).as_decimal());
        assert_eq!(
            position.realized_pnl,
            Some(Money::from("-14.17500000 USDT"))
        );
        assert_eq!(position.commissions(), vec![Money::from("0 USDT")]);
        assert_eq!(
            position.opening_order_id,
            ClientOrderId::new("O-REVERSAL-OPEN")
        );
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.ts_opened, UnixNanos::from(2_000));
        assert_eq!(position.ts_last, UnixNanos::from(2_100));
        assert_eq!(position.ts_closed, None);
        assert_eq!(position.duration_ns, DurationNanos::default());
        assert_eq!(position.event_count(), 3);
        assert!(position.is_open());
    }

    #[rstest]
    #[case(OrderSide::Buy, OrderSide::Sell)]
    #[case(OrderSide::Sell, OrderSide::Buy)]
    fn test_position_reversal_starts_new_close_episode(
        #[case] entry: OrderSide,
        #[case] exit: OrderSide,
    ) {
        let (
            open_px,
            first_close_px,
            reversal_px,
            expected_side,
            new_first_close_px,
            new_final_close_px,
            expected_avg_close,
            expected_return,
        ) = if entry == OrderSide::Buy {
            (
                "100.00",
                "110.00",
                "120.00",
                PositionSide::Short,
                "110.00",
                "90.00",
                95.0,
                0.208_333_333_333_333_34,
            )
        } else {
            (
                "120.00",
                "110.00",
                "100.00",
                PositionSide::Long,
                "110.00",
                "130.00",
                125.0,
                0.25,
            )
        };
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let position_id = PositionId::new("P-REVERSAL-EPISODE");
        let open_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new("O-OPEN"))
            .side(entry)
            .quantity(Quantity::from("10"))
            .build();
        let open_fill = TestOrderEventStubs::filled(
            &open_order,
            &instrument,
            Some(TradeId::new("T-OPEN")),
            Some(position_id),
            Some(Price::from(open_px)),
            None,
            None,
            Some(Money::from("1 USDT")),
            Some(UnixNanos::from(3_000)),
            None,
        );
        let mut position = Position::new(&instrument, open_fill.into());

        for (client_order_id, trade_id, quantity, price, ts_event) in [
            ("O-CLOSE", "T-CLOSE", "4", first_close_px, 3_100),
            ("O-REVERSE", "T-REVERSE", "8", reversal_px, 3_200),
        ] {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::new(client_order_id))
                .side(exit)
                .quantity(Quantity::from(quantity))
                .build();
            let fill = TestOrderEventStubs::filled(
                &order,
                &instrument,
                Some(TradeId::new(trade_id)),
                Some(position_id),
                Some(Price::from(price)),
                None,
                None,
                Some(Money::from("1 USDT")),
                Some(UnixNanos::from(ts_event)),
                None,
            );
            position.apply(&fill.into());
        }

        assert_eq!(position.side, expected_side);
        assert_eq!(position.quantity, Quantity::from("2"));
        assert_eq!(position.avg_px_open, Price::from(reversal_px).as_decimal());
        assert_eq!(position.avg_px_close, None);
        assert_eq!(position.realized_return, 0.0);
        if expected_side == PositionSide::Long {
            assert_eq!(position.buy_qty, Quantity::from("2"));
            assert_eq!(position.sell_qty, Quantity::from("0"));
        } else {
            assert_eq!(position.buy_qty, Quantity::from("0"));
            assert_eq!(position.sell_qty, Quantity::from("2"));
        }
        assert_eq!(position.realized_pnl, Some(Money::from("157 USDT")));

        for (client_order_id, trade_id, quantity, price, ts_event) in [
            (
                "O-NEW-CLOSE-1",
                "T-NEW-CLOSE-1",
                "0.5",
                new_first_close_px,
                3_300,
            ),
            (
                "O-NEW-CLOSE-2",
                "T-NEW-CLOSE-2",
                "1.5",
                new_final_close_px,
                3_400,
            ),
        ] {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::new(client_order_id))
                .side(entry)
                .quantity(Quantity::from(quantity))
                .build();
            let fill = TestOrderEventStubs::filled(
                &order,
                &instrument,
                Some(TradeId::new(trade_id)),
                Some(position_id),
                Some(Price::from(price)),
                None,
                None,
                Some(Money::from("1 USDT")),
                Some(UnixNanos::from(ts_event)),
                None,
            );
            position.apply(&fill.into());
        }

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.quantity, Quantity::zero(6));
        assert_eq!(position.buy_qty, Quantity::from("2"));
        assert_eq!(position.sell_qty, Quantity::from("2"));
        assert_eq!(
            position.avg_px_close,
            Some(Decimal::try_from(expected_avg_close).unwrap())
        );
        assert_eq!(position.realized_return, expected_return);
        assert_eq!(position.realized_pnl, Some(Money::from("205 USDT")));
        assert_eq!(position.commissions(), vec![Money::from("5 USDT")]);
    }

    #[rstest]
    fn test_position_adjustment_floating_point_precision_edge_case() {
        // Test that apply_adjustment handles precision edge cases correctly
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.00")),
            None,
            None,
            None,
            None,
            None,
        );
        let mut position = Position::new(&btc_usdt, fill.into());

        let adjustment = PositionAdjusted::new(
            position.trader_id,
            position.strategy_id,
            position.instrument_id,
            position.id,
            position.account_id,
            PositionAdjustmentType::Commission,
            Some(Decimal::from_str("-1.0").unwrap()),
            None,
            None,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        position.apply_adjustment(adjustment).unwrap();

        assert_eq!(
            position.side,
            PositionSide::Flat,
            "Position should be FLAT after zeroing adjustment"
        );
        assert!(
            position.quantity.is_zero(),
            "Quantity should be zero after adjustment"
        );
        assert_eq!(
            position.signed_qty,
            dec!(0.0),
            "signed_qty should be normalized to 0.0"
        );
    }

    #[rstest]
    fn test_position_spot_buy_partial_fills_with_base_commission() {
        // Reproduce GitHub issue #3546: partial fills with base currency commission
        // should reduce position quantity, not increase it
        let eth_usdt = currency_pair_ethusdt();
        let eth_usdt = InstrumentAny::CurrencyPair(eth_usdt);

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.00350"))
            .build();

        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &eth_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("2042.69")),
            Some(Quantity::from("0.00350")),
            None,
            Some(Money::new(0.00001, eth_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&eth_usdt, fill1.into());

        assert_eq!(position.quantity, Quantity::from("0.00349"));
        assert_eq!(position.signed_qty, dec!(0.00349));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.adjustments.len(), 1);
        assert_eq!(
            position.adjustments[0].quantity_change,
            Some(rust_decimal_macros::dec!(-0.00001))
        );

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.00350"))
            .build();

        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &eth_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("2042.69")),
            Some(Quantity::from("0.00350")),
            None,
            Some(Money::new(0.00001, eth_usdt.base_currency().unwrap())),
            None,
            None,
        );

        position.apply(&fill2.into());

        assert_eq!(position.quantity, Quantity::from("0.00698"));
        assert_eq!(position.signed_qty, dec!(0.00698));
        assert_eq!(position.adjustments.len(), 2);

        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.00300"))
            .build();

        let fill3 = TestOrderEventStubs::filled(
            &order3,
            &eth_usdt,
            Some(TradeId::new("3")),
            None,
            Some(Price::from("2042.69")),
            Some(Quantity::from("0.00300")),
            None,
            Some(Money::new(0.00001, eth_usdt.base_currency().unwrap())),
            None,
            None,
        );

        position.apply(&fill3.into());

        // Total filled: 0.01000, total commission: 0.00003
        // Position should be 0.01000 - 0.00003 = 0.00997
        assert_eq!(position.quantity, Quantity::from("0.00997"));
        assert_eq!(position.signed_qty, dec!(0.00997));
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.adjustments.len(), 3);

        // buy_qty tracks order fill amounts, not commission-adjusted
        assert_eq!(position.buy_qty, Quantity::from("0.01000"));
    }

    #[rstest]
    fn test_position_spot_sell_partial_fills_with_base_commission() {
        let btc_usdt = currency_pair_btcusdt();
        let btc_usdt = InstrumentAny::CurrencyPair(btc_usdt);

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.5"))
            .build();

        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &btc_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("0.5")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&btc_usdt, fill1.into());

        // Short: sold 0.5 + paid 0.001 commission = -0.501 exposure
        assert!((position.signed_qty - (dec!(-0.501))).abs() < dec!(1e-9));
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.adjustments.len(), 1);

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(btc_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.5"))
            .build();

        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &btc_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("50000.0")),
            Some(Quantity::from("0.5")),
            None,
            Some(Money::new(0.001, btc_usdt.base_currency().unwrap())),
            None,
            None,
        );

        position.apply(&fill2.into());

        // Total short: 1.0 sold + 0.002 commission = -1.002
        assert!((position.signed_qty - (dec!(-1.002))).abs() < dec!(1e-9));
        assert!((position.quantity.as_f64() - 1.002).abs() < 1e-9);
        assert_eq!(position.adjustments.len(), 2);
        assert_eq!(position.sell_qty, Quantity::from("1.0"));
    }

    #[rstest]
    fn test_position_spot_round_trip_close_flat_with_quote_commission() {
        let eth_usdt = currency_pair_ethusdt();
        let eth_usdt = InstrumentAny::CurrencyPair(eth_usdt);

        let buy_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.00000"))
            .build();

        let buy_fill = TestOrderEventStubs::filled(
            &buy_order,
            &eth_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("2000.00")),
            Some(Quantity::from("1.00000")),
            None,
            Some(Money::new(0.001, eth_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&eth_usdt, buy_fill.into());

        // Position = 1.0 - 0.001 = 0.999
        assert_eq!(position.quantity, Quantity::from("0.99900"));
        assert_eq!(position.side, PositionSide::Long);

        let sell_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.99900"))
            .build();

        let sell_fill = TestOrderEventStubs::filled(
            &sell_order,
            &eth_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("2100.00")),
            Some(Quantity::from("0.99900")),
            None,
            Some(Money::new(2.0, Currency::USDT())),
            None,
            None,
        );

        position.apply(&sell_fill.into());

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.signed_qty, dec!(0.0));
        assert!(position.is_closed());
        // Only 1 adjustment from the buy (quote commission doesn't create adjustment)
        assert_eq!(position.adjustments.len(), 1);

        // PnL: 0.999 ETH * $100 price move = $99.90, minus $2 commission
        let realized = position.realized_pnl.unwrap().as_f64();
        assert!(
            (realized - 97.9).abs() < 0.01,
            "Realized PnL should be ~97.90 USDT, was {realized}"
        );
    }

    #[rstest]
    fn test_position_spot_commission_accumulation_multiple_partial_fills() {
        let eth_usdt = currency_pair_ethusdt();
        let eth_usdt = InstrumentAny::CurrencyPair(eth_usdt);

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.50000"))
            .build();

        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &eth_usdt,
            Some(TradeId::new("1")),
            None,
            Some(Price::from("2000.00")),
            Some(Quantity::from("0.50000")),
            None,
            Some(Money::new(0.0005, eth_usdt.base_currency().unwrap())),
            None,
            None,
        );

        let mut position = Position::new(&eth_usdt, fill1.into());

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.50000"))
            .build();

        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &eth_usdt,
            Some(TradeId::new("2")),
            None,
            Some(Price::from("2010.00")),
            Some(Quantity::from("0.50000")),
            None,
            Some(Money::new(0.0005, eth_usdt.base_currency().unwrap())),
            None,
            None,
        );

        position.apply(&fill2.into());

        // Total: 1.0 filled, 0.001 total commission
        assert_eq!(position.quantity, Quantity::from("0.99900"));
        assert_eq!(position.buy_qty, Quantity::from("1.00000"));

        assert_eq!(position.adjustments.len(), 2);
        for adj in &position.adjustments {
            assert_eq!(adj.adjustment_type, PositionAdjustmentType::Commission);
            assert_eq!(
                adj.quantity_change,
                Some(rust_decimal_macros::dec!(-0.0005))
            );
        }

        let commissions = position.commissions();
        assert_eq!(commissions.len(), 1);
        let eth_commission = commissions[0];
        assert!(
            (eth_commission.as_f64() - 0.001).abs() < 1e-9,
            "Total ETH commission should be 0.001, was {}",
            eth_commission.as_f64()
        );
    }

    #[rstest]
    fn test_position_apply_fill_with_earlier_timestamp_adjusts_ts_opened(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        // First fill at ts=2000
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &audusd_sim,
            Some(TradeId::new("t1")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            Some(UnixNanos::from(2_000u64)),
            None,
        );
        let mut position = Position::new(&audusd_sim, fill1.into());
        assert_eq!(position.ts_opened, UnixNanos::from(2_000u64));

        // Second fill at ts=1000 (earlier than position open)
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &audusd_sim,
            Some(TradeId::new("t2")),
            None,
            Some(Price::from("1.00002")),
            None,
            None,
            None,
            Some(UnixNanos::from(1_000u64)),
            None,
        );

        // Should not panic; ts_opened and opening_order_id stay unchanged
        position.apply(&fill2.into());
        assert_eq!(position.ts_opened, UnixNanos::from(2_000u64));
        assert_eq!(position.opening_order_id, order1.client_order_id());
        assert_eq!(position.events.len(), 2);
    }

    #[rstest]
    fn test_position_close_before_open_clamps_duration(audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let opening_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let closing_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(100_000))
            .build();
        let opening_fill = TestOrderEventStubs::filled(
            &opening_order,
            &audusd_sim,
            Some(TradeId::new("OPEN")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            None,
            Some(UnixNanos::from(2_000u64)),
            None,
        );
        let closing_fill = TestOrderEventStubs::filled(
            &closing_order,
            &audusd_sim,
            Some(TradeId::new("CLOSE")),
            None,
            Some(Price::from("1.00002")),
            None,
            None,
            None,
            Some(UnixNanos::from(1_000u64)),
            None,
        );
        let mut position = Position::new(&audusd_sim, opening_fill.into());

        position.apply(&closing_fill.into());

        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.ts_opened, UnixNanos::from(2_000u64));
        assert_eq!(position.ts_closed, Some(UnixNanos::from(1_000u64)));
        assert_eq!(position.duration_ns, DurationNanos::default());
        assert_eq!(
            position.closing_order_id,
            Some(closing_order.client_order_id())
        );
    }

    #[rstest]
    fn test_position_commissions_multi_currency_insertion_order(audusd_sim: CurrencyPair) {
        // Locks in IndexMap iteration order for Position::commissions:
        // new currencies append to the end, existing currencies accumulate
        // in place. PositionSnapshot.commissions builds its Vec from this
        // iteration; the order must be deterministic across runs.
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order_template = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();

        let fill_usd = TestOrderEventStubs::filled(
            &order_template,
            &audusd_sim,
            Some(TradeId::new("t1")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            Some(Money::from("1.0 USD")),
            None,
            None,
        );
        let mut position = Position::new(&audusd_sim, fill_usd.into());

        let fill_usdt = TestOrderEventStubs::filled(
            &order_template,
            &audusd_sim,
            Some(TradeId::new("t2")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            Some(Money::from("2.0 USDT")),
            None,
            None,
        );
        position.apply(&fill_usdt.into());

        let fill_usd_again = TestOrderEventStubs::filled(
            &order_template,
            &audusd_sim,
            Some(TradeId::new("t3")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            Some(Money::from("0.5 USD")),
            None,
            None,
        );
        position.apply(&fill_usd_again.into());

        let fill_btc = TestOrderEventStubs::filled(
            &order_template,
            &audusd_sim,
            Some(TradeId::new("t4")),
            None,
            Some(Price::from("1.00001")),
            None,
            None,
            Some(Money::from("0.0001 BTC")),
            None,
            None,
        );
        position.apply(&fill_btc.into());

        // USD entered first and accumulates in place, USDT appends second,
        // BTC appends third
        assert_eq!(
            position.commissions(),
            vec![
                Money::from("1.5 USD"),
                Money::from("2.0 USDT"),
                Money::from("0.0001 BTC"),
            ]
        );
    }

    #[rstest]
    fn test_fold_net_position_empty() {
        let (net_qty, net_px) = fold_net_position(&[]);
        assert_eq!(net_qty, Decimal::ZERO);
        assert_eq!(net_px, Decimal::ZERO);
    }

    #[rstest]
    fn test_fold_net_position_single_long() {
        let legs = [(dec!(100), dec!(1.5), 1u64)];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(100));
        assert_eq!(net_px, dec!(1.5));
    }

    #[rstest]
    fn test_fold_net_position_single_short() {
        let legs = [(dec!(-100), dec!(1.5), 1u64)];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(-100));
        assert_eq!(net_px, dec!(1.5));
    }

    #[rstest]
    fn test_fold_net_position_same_side_weighted_average() {
        // Long 100 @ 1.00, long 200 @ 0.50 -> net 300 @ 0.6667 (rounded weighted avg).
        let legs = [(dec!(100), dec!(1.0), 1u64), (dec!(200), dec!(0.5), 2u64)];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(300));
        // (100 * 1.0 + 200 * 0.5) / 300 = 200/300 = 0.6666...
        assert_eq!(net_px, dec!(200) / dec!(300));
    }

    #[rstest]
    fn test_fold_net_position_partial_close_preserves_avg() {
        // Long 300 @ 0.80, short 100 @ 1.00 -> net long 200 @ 0.80 (short closes part of long).
        let legs = [
            (dec!(300), dec!(0.80), 1u64),
            (dec!(-100), dec!(1.00), 2u64),
        ];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(200));
        assert_eq!(net_px, dec!(0.80));
    }

    #[rstest]
    fn test_fold_net_position_full_close() {
        let legs = [(dec!(100), dec!(1.0), 1u64), (dec!(-100), dec!(2.0), 2u64)];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, Decimal::ZERO);
        assert_eq!(net_px, Decimal::ZERO);
    }

    #[rstest]
    fn test_fold_net_position_single_flip_uses_flipping_price() {
        // L100@1, S50@2 partial-closes to L50@1, S100@3 flips to S50 @ 3
        let legs = [
            (dec!(100), dec!(1.00), 1u64),
            (dec!(-50), dec!(2.00), 2u64),
            (dec!(-100), dec!(3.00), 3u64),
        ];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(-50));
        assert_eq!(net_px, dec!(3.00));
    }

    #[rstest]
    fn test_fold_net_position_double_flip() {
        // L50, S100 flips to S50@2, B100 flips to L50@3
        let legs = [
            (dec!(50), dec!(1.00), 1u64),
            (dec!(-100), dec!(2.00), 2u64),
            (dec!(100), dec!(3.00), 3u64),
        ];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(50));
        assert_eq!(net_px, dec!(3.00));
    }

    #[rstest]
    fn test_fold_net_position_zero_quantity_legs_skipped() {
        // Zero-qty legs are filtered out (closed positions have signed_qty == 0)
        let legs = [
            (dec!(100), dec!(1.0), 1u64),
            (Decimal::ZERO, dec!(99.0), 2u64),
            (dec!(50), dec!(2.0), 3u64),
        ];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(150));
        // (100 * 1.0 + 50 * 2.0) / 150 = 200/150 = 1.333
        assert_eq!(net_px, dec!(200) / dec!(150));
    }

    #[rstest]
    fn test_fold_net_position_stable_sort_preserves_input_order_for_equal_ts() {
        // Caller owns tie-breaking; this pins the input-order contract for equal-ts legs
        let leg_a = (dec!(100), dec!(1.00), 1u64);
        let leg_b = (dec!(-100), dec!(2.00), 1u64);

        let ab = [leg_a, leg_b];
        let ba = [leg_b, leg_a];

        // a-then-b: L100@1, S100@2 -> net zero
        assert_eq!(fold_net_position(&ab), (Decimal::ZERO, Decimal::ZERO));
        // b-then-a: S100@2, B100@1 -> net zero
        assert_eq!(fold_net_position(&ba), (Decimal::ZERO, Decimal::ZERO));

        // Same-ts legs that do NOT fully cancel: input order picks the surviving avg
        let leg_c = (dec!(150), dec!(1.00), 1u64);
        let leg_d = (dec!(-100), dec!(2.00), 1u64);
        let cd = [leg_c, leg_d];
        let dc = [leg_d, leg_c];
        // c-then-d: L150@1, S100@2 -> long 50 @ 1
        assert_eq!(fold_net_position(&cd), (dec!(50), dec!(1.00)));
        // d-then-c: S100@2, B150@1 -> flip to long, residual @ 1
        assert_eq!(fold_net_position(&dc), (dec!(50), dec!(1.00)));
    }

    #[rstest]
    fn test_fold_net_position_close_then_reopen() {
        // A leg after a full close opens fresh (new net, new avg)
        let legs = [
            (dec!(100), dec!(1.00), 1u64),
            (dec!(-100), dec!(1.50), 2u64),
            (dec!(50), dec!(3.00), 3u64),
        ];
        let (net_qty, net_px) = fold_net_position(&legs);
        assert_eq!(net_qty, dec!(50));
        assert_eq!(net_px, dec!(3.00));
    }

    #[rstest]
    fn test_fold_net_position_orders_by_ts_opened() {
        // Sorted vs shuffled input must fold to the same result.
        let in_order = [
            (dec!(100), dec!(1.00), 1u64),
            (dec!(-50), dec!(2.00), 2u64),
            (dec!(-100), dec!(3.00), 3u64),
        ];
        let shuffled = [
            (dec!(-100), dec!(3.00), 3u64),
            (dec!(100), dec!(1.00), 1u64),
            (dec!(-50), dec!(2.00), 2u64),
        ];
        assert_eq!(fold_net_position(&in_order), fold_net_position(&shuffled));
    }

    // Build a NETTING-mode reference Position by applying fills sorted by ts_opened
    // (the same order fold_net_position uses). Returns (signed_qty, avg_px_open) as Decimals.
    fn netting_reference(
        instrument: &InstrumentAny,
        fills: &[(OrderSide, u32, u32, u64)],
    ) -> (Decimal, Decimal) {
        let mut sorted_fills = fills.to_vec();
        sorted_fills.sort_by_key(|(_, _, _, ts)| *ts);

        let mut position: Option<Position> = None;

        for (idx, &(side, qty, px, ts)) in sorted_fills.iter().enumerate() {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .side(side)
                .quantity(Quantity::from(qty))
                .build();
            let fill = TestOrderEventStubs::filled(
                &order,
                instrument,
                Some(TradeId::new(format!("T{idx}").as_str())),
                Some(PositionId::new("P-NET")),
                Some(Price::from(px.to_string().as_str())),
                None,
                None,
                Some(Money::new(0.0, instrument.quote_currency())),
                Some(UnixNanos::from(ts)),
                None,
            );
            let event: OrderFilled = fill.into();
            if let Some(p) = position.as_mut() {
                p.apply(&event);
            } else {
                position = Some(Position::new(instrument, event));
            }
        }
        let p = position.expect("at least one fill");
        (p.signed_qty, p.avg_px_open)
    }

    // Build the HEDGING leg list (one leg per fill) as Decimal tuples.
    fn hedging_legs(fills: &[(OrderSide, u32, u32, u64)]) -> Vec<(Decimal, Decimal, u64)> {
        fills
            .iter()
            .map(|&(side, qty, px, ts)| {
                let signed = if side == OrderSide::Buy {
                    Decimal::from(qty)
                } else {
                    -Decimal::from(qty)
                };
                (signed, Decimal::from(px), ts)
            })
            .collect()
    }

    proptest! {
        // For any fill sequence that does not fully close mid-stream, fold_net_position
        // produces the same (signed_qty, avg_px_open) as applying the same fills in order
        // to a single NETTING-mode Position. Sequences that pass through zero mid-stream are
        // filtered out because Position::apply does not reset avg_px_open on the next fill
        // after a close (production fills are pre-split before reaching that path).
        #[rstest]
        fn prop_fold_matches_netting_replay(
            fills in proptest::collection::vec(
                (
                    prop_oneof![Just(OrderSide::Buy), Just(OrderSide::Sell)],
                    1u32..1_000u32,
                    1u32..100u32,
                    0u64..1_000_000u64,
                ),
                1..6,
            )
        ) {
            // Filter sequences with duplicate ts_opened: equal-ts ties resolve to stable
            // input order, but the proptest generator does not preserve ties meaningfully.
            let mut seen_ts: AHashSet<u64> = AHashSet::new();
            for &(_, _, _, ts) in &fills {
                if !seen_ts.insert(ts) {
                    prop_assume!(false);
                }
            }

            // Filter sequences that fully close mid-stream on the sorted (ts_opened) order.
            // Position::apply on a closed position does not reset avg_px_open on the next
            // re-open fill (production fills are pre-split, so the path is not exercised).
            let mut sorted_fills = fills.clone();
            sorted_fills.sort_by_key(|(_, _, _, ts)| *ts);
            let mut running: i64 = 0;
            let mut zero_mid = false;

            for (idx, &(side, qty, _, _)) in sorted_fills.iter().enumerate() {
                let qty_i64 = i64::from(qty);
                let signed: i64 = if side == OrderSide::Buy {
                    qty_i64
                } else {
                    -qty_i64
                };
                running += signed;
                if idx + 1 < sorted_fills.len() && running == 0 {
                    zero_mid = true;
                    break;
                }
            }
            prop_assume!(!zero_mid);

            let instrument = InstrumentAny::CurrencyPair(audusd_sim());
            let (ref_qty, ref_px) = netting_reference(&instrument, &fills);
            let legs = hedging_legs(&fills);
            let (fold_qty, fold_px) = fold_net_position(&legs);

            prop_assert_eq!(fold_qty, ref_qty);

            // avg_px_open is only meaningful when the position is non-flat. Compare via
            // f64 round-trip since the reference goes through Position::apply f64 arithmetic;
            // fold's full-precision Decimal is more accurate but not exactly equal.
            if !ref_qty.is_zero() {
                let fold_px_f64 = fold_px.to_f64().unwrap_or(0.0);
                let ref_px_f64 = ref_px.to_f64().unwrap_or(0.0);
                let max_mag = fold_px_f64.abs().max(ref_px_f64.abs()).max(1.0);
                prop_assert!(
                    (fold_px_f64 - ref_px_f64).abs() < 1e-9 * max_mag,
                    "fold_px {fold_px_f64} vs ref_px {ref_px_f64}",
                );
            }
        }
    }

    fn matching_fill_void(
        fill: &OrderFilled,
        voided_qty: Quantity,
        commission_voided: Option<Money>,
    ) -> OrderFillVoided {
        OrderFillVoidedSpec::builder()
            .trader_id(fill.trader_id)
            .strategy_id(fill.strategy_id)
            .instrument_id(fill.instrument_id)
            .client_order_id(fill.client_order_id)
            .venue_order_id(fill.venue_order_id)
            .account_id(fill.account_id)
            .trade_id(fill.trade_id)
            .voided_qty(voided_qty)
            .order_side(fill.order_side)
            .order_type(fill.order_type)
            .last_px(fill.last_px)
            .currency(fill.currency)
            .liquidity_side(fill.liquidity_side)
            .maybe_position_id(fill.position_id)
            .maybe_commission_voided(commission_voided)
            .build()
    }

    #[rstest]
    #[case(0, 56, Some((0, 0)))]
    #[case(123_450_000, 7, Some((12345, 3)))]
    #[case(-123_450_000, 7, Some((-12345, 3)))]
    #[case(Decimal::MAX.mantissa() * 10, 1, Some((Decimal::MAX.mantissa(), 0)))]
    #[case(Decimal::MAX.mantissa() * 10 + 1, 1, None)]
    #[case(10, 29, Some((1, 28)))]
    #[case(1, 29, None)]
    fn test_raw_conversion_preserves_exact_bounds(
        #[case] raw: i128,
        #[case] scale: u32,
        #[case] expected: Option<(i128, u32)>,
    ) {
        let actual = from_raw(raw, scale);

        match expected {
            Some(expected) => {
                let actual = actual.unwrap();
                assert_eq!((actual.mantissa(), actual.scale()), expected);
            }
            None => assert!(actual.is_err()),
        }
    }

    proptest! {
        #[rstest]
        fn prop_raw_conversion_matches_exact_normalization(
            mut raw in prop_oneof![any::<i128>(), -Decimal::MAX.mantissa()..=Decimal::MAX.mantissa()],
            mut scale in 0_u32..=56,
        ) {
            let actual = from_raw(raw, scale);
            while scale > 0 && raw % 10 == 0 {
                raw /= 10;
                scale -= 1;
            }
            let expected = Decimal::try_from_i128_with_scale(raw, scale);

            match expected {
                Ok(expected) => {
                    let actual = actual.unwrap();
                    prop_assert_eq!((actual.mantissa(), actual.scale()), (expected.mantissa(), expected.scale()));
                }
                Err(_) => prop_assert!(actual.is_err()),
            }
        }

        #[rstest]
        fn prop_domain_fee_split_matches_exact_ratio(
            raw in -MoneyRaw::from(1_000_000_i32)..=MoneyRaw::from(1_000_000_i32),
            closing in QuantityRaw::from(1_u32)..=QuantityRaw::from(1_000_000_000_u32),
            remainder in QuantityRaw::from(1_u32)..=QuantityRaw::from(1_000_000_000_u32),
        ) {
            let fee = Money::from_raw(raw, Currency::USD());
            let closing = Quantity::from_raw(closing, FIXED_PRECISION);
            let total = Quantity::from_raw(closing.raw + remainder, FIXED_PRECISION);
            let expected = split_commission(fee, qty(closing).unwrap(), qty(total).unwrap()).unwrap();
            let actual = split_commission_qty(fee, closing, total).unwrap();

            prop_assert_eq!(actual, expected);
            prop_assert_eq!(actual.0.checked_add(actual.1), Some(fee));
        }
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    fn test_domain_subtraction_rejects_unsupported_input_before_cancellation() {
        let raw = Decimal::MAX.mantissa() as u128 + 1;
        let value = Quantity::from_raw(raw, FIXED_PRECISION);

        assert!(quantity_sub(value, value, FIXED_PRECISION).is_err());
        assert!(money_from_qty(value, Currency::USD()).is_err());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_domain_operations_preserve_mixed_raw_scales() {
        let lhs = Quantity::from("1.000000000000000001");
        let rhs = Quantity::from("0.5000000000000000");
        let sum = quantity_add(lhs, rhs, 16).unwrap();
        let difference = quantity_sub(lhs, rhs, 16).unwrap();
        let fee = Money::from_raw(-7, Currency::USD());
        let split = split_commission_qty(fee, rhs, lhs).unwrap();
        let money = money_from_qty(lhs, Currency::USD()).unwrap();

        assert_eq!((sum.raw, sum.precision), (1_500_000_000_000_000_001, 18));
        assert_eq!(
            (difference.raw, difference.precision),
            (500_000_000_000_000_001, 18)
        );
        assert_eq!(
            split,
            (
                Money::from_raw(-3, Currency::USD()),
                Money::from_raw(-4, Currency::USD())
            )
        );
        assert_eq!(money, Money::from("1 USD"));
    }

    #[rstest]
    #[case("1.2300", "0.230", 2, "1.46", "1.00")]
    #[case("1.23", "0.004", 2, "1.234", "1.226")]
    #[case("2", "2", 3, "4.000", "0.000")]
    fn test_quantity_operations_preserve_value_and_precision(
        #[case] lhs: &str,
        #[case] rhs: &str,
        #[case] precision: u8,
        #[case] sum: &str,
        #[case] difference: &str,
    ) {
        let lhs = Quantity::from(lhs);
        let rhs = Quantity::from(rhs);
        let sum = Quantity::from(sum);
        let difference = Quantity::from(difference);
        let actual_sum = quantity_add(lhs, rhs, precision).unwrap();
        let actual_difference = quantity_sub(lhs, rhs, precision).unwrap();

        assert_eq!(
            (actual_sum.raw, actual_sum.precision),
            (sum.raw, sum.precision)
        );
        assert_eq!(
            (actual_difference.raw, actual_difference.precision),
            (difference.raw, difference.precision),
        );
    }

    #[rstest]
    fn test_domain_operations_preserve_fractional_raw_units() {
        let value = Quantity::from_raw(1, 2);
        let zero = Quantity::zero(2);
        let sum = quantity_add(value, zero, 2).unwrap();
        let difference = quantity_sub(value, zero, 2).unwrap();
        let amount = money_from_qty(value, Currency::USD()).unwrap();

        assert_eq!((sum.raw, sum.precision), (1, FIXED_PRECISION));
        assert_eq!((difference.raw, difference.precision), (1, FIXED_PRECISION));
        assert_eq!(amount, Money::from_raw(1, Currency::USD()));
    }

    #[rstest]
    #[case(5, 1, 4)]
    #[case(-5, -1, -4)]
    #[case(0, 0, 0)]
    fn test_domain_fee_split_conserves_raw_units(
        #[case] raw: MoneyRaw,
        #[case] closing: MoneyRaw,
        #[case] opening: MoneyRaw,
    ) {
        let fee = Money::from_raw(raw, Currency::USD());
        let actual = split_commission_qty(fee, Quantity::from("1"), Quantity::from("3")).unwrap();

        assert_eq!(
            actual,
            (
                Money::from_raw(closing, Currency::USD()),
                Money::from_raw(opening, Currency::USD()),
            ),
        );
        assert_eq!(actual.0.checked_add(actual.1), Some(fee));
    }

    #[rstest]
    fn test_domain_fee_split_preserves_ratio_after_intermediate_overflow() {
        let fee = Money::from("1000000 USD");
        let closing_qty = Quantity::from("1000000");
        let total_qty = Quantity::from("4000000");
        let actual = split_commission_qty(fee, closing_qty, total_qty).unwrap();

        assert_eq!(
            actual,
            (Money::from("250000 USD"), Money::from("750000 USD"))
        );
        assert_eq!(actual.0.checked_add(actual.1), Some(fee));
    }

    #[rstest]
    fn test_quantity_operations_reject_overflow_and_underflow() {
        let max = Quantity::from_raw(crate::types::quantity::QUANTITY_RAW_MAX, FIXED_PRECISION);
        let unit = Quantity::from_raw(1, FIXED_PRECISION);

        assert!(quantity_add(max, unit, FIXED_PRECISION).is_err());
        assert!(quantity_sub(unit, max, FIXED_PRECISION).is_err());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_fee_allocation_uses_exact_integer_ratio(#[case] rebate: bool) {
        let fee =
            Money::from_decimal_exact(Decimal::new(1, u32::from(FIXED_PRECISION)), Currency::USD())
                .unwrap();

        let fee = if rebate { -fee } else { fee };
        let (closing, opening) =
            split_commission(fee, Decimal::MAX - Decimal::ONE, Decimal::MAX).unwrap();
        assert_eq!(closing, Money::zero(Currency::USD()));
        assert_eq!(opening, fee);
    }

    #[rstest]
    #[case(5, 0)]
    #[case(15, 2)]
    #[case(25, 2)]
    #[case(-5, 0)]
    #[case(-15, -2)]
    #[case(-25, -2)]
    fn test_money_rounds_native_midpoints_to_even(#[case] coefficient: i64, #[case] expected: i64) {
        let amount = Decimal::new(coefficient, u32::from(FIXED_PRECISION) + 1);
        let expected = Decimal::new(expected, u32::from(FIXED_PRECISION));

        assert_eq!(
            money(amount, Currency::USD()).unwrap().as_decimal(),
            expected
        );
    }

    #[rstest]
    fn test_proportion_preserves_exact_cancellation() {
        let allocated = proportion(dec!(11382), Decimal::ONE, dec!(271)).unwrap();
        assert_eq!(allocated, dec!(42));
        assert_eq!(sub(dec!(11382), allocated).unwrap(), dec!(11340));
    }

    #[rstest]
    fn test_rounded_arithmetic_rejects_underflow() {
        assert!(div(dec!(0.0000000000000000000000000001), dec!(10)).is_err());
        assert!(mul_rounded(dec!(0.0000000000000001), dec!(0.0000000000000001)).is_err());
        assert!(add_rounded(Decimal::MAX, dec!(0.1)).is_err());
        assert!(add_rounded(dec!(0.1), Decimal::MAX).is_err());
    }

    #[rstest]
    fn test_pnl_sum_bounds_lost_residual(
        #[values(false, true)] reverse: bool,
        #[values(Decimal::ONE, Decimal::NEGATIVE_ONE)] sign: Decimal,
    ) {
        let currency = Currency::USD();
        let unit = amount(Money::from_raw(1, currency)).unwrap() * sign;
        let residual = unit / dec!(10);
        let (lhs, rhs) = if reverse {
            (unit, Decimal::MAX)
        } else {
            (Decimal::MAX, unit)
        };

        let (lhs_residual, rhs_residual) = if reverse {
            (residual, Decimal::MAX)
        } else {
            (Decimal::MAX, residual)
        };

        assert!(add_pnl(lhs, rhs, currency).is_err());
        assert_eq!(
            add_pnl(lhs_residual, rhs_residual, currency).unwrap(),
            Decimal::MAX
        );
        assert_eq!(
            add_pnl(Decimal::ONE, residual, currency).unwrap(),
            Decimal::ONE + residual
        );
    }

    #[rstest]
    fn test_checked_product_rejects_silent_underflow() {
        assert!(mul(dec!(0.0000000000000001), dec!(0.0000000000000001)).is_err());
        assert_eq!(mul(dec!(1.2345), dec!(0.01)).unwrap(), dec!(0.012345));
    }

    #[rstest]
    fn test_checked_sum_rejects_lost_increment() {
        assert!(add(Decimal::MAX, dec!(0.1)).is_err());
        assert_eq!(sub(dec!(1.000001), Decimal::ONE).unwrap(), dec!(0.000001));
        assert_eq!(
            money(dec!(0.000001), Currency::USD()).unwrap().as_decimal(),
            dec!(0.000001)
        );
        assert_eq!(
            add(dec!(1000000000), dec!(0.000001)).unwrap(),
            dec!(1000000000.000001)
        );
    }

    #[rstest]
    #[case(dec!(0.000002), dec!(0.5), dec!(0.000001), dec!(0.000001))]
    #[case(dec!(0.006000), dec!(0.9), dec!(0.005400), dec!(0.000600))]
    #[case(dec!(-0.006000), dec!(0.9), dec!(-0.005400), dec!(-0.000600))]
    fn test_fee_allocation(
        #[case] fee: Decimal,
        #[case] closing_qty: Decimal,
        #[case] closing: Decimal,
        #[case] opening: Decimal,
    ) {
        let fee = Money::from_decimal_exact(fee, Currency::USD()).unwrap();
        let (actual_closing, actual_opening) =
            split_commission(fee, closing_qty, Decimal::ONE).unwrap();
        assert_eq!(actual_closing.as_decimal(), closing);
        assert_eq!(actual_opening.as_decimal(), opening);
        assert_eq!(actual_closing.checked_add(actual_opening), Some(fee));
    }
}
