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

//! The set of related prediction market legs that trade one event outcome.

use std::collections::HashSet;

use nautilus_core::UnixNanos;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    identifiers::{InstrumentId, OutcomeGroupId},
    types::Money,
};

/// Whether the venue proves that at most one leg of a group can win.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exclusivity {
    /// The venue contract enforces that at most one outcome can occur.
    Proven,
    /// The venue or its documentation asserts exclusivity without enforcement.
    Claimed,
    /// Exclusivity is unknown, so legs must be treated as independent.
    Unknown,
}

/// Whether the venue proves that the legs cover every possible outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exhaustiveness {
    /// The venue contract enforces that some leg must win.
    Proven,
    /// The venue or its documentation asserts exhaustiveness without enforcement.
    Claimed,
    /// Exhaustiveness is unknown, so a complete set cannot be assumed.
    Unknown,
}

/// One tradable leg of an outcome group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeLeg {
    /// The venue-assigned outcome label, such as `Yes`.
    pub outcome_id: Ustr,
    /// The tradable instrument for this outcome.
    pub instrument_id: InstrumentId,
    /// The amount paid per unit of the instrument if this outcome occurs.
    pub unit_payout: Money,
}

impl OutcomeLeg {
    /// Creates a new [`OutcomeLeg`] instance.
    #[must_use]
    pub fn new(outcome_id: Ustr, instrument_id: InstrumentId, unit_payout: Money) -> Self {
        Self {
            outcome_id,
            instrument_id,
            unit_payout,
        }
    }
}

/// A set of related prediction market legs with declared payout terms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeGroup {
    /// The venue-scoped group identity.
    pub group_id: OutcomeGroupId,
    /// The venue-assigned event identity, when the venue exposes one.
    pub event_id: Option<String>,
    /// The legs of the group, one per outcome.
    pub legs: Vec<OutcomeLeg>,
    /// Whether the legs are declared mutually exclusive.
    pub exclusivity: Exclusivity,
    /// Whether the legs are declared to cover every outcome.
    pub exhaustiveness: Exhaustiveness,
    /// The total paid per unit across a proven exclusive and exhaustive set.
    pub unit_total: Money,
    /// Version of the declared relationship, incremented when membership or payouts change.
    pub version: u32,
    /// Provenance of the declared relationship, such as the venue endpoint that supplied it.
    pub source: Option<Ustr>,
    /// UNIX timestamp (nanoseconds) when the group definition was last observed from the venue.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when this instance was initialized.
    pub ts_init: UnixNanos,
}

/// Error returned when an outcome group violates its declared payout terms.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OutcomeGroupError {
    /// The group declares no legs.
    #[error("outcome group '{group_id}' must declare at least one leg")]
    EmptyLegs {
        /// The group identity.
        group_id: String,
    },
    /// Two legs share an outcome label.
    #[error("outcome group '{group_id}' declares outcome '{outcome_id}' more than once")]
    DuplicateOutcome {
        /// The group identity.
        group_id: String,
        /// The repeated outcome label.
        outcome_id: String,
    },
    /// Two legs share an instrument.
    #[error("outcome group '{group_id}' declares instrument '{instrument_id}' more than once")]
    DuplicateInstrument {
        /// The group identity.
        group_id: String,
        /// The repeated instrument.
        instrument_id: String,
    },
    /// A leg pays in a currency other than the group's.
    #[error(
        "outcome group '{group_id}' leg '{outcome_id}' pays {payout_currency} but the group settles in {group_currency}"
    )]
    CurrencyMismatch {
        /// The group identity.
        group_id: String,
        /// The offending outcome label.
        outcome_id: String,
        /// The leg payout currency code.
        payout_currency: String,
        /// The group settlement currency code.
        group_currency: String,
    },
    /// A leg declares a negative payout.
    #[error("outcome group '{group_id}' leg '{outcome_id}' declares a negative payout")]
    NegativePayout {
        /// The group identity.
        group_id: String,
        /// The offending outcome label.
        outcome_id: String,
    },
    /// A proven exclusive and exhaustive set does not pay out its declared total.
    #[error(
        "outcome group '{group_id}' declares payouts totaling {total}, expected {expected} for a proven exclusive and exhaustive set"
    )]
    PayoutTotalMismatch {
        /// The group identity.
        group_id: String,
        /// The summed leg payouts.
        total: Decimal,
        /// The declared group total.
        expected: Decimal,
    },
}

