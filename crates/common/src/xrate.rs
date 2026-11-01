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

type RateGraph = AHashMap<Ustr, Vec<(Ustr, Decimal)>>;

/// Calculates the exchange rate between two currencies using provided bid and ask quotes.
///
/// This function builds a graph of direct conversion rates from the quotes and searches for a
/// conversion path whose accumulated rate is representable as a positive `Decimal`.
/// If a path calculation is unrepresentable, the search may examine every simple path. In the
/// worst case, this takes exponential time in dense graphs, even when a valid route exists.
///
/// Identical `from_currency` and `to_currency` return a rate of one without inspecting the quotes.
/// Returns `Ok(None)` when no path is found and all quote and path calculations are representable.
///
/// # Errors
///
/// Returns an error if:
/// - `quotes_bid` or `quotes_ask` is empty.
/// - `quotes_bid` and `quotes_ask` lengths are not equal.
/// - `price_type` is equal to `Last` or `Mark` (cannot calculate from quotes).
/// - The bid or ask side of a pair is missing.
/// - No rate is found, and a calculation for any supplied pair or searched path overflows or
///   underflows to zero.
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

    // Validated here, in the same position as the price-type match this replaced, so the
    // identical-currency shortcut and the quote-map errors keep their original precedence.
    if !matches!(price_type, PriceType::Bid | PriceType::Ask | PriceType::Mid) {
        anyhow::bail!("Invalid `price_type`, was '{price_type}'");
    }

    let (graph, unrepresentable) = build_rate_graph(quotes_bid, quotes_ask, price_type)?;
    find_representable_rate(&graph, from_currency, to_currency, unrepresentable)
}

pub(crate) fn get_exchange_rate_with_alternates(
    from_currency: Ustr,
    to_currency: Ustr,
    price_type: PriceType,
    quotes_bid: &AHashMap<Ustr, Decimal>,
    quotes_ask: &AHashMap<Ustr, Decimal>,
    alternates: &[(Ustr, Decimal, Decimal)],
) -> anyhow::Result<Option<Decimal>> {
    let preferred = get_exchange_rate(
        from_currency,
        to_currency,
        price_type,
        quotes_bid.clone(),
        quotes_ask.clone(),
    );

    if matches!(preferred, Ok(Some(_))) || alternates.is_empty() {
        return preferred;
    }

    if !matches!(price_type, PriceType::Bid | PriceType::Ask | PriceType::Mid) {
        return preferred;
    }

    let (mut graph, mut unrepresentable) =
        build_rate_graph(quotes_bid.clone(), quotes_ask.clone(), price_type)?;
    for &(pair, bid, ask) in alternates {
        add_pair_rates(&mut graph, pair, bid, ask, price_type, &mut unrepresentable);
    }

    if !is_connected(&graph, from_currency, to_currency) {
        return preferred;
    }

    sort_rate_graph(&mut graph);
    let available = find_representable_rate(&graph, from_currency, to_currency, unrepresentable);
    if !matches!(available, Ok(Some(_))) {
        return available;
    }

    if let Some(rate) = find_rate_with_quote_choices(
        from_currency,
        to_currency,
        price_type,
        quotes_bid,
        quotes_ask,
        alternates,
    ) {
        return Ok(Some(rate));
    }

    available
}

fn build_rate_graph(
    quotes_bid: AHashMap<Ustr, Decimal>,
    mut quotes_ask: AHashMap<Ustr, Decimal>,
    price_type: PriceType,
) -> anyhow::Result<(RateGraph, bool)> {
    // Construct a graph: each currency maps to its neighbors and corresponding conversion rate
    let mut graph = RateGraph::new();
    let mut unrepresentable = false;

    for (pair, bid) in quotes_bid {
        let ask = quotes_ask
            .remove(&pair)
            .ok_or_else(|| anyhow::anyhow!("Missing ask quote for pair {pair}"))?;
        add_pair_rates(&mut graph, pair, bid, ask, price_type, &mut unrepresentable);
    }

    sort_rate_graph(&mut graph);

    Ok((graph, unrepresentable))
}

