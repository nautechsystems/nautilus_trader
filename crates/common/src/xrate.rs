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

//! Exchange rate calculations between currencies.
//!
//! An exchange rate is the value of one asset versus that of another.

use ahash::{AHashMap, AHashSet};
use nautilus_model::enums::PriceType;
use rust_decimal::Decimal;
use ustr::Ustr;

/// Calculates the exchange rate between two currencies using provided bid and ask quotes.
///
/// This function builds a graph of direct conversion rates from the quotes and uses a DFS to
/// accumulate the conversion rate along a valid conversion path. While a full Floyd–Warshall
/// algorithm could compute all-pairs conversion rates, the DFS approach here provides a quick
/// solution for a single conversion query.
///
/// # Errors
///
/// Returns an error if:
/// - `price_type` is equal to `Last` or `Mark` (cannot calculate from quotes).
/// - `quotes_bid` or `quotes_ask` is empty.
/// - `quotes_bid` and `quotes_ask` lengths are not equal.
/// - The bid or ask side of a pair is missing.
pub fn get_exchange_rate(
    from_currency: Ustr,
    to_currency: Ustr,
    price_type: PriceType,
    quotes_bid: AHashMap<Ustr, Decimal>,
    quotes_ask: AHashMap<Ustr, Decimal>,
) -> anyhow::Result<Option<Decimal>> {
    if from_currency == to_currency {
        // When the source and target currencies are identical,
        // no conversion is needed; return an exchange rate of one.
        return Ok(Some(Decimal::ONE));
    }

    if quotes_bid.is_empty() || quotes_ask.is_empty() {
        anyhow::bail!("Quote maps must not be empty");
    }

    if quotes_bid.len() != quotes_ask.len() {
        anyhow::bail!("Quote maps must have equal lengths");
    }

    // Build effective quotes based on the requested price type
    let effective_quotes: AHashMap<Ustr, Decimal> = match price_type {
        PriceType::Bid => quotes_bid,
        PriceType::Ask => quotes_ask,
        PriceType::Mid => {
            let mut mid_quotes = AHashMap::new();

            for (pair, bid) in &quotes_bid {
                let ask = quotes_ask
                    .get(pair)
                    .ok_or_else(|| anyhow::anyhow!("Missing ask quote for pair {pair}"))?;
                mid_quotes.insert(*pair, (bid + ask) / Decimal::TWO);
            }
            mid_quotes
        }
        _ => anyhow::bail!("Invalid `price_type`, was '{price_type}'"),
    };

    // Construct a graph: each currency maps to its neighbors and corresponding conversion rate
    let mut graph: AHashMap<Ustr, Vec<(Ustr, Decimal)>> = AHashMap::new();
    for (pair, rate) in effective_quotes {
        let parts: Vec<&str> = pair.split('/').collect();
        if parts.len() != 2 {
            log::warn!("Skipping invalid pair string: {pair}");
            continue;
        }

        if rate <= Decimal::ZERO {
            // A non-positive quote would divide by zero on the inverse edge
            log::warn!("Skipping non-positive rate for pair {pair}");
            continue;
        }
        let base = Ustr::from(parts[0]);
        let quote = Ustr::from(parts[1]);

        graph.entry(base).or_default().push((quote, rate));
        graph
            .entry(quote)
            .or_default()
            .push((base, Decimal::ONE / rate));
    }

    // DFS: search for a conversion path from `from_currency` to `to_currency`
    let mut stack: Vec<(Ustr, Decimal)> = vec![(from_currency, Decimal::ONE)];
    let mut visited: AHashSet<Ustr> = AHashSet::new();
    visited.insert(from_currency);

    while let Some((current, current_rate)) = stack.pop() {
        if current == to_currency {
            return Ok(Some(current_rate));
        }

        if let Some(neighbors) = graph.get(&current) {
            for (neighbor, rate) in neighbors {
                if visited.insert(*neighbor) {
                    stack.push((*neighbor, current_rate * rate));
                }
            }
        }
    }

    // No conversion path found
    Ok(None)
}

