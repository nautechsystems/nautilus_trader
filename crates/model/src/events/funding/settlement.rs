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

use nautilus_core::{UUID4, UnixNanos};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::{AccountId, InstrumentId, TraderId},
    types::{Currency, Price},
};

/// Represents a funding settlement for a perpetual swap instrument.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct FundingSettlement {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The instrument ID for the settlement.
    pub instrument_id: InstrumentId,
    /// The account ID receiving the settlement.
    pub account_id: AccountId,
    /// The funding rate applied for the settlement.
    pub rate: Decimal,
    /// The mark or settlement price used to value open positions.
    pub settlement_price: Price,
    /// The currency for resulting funding payments.
    pub currency: Currency,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl FundingSettlement {
    /// Creates a new [`FundingSettlement`] instance.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub const fn new(
        trader_id: TraderId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        rate: Decimal,
        settlement_price: Price,
        currency: Currency,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            instrument_id,
            account_id,
            rate,
            settlement_price,
            currency,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::{UUID4, UnixNanos};
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        identifiers::{AccountId, InstrumentId, TraderId},
        types::{Currency, Price},
    };

    fn create_test_settlement() -> FundingSettlement {
        FundingSettlement::new(
            TraderId::from("TRADER-001"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            AccountId::from("BINANCE-001"),
            Decimal::from_str("0.0001").unwrap(),
            Price::from("65000.00"),
            Currency::USDT(),
            UUID4::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    #[rstest]
    fn test_funding_settlement_serialization() {
        let original = create_test_settlement();

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: FundingSettlement = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }
}
