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

//! Durable position history with disposable lookup state.

use std::{
    collections::hash_map::Entry,
    fmt::Debug,
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

use ahash::AHashMap;
use serde::{Deserialize, Serialize};

use super::PositionReplayEvent;
use crate::identifiers::TradeId;

/// Durable replay events with lazily derived trade and price-restatement lookups.
///
/// Serializes as the event sequence alone. Cloning copies the events without the index,
/// and deserialization starts without an index. Queries rebuild it when needed.
/// Appending maintains an existing index; all other mutable vector access invalidates it
/// before exposing the events, including edits that do not change the vector's length.
///
/// Use `Vec::into` to assign an existing vector and `Vec::from` to extract one. Borrowing
/// vector methods remain available through dereferencing; consuming vector methods require
/// extracting the vector first.
///
/// Queries may rebuild the index after a mutable borrow ends. Before writing through a
/// retained raw pointer after a query, acquire mutable access again to invalidate the index.
#[derive(Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PositionReplayEvents {
    events: Vec<PositionReplayEvent>,
    #[serde(skip)]
    index: OnceLock<Box<ReplayIndex>>,
}

#[derive(Default)]
struct ReplayIndex {
    first_fills: AHashMap<TradeId, usize>,
    has_price_restatement: bool,
}

impl PositionReplayEvents {
    /// Appends an event, incrementally maintaining the index if it has been built.
    ///
    /// # Panics
    ///
    /// Panics if the event vector's capacity overflows or comparing malformed fill prices
    /// panics. On unwind, the events retain any completed append and the index is discarded.
    pub fn push(&mut self, event: PositionReplayEvent) {
        // Keep the derived state out of the container until the append is complete
        let index = self.index.take();
        self.events.push(event);
        if let Some(mut index) = index {
            index.record(self.events.len() - 1, &self.events);
            self.index = OnceLock::from(index);
        }
    }

    pub(super) fn has_trade_id(&self, trade_id: TradeId) -> bool {
        self.index().first_fills.contains_key(&trade_id)
    }

    pub(super) fn has_price_restatement(&self) -> bool {
        self.index().has_price_restatement
    }

    fn index(&self) -> &ReplayIndex {
        self.index.get_or_init(|| {
            let mut index = Box::<ReplayIndex>::default();
            for event_index in 0..self.events.len() {
                index.record(event_index, &self.events);
            }
            index
        })
    }
}

impl ReplayIndex {
    fn record(&mut self, event_index: usize, events: &[PositionReplayEvent]) {
        if let PositionReplayEvent::Filled(fill) = &events[event_index] {
            match self.first_fills.entry(fill.trade_id) {
                Entry::Vacant(entry) => {
                    entry.insert(event_index);
                }
                Entry::Occupied(entry) => {
                    if let PositionReplayEvent::Filled(first) = &events[*entry.get()] {
                        self.has_price_restatement |= first.last_px != fill.last_px;
                    }
                }
            }
        }
    }
}

impl Clone for PositionReplayEvents {
    fn clone(&self) -> Self {
        self.events.clone().into()
    }
}

impl Debug for PositionReplayEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.events, f)
    }
}

impl Deref for PositionReplayEvents {
    type Target = Vec<PositionReplayEvent>;

    fn deref(&self) -> &Self::Target {
        &self.events
    }
}

impl DerefMut for PositionReplayEvents {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.index.take();
        &mut self.events
    }
}

impl From<Vec<PositionReplayEvent>> for PositionReplayEvents {
    fn from(events: Vec<PositionReplayEvent>) -> Self {
        Self {
            events,
            index: OnceLock::new(),
        }
    }
}

impl From<PositionReplayEvents> for Vec<PositionReplayEvent> {
    fn from(events: PositionReplayEvents) -> Self {
        events.events
    }
}

impl Extend<PositionReplayEvent> for PositionReplayEvents {
    fn extend<T: IntoIterator<Item = PositionReplayEvent>>(&mut self, iter: T) {
        for event in iter {
            self.push(event);
        }
    }
}

impl IntoIterator for PositionReplayEvents {
    type Item = PositionReplayEvent;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

impl<'a> IntoIterator for &'a PositionReplayEvents {
    type Item = &'a PositionReplayEvent;
    type IntoIter = std::slice::Iter<'a, PositionReplayEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}

impl<'a> IntoIterator for &'a mut PositionReplayEvents {
    type Item = &'a mut PositionReplayEvent;
    type IntoIter = std::slice::IterMut<'a, PositionReplayEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        mem::size_of,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Barrier,
        thread,
    };

    use nautilus_core::UUID4;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::{
        events::order::spec::OrderFilledSpec,
        identifiers::ClientOrderId,
        position::Position,
        stubs::stub_position_long,
        types::{Price, Quantity},
    };

    fn event(trade: u8, price: u8) -> PositionReplayEvent {
        PositionReplayEvent::Filled(
            OrderFilledSpec::builder()
                .trade_id(TradeId::from(format!("T-{trade}")))
                .last_px(Price::from(price.to_string()))
                .build(),
        )
    }

