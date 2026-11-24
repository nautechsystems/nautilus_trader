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

//! Instrument status mapping and polling for the Binance adapter.

use ahash::AHashMap;
use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_model::{
    data::{Data, InstrumentStatus},
    enums::MarketStatusAction,
    identifiers::InstrumentId,
};
use parking_lot::Mutex;

use crate::{
    common::parse::{CONTRACT_TYPE_PERPETUAL, CONTRACT_TYPE_TRADIFI_PERPETUAL},
    spot::sbe::generated::symbol_status::SymbolStatus,
};

// Binance uses 2100-12-25 08:00 UTC or a later value for unscheduled perpetuals
const PERPETUAL_DELIVERY_DATE_MS: i64 = 4_133_404_800_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SymbolTradingStatus {
    pub(crate) action: MarketStatusAction,
    is_trading: bool,
}

impl From<MarketStatusAction> for SymbolTradingStatus {
    fn from(action: MarketStatusAction) -> Self {
        Self {
            action,
            is_trading: action == MarketStatusAction::Trading,
        }
    }
}

/// Serializes status changes and subscription replay through the same event sender.
#[derive(Debug)]
pub(crate) struct InstrumentStatusCache<S>(Mutex<AHashMap<InstrumentId, S>>);

impl<S: Copy + PartialEq + Into<SymbolTradingStatus>> InstrumentStatusCache<S> {
    pub(crate) fn new() -> Self {
        Self(Mutex::default())
    }

    pub(crate) fn seed(&self, statuses: AHashMap<InstrumentId, S>) {
        *self.0.lock() = statuses;
    }

    pub(crate) fn update(
        &self,
        statuses: &AHashMap<InstrumentId, S>,
        sender: &EventSender<DataEvent>,
        clock: &AtomicTime,
    ) {
        let mut cached_statuses = self.0.lock();
        let ts = clock.get_time_ns();
        diff_statuses(statuses, &mut cached_statuses, sender, ts, ts);
    }

    pub(crate) fn replay(
        &self,
        instrument_id: InstrumentId,
        sender: &EventSender<DataEvent>,
        clock: &AtomicTime,
    ) {
        let cached_statuses = self.0.lock();
        if let Some(&status) = cached_statuses.get(&instrument_id) {
            let ts = clock.get_time_ns();
            emit_status(sender, instrument_id, status.into(), ts, ts);
        }
    }
}

/// Maps a trading perpetual with a scheduled delivery to `PreClose` while it remains tradable.
#[must_use]
pub(crate) fn futures_symbol_status(
    action: MarketStatusAction,
    contract_type: &str,
    delivery_date: i64,
) -> SymbolTradingStatus {
    let mut status = SymbolTradingStatus::from(action);
    let perpetual = matches!(
        contract_type,
        CONTRACT_TYPE_PERPETUAL | CONTRACT_TYPE_TRADIFI_PERPETUAL
    );

    if status.is_trading && perpetual && (1..PERPETUAL_DELIVERY_DATE_MS).contains(&delivery_date) {
        status.action = MarketStatusAction::PreClose;
    }

    status
}

