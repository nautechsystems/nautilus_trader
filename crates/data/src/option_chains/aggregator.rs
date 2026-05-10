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

//! Per-series option chain aggregator for event accumulation and snapshots.

use std::collections::{BTreeMap, HashMap, HashSet};

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        QuoteTick,
        option_chain::{OptionChainSlice, OptionGreeks, OptionStrikeData, StrikeRange},
    },
    enums::OptionKind,
    identifiers::{InstrumentId, OptionSeriesId},
    types::Price,
};

use super::{
    AtmTracker,
    constants::{DEFAULT_REBALANCE_COOLDOWN_NS, DEFAULT_REBALANCE_HYSTERESIS},
};

/// Per-series aggregator that accumulates quotes and greeks between snapshots.
///
/// Owns mutable accumulator buffers and produces immutable `OptionChainSlice`
/// snapshots on each timer tick.
#[derive(Debug)]
pub struct OptionChainAggregator {
    /// The option series identifier for this aggregator.
    series_id: OptionSeriesId,
    /// Defines which strikes to include in the active set.
    strike_range: StrikeRange,
    /// Tracks the current ATM price from market data events.
    atm_tracker: AtmTracker,
    /// All instruments for this series. Grows dynamically when the exchange
    /// lists new strikes via [`Self::add_instrument`].
    instruments: HashMap<InstrumentId, (Price, OptionKind)>,
    /// Currently active instrument IDs (subset of `instruments`).
    active_ids: HashSet<InstrumentId>,
    /// The closest ATM strike at the time of the last rebalance.
    last_atm_strike: Option<Price>,
    /// Hysteresis band for ATM rebalancing.
    hysteresis: f64,
    /// Minimum nanoseconds between rebalances.
    cooldown_ns: u64,
    /// Timestamp of the last rebalance.
    last_rebalance_ns: Option<UnixNanos>,
    /// Maximum `ts_event` seen across all quote updates.
    max_ts_event: UnixNanos,
    /// Greeks received before the corresponding quote arrived.
    pending_greeks: HashMap<InstrumentId, OptionGreeks>,
    /// Call option accumulator buffer keyed by strike price.
    call_buffer: BTreeMap<Price, OptionStrikeData>,
    /// Put option accumulator buffer keyed by strike price.
    put_buffer: BTreeMap<Price, OptionStrikeData>,
}

impl OptionChainAggregator {
    /// Creates a new aggregator for the given series.
    ///
    /// `instruments` contains ALL instruments for the series. The initial
    /// `active_ids` subset is resolved from the strike range and the current
    /// ATM price (if available). When no ATM price is set for ATM-based
    /// ranges, all instruments are active.
    pub fn new(
        series_id: OptionSeriesId,
        strike_range: StrikeRange,
        atm_tracker: AtmTracker,
        instruments: HashMap<InstrumentId, (Price, OptionKind)>,
    ) -> Self {
        let all_strikes = Self::sorted_strikes(&instruments);
        let atm_price = atm_tracker.atm_price();
        let active_strikes: HashSet<Price> = strike_range
            .resolve(atm_price, &all_strikes)
            .into_iter()
            .collect();
        let active_ids: HashSet<InstrumentId> = instruments
            .iter()
            .filter(|(_, (strike, _))| active_strikes.contains(strike))
            .map(|(id, _)| *id)
            .collect();
        let last_atm_strike =
            atm_price.and_then(|atm| Self::find_closest_strike(&all_strikes, atm));

        Self {
            series_id,
            strike_range,
            atm_tracker,
            instruments,
            active_ids,
            last_atm_strike,
            hysteresis: DEFAULT_REBALANCE_HYSTERESIS,
            cooldown_ns: DEFAULT_REBALANCE_COOLDOWN_NS,
            last_rebalance_ns: None,
            max_ts_event: UnixNanos::default(),
            pending_greeks: HashMap::new(),
            call_buffer: BTreeMap::new(),
            put_buffer: BTreeMap::new(),
        }
    }

    /// Returns a mutable reference to the ATM tracker.
    pub fn atm_tracker_mut(&mut self) -> &mut AtmTracker {
        &mut self.atm_tracker
    }

    /// Returns the currently active instrument IDs.
    #[must_use]
    pub fn instrument_ids(&self) -> Vec<InstrumentId> {
        self.active_ids.iter().copied().collect()
    }

    /// Returns a reference to the active instrument ID set.
    #[must_use]
    pub fn active_ids(&self) -> &HashSet<InstrumentId> {
        &self.active_ids
    }

    /// Returns the series ID.
    #[must_use]
    pub fn series_id(&self) -> OptionSeriesId {
        self.series_id
    }

    /// Returns `true` if the given timestamp is at or past the series expiration.
    #[must_use]
    pub fn is_expired(&self, now_ns: UnixNanos) -> bool {
        now_ns >= self.series_id.expiration_ns
    }

    /// Returns a reference to the full instrument set.
    #[must_use]
    pub fn instruments(&self) -> &HashMap<InstrumentId, (Price, OptionKind)> {
        &self.instruments
    }

    /// Returns all instrument IDs in the full set.
    #[must_use]
    pub fn all_instrument_ids(&self) -> Vec<InstrumentId> {
        self.instruments.keys().copied().collect()
    }

    /// Returns `true` if the instrument catalog is empty.
    #[must_use]
    pub fn is_catalog_empty(&self) -> bool {
        self.instruments.is_empty()
    }

