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

use std::sync::Arc;

use nautilus_core::{AtomicMap, AtomicSet};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

pub(super) fn resolve_token_id_from(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_id: InstrumentId,
) -> anyhow::Result<String> {
    let loaded = instruments.load();
    let instrument = loaded
        .get(&instrument_id)
        .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))?;
    Ok(instrument.raw_symbol().as_str().to_string())
}

// Reconciles the WS subscription for `instrument_id` with the union of caller
// intents. Holds `ws_sub_mutex` across the async WS send so concurrent
// subscribe/unsubscribe calls arrive at the WS handler in mutex-release order;
// that makes the final wire state consistent with the last writer.
#[allow(
    clippy::too_many_arguments,
    reason = "shared state comes in as Arc refs"
)]
pub(super) async fn sync_ws_subscription_async(
    instrument_id: InstrumentId,
    token_id_str: String,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    ws_open_tokens: Arc<AtomicSet<Ustr>>,
    ws_sub_mutex: Arc<tokio::sync::Mutex<()>>,
    ws: crate::websocket::client::WsSubscriptionHandle,
) {
    let token_id = Ustr::from(token_id_str.as_str());
    let _guard = ws_sub_mutex.lock().await;

    let wants_subscribe = active_quote_subs.contains(&instrument_id)
        || active_delta_subs.contains(&instrument_id)
        || active_trade_subs.contains(&instrument_id);
    let is_open = ws_open_tokens.contains(&token_id);

    if wants_subscribe && !is_open {
        ws_open_tokens.insert(token_id);

        if let Err(e) = ws.subscribe_market(vec![token_id_str]).await {
            log::error!("Failed to subscribe to market data: {e:?}");
            // Roll back tracked WS state so a retry can take effect.
            ws_open_tokens.remove(&token_id);
        }
    } else if !wants_subscribe && is_open {
        ws_open_tokens.remove(&token_id);

        if let Err(e) = ws.unsubscribe_market(vec![token_id_str]).await {
            log::error!("Failed to unsubscribe from market data: {e:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::websocket::{client::WsSubscriptionHandle, handler::HandlerCommand};

    type ActiveSet = Arc<AtomicSet<InstrumentId>>;
    type OpenTokens = Arc<AtomicSet<Ustr>>;
    type WsMutex = Arc<tokio::sync::Mutex<()>>;

    fn make_handle() -> (
        WsSubscriptionHandle,
        tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        (WsSubscriptionHandle::from_sender(tx), rx)
    }

    fn make_state() -> (ActiveSet, ActiveSet, ActiveSet, OpenTokens, WsMutex) {
        (
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(AtomicSet::new()),
            Arc::new(tokio::sync::Mutex::new(())),
        )
    }

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET")
    }

    fn token_ustr() -> Ustr {
        Ustr::from("0xCOND-0xTOKEN")
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_subscribes_when_intent_present_and_ws_closed() {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes.clone(),
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(open.contains(&token_ustr()));

        match rx.try_recv().expect("expected SubscribeMarket command") {
            HandlerCommand::SubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.symbol.as_str().to_string()]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_unsubscribes_when_intent_absent_and_ws_open() {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();
        open.insert(token_ustr());

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(!open.contains(&token_ustr()));

        match rx.try_recv().expect("expected UnsubscribeMarket command") {
            HandlerCommand::UnsubscribeMarket(ids) => {
                assert_eq!(ids, vec![inst.symbol.as_str().to_string()]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[rstest]
    #[case::intent_matches_open(true, true, false)]
    #[case::no_intent_not_open(false, false, false)]
    #[tokio::test]
    async fn sync_ws_no_op_when_state_already_matches(
        #[case] want: bool,
        #[case] is_open_initial: bool,
        #[case] expect_command: bool,
    ) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();

        if want {
            quotes.insert(inst);
        }

        if is_open_initial {
            open.insert(token_ustr());
        }

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert_eq!(open.contains(&token_ustr()), is_open_initial);
        assert_eq!(rx.try_recv().is_ok(), expect_command);
    }

    #[rstest]
    #[tokio::test]
    async fn sync_ws_rolls_back_open_tokens_on_send_failure() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        drop(rx);
        let ws = WsSubscriptionHandle::from_sender(tx);

        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();
        quotes.insert(inst);

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(!open.contains(&token_ustr()));
    }

    #[rstest]
    #[case::any_kind(true, false, false)]
    #[case::another_kind(false, true, false)]
    #[case::third_kind(false, false, true)]
    #[tokio::test]
    async fn sync_ws_opens_for_any_active_kind(#[case] q: bool, #[case] d: bool, #[case] t: bool) {
        let (ws, mut rx) = make_handle();
        let (quotes, deltas, trades, open, mutex) = make_state();

        let inst = instrument_id();

        if q {
            quotes.insert(inst);
        }

        if d {
            deltas.insert(inst);
        }

        if t {
            trades.insert(inst);
        }

        sync_ws_subscription_async(
            inst,
            inst.symbol.as_str().to_string(),
            quotes,
            deltas,
            trades,
            open.clone(),
            mutex,
            ws,
        )
        .await;

        assert!(open.contains(&token_ustr()));
        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(_))
        ));
    }
}