#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;

    fn setup_test_quotes() -> (AHashMap<Ustr, Decimal>, AHashMap<Ustr, Decimal>) {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();

        // Direct pairs
        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1000));
        quotes_ask.insert(Ustr::from("EUR/USD"), dec!(1.1002));

        quotes_bid.insert(Ustr::from("GBP/USD"), dec!(1.3000));
        quotes_ask.insert(Ustr::from("GBP/USD"), dec!(1.3002));

        quotes_bid.insert(Ustr::from("USD/JPY"), dec!(110.00));
        quotes_ask.insert(Ustr::from("USD/JPY"), dec!(110.02));

        quotes_bid.insert(Ustr::from("AUD/USD"), dec!(0.7500));
        quotes_ask.insert(Ustr::from("AUD/USD"), dec!(0.7502));

        (quotes_bid, quotes_ask)
    }

    #[rstest]
    fn test_invalid_pair_string() {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        // Invalid pair string (missing '/')
        quotes_bid.insert(Ustr::from("EURUSD"), dec!(1.1000));
        quotes_ask.insert(Ustr::from("EURUSD"), dec!(1.1002));
        // Valid pair string
        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1000));
        quotes_ask.insert(Ustr::from("EUR/USD"), dec!(1.1002));

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        assert_eq!(rate, Some(dec!(1.1001)));
    }

    #[rstest]
    fn test_same_currency() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();
        let rate = get_exchange_rate(
            Ustr::from("USD"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();
        assert_eq!(rate, Some(Decimal::ONE));
    }

    #[rstest(
        price_type,
        expected,
        case(PriceType::Bid, dec!(1.1000)),
        case(PriceType::Ask, dec!(1.1002)),
        case(PriceType::Mid, dec!(1.1001))
    )]
    fn test_direct_pair(price_type: PriceType, expected: Decimal) {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            price_type,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        let rate = rate.unwrap_or_else(|| panic!("Expected a conversion rate for {price_type}"));
        assert_eq!(rate, expected);
    }

    #[rstest]
    fn test_inverse_pair() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let rate_eur_usd = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid.clone(),
            quotes_ask.clone(),
        )
        .unwrap();
        let rate_usd_eur = get_exchange_rate(
            Ustr::from("USD"),
            Ustr::from("EUR"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        if let (Some(eur_usd), Some(usd_eur)) = (rate_eur_usd, rate_usd_eur) {
            // Inverse-edge rounding makes the round-trip near one, not exactly one
            assert!((eur_usd * usd_eur - Decimal::ONE).abs() < dec!(0.0001));
        } else {
            panic!("Expected valid conversion rates for inverse conversion");
        }
    }

    #[rstest]
    fn test_cross_pair_through_usd() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();
        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("JPY"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();
        // Expected rate: (EUR/USD mid) * (USD/JPY mid)
        let expected = dec!(1.1001) * dec!(110.01);

        assert_eq!(rate, Some(expected));
    }

    #[rstest]
    #[case(dec!(0))]
    #[case(dec!(-1.1))]
    fn test_non_positive_rate_is_skipped(#[case] rate: Decimal) {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        quotes_bid.insert(Ustr::from("EUR/USD"), rate);
        quotes_ask.insert(Ustr::from("EUR/USD"), rate);

        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(result.unwrap(), None);
    }

    #[rstest]
    fn test_no_conversion_path() {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();

        // Only one pair provided
        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1000));
        quotes_ask.insert(Ustr::from("EUR/USD"), dec!(1.1002));

        // Attempt conversion from EUR to JPY should yield None
        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("JPY"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();
        assert_eq!(rate, None);
    }

    #[rstest]
    fn test_empty_quotes() {
        let quotes_bid: AHashMap<Ustr, Decimal> = AHashMap::new();
        let quotes_ask: AHashMap<Ustr, Decimal> = AHashMap::new();
        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_unequal_quotes_length() {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();

        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1000));
        quotes_bid.insert(Ustr::from("GBP/USD"), dec!(1.3000));
        quotes_ask.insert(Ustr::from("EUR/USD"), dec!(1.1002));
        // Missing GBP/USD in ask quotes.

        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_invalid_price_type() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();
        // Using an invalid price type variant (assume PriceType::Last is unsupported)
        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Last,
            quotes_bid,
            quotes_ask,
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_cycle_handling() {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        // Create a cycle by including both EUR/USD and USD/EUR quotes
        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1));
        quotes_ask.insert(Ustr::from("EUR/USD"), dec!(1.1002));
        quotes_bid.insert(Ustr::from("USD/EUR"), dec!(0.909));
        quotes_ask.insert(Ustr::from("USD/EUR"), dec!(0.9091));

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        // Edge order is non-deterministic, so allow a small tolerance around the mid rate
        let expected = dec!(1.1001);
        assert!((rate.unwrap() - expected).abs() < dec!(0.0001));
    }

    #[rstest]
    fn test_multiple_paths() {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        // Direct conversion
        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1000));
        quotes_ask.insert(Ustr::from("EUR/USD"), dec!(1.1002));
        // Indirect path via GBP: EUR/GBP and GBP/USD
        quotes_bid.insert(Ustr::from("EUR/GBP"), dec!(0.8461));
        quotes_ask.insert(Ustr::from("EUR/GBP"), dec!(0.8463));
        quotes_bid.insert(Ustr::from("GBP/USD"), dec!(1.3000));
        quotes_ask.insert(Ustr::from("GBP/USD"), dec!(1.3002));

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        // Both paths should be consistent:
        let direct = dec!(1.1001);
        let indirect = dec!(0.8462) * dec!(1.3001);
        assert!((direct - indirect).abs() < dec!(0.0001));
        assert!((rate.unwrap() - direct).abs() < dec!(0.0001));
    }
}