    fn assert_matches_scan(events: &PositionReplayEvents) {
        let mut prices = AHashMap::new();
        let mut restated = false;

        for event in events {
            if let PositionReplayEvent::Filled(fill) = event {
                if let Some(first) = prices.get(&fill.trade_id) {
                    restated |= *first != fill.last_px;
                } else {
                    prices.insert(fill.trade_id, fill.last_px);
                }
            }
        }

        for trade in 0..9 {
            let id = TradeId::from(format!("T-{trade}"));
            assert_eq!(events.has_trade_id(id), prices.contains_key(&id));
        }
        assert_eq!(events.has_price_restatement(), restated);
    }

    #[rstest]
    fn test_index_is_lazy_incremental_and_not_copied() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Position>();
        assert_send_sync::<PositionReplayEvents>();
        assert!(
            size_of::<PositionReplayEvents>()
                <= size_of::<Vec<PositionReplayEvent>>() + 2 * size_of::<usize>()
        );
        let mut events = PositionReplayEvents::default();
        events.push(event(0, 1));
        assert!(events.index.get().is_none());
        assert_matches_scan(&events);
        let index = std::ptr::from_ref(events.index());
        events.extend([event(1, 2), event(0, 2)]);
        assert_eq!(std::ptr::from_ref(events.index()), index);
        assert_matches_scan(&events);
        let mut copied = events.clone();
        assert!(copied.index.get().is_none());
        copied[2] = event(0, 1);
        assert!(!copied.has_price_restatement());
        assert!(events.has_price_restatement());
        assert_eq!(std::ptr::from_ref(events.index()), index);
    }