fn find_rate_with_quote_choices(
    from_currency: Ustr,
    to_currency: Ustr,
    price_type: PriceType,
    quotes_bid: &AHashMap<Ustr, Decimal>,
    quotes_ask: &AHashMap<Ustr, Decimal>,
    alternates: &[(Ustr, Decimal, Decimal)],
) -> Option<Decimal> {
    for count in 1..=alternates.len() {
        let mut indices: Vec<usize> = (0..count).collect();

        loop {
            let mut bid = quotes_bid.clone();
            let mut ask = quotes_ask.clone();

            for &index in &indices {
                let (pair, alternate_bid, alternate_ask) = alternates[index];
                bid.insert(pair, alternate_bid);
                ask.insert(pair, alternate_ask);
            }

            if let Ok(Some(rate)) =
                get_exchange_rate(from_currency, to_currency, price_type, bid, ask)
            {
                return Some(rate);
            }

            let mut position = count;
            while position > 0 && indices[position - 1] == alternates.len() - count + position - 1 {
                position -= 1;
            }

            if position == 0 {
                break;
            }

            indices[position - 1] += 1;
            for index in position..count {
                indices[index] = indices[index - 1] + 1;
            }
        }
    }

    None
}

fn add_pair_rates(
    graph: &mut RateGraph,
    pair: Ustr,
    bid: Decimal,
    ask: Decimal,
    price_type: PriceType,
    unrepresentable: &mut bool,
) {
    let mut parts = pair.split('/');

    let (Some(base), Some(quote), None) = (parts.next(), parts.next(), parts.next()) else {
        log::warn!("Skipping invalid pair string: {pair}");
        return;
    };

    if bid <= Decimal::ZERO || ask <= Decimal::ZERO {
        // Both sides are required to build valid forward and reverse edges.
        log::warn!("Skipping pair with non-positive bid or ask rate: {pair}");
        return;
    }

    let base = Ustr::from(base);
    let quote = Ustr::from(quote);
    let (forward_rate, reverse_rate) = directional_rates(bid, ask, price_type);

    if let Some(rate) = forward_rate {
        graph.entry(base).or_default().push((quote, rate));
    } else {
        *unrepresentable = true;
    }

    if let Some(rate) = reverse_rate {
        graph.entry(quote).or_default().push((base, rate));
    } else {
        *unrepresentable = true;
    }
}

fn sort_rate_graph(graph: &mut RateGraph) {
    // The LIFO fast search visits smaller neighbors first and favors higher parallel rates.
    for neighbors in graph.values_mut() {
        neighbors.sort_unstable_by(|left, right| right.cmp(left));
    }
}

fn find_representable_rate(
    graph: &RateGraph,
    from_currency: Ustr,
    to_currency: Ustr,
    unrepresentable: bool,
) -> anyhow::Result<Option<Decimal>> {
    let (rate, path_unrepresentable) =
        find_rate_without_backtracking(graph, from_currency, to_currency);

    if rate.is_some() {
        return Ok(rate);
    }

    if path_unrepresentable
        && is_connected(graph, from_currency, to_currency)
        && let Some(rate) = find_rate_with_backtracking(graph, from_currency, to_currency)
    {
        return Ok(Some(rate));
    }

    if unrepresentable || path_unrepresentable {
        anyhow::bail!("No representable exchange rate from {from_currency} to {to_currency}");
    }

    Ok(None)
}

