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

//! Execution state reconciliation.
//!
//! Pure functions for bringing the engine's local order, fill, and position state
//! into line with what the venue reports. Called at startup (mass status) and
//! continuously at runtime (open-order and position checks).
//!
//! Public entry points:
//! - [`process_mass_status_for_reconciliation`] - partial-window fill reconstruction
//! - [`reconcile_order_report`] - bring a cached order into line with a venue report
//! - [`reconcile_fill_report`] - apply a venue fill to a cached order, with dedup
//! - [`check_position_reconciliation`] - final qty and avg-px tolerance check
//!
//! Invariants maintained across all paths:
//! 1. Final position quantity matches the venue within instrument precision.
//! 2. Position average price matches within tolerance (default 0.01%).
//! 3. All generated fills preserve correct unrealized PnL.
//! 4. Synthetic `trade_id` and `venue_order_id` values are deterministic
//!    functions of the logical event, so restart replays dedupe.
//!
//! See `docs/concepts/live.md` for the operator-facing description.

mod ids;
mod orders;
mod positions;
mod types;

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod tests;

pub use ids::{
    create_inferred_reconciliation_trade_id, create_position_reconciliation_venue_order_id,
    create_synthetic_trade_id, create_synthetic_venue_order_id,
};
pub use orders::{
    create_incremental_inferred_fill, create_inferred_fill, create_inferred_fill_for_qty,
    create_reconciliation_accepted, create_reconciliation_canceled, create_reconciliation_expired,
    create_reconciliation_rejected, create_reconciliation_triggered, create_reconciliation_updated,
    generate_external_order_status_events, generate_reconciliation_order_events,
    reconcile_fill_report, reconcile_order_report, should_reconciliation_update,
};
pub use positions::{
    adjust_fills_for_partial_window, calculate_reconciliation_price, check_position_match,
    check_position_reconciliation, create_synthetic_fill_report, create_synthetic_order_report,
    detect_zero_crossings, is_within_single_unit_tolerance, process_mass_status_for_reconciliation,
    simulate_position,
};
pub use types::{FillAdjustmentResult, FillSnapshot, ReconciliationResult, VenuePositionSnapshot};
