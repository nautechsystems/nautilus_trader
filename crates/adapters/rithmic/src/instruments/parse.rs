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

//! Parsing utilities for Rithmic instrument data.

use rithmic_rs::rti::ResponseReferenceData;

use crate::{
    common::parse::tick_size_to_precision,
    error::{Result, RithmicError},
};

use super::provider::RithmicInstrument;

/// Parses instrument data from Rithmic response.
///
/// This function will be used to convert Rithmic protocol buffer
/// messages into `RithmicInstrument` structs.
#[allow(dead_code)] // Scaffolding for future implementation
pub fn parse_instrument(
    symbol: &str,
    exchange: &str,
    product_code: &str,
    description: &str,
    tick_size: f64,
    point_value: f64,
    currency: &str,
) -> Result<RithmicInstrument> {
    if symbol.is_empty() {
        return Err(RithmicError::Parse("Symbol cannot be empty".to_string()));
    }

    if exchange.is_empty() {
        return Err(RithmicError::Parse("Exchange cannot be empty".to_string()));
    }

    if tick_size <= 0.0 {
        return Err(RithmicError::Parse(format!(
            "Invalid tick size: {tick_size}"
        )));
    }

    let price_precision = tick_size_to_precision(tick_size);

    Ok(RithmicInstrument {
        symbol: symbol.to_string(),
        exchange: exchange.to_string(),
        product_code: product_code.to_string(),
        description: description.to_string(),
        tick_size,
        point_value,
        currency: currency.to_string(),
        contract_size: 1.0,
        price_precision,
        size_precision: 0, // Futures typically have integer quantities
        expiration_ts: None,
        is_tradeable: true,
    })
}

/// Parses expiration from contract symbol.
///
/// Returns (month, year) tuple.
/// Example: "ESZ4" -> (12, 2024)
#[allow(dead_code)] // Scaffolding for future implementation
pub fn parse_contract_expiry(symbol: &str) -> Result<(u32, u32)> {
    if symbol.len() < 2 {
        return Err(RithmicError::Parse(format!(
            "Symbol too short for expiry: {symbol}"
        )));
    }

    let expiry_part = &symbol[symbol.len() - 2..];
    let month_code = expiry_part.chars().next().unwrap();
    let year_digit = expiry_part.chars().nth(1).unwrap();

    let month = crate::common::converters::month_code_to_number(month_code)?;

    let year_suffix = year_digit
        .to_digit(10)
        .ok_or_else(|| RithmicError::Parse(format!("Invalid year digit: {year_digit}")))?;

    // Assume 2020s for single digit years (0-9 -> 2020-2029)
    // This will need adjustment after 2029
    let year = 2020 + year_suffix;

    Ok((month, year))
}

/// Converts a Rithmic `ResponseReferenceData` to a `RithmicInstrument`.
///
/// This is the primary transformation function for converting Rithmic's
/// protocol buffer response into the adapter's instrument representation.
///
/// # Arguments
///
/// * `ref_data` - The reference data response from Rithmic ticker plant
///
/// # Returns
///
/// A `RithmicInstrument` with all available fields populated, or an error
/// if required fields (symbol, exchange) are missing.
///
/// # Example
///
/// ```ignore
/// let response = ticker_handle.get_reference_data("ESH5", "CME").await?;
/// if let RithmicMessage::ResponseReferenceData(ref_data) = response.message {
///     let instrument = response_to_instrument(&ref_data)?;
/// }
/// ```
pub fn response_to_instrument(ref_data: &ResponseReferenceData) -> Result<RithmicInstrument> {
    let symbol = ref_data
        .symbol
        .as_ref()
        .ok_or_else(|| RithmicError::Instrument("Missing symbol in reference data".to_string()))?
        .clone();

    let exchange = ref_data
        .exchange
        .as_ref()
        .ok_or_else(|| RithmicError::Instrument("Missing exchange in reference data".to_string()))?
        .clone();

    let product_code = ref_data.product_code.clone().unwrap_or_default();
    let description = ref_data.symbol_name.clone().unwrap_or_default();

    // min_qprice_change is the tick size (minimum price change in quote units)
    let tick_size = ref_data.min_qprice_change.unwrap_or_else(|| {
        tracing::warn!(
            "No tick size in reference data for {}, using default 0.01",
            symbol
        );
        0.01
    });

    // single_point_value is the dollar value per point
    let point_value = ref_data.single_point_value.unwrap_or_else(|| {
        tracing::warn!(
            "No point value in reference data for {}, using default 1.0",
            symbol
        );
        1.0
    });

    let currency = ref_data
        .currency
        .clone()
        .unwrap_or_else(|| "USD".to_string());

    let price_precision = tick_size_to_precision(tick_size);

    // Parse expiration date from Rithmic format (typically YYYYMMDD or similar)
    let expiration_ts = ref_data
        .expiration_date
        .as_ref()
        .and_then(|date_str| parse_expiration_date(date_str).ok());

    // is_tradable is a string "true"/"false" in the protobuf
    let is_tradeable = ref_data
        .is_tradable
        .as_ref()
        .is_none_or(|s| s.eq_ignore_ascii_case("true") || s == "1");

    Ok(RithmicInstrument {
        symbol,
        exchange,
        product_code,
        description,
        tick_size,
        point_value,
        currency,
        contract_size: 1.0, // Futures are typically 1 contract
        price_precision,
        size_precision: 0, // Futures are whole contracts
        expiration_ts,
        is_tradeable,
    })
}

