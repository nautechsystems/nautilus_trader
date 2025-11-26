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

use crate::identifiers::{
    AccountId, ClientId, ClientOrderId, PositionId, StrategyId, Symbol, TradeId, TraderId, Venue,
    VenueOrderId,
};

impl Default for AccountId {
    /// Creates a new default [`AccountId`] instance for testing.
    fn default() -> Self {
        Self::from("SIM-001")
    }
}

impl Default for ClientId {
    /// Creates a new default [`ClientId`] instance for testing.
    fn default() -> Self {
        Self::from("SIM")
    }
}

impl Default for ClientOrderId {
    /// Creates a new default [`ClientOrderId`] instance for testing.
    fn default() -> Self {
        Self::from("O-19700101-000000-001-001-1")
    }
}

impl Default for PositionId {
    /// Creates a new default [`PositionId`] instance for testing.
    fn default() -> Self {
        Self::from("P-001")
    }
}

impl Default for StrategyId {
    /// Creates a new default [`StrategyId`] instance for testing.
    fn default() -> Self {
        Self::from("S-001")
    }
}

impl Default for TradeId {
    /// Creates a new default [`TradeId`] instance for testing.
    fn default() -> Self {
        Self::from("1")
    }
}

impl Default for TraderId {
    /// Creates a new default [`TraderId`] instance for testing.
    fn default() -> Self {
        Self::from("TRADER-001")
    }
}
impl Default for Symbol {
    /// Creates a new default [`Symbol`] instance for testing.
    fn default() -> Self {
        Self::from("AUD/USD")
    }
}

impl Default for Venue {
    /// Creates a new default [`Venue`] instance for testing.
    fn default() -> Self {
        Self::from("SIM")
    }
}

impl Default for VenueOrderId {
    /// Creates a new default [`VenueOrderId`] instance for testing.
    fn default() -> Self {
        Self::from("001")
    }
}
