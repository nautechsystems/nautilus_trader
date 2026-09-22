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

//! Generic spread identifier adaptation for Interactive Brokers combo contracts.

use nautilus_model::identifiers::{
    InstrumentId, new_generic_spread_id, parse_generic_spread_id_legs,
};

/// Returns whether an instrument ID uses the generic spread grammar.
#[must_use]
pub fn is_spread_instrument_id(instrument_id: &InstrumentId) -> bool {
    parse_generic_spread_id_legs(instrument_id).is_some()
}

/// Creates a generic spread instrument ID from IB-compatible leg ratios.
///
/// # Errors
///
/// Returns an error if the legs do not satisfy the generic spread grammar.
pub fn create_spread_instrument_id(legs: &[(InstrumentId, i32)]) -> anyhow::Result<InstrumentId> {
    let legs = legs
        .iter()
        .map(|(instrument_id, ratio)| (*instrument_id, i64::from(*ratio)))
        .collect::<Vec<_>>();
    new_generic_spread_id(&legs)
}

/// Parses a generic spread instrument ID into IB-compatible leg ratios.
///
/// # Errors
///
/// Returns an error if the ID does not use the generic spread grammar or a ratio is outside the
/// range supported by IB combo legs.
pub fn parse_spread_instrument_id_to_legs(
    instrument_id: &InstrumentId,
) -> anyhow::Result<Vec<(InstrumentId, i32)>> {
    parse_generic_spread_id_legs(instrument_id)
        .ok_or_else(|| anyhow::anyhow!("Invalid generic spread instrument ID: {instrument_id}"))?
        .into_iter()
        .map(|(instrument_id, ratio)| {
            i32::try_from(ratio)
                .map(|ratio| (instrument_id, ratio))
                .map_err(|_| anyhow::anyhow!("Spread leg ratio {ratio} exceeds the IB i32 range"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::{InstrumentId, new_generic_spread_id};
    use rstest::rstest;

    use super::{
        create_spread_instrument_id, is_spread_instrument_id, parse_spread_instrument_id_to_legs,
    };

    #[rstest]
    fn test_spread_adapter_matches_generic_spread_grammar() {
        let legs = [
            (InstrumentId::from("MSFT.NASDAQ"), -2),
            (InstrumentId::from("AAPL.NASDAQ"), 1),
        ];
        let generic_legs = legs
            .iter()
            .map(|(instrument_id, ratio)| (*instrument_id, i64::from(*ratio)))
            .collect::<Vec<_>>();

        let spread = create_spread_instrument_id(&legs).unwrap();

        assert_eq!(spread, new_generic_spread_id(&generic_legs).unwrap());
        assert_eq!(spread, InstrumentId::from("(1)AAPL___((2))MSFT.NASDAQ"));
        assert_eq!(
            parse_spread_instrument_id_to_legs(&spread).unwrap(),
            [
                (InstrumentId::from("AAPL.NASDAQ"), 1),
                (InstrumentId::from("MSFT.NASDAQ"), -2),
            ]
        );
        assert!(is_spread_instrument_id(&spread));
    }

    #[rstest]
    fn test_spread_adapter_rejects_legacy_single_underscore_separator() {
        let instrument_id = InstrumentId::from("(1)AAPL_((2))MSFT.NASDAQ");

        assert!(parse_spread_instrument_id_to_legs(&instrument_id).is_err());
        assert!(!is_spread_instrument_id(&instrument_id));
    }
}
