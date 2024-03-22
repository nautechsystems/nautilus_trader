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

use databento::historical::DateTimeRange;
use nautilus_core::time::UnixNanos;
use time::OffsetDateTime;

pub const DATABENTO: &str = "DATABENTO";
pub const ALL_SYMBOLS: &str = "ALL_SYMBOLS";

pub fn get_date_time_range(start: UnixNanos, end: UnixNanos) -> anyhow::Result<DateTimeRange> {
    Ok(DateTimeRange::from((
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(start))?,
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(end))?,
    )))
}

#[must_use]
pub fn infer_symbology_type(symbol: &str) -> String {
    if symbol.ends_with(".FUT") || symbol.ends_with(".OPT") {
        return "parent".to_string();
    }

    let parts: Vec<&str> = symbol.split('.').collect();
    if parts.len() == 3 && parts[2].chars().all(|c| c.is_ascii_digit()) {
        return "continuous".to_string();
    }

    "raw_symbol".to_string()
}

pub fn check_consistent_symbology(symbols: &[&str]) -> anyhow::Result<()> {
    if symbols.is_empty() {
        return Err(anyhow::anyhow!("Symbols was empty"));
    };

    // SAFETY: We checked len so know there must be at least one symbol
    let first_symbol = symbols.first().unwrap();
    let first_stype = infer_symbology_type(first_symbol);

    for symbol in symbols {
        let next_stype = infer_symbology_type(symbol);
        if next_stype != first_stype {
            return Err(anyhow::anyhow!(
                "Inconsistent symbology types: '{}' for {} vs '{}' for {}",
                first_stype,
                first_symbol,
                next_stype,
                symbol
            ));
        }
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    #[case("AAPL", "raw_symbol")]
    #[case("ESM4", "raw_symbol")]
    #[case("BRN FMM0024!", "raw_symbol")]
    #[case("BRN  99   5617289", "raw_symbol")]
    #[case("SPY   240319P00511000", "raw_symbol")]
    #[case("ES.FUT", "parent")]
    #[case("ES.OPT", "parent")]
    #[case("BRN.FUT", "parent")]
    #[case("SPX.OPT", "parent")]
    #[case("ES.c.0", "continuous")]
    #[case("SPX.n.0", "continuous")]
    fn test_infer_symbology_type(#[case] symbol: String, #[case] expected: String) {
        let result = infer_symbology_type(&symbol);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_check_consistent_symbology_when_empty_symbols() {
        let symbols: Vec<&str> = vec![];
        let result = check_consistent_symbology(&symbols);
        assert!(result.is_err());
        assert_eq!(result.err().unwrap().to_string(), "Symbols was empty");
    }

    #[rstest]
    fn test_check_consistent_symbology_when_inconsistent() {
        let symbols = vec!["ESM4", "ES.OPT"];
        let result = check_consistent_symbology(&symbols);
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Inconsistent symbology types: 'raw_symbol' for ESM4 vs 'parent' for ES.OPT"
        );
    }

    #[rstest]
    #[case(vec!["AAPL,MSFT"])]
    #[case(vec!["ES.OPT,ES.FUT"])]
    #[case(vec!["ES.c.0,ES.c.1"])]
    fn test_check_consistent_symbology_when_consistent(#[case] symbols: Vec<&str>) {
        let result = check_consistent_symbology(&symbols);
        assert!(result.is_ok());
    }
}
