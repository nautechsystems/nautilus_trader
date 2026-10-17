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

use std::hash::Hash;

use ahash::AHashSet;
use nautilus_core::Params;
use nautilus_model::{
    data::BarType,
    identifiers::{ClientId, InstrumentId},
    instruments::InstrumentAny,
};
use serde::{Deserialize, de::Error};

use crate::cache::Cache;

/// Converts a single instrument ID or collection to a non-empty list.
///
/// Duplicate IDs are removed in first-occurrence order.
pub trait IntoInstrumentIds: sealed::Sealed {
    /// Returns the normalized instrument IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection is empty.
    fn into_instrument_ids(self) -> anyhow::Result<Vec<InstrumentId>>;
}

impl IntoInstrumentIds for InstrumentId {
    fn into_instrument_ids(self) -> anyhow::Result<Vec<InstrumentId>> {
        Ok(vec![self])
    }
}

impl IntoInstrumentIds for &InstrumentId {
    fn into_instrument_ids(self) -> anyhow::Result<Vec<InstrumentId>> {
        (*self).into_instrument_ids()
    }
}

impl IntoInstrumentIds for Vec<InstrumentId> {
    fn into_instrument_ids(mut self) -> anyhow::Result<Vec<InstrumentId>> {
        anyhow::ensure!(!self.is_empty(), "instrument IDs cannot be empty");

        deduplicate(&mut self);

        Ok(self)
    }
}

impl IntoInstrumentIds for &Vec<InstrumentId> {
    fn into_instrument_ids(self) -> anyhow::Result<Vec<InstrumentId>> {
        self.as_slice().into_instrument_ids()
    }
}

impl IntoInstrumentIds for &[InstrumentId] {
    fn into_instrument_ids(self) -> anyhow::Result<Vec<InstrumentId>> {
        self.to_vec().into_instrument_ids()
    }
}

impl<const N: usize> IntoInstrumentIds for [InstrumentId; N] {
    fn into_instrument_ids(self) -> anyhow::Result<Vec<InstrumentId>> {
        Vec::from(self).into_instrument_ids()
    }
}

impl<const N: usize> IntoInstrumentIds for &[InstrumentId; N] {
    fn into_instrument_ids(self) -> anyhow::Result<Vec<InstrumentId>> {
        self.as_slice().into_instrument_ids()
    }
}

pub(super) fn deserialize_instrument_ids<'de, D>(
    deserializer: D,
) -> Result<Vec<InstrumentId>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<InstrumentId>::deserialize(deserializer)?
        .into_instrument_ids()
        .map_err(D::Error::custom)
}

/// Checks that a list can route through one venue or an explicit client.
///
/// # Errors
///
/// Returns an error if:
/// - The list is empty.
/// - The list contains duplicate IDs.
/// - The IDs span venues without an explicit client.
pub fn ensure_routable_instrument_ids(
    instrument_ids: &[InstrumentId],
    has_client_id: bool,
) -> anyhow::Result<()> {
    let first = instrument_ids
        .first()
        .ok_or_else(|| anyhow::anyhow!("instrument IDs cannot be empty"))?;
    let mut seen = AHashSet::new();
    anyhow::ensure!(
        instrument_ids.iter().all(|id| seen.insert(*id)),
        "instrument IDs must be unique"
    );
    anyhow::ensure!(
        has_client_id || instrument_ids.iter().all(|id| id.venue == first.venue),
        "multiple instrument venues require an explicit client ID"
    );
    Ok(())
}

/// Checks identifier-list routing for instrument metadata, books, status, close, and option greeks.
///
/// External data clients handle these lists themselves.
///
/// # Errors
///
/// Returns an error if the client is not external and the list contains multiple IDs
/// with a synthetic member.
pub fn ensure_subscription_routes(
    ids: &[InstrumentId],
    client_id: Option<&ClientId>,
    cache: &Cache,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        client_id.is_some_and(|id| cache.is_external_data_client(id))
            || ids.len() <= 1
            || !ids.iter().any(InstrumentId::is_synthetic),
        "Cannot subscribe synthetic instruments for this data type"
    );
    Ok(())
}

/// The engine route shared by every member of a quote subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteRouting {
    Client,
    Synthetic,
    Spread,
}

/// Returns the common quote route using the current instrument cache and spread parameters.
///
/// External data clients route the full list. A spread absent from the cache routes to the client.
///
/// # Errors
///
/// Returns an error if the list is empty, or if a non-external client receives mixed
/// client, synthetic, and spread routes.
pub fn quote_routing(
    ids: &[InstrumentId],
    client_id: Option<&ClientId>,
    params: Option<&Params>,
    cache: &Cache,
) -> anyhow::Result<QuoteRouting> {
    let first = ids
        .first()
        .ok_or_else(|| anyhow::anyhow!("instrument IDs cannot be empty"))?;

    if client_id.is_some_and(|id| cache.is_external_data_client(id)) {
        return Ok(QuoteRouting::Client);
    }

    let aggregate_spreads = params
        .and_then(|params| params.get_bool("aggregate_spread_quotes"))
        .unwrap_or(false);

    let classify = |id: &InstrumentId| {
        if id.is_synthetic() {
            QuoteRouting::Synthetic
        } else if aggregate_spreads && cache.instrument(id).is_some_and(InstrumentAny::is_spread) {
            QuoteRouting::Spread
        } else {
            QuoteRouting::Client
        }
    };

    let routing = classify(first);
    anyhow::ensure!(
        ids.iter().all(|id| classify(id) == routing),
        "quote identifier lists cannot mix client, synthetic, and spread routes"
    );
    Ok(routing)
}

