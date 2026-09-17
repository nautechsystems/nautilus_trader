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

//! Bounded scenario exposure for prediction market outcome groups.
//!
//! An outcome group declares how its legs pay out, but holding several legs only reduces risk
//! when the venue proves a relationship strong enough to net them. This module turns per-leg
//! worst-case quantities into one bounded loss figure that a risk limit can compare against.

use nautilus_model::{identifiers::InstrumentId, prediction::OutcomeGroup, types::Money};

/// The worst-case exposure inputs for one leg of an outcome group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupLegExposure {
    /// The leg instrument.
    pub instrument_id: InstrumentId,
    /// Worst-case cash required to acquire the leg's net long quantity.
    pub long_notional: Money,
    /// Worst-case cash the leg's net short quantity can owe at a full payout.
    pub short_notional: Money,
    /// Worst-case cash the leg's net long quantity receives when this leg wins.
    pub long_payout: Money,
}

/// The bounded worst-case exposure of a basket over an outcome group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupExposure {
    /// Worst-case cash at risk across the group's legs.
    pub outlay: Money,
    /// Guaranteed payout floor across the group's declared scenarios.
    pub payout_floor: Money,
}

impl GroupExposure {
    /// Returns the exposure that the payout floor does not cover, never below zero.
    #[must_use]
    pub fn net(&self) -> Money {
        if self.outlay <= self.payout_floor {
            Money::zero(self.outlay.currency)
        } else {
            self.outlay - self.payout_floor
        }
    }
}

