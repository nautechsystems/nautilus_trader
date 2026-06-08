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

//! CRC32 checksum validation for Kraken `level3` book updates.

use ahash::AHashMap;
use nautilus_model::enums::OrderSide;

use super::parse::CachedL3Order;

/// Formats a decimal string per Kraken L3 checksum rules.
///
/// Removes the decimal point then strips leading zeros, so `"0.12730000"` → `"12730000"`
/// and `"79754.0"` → `"797540"`.
fn format_raw(raw: &str) -> String {
    let no_dot = raw.replace('.', "");
    let trimmed = no_dot.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Builds the checksum input string from the open-order cache.
///
/// Iterates individual orders at the top-10 ask price levels (ascending) then the
/// top-10 bid price levels (descending). Within each price level orders are sorted
/// by insertion sequence (FIFO queue priority). Each order contributes
/// `format_raw(price_raw) + format_raw(size_raw)` to the string.
pub(crate) fn build_checksum_string(open_orders: &AHashMap<u64, CachedL3Order>) -> String {
    let mut asks: Vec<(f64, u64, &str, &str)> = open_orders
        .iter()
        .filter(|(_, v)| v.side == OrderSide::Sell)
        .map(|(_, v)| (v.price, v.seq, v.price_raw.as_str(), v.size_raw.as_str()))
        .collect();

    let mut bids: Vec<(f64, u64, &str, &str)> = open_orders
        .iter()
        .filter(|(_, v)| v.side == OrderSide::Buy)
        .map(|(_, v)| (v.price, v.seq, v.price_raw.as_str(), v.size_raw.as_str()))
        .collect();

    asks.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    bids.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut s = String::new();
    append_top_10_levels(&asks, &mut s);
    append_top_10_levels(&bids, &mut s);

    s
}

/// Computes the Kraken `level3` CRC32 checksum from the open-order cache.
pub(crate) fn compute_checksum(open_orders: &AHashMap<u64, CachedL3Order>) -> u32 {
    let s = build_checksum_string(open_orders);
    crc32_ieee(s.as_bytes())
}

/// Appends the top-10 price levels (all orders per level, FIFO) to `s`.
fn append_top_10_levels(sorted: &[(f64, u64, &str, &str)], s: &mut String) {
    let mut level_count = 0u32;
    let mut last_price_bits: Option<u64> = None;

    for &(price, _, price_raw, size_raw) in sorted {
        let price_bits = price.to_bits();
        if Some(price_bits) != last_price_bits {
            if level_count == 10 {
                break;
            }
            level_count += 1;
            last_price_bits = Some(price_bits);
        }
        s.push_str(&format_raw(price_raw));
        s.push_str(&format_raw(size_raw));
    }
}

/// IEEE CRC32 polynomial (reflected), inline to avoid adding a new dependency.
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::websocket::spot_v2::level_3::parse::CachedL3Order;

    #[rstest]
    fn test_crc32_ieee_known_value() {
        // CRC32 of b"123456789" == 0xCBF43926 (standard check value)
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }

    #[rstest]
    fn test_format_raw_integer_price() {
        assert_eq!(format_raw("42000"), "42000");
    }

    #[rstest]
    fn test_format_raw_price_with_trailing_zero() {
        // "79754.0" → remove dot → "797540" → no leading zeros → "797540"
        assert_eq!(format_raw("79754.0"), "797540");
    }

    #[rstest]
    fn test_format_raw_qty_with_trailing_zeros() {
        // "0.12730000" → remove dot → "012730000" → strip → "12730000"
        assert_eq!(format_raw("0.12730000"), "12730000");
    }

    #[rstest]
    fn test_format_raw_leading_zeros() {
        // "0.00125386" → "000125386" → "125386"
        assert_eq!(format_raw("0.00125386"), "125386");
    }

    #[rstest]
    fn test_format_raw_kraken_docs_examples() {
        assert_eq!(format_raw("44939.5"), "449395");
        assert_eq!(format_raw("4.52308393"), "452308393");
        assert_eq!(format_raw("0.88968699"), "88968699");
    }

    #[rstest]
    fn test_compute_checksum_kraken_docs_example() {
        // Data extracted from the Kraken L3 checksum documentation example.
        // Expected checksum: 1063832831
        // Asks: top-10 levels ascending, bids: top-10 levels descending; FIFO within each level.
        let mut orders: AHashMap<u64, CachedL3Order> = AHashMap::new();

        // BID side — 10 levels, 21 orders total, descending price
        // Level 1: 44939.4 × 8 orders
        for (key, (size_raw, seq)) in [
            ("0.88968699", 1u64),
            ("0.45210000", 2),
            ("0.10000000", 3),
            ("0.14296323", 4),
            ("0.25000000", 5),
            ("0.10292988", 6),
            ("0.33880000", 7),
            ("1.28140860", 8),
        ]
        .iter()
        .enumerate()
        {
            orders.insert(
                *seq,
                CachedL3Order {
                    price: 44939.4,
                    price_raw: "44939.4".to_string(),
                    size: 0.0,
                    size_raw: (*size_raw).to_string(),
                    side: OrderSide::Buy,
                    seq: *seq,
                },
            );
            let _ = key;
        }
        // Level 2: 44937.1 × 1
        orders.insert(
            9,
            CachedL3Order {
                price: 44937.1,
                price_raw: "44937.1".to_string(),
                size: 0.0,
                size_raw: "0.3346877".to_string(),
                side: OrderSide::Buy,
                seq: 9,
            },
        );
        // Level 3: 44934.7 × 1
        orders.insert(
            10,
            CachedL3Order {
                price: 44934.7,
                price_raw: "44934.7".to_string(),
                size: 0.0,
                size_raw: "0.35630000".to_string(),
                side: OrderSide::Buy,
                seq: 10,
            },
        );
        // Level 4: 44930.2 × 5
        for (size_raw, seq) in [
            ("0.22734299", 11u64),
            ("0.1000000", 12),
            ("0.5550000", 13),
            ("0.70000000", 14),
            ("0.15000000", 15),
        ] {
            orders.insert(
                seq,
                CachedL3Order {
                    price: 44930.2,
                    price_raw: "44930.2".to_string(),
                    size: 0.0,
                    size_raw: size_raw.to_string(),
                    side: OrderSide::Buy,
                    seq,
                },
            );
        }
        // Level 5: 44928.0 × 1
        orders.insert(
            16,
            CachedL3Order {
                price: 44928.0,
                price_raw: "44928.0".to_string(),
                size: 0.0,
                size_raw: "0.105240".to_string(),
                side: OrderSide::Buy,
                seq: 16,
            },
        );
        // Level 6: 44919.6 × 1
        orders.insert(
            17,
            CachedL3Order {
                price: 44919.6,
                price_raw: "44919.6".to_string(),
                size: 0.0,
                size_raw: "0.33870000".to_string(),
                side: OrderSide::Buy,
                seq: 17,
            },
        );
        // Level 7: 44919.5 × 1
        orders.insert(
            18,
            CachedL3Order {
                price: 44919.5,
                price_raw: "44919.5".to_string(),
                size: 0.0,
                size_raw: "0.7610000".to_string(),
                side: OrderSide::Buy,
                seq: 18,
            },
        );
        // Level 8: 44912.0 × 1
        orders.insert(
            19,
            CachedL3Order {
                price: 44912.0,
                price_raw: "44912.0".to_string(),
                size: 0.0,
                size_raw: "0.35630000".to_string(),
                side: OrderSide::Buy,
                seq: 19,
            },
        );
        // Level 9: 44909.7 × 1
        orders.insert(
            20,
            CachedL3Order {
                price: 44909.7,
                price_raw: "44909.7".to_string(),
                size: 0.0,
                size_raw: "0.6690000".to_string(),
                side: OrderSide::Buy,
                seq: 20,
            },
        );
        // Level 10: 44901.9 × 1
        orders.insert(
            21,
            CachedL3Order {
                price: 44901.9,
                price_raw: "44901.9".to_string(),
                size: 0.0,
                size_raw: "0.88982".to_string(),
                side: OrderSide::Buy,
                seq: 21,
            },
        );

        // ASK side — 10 levels, 14 orders total, ascending price
        // Level 1: 44939.5 × 4 orders
        for (size_raw, seq) in [
            ("4.52308393", 22u64),
            ("0.00111261", 23),
            ("0.00100000", 24),
            ("0.01000000", 25),
        ] {
            orders.insert(
                seq,
                CachedL3Order {
                    price: 44939.5,
                    price_raw: "44939.5".to_string(),
                    size: 0.0,
                    size_raw: size_raw.to_string(),
                    side: OrderSide::Sell,
                    seq,
                },
            );
        }
        // Level 2: 44950.0 × 1
        orders.insert(
            26,
            CachedL3Order {
                price: 44950.0,
                price_raw: "44950.0".to_string(),
                size: 0.0,
                size_raw: "1.0334926".to_string(),
                side: OrderSide::Sell,
                seq: 26,
            },
        );
        // Level 3: 44953.0 × 1
        orders.insert(
            27,
            CachedL3Order {
                price: 44953.0,
                price_raw: "44953.0".to_string(),
                size: 0.0,
                size_raw: "0.64537".to_string(),
                side: OrderSide::Sell,
                seq: 27,
            },
        );
        // Level 4: 44955.0 × 1
        orders.insert(
            28,
            CachedL3Order {
                price: 44955.0,
                price_raw: "44955.0".to_string(),
                size: 0.0,
                size_raw: "0.250000".to_string(),
                side: OrderSide::Sell,
                seq: 28,
            },
        );
        // Level 5: 44959.6 × 2
        orders.insert(
            29,
            CachedL3Order {
                price: 44959.6,
                price_raw: "44959.6".to_string(),
                size: 0.0,
                size_raw: "0.35630000".to_string(),
                side: OrderSide::Sell,
                seq: 29,
            },
        );
        orders.insert(
            30,
            CachedL3Order {
                price: 44959.6,
                price_raw: "44959.6".to_string(),
                size: 0.0,
                size_raw: "0.35630000".to_string(),
                side: OrderSide::Sell,
                seq: 30,
            },
        );
        // Level 6: 44960.1 × 1
        orders.insert(
            31,
            CachedL3Order {
                price: 44960.1,
                price_raw: "44960.1".to_string(),
                size: 0.0,
                size_raw: "3.38072".to_string(),
                side: OrderSide::Sell,
                seq: 31,
            },
        );
        // Level 7: 44960.2 × 1
        orders.insert(
            32,
            CachedL3Order {
                price: 44960.2,
                price_raw: "44960.2".to_string(),
                size: 0.0,
                size_raw: "0.88967575".to_string(),
                side: OrderSide::Sell,
                seq: 32,
            },
        );
        // Level 8: 44967.0 × 1
        orders.insert(
            33,
            CachedL3Order {
                price: 44967.0,
                price_raw: "44967.0".to_string(),
                size: 0.0,
                size_raw: "3.14392283".to_string(),
                side: OrderSide::Sell,
                seq: 33,
            },
        );
        // Level 9: 44978.5 × 1
        orders.insert(
            34,
            CachedL3Order {
                price: 44978.5,
                price_raw: "44978.5".to_string(),
                size: 0.0,
                size_raw: "0.6778960".to_string(),
                side: OrderSide::Sell,
                seq: 34,
            },
        );
        // Level 10: 44979.2 × 1
        orders.insert(
            35,
            CachedL3Order {
                price: 44979.2,
                price_raw: "44979.2".to_string(),
                size: 0.0,
                size_raw: "0.35630000".to_string(),
                side: OrderSide::Sell,
                seq: 35,
            },
        );

        let result = build_checksum_string(&orders);
        let expected_asks = "44939545230839344939511126144939510000044939510000004495001033492644953064537449550250000449596356300004495963563000044960133807244960288967575449670314392283449785677896044979235630000";
        let expected_bids = "449394889686994493944521000044939410000000449394142963234493942500000044939410292988449394338800004493941281408604493713346877449347356300004493022273429944930210000004493025550000449302700000004493021500000044928010524044919633870000449195761000044912035630000449097669000044901988982";
        assert_eq!(
            result,
            format!("{expected_asks}{expected_bids}"),
            "checksum string mismatch\ngot:      {result}\nexpected: {expected_asks}{expected_bids}",
        );
        assert_eq!(compute_checksum(&orders), 1_063_832_831u32);
    }

    #[rstest]
    fn test_compute_checksum_single_ask_bid() {
        let mut orders = AHashMap::new();
        // ask: "42001.0" → "420010", "0.50000000" → "50000000"
        orders.insert(
            1_u64,
            CachedL3Order {
                price: 42001.0,
                price_raw: "42001.0".to_string(),
                size: 0.5,
                size_raw: "0.50000000".to_string(),
                side: OrderSide::Sell,
                seq: 0,
            },
        );
        // bid: "41999.0" → "419990", "0.30000000" → "30000000"
        orders.insert(
            2_u64,
            CachedL3Order {
                price: 41999.0,
                price_raw: "41999.0".to_string(),
                size: 0.3,
                size_raw: "0.30000000".to_string(),
                side: OrderSide::Buy,
                seq: 1,
            },
        );
        // full string: "42001050000000" + "41999030000000"
        let checksum = compute_checksum(&orders);
        assert_eq!(checksum, crc32_ieee(b"4200105000000041999030000000"));
    }
}