/// Parses a Rithmic expiration date string to Unix timestamp (nanoseconds).
///
/// Rithmic typically returns dates in YYYYMMDD format.
///
/// # Arguments
///
/// * `date_str` - The date string from Rithmic (e.g., "20240315")
///
/// # Returns
///
/// Unix timestamp in nanoseconds, or an error if parsing fails.
pub fn parse_expiration_date(date_str: &str) -> Result<u64> {
    // Rithmic uses YYYYMMDD format
    if date_str.len() < 8 {
        return Err(RithmicError::Parse(format!(
            "Invalid expiration date format: {date_str}"
        )));
    }

    let year: i32 = date_str[0..4]
        .parse()
        .map_err(|_| RithmicError::Parse(format!("Invalid year in date: {date_str}")))?;

    let month: u32 = date_str[4..6]
        .parse()
        .map_err(|_| RithmicError::Parse(format!("Invalid month in date: {date_str}")))?;

    let day: u32 = date_str[6..8]
        .parse()
        .map_err(|_| RithmicError::Parse(format!("Invalid day in date: {date_str}")))?;

    // Convert to Unix timestamp at midnight UTC
    // Using a simple calculation (days since epoch * seconds per day * nanos per second)
    // This is a simplified calculation; for production, consider using chrono
    let days_since_epoch = days_from_civil(year, month, day);
    let timestamp_secs = days_since_epoch as i64 * 86400;
    let timestamp_nanos = timestamp_secs as u64 * 1_000_000_000;

    Ok(timestamp_nanos)
}

