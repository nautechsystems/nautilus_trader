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

//! Dataset-specific decode configuration and option expiration correction.
//!
//! Some Databento datasets supply option `expiration` with date-level precision only: the
//! time-of-day is zeroed to midnight UTC. OPRA.PILLAR is the motivating case, where an option
//! expiring at 16:00 New York time arrives stamped at midnight UTC, which is the prior evening in
//! New York, causing the matching engine to treat the contract as expired before its final trading
//! session begins. [`DatabentoDecodeConfig`] holds per-dataset [`OptionExpirationRule`]s, keyed by
//! [`dbn::Dataset`], that reinterpret such midnight-UTC expirations at a configured exchange-local
//! wall-clock time. Datasets without a rule, and any expiration already carrying an intraday time,
//! are left untouched.

use std::sync::LazyLock;

use ahash::AHashMap;
use chrono::{DateTime, LocalResult, NaiveTime, TimeZone};
use chrono_tz::{America::New_York, Tz};
use databento::dbn;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_DAY};
use ustr::Ustr;

// Built-in defaults applied when a caller does not supply a `DatabentoDecodeConfig`
static DEFAULT_CONFIG: LazyLock<DatabentoDecodeConfig> =
    LazyLock::new(DatabentoDecodeConfig::default);

// New York wall-clock time applied to OPRA options by default (16:00, the regular close)
fn opra_default_time() -> NaiveTime {
    NaiveTime::from_hms_opt(16, 0, 0).expect("16:00:00 is a valid time")
}

/// Rule for reinterpreting a dataset's date-level (midnight-UTC) option expiration timestamps.
#[derive(Clone, Debug)]
pub struct OptionExpirationRule {
    /// Exchange-local timezone the wall-clock times are expressed in.
    pub timezone: Tz,
    /// Wall-clock expiration time applied when no per-underlying override matches.
    pub default_time: NaiveTime,
    /// Per-underlying wall-clock overrides, keyed by underlying symbol.
    pub overrides: AHashMap<Ustr, NaiveTime>,
}

impl OptionExpirationRule {
    /// Creates the default OPRA rule: 16:00 `America/New_York`, no per-underlying overrides.
    #[must_use]
    pub fn opra() -> Self {
        Self {
            timezone: New_York,
            default_time: opra_default_time(),
            overrides: AHashMap::new(),
        }
    }

    fn time_for(&self, underlying: Ustr) -> NaiveTime {
        self.overrides
            .get(&underlying)
            .copied()
            .unwrap_or(self.default_time)
    }
}

/// Dataset-specific configuration applied while decoding Databento definitions.
///
/// The configuration is keyed by [`dbn::Dataset`] so per-dataset parsing rules scale without
/// changing decode function signatures: adding behavior for another dataset is a new map entry.
#[derive(Clone, Debug)]
pub struct DatabentoDecodeConfig {
    /// Per-dataset option expiration correction rules.
    pub option_expiration: AHashMap<dbn::Dataset, OptionExpirationRule>,
}

impl Default for DatabentoDecodeConfig {
    fn default() -> Self {
        let mut option_expiration = AHashMap::new();
        option_expiration.insert(dbn::Dataset::OpraPillar, OptionExpirationRule::opra());
        Self { option_expiration }
    }
}

