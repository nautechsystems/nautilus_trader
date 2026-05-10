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

//! Position-state reconciliation.
//!
//! Position simulation, partial-window fill reconstruction, mass-status processing,
//! and final position-match checks. The core invariant maintained here is that the
//! reconstructed position matches the venue's reported position within tolerance
//! (default 0.01%) after reconciliation is applied.

use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSideSpecified, TimeInForce},
    identifiers::{AccountId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::{
    ids::{create_synthetic_trade_id, create_synthetic_venue_order_id},
    types::{FillAdjustmentResult, FillSnapshot, ReconciliationResult, VenuePositionSnapshot},
};

const DEFAULT_TOLERANCE: Decimal = Decimal::from_parts(1, 0, 0, false, 4); // 0.0001

/// Simulate position from chronologically ordered fills using netting logic.
///
/// # Returns
///
/// Returns a tuple of (quantity, value) after applying all fills.
#[must_use]
pub fn simulate_position(fills: &[FillSnapshot]) -> (Decimal, Decimal) {
    let mut qty = Decimal::ZERO;
    let mut value = Decimal::ZERO;

    for fill in fills {
        debug_assert!(
            fill.qty > Decimal::ZERO,
            "fill snapshot qty must be positive, received {}",
            fill.qty,
        );
        let direction = Decimal::from(fill.direction());
        let new_qty = qty + (direction * fill.qty);

        // Check if we're accumulating or crossing zero (flip/close)
        if (qty >= Decimal::ZERO && direction > Decimal::ZERO)
            || (qty <= Decimal::ZERO && direction < Decimal::ZERO)
        {
            // Accumulating in same direction
            value += fill.qty * fill.px;
            qty = new_qty;
        } else {
            // Closing or flipping position
            if qty.abs() >= fill.qty {
                // Partial close - maintain average price by reducing value proportionally
                let close_ratio = fill.qty / qty.abs();
                value *= Decimal::ONE - close_ratio;
                qty = new_qty;
            } else {
                // Close and flip - reset value to opening position
                let remaining = fill.qty - qty.abs();
                qty = direction * remaining;
                value = remaining * fill.px;
            }
        }
    }

    debug_assert!(
        value >= Decimal::ZERO,
        "simulated position value must be non-negative, was {value}",
    );
    debug_assert!(
        !(qty != Decimal::ZERO && value.is_sign_negative()),
        "simulated avg price invariant: qty={qty}, value={value}",
    );

    (qty, value)
}

/// Detect zero-crossing timestamps in a sequence of fills.
///
/// A zero-crossing occurs when position quantity crosses through zero (FLAT).
/// This includes both landing exactly on zero and flipping from long to short or vice versa.
///
/// # Returns
///
/// Returns a list of timestamps where position crosses through zero.
#[must_use]
pub fn detect_zero_crossings(fills: &[FillSnapshot]) -> Vec<u64> {
    let mut running_qty = Decimal::ZERO;
    let mut zero_crossings = Vec::new();

    for fill in fills {
        let prev_qty = running_qty;
        running_qty += Decimal::from(fill.direction()) * fill.qty;

        // Detect when position crosses zero
        if prev_qty != Decimal::ZERO {
            if running_qty == Decimal::ZERO {
                // Landed exactly on zero
                zero_crossings.push(fill.ts_event);
            } else if (prev_qty > Decimal::ZERO) != (running_qty > Decimal::ZERO) {
                // Sign changed - crossed through zero (flip)
                zero_crossings.push(fill.ts_event);
            }
        }
    }

    zero_crossings
}

/// Check if simulated position matches venue position within tolerance.
///
/// # Returns
///
/// Returns true if quantities and average prices match within tolerance.
#[must_use]
pub fn check_position_match(
    simulated_qty: Decimal,
    simulated_value: Decimal,
    venue_qty: Decimal,
    venue_avg_px: Decimal,
    tolerance: Decimal,
) -> bool {
    if simulated_qty != venue_qty {
        return false;
    }

    if simulated_qty == Decimal::ZERO {
        return true; // Both FLAT
    }

    // Guard against division by zero
    let abs_qty = simulated_qty.abs();
    if abs_qty == Decimal::ZERO {
        return false;
    }

    let simulated_avg_px = simulated_value / abs_qty;

    // If venue avg px is zero, we cannot calculate relative difference
    if venue_avg_px == Decimal::ZERO {
        return false;
    }

    let relative_diff = (simulated_avg_px - venue_avg_px).abs() / venue_avg_px;

    relative_diff <= tolerance
}

/// Calculate the price needed for a reconciliation order to achieve target position.
///
/// This is a pure function that calculates what price a fill would need to have
/// to move from the current position state to the target position state with the
/// correct average price, accounting for the netting simulation logic.
///
/// # Returns
///
/// Returns `Some(Decimal)` if a valid reconciliation price can be calculated, `None` otherwise.
///
/// # Notes
///
/// The function handles four scenarios:
/// 1. Position to flat: reconciliation_px = current_avg_px (close at current average)
/// 2. Flat to position: reconciliation_px = target_avg_px
/// 3. Position flip (sign change): reconciliation_px = target_avg_px (due to value reset in simulation)
/// 4. Accumulation/reduction: weighted average formula
pub fn calculate_reconciliation_price(
    current_position_qty: Decimal,
    current_position_avg_px: Option<Decimal>,
    target_position_qty: Decimal,
    target_position_avg_px: Option<Decimal>,
) -> Option<Decimal> {
    let qty_diff = target_position_qty - current_position_qty;

    if qty_diff == Decimal::ZERO {
        return None; // No reconciliation needed
    }

    // Special case: closing to flat (target_position_qty == 0)
    // When flattening, the reconciliation price equals the current position's average price
    if target_position_qty == Decimal::ZERO {
        return current_position_avg_px;
    }

    // If target average price is not provided or zero, we cannot calculate
    let target_avg_px = target_position_avg_px?;
    if target_avg_px == Decimal::ZERO {
        return None;
    }

    // If current position is flat, the reconciliation price equals target avg price
    if current_position_qty == Decimal::ZERO || current_position_avg_px.is_none() {
        return Some(target_avg_px);
    }

    let current_avg_px = current_position_avg_px?;

    // Check if this is a flip scenario (sign change)
    // In simulation, flips reset value to remaining * px, so reconciliation_px = target_avg_px
    let is_flip = (current_position_qty > Decimal::ZERO) != (target_position_qty > Decimal::ZERO)
        && target_position_qty != Decimal::ZERO;

    if is_flip {
        return Some(target_avg_px);
    }

    // For accumulation or reduction (same side), use weighted average formula
    // Formula: (target_qty * target_avg_px) = (current_qty * current_avg_px) + (qty_diff * reconciliation_px)
    let target_value = target_position_qty * target_avg_px;
    let current_value = current_position_qty * current_avg_px;
    let diff_value = target_value - current_value;

    // qty_diff is guaranteed to be non-zero here due to early return at line 270
    let reconciliation_px = diff_value / qty_diff;

    // Ensure price is positive
    if reconciliation_px > Decimal::ZERO {
        return Some(reconciliation_px);
    }

    None
}

/// Adjust fills for partial reconciliation window to handle incomplete position lifecycles.
///
/// This function analyzes fills and determines if adjustments are needed when the reconciliation
/// window doesn't capture the complete position history (missing opening fills).
///
/// # Returns
///
/// Returns `FillAdjustmentResult` indicating what adjustments (if any) are needed.
///
#[must_use]
#[expect(clippy::missing_panics_doc)] // All unwraps guarded by prior checks
pub fn adjust_fills_for_partial_window(
    fills: &[FillSnapshot],
    venue_position: &VenuePositionSnapshot,
    _instrument: &InstrumentAny,
    tolerance: Decimal,
) -> FillAdjustmentResult {
    // If no fills, nothing to adjust
    if fills.is_empty() {
        return FillAdjustmentResult::NoAdjustment;
    }

    // If venue position is FLAT, return unchanged
    if venue_position.qty == Decimal::ZERO {
        return FillAdjustmentResult::NoAdjustment;
    }

    // Detect zero-crossings
    let zero_crossings = detect_zero_crossings(fills);

    // Convert venue position to signed quantity
    let venue_qty_signed = match venue_position.side {
        OrderSide::Buy => venue_position.qty,
        OrderSide::Sell => -venue_position.qty,
        _ => Decimal::ZERO,
    };

    // Case 1: Has zero-crossings - focus on current lifecycle after last zero-crossing
    if !zero_crossings.is_empty() {
        // Find the last zero-crossing that lands on FLAT (qty==0)
        // This separates lifecycles; flips within a lifecycle don't count
        let mut last_flat_crossing_ts = None;
        let mut running_qty = Decimal::ZERO;

        for fill in fills {
            let prev_qty = running_qty;
            running_qty += Decimal::from(fill.direction()) * fill.qty;

            if prev_qty != Decimal::ZERO && running_qty == Decimal::ZERO {
                last_flat_crossing_ts = Some(fill.ts_event);
            }
        }

        let lifecycle_boundary_ts =
            last_flat_crossing_ts.unwrap_or(*zero_crossings.last().unwrap());

        // Get fills from current lifecycle (after lifecycle boundary)
        let current_lifecycle_fills: Vec<FillSnapshot> = fills
            .iter()
            .filter(|f| f.ts_event > lifecycle_boundary_ts)
            .cloned()
            .collect();

        if current_lifecycle_fills.is_empty() {
            return FillAdjustmentResult::NoAdjustment;
        }

        // Simulate current lifecycle
        let (current_qty, current_value) = simulate_position(&current_lifecycle_fills);

        // Check if current lifecycle matches venue
        if check_position_match(
            current_qty,
            current_value,
            venue_qty_signed,
            venue_position.avg_px,
            tolerance,
        ) {
            // Current lifecycle matches - filter out old lifecycles
            return FillAdjustmentResult::FilterToCurrentLifecycle {
                last_zero_crossing_ts: lifecycle_boundary_ts,
                current_lifecycle_fills,
            };
        }

        // Current lifecycle doesn't match - replace with synthetic fill
        if let Some(first_fill) = current_lifecycle_fills.first() {
            let synthetic_fill = FillSnapshot::new(
                first_fill.ts_event.saturating_sub(1), // Timestamp before first fill
                venue_position.side,
                venue_position.qty,
                venue_position.avg_px,
                first_fill.venue_order_id,
            );

            return FillAdjustmentResult::ReplaceCurrentLifecycle {
                synthetic_fill,
                first_venue_order_id: first_fill.venue_order_id,
            };
        }

        return FillAdjustmentResult::NoAdjustment;
    }

    // Case 2: Single lifecycle or one zero-crossing
    // Determine which fills to analyze
    let oldest_lifecycle_fills: Vec<FillSnapshot> =
        if let Some(&first_zero_crossing_ts) = zero_crossings.first() {
            // Get fills before first zero-crossing
            fills
                .iter()
                .filter(|f| f.ts_event <= first_zero_crossing_ts)
                .cloned()
                .collect()
        } else {
            // No zero-crossings - all fills are in single lifecycle
            fills.to_vec()
        };

    if oldest_lifecycle_fills.is_empty() {
        return FillAdjustmentResult::NoAdjustment;
    }

    // Simulate oldest lifecycle
    let (oldest_qty, oldest_value) = simulate_position(&oldest_lifecycle_fills);

    // If single lifecycle (no zero-crossings)
    if zero_crossings.is_empty() {
        // Check if simulated position matches venue
        if check_position_match(
            oldest_qty,
            oldest_value,
            venue_qty_signed,
            venue_position.avg_px,
            tolerance,
        ) {
            return FillAdjustmentResult::NoAdjustment;
        }

        // Doesn't match - need to add synthetic opening fill
        if let Some(first_fill) = oldest_lifecycle_fills.first() {
            // Calculate what opening fill is needed
            // Use simulated position as current, venue position as target
            let oldest_avg_px = if oldest_qty == Decimal::ZERO {
                None
            } else {
                Some(oldest_value / oldest_qty.abs())
            };

            let reconciliation_price = calculate_reconciliation_price(
                oldest_qty,
                oldest_avg_px,
                venue_qty_signed,
                Some(venue_position.avg_px),
            );

            if let Some(opening_px) = reconciliation_price {
                // Calculate opening quantity needed
                let opening_qty = if oldest_qty == Decimal::ZERO {
                    venue_qty_signed
                } else {
                    // Work backwards: venue = opening + current fills
                    venue_qty_signed - oldest_qty
                };

                if opening_qty.abs() > Decimal::ZERO {
                    let synthetic_side = if opening_qty > Decimal::ZERO {
                        OrderSide::Buy
                    } else {
                        OrderSide::Sell
                    };

                    let synthetic_fill = FillSnapshot::new(
                        first_fill.ts_event.saturating_sub(1),
                        synthetic_side,
                        opening_qty.abs(),
                        opening_px,
                        first_fill.venue_order_id,
                    );

                    return FillAdjustmentResult::AddSyntheticOpening {
                        synthetic_fill,
                        existing_fills: oldest_lifecycle_fills,
                    };
                }
            }
        }

        return FillAdjustmentResult::NoAdjustment;
    }

    // Has one zero-crossing - check if oldest lifecycle closes at zero
    if oldest_qty == Decimal::ZERO {
        // Lifecycle closes correctly - no adjustment needed
        return FillAdjustmentResult::NoAdjustment;
    }

    // Oldest lifecycle doesn't close at zero - add synthetic opening fill
    if !oldest_lifecycle_fills.is_empty()
        && let Some(&first_zero_crossing_ts) = zero_crossings.first()
    {
        // Need to add opening fill that makes position close at zero-crossing
        let current_lifecycle_fills: Vec<FillSnapshot> = fills
            .iter()
            .filter(|f| f.ts_event > first_zero_crossing_ts)
            .cloned()
            .collect();

        if !current_lifecycle_fills.is_empty()
            && let Some(first_current_fill) = current_lifecycle_fills.first()
        {
            let synthetic_fill = FillSnapshot::new(
                first_current_fill.ts_event.saturating_sub(1),
                venue_position.side,
                venue_position.qty,
                venue_position.avg_px,
                first_current_fill.venue_order_id,
            );

            return FillAdjustmentResult::AddSyntheticOpening {
                synthetic_fill,
                existing_fills: oldest_lifecycle_fills,
            };
        }
    }

    FillAdjustmentResult::NoAdjustment
}

/// Create a synthetic `OrderStatusReport` from a `FillSnapshot`.
///
/// Populates `avg_px` from the fill's price so downstream reconciliation paths
/// (e.g. [`crate::reconciliation::orders::create_inferred_fill`]) can resolve a
/// fill price without falling back to the "no avg_px or price available" warning.
///
/// # Errors
///
/// Returns an error if the fill quantity cannot be converted to f64.
pub fn create_synthetic_order_report(
    fill: &FillSnapshot,
    account_id: AccountId,
    instrument_id: InstrumentId,
    instrument: &InstrumentAny,
    venue_order_id: VenueOrderId,
) -> anyhow::Result<OrderStatusReport> {
    let order_qty = Quantity::from_decimal_dp(fill.qty, instrument.size_precision())?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // client_order_id
        venue_order_id,
        fill.side,
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        order_qty,
        order_qty, // filled_qty = order_qty (fully filled)
        UnixNanos::from(fill.ts_event),
        UnixNanos::from(fill.ts_event),
        UnixNanos::from(fill.ts_event),
        None, // report_id
    );
    report.avg_px = Some(fill.px);
    Ok(report)
}