    #[rstest]
    fn test_concurrent_cold_queries_share_one_index() {
        let events: PositionReplayEvents = vec![event(0, 1), event(1, 1), event(0, 2)].into();
        let start = Barrier::new(8);
        assert!(events.index.get().is_none());

        thread::scope(|scope| {
            let readers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        start.wait();
                        assert_matches_scan(&events);
                        events.index()
                    })
                })
                .collect();

            for reader in readers {
                assert!(std::ptr::eq(reader.join().unwrap(), events.index()));
            }
        });
    }

    #[rstest]
    #[case::index(|events: &mut PositionReplayEvents| { events[2] = event(0, 1); })]
    #[case::iter_mut(|events: &mut PositionReplayEvents| { *events.iter_mut().last().unwrap() = event(0, 1); })]
    #[case::into_iter_mut(|events: &mut PositionReplayEvents| { for item in events { *item = event(0, 1); } })]
    #[case::last_mut(|events: &mut PositionReplayEvents| { *events.last_mut().unwrap() = event(0, 1); })]
    #[case::slice(|events: &mut PositionReplayEvents| { events.as_mut_slice()[2] = event(0, 1); })]
    #[case::vec(|events: &mut PositionReplayEvents| { let vec: &mut Vec<_> = events; vec[2] = event(0, 1); })]
    #[case::retain(|events: &mut PositionReplayEvents| { events.retain(|item| matches!(item, PositionReplayEvent::Filled(fill) if fill.last_px == Price::from("1"))); })]
    #[case::pop(|events: &mut PositionReplayEvents| { events.pop(); })]
    #[case::truncate(|events: &mut PositionReplayEvents| { events.truncate(2); })]
    #[case::clear(|events: &mut PositionReplayEvents| { events.clear(); })]
    #[case::drain(|events: &mut PositionReplayEvents| { events.drain(2..); })]
    #[case::splice(|events: &mut PositionReplayEvents| { events.splice(2.., [event(0, 1)]); })]
    #[case::replace_vec(|events: &mut PositionReplayEvents| { **events = vec![event(0, 1), event(1, 1), event(2, 1)]; })]
    fn test_mutable_access_invalidates_before_edit(#[case] mutate: fn(&mut PositionReplayEvents)) {
        let mut events: PositionReplayEvents = vec![event(0, 1), event(1, 1), event(0, 2)].into();
        assert!(events.has_price_restatement());
        mutate(&mut events);
        assert!(events.index.get().is_none());
        assert!(!events.has_price_restatement());
        assert_matches_scan(&events);
    }

    #[rstest]
    fn test_panicking_mutation_and_partial_extend_remain_consistent() {
        let mut events: PositionReplayEvents = vec![event(0, 1), event(0, 2)].into();
        assert!(events.has_price_restatement());
        let result = catch_unwind(AssertUnwindSafe(|| {
            events.retain_mut(|item| {
                *item = event(2, 1);
                panic!("edit interrupted");
            });
        }));
        assert!(result.is_err());
        assert!(events.index.get().is_none());
        assert_matches_scan(&events);
        let index = std::ptr::from_ref(events.index());
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut count = 0;
            events.extend(std::iter::from_fn(|| {
                count += 1;
                assert!(count < 3, "iterator interrupted");
                Some(event(3, count))
            }));
        }));
        assert!(result.is_err());
        assert_eq!(std::ptr::from_ref(events.index()), index);
        assert_matches_scan(&events);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_panicking_price_comparison_invalidates_index() {
        let mut events: PositionReplayEvents = vec![event(0, 1)].into();
        assert_matches_scan(&events);
        let mut invalid = event(0, 2);
        let PositionReplayEvent::Filled(fill) = &mut invalid else {
            unreachable!();
        };
        // Public precision can bypass Price's checked constructors
        fill.last_px.precision = u8::MAX;

        let result = catch_unwind(AssertUnwindSafe(|| events.push(invalid)));

        assert!(result.is_err());
        assert!(events.index.get().is_none());
        events.pop();
        assert_matches_scan(&events);
    }

    #[rstest]
    fn test_legacy_sequence_serialization() {
        let original = vec![event(0, 1), event(0, 2)];
        let mut events: PositionReplayEvents =
            serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
        assert!(events.index.get().is_none());
        assert_matches_scan(&events);
        assert_eq!(
            serde_json::to_vec(&events).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );
        assert_eq!(
            rmp_serde::to_vec_named(&events).unwrap(),
            rmp_serde::to_vec_named(&original).unwrap()
        );
        events = rmp_serde::from_slice(&rmp_serde::to_vec_named(&events).unwrap()).unwrap();
        assert!(events.index.get().is_none());
        assert_matches_scan(&events);
    }

    #[rstest]
    fn test_position_transfer_snapshot_restore_and_failed_fill(mut stub_position_long: Position) {
        let old_trade = stub_position_long.events[0].trade_id;
        assert!(stub_position_long.has_replay_trade_id(old_trade));
        let old_index = std::ptr::from_ref(stub_position_long.replay_events.index());
        let mut next = stub_position_long.clone();
        next.replay_events.clear();
        next.replay_events.push(event(8, 1));
        next.transfer_replay_state_from(&mut stub_position_long);
        assert!(stub_position_long.replay_events.is_empty());
        assert!(stub_position_long.replay_events.index.get().is_none());
        assert_eq!(std::ptr::from_ref(next.replay_events.index()), old_index);
        assert!(next.has_replay_trade_id(old_trade));
        assert!(next.has_replay_trade_id(TradeId::from("T-8")));
        for snapshot in [next.clone_without_events(), next.clone_for_snapshot()] {
            assert!(snapshot.replay_events.is_empty());
            assert!(snapshot.replay_events.index.get().is_none());
        }
        let before = serde_json::to_value(&next).unwrap();
        let mut fill = next.events[0].clone();
        fill.trade_id = TradeId::from("INVALID");
        fill.client_order_id = ClientOrderId::from("INVALID");
        fill.event_id = UUID4::new();
        fill.last_qty = Quantity::zero(0);
        assert!(next.try_apply(&fill).is_err());
        assert_eq!(serde_json::to_value(&next).unwrap(), before);
        assert!(!next.has_replay_trade_id(fill.trade_id));
        let restored: Position = serde_json::from_value(before.clone()).unwrap();
        assert!(restored.replay_events.index.get().is_none());
        assert!(restored.has_replay_trade_id(old_trade));
        let mut legacy = before;
        legacy.as_object_mut().unwrap().remove("replay_events");
        let restored: Position = serde_json::from_value(legacy).unwrap();
        assert!(restored.replay_events.is_empty());
    }

    #[rstest]
    fn test_purge_removes_warmed_trade_and_restatement(mut stub_position_long: Position) {
        let mut restated = stub_position_long.events[0].clone();
        let mut other = restated.clone();
        other.trade_id = TradeId::from("OTHER-TRADE");
        other.client_order_id = ClientOrderId::from("OTHER-ORDER");
        other.event_id = UUID4::new();
        stub_position_long.try_apply(&other).unwrap();
        restated.event_id = UUID4::new();
        restated.last_px = Price::from("1.10000");
        stub_position_long.try_apply(&restated).unwrap();
        assert!(stub_position_long.requires_replay_state());
        assert!(stub_position_long.replay_events.index.get().is_some());

        stub_position_long
            .purge_events_for_order(restated.client_order_id)
            .unwrap();

        assert!(!stub_position_long.has_replay_trade_id(restated.trade_id));
        assert!(stub_position_long.has_replay_trade_id(other.trade_id));
        assert!(!stub_position_long.requires_replay_state());
        assert!(!stub_position_long.replay_events.has_price_restatement());
    }

    proptest! {
        #[rstest]
        fn test_index_matches_scan_after_edits(operations in prop::collection::vec((0_u8..8, 0_u8..8, 1_u8..4), 1..100)) {
            let mut events = PositionReplayEvents::default();

            for (operation, trade, price) in operations {
                match operation {
                    0 | 1 => events.push(event(trade, price)),
                    2 => events.extend([event(trade, price), event(trade, 1)]),
                    3 => { events.pop(); },
                    4 if !events.is_empty() => { events[0] = event(trade, price); },
                    5 => events.reverse(),
                    6 => events.retain(|item| !matches!(item, PositionReplayEvent::Filled(fill) if fill.trade_id == TradeId::from(format!("T-{trade}")))),
                    _ => events.clear(),
                }
                assert_matches_scan(&events);
            }
        }
    }
}
