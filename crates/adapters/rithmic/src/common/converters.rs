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

//! Symbol parsing utilities.

use crate::error::{Result, RithmicError};

/// Parses a futures symbol into (product, expiry) components.
/// e.g. "ESZ4" -> ("ES", "Z4")
pub fn parse_symbol(symbol: &str) -> Result<(&str, &str)> {
    if symbol.len() < 3 {
        return Err(RithmicError::Parse(format!("Invalid symbol: {symbol}")));
    }
    let split_idx = symbol.len() - 2;
    Ok((&symbol[..split_idx], &symbol[split_idx..]))
}

/// Converts futures month code to month number (F=1, G=2, ..., Z=12).
pub fn month_code_to_number(code: char) -> Result<u32> {
    match code.to_ascii_uppercase() {
        'F' => Ok(1),
        'G' => Ok(2),
        'H' => Ok(3),
        'J' => Ok(4),
        'K' => Ok(5),
        'M' => Ok(6),
        'N' => Ok(7),
        'Q' => Ok(8),
        'U' => Ok(9),
        'V' => Ok(10),
        'X' => Ok(11),
        'Z' => Ok(12),
        _ => Err(RithmicError::Parse(format!("Invalid month code: {code}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn test_parse_symbol() {
        let (product, expiry) = parse_symbol("ESZ4").unwrap();
        assert_eq!(product, "ES");
        assert_eq!(expiry, "Z4");

        let (product, expiry) = parse_symbol("MESZ4").unwrap();
        assert_eq!(product, "MES");
        assert_eq!(expiry, "Z4");

        assert!(parse_symbol("ES").is_err()); // too short
    }

    #[rstest::rstest]
    fn test_month_code_to_number() {
        assert_eq!(month_code_to_number('F').unwrap(), 1);
        assert_eq!(month_code_to_number('Z').unwrap(), 12);
        assert!(month_code_to_number('A').is_err());
    }
}
