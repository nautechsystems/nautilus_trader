use crate::websocket::{client::WsSubscriptionHandle, handler::HandlerCommand};

use super::{super::*, support::*};
use rstest::rstest;

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
