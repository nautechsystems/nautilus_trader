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

//! Subscription identity and ownership tracking.

use std::{hash::Hash, num::NonZeroUsize};

use ahash::{AHashMap, AHashSet};
use nautilus_common::messages::data::{SubscribeCommand, UnsubscribeCommand};
use nautilus_core::UUID4;
#[cfg(feature = "defi")]
use nautilus_model::defi::Blockchain;
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{InstrumentId, OptionSeriesId, Venue},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SubscriptionKey {
    Data(DataType),
    Instrument(InstrumentId),
    Instruments(Venue),
    BookDeltas(InstrumentId),
    BookDepth10(InstrumentId),
    BookSnapshots(InstrumentId, NonZeroUsize),
    Quotes(InstrumentId),
    Trades(InstrumentId),
    Bars(BarType),
    MarkPrices(InstrumentId),
    IndexPrices(InstrumentId),
    FundingRates(InstrumentId),
    InstrumentStatus(InstrumentId),
    InstrumentClose(InstrumentId),
    OptionGreeks(InstrumentId),
    OptionChain(OptionSeriesId),
}

impl SubscriptionKey {
    pub(crate) fn from_subscribe(command: &SubscribeCommand) -> Self {
        match command {
            SubscribeCommand::Data(command) => Self::Data(command.data_type.clone()),
            SubscribeCommand::Instrument(command) => Self::Instrument(command.instrument_id),
            SubscribeCommand::Instruments(command) => Self::Instruments(command.venue),
            SubscribeCommand::BookDeltas(command) => Self::BookDeltas(command.instrument_id),
            SubscribeCommand::BookDepth10(command) => Self::BookDepth10(command.instrument_id),
            SubscribeCommand::BookSnapshots(command) => {
                Self::BookSnapshots(command.instrument_id, command.interval_ms)
            }
            SubscribeCommand::OptionChain(command) => Self::OptionChain(command.series_id),
            SubscribeCommand::Quotes(command) => Self::Quotes(command.instrument_id),
            SubscribeCommand::Trades(command) => Self::Trades(command.instrument_id),
            SubscribeCommand::Bars(command) => Self::Bars(command.bar_type),
            SubscribeCommand::MarkPrices(command) => Self::MarkPrices(command.instrument_id),
            SubscribeCommand::IndexPrices(command) => Self::IndexPrices(command.instrument_id),
            SubscribeCommand::FundingRates(command) => Self::FundingRates(command.instrument_id),
            SubscribeCommand::InstrumentStatus(command) => {
                Self::InstrumentStatus(command.instrument_id)
            }
            SubscribeCommand::InstrumentClose(command) => {
                Self::InstrumentClose(command.instrument_id)
            }
            SubscribeCommand::OptionGreeks(command) => Self::OptionGreeks(command.instrument_id),
        }
    }

    pub(crate) fn from_unsubscribe(command: &UnsubscribeCommand) -> Self {
        match command {
            UnsubscribeCommand::Data(command) => Self::Data(command.data_type.clone()),
            UnsubscribeCommand::Instrument(command) => Self::Instrument(command.instrument_id),
            UnsubscribeCommand::Instruments(command) => Self::Instruments(command.venue),
            UnsubscribeCommand::BookDeltas(command) => Self::BookDeltas(command.instrument_id),
            UnsubscribeCommand::BookDepth10(command) => Self::BookDepth10(command.instrument_id),
            UnsubscribeCommand::BookSnapshots(command) => {
                Self::BookSnapshots(command.instrument_id, command.interval_ms)
            }
            UnsubscribeCommand::OptionChain(command) => Self::OptionChain(command.series_id),
            UnsubscribeCommand::Quotes(command) => Self::Quotes(command.instrument_id),
            UnsubscribeCommand::Trades(command) => Self::Trades(command.instrument_id),
            UnsubscribeCommand::Bars(command) => Self::Bars(command.bar_type),
            UnsubscribeCommand::MarkPrices(command) => Self::MarkPrices(command.instrument_id),
            UnsubscribeCommand::IndexPrices(command) => Self::IndexPrices(command.instrument_id),
            UnsubscribeCommand::FundingRates(command) => Self::FundingRates(command.instrument_id),
            UnsubscribeCommand::InstrumentStatus(command) => {
                Self::InstrumentStatus(command.instrument_id)
            }
            UnsubscribeCommand::InstrumentClose(command) => {
                Self::InstrumentClose(command.instrument_id)
            }
            UnsubscribeCommand::OptionGreeks(command) => Self::OptionGreeks(command.instrument_id),
        }
    }
}

#[cfg(feature = "defi")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DefiSubscriptionKey {
    Blocks(Blockchain),
    Pool(InstrumentId),
    PoolSwaps(InstrumentId),
    PoolLiquidityUpdates(InstrumentId),
    PoolFeeCollects(InstrumentId),
    PoolFlashEvents(InstrumentId),
}

