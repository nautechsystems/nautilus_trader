// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_model::{enums::PriceType, identifiers::Symbol, types::currency::Currency};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

const DECIMAL_ONE: Decimal = dec!(1.0);
const DECIMAL_TWO: Decimal = dec!(2.0);

/// Returns the calculated exchange rate for the given price type using the
/// given dictionary of bid and ask quotes.
pub fn get_exchange_rate(
    from_currency: Currency,
    to_currency: Currency,
    price_type: PriceType,
    quotes_bid: HashMap<Symbol, Decimal>,
    quotes_ask: HashMap<Symbol, Decimal>,
) -> anyhow::Result<Decimal> {
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
        return Ok(DECIMAL_ONE); // No conversion necessary
    }

    let calculation_quotes: HashMap<Symbol, Decimal> = match price_type {
        PriceType::Bid => quotes_bid,
        PriceType::Ask => quotes_ask,
        PriceType::Mid => {
            let mut calculation_quotes = HashMap::new();
            for (symbol, bid_quote) in &quotes_bid {
                if let Some(ask_quote) = quotes_ask.get(symbol) {
                    calculation_quotes.insert(*symbol, (bid_quote + ask_quote) / DECIMAL_TWO);
                }
            }
            calculation_quotes
        }
        _ => panic!("Cannot calculate exchange rate for PriceType {price_type:?}"),
    };

    let mut exchange_rates: HashMap<Ustr, HashMap<Ustr, Decimal>> = HashMap::new();

    // Build quote table
    for (symbol, quote) in &calculation_quotes {
        let pieces: Vec<&str> = symbol.as_str().split('/').collect();
        let code_lhs = Ustr::from(pieces[0]);
        let code_rhs = Ustr::from(pieces[1]);

        let lhs_rates = exchange_rates.entry(code_lhs).or_default();
        lhs_rates.insert(code_lhs, Decimal::new(1, 0));
        lhs_rates.insert(code_rhs, *quote);

        let rhs_rates = exchange_rates.entry(code_rhs).or_default();
        rhs_rates.insert(code_lhs, Decimal::new(1, 0));
        rhs_rates.insert(code_rhs, *quote);
    }

    // Clone exchange_rates to avoid borrowing conflicts
    let exchange_rates_cloned = exchange_rates.clone();

    // Generate possible currency pairs from all symbols
    let mut codes: HashSet<&Ustr> = HashSet::new();
    for (code_lhs, code_rhs) in exchange_rates_cloned.keys().flat_map(|k| {
        exchange_rates_cloned
            .keys()
            .map(move |code_rhs| (k, code_rhs))
    }) {
        codes.insert(code_lhs);
        codes.insert(code_rhs);
    }
    let _code_perms: Vec<(&Ustr, &Ustr)> = codes
        .iter()
        .cartesian_product(codes.iter())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| (*a, *b))
        .collect();

    // TODO: Unable to solve borrowing issues for now (see top comment)
    // Calculate currency inverses
    // for (perm_0, perm_1) in code_perms.iter() {
    //     let exchange_rates_perm_0 = exchange_rates.entry(**perm_0).or_insert_with(HashMap::new);
    //     let exchange_rates_perm_1 = exchange_rates.entry(**perm_1).or_insert_with(HashMap::new);
    //     if !exchange_rates_perm_0.contains_key(perm_1) {
    //         if let Some(rate) = exchange_rates_perm_0.get(perm_1) {
    //             exchange_rates_perm_1
    //                 .entry(**perm_0)
    //                 .or_insert_with(|| Decimal::new(1, 0) / rate);
    //         }
    //     }
    //     if !exchange_rates_perm_1.contains_key(perm_0) {
    //         if let Some(rate) = exchange_rates_perm_1.get(perm_0) {
    //             exchange_rates_perm_0
    //                 .entry(**perm_1)
    //                 .or_insert_with(|| Decimal::new(1, 0) / rate);
    //         }
    //     }
    // }

    if let Some(quotes) = exchange_rates.get(&from_currency.code) {
        if let Some(xrate) = quotes.get(&to_currency.code) {
            return Ok(*xrate);
        }
    }

    // TODO: Improve efficiency
    let empty: HashMap<Ustr, Decimal> = HashMap::new();
    let quotes = exchange_rates.get(&from_currency.code).unwrap_or(&empty);

    Ok(quotes.get(&to_currency.code).copied().unwrap_or(dec!(0.0)))
}
