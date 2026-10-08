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

//! Computes candidate option identifiers under an explicit symbology scheme.
//!
//! CME inference follows nominal weekday expiry schedules. Exchange holidays and instrument
//! listings require definition validation by the caller. Standard quarterly equity options
//! take precedence when the futures month matches the quarterly expiry month; otherwise
//! weekly or EOM roots apply.

use std::str::FromStr;

use jiff::{civil::Date, tz::TimeZone};
use nautilus_core::UnixNanos;
use thiserror::Error;

use crate::{
    enums::OptionKind,
    identifiers::{InstrumentId, Symbol, Venue},
    types::{
        Price,
        fixed::{check_fixed_precision, raw_scale},
    },
};

const MONTH_CODES: &[u8; 12] = b"FGHJKMNQUVXZ";

/// The contract inputs used to compute a candidate option identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionContractSpec {
    /// The option's actual underlying, independent of its series reference-price instrument.
    pub underlying_instrument_id: InstrumentId,
    /// The exact strike price.
    pub strike: Price,
    /// UNIX expiration timestamp in nanoseconds.
    pub expiration_ns: UnixNanos,
    /// The option kind.
    pub option_kind: OptionKind,
}

/// A scheme for computing raw option symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionSymbologyScheme {
    /// CME Globex symbols, with an optional SOFR midcurve tenor code.
    CmeGlobex {
        /// `0` denotes one year; `2`, `3`, `4`, and `5` denote those year tenors.
        mid_curve: Option<u8>,
    },
    /// Padded 21-character OSI symbols.
    Osi {
        /// An option-root override, such as `SPXW`; otherwise uses the underlying symbol.
        root: Option<String>,
    },
}

/// A contract component cannot be represented by the selected scheme.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum OptionInstrumentIdError {
    /// The underlying futures code is invalid.
    #[error("invalid future code '{value}': {reason}")]
    InvalidFutureCode {
        /// The invalid code.
        value: String,
        /// The validation failure.
        reason: String,
    },
    /// The expiry is unsupported or ambiguous.
    #[error("invalid expiration '{value}': {reason}")]
    InvalidExpiration {
        /// The invalid expiry.
        value: String,
        /// The validation failure.
        reason: String,
    },
    /// The strike cannot be represented exactly.
    #[error("invalid strike '{value}': {reason}")]
    InvalidStrike {
        /// The invalid strike.
        value: String,
        /// The validation failure.
        reason: String,
    },
    /// The option root is invalid or unsupported.
    #[error("invalid option root '{value}': {reason}")]
    InvalidRoot {
        /// The invalid root.
        value: String,
        /// The validation failure.
        reason: String,
    },
}

impl OptionContractSpec {
    /// Creates a contract specification.
    #[must_use]
    pub const fn new(
        underlying_instrument_id: InstrumentId,
        strike: Price,
        expiration_ns: UnixNanos,
        option_kind: OptionKind,
    ) -> Self {
        Self {
            underlying_instrument_id,
            strike,
            expiration_ns,
            option_kind,
        }
    }

