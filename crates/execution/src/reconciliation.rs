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

//! Execution state reconciliation functions.
//!
//! Pure functions for reconciling orders and positions between local state and venue reports.

use std::str::FromStr;

use ahash::AHashMap;
use nautilus_common::enums::LogColor;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSideSpecified, TimeInForce},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
        OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

/// Immutable snapshot of fill data for position simulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillSnapshot {
    /// The event timestamp (nanoseconds).
    pub ts_event: u64,
    /// The order side (BUY or SELL).
    pub side: OrderSide,
    /// The fill quantity.
    pub qty: Decimal,
    /// The fill price.
    pub px: Decimal,
    /// The venue order ID.
    pub venue_order_id: VenueOrderId,
}

/// Represents a position snapshot from the venue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenuePositionSnapshot {
    /// The position side (LONG, SHORT, or FLAT).
    pub side: OrderSide, // Using OrderSide to represent position side for simplicity
    /// The position quantity (always positive, even for SHORT).
    pub qty: Decimal,
    /// The average entry price (can be zero for FLAT positions).
    pub avg_px: Decimal,
}

/// Result of the fill adjustment process.
#[derive(Debug, Clone, PartialEq)]
pub enum FillAdjustmentResult {
    /// No adjustment needed - return fills unchanged.
    NoAdjustment,
    /// Add synthetic opening fill to oldest lifecycle.
    AddSyntheticOpening {
        /// The synthetic fill to add at the beginning.
        synthetic_fill: FillSnapshot,
        /// All existing fills to keep.
        existing_fills: Vec<FillSnapshot>,
    },
    /// Replace entire current lifecycle with single synthetic fill.
    ReplaceCurrentLifecycle {
        /// The single synthetic fill representing the entire position.
        synthetic_fill: FillSnapshot,
        /// The first venue order ID to use.
        first_venue_order_id: VenueOrderId,
    },
    /// Filter fills to current lifecycle only (after last zero-crossing).
    FilterToCurrentLifecycle {
        /// Timestamp of the last zero-crossing.
        last_zero_crossing_ts: u64,
        /// Fills from current lifecycle.
        current_lifecycle_fills: Vec<FillSnapshot>,
    },
}

impl FillSnapshot {
    /// Create a new fill snapshot.
    #[must_use]
    pub fn new(
        ts_event: u64,
        side: OrderSide,
        qty: Decimal,
        px: Decimal,
        venue_order_id: VenueOrderId,
    ) -> Self {
        Self {
            ts_event,
            side,
            qty,
            px,
            venue_order_id,
        }
    }

    /// Return signed direction multiplier: +1 for BUY, -1 for SELL.
    #[must_use]
    pub fn direction(&self) -> i8 {
        match self.side {
            OrderSide::Buy => 1,
            OrderSide::Sell => -1,
            _ => 0,
        }
    }
}

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
/// # Panics
///
/// This function does not panic under normal circumstances as all unwrap calls are guarded by prior checks.
#[must_use]
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

/// Create a synthetic `VenueOrderId` using timestamp and UUID suffix.
///
/// Format: `S-{hex_timestamp}-{uuid_prefix}`
#[must_use]
pub fn create_synthetic_venue_order_id(ts_event: u64) -> VenueOrderId {
    let uuid = UUID4::new();
    let uuid_str = uuid.to_string();
    let uuid_suffix = &uuid_str[..8];
    let venue_order_id_value = format!("S-{ts_event:x}-{uuid_suffix}");
    VenueOrderId::new(&venue_order_id_value)
}

/// Create a synthetic `TradeId` using timestamp and UUID suffix.
///
/// Format: `S-{hex_timestamp}-{uuid_prefix}`
#[must_use]
pub fn create_synthetic_trade_id(ts_event: u64) -> TradeId {
    let uuid = UUID4::new();
    let uuid_str = uuid.to_string();
    let uuid_suffix = &uuid_str[..8];
    let trade_id_value = format!("S-{ts_event:x}-{uuid_suffix}");
    TradeId::new(&trade_id_value)
}

/// Create a synthetic `OrderStatusReport` from a `FillSnapshot`.
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

    Ok(OrderStatusReport::new(
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
    ))
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
    let trade_id = create_synthetic_trade_id(fill.ts_event);
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

/// Result of processing fill reports for reconciliation.
#[derive(Debug, Clone)]
pub struct ReconciliationResult {
    /// Order status reports keyed by venue order ID.
    pub orders: AHashMap<VenueOrderId, OrderStatusReport>,
    /// Fill reports keyed by venue order ID.
    pub fills: AHashMap<VenueOrderId, Vec<FillReport>>,
}

const DEFAULT_TOLERANCE: Decimal = Decimal::from_parts(1, 0, 0, false, 4); // 0.0001

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
            let venue_order_id = create_synthetic_venue_order_id(synthetic_fill.ts_event);
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
    let mut orders = AHashMap::new();
    let mut fills = AHashMap::new();

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
    orders: AHashMap<VenueOrderId, OrderStatusReport>,
    fills: AHashMap<VenueOrderId, Vec<FillReport>>,
}

/// Extract fills for an instrument and convert to snapshots.
fn extract_fills_for_instrument(
    mass_status: &ExecutionMassStatus,
    instrument_id: InstrumentId,
) -> ExtractedFills {
    let mut snapshots = Vec::new();
    let mut order_map = AHashMap::new();
    let mut fill_map = AHashMap::new();

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
/// through states.
#[must_use]
pub fn generate_external_order_status_events(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    let accepted = OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        report.venue_order_id,
        *account_id,
        UUID4::new(),
        report.ts_accepted,
        ts_now,
        true, // reconciliation
    ));

    match report.order_status {
        OrderStatus::Accepted | OrderStatus::Triggered => vec![accepted],
        OrderStatus::PartiallyFilled | OrderStatus::Filled => {
            let mut events = vec![accepted];

            if !report.filled_qty.is_zero()
                && let Some(filled) =
                    create_inferred_fill(order, report, account_id, instrument, ts_now)
            {
                events.push(filled);
            }

            events
        }
        OrderStatus::Canceled => {
            let canceled = OrderEventAny::Canceled(OrderCanceled::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                report.ts_last,
                ts_now,
                true, // reconciliation
                Some(report.venue_order_id),
                Some(*account_id),
            ));
            vec![accepted, canceled]
        }
        OrderStatus::Expired => {
            let expired = OrderEventAny::Expired(OrderExpired::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                report.ts_last,
                ts_now,
                true, // reconciliation
                Some(report.venue_order_id),
                Some(*account_id),
            ));
            vec![accepted, expired]
        }
        OrderStatus::Rejected => {
            // Rejected goes directly to terminal state without acceptance
            vec![OrderEventAny::Rejected(OrderRejected::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                *account_id,
                Ustr::from(report.cancel_reason.as_deref().unwrap_or("UNKNOWN")),
                UUID4::new(),
                report.ts_last,
                ts_now,
                true, // reconciliation
                false,
            ))]
        }
        _ => {
            log::warn!(
                "Unhandled order status {} for external order {}",
                report.order_status,
                order.client_order_id()
            );
            Vec::new()
        }
    }
}

/// Creates an inferred fill event for reconciliation when fill reports are missing.
#[must_use]
pub fn create_inferred_fill(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    let liquidity_side = match order.order_type() {
        OrderType::Market | OrderType::StopMarket | OrderType::TrailingStopMarket => {
            LiquiditySide::Taker
        }
        _ if report.post_only => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    let last_px = if let Some(avg_px) = report.avg_px {
        match Price::from_decimal_dp(avg_px, instrument.price_precision()) {
            Ok(px) => px,
            Err(e) => {
                log::warn!("Failed to create price from avg_px for inferred fill: {e}");
                return None;
            }
        }
    } else if let Some(price) = report.price {
        price
    } else {
        log::warn!(
            "Cannot create inferred fill for {}: no avg_px or price available",
            order.client_order_id()
        );
        return None;
    };

    let trade_id = TradeId::from(UUID4::new().as_str());

    log::info!(
        "Generated inferred fill for {} ({}) qty={} px={}",
        order.client_order_id(),
        report.venue_order_id,
        report.filled_qty,
        last_px,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        report.venue_order_id,
        *account_id,
        trade_id,
        report.order_side,
        order.order_type(),
        report.filled_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        report.venue_position_id,
        None, // commission - not available for inferred fills
    )))
}

/// Creates an OrderAccepted event for reconciliation.
///
/// # Panics
///
/// Panics if the order does not have an `account_id` set.
#[must_use]
pub fn create_reconciliation_accepted(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.venue_order_id().unwrap_or(report.venue_order_id),
        order
            .account_id()
            .expect("Order should have account_id for reconciliation"),
        UUID4::new(),
        report.ts_accepted,
        ts_now,
        true, // reconciliation
    ))
}

/// Creates an OrderRejected event for reconciliation.
#[must_use]
pub fn create_reconciliation_rejected(
    order: &OrderAny,
    reason: Option<&str>,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    let account_id = order.account_id()?;
    let reason = reason.unwrap_or("UNKNOWN");

    Some(OrderEventAny::Rejected(OrderRejected::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id,
        Ustr::from(reason),
        UUID4::new(),
        ts_now,
        ts_now,
        true,  // reconciliation
        false, // due_post_only
    )))
}

/// Creates an OrderTriggered event for reconciliation.
#[must_use]
pub fn create_reconciliation_triggered(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Triggered(OrderTriggered::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        UUID4::new(),
        report.ts_triggered.unwrap_or(ts_now),
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
    ))
}

/// Creates an OrderCanceled event for reconciliation.
#[must_use]
pub fn create_reconciliation_canceled(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Canceled(OrderCanceled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
    ))
}

/// Creates an OrderExpired event for reconciliation.
#[must_use]
pub fn create_reconciliation_expired(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Expired(OrderExpired::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
    ))
}

/// Creates an OrderUpdated event for reconciliation.
#[must_use]
pub fn create_reconciliation_updated(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Updated(OrderUpdated::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        report.quantity,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
        report.price,
        report.trigger_price,
        None, // protection_price
    ))
}

