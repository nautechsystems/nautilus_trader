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

//! Exact value validation for Polymarket execution reports.

use anyhow::Context;
use nautilus_core::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};
use nautilus_model::{
    identifiers::{TradeId, VenueOrderId},
    instruments::InstrumentAny,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use crate::http::models::FeeSchedule;

pub(crate) fn decimal_from_str_exact(value: &str, field: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str_exact(value)
        .with_context(|| format!("{field} {value:?} is not an exact decimal"))
}

pub(crate) fn positive_quantity(
    value: Decimal,
    precision: u8,
    field: &str,
) -> anyhow::Result<Quantity> {
    anyhow::ensure!(
        value > Decimal::ZERO,
        "{field} must be positive, was {value}"
    );

    let quantity = Quantity::from_decimal_dp(value, precision)
        .with_context(|| format!("{field} {value} is not representable as Quantity"))?;
    anyhow::ensure!(
        quantity.as_decimal() == value,
        "{field} {value} is not exact at precision {precision}"
    );

    Ok(quantity)
}

pub(crate) fn non_negative_quantity(
    value: Decimal,
    precision: u8,
    field: &str,
) -> anyhow::Result<Quantity> {
    anyhow::ensure!(
        value >= Decimal::ZERO,
        "{field} must be non-negative, was {value}"
    );

    let quantity = Quantity::from_decimal_dp(value, precision)
        .with_context(|| format!("{field} {value} is not representable as Quantity"))?;
    anyhow::ensure!(
        quantity.as_decimal() == value,
        "{field} {value} is not exact at precision {precision}"
    );

    Ok(quantity)
}

/// Validates a provider price while preserving its exact reported scale.
///
/// Historical reports can retain a price from an earlier tick regime, so current instrument tick
/// precision is not an authority boundary for provider evidence.
pub(crate) fn exact_binary_price(value: Decimal, field: &str) -> anyhow::Result<Price> {
    anyhow::ensure!(
        value > Decimal::ZERO && value < Decimal::ONE,
        "{field} must satisfy 0 < price < 1, was {value}"
    );

    let normalized = value.normalize();
    let price = Price::from_decimal(normalized)
        .with_context(|| format!("{field} {value} is not representable as Price"))?;
    anyhow::ensure!(price.as_decimal() == value, "{field} {value} is not exact");

    Ok(price)
}

pub(crate) fn venue_order_id(value: &str, field: &str) -> anyhow::Result<VenueOrderId> {
    VenueOrderId::new_checked(value)
        .with_context(|| format!("{field} {value:?} is not a valid venue order ID"))
}

pub(crate) fn trade_id(value: &str, field: &str) -> anyhow::Result<TradeId> {
    TradeId::new_checked(value)
        .with_context(|| format!("{field} {value:?} is not a valid trade ID"))
}

pub(crate) fn positive_unix_seconds(value: u64, field: &str) -> anyhow::Result<UnixNanos> {
    anyhow::ensure!(value > 0, "{field} must be positive, was {value}");
    value
        .checked_mul(NANOSECONDS_IN_SECOND)
        .map(UnixNanos::from)
        .with_context(|| format!("{field} {value} overflows Unix nanoseconds"))
}

pub(crate) fn parse_match_time(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        let seconds = value
            .parse::<u64>()
            .with_context(|| format!("{field} {value:?} is not Unix seconds"))?;
        return positive_unix_seconds(seconds, field);
    }

    let timestamp = value
        .parse::<jiff::Timestamp>()
        .with_context(|| format!("{field} {value:?} is not a valid timestamp"))?;
    let nanos = u64::try_from(timestamp.as_nanosecond())
        .with_context(|| format!("{field} {value:?} is before the Unix epoch"))?;
    anyhow::ensure!(nanos > 0, "{field} must be positive");
    Ok(UnixNanos::from(nanos))
}