/// Converts civil date (year, month, day) to days since Unix epoch.
///
/// This is a simplified algorithm for date conversion.
/// Based on Howard Hinnant's date algorithms.
fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i32 - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn test_parse_instrument() {
        let instrument =
            parse_instrument("ESZ4", "CME", "ES", "E-mini S&P 500", 0.25, 50.0, "USD").unwrap();

        assert_eq!(instrument.symbol, "ESZ4");
        assert_eq!(instrument.exchange, "CME");
        assert_eq!(instrument.tick_size, 0.25);
        assert_eq!(instrument.price_precision, 2);
    }

    #[rstest::rstest]
    fn test_parse_contract_expiry() {
        let (month, year) = parse_contract_expiry("ESZ4").unwrap();
        assert_eq!(month, 12); // December
        assert_eq!(year, 2024);

        let (month, year) = parse_contract_expiry("CLF5").unwrap();
        assert_eq!(month, 1); // January
        assert_eq!(year, 2025);
    }

    #[rstest::rstest]
    fn test_response_to_instrument() {
        let ref_data = ResponseReferenceData {
            template_id: 15,
            user_msg: vec![],
            rp_code: vec![],
            presence_bits: None,
            clear_bits: None,
            symbol: Some("ESH5".to_string()),
            exchange: Some("CME".to_string()),
            exchange_symbol: None,
            symbol_name: Some("E-mini S&P 500 Mar25".to_string()),
            trading_symbol: None,
            trading_exchange: None,
            product_code: Some("ES".to_string()),
            instrument_type: Some("Future".to_string()),
            underlying_symbol: None,
            expiration_date: Some("20250321".to_string()),
            currency: Some("USD".to_string()),
            put_call_indicator: None,
            tick_size_type: None,
            price_display_format: None,
            is_tradable: Some("true".to_string()),
            is_underlying_for_binary_contrats: None,
            strike_price: None,
            ftoq_price: None,
            qtof_price: None,
            min_qprice_change: Some(0.25),
            min_fprice_change: None,
            single_point_value: Some(50.0),
        };

        let instrument = response_to_instrument(&ref_data).unwrap();

        assert_eq!(instrument.symbol, "ESH5");
        assert_eq!(instrument.exchange, "CME");
        assert_eq!(instrument.product_code, "ES");
        assert_eq!(instrument.description, "E-mini S&P 500 Mar25");
        assert_eq!(instrument.tick_size, 0.25);
        assert_eq!(instrument.point_value, 50.0);
        assert_eq!(instrument.currency, "USD");
        assert_eq!(instrument.price_precision, 2);
        assert!(instrument.is_tradeable);
        assert!(instrument.expiration_ts.is_some());
    }

    #[rstest::rstest]
    fn test_response_to_instrument_missing_symbol() {
        let ref_data = ResponseReferenceData {
            template_id: 15,
            user_msg: vec![],
            rp_code: vec![],
            presence_bits: None,
            clear_bits: None,
            symbol: None, // Missing symbol
            exchange: Some("CME".to_string()),
            exchange_symbol: None,
            symbol_name: None,
            trading_symbol: None,
            trading_exchange: None,
            product_code: None,
            instrument_type: None,
            underlying_symbol: None,
            expiration_date: None,
            currency: None,
            put_call_indicator: None,
            tick_size_type: None,
            price_display_format: None,
            is_tradable: None,
            is_underlying_for_binary_contrats: None,
            strike_price: None,
            ftoq_price: None,
            qtof_price: None,
            min_qprice_change: None,
            min_fprice_change: None,
            single_point_value: None,
        };

        let result = response_to_instrument(&ref_data);
        assert!(result.is_err());
    }

    #[rstest::rstest]
    fn test_response_to_instrument_defaults() {
        // Test with minimal data to verify defaults are applied
        let ref_data = ResponseReferenceData {
            template_id: 15,
            user_msg: vec![],
            rp_code: vec![],
            presence_bits: None,
            clear_bits: None,
            symbol: Some("NQZ4".to_string()),
            exchange: Some("CME".to_string()),
            exchange_symbol: None,
            symbol_name: None,
            trading_symbol: None,
            trading_exchange: None,
            product_code: None,
            instrument_type: None,
            underlying_symbol: None,
            expiration_date: None,
            currency: None,
            put_call_indicator: None,
            tick_size_type: None,
            price_display_format: None,
            is_tradable: None,
            is_underlying_for_binary_contrats: None,
            strike_price: None,
            ftoq_price: None,
            qtof_price: None,
            min_qprice_change: None,
            min_fprice_change: None,
            single_point_value: None,
        };

        let instrument = response_to_instrument(&ref_data).unwrap();

        assert_eq!(instrument.symbol, "NQZ4");
        assert_eq!(instrument.exchange, "CME");
        assert_eq!(instrument.product_code, ""); // Default empty
        assert_eq!(instrument.tick_size, 0.01); // Default
        assert_eq!(instrument.point_value, 1.0); // Default
        assert_eq!(instrument.currency, "USD"); // Default
        assert!(instrument.is_tradeable); // Default true
        assert!(instrument.expiration_ts.is_none());
    }

    #[rstest::rstest]
    fn test_parse_expiration_date() {
        // March 15, 2024
        let ts = parse_expiration_date("20240315").unwrap();
        // This should be approximately 1710460800000000000 nanoseconds
        // (seconds since epoch * 1e9)
        assert!(ts > 0);

        // December 31, 2024
        let ts2 = parse_expiration_date("20241231").unwrap();
        assert!(ts2 > ts); // Later date should have higher timestamp
    }

    #[rstest::rstest]
    fn test_parse_expiration_date_invalid() {
        // Too short
        assert!(parse_expiration_date("2024").is_err());

        // Invalid month (not a number)
        assert!(parse_expiration_date("2024XX15").is_err());
    }

    #[rstest::rstest]
    fn test_days_from_civil() {
        // Unix epoch is 1970-01-01
        assert_eq!(days_from_civil(1970, 1, 1), 0);

        // 2024-01-01 should be positive
        let days = days_from_civil(2024, 1, 1);
        assert!(days > 0);

        // 2024 is a leap year, so 2024-03-01 - 2024-02-01 = 29 days
        let feb1 = days_from_civil(2024, 2, 1);
        let mar1 = days_from_civil(2024, 3, 1);
        assert_eq!(mar1 - feb1, 29);
    }
}
