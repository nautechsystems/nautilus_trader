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