    /// Creates a specification from a UTC expiry date or timestamp string.
    ///
    /// # Errors
    ///
    /// Returns an error if the expiry cannot be parsed.
    pub fn from_expiry(
        underlying_instrument_id: InstrumentId,
        strike: Price,
        expiry: &str,
        option_kind: OptionKind,
    ) -> Result<Self, OptionInstrumentIdError> {
        let expiration_ns = UnixNanos::from_str(expiry).map_err(|e| {
            OptionInstrumentIdError::InvalidExpiration {
                value: expiry.to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(Self::new(
            underlying_instrument_id,
            strike,
            expiration_ns,
            option_kind,
        ))
    }

    /// Returns the UTC calendar date of expiration.
    #[must_use]
    pub fn expiry_date(&self) -> Date {
        TimeZone::UTC
            .to_datetime(self.expiration_ns.to_datetime_utc())
            .date()
    }
}

/// Computes a candidate option ID at the option venue.
///
/// The computed symbol does not prove that a matching contract is listed.
/// CME reads the underlying symbol as a futures code such as `ESH6`, rather than `ES`.
/// OSI uses that symbol as the option root unless the scheme supplies an override.
///
/// # Errors
///
/// Returns a typed error if a contract component is invalid, unsupported, or ambiguous.
pub fn compute_option_instrument_id(
    spec: &OptionContractSpec,
    scheme: &OptionSymbologyScheme,
    venue: Venue,
) -> Result<InstrumentId, OptionInstrumentIdError> {
    let symbol = match scheme {
        OptionSymbologyScheme::CmeGlobex { mid_curve } => cme_globex_option_symbol(
            spec.underlying_instrument_id.symbol.as_str(),
            spec.strike,
            spec.option_kind,
            spec.expiry_date(),
            *mid_curve,
        )?,
        OptionSymbologyScheme::Osi { root } => osi_option_symbol(
            root.as_deref()
                .unwrap_or(spec.underlying_instrument_id.symbol.as_str()),
            spec.expiry_date(),
            spec.option_kind,
            spec.strike,
        )?,
    };

    let checked =
        Symbol::new_checked(&symbol).map_err(|e| OptionInstrumentIdError::InvalidRoot {
            value: symbol,
            reason: e.to_string(),
        })?;

    Ok(InstrumentId::new(checked, venue))
}

/// Computes a CME Globex option symbol from a nominal UTC expiry date.
///
/// Uses the expiry year with the underlying's one- or two-digit year convention.
/// Supports ES, NQ, RTY, MES, MNQ, SR3 SOFR contracts, and CL WTI weeklies.
/// Other roots return typed errors. Physically settled Micro E-mini contracts are supported
/// before June 29, 2026; subsequent physical and financial families are ambiguous.
/// SOFR and WTI strike digits use hundredths; equity-index strike digits use whole index points.
/// Holiday-adjusted expiries require validation against instrument definitions.
///
/// # Errors
///
/// Returns a typed error if:
/// - The futures code, root, or midcurve tenor is unsupported.
/// - The expiry is on a weekend, a nominal WTI monthly date, or an ambiguous micro date.
/// - The strike is non-positive or cannot fit the supported integral symbol scale.
pub fn cme_globex_option_symbol(
    future_code: &str,
    strike: Price,
    option_kind: OptionKind,
    expiry: Date,
    mid_curve: Option<u8>,
) -> Result<String, OptionInstrumentIdError> {
    let (root, future_month, future_year) = parse_futures_symbol(future_code)?;
    let day = expiry.weekday().to_monday_one_offset();
    if day > 5 {
        return Err(invalid_expiry(
            expiry,
            "CME nominal expiry must be a weekday",
        ));
    }

    if mid_curve.is_some() && root != "SR3" {
        return Err(OptionInstrumentIdError::InvalidRoot {
            value: root.to_string(),
            reason: "midcurve tenors apply only to SR3".to_string(),
        });
    }

    let week = (expiry.day() - 1) / 7 + 1;
    let month = expiry.month();

    let option_root = match root {
        "ES" | "NQ" | "RTY" | "MES" | "MNQ" => equity_root(root, future_month, expiry, day, week)?,
        "SR3" => sofr_root(expiry, mid_curve, day, week)?,
        "CL" => crude_root(expiry, day, week)?,
        _ => {
            return Err(OptionInstrumentIdError::InvalidRoot {
                value: root.to_string(),
                reason: "unsupported CME option root".to_string(),
            });
        }
    };

    let month_code = char::from(MONTH_CODES[(month - 1) as usize]);
    let side = option_letter(option_kind);
    let year_width = future_year.len();
    let year_modulus = if year_width == 1 { 10 } else { 100 };
    let year = expiry.year().rem_euclid(year_modulus);
    let strike = cme_strike(root, strike)?;
    Ok(format!(
        "{option_root}{month_code}{year:0year_width$} {side}{strike}"
    ))
}

/// Computes a padded OSI option symbol with an exact thousandths strike.
///
/// # Errors
///
/// Returns a typed error if:
/// - The root is outside one to six ASCII alphanumeric characters.
/// - The expiry year is outside `[2000, 2099]`.
/// - The strike cannot fit eight digits of exact non-negative thousandths.
pub fn osi_option_symbol(
    root: &str,
    expiry: Date,
    option_kind: OptionKind,
    strike: Price,
) -> Result<String, OptionInstrumentIdError> {
    if root.is_empty() || root.len() > 6 || !root.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return Err(OptionInstrumentIdError::InvalidRoot {
            value: root.to_string(),
            reason: "OSI root must be one to six ASCII alphanumeric characters".to_string(),
        });
    }

    if !(2000..=2099).contains(&expiry.year()) {
        return Err(invalid_expiry(
            expiry,
            "OSI expiry year must be in [2000, 2099]",
        ));
    }

    let reason = "OSI strike must fit eight digits of exact non-negative thousandths";
    let scaled = strike_digits(strike, 3, reason)?;
    if scaled > 99_999_999 {
        return Err(OptionInstrumentIdError::InvalidStrike {
            value: strike.to_string(),
            reason: reason.to_string(),
        });
    }

    let year = expiry.year() % 100;
    let month = expiry.month();
    let day = expiry.day();
    let side = option_letter(option_kind);
    Ok(format!(
        "{root:<6}{year:02}{month:02}{day:02}{side}{scaled:08}"
    ))
}

fn equity_root(
    root: &str,
    future_month: u32,
    expiry: Date,
    day: i8,
    week: i8,
) -> Result<String, OptionInstrumentIdError> {
    if matches!(root, "MES" | "MNQ")
        && expiry >= Date::new(2026, 6, 29).expect("valid transition date")
    {
        return Err(invalid_expiry(
            expiry,
            "physical and financial micro option families are ambiguous",
        ));
    }

    let (daily, friday, eom, quarterly) = match root {
        "ES" => ("E", "EW", "EW", "ES"),
        "NQ" => ("Q", "QN", "QNE", "NQ"),
        "RTY" => ("R", "R", "RTM", "RTO"),
        "MES" => ("X", "EX", "EX", "MES"),
        "MNQ" => ("D", "MQ", "MQE", "MNQ"),
        _ => unreachable!("equity root dispatched by caller"),
    };

    if business_days_remaining(expiry) == 0 {
        return Ok(eom.to_string());
    }

    if day == 5 && week == 3 && expiry.month() % 3 == 0 && future_month == expiry.month() as u32 {
        return Ok(quarterly.to_string());
    }

    if day == 5 {
        let suffix = if root == "RTY" { "E" } else { "" };
        return Ok(format!("{friday}{week}{suffix}"));
    }

    let day_code = match (root, day) {
        ("RTY", 2) => 'U',
        (_, 1) => 'A',
        (_, 2) => 'B',
        (_, 3) => 'C',
        _ => 'D',
    };

    Ok(format!("{daily}{week}{day_code}"))
}

fn sofr_root(
    expiry: Date,
    mid_curve: Option<u8>,
    day: i8,
    week: i8,
) -> Result<String, OptionInstrumentIdError> {
    let day_fifteen = Date::new(expiry.year(), expiry.month(), 15).expect("valid fifteenth day");
    let third_wednesday = 15 + (3 - day_fifteen.weekday().to_monday_one_offset()).rem_euclid(7);
    let monthly = day == 5 && expiry.day() == third_wednesday - 5;

    let base = match mid_curve {
        None => "SR3".to_string(),
        Some(tenor @ (0 | 2 | 3 | 4 | 5)) => format!("S{tenor}"),
        _ => {
            return Err(OptionInstrumentIdError::InvalidRoot {
                value: format!("SR3 midcurve {mid_curve:?}"),
                reason: "unsupported SOFR midcurve tenor".to_string(),
            });
        }
    };

    if monthly {
        return Ok(base);
    }

    if day != 5 || !matches!(mid_curve, Some(0 | 2 | 3)) {
        return Err(invalid_expiry(
            expiry,
            "unsupported SOFR weekly expiry or tenor",
        ));
    }

    Ok(format!("{base}{week}"))
}

fn crude_root(expiry: Date, day: i8, week: i8) -> Result<String, OptionInstrumentIdError> {
    let anchor = Date::new(expiry.year(), expiry.month(), 25).expect("valid twenty-fifth day");
    let monthly = previous_weekdays(previous_weekdays(anchor, 0)?, 6)?;
    if expiry == monthly {
        return Err(invalid_expiry(
            expiry,
            "monthly and weekly WTI expiries require a verified definition",
        ));
    }

    let prefix = match day {
        1 => "ML",
        2 => "NL",
        3 => "WL",
        4 => "XL",
        _ => "LO",
    };

    Ok(format!("{prefix}{week}"))
}

fn cme_strike(root: &str, strike: Price) -> Result<u128, OptionInstrumentIdError> {
    let precision = match root {
        "SR3" | "CL" => 2,
        _ => 0,
    };

    let scaled = strike_digits(
        strike,
        precision,
        "strike cannot be represented as integral CME symbol digits",
    )?;

    if scaled == 0 {
        return Err(OptionInstrumentIdError::InvalidStrike {
            value: strike.to_string(),
            reason: "strike must be positive".to_string(),
        });
    }

    Ok(scaled)
}

fn strike_digits(
    strike: Price,
    precision: u8,
    reason: &str,
) -> Result<u128, OptionInstrumentIdError> {
    if strike.is_undefined()
        || strike.is_error()
        || check_fixed_precision(strike.precision).is_err()
    {
        return Err(OptionInstrumentIdError::InvalidStrike {
            value: format!("raw={}, precision={}", strike.raw(), strike.precision),
            reason: "strike must be a numeric price with valid precision".to_string(),
        });
    }

    let divisor = raw_scale(strike.precision) / 10_u128.pow(u32::from(precision));
    #[allow(
        clippy::useless_conversion,
        reason = "raw magnitude is u64 in standard precision and u128 in high precision"
    )]
    let raw = u128::from(strike.raw().unsigned_abs());
    if strike.is_negative() || raw % divisor != 0 {
        return Err(OptionInstrumentIdError::InvalidStrike {
            value: strike.to_string(),
            reason: reason.to_string(),
        });
    }