fn find_rate_without_backtracking(
    graph: &RateGraph,
    from_currency: Ustr,
    to_currency: Ustr,
) -> (Option<Decimal>, bool) {
    let mut stack = vec![(from_currency, Decimal::ONE)];
    let mut visited: AHashSet<Ustr> = AHashSet::new();
    visited.insert(from_currency);
    let mut path_unrepresentable = false;

    while let Some((current, current_rate)) = stack.pop() {
        if current == to_currency {
            return (Some(current_rate), path_unrepresentable);
        }

        let neighbors = graph.get(&current).map_or(&[][..], Vec::as_slice);
        for (neighbor, rate) in neighbors {
            if visited.contains(neighbor) {
                continue;
            }

            if let Some(next_rate) = current_rate
                .checked_mul(*rate)
                .filter(|rate| !rate.is_zero())
            {
                visited.insert(*neighbor);
                stack.push((*neighbor, next_rate));
            } else {
                path_unrepresentable = true;
            }
        }
    }

    (None, path_unrepresentable)
}

fn is_connected(graph: &RateGraph, from_currency: Ustr, to_currency: Ustr) -> bool {
    let mut stack = vec![from_currency];
    let mut visited = AHashSet::from_iter([from_currency]);

    while let Some(current) = stack.pop() {
        if current == to_currency {
            return true;
        }

        if let Some(neighbors) = graph.get(&current) {
            for (neighbor, _) in neighbors {
                if visited.insert(*neighbor) {
                    stack.push(*neighbor);
                }
            }
        }
    }

    false
}

fn find_rate_with_backtracking(
    graph: &RateGraph,
    from_currency: Ustr,
    to_currency: Ustr,
) -> Option<Decimal> {
    let mut stack = vec![(from_currency, Decimal::ONE, 0)];
    let mut visited: AHashSet<Ustr> = AHashSet::new();
    visited.insert(from_currency);

    while let Some((current, current_rate, next_index)) = stack.last_mut() {
        if *current == to_currency {
            return Some(*current_rate);
        }

        let neighbors = graph.get(current).map_or(&[][..], Vec::as_slice);
        if *next_index == neighbors.len() {
            visited.remove(current);
            stack.pop();
            continue;
        }

        let (neighbor, rate) = neighbors[neighbors.len() - 1 - *next_index];
        *next_index += 1;

        if visited.contains(&neighbor) {
            continue;
        }

        if let Some(next_rate) = current_rate
            .checked_mul(rate)
            .filter(|rate| !rate.is_zero())
        {
            visited.insert(neighbor);
            stack.push((neighbor, next_rate, 0));
        }
    }

    None
}