/// Checks if the order should be updated based on quantity, price, or trigger price
/// differences from the venue report.
#[must_use]
pub fn should_reconciliation_update(order: &OrderAny, report: &OrderStatusReport) -> bool {
    // Quantity change only valid if new qty >= filled qty
    if report.quantity != order.quantity() && report.quantity >= order.filled_qty() {
        return true;
    }

    match order.order_type() {
        OrderType::Limit => report.price != order.price(),
        OrderType::StopMarket | OrderType::TrailingStopMarket => {
            report.trigger_price != order.trigger_price()
        }
        OrderType::StopLimit | OrderType::TrailingStopLimit => {
            report.trigger_price != order.trigger_price() || report.price != order.price()
        }
        _ => false,
    }
}

/// Reconciles an order with a venue status report, generating appropriate events.
///
/// This is the core reconciliation logic that handles all order status transitions.
/// For fill reconciliation with inferred fills, use `reconcile_order_with_fills`.
#[must_use]
pub fn reconcile_order_report(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    if order.status() == report.order_status && order.filled_qty() == report.filled_qty {
        if should_reconciliation_update(order, report) {
            log::info!(
                "Order {} has been updated at venue: qty={}->{}, price={:?}->{:?}",
                order.client_order_id(),
                order.quantity(),
                report.quantity,
                order.price(),
                report.price
            );
            return Some(create_reconciliation_updated(order, report, ts_now));
        }
        return None; // Already in sync
    }

    match report.order_status {
        OrderStatus::Accepted => {
            if order.status() == OrderStatus::Accepted
                && should_reconciliation_update(order, report)
            {
                return Some(create_reconciliation_updated(order, report, ts_now));
            }
            Some(create_reconciliation_accepted(order, report, ts_now))
        }
        OrderStatus::Rejected => {
            create_reconciliation_rejected(order, report.cancel_reason.as_deref(), ts_now)
        }
        OrderStatus::Triggered => Some(create_reconciliation_triggered(order, report, ts_now)),
        OrderStatus::Canceled => Some(create_reconciliation_canceled(order, report, ts_now)),
        OrderStatus::Expired => Some(create_reconciliation_expired(order, report, ts_now)),

        OrderStatus::PartiallyFilled | OrderStatus::Filled => {
            reconcile_fill_quantity_mismatch(order, report, instrument, ts_now)
        }

        // Pending states - venue will confirm, just log
        OrderStatus::PendingUpdate | OrderStatus::PendingCancel => {
            log::debug!(
                "Order {} in pending state: {:?}",
                order.client_order_id(),
                report.order_status
            );
            None
        }

        // Internal states - should not appear in venue reports
        OrderStatus::Initialized
        | OrderStatus::Submitted
        | OrderStatus::Denied
        | OrderStatus::Emulated
        | OrderStatus::Released => {
            log::warn!(
                "Unexpected order status in venue report for {}: {:?}",
                order.client_order_id(),
                report.order_status
            );
            None
        }
    }
}

/// Handles fill quantity mismatch between cached order and venue report.
///
/// Returns an inferred fill event if the venue reports more filled quantity than we have.
fn reconcile_fill_quantity_mismatch(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    let order_filled_qty = order.filled_qty();
    let report_filled_qty = report.filled_qty;

    if report_filled_qty < order_filled_qty {
        // Venue reports less filled than we have - potential state corruption
        log::error!(
            "Fill qty mismatch for {}: cached={}, venue={} (venue < cached)",
            order.client_order_id(),
            order_filled_qty,
            report_filled_qty
        );
        return None;
    }

    if report_filled_qty > order_filled_qty {
        // Check if order is already closed - skip inferred fill to avoid invalid state
        // (matching Python behavior in _handle_fill_quantity_mismatch)
        if order.is_closed() {
            let precision = order_filled_qty.precision.max(report_filled_qty.precision);
            if is_within_single_unit_tolerance(
                report_filled_qty.as_decimal(),
                order_filled_qty.as_decimal(),
                precision,
            ) {
                return None;
            }

            log::debug!(
                "{} {} already closed but reported difference in filled_qty: \
                report={}, cached={}, skipping inferred fill generation for closed order",
                order.instrument_id(),
                order.client_order_id(),
                report_filled_qty,
                order_filled_qty,
            );
            return None;
        }

        // Venue has more fills - generate inferred fill for the difference
        let Some(instrument) = instrument else {
            log::warn!(
                "Cannot generate inferred fill for {}: instrument not available",
                order.client_order_id()
            );
            return None;
        };

        let account_id = order.account_id()?;
        return create_incremental_inferred_fill(order, report, &account_id, instrument, ts_now);
    }

    // Quantities match but status differs - potential state inconsistency
    if order.status() != report.order_status {
        log::warn!(
            "Status mismatch with matching fill qty for {}: local={:?}, venue={:?}, filled_qty={}",
            order.client_order_id(),
            order.status(),
            report.order_status,
            report.filled_qty
        );
    }

    None
}

/// Creates an inferred fill for the quantity difference between order and report.
///
/// This handles incremental fills where the order already has some filled quantity.
pub fn create_incremental_inferred_fill(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    let order_filled_qty = order.filled_qty();
    let last_qty = report.filled_qty - order_filled_qty;

    if last_qty <= Quantity::zero(instrument.size_precision()) {
        return None;
    }

    let liquidity_side = match order.order_type() {
        OrderType::Market
        | OrderType::StopMarket
        | OrderType::MarketToLimit
        | OrderType::TrailingStopMarket => LiquiditySide::Taker,
        _ if order.is_post_only() => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    let last_px = calculate_incremental_fill_price(order, report, instrument)?;

    let trade_id = TradeId::new(UUID4::new().to_string());

    let venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);

    log::info!(
        color = LogColor::Blue as u8;
        "Generated inferred fill for {}: qty={}, px={}",
        order.client_order_id(),
        last_qty,
        last_px,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        *account_id,
        trade_id,
        order.order_side(),
        order.order_type(),
        last_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        None, // venue_position_id
        None, // commission - unknown for inferred fills
    )))
}

/// Creates an inferred fill with a specific quantity.
///
/// Unlike `create_incremental_inferred_fill`, this takes the fill quantity directly
/// rather than calculating it from order state. Useful when order state hasn't been
/// updated yet (e.g., during external order processing).
pub fn create_inferred_fill_for_qty(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    fill_qty: Quantity,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    if fill_qty.is_zero() {
        return None;
    }

    let liquidity_side = match order.order_type() {
        OrderType::Market
        | OrderType::StopMarket
        | OrderType::MarketToLimit
        | OrderType::TrailingStopMarket => LiquiditySide::Taker,
        _ if order.is_post_only() => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    let last_px = if let Some(avg_px) = report.avg_px {
        Price::from_decimal_dp(avg_px, instrument.price_precision()).ok()?
    } else if let Some(price) = report.price {
        price
    } else if let Some(price) = order.price() {
        price
    } else {
        log::warn!(
            "Cannot determine fill price for {}: no avg_px or price available",
            order.client_order_id()
        );
        return None;
    };

    let trade_id = TradeId::new(UUID4::new().to_string());

    let venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);

    log::info!(
        color = LogColor::Blue as u8;
        "Generated inferred fill for {}: qty={}, px={}",
        order.client_order_id(),
        fill_qty,
        last_px,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        *account_id,
        trade_id,
        order.order_side(),
        order.order_type(),
        fill_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        None, // venue_position_id
        None, // commission - unknown for inferred fills
    )))
}

/// Calculates the fill price for an incremental inferred fill.
fn calculate_incremental_fill_price(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: &InstrumentAny,
) -> Option<Price> {
    let order_filled_qty = order.filled_qty();

    // First fill - use avg_px from report or order price
    if order_filled_qty.is_zero() {
        if let Some(avg_px) = report.avg_px {
            return Price::from_decimal_dp(avg_px, instrument.price_precision()).ok();
        }
        if let Some(price) = report.price {
            return Some(price);
        }
        if let Some(price) = order.price() {
            return Some(price);
        }
        log::warn!(
            "Cannot determine fill price for {}: no avg_px, report price, or order price",
            order.client_order_id()
        );
        return None;
    }

    // Incremental fill - calculate price using weighted average
    if let Some(report_avg_px) = report.avg_px {
        let Some(order_avg_px) = order.avg_px() else {
            // No previous avg_px, use report avg_px
            return Price::from_decimal_dp(report_avg_px, instrument.price_precision()).ok();
        };
        let report_filled_qty = report.filled_qty;
        let last_qty = report_filled_qty - order_filled_qty;

        let report_notional = report_avg_px * report_filled_qty.as_decimal();
        let order_notional = Decimal::from_str(&order_avg_px.to_string()).unwrap_or_default()
            * order_filled_qty.as_decimal();
        let last_notional = report_notional - order_notional;
        let last_px_decimal = last_notional / last_qty.as_decimal();

        return Price::from_decimal_dp(last_px_decimal, instrument.price_precision()).ok();
    }

    // Fallback to report price or order price
    if let Some(price) = report.price {
        return Some(price);
    }

    order.price()
}

