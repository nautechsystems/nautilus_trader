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

//! Reconciliation calculation functions for live trading.

use nautilus_model::{enums::OrderSide, identifiers::VenueOrderId, instruments::InstrumentAny};
use rust_decimal::Decimal;

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
    // Calculate the difference in quantity
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
            let oldest_avg_px = if oldest_qty != Decimal::ZERO {
                Some(oldest_value / oldest_qty.abs())
            } else {
                None
            };

            let reconciliation_price = calculate_reconciliation_price(
                oldest_qty,
                oldest_avg_px,
                venue_qty_signed,
                Some(venue_position.avg_px),
            );

            if let Some(opening_px) = reconciliation_price {
                // Calculate opening quantity needed
                let opening_qty = if oldest_qty != Decimal::ZERO {
                    // Work backwards: venue = opening + current fills
                    venue_qty_signed - oldest_qty
                } else {
                    venue_qty_signed
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::stubs::audusd_sim;
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

    // Tests for adjust_fills_for_partial_window

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
            _ => panic!("Expected FilterToCurrentLifecycle, was {:?}", result),
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
            _ => panic!("Expected ReplaceCurrentLifecycle, was {:?}", result),
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
            _ => panic!("Expected AddSyntheticOpening, was {:?}", result),
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
            _ => panic!("Expected AddSyntheticOpening, was {:?}", result),
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
            _ => panic!("Expected NoAdjustment for matching flip, was {:?}", result),
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
}