    /// Permanently removes an instrument from the catalog.
    ///
    /// Removes from `instruments`, `active_ids`, `pending_greeks`, and cleans
    /// buffer entries (only if no other instrument shares the same strike+kind).
    /// Returns `true` if the instrument was found and removed.
    #[must_use]
    pub fn remove_instrument(&mut self, instrument_id: &InstrumentId) -> bool {
        let Some((strike, kind)) = self.instruments.remove(instrument_id) else {
            return false;
        };

        self.active_ids.remove(instrument_id);
        self.pending_greeks.remove(instrument_id);

        // Only remove buffer entry if no sibling instrument shares the same strike+kind
        let has_sibling = self
            .instruments
            .values()
            .any(|(s, k)| *s == strike && *k == kind);

        if !has_sibling {
            let buffer = match kind {
                OptionKind::Call => &mut self.call_buffer,
                OptionKind::Put => &mut self.put_buffer,
            };
            buffer.remove(&strike);
        }

        true
    }

    /// Returns a reference to the ATM tracker.
    #[must_use]
    pub fn atm_tracker(&self) -> &AtmTracker {
        &self.atm_tracker
    }

    /// Recomputes the active instrument set from the current ATM price.
    ///
    /// Returns the new active instrument IDs. Used during bootstrap when the
    /// first ATM price arrives after deferred subscription setup.
    pub fn recompute_active_set(&mut self) -> Vec<InstrumentId> {
        let atm_price = self.atm_tracker.atm_price();
        let all_strikes = Self::sorted_strikes(&self.instruments);
        let active_strikes: HashSet<Price> = self
            .strike_range
            .resolve(atm_price, &all_strikes)
            .into_iter()
            .collect();
        self.active_ids = self
            .instruments
            .iter()
            .filter(|(_, (strike, _))| active_strikes.contains(strike))
            .map(|(id, _)| *id)
            .collect();
        self.last_atm_strike =
            atm_price.and_then(|atm| Self::find_closest_strike(&all_strikes, atm));
        self.active_ids.iter().copied().collect()
    }

    /// Adds a newly discovered instrument to the series.
    ///
    /// Returns `true` if the instrument was newly inserted. Returns `false`
    /// if it was already known (no-op). When the new instrument's strike
    /// falls within the current active range, it is immediately added to
    /// `active_ids`.
    #[must_use]
    pub fn add_instrument(
        &mut self,
        instrument_id: InstrumentId,
        strike: Price,
        kind: OptionKind,
    ) -> bool {
        if self.instruments.contains_key(&instrument_id) {
            return false;
        }

        self.instruments.insert(instrument_id, (strike, kind));

        // Determine if the new strike is in the current active range
        let all_strikes = Self::sorted_strikes(&self.instruments);
        let atm_price = self.atm_tracker.atm_price();
        let active_strikes: HashSet<Price> = self
            .strike_range
            .resolve(atm_price, &all_strikes)
            .into_iter()
            .collect();

        if active_strikes.contains(&strike) {
            self.active_ids.insert(instrument_id);
        }

        true
    }

    /// Returns sorted, deduplicated strikes from the given instruments.
    fn sorted_strikes(instruments: &HashMap<InstrumentId, (Price, OptionKind)>) -> Vec<Price> {
        let mut strikes: Vec<Price> = instruments.values().map(|(s, _)| *s).collect();
        strikes.sort();
        strikes.dedup();
        strikes
    }

