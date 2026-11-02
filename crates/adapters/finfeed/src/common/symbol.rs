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

//! Provider outcome IDs retain their spelling under separate FinFeed venues.

use nautilus_model::identifiers::{InstrumentId, Symbol, Venue};
use ustr::Ustr;

use crate::http::error::Error;

const VENUE_PREFIX: &str = "FINFEED_";

/// Maps an exact exchange and outcome ID to a provider-owned instrument ID.
///
/// Native adapters use separate venues and remain independent definition sources.
///
/// # Errors
///
/// Returns an error if the exchange is unknown or the outcome ID is invalid.
pub fn to_instrument_id(exchange_id: &str, market_id: &str) -> Result<InstrumentId, Error> {
    validate_exchange_id(exchange_id)?;

    if matches!(market_id, "." | "..") {
        return Err(Error::Validation("invalid outcome ID".into()));
    }

    let symbol = Symbol::new_checked(market_id)
        .map_err(|_| Error::Validation("invalid outcome ID".into()))?;
    let venue = Venue::new_checked(format!("{VENUE_PREFIX}{exchange_id}"))
        .map_err(|_| Error::Validation("invalid FinFeed venue".into()))?;
    Ok(InstrumentId::new(symbol, venue))
}

/// Returns the exact provider exchange and outcome ID for a FinFeed instrument.
///
/// # Errors
///
/// Returns an error if the instrument does not use a known FinFeed venue or valid outcome ID.
pub fn from_instrument_id(instrument_id: InstrumentId) -> Result<(Ustr, Ustr), Error> {
    let exchange_id = instrument_id
        .venue
        .as_str()
        .strip_prefix(VENUE_PREFIX)
        .ok_or_else(|| Error::Validation("instrument does not use a FinFeed venue".into()))?;
    let market_id = instrument_id.symbol.as_str();
    to_instrument_id(exchange_id, market_id)?;
    Ok((exchange_id.into(), market_id.into()))
}

pub(crate) fn validate_exchange_id(exchange_id: &str) -> Result<(), Error> {
    if !matches!(
        exchange_id,
        "KALSHI"
            | "POLYMARKET"
            | "MYRIAD"
            | "MANIFOLD"
            | "HYPERLIQUIDOUTCOME"
            | "PASCAL"
            | "GEMINIPM"
            | "CRYPTOCOMPM"
            | "FORECASTEX"
    ) {
        return Err(Error::Validation(format!("unknown exchange {exchange_id}")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::http::models::MarketsPage;

    #[rstest]
    #[case("KALSHI", include_str!("../../test_data/kalshi-listings.json"))]
    #[case("POLYMARKET", include_str!("../../test_data/polymarket-listings.json"))]
    #[case("MYRIAD", include_str!("../../test_data/myriad-listings.json"))]
    #[case("MANIFOLD", include_str!("../../test_data/manifold-listings.json"))]
    #[case("HYPERLIQUIDOUTCOME", include_str!("../../test_data/hyperliquidoutcome-listings.json"))]
    #[case("PASCAL", include_str!("../../test_data/pascal-listings.json"))]
    #[case("GEMINIPM", include_str!("../../test_data/geminipm-listings.json"))]
    #[case("CRYPTOCOMPM", include_str!("../../test_data/cryptocompm-listings.json"))]
    #[case("FORECASTEX", include_str!("../../test_data/forecastex-listings.json"))]
    fn outcome_identity_round_trips_every_known_venue(
        #[case] exchange: &str,
        #[case] fixture: &str,
    ) {
        let page: MarketsPage = serde_json::from_str(fixture).unwrap();
        for market in page.data.unwrap() {
            let market_id = market.market_id.unwrap();
            let id = to_instrument_id(exchange, &market_id).unwrap();
            let serialized = id.to_string();
            let reparsed: InstrumentId = serialized.parse().unwrap();
            assert_eq!(id.symbol.as_str(), market_id);
            assert_eq!(id.venue.as_str(), format!("FINFEED_{exchange}"));
            assert_eq!(reparsed, id);
            assert_eq!(
                from_instrument_id(reparsed).unwrap(),
                (exchange.into(), market_id)
            );
        }
    }

    #[rstest]
    #[case("UNKNOWN", "contract_NO")]
    #[case("polymarket", "contract_NO")]
    #[case("POLYMARKET", "")]
    #[case("POLYMARKET", " ")]
    #[case("POLYMARKET", "..")]
    fn invalid_identity_is_rejected(#[case] exchange: &str, #[case] market_id: &str) {
        assert!(matches!(
            to_instrument_id(exchange, market_id),
            Err(Error::Validation(_))
        ));
    }

    #[rstest]
    fn native_venue_cannot_claim_a_finfeed_definition() {
        let native = InstrumentId::from("condition-token.POLYMARKET");
        assert!(matches!(
            from_instrument_id(native),
            Err(Error::Validation(_))
        ));
        let provider = to_instrument_id("POLYMARKET", "condition-token").unwrap();
        assert_ne!(provider, native);
    }
}
