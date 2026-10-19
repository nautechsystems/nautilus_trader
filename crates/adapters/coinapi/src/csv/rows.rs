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

//! Flat file rows: reading, column lookup, and field parsing shared by every schema.

use std::{
    ffi::OsStr,
    fs::File,
    io::{BufReader, Read},
    path::{Component, Path},
    str::FromStr,
};

use csv::{Reader, ReaderBuilder, StringRecord};
use flate2::read::MultiGzDecoder;
use jiff::{
    civil::{Date, DateTime, Time},
    tz::TimeZone,
};
use nautilus_core::{UnixNanos, serialization::decimal};
use nautilus_model::types::fixed::FIXED_PRECISION;
use rust_decimal::Decimal;

use crate::http::parse::unix_nanos;

const BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// Rows of one flat file, read in file order.
pub(crate) struct Rows {
    reader: Reader<Box<dyn Read>>,
    record: StringRecord,
}

impl Rows {
    /// Opens a semicolon-delimited flat file, decompressing it when its extension is `.gz`.
    pub(crate) fn open(path: &Path) -> anyhow::Result<Self> {
        let file = File::open(path)
            .map_err(|e| anyhow::anyhow!("failed to open '{}': {e}", path.display()))?;
        let buffered = BufReader::with_capacity(BUFFER_SIZE, file);

        let is_gzipped = path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gz"));

        let input: Box<dyn Read> = if is_gzipped {
            Box::new(MultiGzDecoder::new(buffered))
        } else {
            Box::new(buffered)
        };

        let reader = ReaderBuilder::new()
            .delimiter(b';')
            .has_headers(true)
            .from_reader(input);

        Ok(Self {
            reader,
            record: StringRecord::new(),
        })
    }

    /// Returns the index of the `name` column.
    pub(crate) fn column(&mut self, name: &str) -> anyhow::Result<usize> {
        self.reader
            .headers()?
            .iter()
            .position(|header| header == name)
            .ok_or_else(|| anyhow::anyhow!("flat file has no '{name}' column"))
    }

    /// Reads the next row, returning `None` at the end of the file.
    pub(crate) fn next(&mut self) -> anyhow::Result<Option<&StringRecord>> {
        if self.reader.read_record(&mut self.record)? {
            Ok(Some(&self.record))
        } else {
            Ok(None)
        }
    }
}

/// Returns the text of column `index`.
pub(crate) fn text(record: &StringRecord, index: usize) -> anyhow::Result<&str> {
    record.get(index).ok_or_else(|| {
        anyhow::anyhow!(
            "row at line {} has no column {index}",
            record.position().map_or(0, csv::Position::line)
        )
    })
}

/// Parses column `index` as an exact decimal.
pub(crate) fn number(record: &StringRecord, index: usize, field: &str) -> anyhow::Result<Decimal> {
    let value = text(record, index)?;
    decimal::parse(value).map_err(|e| anyhow::anyhow!("invalid {field} '{value}': {e}"))
}

/// Parses a zone-less UTC date-time such as `2025-09-01T00:00:00.1780000`.
pub(crate) fn date_time(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    let parsed =
        DateTime::from_str(value).map_err(|e| anyhow::anyhow!("invalid {field} '{value}': {e}"))?;
    utc_nanos(parsed, field)
}

/// Parses a time of day such as `00:00:00.0140000` on `date`.
pub(crate) fn time_on(date: Date, value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    let time =
        Time::from_str(value).map_err(|e| anyhow::anyhow!("invalid {field} '{value}': {e}"))?;
    utc_nanos(date.to_datetime(time), field)
}

fn utc_nanos(value: DateTime, field: &str) -> anyhow::Result<UnixNanos> {
    let timestamp = value
        .to_zoned(TimeZone::UTC)
        .map_err(|e| anyhow::anyhow!("invalid {field} {value}: {e}"))?
        .timestamp();
    unix_nanos(timestamp).map_err(|e| anyhow::anyhow!("invalid {field}: {e}"))
}

/// Returns the date of the `D-YYYYMMDD` or `D-YYYYMMDDHH` partition that contains `path`.
#[must_use]
pub(crate) fn partition_date(path: &Path) -> Option<Date> {
    path.components().rev().find_map(|component| {
        let Component::Normal(name) = component else {
            return None;
        };

        let digits = name.to_str()?.strip_prefix("D-")?;

        if !(digits.len() == 8 || digits.len() == 10) || !digits.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }

        Date::new(
            digits[0..4].parse().ok()?,
            digits[4..6].parse().ok()?,
            digits[6..8].parse().ok()?,
        )
        .ok()
    })
}

/// A price or size precision, fixed by the caller or inferred from the values seen.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Precision {
    pub(crate) value: u8,
    pub(crate) is_inferred: bool,
}

impl Precision {
    pub(crate) fn new(explicit: Option<u8>) -> Self {
        Self {
            value: explicit.unwrap_or(0),
            is_inferred: explicit.is_none(),
        }
    }

    /// Fixes the precision at its current value.
    pub(crate) fn fixed(self) -> Self {
        Self {
            value: self.value,
            is_inferred: false,
        }
    }

    /// Raises an inferred precision to fit `value`, up to the maximum precision.
    pub(crate) fn fit(&mut self, value: Decimal) {
        if self.is_inferred {
            let scale = u8::try_from(value.normalize().scale()).unwrap_or(u8::MAX);
            self.value = self.value.max(scale.min(FIXED_PRECISION));
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("T-LIMITBOOK_FULL/D-20250901/E-BINANCE/IDDI-1+SC-X+S-Y.csv.gz", Some((2025, 9, 1)))]
    #[case("T-LIMITBOOK_FULL/D-2026011722/E-COINBASE/file.csv.gz", Some((2026, 1, 17)))]
    #[case("data/D-2025090/file.csv", None)]
    #[case("data/D-20251301/file.csv", None)]
    #[case("limitbook_full.csv", None)]
    fn partition_date_reads_daily_and_hourly_partitions(
        #[case] path: &str,
        #[case] expected: Option<(i16, i8, i8)>,
    ) {
        let expected = expected.map(|(y, m, d)| Date::new(y, m, d).unwrap());
        assert_eq!(partition_date(Path::new(path)), expected);
    }

    #[rstest]
    fn date_time_reads_seven_fractional_digits_as_utc() {
        assert_eq!(
            date_time("2025-09-01T00:00:00.1780000", "time_exchange").unwrap(),
            UnixNanos::from(1_756_684_800_178_000_000)
        );
    }

    #[rstest]
    fn time_on_combines_date_and_time_of_day() {
        let date = Date::new(2025, 9, 1).unwrap();
        assert_eq!(
            time_on(date, "00:00:00.0140000", "time_exchange").unwrap(),
            UnixNanos::from(1_756_684_800_014_000_000)
        );
    }

    #[rstest]
    #[case(None, &["1.5", "2.25", "3"], 2, true)]
    #[case(Some(4), &["1.5", "2.25"], 4, false)]
    #[case(None, &["1.500"], 1, true)]
    fn precision_fits_inferred_values_only(
        #[case] explicit: Option<u8>,
        #[case] values: &[&str],
        #[case] expected: u8,
        #[case] is_inferred: bool,
    ) {
        let mut precision = Precision::new(explicit);
        for value in values {
            precision.fit(Decimal::from_str(value).unwrap());
        }

        assert_eq!(precision.value, expected);
        assert_eq!(precision.is_inferred, is_inferred);
    }
}