    /// Finds the strike in `all_strikes` closest to `atm`.
    fn find_closest_strike(all_strikes: &[Price], atm: Price) -> Option<Price> {
        all_strikes
            .iter()
            .min_by(|a, b| {
                let da = (a.as_f64() - atm.as_f64()).abs();
                let db = (b.as_f64() - atm.as_f64()).abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
    }

    /// Handles an incoming quote tick by updating the accumulator buffers.
    pub fn update_quote(&mut self, quote: &QuoteTick) {
        if self.is_expired(quote.ts_event) {
            log::warn!(
                "Dropping quote for {}, series {} expired at {}",
                quote.instrument_id,
                self.series_id,
                self.series_id.expiration_ns,
            );
            return;
        }

        if !self.active_ids.contains(&quote.instrument_id) {
            return;
        }

        if let Some(&(strike, kind)) = self.instruments.get(&quote.instrument_id) {
            // Track max ts_event across all quotes
            if quote.ts_event > self.max_ts_event {
                self.max_ts_event = quote.ts_event;
            }

            let buffer = match kind {
                OptionKind::Call => &mut self.call_buffer,
                OptionKind::Put => &mut self.put_buffer,
            };

            match buffer.get_mut(&strike) {
                Some(data) => data.quote = *quote,
                None => {
                    // Check for pending greeks that arrived before this first quote
                    let greeks = self.pending_greeks.remove(&quote.instrument_id);
                    buffer.insert(
                        strike,
                        OptionStrikeData {
                            quote: *quote,
                            greeks,
                        },
                    );
                }
            }
        }
    }

    /// Handles incoming greeks by updating the accumulator buffers.
    ///
    /// If no quote has arrived yet for this instrument (no buffer entry),
    /// the greeks are stored in `pending_greeks` and will be attached when
    /// the first quote arrives.
    pub fn update_greeks(&mut self, greeks: &OptionGreeks) {
        if self.is_expired(greeks.ts_event) {
            log::warn!(
                "Dropping greeks for {}, series {} expired at {}",
                greeks.instrument_id,
                self.series_id,
                self.series_id.expiration_ns,
            );
            return;
        }

        if !self.active_ids.contains(&greeks.instrument_id) {
            return;
        }

        if let Some(&(strike, kind)) = self.instruments.get(&greeks.instrument_id) {
            let buffer = match kind {
                OptionKind::Call => &mut self.call_buffer,
                OptionKind::Put => &mut self.put_buffer,
            };

            match buffer.get_mut(&strike) {
                Some(data) => data.greeks = Some(*greeks),
                None => {
                    // No quote yet — park the greeks for later
                    self.pending_greeks.insert(greeks.instrument_id, *greeks);
                }
            }
        }
    }

    /// Creates a point-in-time snapshot from accumulated buffers, applying strike filtering.
    ///
    /// Buffers are preserved (keep-latest semantics) so instruments that didn't
    /// quote since the last tick are still included in subsequent snapshots.
    ///
    /// # Panics
    ///
    /// Panics if strike prices cannot be compared (NaN values).
    pub fn snapshot(&self, ts_init: UnixNanos) -> OptionChainSlice {
        let atm_price = self.atm_tracker.atm_price();

        // Use catalog strikes for ATM strike (most accurate closest-strike lookup)
        let catalog_strikes = Self::sorted_strikes(&self.instruments);
        let atm_strike = atm_price.and_then(|atm| Self::find_closest_strike(&catalog_strikes, atm));

        // Filter buffers using active set strikes directly. The active set is already
        // the result of strike range resolution from the last rebalance. Re-resolving
        // here would shift the window during hysteresis/cooldown, dropping buffered data.
        let active_strikes: HashSet<Price> = self
            .active_ids
            .iter()
            .filter_map(|id| self.instruments.get(id).map(|(s, _)| *s))
            .collect();

        // Build filtered snapshot (clone from buffers)
        let mut calls = BTreeMap::new();

        for (strike, data) in &self.call_buffer {
            if active_strikes.contains(strike) {
                calls.insert(*strike, data.clone());
            }
        }
        let mut puts = BTreeMap::new();

        for (strike, data) in &self.put_buffer {
            if active_strikes.contains(strike) {
                puts.insert(*strike, data.clone());
            }
        }

        // Use the max observed ts_event from quotes, falling back to ts_init
        let ts_event = if self.max_ts_event == UnixNanos::default() {
            ts_init
        } else {
            self.max_ts_event
        };

        OptionChainSlice {
            series_id: self.series_id,
            atm_strike,
            calls,
            puts,
            ts_event,
            ts_init,
        }
    }

    /// Returns `true` if both buffers are empty.
    #[must_use]
    pub fn is_buffer_empty(&self) -> bool {
        self.call_buffer.is_empty() && self.put_buffer.is_empty()
    }

    /// Checks whether the instrument set should be rebalanced around the current ATM.
    ///
    /// Returns `None` when no rebalancing is needed (fixed ranges, no ATM price,
    /// ATM strike unchanged, hysteresis not exceeded, or cooldown not elapsed).
    /// Returns `Some(RebalanceAction)` with instrument add/remove lists when the
    /// closest ATM strike shifts past the hysteresis threshold.
    #[must_use]
    pub fn check_rebalance(&self, now_ns: UnixNanos) -> Option<RebalanceAction> {
        // Fixed ranges never rebalance
        if matches!(self.strike_range, StrikeRange::Fixed(_)) {
            return None;
        }

        let atm_price = self.atm_tracker.atm_price()?;
        let all_strikes = Self::sorted_strikes(&self.instruments);
        let current_atm_strike = Self::find_closest_strike(&all_strikes, atm_price)?;

        // No change → no rebalance
        if self.last_atm_strike == Some(current_atm_strike) {
            return None;
        }

        // Hysteresis check: price must cross hysteresis fraction of the gap to next strike
        if let Some(last_strike) = self.last_atm_strike
            && self.hysteresis > 0.0
        {
            let last_f = last_strike.as_f64();
            let atm_f = atm_price.as_f64();
            let direction = atm_f - last_f;

            // Find the next strike in the direction of price movement
            let next_strike = if direction > 0.0 {
                all_strikes.iter().find(|s| s.as_f64() > last_f)
            } else {
                all_strikes.iter().rev().find(|s| s.as_f64() < last_f)
            };

            if let Some(next) = next_strike {
                let gap = (next.as_f64() - last_f).abs();
                let threshold = last_f + direction.signum() * self.hysteresis * gap;
                // Check if price has not crossed the threshold
                if direction > 0.0 && atm_f < threshold {
                    return None;
                }

                if direction < 0.0 && atm_f > threshold {
                    return None;
                }
            }
        }

        // Cooldown check
        if self.cooldown_ns > 0
            && let Some(last_ts) = self.last_rebalance_ns
            && now_ns.as_u64().saturating_sub(last_ts.as_u64()) < self.cooldown_ns
        {
            return None;
        }

        // Compute new active set
        let new_active_strikes: HashSet<Price> = self
            .strike_range
            .resolve(Some(atm_price), &all_strikes)
            .into_iter()
            .collect();
        let new_active: HashSet<InstrumentId> = self
            .instruments
            .iter()
            .filter(|(_, (s, _))| new_active_strikes.contains(s))
            .map(|(id, _)| *id)
            .collect();

        let add = new_active.difference(&self.active_ids).copied().collect();
        let remove = self.active_ids.difference(&new_active).copied().collect();

        Some(RebalanceAction { add, remove })
    }

    /// Applies a rebalance action: updates the active ID set, cleans stale buffers,
    /// and records the rebalance timestamp.
    pub fn apply_rebalance(&mut self, action: &RebalanceAction, now_ns: UnixNanos) {
        for id in &action.add {
            self.active_ids.insert(*id);
        }

        for id in &action.remove {
            self.active_ids.remove(id);
        }

        // Clean buffers for strikes no longer in active set
        let active_strikes: HashSet<Price> = self
            .active_ids
            .iter()
            .filter_map(|id| self.instruments.get(id))
            .map(|(s, _)| *s)
            .collect();
        self.call_buffer
            .retain(|strike, _| active_strikes.contains(strike));
        self.put_buffer
            .retain(|strike, _| active_strikes.contains(strike));
        self.pending_greeks
            .retain(|id, _| self.active_ids.contains(id));

        // Update last_atm_strike and record rebalance timestamp
        if let Some(atm) = self.atm_tracker.atm_price() {
            let all_strikes = Self::sorted_strikes(&self.instruments);
            self.last_atm_strike = Self::find_closest_strike(&all_strikes, atm);
        }
        self.last_rebalance_ns = Some(now_ns);
    }
}

/// Describes instruments to add and remove during an ATM rebalance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebalanceAction {
    /// Instruments to subscribe to (newly in range).
    pub add: Vec<InstrumentId>,
    /// Instruments to unsubscribe from (no longer in range).
    pub remove: Vec<InstrumentId>,
}

#[cfg(test)]
impl OptionChainAggregator {
    fn call_buffer_len(&self) -> usize {
        self.call_buffer.len()
    }

