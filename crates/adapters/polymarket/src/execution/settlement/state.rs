// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under the
//  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
//  either express or implied. See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Settlement record types for the Polymarket process-local settlement registry.

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide},
    events::OrderFilled,
    identifiers::{InstrumentId, TradeId, VenueOrderId},
    types::{Money, Price, Quantity},
};

use super::admission::AdmittedLeg;

/// Maximum number of settlement entries created after hydration before the registry faults the
/// client closed.
///
/// Records are retained for the lifetime of the initialized execution client and are never
/// silently evicted. Entries reconstructed from retained core events during hydration do not
/// count toward this limit, so a large retained history cannot fault the client at connect.
pub(crate) const MAX_SETTLEMENT_RECORDS: usize = 100_000;

/// Venue settlement disposition of a trade, tracked separately from per-leg core application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettlementState {
    /// Only provisional-status stream evidence has been observed.
    Provisional,
    /// The stream reported terminal `CONFIRMED` settlement.
    StreamConfirmed,
    /// Conflicting or failed stream evidence; awaiting targeted terminal REST resolution.
    Quarantined,
    /// An admitted terminal REST result established `CONFIRMED` settlement.
    RestConfirmed,
    /// An admitted terminal REST result established `FAILED` settlement.
    RestFailed,
}

impl SettlementState {
    /// Returns `true` once settlement was established by an admitted terminal REST result.
    pub(crate) const fn is_rest_terminal(self) -> bool {
        matches!(self, Self::RestConfirmed | Self::RestFailed)
    }
}

/// Core application state of one owned leg of a trade.
///
/// `Absent` covers legs never sent to core as well as legs whose fill was declined; the
/// settlement state distinguishes a tombstone (`RestFailed` with `Absent`) from a leg still
/// awaiting application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegApplication {
    /// No `OrderFilled` for this leg is applied or awaiting application in core.
    Absent,
    /// An `OrderFilled` was sent and the applied event or a decline is still awaited.
    FillPending,
    /// Core applied an `OrderFilled` for this leg.
    FillObserved,
    /// An `OrderFillVoided` was sent and the applied event or a decline is still awaited.
    VoidPending,
    /// Core applied an `OrderFillVoided` for this leg.
    VoidObserved,
}

/// One owned leg of a settlement record.
///
/// An `Absent` leg is `authorized` once the registry allows its `OrderFilled`, which may still
/// wait in the fill buffer for its order to register; a buffered fill emits only if the trade
/// still permits application when it drains. A `report_routed` leg was delivered through a fill
/// report for an order without captured context, so its application, if core performs it, is
/// observed through applied events rather than awaited.
#[derive(Debug)]
pub(crate) struct SettlementLeg {
    pub venue_order_id: VenueOrderId,
    pub trade_id: TradeId,
    pub instrument_id: InstrumentId,
    pub order_side: OrderSide,
    pub liquidity_side: LiquiditySide,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub commission: Money,
    pub ts_event: UnixNanos,
    pub application: LegApplication,
    /// The canonical applied fill as published by core, retained for void construction.
    pub applied_fill: Option<Box<OrderFilled>>,
    pub authorized: bool,
    pub report_routed: bool,
}

impl SettlementLeg {
    pub(crate) fn from_admitted(leg: &AdmittedLeg) -> Self {
        Self {
            venue_order_id: leg.venue_order_id,
            trade_id: leg.trade_id,
            instrument_id: leg.instrument_id,
            order_side: leg.order_side,
            liquidity_side: leg.liquidity_side,
            last_qty: leg.last_qty,
            last_px: leg.last_px,
            commission: leg.commission.clone(),
            ts_event: leg.ts_event,
            application: LegApplication::Absent,
            applied_fill: None,
            authorized: false,
            report_routed: false,
        }
    }

    /// Returns `true` when the leg was never sent to core and still requires an application.
    pub(crate) fn awaits_application(&self) -> bool {
        self.application == LegApplication::Absent && !self.authorized && !self.report_routed
    }
}

/// Effects the caller must execute after a registry transition.
#[derive(Debug)]
pub(crate) enum SettlementAction {
    /// Apply the leg by emitting an `OrderFilled` through the established emission machinery.
    ApplyLeg {
        venue_trade_id: String,
        leg: AdmittedLeg,
    },
    /// Emit an `OrderFillVoided` constructed from the canonical applied fill of the leg.
    VoidAppliedFill {
        venue_trade_id: String,
        fill: Box<OrderFilled>,
    },
}

/// A submitted order whose venue outcome is unknown, awaiting a targeted REST read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UncertainOrder {
    pub instrument_id: InstrumentId,
    pub noted_at: UnixNanos,
}

/// One synchronized settlement record per venue trade.
#[derive(Debug)]
pub(crate) struct SettlementRecord {
    pub venue_trade_id: String,
    pub settlement: SettlementState,
    pub legs: Vec<SettlementLeg>,
    /// The stream session epoch in which this trade was first admitted.
    pub admitted_session: u64,
    /// Sticky hard fault reason; a hard-faulted trade admits no new discretionary effects.
    pub hard_fault: Option<String>,
    /// The first admitted terminal REST result, retained for material comparison.
    pub terminal_rest_legs: Option<Vec<AdmittedLeg>>,
    /// Whether a targeted REST read is requested outside quarantine: contradicting stream
    /// evidence after a terminal REST result, or a provisional trade from an earlier stream
    /// session.
    pub refresh_requested: bool,
}

impl SettlementRecord {
    pub(crate) fn new(venue_trade_id: String, admitted_session: u64) -> Self {
        Self {
            venue_trade_id,
            settlement: SettlementState::Provisional,
            legs: Vec::new(),
            admitted_session,
            hard_fault: None,
            terminal_rest_legs: None,
            refresh_requested: false,
        }
    }

    /// Returns the leg bound to a trade ID emitted for a fill or void of this trade.
    pub(crate) fn leg_by_trade_id(&mut self, trade_id: &TradeId) -> Option<&mut SettlementLeg> {
        self.legs.iter_mut().find(|leg| &leg.trade_id == trade_id)
    }

    /// Returns `true` while authorized fills of this trade may still be emitted.
    pub(crate) fn permits_application(&self) -> bool {
        self.hard_fault.is_none()
            && matches!(
                self.settlement,
                SettlementState::Provisional
                    | SettlementState::StreamConfirmed
                    | SettlementState::RestConfirmed
            )
    }

    /// Returns `true` while the trade awaits a targeted terminal REST read: it is quarantined,
    /// later stream evidence contradicted its REST-settled outcome, or it stayed provisional
    /// across a stream session change.
    pub(crate) fn awaits_resolution(&self) -> bool {
        self.hard_fault.is_none()
            && (self.settlement == SettlementState::Quarantined || self.refresh_requested)
    }

    /// Returns `true` while the record holds evidence that reconciliation must not treat as
    /// covered: a hard fault, a targeted REST read still owed, a pending application, or a leg
    /// awaiting application before terminal settlement.
    pub(crate) fn is_unresolved(&self) -> bool {
        self.hard_fault.is_some()
            || self.awaits_resolution()
            || self.legs.iter().any(|leg| {
                matches!(
                    leg.application,
                    LegApplication::FillPending | LegApplication::VoidPending
                ) || (!self.settlement.is_rest_terminal()
                    && leg.application == LegApplication::Absent
                    && !leg.report_routed)
            })
    }
}