pub(super) fn single_instrument_id(
    instrument_ids: &[InstrumentId],
) -> anyhow::Result<InstrumentId> {
    match instrument_ids {
        [instrument_id] => Ok(*instrument_id),
        _ => anyhow::bail!(
            "subscription requires one instrument ID on this adapter path, received {}",
            instrument_ids.len()
        ),
    }
}

/// Converts a single bar type or collection to a non-empty list.
///
/// Duplicate types are removed in first-occurrence order.
pub trait IntoBarTypes: sealed::Sealed {
    /// Returns the normalized bar types.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection is empty.
    fn into_bar_types(self) -> anyhow::Result<Vec<BarType>>;
}

impl IntoBarTypes for BarType {
    fn into_bar_types(self) -> anyhow::Result<Vec<BarType>> {
        Ok(vec![self])
    }
}

impl IntoBarTypes for &BarType {
    fn into_bar_types(self) -> anyhow::Result<Vec<BarType>> {
        (*self).into_bar_types()
    }
}

impl IntoBarTypes for Vec<BarType> {
    fn into_bar_types(mut self) -> anyhow::Result<Vec<BarType>> {
        anyhow::ensure!(!self.is_empty(), "bar types cannot be empty");

        deduplicate(&mut self);

        Ok(self)
    }
}

impl IntoBarTypes for &Vec<BarType> {
    fn into_bar_types(self) -> anyhow::Result<Vec<BarType>> {
        self.as_slice().into_bar_types()
    }
}

impl IntoBarTypes for &[BarType] {
    fn into_bar_types(self) -> anyhow::Result<Vec<BarType>> {
        self.to_vec().into_bar_types()
    }
}

impl<const N: usize> IntoBarTypes for [BarType; N] {
    fn into_bar_types(self) -> anyhow::Result<Vec<BarType>> {
        Vec::from(self).into_bar_types()
    }
}

impl<const N: usize> IntoBarTypes for &[BarType; N] {
    fn into_bar_types(self) -> anyhow::Result<Vec<BarType>> {
        self.as_slice().into_bar_types()
    }
}

pub(super) fn deserialize_bar_types<'de, D>(deserializer: D) -> Result<Vec<BarType>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<BarType>::deserialize(deserializer)?
        .into_bar_types()
        .map_err(D::Error::custom)
}

/// Checks that a list can route through one venue or an explicit client.
///
/// # Errors
///
/// Returns an error if:
/// - The list is empty.
/// - The list contains duplicate types.
/// - The types span venues without an explicit client.
pub fn ensure_routable_bar_types(bar_types: &[BarType], has_client_id: bool) -> anyhow::Result<()> {
    let first = bar_types
        .first()
        .ok_or_else(|| anyhow::anyhow!("bar types cannot be empty"))?;
    let mut seen = AHashSet::new();
    anyhow::ensure!(
        bar_types.iter().all(|bar_type| seen.insert(*bar_type)),
        "bar types must be unique"
    );
    anyhow::ensure!(
        has_client_id
            || bar_types
                .iter()
                .all(|bar_type| bar_type.instrument_id().venue == first.instrument_id().venue),
        "multiple bar type venues require an explicit client ID"
    );
    Ok(())
}

pub(super) fn single_bar_type(bar_types: &[BarType]) -> anyhow::Result<BarType> {
    match bar_types {
        [bar_type] => Ok(*bar_type),
        _ => anyhow::bail!(
            "subscription requires one bar type on this adapter path, received {}",
            bar_types.len()
        ),
    }
}

fn deduplicate<T: Copy + Eq + Hash>(values: &mut Vec<T>) {
    if values.len() > 1 {
        let mut seen = AHashSet::new();
        values.retain(|value| seen.insert(*value));
    }
}

mod sealed {
    use nautilus_model::{data::BarType, identifiers::InstrumentId};

    pub trait Sealed {}

    impl Sealed for InstrumentId {}
    impl Sealed for &InstrumentId {}
    impl Sealed for Vec<InstrumentId> {}
    impl Sealed for &Vec<InstrumentId> {}
    impl Sealed for &[InstrumentId] {}
    impl<const N: usize> Sealed for [InstrumentId; N] {}
    impl<const N: usize> Sealed for &[InstrumentId; N] {}
    impl Sealed for BarType {}
    impl Sealed for &BarType {}
    impl Sealed for Vec<BarType> {}
    impl Sealed for &Vec<BarType> {}
    impl Sealed for &[BarType] {}
    impl<const N: usize> Sealed for [BarType; N] {}
    impl<const N: usize> Sealed for &[BarType; N] {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn converts_single_inputs() {
        let instrument_id = InstrumentId::from("BTC/USD.SIM");

