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

//! Deduplication cache for derivative ticker data.

use ahash::AHashMap;
use nautilus_model::{
    data::{FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate},
    identifiers::InstrumentId,
    types::Price,
};

/// Caches last-emitted derivative ticker values per instrument to suppress
/// duplicates when the exchange re-sends unchanged fields.
#[derive(Debug, Default)]
pub struct DerivativeTickerCache {
    funding_rates: AHashMap<InstrumentId, FundingRateUpdate>,
    mark_prices: AHashMap<InstrumentId, Price>,
    index_prices: AHashMap<InstrumentId, Price>,
}

impl DerivativeTickerCache {
    /// Returns `true` if the funding rate changed (or is new) and should be emitted.
    pub fn should_emit_funding_rate(&mut self, update: &FundingRateUpdate) -> bool {
        let id = update.instrument_id;

        if let Some(cached) = self.funding_rates.get(&id)
            && cached == update
        {
            return false;
        }

        self.funding_rates.insert(id, *update);
        true
    }

    /// Returns `true` if the mark price changed (or is new) and should be emitted.
    pub fn should_emit_mark_price(&mut self, update: &MarkPriceUpdate) -> bool {
        let id = update.instrument_id;

        if let Some(cached) = self.mark_prices.get(&id)
            && *cached == update.value
        {
            return false;
        }

        self.mark_prices.insert(id, update.value);
        true
    }

    /// Returns `true` if the index price changed (or is new) and should be emitted.
    pub fn should_emit_index_price(&mut self, update: &IndexPriceUpdate) -> bool {
        let id = update.instrument_id;

        if let Some(cached) = self.index_prices.get(&id)
            && *cached == update.value
        {
            return false;
        }

        self.index_prices.insert(id, update.value);
        true
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate},
        identifiers::InstrumentId,
        types::Price,
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTCUSDT-PERP.BINANCE")
    }

    #[rstest]
    fn test_funding_rate_first_value_emits() {
        let mut cache = DerivativeTickerCache::default();
        let update = FundingRateUpdate::new(
            instrument_id(),
            dec!(0.0001),
            None,
            None,
            1.into(),
            2.into(),
        );
        assert!(cache.should_emit_funding_rate(&update));
    }

    #[rstest]
    fn test_funding_rate_duplicate_suppressed() {
        let mut cache = DerivativeTickerCache::default();
        let update = FundingRateUpdate::new(
            instrument_id(),
            dec!(0.0001),
            None,
            None,
            1.into(),
            2.into(),
        );
        cache.should_emit_funding_rate(&update);

        let duplicate = FundingRateUpdate::new(
            instrument_id(),
            dec!(0.0001),
            None,
            None,
            3.into(),
            4.into(),
        );
        assert!(!cache.should_emit_funding_rate(&duplicate));
    }

    #[rstest]
    fn test_funding_rate_changed_value_emits() {
        let mut cache = DerivativeTickerCache::default();
        let first = FundingRateUpdate::new(
            instrument_id(),
            dec!(0.0001),
            None,
            None,
            1.into(),
            2.into(),
        );
        cache.should_emit_funding_rate(&first);

        let changed = FundingRateUpdate::new(
            instrument_id(),
            dec!(0.0002),
            None,
            None,
            3.into(),
            4.into(),
        );
        assert!(cache.should_emit_funding_rate(&changed));
    }

    #[rstest]
    fn test_mark_price_first_value_emits() {
        let mut cache = DerivativeTickerCache::default();
        let update =
            MarkPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 1.into(), 2.into());
        assert!(cache.should_emit_mark_price(&update));
    }

    #[rstest]
    fn test_mark_price_duplicate_suppressed() {
        let mut cache = DerivativeTickerCache::default();
        let update =
            MarkPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 1.into(), 2.into());
        cache.should_emit_mark_price(&update);

        let duplicate =
            MarkPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 3.into(), 4.into());
        assert!(!cache.should_emit_mark_price(&duplicate));
    }

    #[rstest]
    fn test_mark_price_changed_value_emits() {
        let mut cache = DerivativeTickerCache::default();
        let first =
            MarkPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 1.into(), 2.into());
        cache.should_emit_mark_price(&first);

        let changed =
            MarkPriceUpdate::new(instrument_id(), Price::new(42001.0, 1), 3.into(), 4.into());
        assert!(cache.should_emit_mark_price(&changed));
    }

    #[rstest]
    fn test_index_price_first_value_emits() {
        let mut cache = DerivativeTickerCache::default();
        let update =
            IndexPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 1.into(), 2.into());
        assert!(cache.should_emit_index_price(&update));
    }

    #[rstest]
    fn test_index_price_duplicate_suppressed() {
        let mut cache = DerivativeTickerCache::default();
        let update =
            IndexPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 1.into(), 2.into());
        cache.should_emit_index_price(&update);

        let duplicate =
            IndexPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 3.into(), 4.into());
        assert!(!cache.should_emit_index_price(&duplicate));
    }

    #[rstest]
    fn test_index_price_changed_value_emits() {
        let mut cache = DerivativeTickerCache::default();
        let first =
            IndexPriceUpdate::new(instrument_id(), Price::new(42000.0, 1), 1.into(), 2.into());
        cache.should_emit_index_price(&first);

        let changed =
            IndexPriceUpdate::new(instrument_id(), Price::new(42001.0, 1), 3.into(), 4.into());
        assert!(cache.should_emit_index_price(&changed));
    }
}