/// Create a synthetic `FillReport` from a `FillSnapshot`.
///
/// # Errors
///
/// Returns an error if the fill quantity or price cannot be converted.
pub fn create_synthetic_fill_report(
    fill: &FillSnapshot,
    account_id: AccountId,
    instrument_id: InstrumentId,
    instrument: &InstrumentAny,
    venue_order_id: VenueOrderId,
) -> anyhow::Result<FillReport> {
    let trade_id = create_synthetic_trade_id(fill);
    let qty = Quantity::from_decimal_dp(fill.qty, instrument.size_precision())?;
    let px = Price::from_decimal_dp(fill.px, instrument.price_precision())?;

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        fill.side,
        qty,
        px,
        Money::new(0.0, instrument.quote_currency()),
        LiquiditySide::NoLiquiditySide,
        None, // client_order_id
        None, // venue_position_id
        fill.ts_event.into(),
        fill.ts_event.into(),
        None, // report_id
    ))
}

/// Process fill reports from a mass status for position reconciliation.
///
/// This is the main entry point for position reconciliation. It:
/// 1. Extracts fills and position for the given instrument
/// 2. Detects position discrepancies
/// 3. Returns adjusted order/fill reports ready for processing
///
/// # Errors
///
/// Returns an error if synthetic report creation fails.
pub fn process_mass_status_for_reconciliation(
    mass_status: &ExecutionMassStatus,
    instrument: &InstrumentAny,
    tolerance: Option<Decimal>,
) -> anyhow::Result<ReconciliationResult> {
    let instrument_id = instrument.id();
    let account_id = mass_status.account_id;
    let tol = tolerance.unwrap_or(DEFAULT_TOLERANCE);

    // Get position report for this instrument
    let position_reports = mass_status.position_reports();
    let venue_position = match position_reports.get(&instrument_id).and_then(|r| r.first()) {
        Some(report) => position_report_to_snapshot(report),
        None => {
            // No position report - return orders/fills unchanged
            return Ok(extract_instrument_reports(mass_status, instrument_id));
        }
    };

    // Extract and convert fills to snapshots
    let extracted = extract_fills_for_instrument(mass_status, instrument_id);
    let fill_snapshots = extracted.snapshots;
    let mut order_map = extracted.orders;
    let mut fill_map = extracted.fills;

    if fill_snapshots.is_empty() {
        return Ok(ReconciliationResult {
            orders: order_map,
            fills: fill_map,
        });
    }

    // Run adjustment logic
    let result = adjust_fills_for_partial_window(&fill_snapshots, &venue_position, instrument, tol);

    // Apply adjustments
    match result {
        FillAdjustmentResult::NoAdjustment => {}

        FillAdjustmentResult::AddSyntheticOpening {
            synthetic_fill,
            existing_fills: _,
        } => {
            let venue_order_id = create_synthetic_venue_order_id(&synthetic_fill, instrument_id);
            let order = create_synthetic_order_report(
                &synthetic_fill,
                account_id,
                instrument_id,
                instrument,
                venue_order_id,
            )?;
            let fill = create_synthetic_fill_report(
                &synthetic_fill,
                account_id,
                instrument_id,
                instrument,
                venue_order_id,
            )?;

            order_map.insert(venue_order_id, order);
            fill_map.entry(venue_order_id).or_default().insert(0, fill);
        }

        FillAdjustmentResult::ReplaceCurrentLifecycle {
            synthetic_fill,
            first_venue_order_id,
        } => {
            let order = create_synthetic_order_report(
                &synthetic_fill,
                account_id,
                instrument_id,
                instrument,
                first_venue_order_id,
            )?;
            let fill = create_synthetic_fill_report(
                &synthetic_fill,
                account_id,
                instrument_id,
                instrument,
                first_venue_order_id,
            )?;

            // Replace with only synthetic
            order_map.clear();
            fill_map.clear();
            order_map.insert(first_venue_order_id, order);
            fill_map.insert(first_venue_order_id, vec![fill]);
        }

        FillAdjustmentResult::FilterToCurrentLifecycle {
            last_zero_crossing_ts,
            current_lifecycle_fills: _,
        } => {
            // Filter fills to current lifecycle
            for fills in fill_map.values_mut() {
                fills.retain(|f| f.ts_event.as_u64() > last_zero_crossing_ts);
            }
            fill_map.retain(|_, fills| !fills.is_empty());

            // Keep only orders that have fills or are still working
            let orders_with_fills: ahash::AHashSet<VenueOrderId> =
                fill_map.keys().copied().collect();
            order_map.retain(|id, order| {
                orders_with_fills.contains(id)
                    || !matches!(
                        order.order_status,
                        OrderStatus::Denied
                            | OrderStatus::Rejected
                            | OrderStatus::Canceled
                            | OrderStatus::Expired
                            | OrderStatus::Filled
                    )
            });
        }
    }

    Ok(ReconciliationResult {
        orders: order_map,
        fills: fill_map,
    })
}