impl OutcomeGroup {
    /// Creates a new [`OutcomeGroup`], validating its declared payout terms.
    ///
    /// # Errors
    ///
    /// Returns an error if the legs are empty, repeat an outcome or instrument, pay in another
    /// currency, pay a negative amount, or a proven exclusive and exhaustive set does not total
    /// [`OutcomeGroup::unit_total`].
    #[expect(clippy::too_many_arguments)]
    pub fn new_checked(
        group_id: OutcomeGroupId,
        event_id: Option<String>,
        legs: Vec<OutcomeLeg>,
        exclusivity: Exclusivity,
        exhaustiveness: Exhaustiveness,
        unit_total: Money,
        version: u32,
        source: Option<Ustr>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Result<Self, OutcomeGroupError> {
        let group = Self {
            group_id,
            event_id,
            legs,
            exclusivity,
            exhaustiveness,
            unit_total,
            version,
            source,
            ts_event,
            ts_init,
        };
        group.validate()?;

        Ok(group)
    }

    /// Validates the declared payout terms.
    ///
    /// Call this after deserializing a group, because deserialization bypasses construction.
    ///
    /// # Errors
    ///
    /// Returns an error if the legs are empty, repeat an outcome or instrument, pay in another
    /// currency, pay a negative amount, or a proven exclusive and exhaustive set does not total
    /// [`OutcomeGroup::unit_total`].
    pub fn validate(&self) -> Result<(), OutcomeGroupError> {
        let group_id = self.group_id.to_string();

        if self.legs.is_empty() {
            return Err(OutcomeGroupError::EmptyLegs { group_id });
        }

        let mut outcomes: HashSet<&str> = HashSet::new();
        let mut instruments: HashSet<InstrumentId> = HashSet::new();

        for leg in &self.legs {
            if !outcomes.insert(leg.outcome_id.as_str()) {
                return Err(OutcomeGroupError::DuplicateOutcome {
                    group_id,
                    outcome_id: leg.outcome_id.to_string(),
                });
            }

            if !instruments.insert(leg.instrument_id) {
                return Err(OutcomeGroupError::DuplicateInstrument {
                    group_id,
                    instrument_id: leg.instrument_id.to_string(),
                });
            }

            if leg.unit_payout.currency != self.unit_total.currency {
                return Err(OutcomeGroupError::CurrencyMismatch {
                    group_id,
                    outcome_id: leg.outcome_id.to_string(),
                    payout_currency: leg.unit_payout.currency.code.to_string(),
                    group_currency: self.unit_total.currency.code.to_string(),
                });
            }

            if leg.unit_payout.as_decimal() < Decimal::ZERO {
                return Err(OutcomeGroupError::NegativePayout {
                    group_id,
                    outcome_id: leg.outcome_id.to_string(),
                });
            }
        }

        if self.supports_complement_offsets() {
            let total = self.total_payout();
            let expected = self.unit_total.as_decimal();

            if total != expected {
                return Err(OutcomeGroupError::PayoutTotalMismatch {
                    group_id,
                    total,
                    expected,
                });
            }
        }

        Ok(())
    }

    /// Returns whether the group is proven mutually exclusive and exhaustive.
    ///
    /// Only such a group can offset one leg against another, because only then is one unit of
    /// payout guaranteed to be paid out across the set.
    #[must_use]
    pub fn supports_complement_offsets(&self) -> bool {
        self.exclusivity == Exclusivity::Proven && self.exhaustiveness == Exhaustiveness::Proven
    }

    /// Returns the summed per-unit payouts across the legs.
    #[must_use]
    pub fn total_payout(&self) -> Decimal {
        super::sum_payouts(self.legs.iter().map(|leg| &leg.unit_payout))
    }

    /// Returns the leg for the given outcome label, if the group declares one.
    #[must_use]
    pub fn leg(&self, outcome_id: &str) -> Option<&OutcomeLeg> {
        self.legs
            .iter()
            .find(|leg| leg.outcome_id.as_str() == outcome_id)
    }

    /// Returns whether the group declares the given outcome label.
    #[must_use]
    pub fn contains_outcome(&self, outcome_id: &str) -> bool {
        self.leg(outcome_id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{identifiers::OutcomeGroupId, types::Currency};

    fn group_id() -> OutcomeGroupId {
        OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap()
    }

    fn leg(outcome: &str, symbol: &str, payout: Money) -> OutcomeLeg {
        OutcomeLeg::new(
            Ustr::from(outcome),
            InstrumentId::from(format!("{symbol}.POLYMARKET").as_str()),
            payout,
        )
    }

    fn three_leg_group() -> OutcomeGroup {
        OutcomeGroup::new_checked(
            group_id(),
            Some("0xEVENT".to_string()),
            vec![
                leg("ALPHA", "ALPHA", Money::from("1.00 USDC")),
                leg("BETA", "BETA", Money::from("0.00 USDC")),
                leg("GAMMA", "GAMMA", Money::from("0.00 USDC")),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            Some(Ustr::from("gamma")),
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap()
    }

    #[rstest]
    fn test_three_leg_group_round_trips() {
        let group = three_leg_group();

        let json = serde_json::to_string(&group).unwrap();
        let restored: OutcomeGroup = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, group);
        assert_eq!(restored.total_payout(), dec!(1.00));
        assert!(restored.supports_complement_offsets());
        assert!(restored.contains_outcome("BETA"));
    }

    #[rstest]
    fn test_binary_group_round_trips() {
        let group = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![
                leg("Yes", "0xYES", Money::from("1.00 USDC")),
                leg("No", "0xNO", Money::from("0.00 USDC")),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap();

        let json = serde_json::to_string(&group).unwrap();
        let restored: OutcomeGroup = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, group);
        assert_eq!(restored.legs.len(), 2);
    }

    #[rstest]
    fn test_empty_legs_rejected() {
        let error = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap_err();

        assert!(matches!(error, OutcomeGroupError::EmptyLegs { .. }));
    }

    #[rstest]
    fn test_duplicate_outcome_rejected() {
        let error = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![
                leg("Yes", "0xYES", Money::from("1.00 USDC")),
                leg("Yes", "0xNO", Money::from("0.00 USDC")),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap_err();

        assert!(matches!(error, OutcomeGroupError::DuplicateOutcome { .. }));
    }

    #[rstest]
    fn test_duplicate_instrument_rejected() {
        let error = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![
                leg("Yes", "0xYES", Money::from("1.00 USDC")),
                leg("No", "0xYES", Money::from("0.00 USDC")),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OutcomeGroupError::DuplicateInstrument { .. }
        ));
    }

    #[rstest]
    fn test_currency_mismatch_rejected() {
        let error = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![
                leg("Yes", "0xYES", Money::from("1.00 USDC")),
                leg("No", "0xNO", Money::from("0.00 USD")),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::new(1.0, Currency::USDC()),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap_err();

        assert!(matches!(error, OutcomeGroupError::CurrencyMismatch { .. }));
    }

    #[rstest]
    fn test_negative_payout_rejected() {
        let error = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![leg("Yes", "0xYES", Money::from("-1.00 USDC"))],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap_err();

        assert!(matches!(error, OutcomeGroupError::NegativePayout { .. }));
    }

    #[rstest]
    fn test_proven_exhaustive_payout_total_mismatch_rejected() {
        let error = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![
                leg("Yes", "0xYES", Money::from("0.60 USDC")),
                leg("No", "0xNO", Money::from("0.00 USDC")),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            OutcomeGroupError::PayoutTotalMismatch { .. }
        ));
    }

    #[rstest]
    fn test_claimed_exhaustive_payout_total_is_not_assumed() {
        let group = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![
                leg("Yes", "0xYES", Money::from("0.60 USDC")),
                leg("No", "0xNO", Money::from("0.00 USDC")),
            ],
            Exclusivity::Claimed,
            Exhaustiveness::Claimed,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap();

        assert!(!group.supports_complement_offsets());
        assert_eq!(group.total_payout(), dec!(0.60));
    }

    #[rstest]
    #[case(Exclusivity::Proven, Exhaustiveness::Claimed)]
    #[case(Exclusivity::Claimed, Exhaustiveness::Proven)]
    #[case(Exclusivity::Unknown, Exhaustiveness::Unknown)]
    fn test_complement_offsets_require_both_proven(
        #[case] exclusivity: Exclusivity,
        #[case] exhaustiveness: Exhaustiveness,
    ) {
        let group = OutcomeGroup::new_checked(
            group_id(),
            None,
            vec![leg("Yes", "0xYES", Money::from("1.00 USDC"))],
            exclusivity,
            exhaustiveness,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap();

        assert!(!group.supports_complement_offsets());
    }

    #[rstest]
    fn test_validate_catches_deserialized_violation() {
        let mut group = three_leg_group();
        group.legs[1].outcome_id = group.legs[0].outcome_id;

        assert!(matches!(
            group.validate().unwrap_err(),
            OutcomeGroupError::DuplicateOutcome { .. }
        ));
    }
}