/// Creates an OrderFilled event from a FillReport.
///
/// This is used during reconciliation when a fill report is received from the venue.
/// Returns `None` if the fill is a duplicate or would cause an overfill.
#[must_use]
pub fn reconcile_fill_report(
    order: &OrderAny,
    report: &FillReport,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
    allow_overfills: bool,
) -> Option<OrderEventAny> {
    if order.trade_ids().iter().any(|id| **id == report.trade_id) {
        log::debug!(
            "Duplicate fill detected: trade_id {} already exists for order {}",
            report.trade_id,
            order.client_order_id()
        );
        return None;
    }

    let potential_filled_qty = order.filled_qty() + report.last_qty;
    if potential_filled_qty > order.quantity() {
        if !allow_overfills {
            log::warn!(
                "Rejecting fill that would cause overfill for {}: order.quantity={}, order.filled_qty={}, fill.last_qty={}, would result in filled_qty={}",
                order.client_order_id(),
                order.quantity(),
                order.filled_qty(),
                report.last_qty,
                potential_filled_qty
            );
            return None;
        }
        log::warn!(
            "Allowing overfill during reconciliation for {}: order.quantity={}, order.filled_qty={}, fill.last_qty={}, will result in filled_qty={}",
            order.client_order_id(),
            order.quantity(),
            order.filled_qty(),
            report.last_qty,
            potential_filled_qty
        );
    }

    // Use order's account_id if available, fallback to report's account_id
    let account_id = order.account_id().unwrap_or(report.account_id);
    let venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);

    log::info!(
        color = LogColor::Blue as u8;
        "Reconciling fill for {}: qty={}, px={}, trade_id={}",
        order.client_order_id(),
        report.last_qty,
        report.last_px,
        report.trade_id,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        account_id,
        report.trade_id,
        order.order_side(),
        order.order_type(),
        report.last_qty,
        report.last_px,
        instrument.quote_currency(),
        report.liquidity_side,
        UUID4::new(),
        report.ts_event,
        ts_now,
        true, // reconciliation
        report.venue_position_id,
        Some(report.commission),
    )))
}

/// Reconciles a position status report, comparing venue state with local state.
///
/// Returns true if positions are reconciled (match or discrepancy handled),
/// false if there's an error that couldn't be resolved.
#[must_use]
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

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
mod tests {
    use nautilus_model::{
        enums::TimeInForce,
        events::{OrderAccepted, OrderSubmitted},
        identifiers::{AccountId, ClientOrderId, VenueOrderId},
        instruments::stubs::{audusd_sim, crypto_perpetual_ethusdt},
        orders::OrderTestBuilder,
        reports::OrderStatusReport,
        types::Currency,
    };
    use rstest::{fixture, rstest};
    use rust_decimal_macros::dec;

    use super::*;

