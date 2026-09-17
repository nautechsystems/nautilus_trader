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

//! Subscription state for the Kalshi data client.
//!
//! The client polls, so a subscription is a registration of interest rather than a socket
//! subscription. The registry also carries the per-instrument book sequence, which the exchange
//! does not publish: a polled book is emitted as a snapshot that clears and rebuilds the book, and
//! the sequence only has to increase.

use std::collections::HashSet;

use nautilus_model::identifiers::InstrumentId;

/// The order book depth a subscription requests.
pub const DEFAULT_BOOK_DEPTH: u32 = 10;

/// The market data a client is subscribed to.
#[derive(Debug, Default)]
pub struct KalshiSubscriptions {
    quotes: HashSet<InstrumentId>,
    trades: HashSet<InstrumentId>,
    book_deltas: HashSet<InstrumentId>,
    book_depths: std::collections::HashMap<InstrumentId, u32>,
    sequences: std::collections::HashMap<InstrumentId, u64>,
}

impl KalshiSubscriptions {
    /// Creates a new [`KalshiSubscriptions`] registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether the registry holds no subscriptions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of distinct instruments subscribed to any market data.
    #[must_use]
    pub fn len(&self) -> usize {
        self.instrument_ids().count()
    }

    /// Returns every instrument subscribed to any market data.
    pub fn instrument_ids(&self) -> impl Iterator<Item = InstrumentId> + '_ {
        self.quotes
            .iter()
            .chain(self.trades.iter())
            .chain(self.book_deltas.iter())
            .copied()
            .collect::<HashSet<InstrumentId>>()
            .into_iter()
    }

    /// Returns whether the instrument is subscribed to quotes.
    #[must_use]
    pub fn is_quote_subscribed(&self, instrument_id: &InstrumentId) -> bool {
        self.quotes.contains(instrument_id)
    }

    /// Returns whether the instrument is subscribed to trades.
    #[must_use]
    pub fn is_trade_subscribed(&self, instrument_id: &InstrumentId) -> bool {
        self.trades.contains(instrument_id)
    }

    /// Returns whether the instrument is subscribed to book deltas.
    #[must_use]
    pub fn is_book_subscribed(&self, instrument_id: &InstrumentId) -> bool {
        self.book_deltas.contains(instrument_id)
    }

    /// Returns the instruments subscribed to trades.
    #[must_use]
    pub fn trade_instrument_ids(&self) -> Vec<InstrumentId> {
        self.trades.iter().copied().collect()
    }

    /// Returns the book depth requested for the given instrument.
    ///
    /// A subscription without an explicit depth uses [`DEFAULT_BOOK_DEPTH`]. An instrument that is
    /// not subscribed to book deltas has no depth.
    #[must_use]
    pub fn book_depth(&self, instrument_id: &InstrumentId) -> Option<u32> {
        if !self.book_deltas.contains(instrument_id) {
            return None;
        }

        Some(
            self.book_depths
                .get(instrument_id)
                .copied()
                .unwrap_or(DEFAULT_BOOK_DEPTH),
        )
    }

    /// Subscribes the instrument to quotes.
    pub fn subscribe_quotes(&mut self, instrument_id: InstrumentId) {
        self.quotes.insert(instrument_id);
    }

    /// Subscribes the instrument to trades.
    pub fn subscribe_trades(&mut self, instrument_id: InstrumentId) {
        self.trades.insert(instrument_id);
    }

    /// Subscribes the instrument to book deltas at the given depth.
    pub fn subscribe_book_deltas(&mut self, instrument_id: InstrumentId, depth: Option<u32>) {
        self.book_deltas.insert(instrument_id);

        if let Some(depth) = depth {
            self.book_depths.insert(instrument_id, depth);
        }
    }

    /// Unsubscribes the instrument from quotes.
    pub fn unsubscribe_quotes(&mut self, instrument_id: InstrumentId) {
        self.quotes.remove(&instrument_id);
    }

    /// Unsubscribes the instrument from trades.
    pub fn unsubscribe_trades(&mut self, instrument_id: InstrumentId) {
        self.trades.remove(&instrument_id);
    }

    /// Unsubscribes the instrument from book deltas.
    pub fn unsubscribe_book_deltas(&mut self, instrument_id: InstrumentId) {
        self.book_deltas.remove(&instrument_id);
        self.book_depths.remove(&instrument_id);
    }

    /// Returns the next sequence for the instrument's book snapshot.
    ///
    /// Kalshi publishes no book sequence, so the adapter owns it. It only has to increase.
    pub fn next_sequence(&mut self, instrument_id: &InstrumentId) -> u64 {
        let sequence = self.sequences.entry(*instrument_id).or_insert(0);
        let current = *sequence;

        *sequence = match current {
            u64::MAX => 1,
            _ => current + 1,
        };

        current
    }

    /// Clears every subscription, depth, and sequence.
    pub fn clear(&mut self) {
        self.quotes.clear();
        self.trades.clear();
        self.book_deltas.clear();
        self.book_depths.clear();
        self.sequences.clear();
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn instrument(symbol: &str) -> InstrumentId {
        InstrumentId::from(format!("{symbol}.KALSHI").as_str())
    }

    #[rstest]
    fn test_subscriptions_track_each_data_type_independently() {
        let mut subscriptions = KalshiSubscriptions::new();
        let quotes = instrument("KXHIGHNY-25JAN01-T50");
        let trades = instrument("KXHIGHNY-25JAN01-T60");

        subscriptions.subscribe_quotes(quotes);
        subscriptions.subscribe_trades(trades);

        assert!(subscriptions.is_quote_subscribed(&quotes));
        assert!(!subscriptions.is_trade_subscribed(&quotes));
        assert!(subscriptions.is_trade_subscribed(&trades));
        assert_eq!(subscriptions.len(), 2);
        assert_eq!(subscriptions.trade_instrument_ids(), vec![trades]);
    }

    #[rstest]
    fn test_one_instrument_subscribed_to_two_data_types_counts_once() {
        let mut subscriptions = KalshiSubscriptions::new();
        let instrument_id = instrument("KXHIGHNY-25JAN01-T50");

        subscriptions.subscribe_quotes(instrument_id);
        subscriptions.subscribe_book_deltas(instrument_id, None);

        assert_eq!(subscriptions.len(), 1);
        assert!(!subscriptions.is_empty());
    }

    #[rstest]
    fn test_unsubscribe_removes_only_the_named_data_type() {
        let mut subscriptions = KalshiSubscriptions::new();
        let instrument_id = instrument("KXHIGHNY-25JAN01-T50");

        subscriptions.subscribe_quotes(instrument_id);
        subscriptions.subscribe_trades(instrument_id);
        subscriptions.unsubscribe_quotes(instrument_id);

        assert!(!subscriptions.is_quote_subscribed(&instrument_id));
        assert!(subscriptions.is_trade_subscribed(&instrument_id));
    }

    #[rstest]
    fn test_sequences_increase_per_instrument() {
        let mut subscriptions = KalshiSubscriptions::new();
        let first = instrument("KXHIGHNY-25JAN01-T50");
        let second = instrument("KXHIGHNY-25JAN01-T60");

        assert_eq!(subscriptions.next_sequence(&first), 0);
        assert_eq!(subscriptions.next_sequence(&first), 1);
        assert_eq!(subscriptions.next_sequence(&second), 0);
    }

    #[rstest]
    fn test_clear_resets_subscriptions_and_sequences() {
        let mut subscriptions = KalshiSubscriptions::new();
        let instrument_id = instrument("KXHIGHNY-25JAN01-T50");

        subscriptions.subscribe_book_deltas(instrument_id, None);
        subscriptions.next_sequence(&instrument_id);
        subscriptions.clear();

        assert!(subscriptions.is_empty());
        assert_eq!(subscriptions.next_sequence(&instrument_id), 0);
    }

    #[rstest]
    fn test_book_depth_defaults_is_per_instrument_and_lasts_until_unsubscribe() {
        let mut subscriptions = KalshiSubscriptions::new();
        let deep = instrument("KXHIGHNY-25JAN01-T50");
        let shallow = instrument("KXHIGHNY-25JAN01-T60");

        assert_eq!(subscriptions.book_depth(&deep), None);

        subscriptions.subscribe_book_deltas(deep, None);
        subscriptions.subscribe_book_deltas(shallow, Some(25));

        assert_eq!(subscriptions.book_depth(&deep), Some(DEFAULT_BOOK_DEPTH));
        assert_eq!(subscriptions.book_depth(&shallow), Some(25));

        subscriptions.unsubscribe_book_deltas(shallow);

        assert_eq!(subscriptions.book_depth(&shallow), None);
    }
}
