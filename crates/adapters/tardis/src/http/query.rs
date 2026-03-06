use chrono::{DateTime, Utc};
use derive_builder::Builder;
use serde::Serialize;

mod datetime_format {
    use chrono::{DateTime, Utc};
    use serde::{self, Serializer};

    pub fn serialize<S>(date: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(dt) => {
                serializer.serialize_str(&dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            }
            None => serializer.serialize_none(),
        }
    }
}

/// Provides an instrument metadata API filter object.
///
/// See <https://docs.tardis.dev/api/instruments-metadata-api>.
#[derive(Debug, Default, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub base_currency: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub quote_currency: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    #[builder(default)]
    pub instrument_type: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub contract_type: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(with = "datetime_format")]
    #[builder(default)]
    pub available_since: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(with = "datetime_format")]
    #[builder(default)]
    pub available_to: Option<DateTime<Utc>>,
}