impl From<SymbolStatus> for MarketStatusAction {
    fn from(status: SymbolStatus) -> Self {
        match status {
            SymbolStatus::Trading => Self::Trading,
            SymbolStatus::EndOfDay => Self::Close,
            SymbolStatus::Halt => Self::Halt,
            SymbolStatus::Break => Self::Pause,
            SymbolStatus::CancelOnly => Self::Halt,
            SymbolStatus::NonRepresentable | SymbolStatus::NullVal => Self::NotAvailableForTrading,
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
    sender: &EventSender<DataEvent>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    diff_statuses(new_statuses, cached_statuses, sender, ts_event, ts_init);
}

fn diff_statuses<S: Copy + PartialEq + Into<SymbolTradingStatus>>(
    new_statuses: &AHashMap<InstrumentId, S>,
    cached_statuses: &mut AHashMap<InstrumentId, S>,
    sender: &EventSender<DataEvent>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    for (instrument_id, &new_status) in new_statuses {
        let changed = cached_statuses
            .get(instrument_id)
            .is_none_or(|&prev| prev != new_status);
        if changed {
            cached_statuses.insert(*instrument_id, new_status);
            emit_status(sender, *instrument_id, new_status.into(), ts_event, ts_init);
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
        let status = MarketStatusAction::NotAvailableForTrading.into();
        emit_status(sender, instrument_id, status, ts_event, ts_init);
    }
}

fn emit_status(
    sender: &EventSender<DataEvent>,
    instrument_id: InstrumentId,
    trading_status: SymbolTradingStatus,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let status = InstrumentStatus::new(
        instrument_id,
        trading_status.action,
        ts_event,
        ts_init,
        None,
        None,
        Some(trading_status.is_trading),
        None,
        None,
    );

    if let Err(e) = sender.send(DataEvent::Data(Data::InstrumentStatus(status))) {
        log::error!("Failed to emit instrument status event: {e}");
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::MarketStatusAction::{Close, PreClose, Trading},
        identifiers::InstrumentId,
    };
    use rstest::rstest;

    use super::{
        super::enums::{BinanceContractStatus, BinanceTradingStatus},
        *,
    };

    #[rstest]
    #[case(SymbolStatus::Trading, MarketStatusAction::Trading)]
    #[case(SymbolStatus::EndOfDay, MarketStatusAction::Close)]
    #[case(SymbolStatus::Halt, MarketStatusAction::Halt)]
    #[case(SymbolStatus::Break, MarketStatusAction::Pause)]
    #[case(SymbolStatus::CancelOnly, MarketStatusAction::Halt)]
    #[case(
        SymbolStatus::NonRepresentable,
        MarketStatusAction::NotAvailableForTrading
    )]
    #[case(SymbolStatus::NullVal, MarketStatusAction::NotAvailableForTrading)]
    fn test_symbol_status_to_market_action(
        #[case] input: SymbolStatus,
        #[case] expected: MarketStatusAction,
    ) {
        assert_eq!(MarketStatusAction::from(input), expected);
    }

    #[rstest]
    #[case(BinanceTradingStatus::Trading, MarketStatusAction::Trading)]
    #[case(BinanceTradingStatus::PendingTrading, MarketStatusAction::PreOpen)]
    #[case(BinanceTradingStatus::PreTrading, MarketStatusAction::PreOpen)]
    #[case(BinanceTradingStatus::PostTrading, MarketStatusAction::PostClose)]
    #[case(BinanceTradingStatus::EndOfDay, MarketStatusAction::Close)]
    #[case(BinanceTradingStatus::Halt, MarketStatusAction::Halt)]
    #[case(BinanceTradingStatus::AuctionMatch, MarketStatusAction::Cross)]
    #[case(BinanceTradingStatus::Break, MarketStatusAction::Pause)]
    #[case(BinanceTradingStatus::PreDelivering, MarketStatusAction::PreClose)]
    #[case(BinanceTradingStatus::Delivering, MarketStatusAction::Close)]
    #[case(BinanceTradingStatus::Delivered, MarketStatusAction::Close)]
    #[case(BinanceTradingStatus::PreSettle, MarketStatusAction::PreClose)]
    #[case(BinanceTradingStatus::Settling, MarketStatusAction::Close)]
    #[case(BinanceTradingStatus::Close, MarketStatusAction::Close)]
    #[case(BinanceTradingStatus::TradingHalt, MarketStatusAction::Halt)]
    #[case(BinanceTradingStatus::TradingCancelOnly, MarketStatusAction::Halt)]
    #[case(
        BinanceTradingStatus::Unknown,
        MarketStatusAction::NotAvailableForTrading
    )]
    fn test_trading_status_to_market_action(
        #[case] input: BinanceTradingStatus,
        #[case] expected: MarketStatusAction,
    ) {
        assert_eq!(MarketStatusAction::from(input), expected);
    }

    #[rstest]
    #[case(BinanceContractStatus::Trading, MarketStatusAction::Trading)]
    #[case(BinanceContractStatus::PendingTrading, MarketStatusAction::PreOpen)]
    #[case(BinanceContractStatus::PreDelivering, MarketStatusAction::PreClose)]
    #[case(BinanceContractStatus::Delivering, MarketStatusAction::Close)]
    #[case(BinanceContractStatus::Delivered, MarketStatusAction::Close)]
    #[case(BinanceContractStatus::PreSettle, MarketStatusAction::PreClose)]
    #[case(BinanceContractStatus::Settling, MarketStatusAction::Close)]
    #[case(BinanceContractStatus::Close, MarketStatusAction::Close)]
    #[case(BinanceContractStatus::TradingHalt, MarketStatusAction::Halt)]
    #[case(BinanceContractStatus::TradingCancelOnly, MarketStatusAction::Halt)]
    #[case(BinanceContractStatus::PreDelisting, MarketStatusAction::PreClose)]
    #[case(BinanceContractStatus::Delisting, MarketStatusAction::Suspend)]
    #[case(
        BinanceContractStatus::Down,
        MarketStatusAction::NotAvailableForTrading
    )]
    #[case(
        BinanceContractStatus::Unknown,
        MarketStatusAction::NotAvailableForTrading
    )]
    fn test_contract_status_to_market_action(
        #[case] input: BinanceContractStatus,
        #[case] expected: MarketStatusAction,
    ) {
        assert_eq!(MarketStatusAction::from(input), expected);
    }

    #[rstest]
    #[case::perpetual_scheduled(Trading, "PERPETUAL", 1_793_178_000_000, PreClose, true)]
    #[case::tradfi_perpetual_scheduled(
        Trading,
        "TRADIFI_PERPETUAL",
        1_793_178_000_000,
        PreClose,
        true
    )]
    #[case::perpetual_unscheduled(Trading, "PERPETUAL", 4_133_404_800_000, Trading, true)]
    #[case::btc_demo_unscheduled(Trading, "PERPETUAL", 4_133_404_802_000, Trading, true)]
    #[case::fet_demo_unscheduled(Trading, "PERPETUAL", 4_133_408_400_000, Trading, true)]
    #[case::no_delivery_date(Trading, "PERPETUAL", 0, Trading, true)]
    #[case::perpetual_pre_settle(PreClose, "PERPETUAL", 1_793_178_000_000, PreClose, false)]
    #[case::perpetual_settling(Close, "PERPETUAL", 1_793_178_000_000, Close, false)]
    #[case::delivery_contract(Trading, "CURRENT_QUARTER", 1_793_178_000_000, Trading, true)]
    fn test_futures_symbol_status(
        #[case] action: MarketStatusAction,
        #[case] contract_type: &str,
        #[case] delivery_date: i64,
        #[case] expected_action: MarketStatusAction,
        #[case] is_trading: bool,
    ) {
        assert_eq!(
            futures_symbol_status(action, contract_type, delivery_date),
            SymbolTradingStatus {
                action: expected_action,
                is_trading
            },
        );
    }

    #[rstest]
    fn test_status_update_emits_when_only_trading_flag_changes() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = tx.into();
        let id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ts = UnixNanos::from(42);
        let clock = AtomicTime::new(false, ts);
        let cache = InstrumentStatusCache::new();
        let scheduled = futures_symbol_status(Trading, "PERPETUAL", 1_793_178_000_000);
        cache.seed(AHashMap::from_iter([(id, scheduled)]));

        let pre_settle = SymbolTradingStatus::from(PreClose);
        let statuses = AHashMap::from_iter([(id, pre_settle)]);
        cache.update(&statuses, &sender, &clock);

        let DataEvent::Data(Data::InstrumentStatus(status)) = rx.try_recv().unwrap() else {
            panic!("Expected an instrument status event");
        };

        let expected =
            InstrumentStatus::new(id, PreClose, ts, ts, None, None, Some(false), None, None);
        assert_eq!(status, expected);
    }

    #[rstest]
    fn test_diff_emits_on_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("BTCUSDT.BINANCE");

        let mut cached = AHashMap::new();
        cached.insert(id, MarketStatusAction::Trading);

        let mut new_statuses = AHashMap::new();
        new_statuses.insert(id, MarketStatusAction::Halt);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let event = rx.try_recv().expect("expected status event");
        match event {
            DataEvent::Data(Data::InstrumentStatus(status)) => {
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
        let id = InstrumentId::from("BTCUSDT.BINANCE");

        let mut cached = AHashMap::new();
        cached.insert(id, MarketStatusAction::Trading);

        let mut new_statuses = AHashMap::new();
        new_statuses.insert(id, MarketStatusAction::Trading);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        rx.try_recv().unwrap_err();
    }

    #[rstest]
    fn test_diff_emits_for_new_symbol() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = InstrumentId::from("ETHUSDT.BINANCE");

        let mut cached = AHashMap::new();
        let mut new_statuses = AHashMap::new();
        new_statuses.insert(id, MarketStatusAction::Trading);

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let event = rx.try_recv().expect("expected status event for new symbol");
        match event {
            DataEvent::Data(Data::InstrumentStatus(status)) => {
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
        let id = InstrumentId::from("BTCUSDT.BINANCE");

        let mut cached = AHashMap::new();
        cached.insert(id, MarketStatusAction::Trading);

        let new_statuses = AHashMap::new(); // Symbol disappeared

        diff_and_emit_statuses(
            &new_statuses,
            &mut cached,
            &tx.into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let event = rx
            .try_recv()
            .expect("expected status event for removed symbol");

        match event {
            DataEvent::Data(Data::InstrumentStatus(status)) => {
                assert_eq!(status.instrument_id, id);
                assert_eq!(status.action, MarketStatusAction::NotAvailableForTrading);
                assert_eq!(status.is_trading, Some(false));
            }
            _ => panic!("expected InstrumentStatus event"),
        }

        assert!(!cached.contains_key(&id));
    }
}