    #[fixture]
    fn instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(audusd_sim())
    }

    fn create_test_venue_order_id(value: &str) -> VenueOrderId {
        VenueOrderId::new(value)
    }

    #[rstest]
    fn test_fill_snapshot_direction() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let buy_fill = FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id);
        assert_eq!(buy_fill.direction(), 1);

        let sell_fill =
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id);
        assert_eq!(sell_fill.direction(), -1);
    }

    #[rstest]
    fn test_simulate_position_accumulate_long() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Buy, dec!(5), dec!(102), venue_order_id),
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(15));
        assert_eq!(value, dec!(1510)); // 10*100 + 5*102
    }

    #[rstest]
    fn test_simulate_position_close_and_flip() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(15), dec!(102), venue_order_id),
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(-5)); // Flipped from +10 to -5
        assert_eq!(value, dec!(510)); // Remaining 5 @ 102
    }

    #[rstest]
    fn test_simulate_position_partial_close() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(102), venue_order_id),
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(5));
        assert_eq!(value, dec!(500)); // Reduced proportionally: 1000 * (1 - 5/10) = 500

        // Verify average price is maintained
        let avg_px = value / qty;
        assert_eq!(avg_px, dec!(100));
    }

    #[rstest]
    fn test_simulate_position_multiple_partial_closes() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(10.0), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(25), dec!(11.0), venue_order_id), // Close 25%
            FillSnapshot::new(3000, OrderSide::Sell, dec!(25), dec!(12.0), venue_order_id), // Close another 25%
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(50));
        // After first close: value = 1000 * (1 - 25/100) = 1000 * 0.75 = 750
        // After second close: value = 750 * (1 - 25/75) = 750 * (50/75) = 500
        // Due to decimal precision, we check it's close to 500
        assert!((value - dec!(500)).abs() < dec!(0.01));

        // Verify average price is maintained at 10.0
        let avg_px = value / qty;
        assert!((avg_px - dec!(10.0)).abs() < dec!(0.01));
    }

    #[rstest]
    fn test_simulate_position_short_partial_close() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Buy, dec!(5), dec!(98), venue_order_id), // Partial close
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(-5));
        assert_eq!(value, dec!(500)); // Reduced proportionally: 1000 * (1 - 5/10) = 500

        // Verify average price is maintained
        let avg_px = value / qty.abs();
        assert_eq!(avg_px, dec!(100));
    }

    #[rstest]
    fn test_detect_zero_crossings() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Close to zero
            FillSnapshot::new(3000, OrderSide::Buy, dec!(5), dec!(103), venue_order_id),
            FillSnapshot::new(4000, OrderSide::Sell, dec!(5), dec!(104), venue_order_id), // Close to zero again
        ];

        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 2);
        assert_eq!(crossings[0], 2000);
        assert_eq!(crossings[1], 4000);
    }

    #[rstest]
    fn test_check_position_match_exact() {
        let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(100), dec!(0.0001));
        assert!(result);
    }

    #[rstest]
    fn test_check_position_match_within_tolerance() {
        // Simulated avg px = 1000/10 = 100, venue = 100.005
        // Relative diff = 0.005 / 100.005 = 0.00004999 < 0.0001
        let result =
            check_position_match(dec!(10), dec!(1000), dec!(10), dec!(100.005), dec!(0.0001));
        assert!(result);
    }

    #[rstest]
    fn test_check_position_match_qty_mismatch() {
        let result = check_position_match(dec!(10), dec!(1000), dec!(11), dec!(100), dec!(0.0001));
        assert!(!result);
    }

    #[rstest]
    fn test_check_position_match_both_flat() {
        let result = check_position_match(dec!(0), dec!(0), dec!(0), dec!(0), dec!(0.0001));
        assert!(result);
    }

    #[rstest]
    fn test_reconciliation_price_flat_to_long(_instrument: InstrumentAny) {
        let result = calculate_reconciliation_price(dec!(0), None, dec!(10), Some(dec!(100)));
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(100));
    }

    #[rstest]
    fn test_reconciliation_price_no_target_avg_px(_instrument: InstrumentAny) {
        let result = calculate_reconciliation_price(dec!(5), Some(dec!(100)), dec!(10), None);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconciliation_price_no_quantity_change(_instrument: InstrumentAny) {
        let result =
            calculate_reconciliation_price(dec!(10), Some(dec!(100)), dec!(10), Some(dec!(105)));
        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconciliation_price_long_position_increase(_instrument: InstrumentAny) {
        let result =
            calculate_reconciliation_price(dec!(10), Some(dec!(100)), dec!(15), Some(dec!(102)));
        assert!(result.is_some());
        // Expected: (15 * 102 - 10 * 100) / 5 = (1530 - 1000) / 5 = 106
        assert_eq!(result.unwrap(), dec!(106));
    }

    #[rstest]
    fn test_reconciliation_price_flat_to_short(_instrument: InstrumentAny) {
        let result = calculate_reconciliation_price(dec!(0), None, dec!(-10), Some(dec!(100)));
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(100));
    }

    #[rstest]
    fn test_reconciliation_price_long_to_flat(_instrument: InstrumentAny) {
        // Close long position to flat: 100 @ 1.20 to 0
        // When closing to flat, reconciliation price equals current average price
        let result =
            calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(0), Some(dec!(0)));
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(1.20));
    }

    #[rstest]
    fn test_reconciliation_price_short_to_flat(_instrument: InstrumentAny) {
        // Close short position to flat: -50 @ 2.50 to 0
        // When closing to flat, reconciliation price equals current average price
        let result = calculate_reconciliation_price(dec!(-50), Some(dec!(2.50)), dec!(0), None);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(2.50));
    }

    #[rstest]
    fn test_reconciliation_price_short_position_increase(_instrument: InstrumentAny) {
        // Short position increase: -100 @ 1.30 to -200 @ 1.28
        // (−200 × 1.28) = (−100 × 1.30) + (−100 × reconciliation_px)
        // −256 = −130 + (−100 × reconciliation_px)
        // reconciliation_px = 1.26
        let result = calculate_reconciliation_price(
            dec!(-100),
            Some(dec!(1.30)),
            dec!(-200),
            Some(dec!(1.28)),
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(1.26));
    }

    #[rstest]
    fn test_reconciliation_price_long_position_decrease(_instrument: InstrumentAny) {
        // Long position decrease: 200 @ 1.20 to 100 @ 1.20
        let result = calculate_reconciliation_price(
            dec!(200),
            Some(dec!(1.20)),
            dec!(100),
            Some(dec!(1.20)),
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(1.20));
    }

    #[rstest]
    fn test_reconciliation_price_long_to_short_flip(_instrument: InstrumentAny) {
        // Long to short flip: 100 @ 1.20 to -100 @ 1.25
        // Due to netting simulation resetting value on flip, reconciliation_px = target_avg_px
        let result = calculate_reconciliation_price(
            dec!(100),
            Some(dec!(1.20)),
            dec!(-100),
            Some(dec!(1.25)),
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(1.25));
    }

    #[rstest]
    fn test_reconciliation_price_short_to_long_flip(_instrument: InstrumentAny) {
        // Short to long flip: -100 @ 1.30 to 100 @ 1.25
        // Due to netting simulation resetting value on flip, reconciliation_px = target_avg_px
        let result = calculate_reconciliation_price(
            dec!(-100),
            Some(dec!(1.30)),
            dec!(100),
            Some(dec!(1.25)),
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(1.25));
    }

    #[rstest]
    fn test_reconciliation_price_complex_scenario(_instrument: InstrumentAny) {
        // Complex: 150 @ 1.23456 to 250 @ 1.24567
        // (250 × 1.24567) = (150 × 1.23456) + (100 × reconciliation_px)
        // 311.4175 = 185.184 + (100 × reconciliation_px)
        // reconciliation_px = 1.262335
        let result = calculate_reconciliation_price(
            dec!(150),
            Some(dec!(1.23456)),
            dec!(250),
            Some(dec!(1.24567)),
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(1.262335));
    }

    #[rstest]
    fn test_reconciliation_price_zero_target_avg_px(_instrument: InstrumentAny) {
        let result =
            calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(200), Some(dec!(0)));
        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconciliation_price_negative_price(_instrument: InstrumentAny) {
        // Negative price calculation: 100 @ 2.00 to 200 @ 1.00
        // (200 × 1.00) = (100 × 2.00) + (100 × reconciliation_px)
        // 200 = 200 + (100 × reconciliation_px)
        // reconciliation_px = 0 (should return None as price must be positive)
        let result = calculate_reconciliation_price(
            dec!(100),
            Some(dec!(2.00)),
            dec!(200),
            Some(dec!(1.00)),
        );
        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconciliation_price_flip_simulation_compatibility() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        // Start with long position: 100 @ 1.20
        // Target: -100 @ 1.25
        // Calculate reconciliation price
        let recon_px = calculate_reconciliation_price(
            dec!(100),
            Some(dec!(1.20)),
            dec!(-100),
            Some(dec!(1.25)),
        )
        .expect("reconciliation price");

        assert_eq!(recon_px, dec!(1.25));

        // Simulate the flip with reconciliation fill (sell 200 to go from +100 to -100)
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(200), recon_px, venue_order_id),
        ];

        let (final_qty, final_value) = simulate_position(&fills);
        assert_eq!(final_qty, dec!(-100));
        let final_avg = final_value / final_qty.abs();
        assert_eq!(final_avg, dec!(1.25), "Final average should match target");
    }

    #[rstest]
    fn test_reconciliation_price_accumulation_simulation_compatibility() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        // Start with long position: 100 @ 1.20
        // Target: 200 @ 1.22
        let recon_px = calculate_reconciliation_price(
            dec!(100),
            Some(dec!(1.20)),
            dec!(200),
            Some(dec!(1.22)),
        )
        .expect("reconciliation price");

        // Simulate accumulation with reconciliation fill
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Buy, dec!(100), recon_px, venue_order_id),
        ];

        let (final_qty, final_value) = simulate_position(&fills);
        assert_eq!(final_qty, dec!(200));
        let final_avg = final_value / final_qty.abs();
        assert_eq!(final_avg, dec!(1.22), "Final average should match target");
    }

    #[rstest]
    fn test_simulate_position_accumulate_short() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(98), venue_order_id),
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(-15));
        assert_eq!(value, dec!(1490)); // 10*100 + 5*98
    }

    #[rstest]
    fn test_simulate_position_short_to_long_flip() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Buy, dec!(15), dec!(102), venue_order_id),
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(5)); // Flipped from -10 to +5
        assert_eq!(value, dec!(510)); // Remaining 5 @ 102
    }

    #[rstest]
    fn test_simulate_position_multiple_flips() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(15), dec!(105), venue_order_id), // Flip to -5
            FillSnapshot::new(3000, OrderSide::Buy, dec!(10), dec!(110), venue_order_id), // Flip to +5
        ];

        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(5)); // Final position: +5
        assert_eq!(value, dec!(550)); // 5 @ 110
    }

    #[rstest]
    fn test_simulate_position_empty_fills() {
        let fills: Vec<FillSnapshot> = vec![];
        let (qty, value) = simulate_position(&fills);
        assert_eq!(qty, dec!(0));
        assert_eq!(value, dec!(0));
    }

    #[rstest]
    fn test_detect_zero_crossings_no_crossings() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Buy, dec!(5), dec!(102), venue_order_id),
        ];

        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 0);
    }

    #[rstest]
    fn test_detect_zero_crossings_single_crossing() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Close to zero
        ];

        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0], 2000);
    }

    #[rstest]
    fn test_detect_zero_crossings_empty_fills() {
        let fills: Vec<FillSnapshot> = vec![];
        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 0);
    }

    #[rstest]
    fn test_detect_zero_crossings_long_to_short_flip() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        // Buy 10, then Sell 15 -> flip from +10 to -5
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(15), dec!(102), venue_order_id), // Flip
        ];

        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0], 2000); // Detected the flip
    }

    #[rstest]
    fn test_detect_zero_crossings_short_to_long_flip() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        // Sell 10, then Buy 20 -> flip from -10 to +10
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Sell, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Buy, dec!(20), dec!(102), venue_order_id), // Flip
        ];

        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0], 2000);
    }

    #[rstest]
    fn test_detect_zero_crossings_multiple_flips() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Land on zero
            FillSnapshot::new(3000, OrderSide::Sell, dec!(5), dec!(103), venue_order_id), // Go short
            FillSnapshot::new(4000, OrderSide::Buy, dec!(15), dec!(104), venue_order_id), // Flip to long
        ];

        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 2);
        assert_eq!(crossings[0], 2000); // First zero-crossing (land on zero)
        assert_eq!(crossings[1], 4000); // Second zero-crossing (flip)
    }

    #[rstest]
    fn test_check_position_match_outside_tolerance() {
        // Simulated avg px = 1000/10 = 100, venue = 101
        // Relative diff = 1 / 101 = 0.0099 > 0.0001
        let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(101), dec!(0.0001));
        assert!(!result);
    }

    #[rstest]
    fn test_check_position_match_edge_of_tolerance() {
        // Simulated avg px = 1000/10 = 100, venue = 100.01
        // Relative diff = 0.01 / 100.01 = 0.00009999 < 0.0001
        let result =
            check_position_match(dec!(10), dec!(1000), dec!(10), dec!(100.01), dec!(0.0001));
        assert!(result);
    }

    #[rstest]
    fn test_check_position_match_zero_venue_avg_px() {
        let result = check_position_match(dec!(10), dec!(1000), dec!(10), dec!(0), dec!(0.0001));
        assert!(!result); // Should fail because relative diff calculation with zero denominator
    }

    #[rstest]
    fn test_adjust_fills_no_fills() {
        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(0.02),
            avg_px: dec!(4100.00),
        };
        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&[], &venue_position, &instrument, dec!(0.0001));
        assert!(matches!(result, FillAdjustmentResult::NoAdjustment));
    }

    #[rstest]
    fn test_adjust_fills_flat_position() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let fills = vec![FillSnapshot::new(
            1000,
            OrderSide::Buy,
            dec!(0.01),
            dec!(4100.00),
            venue_order_id,
        )];
        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(0),
            avg_px: dec!(0),
        };
        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));
        assert!(matches!(result, FillAdjustmentResult::NoAdjustment));
    }

    #[rstest]
    fn test_adjust_fills_complete_lifecycle_no_adjustment() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        let venue_order_id2 = create_test_venue_order_id("ORDER2");
        let fills = vec![
            FillSnapshot::new(
                1000,
                OrderSide::Buy,
                dec!(0.01),
                dec!(4100.00),
                venue_order_id,
            ),
            FillSnapshot::new(
                2000,
                OrderSide::Buy,
                dec!(0.01),
                dec!(4100.00),
                venue_order_id2,
            ),
        ];
        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(0.02),
            avg_px: dec!(4100.00),
        };
        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));
        assert!(matches!(result, FillAdjustmentResult::NoAdjustment));
    }

    #[rstest]
    fn test_adjust_fills_incomplete_lifecycle_adds_synthetic() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        // Window only sees +0.02 @ 4200, but venue has 0.04 @ 4100
        let fills = vec![FillSnapshot::new(
            2000,
            OrderSide::Buy,
            dec!(0.02),
            dec!(4200.00),
            venue_order_id,
        )];
        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(0.04),
            avg_px: dec!(4100.00),
        };
        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        match result {
            FillAdjustmentResult::AddSyntheticOpening {
                synthetic_fill,
                existing_fills,
            } => {
                assert_eq!(synthetic_fill.side, OrderSide::Buy);
                assert_eq!(synthetic_fill.qty, dec!(0.02)); // Missing 0.02
                assert_eq!(existing_fills.len(), 1);
            }
            _ => panic!("Expected AddSyntheticOpening"),
        }
    }

    #[rstest]
    fn test_adjust_fills_with_zero_crossings() {
        let venue_order_id1 = create_test_venue_order_id("ORDER1");
        let venue_order_id2 = create_test_venue_order_id("ORDER2");
        let venue_order_id3 = create_test_venue_order_id("ORDER3");

        // Lifecycle 1: LONG 0.02 -> FLAT (zero-crossing at 2000)
        // Lifecycle 2: LONG 0.03 (current)
        let fills = vec![
            FillSnapshot::new(
                1000,
                OrderSide::Buy,
                dec!(0.02),
                dec!(4100.00),
                venue_order_id1,
            ),
            FillSnapshot::new(
                2000,
                OrderSide::Sell,
                dec!(0.02),
                dec!(4150.00),
                venue_order_id2,
            ), // Zero-crossing
            FillSnapshot::new(
                3000,
                OrderSide::Buy,
                dec!(0.03),
                dec!(4200.00),
                venue_order_id3,
            ), // Current lifecycle
        ];

        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(0.03),
            avg_px: dec!(4200.00),
        };

        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Should filter to current lifecycle only
        match result {
            FillAdjustmentResult::FilterToCurrentLifecycle {
                last_zero_crossing_ts,
                current_lifecycle_fills,
            } => {
                assert_eq!(last_zero_crossing_ts, 2000);
                assert_eq!(current_lifecycle_fills.len(), 1);
                assert_eq!(current_lifecycle_fills[0].venue_order_id, venue_order_id3);
            }
            _ => panic!("Expected FilterToCurrentLifecycle, was {result:?}"),
        }
    }

    #[rstest]
    fn test_adjust_fills_multiple_zero_crossings_mismatch() {
        let venue_order_id1 = create_test_venue_order_id("ORDER1");
        let venue_order_id2 = create_test_venue_order_id("ORDER2");
        let _venue_order_id3 = create_test_venue_order_id("ORDER3");
        let venue_order_id4 = create_test_venue_order_id("ORDER4");
        let venue_order_id5 = create_test_venue_order_id("ORDER5");

        // Lifecycle 1: LONG 0.05 -> FLAT
        // Lifecycle 2: Current fills produce 0.10 @ 4050, but venue has 0.05 @ 4142.04
        let fills = vec![
            FillSnapshot::new(
                1000,
                OrderSide::Buy,
                dec!(0.05),
                dec!(4000.00),
                venue_order_id1,
            ),
            FillSnapshot::new(
                2000,
                OrderSide::Sell,
                dec!(0.05),
                dec!(4050.00),
                venue_order_id2,
            ), // Zero-crossing
            FillSnapshot::new(
                3000,
                OrderSide::Buy,
                dec!(0.05),
                dec!(4000.00),
                venue_order_id4,
            ), // Current lifecycle
            FillSnapshot::new(
                4000,
                OrderSide::Buy,
                dec!(0.05),
                dec!(4100.00),
                venue_order_id5,
            ), // Current lifecycle
        ];

        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(0.05),
            avg_px: dec!(4142.04),
        };

        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Should replace current lifecycle with synthetic
        match result {
            FillAdjustmentResult::ReplaceCurrentLifecycle {
                synthetic_fill,
                first_venue_order_id,
            } => {
                assert_eq!(synthetic_fill.qty, dec!(0.05));
                assert_eq!(synthetic_fill.px, dec!(4142.04));
                assert_eq!(synthetic_fill.side, OrderSide::Buy);
                assert_eq!(first_venue_order_id, venue_order_id4);
            }
            _ => panic!("Expected ReplaceCurrentLifecycle, was {result:?}"),
        }
    }

    #[rstest]
    fn test_adjust_fills_short_position() {
        let venue_order_id = create_test_venue_order_id("ORDER1");

        // Window only sees SELL 0.02 @ 4120, but venue has -0.05 @ 4100
        let fills = vec![FillSnapshot::new(
            1000,
            OrderSide::Sell,
            dec!(0.02),
            dec!(4120.00),
            venue_order_id,
        )];

        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Sell,
            qty: dec!(0.05),
            avg_px: dec!(4100.00),
        };

        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Should add synthetic opening SHORT fill
        match result {
            FillAdjustmentResult::AddSyntheticOpening {
                synthetic_fill,
                existing_fills,
            } => {
                assert_eq!(synthetic_fill.side, OrderSide::Sell);
                assert_eq!(synthetic_fill.qty, dec!(0.03)); // Missing 0.03
                assert_eq!(existing_fills.len(), 1);
            }
            _ => panic!("Expected AddSyntheticOpening, was {result:?}"),
        }
    }

    #[rstest]
    fn test_adjust_fills_timestamp_underflow_protection() {
        let venue_order_id = create_test_venue_order_id("ORDER1");

        // First fill at timestamp 0 - saturating_sub should prevent underflow
        let fills = vec![FillSnapshot::new(
            0,
            OrderSide::Buy,
            dec!(0.01),
            dec!(4100.00),
            venue_order_id,
        )];

        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(0.02),
            avg_px: dec!(4100.00),
        };

        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Should add synthetic fill with timestamp 0 (not u64::MAX)
        match result {
            FillAdjustmentResult::AddSyntheticOpening { synthetic_fill, .. } => {
                assert_eq!(synthetic_fill.ts_event, 0); // saturating_sub(1) from 0 = 0
            }
            _ => panic!("Expected AddSyntheticOpening, was {result:?}"),
        }
    }

    #[rstest]
    fn test_adjust_fills_with_flip_scenario() {
        let venue_order_id1 = create_test_venue_order_id("ORDER1");
        let venue_order_id2 = create_test_venue_order_id("ORDER2");

        // Long 10 @ 100, then Sell 20 @ 105 -> flip to Short 10 @ 105
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id1),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(20), dec!(105), venue_order_id2), // Flip
        ];

        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Sell,
            qty: dec!(10),
            avg_px: dec!(105),
        };

        let instrument = instrument();
        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Should recognize the flip and match correctly
        match result {
            FillAdjustmentResult::NoAdjustment => {
                // Verify simulation matches
                let (qty, value) = simulate_position(&fills);
                assert_eq!(qty, dec!(-10));
                let avg = value / qty.abs();
                assert_eq!(avg, dec!(105));
            }
            _ => panic!("Expected NoAdjustment for matching flip, was {result:?}"),
        }
    }

    #[rstest]
    fn test_detect_zero_crossings_complex_lifecycle() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        // Complex scenario with multiple lifecycles
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(50), dec!(1.25), venue_order_id), // Reduce
            FillSnapshot::new(3000, OrderSide::Sell, dec!(100), dec!(1.30), venue_order_id), // Flip to -50
            FillSnapshot::new(4000, OrderSide::Buy, dec!(50), dec!(1.28), venue_order_id), // Close to zero
            FillSnapshot::new(5000, OrderSide::Buy, dec!(75), dec!(1.22), venue_order_id), // Open long
            FillSnapshot::new(6000, OrderSide::Sell, dec!(150), dec!(1.24), venue_order_id), // Flip to -75
        ];

        let crossings = detect_zero_crossings(&fills);
        assert_eq!(crossings.len(), 3);
        assert_eq!(crossings[0], 3000); // First flip
        assert_eq!(crossings[1], 4000); // Close to zero
        assert_eq!(crossings[2], 6000); // Second flip
    }

    #[rstest]
    fn test_reconciliation_price_partial_close() {
        let venue_order_id = create_test_venue_order_id("ORDER1");
        // Partial close scenario: 100 @ 1.20 to 50 @ 1.20
        let recon_px =
            calculate_reconciliation_price(dec!(100), Some(dec!(1.20)), dec!(50), Some(dec!(1.20)))
                .expect("reconciliation price");

        // Simulate partial close
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(100), dec!(1.20), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(50), recon_px, venue_order_id),
        ];

        let (final_qty, final_value) = simulate_position(&fills);
        assert_eq!(final_qty, dec!(50));
        let final_avg = final_value / final_qty.abs();
        assert_eq!(final_avg, dec!(1.20), "Average should be maintained");
    }

    #[rstest]
    fn test_detect_zero_crossings_identical_timestamps() {
        let venue_order_id1 = create_test_venue_order_id("ORDER1");
        let venue_order_id2 = create_test_venue_order_id("ORDER2");

        // Two fills with identical timestamps - should process deterministically
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id1),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(102), venue_order_id1),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(5), dec!(103), venue_order_id2), // Same ts
        ];

        let crossings = detect_zero_crossings(&fills);

        // Position: +10 -> +5 -> 0 (zero crossing at last fill)
        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0], 2000);

        // Verify final position is flat
        let (qty, _) = simulate_position(&fills);
        assert_eq!(qty, dec!(0));
    }

    #[rstest]
    fn test_detect_zero_crossings_five_lifecycles() {
        let venue_order_id = create_test_venue_order_id("ORDER1");

        // Five complete position lifecycles: open->close repeated 5 times
        let fills = vec![
            // Lifecycle 1: Long
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(101), venue_order_id),
            // Lifecycle 2: Short
            FillSnapshot::new(3000, OrderSide::Sell, dec!(20), dec!(102), venue_order_id),
            FillSnapshot::new(4000, OrderSide::Buy, dec!(20), dec!(101), venue_order_id),
            // Lifecycle 3: Long
            FillSnapshot::new(5000, OrderSide::Buy, dec!(15), dec!(103), venue_order_id),
            FillSnapshot::new(6000, OrderSide::Sell, dec!(15), dec!(104), venue_order_id),
            // Lifecycle 4: Short
            FillSnapshot::new(7000, OrderSide::Sell, dec!(25), dec!(105), venue_order_id),
            FillSnapshot::new(8000, OrderSide::Buy, dec!(25), dec!(104), venue_order_id),
            // Lifecycle 5: Long (still open)
            FillSnapshot::new(9000, OrderSide::Buy, dec!(30), dec!(106), venue_order_id),
        ];

        let crossings = detect_zero_crossings(&fills);

        // Should detect 4 zero-crossings (positions closing to flat)
        assert_eq!(crossings.len(), 4);
        assert_eq!(crossings[0], 2000);
        assert_eq!(crossings[1], 4000);
        assert_eq!(crossings[2], 6000);
        assert_eq!(crossings[3], 8000);

        // Final position should be +30
        let (qty, _) = simulate_position(&fills);
        assert_eq!(qty, dec!(30));
    }

    #[rstest]
    fn test_adjust_fills_five_zero_crossings(instrument: InstrumentAny) {
        let venue_order_id = create_test_venue_order_id("ORDER1");

        // Complex scenario: 4 complete lifecycles + current open position
        let fills = vec![
            // Old lifecycles (should be filtered out)
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(101), venue_order_id),
            FillSnapshot::new(3000, OrderSide::Sell, dec!(20), dec!(102), venue_order_id),
            FillSnapshot::new(4000, OrderSide::Buy, dec!(20), dec!(101), venue_order_id),
            FillSnapshot::new(5000, OrderSide::Buy, dec!(15), dec!(103), venue_order_id),
            FillSnapshot::new(6000, OrderSide::Sell, dec!(15), dec!(104), venue_order_id),
            FillSnapshot::new(7000, OrderSide::Sell, dec!(25), dec!(105), venue_order_id),
            FillSnapshot::new(8000, OrderSide::Buy, dec!(25), dec!(104), venue_order_id),
            // Current lifecycle (should be kept)
            FillSnapshot::new(9000, OrderSide::Buy, dec!(30), dec!(106), venue_order_id),
        ];

        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(30),
            avg_px: dec!(106),
        };

        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Should filter to current lifecycle only (after last zero-crossing at 8000)
        match result {
            FillAdjustmentResult::FilterToCurrentLifecycle {
                last_zero_crossing_ts,
                current_lifecycle_fills,
            } => {
                assert_eq!(last_zero_crossing_ts, 8000);
                assert_eq!(current_lifecycle_fills.len(), 1);
                assert_eq!(current_lifecycle_fills[0].ts_event, 9000);
                assert_eq!(current_lifecycle_fills[0].qty, dec!(30));
            }
            _ => panic!("Expected FilterToCurrentLifecycle, was {result:?}"),
        }
    }

    #[rstest]
    fn test_adjust_fills_alternating_long_short_positions(instrument: InstrumentAny) {
        let venue_order_id = create_test_venue_order_id("ORDER1");

        // Alternating: Long -> Short -> Long -> Short -> Long
        // These are flips (sign changes) but never go to exactly zero
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(20), dec!(102), venue_order_id), // Flip to -10
            FillSnapshot::new(3000, OrderSide::Buy, dec!(20), dec!(101), venue_order_id), // Flip to +10
            FillSnapshot::new(4000, OrderSide::Sell, dec!(20), dec!(103), venue_order_id), // Flip to -10
            FillSnapshot::new(5000, OrderSide::Buy, dec!(20), dec!(102), venue_order_id), // Flip to +10
        ];

        // Current position: +10 @ 102
        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(10),
            avg_px: dec!(102),
        };

        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Position never went flat (0), just flipped sides. This is treated as one
        // continuous lifecycle since no explicit close occurred. The final position
        // matches so no adjustment needed.
        assert!(
            matches!(result, FillAdjustmentResult::NoAdjustment),
            "Expected NoAdjustment (continuous lifecycle with matching position), was {result:?}"
        );
    }

    #[rstest]
    fn test_adjust_fills_with_flat_crossings(instrument: InstrumentAny) {
        let venue_order_id = create_test_venue_order_id("ORDER1");

        // Proper lifecycle boundaries with flat crossings (position goes to exactly 0)
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), venue_order_id),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), venue_order_id), // Close to 0
            FillSnapshot::new(3000, OrderSide::Sell, dec!(10), dec!(101), venue_order_id), // New short
            FillSnapshot::new(4000, OrderSide::Buy, dec!(10), dec!(99), venue_order_id), // Close to 0
            FillSnapshot::new(5000, OrderSide::Buy, dec!(10), dec!(98), venue_order_id), // New long
        ];

        // Current position: +10 @ 98
        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(10),
            avg_px: dec!(98),
        };

        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Position went flat at ts=2000 and ts=4000
        // Current lifecycle starts after last flat (4000)
        match result {
            FillAdjustmentResult::FilterToCurrentLifecycle {
                last_zero_crossing_ts,
                current_lifecycle_fills,
            } => {
                assert_eq!(last_zero_crossing_ts, 4000);
                assert_eq!(current_lifecycle_fills.len(), 1);
                assert_eq!(current_lifecycle_fills[0].ts_event, 5000);
                assert_eq!(current_lifecycle_fills[0].qty, dec!(10));
            }
            _ => panic!("Expected FilterToCurrentLifecycle, was {result:?}"),
        }
    }

    #[rstest]
    fn test_replace_current_lifecycle_uses_first_venue_order_id(instrument: InstrumentAny) {
        let order_id_1 = create_test_venue_order_id("ORDER1");
        let order_id_2 = create_test_venue_order_id("ORDER2");
        let order_id_3 = create_test_venue_order_id("ORDER3");

        // Previous lifecycle closes, then current lifecycle has fills from multiple orders
        let fills = vec![
            FillSnapshot::new(1000, OrderSide::Buy, dec!(10), dec!(100), order_id_1),
            FillSnapshot::new(2000, OrderSide::Sell, dec!(10), dec!(102), order_id_1), // Close to 0
            // Current lifecycle: fills from different venue order IDs
            FillSnapshot::new(3000, OrderSide::Buy, dec!(5), dec!(103), order_id_2),
            FillSnapshot::new(4000, OrderSide::Buy, dec!(5), dec!(104), order_id_3),
        ];

        // Venue position differs from simulated (+10 @ 103.5) to trigger replacement
        let venue_position = VenuePositionSnapshot {
            side: OrderSide::Buy,
            qty: dec!(15),
            avg_px: dec!(105),
        };

        let result =
            adjust_fills_for_partial_window(&fills, &venue_position, &instrument, dec!(0.0001));

        // Should replace with synthetic fill using first fill's venue_order_id (order_id_2)
        match result {
            FillAdjustmentResult::ReplaceCurrentLifecycle {
                synthetic_fill,
                first_venue_order_id,
            } => {
                assert_eq!(first_venue_order_id, order_id_2);
                assert_eq!(synthetic_fill.venue_order_id, order_id_2);
                assert_eq!(synthetic_fill.qty, dec!(15));
                assert_eq!(synthetic_fill.px, dec!(105));
            }
            _ => panic!("Expected ReplaceCurrentLifecycle, was {result:?}"),
        }
    }

    fn make_test_report(
        instrument_id: InstrumentId,
        order_type: OrderType,
        status: OrderStatus,
        filled_qty: &str,
        post_only: bool,
    ) -> OrderStatusReport {
        let account_id = AccountId::from("TEST-001");
        let mut report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            VenueOrderId::from("V-001"),
            OrderSide::Buy,
            order_type,
            TimeInForce::Gtc,
            status,
            Quantity::from("1.0"),
            Quantity::from(filled_qty),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("100.00"))
        .with_avg_px(100.0)
        .unwrap();
        report.post_only = post_only;
        report
    }

    #[rstest]
    #[case::accepted(OrderStatus::Accepted, "0", 1, "Accepted")]
    #[case::triggered(OrderStatus::Triggered, "0", 1, "Accepted")]
    #[case::canceled(OrderStatus::Canceled, "0", 2, "Canceled")]
    #[case::expired(OrderStatus::Expired, "0", 2, "Expired")]
    #[case::filled(OrderStatus::Filled, "1.0", 2, "Filled")]
    #[case::partially_filled(OrderStatus::PartiallyFilled, "0.5", 2, "Filled")]
    #[case::rejected(OrderStatus::Rejected, "0", 1, "Rejected")]
    fn test_external_order_status_event_generation(
        #[case] status: OrderStatus,
        #[case] filled_qty: &str,
        #[case] expected_events: usize,
        #[case] last_event_type: &str,
    ) {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .price(Price::from("100.00"))
            .build();
        let report = make_test_report(instrument.id(), OrderType::Limit, status, filled_qty, false);

        let events = generate_external_order_status_events(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            UnixNanos::from(2_000_000),
        );

        assert_eq!(events.len(), expected_events, "status={status}");
        let last = events.last().unwrap();
        let actual_type = match last {
            OrderEventAny::Accepted(_) => "Accepted",
            OrderEventAny::Canceled(_) => "Canceled",
            OrderEventAny::Expired(_) => "Expired",
            OrderEventAny::Filled(_) => "Filled",
            OrderEventAny::Rejected(_) => "Rejected",
            _ => "Other",
        };
        assert_eq!(actual_type, last_event_type, "status={status}");
    }

    #[rstest]
    #[case::market(OrderType::Market, false, LiquiditySide::Taker)]
    #[case::stop_market(OrderType::StopMarket, false, LiquiditySide::Taker)]
    #[case::trailing_stop_market(OrderType::TrailingStopMarket, false, LiquiditySide::Taker)]
    #[case::limit_post_only(OrderType::Limit, true, LiquiditySide::Maker)]
    #[case::limit_default(OrderType::Limit, false, LiquiditySide::NoLiquiditySide)]
    fn test_inferred_fill_liquidity_side(
        #[case] order_type: OrderType,
        #[case] post_only: bool,
        #[case] expected: LiquiditySide,
    ) {
        let instrument = crypto_perpetual_ethusdt();
        let order = match order_type {
            OrderType::Limit => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1.0"))
                .price(Price::from("100.00"))
                .build(),
            OrderType::StopMarket => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1.0"))
                .trigger_price(Price::from("100.00"))
                .build(),
            OrderType::TrailingStopMarket => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1.0"))
                .trigger_price(Price::from("100.00"))
                .trailing_offset(dec!(1.0))
                .build(),
            _ => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1.0"))
                .build(),
        };
        let report = make_test_report(
            instrument.id(),
            order_type,
            OrderStatus::Filled,
            "1.0",
            post_only,
        );

        let fill = create_inferred_fill(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            UnixNanos::from(2_000_000),
        );

        let filled = match fill.unwrap() {
            OrderEventAny::Filled(f) => f,
            _ => panic!("Expected Filled event"),
        };
        assert_eq!(
            filled.liquidity_side, expected,
            "order_type={order_type}, post_only={post_only}"
        );
    }

    #[rstest]
    fn test_inferred_fill_no_price_returns_none() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .build();

        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument.id(),
            None,
            VenueOrderId::from("V-001"),
            OrderSide::Buy,
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        );

        let fill = create_inferred_fill(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            UnixNanos::from(2_000_000),
        );

        assert!(fill.is_none());
    }

    // Tests for reconcile_fill_report

    fn create_test_fill_report(
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        last_qty: Quantity,
        last_px: Price,
    ) -> FillReport {
        FillReport::new(
            AccountId::from("TEST-001"),
            instrument_id,
            venue_order_id,
            trade_id,
            OrderSide::Buy,
            last_qty,
            last_px,
            Money::new(0.10, Currency::USD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
    }

    #[rstest]
    fn test_reconcile_fill_report_success(instrument: InstrumentAny) {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .build();

        let fill_report = create_test_fill_report(
            instrument.id(),
            VenueOrderId::from("V-001"),
            TradeId::from("T-001"),
            Quantity::from("50"),
            Price::from("1.00000"),
        );

        let result = reconcile_fill_report(
            &order,
            &fill_report,
            &instrument,
            UnixNanos::from(2_000_000),
            false,
        );

        assert!(result.is_some());
        if let Some(OrderEventAny::Filled(filled)) = result {
            assert_eq!(filled.last_qty, Quantity::from("50"));
            assert_eq!(filled.last_px, Price::from("1.00000"));
            assert_eq!(filled.trade_id, TradeId::from("T-001"));
            assert!(filled.reconciliation);
        } else {
            panic!("Expected OrderFilled event");
        }
    }

    #[rstest]
    fn test_reconcile_fill_report_duplicate_detected(instrument: InstrumentAny) {
        // Create an order
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .build();

        let account_id = AccountId::from("TEST-001");
        let venue_order_id = VenueOrderId::from("V-001");
        let trade_id = TradeId::from("T-001");

        // Submit the order first
        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id,
            UUID4::new(),
            UnixNanos::from(500_000),
            UnixNanos::from(500_000),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        // Accept the order
        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            UUID4::new(),
            UnixNanos::from(600_000),
            UnixNanos::from(600_000),
            false,
        );
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        // Now apply a fill to the order
        let filled_event = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            trade_id,
            OrderSide::Buy,
            order.order_type(),
            Quantity::from("50"),
            Price::from("1.00000"),
            Currency::USD(),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            false,
            None,
            None,
        );
        order.apply(OrderEventAny::Filled(filled_event)).unwrap();

        // Now try to reconcile the same fill - should be rejected as duplicate
        let fill_report = create_test_fill_report(
            instrument.id(),
            venue_order_id,
            trade_id, // Same trade_id
            Quantity::from("50"),
            Price::from("1.00000"),
        );

        let result = reconcile_fill_report(
            &order,
            &fill_report,
            &instrument,
            UnixNanos::from(2_000_000),
            false,
        );

        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconcile_fill_report_overfill_rejected(instrument: InstrumentAny) {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .build();

        // Fill for 150 would overfill a 100 qty order
        let fill_report = create_test_fill_report(
            instrument.id(),
            VenueOrderId::from("V-001"),
            TradeId::from("T-001"),
            Quantity::from("150"),
            Price::from("1.00000"),
        );

        let result = reconcile_fill_report(
            &order,
            &fill_report,
            &instrument,
            UnixNanos::from(2_000_000),
            false, // Don't allow overfills
        );

        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconcile_fill_report_overfill_allowed(instrument: InstrumentAny) {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .build();

        let fill_report = create_test_fill_report(
            instrument.id(),
            VenueOrderId::from("V-001"),
            TradeId::from("T-001"),
            Quantity::from("150"),
            Price::from("1.00000"),
        );

        let result = reconcile_fill_report(
            &order,
            &fill_report,
            &instrument,
            UnixNanos::from(2_000_000),
            true, // Allow overfills
        );

        // Should produce a fill event when overfills are allowed
        assert!(result.is_some());
    }

    // Tests for check_position_reconciliation

    #[rstest]
    fn test_check_position_reconciliation_both_flat() {
        let report = PositionStatusReport::new(
            AccountId::from("TEST-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSideSpecified::Flat,
            Quantity::from("0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            None,
        );

        let result = check_position_reconciliation(&report, dec!(0), Some(5));
        assert!(result);
    }

    #[rstest]
    fn test_check_position_reconciliation_exact_match_long() {
        let report = PositionStatusReport::new(
            AccountId::from("TEST-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSideSpecified::Long,
            Quantity::from("100"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            None,
        );

        let result = check_position_reconciliation(&report, dec!(100), Some(0));
        assert!(result);
    }

    #[rstest]
    fn test_check_position_reconciliation_exact_match_short() {
        let report = PositionStatusReport::new(
            AccountId::from("TEST-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSideSpecified::Short,
            Quantity::from("50"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            None,
        );

        let result = check_position_reconciliation(&report, dec!(-50), Some(0));
        assert!(result);
    }

    #[rstest]
    fn test_check_position_reconciliation_within_tolerance() {
        let report = PositionStatusReport::new(
            AccountId::from("TEST-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSideSpecified::Long,
            Quantity::from("100.00001"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            None,
        );

        // Cached qty is slightly different but within tolerance
        let result = check_position_reconciliation(&report, dec!(100.00000), Some(5));
        assert!(result);
    }

    #[rstest]
    fn test_check_position_reconciliation_discrepancy() {
        let report = PositionStatusReport::new(
            AccountId::from("TEST-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSideSpecified::Long,
            Quantity::from("100"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            None,
        );

        // Cached qty is significantly different
        let result = check_position_reconciliation(&report, dec!(50), Some(0));
        assert!(!result);
    }

    // Tests for is_within_single_unit_tolerance

    #[rstest]
    fn test_is_within_single_unit_tolerance_exact_match() {
        assert!(is_within_single_unit_tolerance(dec!(100), dec!(100), 0));
        assert!(is_within_single_unit_tolerance(
            dec!(100.12345),
            dec!(100.12345),
            5
        ));
    }

    #[rstest]
    fn test_is_within_single_unit_tolerance_integer_precision() {
        // Integer precision requires exact match
        assert!(is_within_single_unit_tolerance(dec!(100), dec!(100), 0));
        assert!(!is_within_single_unit_tolerance(dec!(100), dec!(101), 0));
    }

    #[rstest]
    fn test_is_within_single_unit_tolerance_fractional_precision() {
        // With precision 2, tolerance is 0.01
        assert!(is_within_single_unit_tolerance(dec!(100), dec!(100.01), 2));
        assert!(is_within_single_unit_tolerance(dec!(100), dec!(99.99), 2));
        assert!(!is_within_single_unit_tolerance(dec!(100), dec!(100.02), 2));
    }

    #[rstest]
    fn test_is_within_single_unit_tolerance_high_precision() {
        // With precision 5, tolerance is 0.00001
        assert!(is_within_single_unit_tolerance(
            dec!(100),
            dec!(100.00001),
            5
        ));
        assert!(is_within_single_unit_tolerance(
            dec!(100),
            dec!(99.99999),
            5
        ));
        assert!(!is_within_single_unit_tolerance(
            dec!(100),
            dec!(100.00002),
            5
        ));
    }

    fn create_test_order_status_report(
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        order_type: OrderType,
        order_status: OrderStatus,
        quantity: Quantity,
        filled_qty: Quantity,
    ) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("SIM-001"),
            instrument_id,
            Some(client_order_id),
            venue_order_id,
            OrderSide::Buy,
            order_type,
            TimeInForce::Gtc,
            order_status,
            quantity,
            filled_qty,
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
    }

    #[rstest]
    #[case::identical_limit_order(
        OrderType::Limit,
        Quantity::from(100),
        Some(Price::from("1.00000")),
        None,
        Quantity::from(100),
        Some(Price::from("1.00000")),
        None,
        false
    )]
    #[case::quantity_changed(
        OrderType::Limit,
        Quantity::from(100),
        Some(Price::from("1.00000")),
        None,
        Quantity::from(150),
        Some(Price::from("1.00000")),
        None,
        true
    )]
    #[case::limit_price_changed(
        OrderType::Limit,
        Quantity::from(100),
        Some(Price::from("1.00000")),
        None,
        Quantity::from(100),
        Some(Price::from("1.00100")),
        None,
        true
    )]
    #[case::stop_trigger_changed(
        OrderType::StopMarket,
        Quantity::from(100),
        None,
        Some(Price::from("0.99000")),
        Quantity::from(100),
        None,
        Some(Price::from("0.98000")),
        true
    )]
    #[case::stop_limit_trigger_changed(
        OrderType::StopLimit,
        Quantity::from(100),
        Some(Price::from("1.00000")),
        Some(Price::from("0.99000")),
        Quantity::from(100),
        Some(Price::from("1.00000")),
        Some(Price::from("0.98000")),
        true
    )]
    #[case::stop_limit_price_changed(
        OrderType::StopLimit,
        Quantity::from(100),
        Some(Price::from("1.00000")),
        Some(Price::from("0.99000")),
        Quantity::from(100),
        Some(Price::from("1.00100")),
        Some(Price::from("0.99000")),
        true
    )]
    #[case::market_order_no_update(
        OrderType::Market,
        Quantity::from(100),
        None,
        None,
        Quantity::from(100),
        None,
        None,
        false
    )]
    fn test_should_reconciliation_update(
        instrument: InstrumentAny,
        #[case] order_type: OrderType,
        #[case] order_qty: Quantity,
        #[case] order_price: Option<Price>,
        #[case] order_trigger: Option<Price>,
        #[case] report_qty: Quantity,
        #[case] report_price: Option<Price>,
        #[case] report_trigger: Option<Price>,
        #[case] expected: bool,
    ) {
        let client_order_id = ClientOrderId::from("O-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let mut order = match (order_price, order_trigger) {
            (Some(price), Some(trigger)) => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .client_order_id(client_order_id)
                .side(OrderSide::Buy)
                .quantity(order_qty)
                .price(price)
                .trigger_price(trigger)
                .build(),
            (Some(price), None) => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .client_order_id(client_order_id)
                .side(OrderSide::Buy)
                .quantity(order_qty)
                .price(price)
                .build(),
            (None, Some(trigger)) => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .client_order_id(client_order_id)
                .side(OrderSide::Buy)
                .quantity(order_qty)
                .trigger_price(trigger)
                .build(),
            (None, None) => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .client_order_id(client_order_id)
                .side(OrderSide::Buy)
                .quantity(order_qty)
                .build(),
        };

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let mut report = create_test_order_status_report(
            client_order_id,
            venue_order_id,
            instrument.id(),
            order_type,
            OrderStatus::Accepted,
            report_qty,
            Quantity::from(0),
        );
        report.price = report_price;
        report.trigger_price = report_trigger;

        assert_eq!(should_reconciliation_update(&order, &report), expected);
    }

    #[rstest]
    fn test_reconcile_order_report_already_in_sync(instrument: InstrumentAny) {
        let client_order_id = ClientOrderId::from("O-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let mut report = create_test_order_status_report(
            client_order_id,
            venue_order_id,
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Accepted,
            Quantity::from(100),
            Quantity::from(0),
        );
        report.price = Some(Price::from("1.00000"));

        let result =
            reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_reconcile_order_report_generates_canceled(instrument: InstrumentAny) {
        let client_order_id = ClientOrderId::from("O-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let report = create_test_order_status_report(
            client_order_id,
            venue_order_id,
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Canceled,
            Quantity::from(100),
            Quantity::from(0),
        );

        let result =
            reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), OrderEventAny::Canceled(_)));
    }

    #[rstest]
    fn test_reconcile_order_report_generates_expired(instrument: InstrumentAny) {
        let client_order_id = ClientOrderId::from("O-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let report = create_test_order_status_report(
            client_order_id,
            venue_order_id,
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Expired,
            Quantity::from(100),
            Quantity::from(0),
        );

        let result =
            reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), OrderEventAny::Expired(_)));
    }

    #[rstest]
    fn test_reconcile_order_report_generates_rejected(instrument: InstrumentAny) {
        let client_order_id = ClientOrderId::from("O-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let mut report = create_test_order_status_report(
            client_order_id,
            venue_order_id,
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Rejected,
            Quantity::from(100),
            Quantity::from(0),
        );
        report.cancel_reason = Some("INSUFFICIENT_MARGIN".to_string());

        let result =
            reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
        assert!(result.is_some());
        if let OrderEventAny::Rejected(rejected) = result.unwrap() {
            assert_eq!(rejected.reason.as_str(), "INSUFFICIENT_MARGIN");
            assert_eq!(rejected.reconciliation, 1);
        } else {
            panic!("Expected Rejected event");
        }
    }

    #[rstest]
    fn test_reconcile_order_report_generates_updated(instrument: InstrumentAny) {
        let client_order_id = ClientOrderId::from("O-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        // Report with changed price - same status, same filled_qty
        let mut report = create_test_order_status_report(
            client_order_id,
            venue_order_id,
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Accepted,
            Quantity::from(100),
            Quantity::from(0),
        );
        report.price = Some(Price::from("1.00100"));

        let result =
            reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), OrderEventAny::Updated(_)));
    }

    #[rstest]
    fn test_reconcile_order_report_generates_fill_for_qty_mismatch(instrument: InstrumentAny) {
        let client_order_id = ClientOrderId::from("O-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        // Report shows 50 filled but order has 0
        let mut report = create_test_order_status_report(
            client_order_id,
            venue_order_id,
            instrument.id(),
            OrderType::Limit,
            OrderStatus::PartiallyFilled,
            Quantity::from(100),
            Quantity::from(50),
        );
        report.avg_px = Some(dec!(1.0));

        let result =
            reconcile_order_report(&order, &report, Some(&instrument), UnixNanos::default());
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), OrderEventAny::Filled(_)));
    }

    #[rstest]
    fn test_create_reconciliation_rejected_with_reason() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let client_order_id = ClientOrderId::from("O-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let result =
            create_reconciliation_rejected(&order, Some("MARGIN_CALL"), UnixNanos::from(1_000));
        assert!(result.is_some());
        if let OrderEventAny::Rejected(rejected) = result.unwrap() {
            assert_eq!(rejected.reason.as_str(), "MARGIN_CALL");
            assert_eq!(rejected.reconciliation, 1);
            assert_eq!(rejected.due_post_only, 0);
        } else {
            panic!("Expected Rejected event");
        }
    }

    #[rstest]
    fn test_create_reconciliation_rejected_without_reason() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let client_order_id = ClientOrderId::from("O-001");

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();

        let result = create_reconciliation_rejected(&order, None, UnixNanos::from(1_000));
        assert!(result.is_some());
        if let OrderEventAny::Rejected(rejected) = result.unwrap() {
            assert_eq!(rejected.reason.as_str(), "UNKNOWN");
        } else {
            panic!("Expected Rejected event");
        }
    }

    #[rstest]
    fn test_create_reconciliation_rejected_no_account_id() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let client_order_id = ClientOrderId::from("O-001");

        // Order without account_id (not yet submitted)
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .price(Price::from("1.00000"))
            .build();

        let result = create_reconciliation_rejected(&order, Some("TEST"), UnixNanos::from(1_000));
        assert!(result.is_none());
    }

    #[rstest]
    fn test_create_synthetic_venue_order_id_format() {
        let ts = 1_000_000_u64;

        let id = create_synthetic_venue_order_id(ts);

        // Format: S-{hex_timestamp}-{uuid_prefix}
        assert!(id.as_str().starts_with("S-"));
        let parts: Vec<&str> = id.as_str().split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "S");
        assert!(!parts[1].is_empty());
    }

    #[rstest]
    fn test_create_synthetic_trade_id_format() {
        let ts = 1_000_000_u64;

        let id = create_synthetic_trade_id(ts);

        // Format: S-{hex_timestamp}-{uuid_prefix}
        assert!(id.as_str().starts_with("S-"));
        let parts: Vec<&str> = id.as_str().split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "S");
        assert!(!parts[1].is_empty());
    }

    #[rstest]
    fn test_create_inferred_fill_for_qty_zero_quantity_returns_none() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .build();

        let report = make_test_report(
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Filled,
            "10.0",
            false,
        );

        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::zero(0),
            UnixNanos::from(1_000_000),
        );

        assert!(result.is_none());
    }

    #[rstest]
    fn test_create_inferred_fill_for_qty_uses_report_avg_px() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .build();

        // Report with avg_px different from order price
        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument.id(),
            Some(order.client_order_id()),
            VenueOrderId::from("V-001"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("10.0"),
            Quantity::from("10.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_avg_px(105.50)
        .unwrap();

        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::from("5.0"),
            UnixNanos::from(2_000_000),
        );

        let filled = match result.unwrap() {
            OrderEventAny::Filled(f) => f,
            _ => panic!("Expected Filled event"),
        };

        // Should use avg_px from report (105.50), not order price (100.00)
        assert_eq!(filled.last_px, Price::from("105.50"));
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    }

    #[rstest]
    fn test_create_inferred_fill_for_qty_uses_report_price_when_no_avg_px() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .build();

        // Report with price but no avg_px
        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument.id(),
            Some(order.client_order_id()),
            VenueOrderId::from("V-001"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("10.0"),
            Quantity::from("10.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("102.00"));

        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::from("5.0"),
            UnixNanos::from(2_000_000),
        );

        let filled = match result.unwrap() {
            OrderEventAny::Filled(f) => f,
            _ => panic!("Expected Filled event"),
        };

        // Should use price from report (102.00)
        assert_eq!(filled.last_px, Price::from("102.00"));
    }

    #[rstest]
    fn test_create_inferred_fill_for_qty_uses_order_price_as_fallback() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .build();

        // Report with no price information
        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument.id(),
            Some(order.client_order_id()),
            VenueOrderId::from("V-001"),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("10.0"),
            Quantity::from("10.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        );

        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::from("5.0"),
            UnixNanos::from(2_000_000),
        );

        let filled = match result.unwrap() {
            OrderEventAny::Filled(f) => f,
            _ => panic!("Expected Filled event"),
        };

        // Should fall back to order price (100.00)
        assert_eq!(filled.last_px, Price::from("100.00"));
    }

    #[rstest]
    fn test_create_inferred_fill_for_qty_no_price_returns_none() {
        let instrument = crypto_perpetual_ethusdt();

        // Market order has no price
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .build();

        // Report with no price information
        let report = OrderStatusReport::new(
            AccountId::from("TEST-001"),
            instrument.id(),
            Some(order.client_order_id()),
            VenueOrderId::from("V-001"),
            OrderSide::Buy,
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("10.0"),
            Quantity::from("10.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        );

        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::from("5.0"),
            UnixNanos::from(2_000_000),
        );

        assert!(result.is_none());
    }

    #[rstest]
    #[case::market_order(OrderType::Market, false, LiquiditySide::Taker)]
    #[case::stop_market(OrderType::StopMarket, false, LiquiditySide::Taker)]
    #[case::trailing_stop_market(OrderType::TrailingStopMarket, false, LiquiditySide::Taker)]
    #[case::limit_post_only(OrderType::Limit, true, LiquiditySide::Maker)]
    #[case::limit_default(OrderType::Limit, false, LiquiditySide::NoLiquiditySide)]
    fn test_create_inferred_fill_for_qty_liquidity_side(
        #[case] order_type: OrderType,
        #[case] post_only: bool,
        #[case] expected: LiquiditySide,
    ) {
        let instrument = crypto_perpetual_ethusdt();
        let order = match order_type {
            OrderType::Limit => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("10.0"))
                .price(Price::from("100.00"))
                .post_only(post_only)
                .build(),
            OrderType::StopMarket => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("10.0"))
                .trigger_price(Price::from("100.00"))
                .build(),
            OrderType::TrailingStopMarket => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("10.0"))
                .trigger_price(Price::from("100.00"))
                .trailing_offset(Decimal::from(1))
                .build(),
            _ => OrderTestBuilder::new(order_type)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("10.0"))
                .build(),
        };

        let report = make_test_report(
            instrument.id(),
            order_type,
            OrderStatus::Filled,
            "10.0",
            post_only,
        );

        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::from("5.0"),
            UnixNanos::from(2_000_000),
        );

        let filled = match result.unwrap() {
            OrderEventAny::Filled(f) => f,
            _ => panic!("Expected Filled event"),
        };

        assert_eq!(
            filled.liquidity_side, expected,
            "order_type={order_type}, post_only={post_only}"
        );
    }

    #[rstest]
    fn test_create_inferred_fill_for_qty_trade_id_format() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .build();

        let report = make_test_report(
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Filled,
            "10.0",
            false,
        );

        let ts_now = UnixNanos::from(2_000_000);
        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::from("5.0"),
            ts_now,
        );

        let filled = match result.unwrap() {
            OrderEventAny::Filled(f) => f,
            _ => panic!("Expected Filled event"),
        };

        // Trade ID should be a valid UUID (36 characters with dashes)
        assert_eq!(filled.trade_id.as_str().len(), 36);
        assert!(filled.trade_id.as_str().contains('-'));
    }

    #[rstest]
    fn test_create_inferred_fill_for_qty_reconciliation_flag() {
        let instrument = crypto_perpetual_ethusdt();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.00"))
            .build();

        let report = make_test_report(
            instrument.id(),
            OrderType::Limit,
            OrderStatus::Filled,
            "10.0",
            false,
        );

        let result = create_inferred_fill_for_qty(
            &order,
            &report,
            &AccountId::from("TEST-001"),
            &InstrumentAny::CryptoPerpetual(instrument),
            Quantity::from("5.0"),
            UnixNanos::from(2_000_000),
        );

        let filled = match result.unwrap() {
            OrderEventAny::Filled(f) => f,
            _ => panic!("Expected Filled event"),
        };

        assert!(filled.reconciliation, "reconciliation flag should be true");
    }
}
