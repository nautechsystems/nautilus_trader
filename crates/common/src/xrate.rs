// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::{HashMap, HashSet};

use nautilus_model::enums::PriceType;
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
/// This function returns an error if:
/// - `price_type` is equal to `Last` or `Mark` (cannot calculate from quotes).
/// - `quotes_bid` or `quotes_ask` is empty.
/// - `quotes_bid` and `quotes_ask` lengths are not equal.
/// - The bid or ask side of a pair is missing.
pub fn get_exchange_rate(
    from_currency: Ustr,
    to_currency: Ustr,
    price_type: PriceType,
    quotes_bid: HashMap<String, f64>,
    quotes_ask: HashMap<String, f64>,
) -> anyhow::Result<Option<f64>> {
    if from_currency == to_currency {
        // When the source and target currencies are identical,
        // no conversion is needed; return an exchange rate of 1.0.
        return Ok(Some(1.0));
    }

    if quotes_bid.is_empty() || quotes_ask.is_empty() {
        anyhow::bail!("Quote maps must not be empty");
    }
    if quotes_bid.len() != quotes_ask.len() {
        anyhow::bail!("Quote maps must have equal lengths");
    }

    // Build effective quotes based on the requested price type
    let effective_quotes: HashMap<String, f64> = match price_type {
        PriceType::Bid => quotes_bid,
        PriceType::Ask => quotes_ask,
        PriceType::Mid => {
            let mut mid_quotes = HashMap::new();
            for (pair, bid) in &quotes_bid {
                let ask = quotes_ask
                    .get(pair)
                    .ok_or_else(|| anyhow::anyhow!("Missing ask quote for pair {pair}"))?;
                mid_quotes.insert(pair.clone(), (bid + ask) / 2.0);
            }
            mid_quotes
        }
        _ => anyhow::bail!("Invalid `price_type`, was '{price_type}'"),
    };

    // Construct a graph: each currency maps to its neighbors and corresponding conversion rate
    let mut graph: HashMap<Ustr, Vec<(Ustr, f64)>> = HashMap::new();
    for (pair, rate) in effective_quotes {
        let parts: Vec<&str> = pair.split('/').collect();
        if parts.len() != 2 {
            log::warn!("Skipping invalid pair string: {pair}");
            continue;
        }
        let base = Ustr::from(parts[0]);
        let quote = Ustr::from(parts[1]);

        graph.entry(base).or_default().push((quote, rate));
        graph.entry(quote).or_default().push((base, 1.0 / rate));
    }

    // DFS: search for a conversion path from `from_currency` to `to_currency`
    let mut stack: Vec<(Ustr, f64)> = vec![(from_currency, 1.0)];
    let mut visited: HashSet<Ustr> = HashSet::new();
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    fn setup_test_quotes() -> (HashMap<String, f64>, HashMap<String, f64>) {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();

        // Direct pairs
        quotes_bid.insert("EUR/USD".to_string(), 1.1000);
        quotes_ask.insert("EUR/USD".to_string(), 1.1002);

        quotes_bid.insert("GBP/USD".to_string(), 1.3000);
        quotes_ask.insert("GBP/USD".to_string(), 1.3002);

        quotes_bid.insert("USD/JPY".to_string(), 110.00);
        quotes_ask.insert("USD/JPY".to_string(), 110.02);

        quotes_bid.insert("AUD/USD".to_string(), 0.7500);
        quotes_ask.insert("AUD/USD".to_string(), 0.7502);

        (quotes_bid, quotes_ask)
    }

    #[rstest]
    fn test_invalid_pair_string() {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();
        // Invalid pair string (missing '/')
        quotes_bid.insert("EURUSD".to_string(), 1.1000);
        quotes_ask.insert("EURUSD".to_string(), 1.1002);
        // Valid pair string
        quotes_bid.insert("EUR/USD".to_string(), 1.1000);
        quotes_ask.insert("EUR/USD".to_string(), 1.1002);

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        let expected = f64::midpoint(1.1000, 1.1002);
        assert!((rate.unwrap() - expected).abs() < 0.0001);
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
        assert_eq!(rate, Some(1.0));
    }

    #[rstest(
        price_type,
        expected,
        case(PriceType::Bid, 1.1000),
        case(PriceType::Ask, 1.1002),
        case(PriceType::Mid, f64::midpoint(1.1000, 1.1002))
    )]
    fn test_direct_pair(price_type: PriceType, expected: f64) {
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
        assert!((rate - expected).abs() < 0.0001);
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
            assert!(eur_usd.mul_add(usd_eur, -1.0).abs() < 0.0001);
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
        let mid_eur_usd = f64::midpoint(1.1000, 1.1002);
        let mid_usd_jpy = f64::midpoint(110.00, 110.02);
        let expected = mid_eur_usd * mid_usd_jpy;
        if let Some(val) = rate {
            assert!((val - expected).abs() < 0.1);
        } else {
            panic!("Expected conversion rate through USD but got None");
        }
    }

    #[rstest]
    fn test_no_conversion_path() {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();

        // Only one pair provided
        quotes_bid.insert("EUR/USD".to_string(), 1.1000);
        quotes_ask.insert("EUR/USD".to_string(), 1.1002);

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
        let quotes_bid: HashMap<String, f64> = HashMap::new();
        let quotes_ask: HashMap<String, f64> = HashMap::new();
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
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();

        quotes_bid.insert("EUR/USD".to_string(), 1.1000);
        quotes_bid.insert("GBP/USD".to_string(), 1.3000);
        quotes_ask.insert("EUR/USD".to_string(), 1.1002);
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
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();
        // Create a cycle by including both EUR/USD and USD/EUR quotes
        quotes_bid.insert("EUR/USD".to_string(), 1.1);
        quotes_ask.insert("EUR/USD".to_string(), 1.1002);
        quotes_bid.insert("USD/EUR".to_string(), 0.909);
        quotes_ask.insert("USD/EUR".to_string(), 0.9091);

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        // Expect the direct EUR/USD mid rate
        let expected = f64::midpoint(1.1, 1.1002);
        assert!((rate.unwrap() - expected).abs() < 0.0001);
    }

    #[rstest]
    fn test_multiple_paths() {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();
        // Direct conversion
        quotes_bid.insert("EUR/USD".to_string(), 1.1000);
        quotes_ask.insert("EUR/USD".to_string(), 1.1002);
        // Indirect path via GBP: EUR/GBP and GBP/USD
        quotes_bid.insert("EUR/GBP".to_string(), 0.8461);
        quotes_ask.insert("EUR/GBP".to_string(), 0.8463);
        quotes_bid.insert("GBP/USD".to_string(), 1.3000);
        quotes_ask.insert("GBP/USD".to_string(), 1.3002);

        let rate = get_exchange_rate(
            Ustr::from("EUR"),
            Ustr::from("USD"),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        )
        .unwrap();

        // Both paths should be consistent:
        let direct: f64 = f64::midpoint(1.1000_f64, 1.1002_f64);
        let indirect: f64 =
            f64::midpoint(0.8461_f64, 0.8463_f64) * f64::midpoint(1.3000_f64, 1.3002_f64);
        assert!((direct - indirect).abs() < 0.0001_f64);
        assert!((rate.unwrap() - direct).abs() < 0.0001_f64);
    }
}
