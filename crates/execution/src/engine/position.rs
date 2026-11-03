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

//! Position types the [`ExecutionEngine`](super::ExecutionEngine) publishes or carries between
//! the stages of a fill correction.

use std::rc::Rc;

use nautilus_common::cache::CacheSnapshotRef;
use nautilus_core::UnixNanos;
use nautilus_model::{
    position::Position,
    types::{Money, Quantity},
};

/// Position state snapshot published to the `snapshots.position.{position_id}` topic.
#[derive(Debug, Clone)]
pub struct PositionStateSnapshot {
    /// The position state at the time of the snapshot.
    pub position: Position,
    /// The unrealized PnL for the position, when a current quote is available.
    pub unrealized_pnl: Option<Money>,
    /// UNIX timestamp (nanoseconds) when the snapshot was taken.
    pub ts_snapshot: UnixNanos,
}

/// Callback that anchors cache snapshot metadata in an external store.
pub type SnapshotAnchorer = Rc<dyn Fn(CacheSnapshotRef) -> anyhow::Result<()>>;

/// A position rebuilt by a fill void, with the quantity that correction removed.
///
/// `absorbed_prior_cycles` is set when the voided fill sits outside the current NETTING cycle
/// or, with replay retention enabled, the rebuild rolls back to fills from an earlier cycle.
/// The archive frames then no longer match the corrected history. `closed_cycles_pnl` holds the PnL of
/// whatever cycles the corrected history does close before the current one, which settles those
/// frames. It is `None` when no closed cycle precedes the current one, including when the
/// corrected history never goes flat or its only closed cycle is the current one.
///
/// Known limitation: a second correction to the same trade that revises only the voided
/// commission without changing cycle membership leaves the flag unset, and the settled frame keeps
/// the realized PnL banked by the first. No in-tree emitter produces that shape, since
/// reconciliation always advances the quantity and the adapters void a fill once.
pub(super) struct CorrectedPosition {
    pub(super) position: Position,
    pub(super) corrected_qty: Quantity,
    pub(super) absorbed_prior_cycles: bool,
    pub(super) closed_cycles_pnl: Option<Money>,
}

pub(super) fn requires_snapshot_settlement(
    prior: &Position,
    rebuilt: &Position,
    voided_qty: Quantity,
    current_cycle_qty: Quantity,
    carry_replay_events_on_reopen: bool,
) -> bool {
    voided_qty > current_cycle_qty
        || (carry_replay_events_on_reopen
            && rebuilt.events.first().is_some_and(|opening| {
                !prior
                    .events
                    .iter()
                    .any(|fill| fill.event_id == opening.event_id)
            }))
}
