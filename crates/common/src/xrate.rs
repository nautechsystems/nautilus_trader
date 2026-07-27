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
/// accumulate the conversion rate along a valid conversion path. While a full Floyd-Warshall
/// algorithm could compute all-pairs conversion rates, the DFS approach here provides a quick
/// solution for a single conversion query.
///
/// # Errors
///
/// For conversions between distinct currencies (an identical `from_currency` and `to_currency`
/// returns a rate of one without inspecting the quotes), returns an error if:
/// - `quotes_bid` or `quotes_ask` is empty.
/// - `quotes_bid` and `quotes_ask` lengths are not equal.
/// - `price_type` is equal to `Last` or `Mark` (cannot calculate from quotes).
/// - The bid or ask side of a pair is missing.
pub fn get_exchange_rate(
    from_currency: Ustr,
    to_currency: Ustr,
    price_type: PriceType,
    quotes_bid: AHashMap<Ustr, Decimal>,
    mut quotes_ask: AHashMap<Ustr, Decimal>,
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

    // Validated here, in the same position as the price-type match this replaced, so the
    // identical-currency shortcut and the quote-map errors keep their original precedence.
    if !matches!(price_type, PriceType::Bid | PriceType::Ask | PriceType::Mid) {
        anyhow::bail!("Invalid `price_type`, was '{price_type}'");
    }

    // Construct a graph: each currency maps to its neighbors and corresponding conversion rate
    let mut graph: AHashMap<Ustr, Vec<(Ustr, Decimal)>> = AHashMap::new();

    for (pair, bid) in quotes_bid {
        let ask = quotes_ask
            .remove(&pair)
            .ok_or_else(|| anyhow::anyhow!("Missing ask quote for pair {pair}"))?;
        let parts: Vec<&str> = pair.split('/').collect();

        if parts.len() != 2 {
            log::warn!("Skipping invalid pair string: {pair}");
            continue;
        }

        if bid <= Decimal::ZERO || ask <= Decimal::ZERO {
            // Both sides are required to build valid forward and reverse edges.
            log::warn!("Skipping pair with non-positive bid or ask rate: {pair}");
            continue;
        }

        let base = Ustr::from(parts[0]);
        let quote = Ustr::from(parts[1]);
        let (forward_rate, reverse_rate) = directional_rates(bid, ask, price_type);

        graph.entry(base).or_default().push((quote, forward_rate));
        graph.entry(quote).or_default().push((base, reverse_rate));
    }

    // Descending total order makes the smallest distinct neighbor the first branch popped from
    // the LIFO stack. The rate only breaks ties between parallel edges.
    for neighbors in graph.values_mut() {
        neighbors.sort_unstable_by(|left, right| right.cmp(left));
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

fn directional_rates(bid: Decimal, ask: Decimal, price_type: PriceType) -> (Decimal, Decimal) {
    match price_type {
        PriceType::Bid => (bid, Decimal::ONE / ask),
        PriceType::Ask => (ask, Decimal::ONE / bid),
        PriceType::Mid => {
            let mid = (bid + ask) / Decimal::TWO;
            (mid, Decimal::ONE / mid)
        }
        _ => unreachable!("Price type was validated before graph construction"),
    }
}

#[cfg(test)]
mod tests {
    use ahash::{AHashMap, RandomState};
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

    #[rstest(
        price_type,
        expected,
        case(PriceType::Bid, Decimal::ONE / dec!(1.1002)),
        case(PriceType::Ask, Decimal::ONE / dec!(1.1000)),
        case(PriceType::Mid, Decimal::ONE / dec!(1.1001))
    )]
    fn test_inverse_pair_uses_opposite_spread_side(price_type: PriceType, expected: Decimal) {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let rate = get_exchange_rate(
            Ustr::from("USD"),
            Ustr::from("EUR"),
            price_type,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        assert_eq!(rate, Some(expected));
    }

    #[rstest]
    fn test_indirect_route_is_deterministic_across_hash_seeds() {
        let pairs = [
            ("AAA/BBB", dec!(2)),
            ("BBB/DDD", dec!(3)),
            ("AAA/CCC", dec!(5)),
            ("CCC/DDD", dec!(7)),
        ];
        let seeds = [(0, 0, 0, 0), (1, 2, 3, 4), (5, 6, 7, 8), (10, 20, 30, 40)];
        let mut iteration_orders = Vec::new();

        let rates = seeds.map(|(k0, k1, k2, k3)| {
            let random_state = RandomState::with_seeds(k0, k1, k2, k3);
            let mut quotes_bid = AHashMap::with_hasher(random_state.clone());
            let mut quotes_ask = AHashMap::with_hasher(random_state);

            for (pair, rate) in pairs {
                quotes_bid.insert(Ustr::from(pair), rate);
                quotes_ask.insert(Ustr::from(pair), rate);
            }
            iteration_orders.push(quotes_bid.keys().copied().collect::<Vec<_>>());

            get_exchange_rate(
                Ustr::from("AAA"),
                Ustr::from("DDD"),
                PriceType::Bid,
                quotes_bid,
                quotes_ask,
            )
            .unwrap()
        });

        assert!(
            iteration_orders
                .windows(2)
                .any(|orders| orders[0] != orders[1]),
            "Explicit hash seeds must produce different raw iteration orders",
        );
        assert_eq!(rates, [Some(dec!(6)); 4]);
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

    #[rstest(
        bid,
        ask,
        price_type,
        case(dec!(1.1), Decimal::ZERO, PriceType::Bid),
        case(Decimal::ZERO, dec!(1.1), PriceType::Ask)
    )]
    fn test_non_positive_opposite_side_is_skipped(
        bid: Decimal,
        ask: Decimal,
        price_type: PriceType,
    ) {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        quotes_bid.insert(Ustr::from("EUR/USD"), bid);
        quotes_ask.insert(Ustr::from("EUR/USD"), ask);

        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            price_type,
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
    fn test_equal_length_quotes_with_different_keys() {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1000));
        quotes_ask.insert(Ustr::from("GBP/USD"), dec!(1.3002));

        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Bid,
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
    fn test_same_currency_shortcut_precedes_all_validation() {
        // The identical-currency shortcut runs before any quote or price-type validation, so an
        // unsupported price type and empty quote maps still yield a rate of one.
        let result = get_exchange_rate(
            Ustr::from("USD"),
            Ustr::from("USD"),
            PriceType::Last,
            AHashMap::new(),
            AHashMap::new(),
        );

        assert_eq!(result.unwrap(), Some(Decimal::ONE));
    }

    #[rstest]
    fn test_quote_map_errors_precede_invalid_price_type() {
        // Empty maps are reported before an unsupported price type for distinct currencies.
        let empty = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Last,
            AHashMap::new(),
            AHashMap::new(),
        );

        assert_eq!(
            empty.unwrap_err().to_string(),
            "Quote maps must not be empty"
        );

        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        quotes_bid.insert(Ustr::from("EUR/USD"), dec!(1.1000));
        quotes_bid.insert(Ustr::from("GBP/USD"), dec!(1.3000));
        quotes_ask.insert(Ustr::from("EUR/USD"), dec!(1.1002));

        let unequal = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Last,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(
            unequal.unwrap_err().to_string(),
            "Quote maps must have equal lengths"
        );
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

        // The total adjacency ordering encounters the higher parallel rate first.
        let expected = dec!(1.1001);
        assert_eq!(rate, Some(expected));
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