/// Convert a position status report to a venue position snapshot.
fn position_report_to_snapshot(report: &PositionStatusReport) -> VenuePositionSnapshot {
    let side = match report.position_side {
        PositionSideSpecified::Long => OrderSide::Buy,
        PositionSideSpecified::Short => OrderSide::Sell,
        PositionSideSpecified::Flat => OrderSide::Buy,
    };

    VenuePositionSnapshot {
        side,
        qty: report.quantity.into(),
        avg_px: report.avg_px_open.unwrap_or(Decimal::ZERO),
    }
}

/// Extract orders and fills for a specific instrument from mass status.
fn extract_instrument_reports(
    mass_status: &ExecutionMassStatus,
    instrument_id: InstrumentId,
) -> ReconciliationResult {
    let mut orders = IndexMap::new();
    let mut fills = IndexMap::new();

    for (id, order) in mass_status.order_reports() {
        if order.instrument_id == instrument_id {
            orders.insert(id, order.clone());
        }
    }

    for (id, fill_list) in mass_status.fill_reports() {
        let filtered: Vec<_> = fill_list
            .iter()
            .filter(|f| f.instrument_id == instrument_id)
            .cloned()
            .collect();

        if !filtered.is_empty() {
            fills.insert(id, filtered);
        }
    }

    ReconciliationResult { orders, fills }
}

