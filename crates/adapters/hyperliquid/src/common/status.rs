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
    data::{Data, InstrumentStatus},
    enums::MarketStatusAction,
    identifiers::InstrumentId,
};

use super::enums::HyperliquidProductType;

/// Instrument status subscriptions and the last observed status of each instrument.
///
/// Kept behind one lock so a subscription's cached replay is ordered with refresh
/// publication.
#[derive(Debug, Default)]
pub(crate) struct InstrumentStatusState {
    subscriptions: AHashSet<InstrumentId>,
    statuses: AHashMap<InstrumentId, MarketStatusAction>,
}

impl InstrumentStatusState {
    /// Registers a subscription and emits the cached status, if any.
    pub(crate) fn subscribe(
        &mut self,
        instrument_id: InstrumentId,
        sender: &EventSender<DataEvent>,
        ts: UnixNanos,
    ) {
        self.subscriptions.insert(instrument_id);

        if let Some(&action) = self.statuses.get(&instrument_id) {
            emit_status(sender, instrument_id, action, ts, ts);
        }
    }

    pub(crate) fn unsubscribe(&mut self, instrument_id: &InstrumentId) {
        self.subscriptions.remove(instrument_id);
    }

    /// Replaces the cached statuses without emitting.
    pub(crate) fn store(&mut self, statuses: AHashMap<InstrumentId, MarketStatusAction>) {
        self.statuses = statuses;
    }

    /// Applies a new status snapshot, emitting [`InstrumentStatus`] events for changes
    /// and removals of subscribed instruments.
    ///
    /// A cached instrument absent from the new snapshot is removed, emitting
    /// `NotAvailableForTrading`, only when its product type is in `loaded`.
    /// Otherwise its absence may be a failed fetch, so it keeps its previous status.
    pub(crate) fn apply(
        &mut self,
        new_statuses: &AHashMap<InstrumentId, MarketStatusAction>,
        loaded: &AHashSet<HyperliquidProductType>,
        sender: &EventSender<DataEvent>,
        ts: UnixNanos,
    ) {
        for (&instrument_id, &new_action) in new_statuses {
            let changed = self.statuses.insert(instrument_id, new_action) != Some(new_action);
            if changed && self.subscriptions.contains(&instrument_id) {
                emit_status(sender, instrument_id, new_action, ts, ts);
            }
        }

        self.statuses.retain(|instrument_id, _| {
            let removed = !new_statuses.contains_key(instrument_id)
                && HyperliquidProductType::from_symbol(instrument_id.symbol.as_str())
                    .is_ok_and(|product_type| loaded.contains(&product_type));

            if removed && self.subscriptions.contains(instrument_id) {
                emit_status(
                    sender,
                    *instrument_id,
                    MarketStatusAction::NotAvailableForTrading,
                    ts,
                    ts,
                );
            }
            !removed
        });
    }

    pub(crate) fn clear(&mut self) {
        self.subscriptions.clear();
        self.statuses.clear();
    }
}

