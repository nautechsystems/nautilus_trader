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

//! Instrument status tracking for Hyperliquid.

use ahash::{AHashMap, AHashSet};
use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::InstrumentStatus, enums::MarketStatusAction, identifiers::InstrumentId,
};

/// Compares a new status snapshot against cached state, emitting [`InstrumentStatus`]
/// events for changes and removals.
///
/// Instruments present in the cache but absent from the new snapshot are treated as
/// removed and emit `NotAvailableForTrading`.
pub fn diff_and_emit_statuses(
    new_statuses: &AHashMap<InstrumentId, MarketStatusAction>,
    cached_statuses: &mut AHashMap<InstrumentId, MarketStatusAction>,
    subscriptions: Option<&AHashSet<InstrumentId>>,
    sender: &EventSender<DataEvent>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let is_subscribed = |id: &InstrumentId| subscriptions.is_none_or(|subs| subs.contains(id));

    for (instrument_id, &new_action) in new_statuses {
        let changed = cached_statuses
            .get(instrument_id)
            .is_none_or(|&prev| prev != new_action);

        if changed {
            cached_statuses.insert(*instrument_id, new_action);
            if is_subscribed(instrument_id) {
                emit_status(sender, *instrument_id, new_action, ts_event, ts_init);
            }
        }
    }

    let removed: Vec<InstrumentId> = cached_statuses
        .keys()
        .filter(|id| !new_statuses.contains_key(id))
        .copied()
        .collect();

    for instrument_id in removed {
        cached_statuses.remove(&instrument_id);
        if is_subscribed(&instrument_id) {
            emit_status(
                sender,
                instrument_id,
                MarketStatusAction::NotAvailableForTrading,
                ts_event,
                ts_init,
            );
        }
    }
}

pub(crate) fn emit_status(
    sender: &EventSender<DataEvent>,
    instrument_id: InstrumentId,
    action: MarketStatusAction,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let is_trading = Some(matches!(action, MarketStatusAction::Trading));
    let status = InstrumentStatus::new(
        instrument_id,
        action,
        ts_event,
        ts_init,
        None,
        None,
        is_trading,
        None,
        None,
    );

    if let Err(e) = sender.send(DataEvent::InstrumentStatus(status)) {
        log::error!("Failed to emit instrument status event: {e}");
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn recv_status(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> InstrumentStatus {
        match rx.try_recv().expect("expected status event") {
            DataEvent::InstrumentStatus(status) => status,
            other => panic!("expected InstrumentStatus event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_diff_emits_on_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let mut cached = AHashMap::from([(id, MarketStatusAction::Trading)]);
        let new_statuses = AHashMap::from([(id, MarketStatusAction::Close)]);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            None,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::Close);
        assert_eq!(status.is_trading, Some(false));
        assert_eq!(cached.get(&id), Some(&MarketStatusAction::Close));
    }

    #[rstest]
    fn test_diff_no_emit_when_unchanged() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let mut cached = AHashMap::from([(id, MarketStatusAction::Trading)]);
        let new_statuses = AHashMap::from([(id, MarketStatusAction::Trading)]);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            None,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        rx.try_recv().unwrap_err();
    }

    #[rstest]
    fn test_diff_emits_for_new_instrument() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");
        let mut cached = AHashMap::new();
        let new_statuses = AHashMap::from([(id, MarketStatusAction::Trading)]);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            None,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::Trading);
        assert_eq!(status.is_trading, Some(true));
    }

    #[rstest]
    fn test_diff_emits_not_available_for_removed_instrument() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let mut cached = AHashMap::from([(id, MarketStatusAction::Trading)]);
        let new_statuses = AHashMap::new();

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            None,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::NotAvailableForTrading);
        assert_eq!(status.is_trading, Some(false));
        assert!(!cached.contains_key(&id));
    }

    #[rstest]
    fn test_diff_only_emits_for_subscribed() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let subscribed_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let unsubscribed_id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");
        let subs = AHashSet::from([subscribed_id]);
        let mut cached = AHashMap::from([
            (subscribed_id, MarketStatusAction::Trading),
            (unsubscribed_id, MarketStatusAction::Trading),
        ]);
        let new_statuses = AHashMap::from([
            (subscribed_id, MarketStatusAction::Close),
            (unsubscribed_id, MarketStatusAction::Close),
        ]);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            Some(&subs),
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, subscribed_id);
        assert_eq!(status.action, MarketStatusAction::Close);
        rx.try_recv().unwrap_err();
        assert_eq!(cached.get(&subscribed_id), Some(&MarketStatusAction::Close));
        assert_eq!(
            cached.get(&unsubscribed_id),
            Some(&MarketStatusAction::Close)
        );
    }

    #[rstest]
    fn test_diff_removal_only_emits_for_subscribed() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let subscribed_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let unsubscribed_id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");
        let subs = AHashSet::from([subscribed_id]);
        let mut cached = AHashMap::from([
            (subscribed_id, MarketStatusAction::Trading),
            (unsubscribed_id, MarketStatusAction::Trading),
        ]);
        let new_statuses = AHashMap::new();

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            Some(&subs),
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, subscribed_id);
        assert_eq!(status.action, MarketStatusAction::NotAvailableForTrading);
        rx.try_recv().unwrap_err();
        assert!(cached.is_empty());
    }
}
