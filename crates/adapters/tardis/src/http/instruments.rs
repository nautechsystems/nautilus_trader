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

use nautilus_core::UnixNanos;
use nautilus_model::{
    currencies::CURRENCY_MAP,
    enums::CurrencyType,
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoFuture, CryptoOption, CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

use super::{models::TardisInstrumentInfo, parse::parse_settlement_currency};
use crate::parse::parse_option_kind;

/// Returns the currency either from the internal currency map or creates a default crypto.
pub(crate) fn get_currency(code: &str) -> Currency {
    // SAFETY: Mutex should not be poisoned in normal operation
    CURRENCY_MAP
        .lock()
        .expect("Failed to acquire CURRENCY_MAP lock")
        .get(code)
        .copied()
        .unwrap_or(Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn create_currency_pair(
    info: &TardisInstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Option<Quantity>,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        Some(size_increment),
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn create_crypto_perpetual(
    info: &TardisInstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Option<Quantity>,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    let is_inverse = info.inverse.unwrap_or(false);

    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(parse_settlement_currency(info, is_inverse).as_str()),
        is_inverse,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        Some(size_increment),
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn create_crypto_future(
    info: &TardisInstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    activation: UnixNanos,
    expiration: UnixNanos,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Option<Quantity>,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    let is_inverse = info.inverse.unwrap_or(false);

    InstrumentAny::CryptoFuture(CryptoFuture::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(parse_settlement_currency(info, is_inverse).as_str()),
        is_inverse,
        activation,
        expiration,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        Some(size_increment),
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}

#[allow(clippy::too_many_arguments)]
/// Create a crypto option instrument definition.
///
/// # Panics
///
/// Panics if the `option_type` field of `InstrumentInfo` is `None`.
#[must_use]
pub fn create_crypto_option(
    info: &TardisInstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    activation: UnixNanos,
    expiration: UnixNanos,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Option<Quantity>,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    let is_inverse = info.inverse.unwrap_or(false);

    InstrumentAny::CryptoOption(CryptoOption::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(parse_settlement_currency(info, is_inverse).as_str()),
        is_inverse,
        parse_option_kind(
            info.option_type
                .expect("CryptoOption should have `option_type` field"),
        ),
        Price::new(
            info.strike_price
                .expect("CryptoOption should have `strike_price` field"),
            price_increment.precision,
        ),
        activation,
        expiration,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        Some(size_increment),
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}

/// Checks if an instrument is available and valid based on time constraints.
pub fn is_available(
    info: &TardisInstrumentInfo,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    available_offset: Option<UnixNanos>,
    effective: Option<UnixNanos>,
) -> bool {
    let available_since =
        UnixNanos::from(info.available_since) + available_offset.unwrap_or_default();
    let available_to = info.available_to.map_or(UnixNanos::max(), UnixNanos::from);

    if let Some(effective_date) = effective {
        // Effective date must be within availability period
        if available_since >= effective_date || available_to <= effective_date {
            return false;
        }

        // Effective date must be within requested [start, end] if provided
        if start.is_some_and(|s| effective_date < s) || end.is_some_and(|e| effective_date > e) {
            return false;
        }
    } else {
        // Otherwise check for overlap between [available_since, available_to] and [start, end]
        if start.is_some_and(|s| available_to < s) || end.is_some_and(|e| available_since > e) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tests::load_test_json;

    // Helper to create a basic instrument info for testing
    fn create_test_instrument(
        available_since: u64,
        available_to: Option<u64>,
    ) -> TardisInstrumentInfo {
        let json_data = load_test_json("instrument_spot.json");
        let mut info: TardisInstrumentInfo = serde_json::from_str(&json_data).unwrap();
        info.available_since = UnixNanos::from(available_since).to_datetime_utc();
        info.available_to = available_to.map(|a| UnixNanos::from(a).to_datetime_utc());
        info
    }

    #[rstest]
    #[case::no_constraints(None, None, None, None, true)]
    #[case::within_start_end(Some(100), Some(300), None, None, true)]
    #[case::before_start(Some(200), Some(300), None, None, true)]
    #[case::after_end(Some(100), Some(150), None, None, true)]
    #[case::with_offset_within_range(Some(200), Some(300), Some(50), None, true)]
    #[case::with_offset_adjusted_within_range(Some(150), Some(300), Some(50), None, true)]
    #[case::effective_within_availability(None, None, None, Some(150), true)]
    #[case::effective_before_availability(None, None, None, Some(50), false)]
    #[case::effective_after_availability(None, None, None, Some(250), false)]
    #[case::effective_within_start_end(Some(100), Some(200), None, Some(150), true)]
    #[case::effective_before_start(Some(150), Some(200), None, Some(120), false)]
    #[case::effective_after_end(Some(100), Some(150), None, Some(180), false)]
    #[case::effective_equals_available_since(None, None, None, Some(100), false)]
    #[case::effective_equals_available_to(None, None, None, Some(200), false)]
    fn test_is_available(
        #[case] start: Option<u64>,
        #[case] end: Option<u64>,
        #[case] available_offset: Option<u64>,
        #[case] effective: Option<u64>,
        #[case] expected: bool,
    ) {
        // Create instrument with fixed availability 100-200
        let info = create_test_instrument(100, Some(200));

        // Convert all u64 values to UnixNanos
        let start_nanos = start.map(UnixNanos::from);
        let end_nanos = end.map(UnixNanos::from);
        let offset_nanos = available_offset.map(UnixNanos::from);
        let effective_nanos = effective.map(UnixNanos::from);

        // Run the test
        let result = is_available(&info, start_nanos, end_nanos, offset_nanos, effective_nanos);

        assert_eq!(
            result, expected,
            "Test failed with start={start:?}, end={end:?}, offset={available_offset:?}, effective={effective:?}"
        );
    }

    #[rstest]
    fn test_infinite_available_to() {
        // Create instrument with infinite availability (no end date)
        let info = create_test_instrument(100, None);

        // Should be available for any end date
        assert!(is_available(
            &info,
            None,
            Some(UnixNanos::from(1000000)),
            None,
            None
        ));

        // Should be available for any effective date after available_since
        assert!(is_available(
            &info,
            None,
            None,
            None,
            Some(UnixNanos::from(101))
        ));

        // Should not be available for effective date before or equal to available_since
        assert!(!is_available(
            &info,
            None,
            None,
            None,
            Some(UnixNanos::from(100))
        ));
        assert!(!is_available(
            &info,
            None,
            None,
            None,
            Some(UnixNanos::from(99))
        ));
    }

    #[rstest]
    fn test_available_offset_effects() {
        // Create instrument with fixed availability 100-200
        let info = create_test_instrument(100, Some(200));

        // Without offset, effective date of 100 is invalid (boundary condition)
        assert!(!is_available(
            &info,
            None,
            None,
            None,
            Some(UnixNanos::from(100))
        ));

        // With offset of 10, effective date of 100 should still be invalid (since available_since becomes 110)
        assert!(!is_available(
            &info,
            None,
            None,
            Some(UnixNanos::from(10)),
            Some(UnixNanos::from(100))
        ));

        // Test with larger offset
        assert!(!is_available(
            &info,
            None,
            None,
            Some(UnixNanos::from(20)),
            Some(UnixNanos::from(119))
        ));
        assert!(is_available(
            &info,
            None,
            None,
            Some(UnixNanos::from(20)),
            Some(UnixNanos::from(121))
        ));
    }

    #[rstest]
    fn test_with_real_dates() {
        // Using realistic Unix timestamps (milliseconds since epoch)
        // April 24, 2023 00:00:00 UTC = 1682294400000
        // April 2, 2024 12:10:00 UTC = 1712061000000

        let info = create_test_instrument(1682294400000, Some(1712061000000));

        // Test effective date is within range
        let mid_date = UnixNanos::from(1695000000000); // Sept 2023
        assert!(is_available(&info, None, None, None, Some(mid_date)));

        // Test with start/end constraints
        let start = UnixNanos::from(1690000000000); // July 2023
        let end = UnixNanos::from(1700000000000); // Nov 2023
        assert!(is_available(
            &info,
            Some(start),
            Some(end),
            None,
            Some(mid_date)
        ));

        // Test with offset (1 day = 86400000 ms)
        let offset = UnixNanos::from(86400000); // 1 day

        // Now the instrument is available 1 day later
        let day_after_start = UnixNanos::from(1682294400000 + 86400000);
        assert!(!is_available(
            &info,
            None,
            None,
            Some(offset),
            Some(day_after_start)
        ));

        // Effective date at exactly the start should fail
        let start_date = UnixNanos::from(1682294400000);
        assert!(!is_available(&info, None, None, None, Some(start_date)));

        // Effective date at exactly the end should fail
        let end_date = UnixNanos::from(1712061000000);
        assert!(!is_available(&info, None, None, None, Some(end_date)));
    }

    #[rstest]
    fn test_complex_scenarios() {
        // Create instrument with fixed availability 100-200
        let info = create_test_instrument(100, Some(200));

        // Scenario: Start and end window partially overlaps with availability
        assert!(is_available(
            &info,
            Some(UnixNanos::from(150)),
            Some(UnixNanos::from(250)),
            None,
            None
        ));
        assert!(is_available(
            &info,
            Some(UnixNanos::from(50)),
            Some(UnixNanos::from(150)),
            None,
            None
        ));

        // Scenario: Start and end window completely contains availability
        assert!(is_available(
            &info,
            Some(UnixNanos::from(50)),
            Some(UnixNanos::from(250)),
            None,
            None
        ));

        // Scenario: Start and end window completely within availability
        assert!(is_available(
            &info,
            Some(UnixNanos::from(120)),
            Some(UnixNanos::from(180)),
            None,
            None
        ));

        // Scenario: Effective date with start/end constraints
        assert!(is_available(
            &info,
            Some(UnixNanos::from(120)),
            Some(UnixNanos::from(180)),
            None,
            Some(UnixNanos::from(150))
        ));

        // Scenario: Effective date outside start/end constraints but within availability
        assert!(!is_available(
            &info,
            Some(UnixNanos::from(120)),
            Some(UnixNanos::from(140)),
            None,
            Some(UnixNanos::from(150))
        ));
    }

    #[rstest]
    fn test_edge_cases() {
        // Test with empty "changes" array
        let mut info = create_test_instrument(100, Some(200));
        info.changes = Some(vec![]);
        assert!(is_available(
            &info,
            None,
            None,
            None,
            Some(UnixNanos::from(150))
        ));

        // Test with very large timestamps (near u64::MAX)
        let far_future_info = create_test_instrument(100, None); // No end date = indefinite future
        let far_future_date = UnixNanos::from(u64::MAX - 1000);
        assert!(is_available(
            &far_future_info,
            None,
            None,
            None,
            Some(UnixNanos::from(101))
        ));
        assert!(is_available(
            &far_future_info,
            None,
            Some(far_future_date),
            None,
            None
        ));

        // Test with offset that increases available_since
        let info = create_test_instrument(100, Some(200));

        // Adding offset of 50 to available_since (100) makes it 150
        let offset = UnixNanos::from(50);
        assert!(!is_available(
            &info,
            None,
            None,
            Some(offset),
            Some(UnixNanos::from(149))
        ));
        assert!(is_available(
            &info,
            None,
            None,
            Some(offset),
            Some(UnixNanos::from(151))
        ));

        // Test with offset equal to zero (no effect)
        let zero_offset = UnixNanos::from(0);
        assert!(!is_available(
            &info,
            None,
            None,
            Some(zero_offset),
            Some(UnixNanos::from(100))
        ));
        assert!(is_available(
            &info,
            None,
            None,
            Some(zero_offset),
            Some(UnixNanos::from(101))
        ));
    }
}
