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

//! Deribit HTTP API query parameter builders.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use super::models::{DeribitCurrency, DeribitInstrumentKind};

/// Query parameters for `/public/get_instruments` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetInstrumentsParams {
    /// Currency filter
    pub currency: DeribitCurrency,
    /// Optional instrument kind filter
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub kind: Option<DeribitInstrumentKind>,
    /// Whether to include expired instruments
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub expired: Option<bool>,
}

impl GetInstrumentsParams {
    /// Creates a new builder for [`GetInstrumentsParams`].
    #[must_use]
    pub fn builder() -> GetInstrumentsParamsBuilder {
        GetInstrumentsParamsBuilder::default()
    }

    /// Creates parameters for a specific currency.
    #[must_use]
    pub fn new(currency: DeribitCurrency) -> Self {
        Self {
            currency,
            kind: None,
            expired: None,
        }
    }

    /// Creates parameters for a specific currency and kind.
    #[must_use]
    pub fn with_kind(currency: DeribitCurrency, kind: DeribitInstrumentKind) -> Self {
        Self {
            currency,
            kind: Some(kind),
            expired: None,
        }
    }
}

/// Query parameters for `/public/get_instrument` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
pub struct GetInstrumentParams {
    /// Instrument name (e.g., "BTC-PERPETUAL", "ETH-25MAR23-2000-C")
    pub instrument_name: String,
}

/// Query parameters for `/private/get_account_summaries` endpoint.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GetAccountSummariesParams {
    /// The user id for the subaccount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subaccount_id: Option<String>,
    /// Include extended fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended: Option<bool>,
}

impl GetAccountSummariesParams {
    /// Creates a new instance with both subaccount ID and extended flag.
    #[must_use]
    pub fn new(subaccount_id: String, extended: bool) -> Self {
        Self {
            subaccount_id: Some(subaccount_id),
            extended: Some(extended),
        }
    }
}

/// Query parameters for `/public/get_last_trades_by_instrument_and_time` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetLastTradesByInstrumentAndTimeParams {
    /// Instrument name (e.g., "BTC-PERPETUAL")
    pub instrument_name: String,
    /// The earliest timestamp to return result from (milliseconds since the UNIX epoch)
    pub start_timestamp: i64,
    /// The most recent timestamp to return result from (milliseconds since the UNIX epoch)
    pub end_timestamp: i64,
    /// Number of requested items, default - 10, maximum - 1000
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub count: Option<u32>,
    /// Direction of results sorting: "asc", "desc", or "default"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub sorting: Option<String>,
}

impl GetLastTradesByInstrumentAndTimeParams {
    /// Creates a new instance with the required parameters.
    #[must_use]
    pub fn new(
        instrument_name: impl Into<String>,
        start_timestamp: i64,
        end_timestamp: i64,
        count: Option<u32>,
        sorting: Option<String>,
    ) -> Self {
        Self {
            instrument_name: instrument_name.into(),
            start_timestamp,
            end_timestamp,
            count,
            sorting,
        }
    }
}
