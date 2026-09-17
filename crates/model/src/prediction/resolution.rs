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

//! The authoritative outcome of a prediction market group, with provenance.

use std::collections::HashSet;

use nautilus_core::UnixNanos;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{OutcomeGroup, sum_payouts};
use crate::{
    identifiers::{InstrumentId, OutcomeGroupId, Venue},
    types::Money,
};

/// The amount paid per unit for one outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomePayout {
    /// The outcome label this payout applies to.
    pub outcome_id: Ustr,
    /// The amount paid per unit of the corresponding instrument.
    pub payout_per_unit: Money,
}

impl OutcomePayout {
    /// Creates a new [`OutcomePayout`] instance.
    #[must_use]
    pub fn new(outcome_id: Ustr, payout_per_unit: Money) -> Self {
        Self {
            outcome_id,
            payout_per_unit,
        }
    }
}

/// Where an authoritative outcome came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionSource {
    /// The venue the resolution applies to.
    pub venue: Venue,
    /// The oracle or report identity, such as a UMA request ID.
    pub reference: String,
    /// A reference to the published outcome, when the venue exposes one.
    pub url: Option<String>,
}

impl ResolutionSource {
    /// Creates a new [`ResolutionSource`] instance.
    #[must_use]
    pub fn new(venue: Venue, reference: &str, url: Option<&str>) -> Self {
        Self {
            venue,
            reference: reference.to_string(),
            url: url.map(str::to_string),
        }
    }
}

/// The terminal payout state of a group.
///
/// Pending and disputed are modeled here rather than beside the payouts, so no instance can carry
/// payouts that must not be applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionOutcome {
    /// Settlement pays the listed amounts per unit. Legs absent from the list pay zero.
    Payouts(Vec<OutcomePayout>),
    /// The market voided. Every leg settles at the given amount per unit.
    Void {
        /// The amount returned per unit of every instrument.
        payout_per_unit: Money,
    },
    /// No authoritative outcome is available yet.
    Pending,
    /// The outcome is contested. Automatic application must not proceed.
    Disputed,
}

impl ResolutionOutcome {
    /// Returns the name of this state, for diagnostics and errors.
    #[must_use]
    pub const fn state(&self) -> &'static str {
        match self {
            Self::Payouts(_) => "payouts",
            Self::Void { .. } => "void",
            Self::Pending => "pending",
            Self::Disputed => "disputed",
        }
    }

    /// Returns whether this outcome carries payouts that may be applied.
    #[must_use]
    pub const fn is_applicable(&self) -> bool {
        matches!(self, Self::Payouts(_) | Self::Void { .. })
    }
}

/// An authoritative outcome for one version of one outcome group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketResolution {
    /// The group the outcome applies to.
    pub group_id: OutcomeGroupId,
    /// The group version the outcome was resolved against.
    pub version: u32,
    /// Where the outcome came from.
    pub source: ResolutionSource,
    /// The terminal payout state.
    pub outcome: ResolutionOutcome,
    /// UNIX timestamp (nanoseconds) the venue's outcome took effect.
    pub effective_ns: UnixNanos,
    /// UNIX timestamp (nanoseconds) the outcome was observed locally.
    pub observed_ns: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the resolution event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when this instance was initialized.
    pub ts_init: UnixNanos,
}

impl crate::data::HasTsInit for MarketResolution {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

/// Error returned when a resolution cannot be mapped onto an outcome group.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ResolutionError {
    /// The resolution names a different group than the one supplied.
    #[error("resolution for group '{resolution_group_id}' does not apply to group '{group_id}'")]
    GroupMismatch {
        /// The group identity supplied by the caller.
        group_id: String,
        /// The group identity named by the resolution.
        resolution_group_id: String,
    },
    /// The resolution was resolved against a different group version.
    #[error(
        "resolution for group '{group_id}' targets version {resolution_version} but the group is version {group_version}"
    )]
    GroupVersionMismatch {
        /// The group identity.
        group_id: String,
        /// The version named by the resolution.
        resolution_version: u32,
        /// The version of the supplied group.
        group_version: u32,
    },
    /// The resolution names an outcome the group does not declare.
    #[error("resolution for group '{group_id}' names unknown outcome '{outcome_id}'")]
    UnknownOutcome {
        /// The group identity.
        group_id: String,
        /// The unknown outcome label.
        outcome_id: String,
    },
    /// The resolution names an outcome more than once.
    #[error("resolution for group '{group_id}' names outcome '{outcome_id}' more than once")]
    DuplicateOutcome {
        /// The group identity.
        group_id: String,
        /// The repeated outcome label.
        outcome_id: String,
    },
    /// The resolution pays in a currency other than the group's.
    #[error(
        "resolution for group '{group_id}' pays {payout_currency} but the group settles in {group_currency}"
    )]
    CurrencyMismatch {
        /// The group identity.
        group_id: String,
        /// The outcome label or state that declared the payout.
        outcome_id: String,
        /// The resolution payout currency code.
        payout_currency: String,
        /// The group settlement currency code.
        group_currency: String,
    },
    /// The resolution declares a negative payout.
    #[error("resolution for group '{group_id}' declares a negative payout for '{outcome_id}'")]
    NegativePayout {
        /// The group identity.
        group_id: String,
        /// The outcome label or state that declared the payout.
        outcome_id: String,
    },
    /// A proven exclusive and exhaustive set is not fully paid out by the resolution.
    #[error(
        "resolution for group '{group_id}' pays out {total}, expected {expected} for a proven exclusive and exhaustive set"
    )]
    PayoutTotalMismatch {
        /// The group identity.
        group_id: String,
        /// The summed resolution payouts.
        total: Decimal,
        /// The declared group total.
        expected: Decimal,
    },
    /// The resolution carries no payout that may be applied.
    #[error("resolution for group '{group_id}' is not applicable in state '{state}'")]
    NotApplicable {
        /// The group identity.
        group_id: String,
        /// The terminal state name.
        state: &'static str,
    },
}