/// Returns a corrected option `expiration` for datasets with date-level (midnight-UTC) timestamps.
///
/// When `dataset` has an [`OptionExpirationRule`] in `config` and `expiration` falls exactly on midnight
/// UTC, the timestamp is reinterpreted at the rule's wall-clock time (the per-underlying override if
/// one matches, otherwise the rule default) in the rule's timezone. Datasets without a rule, and any
/// expiration already carrying an intraday time, are returned unchanged. A `config` of `None` uses
/// the built-in defaults (OPRA corrected to 16:00 New York), so the correction is on by default.
#[must_use]
pub fn corrected_option_expiration(
    expiration: UnixNanos,
    underlying: Ustr,
    dataset: Option<dbn::Dataset>,
    config: Option<&DatabentoDecodeConfig>,
) -> UnixNanos {
    let Some(dataset) = dataset else {
        return expiration;
    };
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let Some(rule) = config.option_expiration.get(&dataset) else {
        return expiration;
    };

    let raw = expiration.as_u64();
    // Only correct date-level timestamps (exact midnight UTC); leave any intraday time untouched,
    // so the correction self-disables should the dataset ever supply real expiration times.
    if raw == 0 || !raw.is_multiple_of(NANOSECONDS_IN_DAY) {
        return expiration;
    }
    let Ok(raw) = i64::try_from(raw) else {
        return expiration;
    };

    let date = DateTime::from_timestamp_nanos(raw).date_naive();
    let corrected = match rule
        .timezone
        .from_local_datetime(&date.and_time(rule.time_for(underlying)))
    {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => return expiration,
    };

    match corrected.timestamp_nanos_opt() {
        Some(ns) if ns >= 0 => UnixNanos::from(ns as u64),
        _ => expiration,
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;
    use databento::dbn;
    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use ustr::Ustr;

    use super::{DatabentoDecodeConfig, corrected_option_expiration};

    const EDT_MIDNIGHT_UTC: u64 = 1_782_691_200_000_000_000; // 2026-06-29 00:00 UTC
    const EDT_1600_ET: u64 = 1_782_763_200_000_000_000; // 2026-06-29 16:00 ET (20:00 UTC)
    const EST_MIDNIGHT_UTC: u64 = 1_768_521_600_000_000_000; // 2026-01-16 00:00 UTC
    const EST_1600_ET: u64 = 1_768_597_200_000_000_000; // 2026-01-16 16:00 ET (21:00 UTC)
    const EDT_0930_ET: u64 = 1_782_739_800_000_000_000; // 2026-06-29 09:30 ET (13:30 UTC)
    const INTRADAY_UTC: u64 = 1_789_738_200_000_000_000; // 2026-09-18 13:30 UTC (non-midnight)

    fn config_with_opra_override(underlying: &str, time: NaiveTime) -> DatabentoDecodeConfig {
        let mut config = DatabentoDecodeConfig::default();
        config
            .option_expiration
            .get_mut(&dbn::Dataset::OpraPillar)
            .unwrap()
            .overrides
            .insert(Ustr::from(underlying), time);
        config
    }

    #[rstest]
    fn test_opra_midnight_corrected_to_1600_et_during_edt() {
        let result = corrected_option_expiration(
            UnixNanos::from(EDT_MIDNIGHT_UTC),
            Ustr::from("SPX"),
            Some(dbn::Dataset::OpraPillar),
            None,
        );
        assert_eq!(result.as_u64(), EDT_1600_ET);
    }

    #[rstest]
    fn test_opra_midnight_corrected_to_1600_et_during_est() {
        let result = corrected_option_expiration(
            UnixNanos::from(EST_MIDNIGHT_UTC),
            Ustr::from("SPX"),
            Some(dbn::Dataset::OpraPillar),
            None,
        );
        assert_eq!(result.as_u64(), EST_1600_ET);
    }

    #[rstest]
    fn test_opra_override_applied_for_matching_underlying() {
        let config = config_with_opra_override("XSP", NaiveTime::from_hms_opt(9, 30, 0).unwrap());
        let result = corrected_option_expiration(
            UnixNanos::from(EDT_MIDNIGHT_UTC),
            Ustr::from("XSP"),
            Some(dbn::Dataset::OpraPillar),
            Some(&config),
        );
        assert_eq!(result.as_u64(), EDT_0930_ET);
    }

    #[rstest]
    fn test_opra_default_used_when_underlying_not_overridden() {
        let config = config_with_opra_override("XSP", NaiveTime::from_hms_opt(9, 30, 0).unwrap());
        let result = corrected_option_expiration(
            UnixNanos::from(EDT_MIDNIGHT_UTC),
            Ustr::from("SPX"),
            Some(dbn::Dataset::OpraPillar),
            Some(&config),
        );
        assert_eq!(result.as_u64(), EDT_1600_ET);
    }

    #[rstest]
    fn test_opra_intraday_expiration_passes_through() {
        let result = corrected_option_expiration(
            UnixNanos::from(INTRADAY_UTC),
            Ustr::from("SPX"),
            Some(dbn::Dataset::OpraPillar),
            None,
        );
        assert_eq!(result.as_u64(), INTRADAY_UTC);
    }

    #[rstest]
    fn test_non_opra_midnight_passes_through() {
        let result = corrected_option_expiration(
            UnixNanos::from(EDT_MIDNIGHT_UTC),
            Ustr::from("ESU6"),
            Some(dbn::Dataset::GlbxMdp3),
            None,
        );
        assert_eq!(result.as_u64(), EDT_MIDNIGHT_UTC);
    }

    #[rstest]
    fn test_unknown_dataset_passes_through() {
        let result = corrected_option_expiration(
            UnixNanos::from(EDT_MIDNIGHT_UTC),
            Ustr::from("SPX"),
            None,
            None,
        );
        assert_eq!(result.as_u64(), EDT_MIDNIGHT_UTC);
    }

    #[rstest]
    fn test_dataset_without_rule_passes_through() {
        // A custom config that omits a rule for OPRA disables the correction for that dataset.
        let config = DatabentoDecodeConfig {
            option_expiration: ahash::AHashMap::new(),
        };
        let result = corrected_option_expiration(
            UnixNanos::from(EDT_MIDNIGHT_UTC),
            Ustr::from("SPX"),
            Some(dbn::Dataset::OpraPillar),
            Some(&config),
        );
        assert_eq!(result.as_u64(), EDT_MIDNIGHT_UTC);
    }
}