fn directional_rates(
    bid: Decimal,
    ask: Decimal,
    price_type: PriceType,
) -> (Option<Decimal>, Option<Decimal>) {
    let (forward_rate, reverse_divisor) = match price_type {
        PriceType::Bid => (Some(bid), Some(ask)),
        PriceType::Ask => (Some(ask), Some(bid)),
        PriceType::Mid => {
            let mid = bid
                .checked_add(ask)
                .and_then(|sum| sum.checked_div(Decimal::TWO))
                .or_else(|| {
                    let (low, high) = if bid < ask { (bid, ask) } else { (ask, bid) };
                    high.checked_sub(low)
                        .and_then(|difference| difference.checked_div(Decimal::TWO))
                        .and_then(|half| low.checked_add(half))
                });

            (mid, mid)
        }
        _ => unreachable!("Price type was validated before graph construction"),
    };

    let reverse_rate = reverse_divisor
        .and_then(|divisor| Decimal::ONE.checked_div(divisor))
        .filter(|rate| !rate.is_zero());

    (forward_rate.filter(|rate| !rate.is_zero()), reverse_rate)
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
    #[case("EURUSD")]
    #[case("EUR/USD/JPY")]
    #[case("EUR/USD/")]
    #[case("EUR//USD")]
    #[case("/EUR/USD")]
    fn test_invalid_pair_string(#[case] pair: &str) {
        let mut quotes_bid = AHashMap::new();
        let mut quotes_ask = AHashMap::new();
        quotes_bid.insert(Ustr::from(pair), dec!(2));
        quotes_ask.insert(Ustr::from(pair), dec!(2));
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
    #[case("/USD", "", "USD")]
    #[case("EUR/", "EUR", "")]
    fn test_pair_with_empty_currency_field(
        #[case] pair: &str,
        #[case] from_currency: &str,
        #[case] to_currency: &str,
    ) {
        let quotes_bid = AHashMap::from([(Ustr::from(pair), dec!(2))]);
        let quotes_ask = AHashMap::from([(Ustr::from(pair), dec!(4))]);

        let rate = get_exchange_rate(
            Ustr::from(from_currency),
            Ustr::from(to_currency),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        assert_eq!(rate, Some(dec!(3)));
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
    fn test_indirect_route_is_deterministic_across_hash_seeds(
        #[values(false, true)] reverse: bool,
    ) {
        let mut pairs = [
            ("AAA/BBB", dec!(2)),
            ("BBB/DDD", dec!(3)),
            ("AAA/CCC", dec!(5)),
            ("CCC/DDD", dec!(7)),
        ];

        if reverse {
            pairs.reverse();
        }

        let seeds = [(0, 0, 0, 0), (1, 2, 3, 4), (5, 6, 7, 8), (10, 20, 30, 40)];

        let rates = seeds.map(|(k0, k1, k2, k3)| {
            let random_state = RandomState::with_seeds(k0, k1, k2, k3);
            let mut quotes_bid = AHashMap::with_hasher(random_state.clone());
            let mut quotes_ask = AHashMap::with_hasher(random_state);

            for (pair, rate) in pairs {
                quotes_bid.insert(Ustr::from(pair), rate);
                quotes_ask.insert(Ustr::from(pair), rate);
            }

            get_exchange_rate(
                Ustr::from("AAA"),
                Ustr::from("DDD"),
                PriceType::Bid,
                quotes_bid,
                quotes_ask,
            )
            .unwrap()
        });

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
    fn test_midpoint_sum_overflow_uses_representable_rate() {
        let quotes_bid = AHashMap::from([(Ustr::from("EUR/USD"), Decimal::MAX)]);
        let quotes_ask = quotes_bid.clone();

        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(result.unwrap(), Some(Decimal::MAX));
    }

    #[rstest]
    #[case(PriceType::Bid)]
    #[case(PriceType::Ask)]
    #[case(PriceType::Mid)]
    fn test_path_overflow_returns_error(#[case] price_type: PriceType) {
        let quotes_bid = AHashMap::from([
            (Ustr::from("EUR/USD"), dec!(100000000000000000000)),
            (Ustr::from("USD/JPY"), dec!(100000000000000000000)),
        ]);
        let quotes_ask = quotes_bid.clone();

        let result = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("JPY"),
            price_type,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "No representable exchange rate from EUR to JPY"
        );
    }

    #[rstest]
    fn test_alternate_quotes_unrepresentable_route_returns_error() {
        let quotes_bid = AHashMap::from([
            (Ustr::from("EUR/USD"), dec!(100000000000000000000)),
            (Ustr::from("USD/JPY"), dec!(100000000000000000000)),
            (Ustr::from("BTC/USDT"), Decimal::ONE),
        ]);
        let quotes_ask = quotes_bid.clone();
        let alternates = vec![(Ustr::from("BTC/USDT"), Decimal::ONE, Decimal::ONE); 30];

        let result = get_exchange_rate_with_alternates(
            Ustr::from("EUR"),
            Ustr::from("JPY"),
            PriceType::Bid,
            &quotes_bid,
            &quotes_ask,
            &alternates,
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "No representable exchange rate from EUR to JPY"
        );
    }

    #[rstest]
    #[case(PriceType::Bid)]
    #[case(PriceType::Ask)]
    #[case(PriceType::Mid)]
    fn test_path_overflow_uses_alternate_route(#[case] price_type: PriceType) {
        let quotes_bid = AHashMap::from([
            (Ustr::from("AAA/BBB"), dec!(1000000000)),
            (Ustr::from("BBB/CCC"), dec!(1000000000)),
            (Ustr::from("CCC/DDD"), dec!(1000000000)),
            (Ustr::from("DDD/TTT"), dec!(1000000000)),
            (Ustr::from("AAA/EEE"), dec!(2)),
            (Ustr::from("EEE/TTT"), dec!(3)),
        ]);
        let quotes_ask = quotes_bid.clone();

        let result = get_exchange_rate(
            Ustr::from("AAA"),
            Ustr::from("TTT"),
            price_type,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(result.unwrap(), Some(dec!(6)));
    }

    #[rstest]
    fn test_path_overflow_revisits_currency_at_representable_rate() {
        let quotes_bid = AHashMap::from([
            (Ustr::from("AAA/BBB"), dec!(100000000000000000000)),
            (Ustr::from("AAA/CCC"), Decimal::ONE),
            (Ustr::from("CCC/BBB"), dec!(0.00000000000000000001)),
            (Ustr::from("BBB/TTT"), dec!(100000000000000000000)),
        ]);
        let quotes_ask = quotes_bid.clone();

        let result = get_exchange_rate(
            Ustr::from("AAA"),
            Ustr::from("TTT"),
            PriceType::Bid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(result.unwrap(), Some(Decimal::ONE));
    }

    #[rstest]
    fn test_path_underflow_uses_alternate_route() {
        let quotes_bid = AHashMap::from([
            (Ustr::from("AAA/BBB"), dec!(0.00000000000000000001)),
            (Ustr::from("BBB/TTT"), dec!(0.00000000000000000001)),
            (Ustr::from("AAA/CCC"), dec!(2)),
            (Ustr::from("CCC/TTT"), dec!(3)),
        ]);
        let quotes_ask = quotes_bid.clone();

        let result = get_exchange_rate(
            Ustr::from("AAA"),
            Ustr::from("TTT"),
            PriceType::Bid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(result.unwrap(), Some(dec!(6)));
    }

    #[rstest]
    fn test_inverse_underflow_uses_alternate_route() {
        let quotes_bid = AHashMap::from([
            (Ustr::from("AAA/BBB"), Decimal::MAX),
            (Ustr::from("BBB/CCC"), Decimal::ONE),
            (Ustr::from("CCC/AAA"), Decimal::ONE),
        ]);
        let quotes_ask = quotes_bid.clone();

        let result = get_exchange_rate(
            Ustr::from("BBB"),
            Ustr::from("AAA"),
            PriceType::Bid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(result.unwrap(), Some(Decimal::ONE));
    }

    #[rstest]
    fn test_unrelated_inverse_underflow_does_not_mask_valid_route() {
        let quotes_bid = AHashMap::from([
            (Ustr::from("AAA/TTT"), dec!(6)),
            (Ustr::from("XXX/YYY"), Decimal::MAX),
        ]);
        let quotes_ask = quotes_bid.clone();

        let result = get_exchange_rate(
            Ustr::from("AAA"),
            Ustr::from("TTT"),
            PriceType::Bid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(result.unwrap(), Some(dec!(6)));
    }

    #[rstest]
    #[case(PriceType::Bid)]
    #[case(PriceType::Ask)]
    fn test_direct_decimal_max_rate(#[case] price_type: PriceType) {
        let quotes_bid = AHashMap::from([(Ustr::from("EUR/USD"), Decimal::MAX)]);
        let quotes_ask = quotes_bid.clone();

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            price_type,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        assert_eq!(rate, Some(Decimal::MAX));
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
        assert_eq!(
            result.unwrap_err().to_string(),
            "Quote maps must not be empty"
        );
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
        assert_eq!(
            result.unwrap_err().to_string(),
            "Quote maps must have equal lengths"
        );
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

        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing ask quote for pair EUR/USD"
        );
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
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid `price_type`, was 'LAST'"
        );
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

        assert_eq!(rate, Some(dec!(1.1001)));
    }
}
