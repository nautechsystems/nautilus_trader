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

//! Polymarket condition resolution tracking and reconciliation.

mod apply;
mod parsing;
mod summary;
mod watchlist;

#[allow(unused_imports)]
pub(crate) use self::{
    apply::{
        ResolveApplyBatchStats, ResolveBatchErrorMode, ResolveContext, apply_condition_resolution,
        fetch_and_apply_resolutions_by_condition_ids, merge_resolve_watch_entry,
    },
    parsing::{
        StrictResolvedMarket, build_resolved_market_from_clob_market, build_strict_resolved_market,
        parse_condition_ids_from_request_params, request_params_has_explicit_condition_selector,
    },
    summary::{
        PolymarketResolveRequestSummaryData, RESOLVE_REQUEST_TYPE_NAME, ResolveRequestSummary,
    },
    watchlist::{
        ResolveWatchEntry, ResolveWatchSelection, ResolveWatchSelectionMode, TrackedInstrument,
        collect_resolve_watch_selection, instrument_market_context, pause_resolve_watch_entries,
        update_resolve_watchlist_from_position_event, upsert_resolve_watch_entry_from_instrument,
    },
};