        assert_eq!(
            instrument_id.into_instrument_ids().unwrap(),
            vec![instrument_id]
        );
        assert_eq!(
            (&instrument_id).into_instrument_ids().unwrap(),
            vec![instrument_id]
        );
    }

    #[rstest::rstest]
    #[case::singleton(["BTC/USD.SIM"], &["BTC/USD.SIM"])]
    #[case::repeated_single_id(
        ["ETH/USD.SIM", "ETH/USD.SIM", "ETH/USD.SIM"],
        &["ETH/USD.SIM"]
    )]
    #[case::distinct_ids(
        ["SOL/USD.SIM", "BTC/USD.SIM", "ETH/USD.SIM"],
        &["SOL/USD.SIM", "BTC/USD.SIM", "ETH/USD.SIM"]
    )]
    #[case::interleaved_duplicates_two_ids(
        ["ETH/USD.SIM", "BTC/USD.SIM", "ETH/USD.SIM"],
        &["ETH/USD.SIM", "BTC/USD.SIM"]
    )]
    #[case::interleaved_duplicates_three_ids(
        [
            "SOL/USD.SIM", "ETH/USD.SIM", "SOL/USD.SIM", "BTC/USD.SIM",
            "ETH/USD.SIM", "BTC/USD.SIM", "SOL/USD.SIM"
        ],
        &["SOL/USD.SIM", "ETH/USD.SIM", "BTC/USD.SIM"]
    )]
    #[case::same_symbol_different_venues(
        ["BTC/USD.OTHER", "BTC/USD.SIM", "BTC/USD.OTHER"],
        &["BTC/USD.OTHER", "BTC/USD.SIM"]
    )]
    fn converts_collection_inputs_in_first_occurrence_order<const N: usize>(
        #[case] input: [&str; N],
        #[case] expected: &[&str],
    ) {
        let input = input.map(InstrumentId::from);
        let expected: Vec<_> = expected.iter().copied().map(InstrumentId::from).collect();
        let vector = input.to_vec();

        assert_eq!(input.into_instrument_ids().unwrap(), expected);
        assert_eq!((&input).into_instrument_ids().unwrap(), expected);
        assert_eq!(input.as_slice().into_instrument_ids().unwrap(), expected);
        assert_eq!((&vector).into_instrument_ids().unwrap(), expected);
        assert_eq!(vector.into_instrument_ids().unwrap(), expected);
    }

    #[rstest::rstest]
    fn rejects_empty_collection_inputs() {
        let input: [InstrumentId; 0] = [];
        let vector = Vec::<InstrumentId>::new();

        assert_eq!(
            input.into_instrument_ids().unwrap_err().to_string(),
            "instrument IDs cannot be empty"
        );
        assert_eq!(
            (&input).into_instrument_ids().unwrap_err().to_string(),
            "instrument IDs cannot be empty"
        );
        assert_eq!(
            input
                .as_slice()
                .into_instrument_ids()
                .unwrap_err()
                .to_string(),
            "instrument IDs cannot be empty"
        );
        assert_eq!(
            (&vector).into_instrument_ids().unwrap_err().to_string(),
            "instrument IDs cannot be empty"
        );
        assert_eq!(
            vector.into_instrument_ids().unwrap_err().to_string(),
            "instrument IDs cannot be empty"
        );
    }

    #[rstest::rstest]
    fn normalizes_full_bar_types_in_first_occurrence_order() {
        let first = BarType::from("BTC/USD.SIM-1-MINUTE-LAST-EXTERNAL");
        let second = BarType::from("BTC/USD.SIM-5-MINUTE-LAST-EXTERNAL");
        let composite = BarType::from("BTC/USD.SIM-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL");
        let input = [second, first, composite, first, second];
        let expected = vec![second, first, composite];

        assert_eq!(first.into_bar_types().unwrap(), vec![first]);
        assert_eq!((&first).into_bar_types().unwrap(), vec![first]);
        assert_eq!(input.into_bar_types().unwrap(), expected);
        assert_eq!((&input).into_bar_types().unwrap(), expected);
        assert_eq!(input.as_slice().into_bar_types().unwrap(), expected);
        assert_eq!(input.to_vec().into_bar_types().unwrap(), expected);
        assert_eq!((&input.to_vec()).into_bar_types().unwrap(), expected);
    }

    #[rstest::rstest]
    fn rejects_empty_bar_type_inputs() {
        let empty: [BarType; 0] = [];
        let vector = Vec::<BarType>::new();
        assert_eq!(
            empty.into_bar_types().unwrap_err().to_string(),
            "bar types cannot be empty"
        );
        assert!((&empty).into_bar_types().is_err());
        assert!(empty.as_slice().into_bar_types().is_err());
        assert!((&vector).into_bar_types().is_err());
        assert!(vector.into_bar_types().is_err());
    }
}
