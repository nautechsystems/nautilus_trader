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

use nautilus_model::identifiers::{ClientOrderId, InstrumentId};
use thiserror::Error;
use ustr::Ustr;

/// Message used for a missing currency lookup.
pub const CURRENCY_NOT_FOUND: &str = "currency not found in cache";

/// Message used for a missing instrument lookup.
pub const INSTRUMENT_NOT_FOUND: &str = "instrument not found in cache";

/// Message used for a missing order lookup.
pub const ORDER_NOT_FOUND: &str = "order not found in cache";

/// Error returned when a currency cannot be resolved from a cache or store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CurrencyLookupError {
    /// The requested currency is not present.
    #[error("{message}: {code}", message = CURRENCY_NOT_FOUND)]
    NotFound {
        /// The currency code that was requested.
        code: Ustr,
    },
}

impl CurrencyLookupError {
    /// Returns a not-found error for `code`.
    #[must_use]
    pub const fn not_found(code: Ustr) -> Self {
        Self::NotFound { code }
    }
}

/// Error returned when an instrument cannot be resolved from a cache or store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InstrumentLookupError {
    /// The requested instrument is not present.
    #[error("{message}: {instrument_id}", message = INSTRUMENT_NOT_FOUND)]
    NotFound {
        /// The instrument identifier that was requested.
        instrument_id: InstrumentId,
    },
}

impl InstrumentLookupError {
    /// Returns a not-found error for `instrument_id`.
    #[must_use]
    pub const fn not_found(instrument_id: InstrumentId) -> Self {
        Self::NotFound { instrument_id }
    }
}

/// Error returned when an order cannot be resolved from a cache or store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OrderLookupError {
    /// The requested order is not present.
    #[error("{message}: {client_order_id}", message = ORDER_NOT_FOUND)]
    NotFound {
        /// The client order identifier that was requested.
        client_order_id: ClientOrderId,
    },
}

impl OrderLookupError {
    /// Returns a not-found error for `client_order_id`.
    #[must_use]
    pub const fn not_found(client_order_id: ClientOrderId) -> Self {
        Self::NotFound { client_order_id }
    }
}