    Ok(raw / divisor)
}

fn previous_weekdays(mut date: Date, mut count: u8) -> Result<Date, OptionInstrumentIdError> {
    while date.weekday().to_monday_one_offset() > 5 || count > 0 {
        date = date
            .yesterday()
            .map_err(|e| invalid_expiry(date, &e.to_string()))?;

        if date.weekday().to_monday_one_offset() <= 5 && count > 0 {
            count -= 1;
        }
    }

    Ok(date)
}

fn business_days_remaining(expiry: Date) -> u8 {
    let mut days = 0;
    let mut date = expiry;
    while let Ok(next) = date.tomorrow() {
        if next.month() != expiry.month() {
            break;
        }

        if next.weekday().to_monday_one_offset() <= 5 {
            days += 1;
        }

        date = next;
    }

    days
}

fn option_letter(kind: OptionKind) -> char {
    match kind {
        OptionKind::Call => 'C',
        OptionKind::Put => 'P',
    }
}

fn invalid_expiry(expiry: Date, reason: &str) -> OptionInstrumentIdError {
    OptionInstrumentIdError::InvalidExpiration {
        value: expiry.to_string(),
        reason: reason.to_string(),
    }
}

fn parse_futures_symbol(code: &str) -> Result<(&str, u32, &str), OptionInstrumentIdError> {
    let invalid = |reason: &str| OptionInstrumentIdError::InvalidFutureCode {
        value: code.to_string(),
        reason: reason.to_string(),
    };

    if !code
        .bytes()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(invalid("expected uppercase ASCII letters and digits"));
    }

    let year_start = code
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |index| index + 1);
    let head = &code[..year_start];
    let year = &code[year_start..];

    if !matches!(year.len(), 1 | 2) {
        return Err(invalid("year must contain one or two digits"));
    }

    let Some(month_code) = head.as_bytes().last() else {
        return Err(invalid("missing month code"));
    };

    let month = MONTH_CODES
        .iter()
        .position(|c| c == month_code)
        .ok_or_else(|| invalid("invalid month code"))?;
    let root = &head[..head.len() - 1];
    if root.is_empty() {
        return Err(invalid("missing future root"));
    }

    Ok((root, month as u32 + 1, year))
}

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;
    use crate::types::{
        ERROR_PRICE, PRICE_ERROR, PRICE_UNDEF, fixed::FIXED_PRECISION, price::PriceRaw,
    };

    #[rstest]
    #[case(0)]
    #[case(FIXED_PRECISION)]
    fn test_option_builders_reject_raw_strike_remainders(#[case] precision: u8) {
        let scale = PriceRaw::try_from(crate::types::fixed::raw_scale(precision)).unwrap();
        let strike = Price::from_raw(150 * scale + 1, precision);
        let cme =
            cme_globex_option_symbol("ESH6", strike, OptionKind::Call, date(2026, 1, 9), None);
        let osi = osi_option_symbol("AAPL", date(2026, 1, 9), OptionKind::Call, strike);

        assert!(matches!(
            cme,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
        assert!(matches!(
            osi,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
    }

    #[rstest]
    #[case(0)]
    #[case(FIXED_PRECISION)]
    fn test_option_builders_preserve_raw_strike(#[case] precision: u8) {
        let scale = PriceRaw::try_from(crate::types::fixed::raw_scale(precision)).unwrap();
        let strike = Price::from_raw(75 * scale + scale / 4, precision);
        let cme =
            cme_globex_option_symbol("CLH6", strike, OptionKind::Call, date(2026, 1, 9), None)
                .unwrap();
        let osi = osi_option_symbol("AAPL", date(2026, 1, 9), OptionKind::Call, strike).unwrap();

        assert_eq!(cme, "LO2F6 C7525");
        assert_eq!(osi, "AAPL  260109C00075250");
    }

    #[rstest]
    #[case(ERROR_PRICE)]
    #[case(Price::from_raw(PRICE_ERROR, 0))]
    #[case(Price::from_raw(PRICE_UNDEF, 0))]
    fn test_option_builders_reject_strike_sentinels(#[case] strike: Price) {
        let cme =
            cme_globex_option_symbol("ESH6", strike, OptionKind::Call, date(2026, 1, 9), None);
        let osi = osi_option_symbol("AAPL", date(2026, 1, 9), OptionKind::Call, strike);

        assert!(matches!(
            cme,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
        assert!(matches!(
            osi,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    fn test_option_builders_reject_strike_beyond_decimal_mantissa() {
        let scale = PriceRaw::try_from(crate::types::fixed::raw_scale(FIXED_PRECISION)).unwrap();
        let strike = Price::from_raw(10_000_000_000_000 * scale + 1, FIXED_PRECISION);
        let cme =
            cme_globex_option_symbol("ESH6", strike, OptionKind::Call, date(2026, 1, 9), None);
        let osi = osi_option_symbol("AAPL", date(2026, 1, 9), OptionKind::Call, strike);

        assert!(matches!(
            cme,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
        assert!(matches!(
            osi,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
    }

    #[rstest]
    #[case("ESH6", "7250", "2026-01-27", OptionKind::Call, "E4BF6 C7250")]
    #[case("ESH6", "7100", "2026-01-08", OptionKind::Put, "E2DF6 P7100")]
    #[case("ESH26", "7000", "2026-01-09", OptionKind::Call, "EW2F26 C7000")]
    #[case("ESH6", "5300", "2026-01-16", OptionKind::Call, "EW3F6 C5300")]
    #[case("ESH6", "7250", "2026-01-30", OptionKind::Call, "EWF6 C7250")]
    #[case("ESH6", "7250", "2026-01-30", OptionKind::Put, "EWF6 P7250")]
    #[case("ESH6", "7000", "2026-03-20", OptionKind::Put, "ESH6 P7000")]
    #[case("ESU6", "9600", "2026-09-18", OptionKind::Call, "ESU6 C9600")]
    #[case("ESU6", "6575", "2026-09-18", OptionKind::Put, "ESU6 P6575")]
    #[case("ESM4", "3550", "2024-06-21", OptionKind::Call, "ESM4 C3550")]
    #[case("SR3Z3", "95.75", "2023-12-15", OptionKind::Put, "SR3Z3 P9575")]
    #[case("ESH6", "6000.00", "2025-12-31", OptionKind::Call, "EWZ5 C6000")]
    #[case("NQH6", "18000", "2026-01-13", OptionKind::Put, "Q2BF6 P18000")]
    #[case("NQH6", "18000", "2026-01-09", OptionKind::Call, "QN2F6 C18000")]
    #[case("NQH6", "18000", "2026-01-30", OptionKind::Put, "QNEF6 P18000")]
    #[case("RTYH6", "2500", "2026-01-13", OptionKind::Call, "R2UF6 C2500")]
    #[case("RTYH6", "2500", "2026-01-09", OptionKind::Put, "R2EF6 P2500")]
    #[case("RTYH6", "2500", "2026-01-30", OptionKind::Call, "RTMF6 C2500")]
    #[case("RTYH6", "2500", "2026-03-20", OptionKind::Call, "RTOH6 C2500")]
    #[case("MESH6", "5300", "2026-01-13", OptionKind::Call, "X2BF6 C5300")]
    #[case("MNQH6", "18000", "2026-01-08", OptionKind::Put, "D2DF6 P18000")]
    #[case("CLZ5", "75", "2025-11-14", OptionKind::Put, "LO2X5 P7500")]
    #[case("CLZ5", "75", "2025-11-10", OptionKind::Call, "ML2X5 C7500")]
    #[case("CLZ5", "75", "2025-11-11", OptionKind::Put, "NL2X5 P7500")]
    #[case("CLZ5", "75", "2025-11-12", OptionKind::Call, "WL2X5 C7500")]
    #[case("CLZ5", "75", "2025-11-13", OptionKind::Put, "XL2X5 P7500")]
    fn test_compute_cme_fixtures(
        #[case] underlying: &str,
        #[case] strike: &str,
        #[case] expiry: &str,
        #[case] kind: OptionKind,
        #[case] expected: &str,
    ) {
        let underlying_id = InstrumentId::from_str(&format!("{underlying}.XCME")).unwrap();
        let spec =
            OptionContractSpec::from_expiry(underlying_id, Price::from(strike), expiry, kind)
                .unwrap();
        let id = compute_option_instrument_id(
            &spec,
            &OptionSymbologyScheme::CmeGlobex { mid_curve: None },
            Venue::from("GLBX"),
        )
        .unwrap();
        assert_eq!(id.symbol.as_str(), expected);
        assert_eq!(id.venue, Venue::from("GLBX"));
        assert_eq!(spec.underlying_instrument_id, underlying_id);
        assert_eq!(spec.strike, Price::from(strike));
        assert_eq!(spec.option_kind, kind);
        assert_eq!(spec.expiration_ns, UnixNanos::from_str(expiry).unwrap());
    }

    #[rstest]
    #[case("2026-03-13", Some(0), "S0H6 C9800")]
    #[case("2026-03-20", Some(0), "S03H6 C9800")]
    #[case("2026-03-13", Some(2), "S2H6 C9800")]
    #[case("2026-03-13", Some(4), "S4H6 C9800")]
    #[case("2026-03-13", None, "SR3H6 C9800")]
    fn test_sofr_midcurve_fixtures(
        #[case] expiry: &str,
        #[case] tenor: Option<u8>,
        #[case] expected: &str,
    ) {
        let spec = OptionContractSpec::from_expiry(
            InstrumentId::from("SR3H7.XCME"),
            Price::from("98"),
            expiry,
            OptionKind::Call,
        )
        .unwrap();
        let id = compute_option_instrument_id(
            &spec,
            &OptionSymbologyScheme::CmeGlobex { mid_curve: tenor },
            Venue::from("XCME"),
        )
        .unwrap();
        assert_eq!(id.to_string(), format!("{expected}.XCME"));
    }

    #[rstest]
    #[case("esh6")]
    #[case("H6")]
    #[case("ES6")]
    #[case("ESH")]
    #[case("ES H6")]
    #[case("")]
    #[case("123")]
    #[case("ESH12345")]
    #[case("ESH123")]
    #[case("ESH2026")]
    #[case("ÉSH6")]
    fn test_invalid_future_code(#[case] code: &str) {
        let error = cme_globex_option_symbol(
            code,
            Price::from("100"),
            OptionKind::Call,
            date(2026, 1, 9),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(error, OptionInstrumentIdError::InvalidFutureCode { value, .. } if value == code)
        );
    }

    #[rstest]
    #[case("6EH6", None)]
    #[case("ZNH6", None)]
    #[case("ZFH6", None)]
    #[case("ZTH6", None)]
    #[case("ZBH6", None)]
    #[case("TNH6", None)]
    #[case("UBH6", None)]
    #[case("GCG6", None)]
    #[case("SIH6", None)]
    #[case("NGZ5", None)]
    #[case("ESH6", Some(0))]
    #[case("SR3H7", Some(1))]
    #[case("SR3H7", Some(6))]
    fn test_unsupported_cme_root(#[case] code: &str, #[case] tenor: Option<u8>) {
        let error = cme_globex_option_symbol(
            code,
            Price::from("100"),
            OptionKind::Call,
            date(2026, 1, 9),
            tenor,
        )
        .unwrap_err();
        assert!(matches!(error, OptionInstrumentIdError::InvalidRoot { .. }));
    }

    #[rstest]
    #[case("ESH6", date(2026, 1, 31), None)]
    #[case("MESH6", date(2026, 7, 2), None)]
    #[case("CLZ5", date(2025, 11, 17), None)]
    #[case("SR3H7", date(2026, 3, 20), None)]
    #[case("SR3H7", date(2026, 3, 20), Some(4))]
    fn test_unrepresentable_cme_expiry(
        #[case] code: &str,
        #[case] expiry: Date,
        #[case] tenor: Option<u8>,
    ) {
        let error =
            cme_globex_option_symbol(code, Price::from("100"), OptionKind::Call, expiry, tenor)
                .unwrap_err();
        assert!(
            matches!(error, OptionInstrumentIdError::InvalidExpiration { value, .. } if value == expiry.to_string())
        );
    }

    #[rstest]
    #[case("0")]
    #[case("-1")]
    fn test_cme_nonpositive_strike(#[case] strike: &str) {
        let error = cme_globex_option_symbol(
            "ESH6",
            Price::from(strike),
            OptionKind::Call,
            date(2026, 1, 9),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(error, OptionInstrumentIdError::InvalidStrike { value, .. } if value == strike)
        );
    }

    #[rstest]
    #[case("ESH6", "7000.5")]
    #[case("SR3H7", "95.751")]
    #[case("CLZ5", "75.001")]
    fn test_cme_unrepresentable_strike(#[case] code: &str, #[case] strike: &str) {
        let error = cme_globex_option_symbol(
            code,
            Price::from(strike),
            OptionKind::Call,
            date(2026, 1, 9),
            code.starts_with("SR3").then_some(0),
        )
        .unwrap_err();
        assert_eq!(
            error,
            OptionInstrumentIdError::InvalidStrike {
                value: strike.to_string(),
                reason: "strike cannot be represented as integral CME symbol digits".to_string(),
            }
        );
    }

    #[rstest]
    #[case(None, "AAPL  231215C00150000.OPRA")]
    #[case(Some("SPXW"), "SPXW  231215C00150000.OPRA")]
    fn test_compute_osi_root_and_venue(#[case] root: Option<&str>, #[case] expected: &str) {
        let spec = OptionContractSpec::from_expiry(
            InstrumentId::from("AAPL.XNAS"),
            Price::from("150"),
            "2023-12-15",
            OptionKind::Call,
        )
        .unwrap();

        let id = compute_option_instrument_id(
            &spec,
            &OptionSymbologyScheme::Osi {
                root: root.map(str::to_string),
            },
            Venue::from("OPRA"),
        )
        .unwrap();

        assert_eq!(id.to_string(), expected);
    }

    #[rstest]
    fn test_invalid_spec_expiry() {
        let error = OptionContractSpec::from_expiry(
            InstrumentId::from("ESH6.XCME"),
            Price::from("7250"),
            "not-a-date",
            OptionKind::Call,
        )
        .unwrap_err();
        assert!(
            matches!(error, OptionInstrumentIdError::InvalidExpiration { value, .. } if value == "not-a-date")
        );
    }

    #[rstest]
    #[case(
        "AAPL",
        date(2023, 12, 15),
        OptionKind::Call,
        "150",
        "AAPL  231215C00150000"
    )]
    #[case(
        "SPY",
        date(2024, 1, 19),
        OptionKind::Put,
        "340",
        "SPY   240119P00340000"
    )]
    #[case(
        "SPXW",
        date(2026, 1, 20),
        OptionKind::Put,
        "6835",
        "SPXW  260120P06835000"
    )]
    #[case(
        "F",
        date(2024, 6, 21),
        OptionKind::Call,
        "12.5",
        "F     240621C00012500"
    )]
    #[case(
        "BRKB",
        date(2024, 6, 21),
        OptionKind::Call,
        "412.375",
        "BRKB  240621C00412375"
    )]
    fn test_osi_option_symbol(
        #[case] root: &str,
        #[case] expiry: Date,
        #[case] kind: OptionKind,
        #[case] strike: &str,
        #[case] expected: &str,
    ) {
        let symbol = osi_option_symbol(root, expiry, kind, Price::from(strike)).unwrap();
        assert_eq!(symbol, expected);
        assert_eq!(symbol.len(), 21);
    }

    #[rstest]
    fn test_osi_rejects_long_root() {
        let result = osi_option_symbol(
            "TOOLONG",
            date(2024, 6, 21),
            OptionKind::Call,
            Price::from("100"),
        );
        assert!(matches!(
            result,
            Err(OptionInstrumentIdError::InvalidRoot { .. })
        ));
    }

    #[rstest]
    fn test_osi_rejects_non_ascii_root() {
        let result = osi_option_symbol(
            "SPÉ",
            date(2024, 6, 21),
            OptionKind::Call,
            Price::from("100"),
        );
        assert!(matches!(
            result,
            Err(OptionInstrumentIdError::InvalidRoot { .. })
        ));
    }

    #[rstest]
    fn test_osi_rejects_sub_thousandth_strike() {
        let result = osi_option_symbol(
            "AAPL",
            date(2024, 6, 21),
            OptionKind::Call,
            Price::from("150.1234"),
        );
        assert!(matches!(
            result,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
    }

    #[rstest]
    fn test_osi_rejects_strike_over_eight_digits() {
        let result = osi_option_symbol(
            "AAPL",
            date(2024, 6, 21),
            OptionKind::Call,
            Price::from("100000"),
        );
        assert!(matches!(
            result,
            Err(OptionInstrumentIdError::InvalidStrike { .. })
        ));
    }

    #[rstest]
    fn test_osi_rejects_pre_2000_expiration() {
        let result = osi_option_symbol(
            "AAPL",
            date(1999, 12, 17),
            OptionKind::Call,
            Price::from("100"),
        );
        assert!(matches!(
            result,
            Err(OptionInstrumentIdError::InvalidExpiration { .. })
        ));
    }

    #[rstest]
    #[case("0", "AAPL  991231C00000000")]
    #[case("99999.999", "AAPL  991231C99999999")]
    fn test_osi_strike_boundaries(#[case] strike: &str, #[case] expected: &str) {
        let symbol = osi_option_symbol(
            "AAPL",
            date(2099, 12, 31),
            OptionKind::Call,
            Price::from(strike),
        )
        .unwrap();
        assert_eq!(symbol, expected);
    }

    #[rstest]
    fn test_osi_negative_strike_returns_typed_error() {
        let error = osi_option_symbol(
            "AAPL",
            date(2024, 6, 21),
            OptionKind::Put,
            Price::from("-1"),
        )
        .unwrap_err();
        assert_eq!(
            error,
            OptionInstrumentIdError::InvalidStrike {
                value: "-1".to_string(),
                reason: "OSI strike must fit eight digits of exact non-negative thousandths"
                    .to_string(),
            }
        );
    }

    fn date(year: i16, month: i8, day: i8) -> Date {
        Date::new(year, month, day).unwrap()
    }
}