    fn put_buffer_len(&self) -> usize {
        self.put_buffer.len()
    }

    fn get_call_greeks_from_buffer(&self, strike: &Price) -> Option<&OptionGreeks> {
        self.call_buffer.get(strike).and_then(|d| d.greeks.as_ref())
    }

    pub(crate) fn last_atm_strike(&self) -> Option<Price> {
        self.last_atm_strike
    }

    fn set_hysteresis(&mut self, h: f64) {
        self.hysteresis = h;
    }

    fn set_cooldown_ns(&mut self, ns: u64) {
        self.cooldown_ns = ns;
    }

    fn pending_greeks_count(&self) -> usize {
        self.pending_greeks.len()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{data::greeks::OptionGreekValues, identifiers::Venue, types::Quantity};
    use rstest::*;

    use super::*;

    fn make_series_id() -> OptionSeriesId {
        OptionSeriesId::new(
            Venue::new("DERIBIT"),
            ustr::Ustr::from("BTC"),
            ustr::Ustr::from("BTC"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        )
    }

    fn make_quote(instrument_id: InstrumentId, bid: &str, ask: &str) -> QuoteTick {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1u64),
            UnixNanos::from(1u64),
        )
    }

    fn now() -> UnixNanos {
        // A base timestamp for tests (far enough from zero to avoid edge cases)
        UnixNanos::from(1_000_000_000_000_000_000u64)
    }

    /// Sets ATM price on an aggregator via a synthetic OptionGreeks with the given forward price.
    fn set_atm_via_greeks(agg: &mut OptionChainAggregator, price: f64) {
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: Some(price),
            ..Default::default()
        };
        agg.atm_tracker_mut().update_from_option_greeks(&greeks);
    }

    fn make_aggregator() -> (OptionChainAggregator, InstrumentId, InstrumentId) {
        let call_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        let put_id = InstrumentId::from("BTC-20240101-50000-P.DERIBIT");
        let strike = Price::from("50000");

        let mut instrument_map = HashMap::new();
        instrument_map.insert(call_id, (strike, OptionKind::Call));
        instrument_map.insert(put_id, (strike, OptionKind::Put));

        let tracker = AtmTracker::new();
        let agg = OptionChainAggregator::new(
            make_series_id(),
            StrikeRange::Fixed(vec![strike]),
            tracker,
            instrument_map,
        );

        (agg, call_id, put_id)
    }