/// Extracted fills and reports for an instrument.
struct ExtractedFills {
    snapshots: Vec<FillSnapshot>,
    orders: IndexMap<VenueOrderId, OrderStatusReport>,
    fills: IndexMap<VenueOrderId, Vec<FillReport>>,
}

/// Extract fills for an instrument and convert to snapshots.
fn extract_fills_for_instrument(
    mass_status: &ExecutionMassStatus,
    instrument_id: InstrumentId,
) -> ExtractedFills {
    let mut snapshots = Vec::new();
    let mut order_map = IndexMap::new();
    let mut fill_map = IndexMap::new();

    // Seed order_map
    for (id, order) in mass_status.order_reports() {
        if order.instrument_id == instrument_id {
            order_map.insert(id, order.clone());
        }
    }

    // Extract fills
    for (venue_order_id, fill_reports) in mass_status.fill_reports() {
        for fill in fill_reports {
            if fill.instrument_id == instrument_id {
                let side = mass_status
                    .order_reports()
                    .get(&venue_order_id)
                    .map_or(fill.order_side, |o| o.order_side);

                snapshots.push(FillSnapshot::new(
                    fill.ts_event.as_u64(),
                    side,
                    fill.last_qty.into(),
                    fill.last_px.into(),
                    venue_order_id,
                ));

                fill_map
                    .entry(venue_order_id)
                    .or_insert_with(Vec::new)
                    .push(fill.clone());
            }
        }
    }

    // Sort chronologically
    snapshots.sort_by_key(|f| f.ts_event);

    ExtractedFills {
        snapshots,
        orders: order_map,
        fills: fill_map,
    }
}

