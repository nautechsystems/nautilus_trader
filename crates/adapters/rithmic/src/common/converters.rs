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

    #[test]
    fn test_parse_symbol() {
        let (product, expiry) = parse_symbol("ESZ4").unwrap();
        assert_eq!(product, "ES");
        assert_eq!(expiry, "Z4");

        let (product, expiry) = parse_symbol("MESZ4").unwrap();
        assert_eq!(product, "MES");
        assert_eq!(expiry, "Z4");

        assert!(parse_symbol("ES").is_err()); // too short
    }

    #[test]
    fn test_month_code_to_number() {
        assert_eq!(month_code_to_number('F').unwrap(), 1);
        assert_eq!(month_code_to_number('Z').unwrap(), 12);
        assert!(month_code_to_number('A').is_err());
    }
}