impl MarketResolution {
    /// Returns the identity under which a settlement applies exactly once.
    ///
    /// Applying two resolutions with the same key must not credit a payout twice.
    #[must_use]
    pub fn settlement_key(&self) -> (OutcomeGroupId, u32) {
        (self.group_id.clone(), self.version)
    }

    /// Returns the terminal payout per unit for each leg instrument of `group`.
    ///
    /// This is the join a settlement path needs: the resolution speaks in outcomes, while the
    /// engine settles instruments.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`MarketResolution::payouts_for`].
    pub fn instrument_payouts_for(
        &self,
        group: &OutcomeGroup,
    ) -> Result<Vec<(InstrumentId, Money)>, ResolutionError> {
        let payouts = self.payouts_for(group)?;
        let mut legs = Vec::with_capacity(payouts.len());

        for payout in payouts {
            let leg = group.leg(payout.outcome_id.as_str()).ok_or_else(|| {
                ResolutionError::UnknownOutcome {
                    group_id: self.group_id.to_string(),
                    outcome_id: payout.outcome_id.to_string(),
                }
            })?;
            legs.push((leg.instrument_id, payout.payout_per_unit));
        }

        Ok(legs)
    }

    /// Returns whether the resolution carries payouts that may be applied.
    #[must_use]
    pub const fn is_applicable(&self) -> bool {
        self.outcome.is_applicable()
    }

    /// Maps the resolution onto one payout per leg of `group`, including zero-payout legs.
    ///
    /// # Errors
    ///
    /// Returns an error if the resolution names another group or version, names an outcome the
    /// group does not declare, repeats an outcome, pays in another currency, pays a negative
    /// amount, does not pay out the declared total of a proven exclusive and exhaustive set, or
    /// carries no payout at all because the outcome is pending or disputed.
    pub fn payouts_for(&self, group: &OutcomeGroup) -> Result<Vec<OutcomePayout>, ResolutionError> {
        let group_id = self.group_id.to_string();

        if self.group_id != group.group_id {
            return Err(ResolutionError::GroupMismatch {
                group_id,
                resolution_group_id: group.group_id.to_string(),
            });
        }

        if self.version != group.version {
            return Err(ResolutionError::GroupVersionMismatch {
                group_id,
                resolution_version: self.version,
                group_version: group.version,
            });
        }

        if !self.is_applicable() {
            return Err(ResolutionError::NotApplicable {
                group_id,
                state: self.outcome.state(),
            });
        }

        let mut payouts = Vec::with_capacity(group.legs.len());

        match &self.outcome {
            ResolutionOutcome::Void { payout_per_unit } => {
                self.check_payout(&group_id, "void", payout_per_unit, group)?;

                for leg in &group.legs {
                    payouts.push(OutcomePayout::new(leg.outcome_id, payout_per_unit.clone()));
                }
            }
            ResolutionOutcome::Payouts(declared) => {
                let mut seen: HashSet<&str> = HashSet::new();

                for payout in declared {
                    let outcome_id = payout.outcome_id.as_str();

                    if !group.contains_outcome(outcome_id) {
                        return Err(ResolutionError::UnknownOutcome {
                            group_id,
                            outcome_id: outcome_id.to_string(),
                        });
                    }

                    if !seen.insert(outcome_id) {
                        return Err(ResolutionError::DuplicateOutcome {
                            group_id,
                            outcome_id: outcome_id.to_string(),
                        });
                    }

                    self.check_payout(&group_id, outcome_id, &payout.payout_per_unit, group)?;
                }

                if group.supports_complement_offsets() {
                    let total = sum_payouts(declared.iter().map(|payout| &payout.payout_per_unit));
                    let expected = group.unit_total.as_decimal();

                    if total != expected {
                        return Err(ResolutionError::PayoutTotalMismatch {
                            group_id,
                            total,
                            expected,
                        });
                    }
                }

                let zero = Money::zero(group.unit_total.currency);

                for leg in &group.legs {
                    let payout = declared
                        .iter()
                        .find(|payout| payout.outcome_id == leg.outcome_id)
                        .map_or_else(|| zero.clone(), |payout| payout.payout_per_unit.clone());
                    payouts.push(OutcomePayout::new(leg.outcome_id, payout));
                }
            }
            ResolutionOutcome::Pending | ResolutionOutcome::Disputed => unreachable!(
                "pending and disputed resolutions are rejected before payout construction"
            ),
        }

        Ok(payouts)
    }