pub(crate) fn parse_user_channel_timestamp(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    anyhow::ensure!(
        value.bytes().all(|byte| byte.is_ascii_digit()),
        "{field} {value:?} must contain only ASCII digits"
    );
    let raw = value
        .parse::<u64>()
        .with_context(|| format!("{field} {value:?} is not an unsigned integer"))?;
    let multiplier = match value.len() {
        10 => NANOSECONDS_IN_SECOND,
        13 => NANOSECONDS_IN_MILLISECOND,
        length => anyhow::bail!("{field} {value:?} has unsupported digit length {length}"),
    };
    let nanos = raw
        .checked_mul(multiplier)
        .with_context(|| format!("{field} {value:?} overflows Unix nanoseconds"))?;
    anyhow::ensure!(nanos > 0, "{field} must be positive");
    Ok(UnixNanos::from(nanos))
}

pub(crate) fn parse_expiration(value: &str, field: &str) -> anyhow::Result<Option<UnixNanos>> {
    if value == "0" {
        return Ok(None);
    }

    let seconds = value
        .parse::<u64>()
        .with_context(|| format!("{field} {value:?} is not Unix seconds"))?;
    positive_unix_seconds(seconds, field).map(Some)
}

pub(crate) fn validate_fee_schedule(
    schedule: &FeeSchedule,
    context: &str,
) -> anyhow::Result<(Decimal, f64)> {
    anyhow::ensure!(
        (Decimal::ZERO..=Decimal::ONE).contains(&schedule.rate),
        "{context} fee rate must be in [0, 1], was {}",
        schedule.rate
    );
    anyhow::ensure!(
        (Decimal::ZERO..=Decimal::ONE).contains(&schedule.rebate_rate),
        "{context} rebate rate must be in [0, 1], was {}",
        schedule.rebate_rate
    );
    anyhow::ensure!(
        schedule.taker_only,
        "{context} requires a taker-only fee schedule"
    );
    anyhow::ensure!(
        schedule.exponent > Decimal::ZERO,
        "{context} fee exponent must be positive, was {}",
        schedule.exponent
    );
    let exponent = f64::try_from(schedule.exponent)
        .with_context(|| format!("{context} fee exponent is not representable as f64"))?;
    anyhow::ensure!(
        exponent.is_finite() && exponent > 0.0,
        "{context} fee exponent must be positive and finite, was {exponent}"
    );
    Ok((schedule.rate, exponent))
}

pub(crate) fn instrument_fee_policy(instrument: &InstrumentAny) -> anyhow::Result<(Decimal, f64)> {
    let InstrumentAny::BinaryOption(binary) = instrument else {
        anyhow::bail!("Polymarket fee policy requires a BinaryOption instrument");
    };
    let info = binary
        .info
        .as_ref()
        .context("Polymarket instrument is missing fee metadata")?;
    let enabled = info.get_bool("fees_enabled");
    let schedule = info
        .get("fee_schedule")
        .map(|value| serde_json::from_value::<FeeSchedule>(value.clone()))
        .transpose()
        .context("invalid Polymarket fee schedule")?;

    match (enabled, schedule.as_ref()) {
        (Some(false), None) => {
            anyhow::ensure!(
                binary.maker_fee.is_zero() && binary.taker_fee.is_zero(),
                "disabled Polymarket fees disagree with instrument fee fields"
            );
            Ok((Decimal::ZERO, 1.0))
        }
        (Some(true), Some(schedule)) => {
            let (rate, exponent) = validate_fee_schedule(schedule, "Polymarket instrument")?;
            anyhow::ensure!(
                binary.maker_fee.is_zero() && binary.taker_fee == rate,
                "Polymarket fee schedule disagrees with instrument fee fields"
            );
            Ok((rate, exponent))
        }
        _ => anyhow::bail!("inconsistent Polymarket instrument fee metadata"),
    }
}

