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

//! Futures symbol parsing and formatting.

/// The parsed components of a futures symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuturesSymbol<'a> {
    /// Symbol root before the month code.
    pub root: &'a str,
    /// Standard futures month code.
    pub month_code: char,
    /// One-, two-, or four-digit year suffix.
    pub year_digits: &'a str,
}

/// Parses a single-leg futures symbol such as `ESZ6`, `ESZ26`, or `ESZ2026`.
/// Callers must split multi-leg symbols before parsing.
#[must_use]
pub fn parse_futures_symbol(symbol: &str) -> Option<FuturesSymbol<'_>> {
    for (month_pos, month_code) in symbol.char_indices().rev() {
        if futures_month(month_code).is_none() {
            continue;
        }

        let year_digits = &symbol[month_pos + month_code.len_utf8()..];
        if !matches!(year_digits.len(), 1 | 2 | 4)
            || !year_digits.bytes().all(|b| b.is_ascii_digit())
            || month_pos == 0
        {
            continue;
        }

        return Some(FuturesSymbol {
            root: &symbol[..month_pos],
            month_code,
            year_digits,
        });
    }

    None
}

/// Formats a futures symbol with one, two, or four year digits.
#[must_use]
pub fn format_futures_symbol(
    root: &str,
    month_code: char,
    year: i32,
    year_digits: u8,
) -> Option<String> {
    if root.is_empty() || futures_month(month_code).is_none() || !(0..=9999).contains(&year) {
        return None;
    }

    let year = match year_digits {
        1 => format!("{}", year % 10),
        2 => format!("{:02}", year % 100),
        4 => format!("{year:04}"),
        _ => return None,
    };

    Some(format!("{root}{month_code}{year}"))
}

/// Resolves one-, two-, or four-digit futures years against a reference year.
///
/// A single digit resolves to the nearest matching year at or after the reference year,
/// matching venue listing practice. Two digits carry the century-local year and resolve
/// inside a window from 30 years before to 69 years after the reference year, so a
/// recently expired id (`ESZ25` referenced in 2026) resolves to its actual year instead
/// of a century ahead. Four digits are exact.
#[must_use]
pub fn resolve_futures_year(year_digits: &str, reference_year: i32) -> Option<i32> {
    if reference_year < 0 || !year_digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let partial = year_digits.parse::<i32>().ok()?;
    match year_digits.len() {
        1 => {
            let mut year = reference_year - reference_year.rem_euclid(10) + partial;
            if year < reference_year {
                year += 10;
            }
            Some(year)
        }
        2 => {
            let mut year = reference_year - reference_year.rem_euclid(100) + partial;
            if year < reference_year - 30 {
                year += 100;
            } else if year > reference_year + 69 {
                year -= 100;
            }
            Some(year)
        }
        4 => Some(partial),
        _ => None,
    }
}

/// Returns the standard futures month code for a calendar month.
#[must_use]
pub const fn futures_month_code(month: u8) -> Option<char> {
    match month {
        1 => Some('F'),
        2 => Some('G'),
        3 => Some('H'),
        4 => Some('J'),
        5 => Some('K'),
        6 => Some('M'),
        7 => Some('N'),
        8 => Some('Q'),
        9 => Some('U'),
        10 => Some('V'),
        11 => Some('X'),
        12 => Some('Z'),
        _ => None,
    }
}

/// Returns the calendar month for a standard futures month code.
#[must_use]
pub const fn futures_month(month_code: char) -> Option<u8> {
    match month_code {
        'F' => Some(1),
        'G' => Some(2),
        'H' => Some(3),
        'J' => Some(4),
        'K' => Some(5),
        'M' => Some(6),
        'N' => Some(7),
        'Q' => Some(8),
        'U' => Some(9),
        'V' => Some(10),
        'X' => Some(11),
        'Z' => Some(12),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("ESZ6", "ES", 'Z', "6")]
    #[case("ESZ26", "ES", 'Z', "26")]
    #[case("ESZ2026", "ES", 'Z', "2026")]
    #[case("EX2G3", "EX2", 'G', "3")]
    fn test_parse_futures_symbol(
        #[case] value: &str,
        #[case] root: &str,
        #[case] month_code: char,
        #[case] year_digits: &str,
    ) {
        assert_eq!(
            parse_futures_symbol(value),
            Some(FuturesSymbol {
                root,
                month_code,
                year_digits,
            })
        );
    }

    #[rstest]
    #[case(1, "ESZ6")]
    #[case(2, "ESZ26")]
    #[case(4, "ESZ2026")]
    fn test_format_futures_symbol(#[case] digits: u8, #[case] expected: &str) {
        assert_eq!(
            format_futures_symbol("ES", 'Z', 2026, digits).as_deref(),
            Some(expected)
        );
    }

    #[rstest]
    #[case("6", 2026, 2026)]
    #[case("5", 2026, 2035)]
    #[case("26", 2026, 2026)]
    #[case("25", 2026, 2025)]
    #[case("96", 2026, 1996)]
    #[case("95", 2026, 2095)]
    #[case("00", 2099, 2100)]
    #[case("01", 2099, 2101)]
    #[case("2026", 2099, 2026)]
    fn test_resolve_futures_year(
        #[case] digits: &str,
        #[case] reference_year: i32,
        #[case] expected: i32,
    ) {
        assert_eq!(resolve_futures_year(digits, reference_year), Some(expected));
    }

    #[rstest]
    #[case(1, 'F')]
    #[case(6, 'M')]
    #[case(12, 'Z')]
    fn test_futures_month_round_trip(#[case] month: u8, #[case] code: char) {
        assert_eq!(futures_month_code(month), Some(code));
        assert_eq!(futures_month(code), Some(month));
    }

    #[rstest]
    #[case("")]
    #[case("ES")]
    #[case("ESZ")]
    #[case("ESZ123")]
    #[case("Z26")]
    fn test_parse_futures_symbol_rejects_invalid_values(#[case] value: &str) {
        assert_eq!(parse_futures_symbol(value), None);
    }
}
