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

// ****************************************************************************
// The design of exchange rate calculations needs to be revisited,
// as its not efficient to be allocating so many structures and doing so many recalculations"
// ****************************************************************************

//! Exchange rate calculations between currencies.
//!
//! An exchange rate is the value of one asset versus that of another.
use std::collections::{HashMap, HashSet};

use itertools::Itertools;
use nautilus_core::correctness::{check_equal_usize, check_map_not_empty, FAILED};
use nautilus_model::{enums::PriceType, identifiers::Symbol, types::Currency};
use rust_decimal::Decimal;
use ustr::Ustr;

// TODO: Improve efficiency: Check Top Comment
/// Returns the calculated exchange rate for the given price type using the
/// given dictionary of bid and ask quotes.
#[must_use]
pub fn get_exchange_rate(
    from_currency: Currency,
    to_currency: Currency,
    price_type: PriceType,
    quotes_bid: HashMap<Symbol, Decimal>,
    quotes_ask: HashMap<Symbol, Decimal>,
) -> Decimal {
    check_map_not_empty(&quotes_bid, stringify!(quotes_bid)).expect(FAILED);
    check_map_not_empty(&quotes_ask, stringify!(quotes_ask)).expect(FAILED);
    check_equal_usize(
        quotes_bid.len(),
        quotes_ask.len(),
        "quotes_bid.len()",
        "quotes_ask.len()",
    )
    .expect(FAILED);

    if from_currency == to_currency {
        return Decimal::ONE;
    }

    let calculation_quotes = match price_type {
        PriceType::Bid => quotes_bid,
        PriceType::Ask => quotes_ask,
        PriceType::Mid => quotes_bid
            .iter()
            .map(|(k, v)| {
                let ask = quotes_ask.get(k).unwrap_or(v);
                (*k, (v + ask) / Decimal::TWO)
            })
            .collect(),
        _ => {
            panic!("Cannot calculate exchange rate for PriceType: {price_type:?}");
        }
    };

    let mut codes = HashSet::new();
    let mut exchange_rates: HashMap<Ustr, HashMap<Ustr, Decimal>> = HashMap::new();

    // Build quote table
    for (symbol, quote) in &calculation_quotes {
        // Split symbol into currency pairs
        let pieces: Vec<&str> = symbol.as_str().split('/').collect();
        let code_lhs = Ustr::from(pieces[0]);
        let code_rhs = Ustr::from(pieces[1]);

        codes.insert(code_lhs);
        codes.insert(code_rhs);

        // Initialize currency dictionaries if they don't exist
        exchange_rates.entry(code_lhs).or_default();
        exchange_rates.entry(code_rhs).or_default();

        // Add base rates
        if let Some(rates_lhs) = exchange_rates.get_mut(&code_lhs) {
            rates_lhs.insert(code_lhs, Decimal::ONE);
            rates_lhs.insert(code_rhs, *quote);
        }
        if let Some(rates_rhs) = exchange_rates.get_mut(&code_rhs) {
            rates_rhs.insert(code_rhs, Decimal::ONE);
        }
    }

    // Generate possible currency pairs from all symbols
    let code_perms: Vec<(Ustr, Ustr)> = codes
        .iter()
        .cartesian_product(codes.iter())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| (*a, *b))
        .collect();

    // Calculate currency inverses
    for (perm0, perm1) in &code_perms {
        // First direction: perm0 -> perm1
        let rate_0_to_1 = exchange_rates
            .get(perm0)
            .and_then(|rates| rates.get(perm1))
            .copied();

        if let Some(rate) = rate_0_to_1 {
            if let Some(xrate_perm1) = exchange_rates.get_mut(perm1) {
                if !xrate_perm1.contains_key(perm0) {
                    xrate_perm1.insert(*perm0, Decimal::ONE / rate);
                }
            }
        }

        // Second direction: perm1 -> perm0
        let rate_1_to_0 = exchange_rates
            .get(perm1)
            .and_then(|rates| rates.get(perm0))
            .copied();

        if let Some(rate) = rate_1_to_0 {
            if let Some(xrate_perm0) = exchange_rates.get_mut(perm0) {
                if !xrate_perm0.contains_key(perm1) {
                    xrate_perm0.insert(*perm1, Decimal::ONE / rate);
                }
            }
        }
    }

    // Check if we already have the rate
    if let Some(quotes) = exchange_rates.get(&from_currency.code) {
        if let Some(&rate) = quotes.get(&to_currency.code) {
            return rate;
        }
    }

    // Calculate remaining exchange rates through common currencies
    for (perm0, perm1) in &code_perms {
        // Skip if rate already exists
        if exchange_rates
            .get(perm1)
            .is_some_and(|rates| rates.contains_key(perm0))
        {
            continue;
        }

        // Search for common currency
        for code in &codes {
            // First check: rates through common currency
            let rates_through_common = {
                let rates_perm0 = exchange_rates.get(perm0);
                let rates_perm1 = exchange_rates.get(perm1);

                match (rates_perm0, rates_perm1) {
                    (Some(rates0), Some(rates1)) => {
                        if let (Some(&rate1), Some(&rate2)) = (rates0.get(code), rates1.get(code)) {
                            Some((rate1, rate2))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };

            // Second check: rates from code's perspective
            let rates_from_code = if rates_through_common.is_none() {
                if let Some(rates_code) = exchange_rates.get(code) {
                    if let (Some(&rate1), Some(&rate2)) =
                        (rates_code.get(perm0), rates_code.get(perm1))
                    {
                        Some((rate1, rate2))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Apply the found rates if any
            if let Some((common_rate1, common_rate2)) = rates_through_common.or(rates_from_code) {
                // Insert forward rate
                if let Some(rates_perm1) = exchange_rates.get_mut(perm1) {
                    rates_perm1.insert(*perm0, common_rate2 / common_rate1);
                }

                // Insert inverse rate
                if let Some(rates_perm0) = exchange_rates.get_mut(perm0) {
                    if !rates_perm0.contains_key(perm1) {
                        rates_perm0.insert(*perm1, common_rate1 / common_rate2);
                    }
                }
            }
        }
    }

    let xrate = exchange_rates
        .get(&from_currency.code)
        .and_then(|quotes| quotes.get(&to_currency.code))
        .copied()
        .unwrap_or(Decimal::ZERO);

    xrate
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal_macros::dec;

    use super::*;

    // Helper function to create test quotes
    fn setup_test_quotes() -> (HashMap<Symbol, Decimal>, HashMap<Symbol, Decimal>) {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();

        // Direct pairs
        quotes_bid.insert(Symbol::from_str_unchecked("EUR/USD"), dec!(1.1000));
        quotes_ask.insert(Symbol::from_str_unchecked("EUR/USD"), dec!(1.1002));

        quotes_bid.insert(Symbol::from_str_unchecked("GBP/USD"), dec!(1.3000));
        quotes_ask.insert(Symbol::from_str_unchecked("GBP/USD"), dec!(1.3002));

        quotes_bid.insert(Symbol::from_str_unchecked("USD/JPY"), dec!(110.00));
        quotes_ask.insert(Symbol::from_str_unchecked("USD/JPY"), dec!(110.02));

        quotes_bid.insert(Symbol::from_str_unchecked("AUD/USD"), dec!(0.7500));
        quotes_ask.insert(Symbol::from_str_unchecked("AUD/USD"), dec!(0.7502));

        (quotes_bid, quotes_ask)
    }

    #[test]
    /// Test same currency conversion
    fn test_same_currency() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();
        let rate = get_exchange_rate(
            Currency::from_str("USD").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );
        assert_eq!(rate, Decimal::ONE);
    }

    #[test]
    /// Test direct pair conversion
    fn test_direct_pair() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        // Test bid price
        let rate_bid = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Bid,
            quotes_bid.clone(),
            quotes_ask.clone(),
        );
        assert_eq!(rate_bid, dec!(1.1000));

        // Test ask price
        let rate_ask = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Ask,
            quotes_bid.clone(),
            quotes_ask.clone(),
        );
        assert_eq!(rate_ask, dec!(1.1002));

        // Test mid price
        let rate_mid = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );
        assert_eq!(rate_mid, dec!(1.1001));
    }

    #[test]
    /// Test inverse pair calculation
    fn test_inverse_pair() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let rate = get_exchange_rate(
            Currency::from_str("USD").unwrap(),
            Currency::from_str("EUR").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        // USD/EUR should be approximately 1/1.1001
        let expected = Decimal::ONE / dec!(1.1001);
        assert!((rate - expected).abs() < dec!(0.0001));
    }

    #[test]
    /// Test cross pair calculation through USD
    fn test_cross_pair_through_usd() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let rate = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("JPY").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        // EUR/JPY should be approximately EUR/USD * USD/JPY
        let expected = dec!(1.1001) * dec!(110.01);
        assert!((rate - expected).abs() < dec!(0.01));
    }

    #[test]
    /// Test cross pair calculation through multiple paths
    fn test_multiple_path_cross_pair() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let rate = get_exchange_rate(
            Currency::from_str("GBP").unwrap(),
            Currency::from_str("AUD").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        // GBP/AUD should be calculated through USD
        // GBP/USD * (1/AUD/USD)
        let expected = dec!(1.3001) / dec!(0.7501);
        assert!((rate - expected).abs() < dec!(0.01));
    }

    #[test]
    /// Test handling of missing pairs
    fn test_missing_pairs() {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();

        // Only adding one pair
        quotes_bid.insert(Symbol::from_str_unchecked("EUR/USD"), dec!(1.1000));
        quotes_ask.insert(Symbol::from_str_unchecked("EUR/USD"), dec!(1.1002));

        let rate = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("JPY").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(rate, Decimal::ZERO); // Should return 0 for impossible conversions
    }

    #[test]
    #[should_panic]
    fn test_empty_quotes() {
        let quotes_bid = HashMap::new();
        let quotes_ask = HashMap::new();

        let out_xrate = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(out_xrate, Decimal::ZERO);
    }

    #[test]
    #[should_panic]
    fn test_unequal_quotes_length() {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();

        quotes_bid.insert(Symbol::from_str_unchecked("EUR/USD"), dec!(1.1000));
        quotes_bid.insert(Symbol::from_str_unchecked("GBP/USD"), dec!(1.3000));
        quotes_ask.insert(Symbol::from_str_unchecked("EUR/USD"), dec!(1.1002));

        let out_xrate = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(out_xrate, Decimal::ZERO);
    }

    #[test]
    #[should_panic]
    /// Test invalid price type handling
    fn test_invalid_price_type() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let out_xrate = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Last, // Invalid price type
            quotes_bid,
            quotes_ask,
        );

        assert_eq!(out_xrate, Decimal::ZERO);
    }

    #[test]
    /// Test extensive cross pairs
    fn test_extensive_cross_pairs() {
        let mut quotes_bid = HashMap::new();
        let mut quotes_ask = HashMap::new();

        // Create a complex network of currency pairs
        let pairs = vec![
            ("EUR/USD", (1.1000, 1.1002)),
            ("GBP/USD", (1.3000, 1.3002)),
            ("USD/JPY", (110.00, 110.02)),
            ("EUR/GBP", (0.8461, 0.8463)),
            ("AUD/USD", (0.7500, 0.7502)),
            ("NZD/USD", (0.7000, 0.7002)),
            ("USD/CAD", (1.2500, 1.2502)),
        ];

        for (pair, (bid, ask)) in pairs {
            quotes_bid.insert(
                Symbol::from_str_unchecked(pair),
                Decimal::from_f64(bid).unwrap(),
            );
            quotes_ask.insert(
                Symbol::from_str_unchecked(pair),
                Decimal::from_f64(ask).unwrap(),
            );
        }

        // Test various cross pairs
        let test_pairs = vec![
            ("EUR", "JPY", 121.022), // EUR/USD * USD/JPY
            ("GBP", "JPY", 143.024), // GBP/USD * USD/JPY
            ("AUD", "JPY", 82.51),   // AUD/USD * USD/JPY
            ("EUR", "CAD", 1.375),   // EUR/USD * USD/CAD
            ("NZD", "CAD", 0.875),   // NZD/USD * USD/CAD
            ("AUD", "NZD", 1.071),   // AUD/USD / NZD/USD
        ];

        for (from, to, expected) in test_pairs {
            let rate = get_exchange_rate(
                Currency::from_str(from).unwrap(),
                Currency::from_str(to).unwrap(),
                PriceType::Mid,
                quotes_bid.clone(),
                quotes_ask.clone(),
            );

            let expected_dec = Decimal::from_f64(expected).unwrap();
            assert!(
                (rate - expected_dec).abs() < dec!(0.01),
                "Failed for pair {from}/{to}: got {rate}, expected {expected_dec}"
            );
        }
    }

    #[test]
    /// Test rate consistency
    fn test_rate_consistency() {
        let (quotes_bid, quotes_ask) = setup_test_quotes();

        let rate_eur_usd = get_exchange_rate(
            Currency::from_str("EUR").unwrap(),
            Currency::from_str("USD").unwrap(),
            PriceType::Mid,
            quotes_bid.clone(),
            quotes_ask.clone(),
        );

        let rate_usd_eur = get_exchange_rate(
            Currency::from_str("USD").unwrap(),
            Currency::from_str("EUR").unwrap(),
            PriceType::Mid,
            quotes_bid,
            quotes_ask,
        );

        // Check if one rate is the inverse of the other
        assert!((rate_eur_usd * rate_usd_eur - Decimal::ONE).abs() < dec!(0.0001));
    }
}
