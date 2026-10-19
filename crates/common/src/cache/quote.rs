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

//! Generic quote cache for maintaining the last known quote per instrument.
//!
//! This cache is commonly used by WebSocket adapters to handle partial quote updates
//! where the exchange may send incomplete bid or ask information. By caching the last
//! complete quote, adapters can merge partial updates with cached values to reconstruct
//! a complete `QuoteTick`.

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::quote::QuoteTick,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

/// A cache for storing the last known quote per instrument.
///
/// This is particularly useful for handling partial quote updates from exchange WebSocket feeds,
/// where updates may only include one side of the market (bid or ask). The cache maintains
/// the most recent complete quote for each instrument, allowing adapters to fill in missing
/// information when processing partial updates.
///
/// # Thread Safety
///
/// This cache is not thread-safe. If shared across threads, wrap it in an appropriate
/// synchronization primitive such as `Arc<RwLock<QuoteCache>>` or `Arc<Mutex<QuoteCache>>`.
#[derive(Debug, Clone)]
pub struct QuoteCache {
    quotes: AHashMap<InstrumentId, QuoteTick>,
}

impl QuoteCache {
    /// Creates a new empty [`QuoteCache`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            quotes: AHashMap::new(),
        }
    }

    /// Returns the cached quote for the given instrument, if available.
    #[must_use]
    pub fn get(&self, instrument_id: &InstrumentId) -> Option<&QuoteTick> {
        self.quotes.get(instrument_id)
    }

    /// Inserts or updates a quote in the cache for the given instrument.
    ///
    /// Returns the previously cached quote if one existed.
    pub fn insert(&mut self, instrument_id: InstrumentId, quote: QuoteTick) -> Option<QuoteTick> {
        self.quotes.insert(instrument_id, quote)
    }

    /// Removes the cached quote for the given instrument.
    ///
    /// Returns the removed quote if one existed.
    pub fn remove(&mut self, instrument_id: &InstrumentId) -> Option<QuoteTick> {
        self.quotes.remove(instrument_id)
    }

    /// Returns `true` if the cache contains a quote for the given instrument.
    #[must_use]
    pub fn contains(&self, instrument_id: &InstrumentId) -> bool {
        self.quotes.contains_key(instrument_id)
    }

    /// Returns the number of cached quotes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.quotes.len()
    }

    /// Returns `true` if the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty()
    }

    /// Clears all cached quotes.
    ///
    /// This is typically called after a reconnection to ensure stale quotes
    /// from before the disconnect are not used.
    pub fn clear(&mut self) {
        self.quotes.clear();
    }

    /// Processes a partial quote update, merging with cached values when needed.
    ///
    /// This method handles partial quote updates where some fields may be missing.
    /// If any field is `None`, it will use the corresponding field from the cached quote.
    /// If there is no cached quote and any field is missing, an error is returned.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Any required field is `None` and there is no cached quote.
    /// - The first quote received is incomplete (no cached values to merge with).
    #[expect(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        instrument_id: InstrumentId,
        bid_price: Option<Price>,
        ask_price: Option<Price>,
        bid_size: Option<Quantity>,
        ask_size: Option<Quantity>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<QuoteTick> {
        let cached = self.quotes.get(&instrument_id);

        // Resolve each field: use provided value or fall back to cache
        let Some(bid_price) = bid_price.or_else(|| cached.map(|quote| quote.bid_price)) else {
            anyhow::bail!(
                "Cannot process partial quote for {instrument_id}: missing bid_price and no cached value"
            );
        };

        let Some(ask_price) = ask_price.or_else(|| cached.map(|quote| quote.ask_price)) else {
            anyhow::bail!(
                "Cannot process partial quote for {instrument_id}: missing ask_price and no cached value"
            );
        };

        let Some(bid_size) = bid_size.or_else(|| cached.map(|quote| quote.bid_size)) else {
            anyhow::bail!(
                "Cannot process partial quote for {instrument_id}: missing bid_size and no cached value"
            );
        };

        let Some(ask_size) = ask_size.or_else(|| cached.map(|quote| quote.ask_size)) else {
            anyhow::bail!(
                "Cannot process partial quote for {instrument_id}: missing ask_size and no cached value"
            );
        };

        let quote = QuoteTick::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        );

        self.quotes.insert(instrument_id, quote);

        Ok(quote)
    }
}