    #[rstest]
    fn test_aggregator_instrument_ids() {
        let (agg, call_id, put_id) = make_aggregator();
        let ids = agg.instrument_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&call_id));
        assert!(ids.contains(&put_id));
    }

    #[rstest]
    fn test_aggregator_update_quote() {
        let (mut agg, call_id, _) = make_aggregator();
        let quote = make_quote(call_id, "100.00", "101.00");

        agg.update_quote(&quote);

        assert_eq!(agg.call_buffer_len(), 1);
        assert_eq!(agg.put_buffer_len(), 0);
    }

    #[rstest]
    fn test_aggregator_update_greeks() {
        let (mut agg, call_id, _) = make_aggregator();
        let quote = make_quote(call_id, "100.00", "101.00");
        agg.update_quote(&quote);

        let greeks = OptionGreeks {
            instrument_id: call_id,
            greeks: OptionGreekValues {
                delta: 0.55,
                ..Default::default()
            },
            ..Default::default()
        };
        agg.update_greeks(&greeks);

        let strike = Price::from("50000");
        let data = agg.get_call_greeks_from_buffer(&strike);
        assert!(data.is_some());
        assert_eq!(data.unwrap().delta, 0.55);
    }

    #[rstest]
    fn test_aggregator_snapshot_preserves_state() {
        let (mut agg, call_id, _) = make_aggregator();
        let quote = make_quote(call_id, "100.00", "101.00");
        agg.update_quote(&quote);

        let slice = agg.snapshot(UnixNanos::from(100u64));
        assert_eq!(slice.call_count(), 1);
        assert_eq!(slice.ts_init, UnixNanos::from(100u64));

        // Buffers should still contain data (keep-latest semantics)
        assert!(!agg.is_buffer_empty());

        // Second snapshot should return the same data
        let slice2 = agg.snapshot(UnixNanos::from(200u64));
        assert_eq!(slice2.call_count(), 1);
        assert_eq!(slice2.ts_init, UnixNanos::from(200u64));
    }

    #[rstest]
    fn test_aggregator_ignores_unknown_instrument() {
        let (mut agg, _, _) = make_aggregator();
        let unknown_id = InstrumentId::from("ETH-20240101-3000-C.DERIBIT");
        let quote = make_quote(unknown_id, "100.00", "101.00");

        agg.update_quote(&quote);

        assert!(agg.is_buffer_empty());
    }

    #[rstest]
    fn test_check_rebalance_returns_none() {
        let (agg, _, _) = make_aggregator();
        assert!(agg.check_rebalance(now()).is_none());
    }

    // -- Rebalance tests --

    /// Builds instruments with 5 strike prices (45000..55000 step 2500) and AtmRelative +-1.
    /// Hysteresis and cooldown are disabled so existing rebalance tests pass unchanged.
    fn make_multi_strike_aggregator() -> OptionChainAggregator {
        let strikes = [45000, 47500, 50000, 52500, 55000];
        let mut instruments = HashMap::new();

        for s in &strikes {
            let strike = Price::from(&s.to_string());
            let call_id = InstrumentId::from(&format!("BTC-20240101-{s}-C.DERIBIT"));
            let put_id = InstrumentId::from(&format!("BTC-20240101-{s}-P.DERIBIT"));
            instruments.insert(call_id, (strike, OptionKind::Call));
            instruments.insert(put_id, (strike, OptionKind::Put));
        }

        let tracker = AtmTracker::new();
        let mut agg = OptionChainAggregator::new(
            make_series_id(),
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            },
            tracker,
            instruments,
        );
        // Disable guards so existing tests exercise pure rebalance logic
        agg.set_hysteresis(0.0);
        agg.set_cooldown_ns(0);
        agg
    }

    #[rstest]
    fn test_check_rebalance_fixed_always_none() {
        // Fixed range + ATM price set → still returns None
        let (mut agg, _, _) = make_aggregator();
        set_atm_via_greeks(&mut agg, 50000.0);
        assert!(agg.check_rebalance(now()).is_none());
    }

    #[rstest]
    fn test_check_rebalance_no_atm_returns_none() {
        let agg = make_multi_strike_aggregator();
        // No ATM price set → None
        assert!(agg.check_rebalance(now()).is_none());
    }

    #[rstest]
    fn test_check_rebalance_atm_unchanged_returns_none() {
        let mut agg = make_multi_strike_aggregator();
        // Set ATM to 50000 and apply initial rebalance
        set_atm_via_greeks(&mut agg, 50000.0);
        // First check detects ATM shift (from None → 50000)
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());

        // ATM moves slightly but stays closest to 50000
        set_atm_via_greeks(&mut agg, 50200.0);
        assert!(agg.check_rebalance(now()).is_none());
    }

    #[rstest]
    fn test_check_rebalance_detects_atm_shift() {
        let mut agg = make_multi_strike_aggregator();
        // Set ATM near 50000
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());
        // Active: 47500, 50000, 52500 (ATM=50000, +-1 strike)
        assert_eq!(agg.instrument_ids().len(), 6); // 3 strikes × 2

        // Now shift ATM to 55000
        set_atm_via_greeks(&mut agg, 55000.0);
        let action2 = agg.check_rebalance(now()).unwrap();
        // Should have instruments to add (55000) and remove (47500)
        assert!(!action2.add.is_empty() || !action2.remove.is_empty());
    }

    #[rstest]
    fn test_apply_rebalance_updates_instrument_map() {
        let mut agg = make_multi_strike_aggregator();
        // Set ATM near 50000
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());

        // Active should be 3 strikes (47500, 50000, 52500)
        let active_ids = agg.instrument_ids();
        assert_eq!(active_ids.len(), 6); // 3 strikes × 2 (call + put)

        // Now shift to 55000
        set_atm_via_greeks(&mut agg, 55000.0);
        let action2 = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action2, now());

        // Active should now be (52500, 55000) — 2 strikes at the top end
        let active_ids2 = agg.instrument_ids();
        assert_eq!(active_ids2.len(), 4); // 2 strikes × 2
    }

    #[rstest]
    fn test_apply_rebalance_cleans_buffers() {
        let mut agg = make_multi_strike_aggregator();
        // Set ATM near 50000
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());

        // Feed quotes for the 47500 call
        let call_47500 = InstrumentId::from("BTC-20240101-47500-C.DERIBIT");
        let quote = make_quote(call_47500, "100.00", "101.00");
        agg.update_quote(&quote);
        assert_eq!(agg.call_buffer_len(), 1);

        // Now shift ATM up so 47500 is out of range
        set_atm_via_greeks(&mut agg, 55000.0);
        let action2 = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action2, now());

        // Buffer for 47500 should be cleaned
        assert_eq!(agg.call_buffer_len(), 0);
    }

    #[rstest]
    fn test_initial_active_set_empty_when_no_atm() {
        let agg = make_multi_strike_aggregator();
        // AtmRelative with no ATM price → empty active set (deferred)
        assert_eq!(agg.instrument_ids().len(), 0);
        assert_eq!(agg.all_instrument_ids().len(), 10);
    }

    #[rstest]
    fn test_catalog_vs_active_separation() {
        let mut agg = make_multi_strike_aggregator();
        // Set ATM near 50000 to narrow active set
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());

        // Catalog should still have all 10 instruments
        assert_eq!(agg.instruments().len(), 10);
        // Active should be a subset
        assert_eq!(agg.instrument_ids().len(), 6);
    }

    // -- add_instrument tests --

    #[rstest]
    fn test_add_instrument_already_known() {
        let (mut agg, call_id, _) = make_aggregator();
        let strike = Price::from("50000");
        let count_before = agg.instruments().len();

        let result = agg.add_instrument(call_id, strike, OptionKind::Call);

        assert!(!result);
        assert_eq!(agg.instruments().len(), count_before);
    }

    #[rstest]
    fn test_add_instrument_new_in_active_range() {
        let (mut agg, _, _) = make_aggregator();
        // Fixed range includes strike 50000; adding another instrument at same strike
        let new_id = InstrumentId::from("BTC-20240101-50000-C2.DERIBIT");
        let strike = Price::from("50000");

        let result = agg.add_instrument(new_id, strike, OptionKind::Call);

        assert!(result);
        assert_eq!(agg.instruments().len(), 3);
        assert!(agg.active_ids().contains(&new_id));
    }

    #[rstest]
    fn test_add_instrument_new_out_of_range() {
        let (mut agg, _, _) = make_aggregator();
        // Fixed range only includes 50000; adding instrument at 60000
        let new_id = InstrumentId::from("BTC-20240101-60000-C.DERIBIT");
        let strike = Price::from("60000");

        let result = agg.add_instrument(new_id, strike, OptionKind::Call);

        assert!(result);
        assert_eq!(agg.instruments().len(), 3);
        assert!(!agg.active_ids().contains(&new_id));
    }

    #[rstest]
    fn test_add_instrument_available_for_rebalance() {
        let mut agg = make_multi_strike_aggregator();
        // Set ATM near 50000 and apply initial rebalance
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());
        // Active: 47500, 50000, 52500 (6 instruments)
        assert_eq!(agg.instrument_ids().len(), 6);

        // Add a new instrument at strike 57500 (out of current range)
        let new_id = InstrumentId::from("BTC-20240101-57500-C.DERIBIT");
        let strike = Price::from("57500");
        let result = agg.add_instrument(new_id, strike, OptionKind::Call);
        assert!(result);
        assert!(!agg.active_ids().contains(&new_id));

        // Shift ATM to 57500 — rebalance should pick up the new instrument
        set_atm_via_greeks(&mut agg, 57500.0);
        let action2 = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action2, now());

        assert!(agg.active_ids().contains(&new_id));
    }

    // -- Hysteresis tests --

    #[rstest]
    fn test_hysteresis_blocks_small_movement() {
        let strikes = [47500, 50000, 52500];
        let mut instruments = HashMap::new();

        for s in &strikes {
            let strike = Price::from(&s.to_string());
            let call_id = InstrumentId::from(&format!("BTC-20240101-{s}-C.DERIBIT"));
            instruments.insert(call_id, (strike, OptionKind::Call));
        }
        let tracker = AtmTracker::new();
        let mut agg = OptionChainAggregator::new(
            make_series_id(),
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            },
            tracker,
            instruments,
        );
        agg.set_hysteresis(0.6);
        agg.set_cooldown_ns(0);

        // Set ATM to 50000
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());
        assert_eq!(agg.last_atm_strike(), Some(Price::from("50000")));

        // Move ATM slightly toward 52500 — gap=2500, threshold=50000+0.6*2500=51500
        // 51000 does NOT cross 51500
        set_atm_via_greeks(&mut agg, 51000.0);
        assert!(agg.check_rebalance(now()).is_none());
    }

    #[rstest]
    fn test_hysteresis_allows_large_movement() {
        let strikes = [47500, 50000, 52500];
        let mut instruments = HashMap::new();

        for s in &strikes {
            let strike = Price::from(&s.to_string());
            let call_id = InstrumentId::from(&format!("BTC-20240101-{s}-C.DERIBIT"));
            instruments.insert(call_id, (strike, OptionKind::Call));
        }
        let tracker = AtmTracker::new();
        let mut agg = OptionChainAggregator::new(
            make_series_id(),
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            },
            tracker,
            instruments,
        );
        agg.set_hysteresis(0.6);
        agg.set_cooldown_ns(0);

        // Set ATM to 50000
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());

        // Move ATM well past threshold: 52000 > 51500
        set_atm_via_greeks(&mut agg, 52000.0);
        assert!(agg.check_rebalance(now()).is_some());
    }

    #[rstest]
    fn test_zero_hysteresis_disables_guard() {
        let mut agg = make_multi_strike_aggregator();
        agg.set_hysteresis(0.0);
        agg.set_cooldown_ns(0);

        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());

        // Any shift past the strike boundary triggers rebalance
        set_atm_via_greeks(&mut agg, 52500.0);
        assert!(agg.check_rebalance(now()).is_some());
    }

    // -- Cooldown tests --

    #[rstest]
    fn test_cooldown_blocks_rapid_rebalance() {
        let mut agg = make_multi_strike_aggregator();
        agg.set_hysteresis(0.0);
        agg.set_cooldown_ns(5_000_000_000); // 5s

        set_atm_via_greeks(&mut agg, 50000.0);
        let t0 = now();
        let action = agg.check_rebalance(t0).unwrap();
        agg.apply_rebalance(&action, t0);

        // Shift ATM immediately — cooldown blocks
        set_atm_via_greeks(&mut agg, 55000.0);
        let t1 = UnixNanos::from(t0.as_u64() + 1_000_000_000); // 1s later
        assert!(agg.check_rebalance(t1).is_none());
    }

    #[rstest]
    fn test_cooldown_allows_after_elapsed() {
        let mut agg = make_multi_strike_aggregator();
        agg.set_hysteresis(0.0);
        agg.set_cooldown_ns(5_000_000_000); // 5s

        set_atm_via_greeks(&mut agg, 50000.0);
        let t0 = now();
        let action = agg.check_rebalance(t0).unwrap();
        agg.apply_rebalance(&action, t0);

        // Shift ATM after cooldown elapses
        set_atm_via_greeks(&mut agg, 55000.0);
        let t1 = UnixNanos::from(t0.as_u64() + 6_000_000_000); // 6s later
        assert!(agg.check_rebalance(t1).is_some());
    }

    #[rstest]
    fn test_zero_cooldown_disables_guard() {
        let mut agg = make_multi_strike_aggregator();
        agg.set_hysteresis(0.0);
        agg.set_cooldown_ns(0);

        set_atm_via_greeks(&mut agg, 50000.0);
        let t0 = now();
        let action = agg.check_rebalance(t0).unwrap();
        agg.apply_rebalance(&action, t0);

        // Shift ATM immediately — no cooldown block
        set_atm_via_greeks(&mut agg, 55000.0);
        assert!(agg.check_rebalance(t0).is_some());
    }

    // -- Pending greeks tests --

    #[rstest]
    fn test_pending_greeks_consumed_on_first_quote() {
        let (mut agg, call_id, _) = make_aggregator();

        // Send greeks before any quote
        let greeks = OptionGreeks {
            instrument_id: call_id,
            greeks: OptionGreekValues {
                delta: 0.55,
                ..Default::default()
            },
            ..Default::default()
        };
        agg.update_greeks(&greeks);
        assert_eq!(agg.pending_greeks_count(), 1);

        // Now send the first quote — pending greeks should be consumed
        let quote = make_quote(call_id, "100.00", "101.00");
        agg.update_quote(&quote);
        assert_eq!(agg.pending_greeks_count(), 0);

        // Verify greeks were attached
        let strike = Price::from("50000");
        let data = agg.get_call_greeks_from_buffer(&strike);
        assert!(data.is_some());
        assert_eq!(data.unwrap().delta, 0.55);
    }

    // -- ts_event tracking tests --

    #[rstest]
    fn test_snapshot_ts_event_reflects_max_quote_timestamp() {
        let (mut agg, call_id, put_id) = make_aggregator();

        let quote1 = QuoteTick::new(
            call_id,
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(500u64), // ts_event
            UnixNanos::from(500u64),
        );
        agg.update_quote(&quote1);

        let quote2 = QuoteTick::new(
            put_id,
            Price::from("50.00"),
            Price::from("51.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(800u64), // ts_event — later
            UnixNanos::from(800u64),
        );
        agg.update_quote(&quote2);

        let slice = agg.snapshot(UnixNanos::from(1000u64));
        assert_eq!(slice.ts_event, UnixNanos::from(800u64));
        assert_eq!(slice.ts_init, UnixNanos::from(1000u64));
    }

    #[rstest]
    fn test_snapshot_ts_event_fallback_when_no_quotes() {
        let (agg, _, _) = make_aggregator();
        let slice = agg.snapshot(UnixNanos::from(1000u64));
        // No quotes → ts_event falls back to ts_init
        assert_eq!(slice.ts_event, UnixNanos::from(1000u64));
    }

    #[rstest]
    fn test_snapshot_retains_buffered_data_during_hysteresis_window() {
        // Setup: 3 strikes at 47500/50000/52500, AtmRelative +-1, hysteresis enabled
        let strikes = [47500, 50000, 52500];
        let mut instruments = HashMap::new();

        for s in &strikes {
            let strike = Price::from(&s.to_string());
            let call_id = InstrumentId::from(&format!("BTC-20240101-{s}-C.DERIBIT"));
            instruments.insert(call_id, (strike, OptionKind::Call));
        }
        let tracker = AtmTracker::new();
        let mut agg = OptionChainAggregator::new(
            make_series_id(),
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            },
            tracker,
            instruments,
        );
        agg.set_hysteresis(0.6);
        agg.set_cooldown_ns(0);

        // Set ATM to 50000, rebalance -> active: {47500, 50000, 52500}
        set_atm_via_greeks(&mut agg, 50000.0);
        let action = agg.check_rebalance(now()).unwrap();
        agg.apply_rebalance(&action, now());
        assert_eq!(agg.instrument_ids().len(), 3);

        // Buffer quotes for all active strikes
        let q1 = make_quote(
            InstrumentId::from("BTC-20240101-47500-C.DERIBIT"),
            "3000.00",
            "3100.00",
        );
        let q2 = make_quote(
            InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            "1500.00",
            "1600.00",
        );
        let q3 = make_quote(
            InstrumentId::from("BTC-20240101-52500-C.DERIBIT"),
            "500.00",
            "600.00",
        );
        agg.update_quote(&q1);
        agg.update_quote(&q2);
        agg.update_quote(&q3);
        assert_eq!(agg.call_buffer_len(), 3);

        // Move ATM slightly toward 52500 but within hysteresis band (no rebalance)
        set_atm_via_greeks(&mut agg, 51000.0);
        assert!(agg.check_rebalance(now()).is_none());

        // Snapshot must still include all 3 buffered strikes
        let slice = agg.snapshot(UnixNanos::from(100u64));
        assert_eq!(slice.call_count(), 3);
    }

    #[rstest]
    fn test_remove_instrument_from_catalog() {
        let (mut agg, call_id, put_id) = make_aggregator();
        assert_eq!(agg.instruments().len(), 2);

        let removed = agg.remove_instrument(&call_id);
        assert!(removed);
        assert_eq!(agg.instruments().len(), 1);
        assert!(!agg.active_ids().contains(&call_id));
        assert!(agg.instruments().contains_key(&put_id));
    }

    #[rstest]
    fn test_remove_instrument_cleans_buffer() {
        let (mut agg, call_id, _) = make_aggregator();
        let quote = make_quote(call_id, "100.00", "101.00");
        agg.update_quote(&quote);
        assert_eq!(agg.call_buffer_len(), 1);

        let _ = agg.remove_instrument(&call_id);
        // No sibling call at same strike, buffer entry should be removed
        assert_eq!(agg.call_buffer_len(), 0);
    }

    #[rstest]
    fn test_remove_instrument_preserves_sibling_buffer() {
        let (mut agg, call_id, _) = make_aggregator();
        // Add a second call at the same strike
        let sibling_id = InstrumentId::from("BTC-20240101-50000-C2.DERIBIT");
        let strike = Price::from("50000");
        let _ = agg.add_instrument(sibling_id, strike, OptionKind::Call);

        let quote = make_quote(call_id, "100.00", "101.00");
        agg.update_quote(&quote);
        assert_eq!(agg.call_buffer_len(), 1);

        // Remove original — sibling still shares the strike+kind
        let _ = agg.remove_instrument(&call_id);
        assert_eq!(agg.call_buffer_len(), 1); // buffer preserved
        assert!(agg.instruments().contains_key(&sibling_id));
    }

    #[rstest]
    fn test_remove_instrument_unknown_noop() {
        let (mut agg, _, _) = make_aggregator();
        let unknown = InstrumentId::from("ETH-20240101-3000-C.DERIBIT");
        assert!(!agg.remove_instrument(&unknown));
        assert_eq!(agg.instruments().len(), 2);
    }

    #[rstest]
    fn test_remove_instrument_cleans_pending_greeks() {
        let (mut agg, call_id, _) = make_aggregator();
        let greeks = OptionGreeks {
            instrument_id: call_id,
            greeks: OptionGreekValues {
                delta: 0.55,
                ..Default::default()
            },
            ..Default::default()
        };
        agg.update_greeks(&greeks);
        assert_eq!(agg.pending_greeks_count(), 1);

        let _ = agg.remove_instrument(&call_id);
        assert_eq!(agg.pending_greeks_count(), 0);
    }

    #[rstest]
    fn test_is_catalog_empty_after_full_removal() {
        let (mut agg, call_id, put_id) = make_aggregator();
        assert!(!agg.is_catalog_empty());

        let _ = agg.remove_instrument(&call_id);
        assert!(!agg.is_catalog_empty());

        let _ = agg.remove_instrument(&put_id);
        assert!(agg.is_catalog_empty());
    }

    // -- Expiry guard tests --

    #[rstest]
    fn test_expired_quote_is_dropped() {
        let (mut agg, call_id, _) = make_aggregator();
        // Series expires at 1_700_000_000_000_000_000; send quote AT that timestamp
        let expired_quote = QuoteTick::new(
            call_id,
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        );
        agg.update_quote(&expired_quote);
        assert!(agg.is_buffer_empty());
    }

    #[rstest]
    fn test_expired_greeks_are_dropped() {
        let (mut agg, call_id, _) = make_aggregator();
        // First add a valid quote so greeks would normally land in the buffer
        let quote = make_quote(call_id, "100.00", "101.00");
        agg.update_quote(&quote);
        assert_eq!(agg.call_buffer_len(), 1);

        // Send greeks at expiry timestamp — should be dropped
        let greeks = OptionGreeks {
            instrument_id: call_id,
            ts_event: UnixNanos::from(1_700_000_000_000_000_000u64),
            greeks: OptionGreekValues {
                delta: 0.55,
                ..Default::default()
            },
            ..Default::default()
        };
        agg.update_greeks(&greeks);

        let strike = Price::from("50000");
        assert!(agg.get_call_greeks_from_buffer(&strike).is_none());
    }
}