#[derive(Debug)]
pub(crate) struct ActiveSubscription<T> {
    pub(crate) command: T,

    // Releases are anonymous, so replay identities live until the physical feed is released
    pub(crate) acquisitions: AHashSet<UUID4>,
    pub(crate) owners: usize,
}

#[derive(Debug)]
pub(crate) struct SubscriptionRegistry<K, T> {
    entries: AHashMap<K, ActiveSubscription<T>>,
}

impl<K, T> Default for SubscriptionRegistry<K, T> {
    fn default() -> Self {
        Self {
            entries: AHashMap::new(),
        }
    }
}

impl<K, T> SubscriptionRegistry<K, T>
where
    K: Eq + Hash,
{
    pub(crate) fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&K, &ActiveSubscription<T>)> {
        self.entries.iter()
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut ActiveSubscription<T>> {
        self.entries.get_mut(key)
    }

    pub(crate) fn remove(&mut self, key: &K) -> Option<ActiveSubscription<T>> {
        self.entries.remove(key)
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn retain(&mut self, key: K, owner_id: UUID4, command: T) -> bool {
        if let Some(active) = self.entries.get_mut(&key) {
            if active.acquisitions.insert(owner_id) {
                active.owners += 1;
            }
            return false;
        }

        self.entries.insert(
            key,
            ActiveSubscription {
                command,
                acquisitions: AHashSet::from_iter([owner_id]),
                owners: 1,
            },
        );
        true
    }

    pub(crate) fn release(&mut self, key: &K) -> SubscriptionRelease<T>
    where
        T: Clone,
    {
        let Some(active) = self.entries.get_mut(key) else {
            return SubscriptionRelease::Untracked;
        };

        active.owners = active.owners.saturating_sub(1);
        if active.owners > 0 {
            return SubscriptionRelease::Retained;
        }

        // Keep the original inverse available until the caller confirms physical release
        SubscriptionRelease::Final(active.command.clone())
    }
}

pub(crate) enum SubscriptionRelease<T> {
    Retained,
    Final(T),
    Untracked,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_churn_retains_replay_ids_until_physical_release() {
        let mut registry = SubscriptionRegistry::default();
        let anchor = UUID4::new();
        assert!(registry.retain(1, anchor, "original"));
        let mut acquisitions = vec![anchor];

        for _ in 0..10_000 {
            let id = UUID4::new();
            acquisitions.push(id);
            assert!(!registry.retain(1, id, "transient"));
            assert!(matches!(
                registry.release(&1),
                SubscriptionRelease::Retained
            ));
        }

        let active = registry.get_mut(&1).unwrap();
        assert_eq!(active.owners, 1);
        assert_eq!(active.acquisitions.len(), 10_001);
        for id in acquisitions {
            assert!(!registry.retain(1, id, "replay"));
        }
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("original")
        ));
        registry.remove(&1);
        assert!(!registry.contains(&1));
        assert!(registry.retain(1, anchor, "fresh"));
        let active = registry.get_mut(&1).unwrap();
        assert_eq!(active.owners, 1);
        assert_eq!(active.acquisitions.len(), 1);
        assert_eq!(active.command, "fresh");
        registry.clear();
        assert!(!registry.contains(&1));
    }

    #[rstest]
    fn test_anonymous_releases_require_balanced_callers() {
        let mut registry = SubscriptionRegistry::default();
        assert!(registry.retain(1, UUID4::new(), "first"));
        assert!(!registry.retain(1, UUID4::new(), "second"));
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Retained
        ));

        // The registry cannot distinguish a duplicate release from the second owner
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("first")
        ));
        assert_eq!(registry.get_mut(&1).unwrap().owners, 0);
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("first")
        ));
        registry.remove(&1);
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Untracked
        ));
    }

    #[rstest]
    fn test_replay_after_partial_release_does_not_acquire_another_owner() {
        let mut registry = SubscriptionRegistry::default();
        let first_id = UUID4::new();
        let second_id = UUID4::new();
        assert!(registry.retain(1, first_id, "original"));
        assert!(!registry.retain(1, second_id, "second"));
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Retained
        ));

        assert!(!registry.retain(1, first_id, "replay first"));
        assert!(!registry.retain(1, second_id, "replay second"));

        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("original")
        ));
    }

    #[rstest]
    fn test_failed_release_retains_identity_until_physical_release() {
        let mut registry = SubscriptionRegistry::default();
        let original_id = UUID4::new();
        assert!(registry.retain(1, original_id, "original"));
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("original")
        ));
        assert!(!registry.retain(1, original_id, "replay"));
        assert!(!registry.retain(1, UUID4::new(), "new consumer"));
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("original")
        ));
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("original")
        ));

        registry.remove(&1);

        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Untracked
        ));
        assert!(registry.retain(1, UUID4::new(), "new physical feed"));
        assert!(matches!(
            registry.release(&1),
            SubscriptionRelease::Final("new physical feed")
        ));
    }
}