    fn check_payout(
        &self,
        group_id: &str,
        outcome_id: &str,
        payout: &Money,
        group: &OutcomeGroup,
    ) -> Result<(), ResolutionError> {
        if payout.currency != group.unit_total.currency {
            return Err(ResolutionError::CurrencyMismatch {
                group_id: group_id.to_string(),
                outcome_id: outcome_id.to_string(),
                payout_currency: payout.currency.code.to_string(),
                group_currency: group.unit_total.currency.code.to_string(),
            });
        }

        if payout.as_decimal() < Decimal::ZERO {
            return Err(ResolutionError::NegativePayout {
                group_id: group_id.to_string(),
                outcome_id: outcome_id.to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::{
        identifiers::{InstrumentId, OutcomeGroupId},
        prediction::{Exclusivity, Exhaustiveness, OutcomeLeg},
    };

    fn group() -> OutcomeGroup {
        OutcomeGroup::new_checked(
            OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap(),
            Some("0xEVENT".to_string()),
            vec![
                OutcomeLeg::new(
                    Ustr::from("Yes"),
                    InstrumentId::from("0xYES.POLYMARKET"),
                    Money::from("1.00 USDC"),
                ),
                OutcomeLeg::new(
                    Ustr::from("No"),
                    InstrumentId::from("0xNO.POLYMARKET"),
                    Money::from("0.00 USDC"),
                ),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            Money::from("1.00 USDC"),
            1,
            None,
            UnixNanos::new(1),
            UnixNanos::new(2),
        )
        .unwrap()
    }

    fn resolution(outcome: ResolutionOutcome) -> MarketResolution {
        MarketResolution {
            group_id: OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap(),
            version: 1,
            source: ResolutionSource::new(
                Venue::from("POLYMARKET"),
                "uma-request-1",
                Some("https://example.com/outcome"),
            ),
            outcome,
            effective_ns: UnixNanos::new(10),
            observed_ns: UnixNanos::new(11),
            ts_event: UnixNanos::new(10),
            ts_init: UnixNanos::new(11),
        }
    }

    fn settled(winner: &str) -> MarketResolution {
        resolution(ResolutionOutcome::Payouts(vec![
            OutcomePayout::new(Ustr::from(winner), Money::from("1.00 USDC")),
            OutcomePayout::new(Ustr::from("No"), Money::from("0.00 USDC")),
        ]))
    }

    #[rstest]
    fn test_winner_takes_full_payout_and_loser_takes_zero() {
        let payouts = settled("Yes").payouts_for(&group()).unwrap();

        assert_eq!(payouts.len(), 2);
        assert_eq!(payouts[0].outcome_id, Ustr::from("Yes"));
        assert_eq!(payouts[0].payout_per_unit, Money::from("1.00 USDC"));
        assert_eq!(payouts[1].outcome_id, Ustr::from("No"));
        assert_eq!(payouts[1].payout_per_unit, Money::from("0.00 USDC"));
    }

    #[rstest]
    fn test_unlisted_legs_pay_zero() {
        let resolution = resolution(ResolutionOutcome::Payouts(vec![OutcomePayout::new(
            Ustr::from("No"),
            Money::from("1.00 USDC"),
        )]));

        let payouts = resolution.payouts_for(&group()).unwrap();

        assert_eq!(payouts[0].payout_per_unit, Money::from("0.00 USDC"));
        assert_eq!(payouts[1].payout_per_unit, Money::from("1.00 USDC"));
    }

    #[rstest]
    fn test_fractional_split_payouts_accepted() {
        let resolution = resolution(ResolutionOutcome::Payouts(vec![
            OutcomePayout::new(Ustr::from("Yes"), Money::from("0.50 USDC")),
            OutcomePayout::new(Ustr::from("No"), Money::from("0.50 USDC")),
        ]));

        let payouts = resolution.payouts_for(&group()).unwrap();

        assert_eq!(payouts[0].payout_per_unit, Money::from("0.50 USDC"));
        assert_eq!(payouts[1].payout_per_unit, Money::from("0.50 USDC"));
    }

    #[rstest]
    fn test_void_pays_every_leg() {
        let resolution = resolution(ResolutionOutcome::Void {
            payout_per_unit: Money::from("0.50 USDC"),
        });

        let payouts = resolution.payouts_for(&group()).unwrap();

        assert_eq!(payouts.len(), 2);
        assert_eq!(payouts[0].payout_per_unit, Money::from("0.50 USDC"));
        assert_eq!(payouts[1].payout_per_unit, Money::from("0.50 USDC"));
    }

    #[rstest]
    #[case(ResolutionOutcome::Pending)]
    #[case(ResolutionOutcome::Disputed)]
    fn test_pending_and_disputed_are_not_applied(#[case] outcome: ResolutionOutcome) {
        let resolution = resolution(outcome);
        let state = resolution.outcome.state();

        assert!(!resolution.is_applicable());
        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::NotApplicable { state: error_state, .. } if error_state == state
        ));
    }

    #[rstest]
    fn test_unknown_outcome_rejected() {
        let resolution = resolution(ResolutionOutcome::Payouts(vec![OutcomePayout::new(
            Ustr::from("Maybe"),
            Money::from("1.00 USDC"),
        )]));

        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::UnknownOutcome { .. }
        ));
    }

    #[rstest]
    fn test_duplicate_outcome_rejected() {
        let resolution = resolution(ResolutionOutcome::Payouts(vec![
            OutcomePayout::new(Ustr::from("Yes"), Money::from("0.50 USDC")),
            OutcomePayout::new(Ustr::from("Yes"), Money::from("0.50 USDC")),
        ]));

        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::DuplicateOutcome { .. }
        ));
    }

    #[rstest]
    fn test_currency_mismatch_rejected() {
        let resolution = resolution(ResolutionOutcome::Payouts(vec![
            OutcomePayout::new(Ustr::from("Yes"), Money::from("1.00 USD")),
            OutcomePayout::new(Ustr::from("No"), Money::from("0.00 USD")),
        ]));

        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::CurrencyMismatch { .. }
        ));
    }

    #[rstest]
    fn test_negative_payout_rejected() {
        let resolution = resolution(ResolutionOutcome::Payouts(vec![OutcomePayout::new(
            Ustr::from("Yes"),
            Money::from("-1.00 USDC"),
        )]));

        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::NegativePayout { .. }
        ));
    }

    #[rstest]
    fn test_incomplete_payout_of_proven_set_rejected() {
        let resolution = resolution(ResolutionOutcome::Payouts(vec![OutcomePayout::new(
            Ustr::from("Yes"),
            Money::from("0.60 USDC"),
        )]));

        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::PayoutTotalMismatch { .. }
        ));
    }

    #[rstest]
    fn test_group_version_mismatch_rejected() {
        let mut resolution = settled("Yes");
        resolution.version = 2;

        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::GroupVersionMismatch {
                resolution_version: 2,
                group_version: 1,
                ..
            }
        ));
    }

    #[rstest]
    fn test_group_mismatch_rejected() {
        let mut resolution = settled("Yes");
        resolution.group_id = OutcomeGroupId::new_checked("POLYMARKET", "0xOTHER").unwrap();

        assert!(matches!(
            resolution.payouts_for(&group()).unwrap_err(),
            ResolutionError::GroupMismatch { .. }
        ));
    }

    #[rstest]
    fn test_instrument_payouts_join_outcomes_to_leg_instruments() {
        let payouts = settled("Yes").instrument_payouts_for(&group()).unwrap();

        assert_eq!(
            payouts,
            vec![
                (
                    InstrumentId::from("0xYES.POLYMARKET"),
                    Money::from("1.00 USDC")
                ),
                (
                    InstrumentId::from("0xNO.POLYMARKET"),
                    Money::from("0.00 USDC")
                ),
            ]
        );
    }

    #[rstest]
    fn test_instrument_payouts_reject_non_applicable_resolution() {
        let resolution = resolution(ResolutionOutcome::Pending);

        assert!(matches!(
            resolution.instrument_payouts_for(&group()).unwrap_err(),
            ResolutionError::NotApplicable { .. }
        ));
    }

    #[rstest]
    fn test_settlement_key_is_stable_for_idempotent_application() {
        let group_id = OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap();
        let first = settled("Yes");
        let second = settled("Yes");

        assert_eq!(first.settlement_key(), second.settlement_key());
        assert_eq!(first.settlement_key(), (group_id, 1));
    }

    #[rstest]
    fn test_serialization_round_trip() {
        let resolution = settled("Yes");

        let json = serde_json::to_string(&resolution).unwrap();
        let restored: MarketResolution = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, resolution);
    }
}
