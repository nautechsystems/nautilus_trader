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

//! Prediction market outcome groups and resolution contracts.
//!
//! A prediction market event trades as one or more *outcome groups*. Each group carries one leg per
//! outcome, and each leg declares the amount paid per unit if that outcome occurs. Group identity,
//! event identity, and instrument identity stay separate, because one event can carry several
//! groups and one group carries one instrument per outcome.
//!
//! # Declared versus proven relationships
//!
//! Knowing that two legs share an event does not prove they are mutually exclusive or that they
//! cover every outcome. [`Exclusivity`] and [`Exhaustiveness`] record which of those claims the
//! venue actually enforces. Only a group proven on both can offset legs against each other, which
//! [`OutcomeGroup::supports_complement_offsets`] reports.
//!
//! # Resolution
//!
//! [`MarketResolution`] carries the authoritative outcome together with its provenance. It maps
//! onto concrete per-leg payouts through [`MarketResolution::payouts_for`], and an outcome that is
//! pending or disputed fails explicitly rather than settling at an assumed winner.

pub mod group;
pub mod resolution;

pub use group::{Exclusivity, Exhaustiveness, OutcomeGroup, OutcomeGroupError, OutcomeLeg};
pub use resolution::{
    MarketResolution, OutcomePayout, ResolutionError, ResolutionOutcome, ResolutionSource,
};
use rust_decimal::Decimal;

use crate::types::Money;

/// Sums per-unit payouts, which callers must already have checked share one currency.
pub(crate) fn sum_payouts<'a, I>(payouts: I) -> Decimal
where
    I: IntoIterator<Item = &'a Money>,
{
    payouts
        .into_iter()
        .fold(Decimal::ZERO, |total, payout| total + payout.as_decimal())
}
