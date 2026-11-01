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

//! Reconciliation instrument scope for Deribit report generation.

use ahash::AHashSet;
use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::identifiers::{InstrumentId, Symbol};
use ustr::Ustr;

use crate::common::consts::DERIBIT_VENUE;

/// Instruments a reconciliation read treats as in scope.
///
/// `None` means every instrument is in scope. A non-empty set is the only in-scope set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReconciliationScope {
    load_ids: Option<AHashSet<InstrumentId>>,
}

impl ReconciliationScope {
    /// Returns a scope in which every instrument is in scope.
    #[must_use]
    pub(crate) fn unrestricted() -> Self {
        Self { load_ids: None }
    }

    /// Builds a scope from execution `load_ids`.
    ///
    /// `None` and an empty slice are unrestricted. A non-empty slice is explicit.
    #[must_use]
    pub(crate) fn from_load_ids(load_ids: Option<&[InstrumentId]>) -> Self {
        match load_ids {
            Some(ids) if !ids.is_empty() => Self {
                load_ids: Some(ids.iter().copied().collect()),
            },
            _ => Self::unrestricted(),
        }
    }

    /// Returns whether a record for `instrument_id` belongs in this read.
    #[must_use]
    pub(crate) fn includes(&self, instrument_id: InstrumentId) -> bool {
        self.load_ids
            .as_ref()
            .is_none_or(|ids| ids.contains(&instrument_id))
    }
}

/// Reports from one reconciliation read, plus in-window historical instrument misses.
#[derive(Clone, Debug)]
pub(crate) struct ReportRead<T> {
    pub reports: Vec<T>,
    pub historical_misses: Vec<InstrumentId>,
}

impl<T> ReportRead<T> {
    /// Returns whether an in-scope historical record was omitted.
    #[must_use]
    pub(crate) fn historical_incomplete(&self) -> bool {
        !self.historical_misses.is_empty()
    }
}

/// Builds the Nautilus instrument id for a Deribit instrument name.
#[must_use]
pub(crate) fn instrument_id_from_name(instrument_name: Ustr) -> InstrumentId {
    InstrumentId::new(Symbol::from_ustr_unchecked(instrument_name), *DERIBIT_VENUE)
}

/// Returns whether a Deribit order state is closed or otherwise historical.
///
/// Unknown states are not historical. The order parser treats them as accepted, so a miss fails
/// the read rather than omitting a possibly open order.
#[must_use]
pub(crate) fn is_historical_order_state(order_state: &str) -> bool {
    matches!(
        order_state,
        "filled" | "rejected" | "cancelled" | "canceled" | "expired"
    )
}

/// Records one in-window historical instrument miss and warns once.
pub(crate) fn record_historical_miss(misses: &mut Vec<InstrumentId>, instrument_id: InstrumentId) {
    if misses.contains(&instrument_id) {
        return;
    }

    log::warn!("Instrument {instrument_id} missing from cache");
    misses.push(instrument_id);
}

/// Names the instruments omitted from a historical read.
#[must_use]
pub(crate) fn historical_miss_message(instrument_ids: &[InstrumentId]) -> String {
    let names = instrument_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("unresolved historical instruments: {names}")
}

/// Returns whether a timestamp falls inside the request window.
///
/// An absent bound does not constrain that side.
#[must_use]
pub(crate) fn timestamp_in_window(
    timestamp: UnixNanos,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> bool {
    match (start, end) {
        (Some(start), Some(end)) => timestamp >= start && timestamp <= end,
        (Some(start), None) => timestamp >= start,
        (None, Some(end)) => timestamp <= end,
        (None, None) => true,
    }
}

/// Returns whether a venue timestamp in milliseconds falls inside the request window.
///
/// A timestamp that cannot be represented is inside the window, so a historical miss still marks
/// bounded history incomplete.
#[must_use]
pub(crate) fn millis_in_window(
    timestamp_ms: u64,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> bool {
    let Some(timestamp) = timestamp_ms
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .map(UnixNanos::from)
    else {
        return true;
    };

    timestamp_in_window(timestamp, start, end)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn btc() -> InstrumentId {
        InstrumentId::from("BTC-PERPETUAL.DERIBIT")
    }

    fn eth() -> InstrumentId {
        InstrumentId::from("ETH-PERPETUAL.DERIBIT")
    }

    #[rstest]
    fn unrestricted_scope_includes_every_instrument() {
        let scope = ReconciliationScope::from_load_ids(None);
        let empty = ReconciliationScope::from_load_ids(Some(&[]));

        assert!(scope.includes(btc()));
        assert!(scope.includes(eth()));
        assert!(empty.includes(eth()));
    }

    #[rstest]
    fn explicit_scope_includes_only_named_instruments() {
        let scope = ReconciliationScope::from_load_ids(Some(&[btc()]));

        assert!(scope.includes(btc()));
        assert!(!scope.includes(eth()));
    }

    #[rstest]
    #[case("filled", true)]
    #[case("rejected", true)]
    #[case("cancelled", true)]
    #[case("canceled", true)]
    #[case("expired", true)]
    #[case("open", false)]
    #[case("untriggered", false)]
    #[case("unknown", false)]
    fn historical_order_state_matches_closed_states(#[case] state: &str, #[case] historical: bool) {
        assert_eq!(is_historical_order_state(state), historical);
    }

    #[rstest]
    #[case("")]
    #[case(" ")]
    #[case("BTC-PERPETUAL")]
    fn instrument_id_from_name_keeps_venue_name(#[case] name: &str) {
        let instrument_id = instrument_id_from_name(Ustr::from(name));

        assert_eq!(instrument_id.symbol.as_str(), name);
        assert_eq!(instrument_id.venue, *DERIBIT_VENUE);
    }

    #[rstest]
    fn timestamp_window_uses_inclusive_bounds() {
        let start = UnixNanos::from(1_000 * NANOSECONDS_IN_MILLISECOND);
        let end = UnixNanos::from(2_000 * NANOSECONDS_IN_MILLISECOND);

        assert!(millis_in_window(1_000, Some(start), Some(end)));
        assert!(millis_in_window(2_000, Some(start), Some(end)));
        assert!(!millis_in_window(999, Some(start), Some(end)));
        assert!(!millis_in_window(2_001, Some(start), Some(end)));
        assert!(millis_in_window(999, None, None));
    }
}