fn emit_status(
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

    if let Err(e) = sender.send(DataEvent::Data(Data::InstrumentStatus(status))) {
        log::error!("Failed to emit instrument status event: {e}");
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn recv_status(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> InstrumentStatus {
        match rx.try_recv().expect("expected status event") {
            DataEvent::Data(Data::InstrumentStatus(status)) => status,
            other => panic!("expected InstrumentStatus event, was {other:?}"),
        }
    }

    fn status_state(
        subscriptions: &[InstrumentId],
        statuses: &[(InstrumentId, MarketStatusAction)],
    ) -> InstrumentStatusState {
        InstrumentStatusState {
            subscriptions: subscriptions.iter().copied().collect(),
            statuses: statuses.iter().copied().collect(),
        }
    }

    #[rstest]
    fn test_apply_emits_on_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let mut state = status_state(&[id], &[(id, MarketStatusAction::Trading)]);
        let new_statuses = AHashMap::from([(id, MarketStatusAction::Close)]);

        state.apply(
            &new_statuses,
            &AHashSet::new(),
            &tx.into(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::Close);
        assert_eq!(status.is_trading, Some(false));
        assert_eq!(state.statuses.get(&id), Some(&MarketStatusAction::Close));
    }

    #[rstest]
    fn test_apply_no_emit_when_unchanged() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let mut state = status_state(&[id], &[(id, MarketStatusAction::Trading)]);
        let new_statuses = AHashMap::from([(id, MarketStatusAction::Trading)]);

        state.apply(
            &new_statuses,
            &AHashSet::new(),
            &tx.into(),
            UnixNanos::default(),
        );

        rx.try_recv().unwrap_err();
    }

    #[rstest]
    fn test_apply_emits_for_new_instrument() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");
        let mut state = status_state(&[id], &[]);
        let new_statuses = AHashMap::from([(id, MarketStatusAction::Trading)]);

        state.apply(
            &new_statuses,
            &AHashSet::new(),
            &tx.into(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::Trading);
        assert_eq!(status.is_trading, Some(true));
    }

    #[rstest]
    #[case::perp("BTC-USD-PERP.HYPERLIQUID", HyperliquidProductType::Perp)]
    #[case::spot("PURR-USDC-SPOT.HYPERLIQUID", HyperliquidProductType::Spot)]
    #[case::outcome("1-YES-OUTCOME.HYPERLIQUID", HyperliquidProductType::Outcome)]
    fn test_apply_emits_not_available_for_removed_instrument(
        #[case] instrument_id: &str,
        #[case] product_type: HyperliquidProductType,
    ) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from(instrument_id);
        let mut state = status_state(&[id], &[(id, MarketStatusAction::Trading)]);

        state.apply(
            &AHashMap::new(),
            &AHashSet::from([product_type]),
            &tx.into(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::NotAvailableForTrading);
        assert_eq!(status.is_trading, Some(false));
        assert!(!state.statuses.contains_key(&id));
    }

    #[rstest]
    #[case::spot_not_loaded("PURR-USDC-SPOT.HYPERLIQUID", &[HyperliquidProductType::Outcome])]
    #[case::perp_not_loaded(
        "BTC-USD-PERP.HYPERLIQUID",
        &[HyperliquidProductType::Spot, HyperliquidProductType::Outcome],
    )]
    fn test_apply_keeps_status_when_absence_is_not_a_removal(
        #[case] instrument_id: &str,
        #[case] loaded: &[HyperliquidProductType],
    ) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from(instrument_id);
        let mut state = status_state(&[id], &[(id, MarketStatusAction::Trading)]);

        state.apply(
            &AHashMap::new(),
            &loaded.iter().copied().collect(),
            &tx.into(),
            UnixNanos::default(),
        );

        rx.try_recv().unwrap_err();
        assert_eq!(state.statuses.get(&id), Some(&MarketStatusAction::Trading));
    }

    #[rstest]
    fn test_apply_only_emits_for_subscribed() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let subscribed_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let unsubscribed_id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");
        let mut state = status_state(
            &[subscribed_id],
            &[
                (subscribed_id, MarketStatusAction::Trading),
                (unsubscribed_id, MarketStatusAction::Trading),
            ],
        );
        let new_statuses = AHashMap::from([
            (subscribed_id, MarketStatusAction::Close),
            (unsubscribed_id, MarketStatusAction::Close),
        ]);

        state.apply(
            &new_statuses,
            &AHashSet::new(),
            &tx.into(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, subscribed_id);
        assert_eq!(status.action, MarketStatusAction::Close);
        rx.try_recv().unwrap_err();
        assert_eq!(
            state.statuses.get(&subscribed_id),
            Some(&MarketStatusAction::Close)
        );
        assert_eq!(
            state.statuses.get(&unsubscribed_id),
            Some(&MarketStatusAction::Close)
        );
    }

    #[rstest]
    fn test_apply_removal_only_emits_for_subscribed() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let subscribed_id = InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID");
        let unsubscribed_id = InstrumentId::from("HYPE-USDC-SPOT.HYPERLIQUID");
        let mut state = status_state(
            &[subscribed_id],
            &[
                (subscribed_id, MarketStatusAction::Trading),
                (unsubscribed_id, MarketStatusAction::Trading),
            ],
        );

        state.apply(
            &AHashMap::new(),
            &AHashSet::from([HyperliquidProductType::Spot]),
            &tx.into(),
            UnixNanos::default(),
        );

        let status = recv_status(&mut rx);
        assert_eq!(status.instrument_id, subscribed_id);
        assert_eq!(status.action, MarketStatusAction::NotAvailableForTrading);
        rx.try_recv().unwrap_err();
        assert!(state.statuses.is_empty());
    }

    fn apply_close(
        state: &mut InstrumentStatusState,
        id: InstrumentId,
        sender: &EventSender<DataEvent>,
    ) {
        state.apply(
            &AHashMap::from([(id, MarketStatusAction::Close)]),
            &AHashSet::from([HyperliquidProductType::Perp]),
            sender,
            UnixNanos::default(),
        );
    }

    #[rstest]
    fn test_state_subscribe_before_change_receives_replay_then_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<DataEvent> = tx.into();
        let id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let mut state = status_state(&[], &[(id, MarketStatusAction::Trading)]);

        state.subscribe(id, &sender, UnixNanos::default());
        apply_close(&mut state, id, &sender);

        assert_eq!(recv_status(&mut rx).action, MarketStatusAction::Trading);
        assert_eq!(recv_status(&mut rx).action, MarketStatusAction::Close);
        rx.try_recv().unwrap_err();
    }

    #[rstest]
    fn test_state_subscribe_after_change_receives_new_status() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<DataEvent> = tx.into();
        let id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
        let mut state = status_state(&[], &[(id, MarketStatusAction::Trading)]);

        apply_close(&mut state, id, &sender);
        state.subscribe(id, &sender, UnixNanos::default());

        assert_eq!(recv_status(&mut rx).action, MarketStatusAction::Close);
        rx.try_recv().unwrap_err();
    }
}
