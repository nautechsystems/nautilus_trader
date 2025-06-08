//! Historical client and related API types.

pub mod batch;
mod client;
mod deserialize;
pub mod metadata;
pub mod symbology;
pub mod timeseries;

pub use client::*;
use time::{
    format_description::BorrowedFormatItem, macros::format_description, Duration, Time, UtcOffset,
};

use crate::{Error, Symbols};

/// The current Databento historical API version.
pub const API_VERSION: u32 = 0;

/// The Historical API gateway to use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoricalGateway {
    /// The default gateway in Boston.
    #[default]
    Bo1,
}

/// A **half**-closed date interval with an inclusive start date and an exclusive end
/// date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateRange {
    /// The start date (inclusive).
    start: time::Date,
    /// The end date (exclusive).
    end: time::Date,
}

/// A **half**-closed datetime interval with an inclusive start time and an exclusive
/// end time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateTimeRange {
    /// The start date time (inclusive).
    start: time::OffsetDateTime,
    /// The end date time (exclusive).
    end: time::OffsetDateTime,
}

impl From<(time::Date, time::Date)> for DateRange {
    fn from(value: (time::Date, time::Date)) -> Self {
        Self {
            start: value.0,
            end: value.1,
        }
    }
}

impl From<(time::Date, time::Duration)> for DateRange {
    fn from(value: (time::Date, time::Duration)) -> Self {
        Self {
            start: value.0,
            end: value.0 + value.1,
        }
    }
}

impl From<time::Date> for DateRange {
    fn from(date: time::Date) -> Self {
        Self {
            start: date,
            end: date.next_day().unwrap(),
        }
    }
}

impl From<time::Date> for DateTimeRange {
    fn from(date: time::Date) -> Self {
        let start = date.with_time(Time::MIDNIGHT).assume_utc();
        Self {
            start,
            end: start + Duration::DAY,
        }
    }
}

impl From<DateRange> for DateTimeRange {
    fn from(date_range: DateRange) -> Self {
        Self {
            start: date_range.start.with_time(Time::MIDNIGHT).assume_utc(),
            end: date_range.end.with_time(Time::MIDNIGHT).assume_utc(),
        }
    }
}

impl From<DateTimeRange> for DateRange {
    fn from(dt_range: DateTimeRange) -> Self {
        let utc_end = dt_range.end.to_offset(UtcOffset::UTC);
        Self {
            start: dt_range.start.to_offset(UtcOffset::UTC).date(),
            // Round up end to nearest date
            end: if utc_end.time() == Time::MIDNIGHT {
                utc_end.date()
            } else {
                utc_end.date().next_day().unwrap()
            },
        }
    }
}

pub(crate) const DATE_FORMAT: &[BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day]");

impl From<(time::OffsetDateTime, time::OffsetDateTime)> for DateTimeRange {
    fn from(value: (time::OffsetDateTime, time::OffsetDateTime)) -> Self {
        Self {
            start: value.0,
            end: value.1,
        }
    }
}

impl From<(time::OffsetDateTime, time::Duration)> for DateTimeRange {
    fn from(value: (time::OffsetDateTime, time::Duration)) -> Self {
        Self {
            start: value.0,
            end: value.0 + value.1,
        }
    }
}

impl TryFrom<(u64, u64)> for DateTimeRange {
    type Error = crate::Error;

    fn try_from(value: (u64, u64)) -> Result<Self, Self::Error> {
        let start = time::OffsetDateTime::from_unix_timestamp_nanos(value.0 as i128)
            .map_err(|e| Error::bad_arg("first UNIX nanos", format!("{e:?}")))?;
        let end = time::OffsetDateTime::from_unix_timestamp_nanos(value.1 as i128)
            .map_err(|e| Error::bad_arg("second UNIX nanos", format!("{e:?}")))?;
        Ok(Self { start, end })
    }
}

trait AddToQuery<T> {
    fn add_to_query(self, param: &T) -> Self;
}

impl AddToQuery<DateRange> for reqwest::RequestBuilder {
    fn add_to_query(self, param: &DateRange) -> Self {
        self.query(&[
            ("start_date", param.start.format(DATE_FORMAT).unwrap()),
            ("end_date", param.end.format(DATE_FORMAT).unwrap()),
        ])
    }
}

impl AddToQuery<DateTimeRange> for reqwest::RequestBuilder {
    fn add_to_query(self, param: &DateTimeRange) -> Self {
        self.query(&[
            ("start", param.start.unix_timestamp_nanos()),
            ("end", param.end.unix_timestamp_nanos()),
        ])
    }
}

impl AddToQuery<Symbols> for reqwest::RequestBuilder {
    fn add_to_query(self, param: &Symbols) -> Self {
        self.query(&[("symbols", param.to_api_string())])
    }
}

impl DateRange {
    pub(crate) fn add_to_form(&self, form: &mut Vec<(&'static str, String)>) {
        form.push(("start_date", self.start.format(DATE_FORMAT).unwrap()));
        form.push(("end_date", self.end.format(DATE_FORMAT).unwrap()));
    }
}

impl DateTimeRange {
    pub(crate) fn add_to_form(&self, form: &mut Vec<(&'static str, String)>) {
        form.push(("start", self.start.unix_timestamp_nanos().to_string()));
        form.push(("end", self.end.unix_timestamp_nanos().to_string()));
    }
}

#[cfg(test)]
mod tests {
    use time::macros::{date, datetime};

    use super::*;

    #[test]
    fn date_range_from_lt_day_duration() {
        let target = DateRange::from((date!(2024 - 02 - 16), time::Duration::SECOND));
        assert_eq!(
            target,
            DateRange {
                start: date!(2024 - 02 - 16),
                end: date!(2024 - 02 - 16)
            }
        )
    }

    #[test]
    fn single_date_conversion() {
        let date = date!(2025 - 03 - 27);
        assert_eq!(
            DateRange::from(date),
            DateRange::from((date!(2025 - 03 - 27), date!(2025 - 03 - 28)))
        );
        assert_eq!(
            DateTimeRange::from(date),
            DateTimeRange::from((
                datetime!(2025 - 03 - 27 00:00 UTC),
                datetime!(2025 - 03 - 28 00:00 UTC)
            ))
        );
    }

    #[test]
    fn range_equivalency() {
        let date_range = DateRange::from((date!(2025 - 03 - 27), date!(2025 - 04 - 10)));
        assert_eq!(
            date_range,
            DateRange::from(DateTimeRange::from(date_range.clone()))
        );
    }

    #[test]
    fn dt_offset_to_date_range() {
        assert_eq!(
            DateRange::from(DateTimeRange::from((
                datetime!(2025-03-27 21:00 -4),
                datetime!(2025-03-28 20:00 -4)
            ))),
            DateRange::from((date!(2025 - 03 - 28), date!(2025 - 03 - 29)))
        );
        assert_eq!(
            DateRange::from(DateTimeRange::from((
                datetime!(2025-03-27 21:00 -4),
                datetime!(2025-03-28 20:30 -4)
            ))),
            DateRange::from((date!(2025 - 03 - 28), date!(2025 - 03 - 30)))
        );
    }
}