/// Computes the bounded worst-case exposure of a basket over `group`.
///
/// Every declared leg contributes its outlay. The payout floor is the smallest long payout across
/// the group's declared scenarios, and is zero unless the group proves an exclusive and exhaustive
/// relationship. Declared legs absent from `legs` contribute zero, so holding a subset of a proven
/// set cannot claim a complement offset. A short leg adds the amount it can owe without granting
/// credit.
///
/// Returns `None` when a leg settles in another currency than the group, because legs in separate
/// collateral pools cannot be netted.
#[must_use]
pub fn compute_group_exposure(
    group: &OutcomeGroup,
    legs: &[GroupLegExposure],
) -> Option<GroupExposure> {
    let currency = group.unit_total.currency;
    let proves_offsets = group.supports_complement_offsets();
    let mut outlay = Money::zero(currency);
    let mut floor: Option<Money> = None;

    for leg in &group.legs {
        let (long_notional, short_notional, long_payout) = match legs
            .iter()
            .find(|exposure| exposure.instrument_id == leg.instrument_id)
        {
            Some(exposure) => {
                if exposure.long_notional.currency != currency
                    || exposure.short_notional.currency != currency
                    || exposure.long_payout.currency != currency
                {
                    return None;
                }
                (
                    exposure.long_notional,
                    exposure.short_notional,
                    exposure.long_payout,
                )
            }
            None => (
                Money::zero(currency),
                Money::zero(currency),
                Money::zero(currency),
            ),
        };

        outlay = outlay + long_notional + short_notional;

        if proves_offsets {
            floor = Some(match floor {
                Some(current) if current <= long_payout => current,
                _ => long_payout,
            });
        }
    }

    Some(GroupExposure {
        outlay,
        payout_floor: floor.unwrap_or_else(|| Money::zero(currency)),
    })
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::OutcomeGroupId,
        prediction::{Exclusivity, Exhaustiveness, OutcomeGroup, OutcomeLeg},
        types::Money,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    fn leg(instrument_id: &str, payout: &str) -> OutcomeLeg {
        OutcomeLeg::new(
            Ustr::from(instrument_id),
            InstrumentId::from(instrument_id),
            Money::from(payout),
        )
    }

    fn exposure(instrument_id: &str, notional: &str, payout: &str) -> GroupLegExposure {
        GroupLegExposure {
            instrument_id: InstrumentId::from(instrument_id),
            long_notional: Money::from(notional),
            short_notional: Money::zero(Money::from(payout).currency),
            long_payout: Money::from(payout),
        }
    }

    fn group(
        legs: Vec<OutcomeLeg>,
        exclusivity: Exclusivity,
        exhaustiveness: Exhaustiveness,
        unit_total: &str,
    ) -> OutcomeGroup {
        OutcomeGroup::new_checked(
            OutcomeGroupId::new_checked("POLYMARKET", "0xCONDITION").unwrap(),
            None,
            legs,
            exclusivity,
            exhaustiveness,
            Money::from(unit_total),
            1,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
        )
        .unwrap()
    }

    fn proven_binary() -> OutcomeGroup {
        group(
            vec![
                leg("BINARY-YES.POLYMARKET", "1.00 USDC"),
                leg("BINARY-NO.POLYMARKET", "1.00 USDC"),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            "2.00 USDC",
        )
    }

    #[rstest]
    fn test_complement_basket_has_no_net_exposure() {
        let group = proven_binary();
        let legs = vec![
            exposure("BINARY-YES.POLYMARKET", "35.00 USDC", "100.00 USDC"),
            exposure("BINARY-NO.POLYMARKET", "65.00 USDC", "100.00 USDC"),
        ];

        let exposure = compute_group_exposure(&group, &legs).unwrap();

        assert_eq!(exposure.outlay, Money::from("100.00 USDC"));
        assert_eq!(exposure.payout_floor, Money::from("100.00 USDC"));
        assert_eq!(
            exposure.net(),
            Money::zero(Money::from("0.00 USDC").currency)
        );
    }

    #[rstest]
    fn test_one_sided_basket_grants_no_offset_credit() {
        let group = proven_binary();
        let legs = vec![exposure(
            "BINARY-YES.POLYMARKET",
            "35.00 USDC",
            "100.00 USDC",
        )];

        let exposure = compute_group_exposure(&group, &legs).unwrap();

        assert_eq!(exposure.outlay, Money::from("35.00 USDC"));
        assert_eq!(exposure.payout_floor, Money::from("0.00 USDC"));
        assert_eq!(exposure.net(), Money::from("35.00 USDC"));
    }

    #[rstest]
    #[case::claimed_exclusivity(Exclusivity::Claimed, Exhaustiveness::Proven)]
    #[case::unknown_exclusivity(Exclusivity::Unknown, Exhaustiveness::Proven)]
    #[case::claimed_exhaustiveness(Exclusivity::Proven, Exhaustiveness::Claimed)]
    #[case::unknown_exhaustiveness(Exclusivity::Proven, Exhaustiveness::Unknown)]
    fn test_unproven_relationships_grants_no_offset_credit(
        #[case] exclusivity: Exclusivity,
        #[case] exhaustiveness: Exhaustiveness,
    ) {
        // A claimed relationship cannot be validated as a complete payout set, so the group
        // declares no unit total.
        let group = group(
            vec![
                leg("BINARY-YES.POLYMARKET", "1.00 USDC"),
                leg("BINARY-NO.POLYMARKET", "1.00 USDC"),
            ],
            exclusivity,
            exhaustiveness,
            "1.00 USDC",
        );
        let legs = vec![
            exposure("BINARY-YES.POLYMARKET", "35.00 USDC", "100.00 USDC"),
            exposure("BINARY-NO.POLYMARKET", "65.00 USDC", "100.00 USDC"),
        ];

        let exposure = compute_group_exposure(&group, &legs).unwrap();

        assert_eq!(exposure.outlay, Money::from("100.00 USDC"));
        assert_eq!(exposure.payout_floor, Money::from("0.00 USDC"));
        assert_eq!(exposure.net(), Money::from("100.00 USDC"));
    }

    #[rstest]
    fn test_three_leg_basket_floor_is_smallest_scenario_payout() {
        let group = group(
            vec![
                leg("RACE-A.POLYMARKET", "0.50 USDC"),
                leg("RACE-B.POLYMARKET", "0.30 USDC"),
                leg("RACE-C.POLYMARKET", "0.20 USDC"),
            ],
            Exclusivity::Proven,
            Exhaustiveness::Proven,
            "1.00 USDC",
        );
        let legs = vec![
            exposure("RACE-A.POLYMARKET", "50.00 USDC", "100.00 USDC"),
            exposure("RACE-B.POLYMARKET", "30.00 USDC", "60.00 USDC"),
            exposure("RACE-C.POLYMARKET", "20.00 USDC", "40.00 USDC"),
        ];

        let exposure = compute_group_exposure(&group, &legs).unwrap();

        assert_eq!(exposure.outlay, Money::from("100.00 USDC"));
        assert_eq!(exposure.payout_floor, Money::from("40.00 USDC"));
        assert_eq!(exposure.net(), Money::from("60.00 USDC"));
    }

    #[rstest]
    fn test_short_leg_adds_outlay_without_credit() {
        let group = proven_binary();
        let legs = vec![
            GroupLegExposure {
                instrument_id: InstrumentId::from("BINARY-YES.POLYMARKET"),
                long_notional: Money::from("35.00 USDC"),
                short_notional: Money::from("20.00 USDC"),
                long_payout: Money::from("100.00 USDC"),
            },
            exposure("BINARY-NO.POLYMARKET", "65.00 USDC", "100.00 USDC"),
        ];

        let exposure = compute_group_exposure(&group, &legs).unwrap();

        assert_eq!(exposure.outlay, Money::from("120.00 USDC"));
        assert_eq!(exposure.payout_floor, Money::from("100.00 USDC"));
        assert_eq!(exposure.net(), Money::from("20.00 USDC"));
    }

    #[rstest]
    fn test_other_settlement_currency_returns_none() {
        let group = proven_binary();
        let legs = vec![
            exposure("BINARY-YES.POLYMARKET", "35.00 USD", "100.00 USD"),
            exposure("BINARY-NO.POLYMARKET", "65.00 USDC", "100.00 USDC"),
        ];

        assert_eq!(compute_group_exposure(&group, &legs), None);
    }
}
