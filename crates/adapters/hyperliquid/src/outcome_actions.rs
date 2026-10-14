// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Hyperliquid HIP-4 binary-option action validation and encoding.

use nautilus_common::messages::execution::{BinaryOptionAction, BinaryOptionPair};
use rust_decimal::Decimal;

use crate::common::{outcome_asset_id_from_instrument_id, outcome_pair_instrument_ids};

/// A decoded Hyperliquid HIP-4 binary-outcome action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HyperliquidOutcomeAction {
    /// Splits quote collateral into equal Yes and No outcome inventory.
    Split { outcome: u32, amount: Decimal },
    /// Merges equal Yes and No outcome inventory into quote collateral.
    Merge { outcome: u32, amount: Decimal },
}

/// Converts a generic binary-option action to its Hyperliquid HIP-4 representation.
///
/// # Errors
///
/// Returns an error if the pair is not the canonical Yes/No pair for one Hyperliquid outcome or
/// if Hyperliquid does not support the supplied venue options.
pub fn hyperliquid_outcome_action(
    action: &BinaryOptionAction,
) -> anyhow::Result<HyperliquidOutcomeAction> {
    action.validate()?;
    if let Some(params) = action.params()
        && !params.is_empty()
    {
        anyhow::bail!("Hyperliquid binary-option actions do not support venue parameters")
    }

    let outcome = outcome_from_pair(action.pair())?;
    match action {
        BinaryOptionAction::Split { quantity, .. } => Ok(HyperliquidOutcomeAction::Split {
            outcome,
            amount: quantity.as_decimal(),
        }),
        BinaryOptionAction::Merge { quantity, .. } => Ok(HyperliquidOutcomeAction::Merge {
            outcome,
            amount: quantity.as_decimal(),
        }),
    }
}

fn outcome_from_pair(pair: BinaryOptionPair) -> anyhow::Result<u32> {
    let yes = outcome_asset_id_from_instrument_id(pair.yes_instrument_id)?;
    let no = outcome_asset_id_from_instrument_id(pair.no_instrument_id)?;
    let yes_encoding = yes
        .outcome_encoding()
        .ok_or_else(|| anyhow::anyhow!("Binary option Yes leg is not a HIP-4 outcome"))?;
    let no_encoding = no
        .outcome_encoding()
        .ok_or_else(|| anyhow::anyhow!("Binary option No leg is not a HIP-4 outcome"))?;
    let outcome = yes_encoding / 10;

    anyhow::ensure!(
        yes_encoding % 10 == 0,
        "Binary option Yes leg is not the HIP-4 Yes outcome"
    );
    anyhow::ensure!(
        no_encoding == outcome * 10 + 1,
        "Binary option legs do not represent one HIP-4 outcome"
    );

    let [expected_yes, expected_no] = outcome_pair_instrument_ids(outcome)?;
    anyhow::ensure!(
        pair.yes_instrument_id == expected_yes && pair.no_instrument_id == expected_no,
        "Binary option pair is not canonical for Hyperliquid outcome {outcome}"
    );
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::execution::{BinaryOptionAction, BinaryOptionPair};
    use nautilus_model::{identifiers::InstrumentId, types::Quantity};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn pair() -> BinaryOptionPair {
        BinaryOptionPair::new(
            InstrumentId::from("42-YES-OUTCOME.HYPERLIQUID"),
            InstrumentId::from("42-NO-OUTCOME.HYPERLIQUID"),
        )
        .unwrap()
    }

    #[rstest]
    fn test_encodes_split_from_pair() {
        let action = hyperliquid_outcome_action(&BinaryOptionAction::Split {
            pair: pair(),
            quantity: Quantity::from("1.25"),
            params: None,
        })
        .unwrap();

        assert_eq!(
            action,
            HyperliquidOutcomeAction::Split {
                outcome: 42,
                amount: dec!(1.25),
            }
        );
    }

    #[rstest]
    fn test_encodes_merge_from_pair() {
        let action = hyperliquid_outcome_action(&BinaryOptionAction::Merge {
            pair: pair(),
            quantity: Quantity::from("1.25"),
            params: None,
        })
        .unwrap();

        assert_eq!(
            action,
            HyperliquidOutcomeAction::Merge {
                outcome: 42,
                amount: dec!(1.25),
            }
        );
    }

    #[rstest]
    fn test_rejects_noncanonical_pair() {
        let pair = BinaryOptionPair::new(
            InstrumentId::from("42-NO-OUTCOME.HYPERLIQUID"),
            InstrumentId::from("42-YES-OUTCOME.HYPERLIQUID"),
        )
        .unwrap();
        let err = hyperliquid_outcome_action(&BinaryOptionAction::Split {
            pair,
            quantity: Quantity::from("1.25"),
            params: None,
        })
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Binary option Yes leg is not the HIP-4 Yes outcome"
        );
    }
}
