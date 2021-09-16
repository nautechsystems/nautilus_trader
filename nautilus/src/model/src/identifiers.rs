// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt;

pub type Symbol = String;
pub type Venue = String;
pub type TraderId = String;
pub type TradeMatchId = String;
pub type StrategyId = String;
pub type PositionId = String;
pub type ClientOrderId = String;
pub type ClientOrderLinkId = String;
pub type VenueOrderId = String;
pub type BracketOrderId = String;

pub struct InstrumentId {
    pub symbol: Symbol,
    pub venue: Venue,
}

impl InstrumentId {
    pub fn new(symbol: Symbol, venue: Venue) -> InstrumentId {
        InstrumentId { symbol, venue }
    }

    pub fn from_string(value: &str) -> InstrumentId {
        let pieces: Vec<&str> = value.split(".").collect();
        let symbol = String::from(pieces[0]);
        let venue = String::from(pieces[1]);
        InstrumentId { symbol, venue }
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.symbol, self.venue)
    }
}

impl fmt::Debug for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InstrumentId({}.{})", self.symbol, self.venue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_id_to_string() {
        let instrument_id = InstrumentId::new(Symbol::from("AUD/USD"), Venue::from("IDEALPRO"));
        assert_eq!("AUD/USD.IDEALPRO", instrument_id.to_string());
    }
}
