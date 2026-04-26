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

//! Shared reconciliation value types.

use indexmap::IndexMap;
use nautilus_model::{
    enums::OrderSide,
    identifiers::VenueOrderId,
    reports::{FillReport, OrderStatusReport},
};
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

/// Result of processing fill reports for reconciliation.
#[derive(Debug, Clone)]
pub struct ReconciliationResult {
    /// Order status reports keyed by venue order ID.
    pub orders: IndexMap<VenueOrderId, OrderStatusReport>,
    /// Fill reports keyed by venue order ID.
    pub fills: IndexMap<VenueOrderId, Vec<FillReport>>,
}
