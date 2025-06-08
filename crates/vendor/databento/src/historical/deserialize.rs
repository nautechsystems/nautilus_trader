//! Custom deserializers
use serde::Deserialize;
use time::format_description::well_known::iso8601::Iso8601;

const LEGACY_DATE_TIME_FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second][optional [.[subsecond digits:6]]][optional [+[offset_hour]:[offset_minute]]]");

pub(crate) fn deserialize_date_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<time::OffsetDateTime, D::Error> {
    let dt_str = String::deserialize(deserializer)?;
    time::PrimitiveDateTime::parse(&dt_str, &Iso8601::DEFAULT)
        .map(|dt| dt.assume_utc())
        .or_else(|_| time::OffsetDateTime::parse(&dt_str, LEGACY_DATE_TIME_FORMAT))
        .map_err(serde::de::Error::custom)
}

pub(crate) fn deserialize_opt_date_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<time::OffsetDateTime>, D::Error> {
    if let Some(dt_str) = Option::<String>::deserialize(deserializer)? {
        time::PrimitiveDateTime::parse(&dt_str, &Iso8601::DEFAULT)
            .map(|dt| dt.assume_utc())
            .or_else(|_| time::OffsetDateTime::parse(&dt_str, LEGACY_DATE_TIME_FORMAT))
            .map(Some)
            .map_err(serde::de::Error::custom)
    } else {
        Ok(None)
    }
}