pub(crate) fn ensure_instrument_binding(
    instrument: &InstrumentAny,
    provider_condition: &str,
    provider_token: &str,
    provider_outcome: Option<&str>,
    context: &str,
) -> anyhow::Result<()> {
    let InstrumentAny::BinaryOption(binary) = instrument else {
        anyhow::bail!("{context} requires a BinaryOption instrument");
    };
    let condition = binary
        .info
        .as_ref()
        .and_then(|info| info.get_str("condition_id"))
        .with_context(|| format!("{context} instrument is missing condition_id metadata"))?;

    ensure_condition_id(condition, "instrument condition_id")?;
    ensure_condition_id(provider_condition, "provider condition_id")?;
    anyhow::ensure!(
        condition.eq_ignore_ascii_case(provider_condition),
        "{context} condition {provider_condition:?} does not match instrument condition {condition:?}"
    );
    anyhow::ensure!(
        binary.raw_symbol.as_str() == provider_token,
        "{context} token {provider_token:?} does not match instrument token {:?}",
        binary.raw_symbol.as_str()
    );

    if let Some(provider_outcome) = provider_outcome {
        let outcome = binary
            .outcome
            .with_context(|| format!("{context} instrument is missing outcome metadata"))?;
        anyhow::ensure!(
            outcome.as_str().eq_ignore_ascii_case(provider_outcome),
            "{context} outcome {provider_outcome:?} does not match instrument outcome {:?}",
            outcome.as_str()
        );
    }

    Ok(())
}

