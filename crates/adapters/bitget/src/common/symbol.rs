// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use chrono::{Datelike, TimeZone, Utc};

use crate::common::enums::BitgetInstrumentKind;

#[must_use]
pub fn nautilus_symbol_for_spot(raw: &str) -> String {
    raw.to_string()
}

#[must_use]
pub fn nautilus_symbol_for_perp(raw: &str) -> String {
    format!("{raw}-PERP")
}

#[must_use]
pub fn nautilus_symbol_for_delivery(raw: &str, delivery_time_ms: i64) -> String {
    let dt = Utc
        .timestamp_millis_opt(delivery_time_ms)
        .single()
        .unwrap_or_else(|| Utc.timestamp_nanos(0));
    let yy = dt.year().rem_euclid(100);
    let mm = dt.month();
    let dd = dt.day();
    format!("{raw}-{yy:02}{mm:02}{dd:02}")
}

#[must_use]
pub fn parse_nautilus_symbol(symbol: &str) -> BitgetInstrumentKind {
    if symbol.ends_with("-PERP") {
        return BitgetInstrumentKind::Perp;
    }

    if let Some((_, suffix)) = symbol.rsplit_once('-')
        && suffix.len() == 6
        && suffix.as_bytes().iter().all(u8::is_ascii_digit)
    {
        return BitgetInstrumentKind::Delivery;
    }

    BitgetInstrumentKind::Spot
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn round_trip_spot() {
        let symbol = nautilus_symbol_for_spot("BTCUSDT");
        assert_eq!(symbol, "BTCUSDT");
        assert_eq!(parse_nautilus_symbol(&symbol), BitgetInstrumentKind::Spot);
    }

    #[rstest]
    fn round_trip_perp() {
        let symbol = nautilus_symbol_for_perp("BTCUSDT");
        assert_eq!(symbol, "BTCUSDT-PERP");
        assert_eq!(parse_nautilus_symbol(&symbol), BitgetInstrumentKind::Perp);
    }

    #[rstest]
    fn round_trip_delivery() {
        let symbol = nautilus_symbol_for_delivery("BTCUSDT", 1_782_432_000_000);
        assert_eq!(symbol, "BTCUSDT-260626");
        assert_eq!(
            parse_nautilus_symbol(&symbol),
            BitgetInstrumentKind::Delivery
        );
    }
}
