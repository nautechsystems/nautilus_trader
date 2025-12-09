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