/// Generates the appropriate order events for an external order and order status report.
///
/// After creating an external order, we need to transition it to its actual state
/// based on the order status report from the venue. For terminal states like
/// Canceled/Expired/Filled, we return multiple events to properly transition
pub fn check_position_reconciliation(
    report: &PositionStatusReport,
    cached_signed_qty: Decimal,
    size_precision: Option<u8>,
) -> bool {
    let venue_signed_qty = report.signed_decimal_qty;

    if venue_signed_qty == Decimal::ZERO && cached_signed_qty == Decimal::ZERO {
        return true;
    }

    if let Some(precision) = size_precision
        && is_within_single_unit_tolerance(cached_signed_qty, venue_signed_qty, precision)
    {
        log::debug!(
            "Position for {} within tolerance: cached={}, venue={}",
            report.instrument_id,
            cached_signed_qty,
            venue_signed_qty
        );
        return true;
    }

    if cached_signed_qty == venue_signed_qty {
        return true;
    }

    log::warn!(
        "Position discrepancy for {}: cached={}, venue={}",
        report.instrument_id,
        cached_signed_qty,
        venue_signed_qty
    );

    false
}

/// Checks if two decimal values are within a single unit of tolerance for the given precision.
///
/// For integer precision (0), requires exact match.
/// For fractional precision, allows difference of 1 unit at that precision.
#[must_use]
pub fn is_within_single_unit_tolerance(value1: Decimal, value2: Decimal, precision: u8) -> bool {
    if precision == 0 {
        return value1 == value2;
    }

    let tolerance = Decimal::new(1, u32::from(precision));
    let difference = (value1 - value2).abs();
    difference <= tolerance
}