fn ensure_condition_id(value: &str, field: &str) -> anyhow::Result<()> {
    let (prefix, hex) = value
        .split_at_checked(2)
        .with_context(|| format!("{field} {value:?} must start with a 0x prefix"))?;
    anyhow::ensure!(
        prefix.eq_ignore_ascii_case("0x"),
        "{field} {value:?} must start with a 0x prefix"
    );
    anyhow::ensure!(
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{field} {value:?} must contain exactly 32 hexadecimal bytes"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::instruments::InstrumentAny;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::http::{
        models::{FeeSchedule, GammaMarket},
        parse::{create_instrument_from_def, parse_gamma_market},
    };

    const CONDITION_ID: &str = "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b";
    const TOKEN_ID: &str =
        "104239898038807136052399800151408521467737075933964991162589336683346093173875";

    fn test_instrument() -> InstrumentAny {
        let mut market: GammaMarket =
            serde_json::from_str(include_str!("../../test_data/gamma_market.json")).unwrap();
        market.fees_enabled = Some(false);
        market.fee_schedule = None;
        let definition = parse_gamma_market(&market).unwrap().remove(0);
        create_instrument_from_def(&definition, UnixNanos::default()).unwrap()
    }

    #[rstest]
    fn test_decimal_from_str_exact_rejects_rounding() {
        assert!(decimal_from_str_exact("111111111111111.111111111111115", "value").is_err());
    }

    #[rstest]
    fn test_exact_binary_price_preserves_provider_scale() {
        let value = Decimal::from_str_exact("0.1234").unwrap();

        assert_eq!(
            exact_binary_price(value, "price").unwrap().as_decimal(),
            value
        );
    }

    #[rstest]
    fn test_exact_binary_price_rejects_invalid_or_unrepresentable_values() {
        let over_precision = Decimal::from_str_exact("0.12345678901234567").unwrap();

        assert!(exact_binary_price(Decimal::ZERO, "price").is_err());
        assert!(exact_binary_price(Decimal::ONE, "price").is_err());
        assert!(exact_binary_price(over_precision, "price").is_err());
    }

    #[rstest]
    fn test_quantities_reject_invalid_or_inexact_values() {
        let negative = Decimal::NEGATIVE_ONE;
        let inexact = Decimal::from_str_exact("0.001").unwrap();

        assert!(positive_quantity(Decimal::ZERO, 2, "quantity").is_err());
        assert!(positive_quantity(negative, 2, "quantity").is_err());
        assert!(positive_quantity(inexact, 2, "quantity").is_err());
        assert!(non_negative_quantity(negative, 2, "quantity").is_err());
        assert!(non_negative_quantity(inexact, 2, "quantity").is_err());
        assert_eq!(
            non_negative_quantity(Decimal::ZERO, 2, "quantity")
                .unwrap()
                .as_decimal(),
            Decimal::ZERO
        );
    }

    #[rstest]
    fn test_checked_identifiers_reject_invalid_values() {
        let oversized = "X".repeat(100);

        assert!(venue_order_id("", "order ID").is_err());
        assert!(venue_order_id("non-ascii-☃", "order ID").is_err());
        assert!(trade_id("", "trade ID").is_err());
        assert!(trade_id(&oversized, "trade ID").is_err());
    }

    #[rstest]
    fn test_positive_unix_seconds_rejects_zero_and_overflow() {
        assert!(positive_unix_seconds(0, "created_at").is_err());
        assert!(positive_unix_seconds(18_446_744_074, "created_at").is_err());
    }

    #[rstest]
    #[case("1700000000", UnixNanos::from(1_700_000_000_000_000_000_u64))]
    #[case("2023-11-14T22:13:20Z", UnixNanos::from(1_700_000_000_000_000_000_u64))]
    fn test_parse_match_time_uses_lexical_grammar(#[case] raw: &str, #[case] expected: UnixNanos) {
        assert_eq!(parse_match_time(raw, "match_time").unwrap(), expected);
    }

    #[rstest]
    #[case("1672290687", 1_672_290_687_000_000_000_u64)]
    #[case("1672290687000", 1_672_290_687_000_000_000_u64)]
    fn test_user_channel_timestamp_accepts_documented_forms(
        #[case] raw: &str,
        #[case] expected: u64,
    ) {
        assert_eq!(
            parse_user_channel_timestamp(raw, "timestamp")
                .unwrap()
                .as_u64(),
            expected
        );
    }

    #[rstest]
    #[case("167229068")]
    #[case("16722906870")]
    #[case("167229068700")]
    #[case("16722906870000")]
    #[case("2023-11-14T22:13:20Z")]
    fn test_user_channel_timestamp_rejects_other_grammars(#[case] raw: &str) {
        assert!(parse_user_channel_timestamp(raw, "timestamp").is_err());
    }

    #[rstest]
    fn test_parse_expiration_preserves_sentinel_and_positive_seconds() {
        assert_eq!(parse_expiration("0", "expiration").unwrap(), None);
        assert_eq!(
            parse_expiration("1700000000", "expiration").unwrap(),
            Some(UnixNanos::from(1_700_000_000_000_000_000_u64))
        );
    }

    #[rstest]
    #[case("")]
    #[case("00")]
    #[case("-1")]
    #[case("1.5")]
    #[case("18446744074")]
    fn test_parse_expiration_rejects_malformed_or_overflow_values(#[case] raw: &str) {
        assert!(parse_expiration(raw, "expiration").is_err());
    }

    #[rstest]
    fn test_instrument_binding_accepts_equivalent_condition_case() {
        let instrument = test_instrument();
        let upper_condition = CONDITION_ID.to_ascii_uppercase();

        assert!(
            ensure_instrument_binding(
                &instrument,
                &upper_condition,
                TOKEN_ID,
                Some("up"),
                "trade",
            )
            .is_ok()
        );
    }

    #[rstest]
    #[case(
        "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63a",
        TOKEN_ID,
        Some("Up")
    )]
    #[case(
        CONDITION_ID,
        "71183960810705820955071415844881728181970340514894896943812046065452395013351",
        Some("Up")
    )]
    #[case(CONDITION_ID, TOKEN_ID, Some("Down"))]
    fn test_instrument_binding_rejects_contradictory_provider_evidence(
        #[case] condition: &str,
        #[case] token: &str,
        #[case] outcome: Option<&str>,
    ) {
        let instrument = test_instrument();

        assert!(
            ensure_instrument_binding(&instrument, condition, token, outcome, "trade").is_err()
        );
    }

    #[rstest]
    #[case(Decimal::ZERO, dec!(0.03), dec!(0.25), true)]
    #[case(dec!(-1), dec!(0.03), dec!(0.25), true)]
    #[case(Decimal::ONE, dec!(-0.01), dec!(0.25), true)]
    #[case(Decimal::ONE, dec!(1.01), dec!(0.25), true)]
    #[case(Decimal::ONE, dec!(0.03), dec!(-0.01), true)]
    #[case(Decimal::ONE, dec!(0.03), dec!(1.01), true)]
    #[case(Decimal::ONE, dec!(0.03), dec!(0.25), false)]
    fn test_commission_policy_rejects_invalid_fee_schedules(
        #[case] exponent: Decimal,
        #[case] rate: Decimal,
        #[case] rebate_rate: Decimal,
        #[case] taker_only: bool,
    ) {
        let schedule = FeeSchedule {
            exponent,
            rate,
            taker_only,
            rebate_rate,
        };

        assert!(validate_fee_schedule(&schedule, "test schedule").is_err());
    }
}
