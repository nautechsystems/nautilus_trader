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

//! Instrument status mapping and polling for the Bybit adapter.

use ahash::AHashMap;
use nautilus_common::messages::DataEvent;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::InstrumentStatus, enums::MarketStatusAction, identifiers::InstrumentId,
};

use super::enums::BybitInstrumentStatus;

impl From<BybitInstrumentStatus> for MarketStatusAction {
    fn from(status: BybitInstrumentStatus) -> Self {
        match status {
            BybitInstrumentStatus::Trading => Self::Trading,
            BybitInstrumentStatus::PreLaunch => Self::PreOpen,
            BybitInstrumentStatus::PreTrading => Self::PreOpen,
            BybitInstrumentStatus::PendingListing => Self::PreOpen,
            BybitInstrumentStatus::ListedOnly => Self::PreOpen,
            BybitInstrumentStatus::Delivering => Self::PreClose,
            BybitInstrumentStatus::Settled => Self::Close,
            BybitInstrumentStatus::Closed => Self::Close,
            BybitInstrumentStatus::Suspended => Self::Suspend,
            BybitInstrumentStatus::Other => Self::NotAvailableForTrading,
        }
    }
}

/// Compares new status snapshot against cached state, emitting [`InstrumentStatus`]
/// events for changes and removals.
///
/// Symbols present in the cache but absent from the new snapshot are treated as
/// removed and emit `NotAvailableForTrading`.
pub fn diff_and_emit_statuses(
    new_statuses: &AHashMap<InstrumentId, MarketStatusAction>,
    cached_statuses: &mut AHashMap<InstrumentId, MarketStatusAction>,
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    for (instrument_id, &new_action) in new_statuses {
        let changed = cached_statuses
            .get(instrument_id)
            .is_none_or(|&prev| prev != new_action);

        if changed {
            cached_statuses.insert(*instrument_id, new_action);
            emit_status(sender, *instrument_id, new_action, ts_event, ts_init);
        }
    }

    // Detect symbols removed from the exchange info snapshot
    let removed: Vec<InstrumentId> = cached_statuses
        .keys()
        .filter(|id| !new_statuses.contains_key(id))
        .copied()
        .collect();

    for instrument_id in removed {
        cached_statuses.remove(&instrument_id);
        emit_status(
            sender,
            instrument_id,
            MarketStatusAction::NotAvailableForTrading,
            ts_event,
            ts_init,
        );
    }
}

fn emit_status(
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
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
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(BybitInstrumentStatus::Trading, MarketStatusAction::Trading)]
    #[case(BybitInstrumentStatus::PreLaunch, MarketStatusAction::PreOpen)]
    #[case(BybitInstrumentStatus::PreTrading, MarketStatusAction::PreOpen)]
    #[case(BybitInstrumentStatus::PendingListing, MarketStatusAction::PreOpen)]
    #[case(BybitInstrumentStatus::ListedOnly, MarketStatusAction::PreOpen)]
    #[case(BybitInstrumentStatus::Delivering, MarketStatusAction::PreClose)]
    #[case(BybitInstrumentStatus::Settled, MarketStatusAction::Close)]
    #[case(BybitInstrumentStatus::Closed, MarketStatusAction::Close)]
    #[case(BybitInstrumentStatus::Suspended, MarketStatusAction::Suspend)]
    #[case(
        BybitInstrumentStatus::Other,
        MarketStatusAction::NotAvailableForTrading
    )]
    fn test_bybit_instrument_status_to_market_action(
        #[case] input: BybitInstrumentStatus,
        #[case] expected: MarketStatusAction,
    ) {
        assert_eq!(MarketStatusAction::from(input), expected);
    }

    #[rstest]
    fn test_diff_emits_on_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");

        let mut cached = AHashMap::new();
        cached.insert(id, MarketStatusAction::Trading);

        let mut new_statuses = AHashMap::new();
        new_statuses.insert(id, MarketStatusAction::Halt);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let event = rx.try_recv().expect("expected status event");
        match event {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(status.instrument_id, id);
                assert_eq!(status.action, MarketStatusAction::Halt);
                assert_eq!(status.is_trading, Some(false));
            }
            _ => panic!("expected InstrumentStatus event"),
        }

        assert_eq!(cached.get(&id), Some(&MarketStatusAction::Halt));
    }

    #[rstest]
    fn test_diff_no_emit_when_unchanged() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");

        let mut cached = AHashMap::new();
        cached.insert(id, MarketStatusAction::Trading);

        let mut new_statuses = AHashMap::new();
        new_statuses.insert(id, MarketStatusAction::Trading);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_diff_emits_for_new_symbol() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");

        let mut cached = AHashMap::new();
        let mut new_statuses = AHashMap::new();
        new_statuses.insert(id, MarketStatusAction::Trading);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let event = rx.try_recv().expect("expected status event for new symbol");
        match event {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(status.instrument_id, id);
                assert_eq!(status.action, MarketStatusAction::Trading);
                assert_eq!(status.is_trading, Some(true));
            }
            _ => panic!("expected InstrumentStatus event"),
        }
    }

    #[rstest]
    fn test_diff_emits_not_available_for_removed_symbol() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");

        let mut cached = AHashMap::new();
        cached.insert(id, MarketStatusAction::Trading);

        let new_statuses = AHashMap::new(); // Symbol disappeared

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let event = rx
            .try_recv()
            .expect("expected status event for removed symbol");
        match event {
            DataEvent::InstrumentStatus(status) => {
                assert_eq!(status.instrument_id, id);
                assert_eq!(status.action, MarketStatusAction::NotAvailableForTrading);
                assert_eq!(status.is_trading, Some(false));
            }
            _ => panic!("expected InstrumentStatus event"),
        }

        assert!(!cached.contains_key(&id));
    }
}