impl Default for QuoteCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::types::{Price, Quantity};
    use rstest::rstest;

    use super::*;

    fn make_quote(instrument_id: InstrumentId, bid: &str, ask: &str) -> QuoteTick {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("10.0"),
            Quantity::from("20.0"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    #[rstest]
    fn test_new_cache_is_empty() {
        let cache = QuoteCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[rstest]
    fn test_insert_and_get() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let quote = make_quote(instrument_id, "100.0", "101.0");

        assert_eq!(cache.insert(instrument_id, quote), None);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(&instrument_id));
        assert_eq!(cache.get(&instrument_id), Some(&quote));
    }

    #[rstest]
    fn test_insert_returns_previous_value() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let quote1 = make_quote(instrument_id, "100.0", "101.0");
        let quote2 = make_quote(instrument_id, "102.0", "103.0");

        cache.insert(instrument_id, quote1);
        let previous = cache.insert(instrument_id, quote2);

        assert_eq!(previous, Some(quote1));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&instrument_id), Some(&quote2));
    }

    #[rstest]
    fn test_remove() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let quote = make_quote(instrument_id, "100.0", "101.0");

        cache.insert(instrument_id, quote);
        assert_eq!(cache.remove(&instrument_id), Some(quote));
        assert!(cache.is_empty());
        assert!(!cache.contains(&instrument_id));
        assert_eq!(cache.get(&instrument_id), None);
    }

    #[rstest]
    fn test_remove_nonexistent() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

        assert_eq!(cache.remove(&instrument_id), None);
    }

    #[rstest]
    fn test_clear() {
        let mut cache = QuoteCache::new();
        let id1 = InstrumentId::from("BTCUSDT.BINANCE");
        let id2 = InstrumentId::from("ETHUSDT.BINANCE");

        cache.insert(id1, make_quote(id1, "100.0", "101.0"));
        cache.insert(id2, make_quote(id2, "200.0", "201.0"));

        assert_eq!(cache.len(), 2);

        cache.clear();

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert!(!cache.contains(&id1));
        assert!(!cache.contains(&id2));
    }

    #[rstest]
    fn test_multiple_instruments() {
        let mut cache = QuoteCache::new();
        let id1 = InstrumentId::from("BTCUSDT.BINANCE");
        let id2 = InstrumentId::from("ETHUSDT.BINANCE");
        let id3 = InstrumentId::from("XRPUSDT.BINANCE");

        let quote1 = make_quote(id1, "100.0", "101.0");
        let quote2 = make_quote(id2, "200.0", "201.0");
        let quote3 = make_quote(id3, "0.50", "0.51");

        cache.insert(id1, quote1);
        cache.insert(id2, quote2);
        cache.insert(id3, quote3);

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&id1), Some(&quote1));
        assert_eq!(cache.get(&id2), Some(&quote2));
        assert_eq!(cache.get(&id3), Some(&quote3));
    }

    #[rstest]
    fn test_default() {
        let cache = QuoteCache::default();
        assert!(cache.is_empty());
    }

    #[rstest]
    fn test_clone() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let quote = make_quote(instrument_id, "100.0", "101.0");

        cache.insert(instrument_id, quote);

        let cloned = cache.clone();
        assert_eq!(cloned.len(), 1);
        assert_eq!(cloned.get(&instrument_id), Some(&quote));
    }

    #[rstest]
    fn test_process_complete_quote() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

        let result = cache.process(
            instrument_id,
            Some(Price::from("100.5")),
            Some(Price::from("101.0")),
            Some(Quantity::from("10.0")),
            Some(Quantity::from("20.0")),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_ok());
        let quote = result.unwrap();
        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("100.5"));
        assert_eq!(quote.ask_price, Price::from("101.0"));
        assert_eq!(quote.bid_size, Quantity::from("10.0"));
        assert_eq!(quote.ask_size, Quantity::from("20.0"));

        // Should be cached
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&instrument_id), Some(&quote));
    }

    #[rstest]
    #[case::bid_price("bid_price")]
    #[case::ask_price("ask_price")]
    #[case::bid_size("bid_size")]
    #[case::ask_size("ask_size")]
    fn test_process_partial_quote_without_cache(#[case] missing_field: &str) {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let mut bid_price = Some(Price::from("100.0"));
        let mut ask_price = Some(Price::from("101.0"));
        let mut bid_size = Some(Quantity::from("10.0"));
        let mut ask_size = Some(Quantity::from("20.0"));

        match missing_field {
            "bid_price" => bid_price = None,
            "ask_price" => ask_price = None,
            "bid_size" => bid_size = None,
            "ask_size" => ask_size = None,
            _ => unreachable!(),
        }

        let error = cache
            .process(
                instrument_id,
                bid_price,
                ask_price,
                bid_size,
                ask_size,
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Cannot process partial quote for {instrument_id}: missing {missing_field} and no cached value"
            )
        );
        assert!(cache.is_empty());
    }

    #[rstest]
    fn test_process_partial_quote_with_cache() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

        // First, process a complete quote
        let first_quote = cache
            .process(
                instrument_id,
                Some(Price::from("100.0")),
                Some(Price::from("101.0")),
                Some(Quantity::from("10.0")),
                Some(Quantity::from("20.0")),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .unwrap();

        // Now process partial update with only bid side
        let result = cache.process(
            instrument_id,
            Some(Price::from("100.5")),
            None, // Use cached ask_price
            Some(Quantity::from("15.0")),
            None, // Use cached ask_size
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_ok());
        let quote = result.unwrap();

        // Bid side should be updated
        assert_eq!(quote.bid_price, Price::from("100.5"));
        assert_eq!(quote.bid_size, Quantity::from("15.0"));

        // Ask side should be from cache
        assert_eq!(quote.ask_price, first_quote.ask_price);
        assert_eq!(quote.ask_size, first_quote.ask_size);

        // Cache should be updated with new quote
        assert_eq!(cache.get(&instrument_id), Some(&quote));
    }

    #[rstest]
    fn test_process_partial_quote_uses_cached_bid_side() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let first_quote = cache
            .process(
                instrument_id,
                Some(Price::from("100.0")),
                Some(Price::from("101.0")),
                Some(Quantity::from("10.0")),
                Some(Quantity::from("20.0")),
                UnixNanos::from(1),
                UnixNanos::from(2),
            )
            .unwrap();

        let quote = cache
            .process(
                instrument_id,
                None,
                Some(Price::from("101.5")),
                None,
                Some(Quantity::from("25.0")),
                UnixNanos::from(3),
                UnixNanos::from(4),
            )
            .unwrap();

        assert_eq!(quote.bid_price, first_quote.bid_price);
        assert_eq!(quote.ask_price, Price::from("101.5"));
        assert_eq!(quote.bid_size, first_quote.bid_size);
        assert_eq!(quote.ask_size, Quantity::from("25.0"));
        assert_eq!(quote.ts_event, UnixNanos::from(3));
        assert_eq!(quote.ts_init, UnixNanos::from(4));
        assert_eq!(cache.get(&instrument_id), Some(&quote));
    }

    #[rstest]
    fn test_process_updates_cache() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

        // First quote
        cache
            .process(
                instrument_id,
                Some(Price::from("100.0")),
                Some(Price::from("101.0")),
                Some(Quantity::from("10.0")),
                Some(Quantity::from("20.0")),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .unwrap();

        // Second complete quote should replace cached values
        let quote2 = cache
            .process(
                instrument_id,
                Some(Price::from("102.0")),
                Some(Price::from("103.0")),
                Some(Quantity::from("30.0")),
                Some(Quantity::from("40.0")),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .unwrap();

        assert_eq!(cache.get(&instrument_id), Some(&quote2));
        assert_eq!(quote2.bid_price, Price::from("102.0"));
    }

    #[rstest]
    fn test_process_multiple_instruments() {
        let mut cache = QuoteCache::new();
        let id1 = InstrumentId::from("BTCUSDT.BINANCE");
        let id2 = InstrumentId::from("ETHUSDT.BINANCE");

        let quote1 = cache
            .process(
                id1,
                Some(Price::from("100.0")),
                Some(Price::from("101.0")),
                Some(Quantity::from("10.0")),
                Some(Quantity::from("20.0")),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .unwrap();

        let quote2 = cache
            .process(
                id2,
                Some(Price::from("200.0")),
                Some(Price::from("201.0")),
                Some(Quantity::from("30.0")),
                Some(Quantity::from("40.0")),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .unwrap();

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&id1), Some(&quote1));
        assert_eq!(cache.get(&id2), Some(&quote2));
    }

    #[rstest]
    fn test_process_clear_removes_cached_values() {
        let mut cache = QuoteCache::new();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

        // Add a quote
        cache
            .process(
                instrument_id,
                Some(Price::from("100.0")),
                Some(Price::from("101.0")),
                Some(Quantity::from("10.0")),
                Some(Quantity::from("20.0")),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .unwrap();

        assert_eq!(cache.len(), 1);

        // Clear cache
        cache.clear();

        // Partial update should now fail (no cached values)
        let result = cache.process(
            instrument_id,
            Some(Price::from("100.5")),
            None,
            Some(Quantity::from("15.0")),
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_err());
    }
}
