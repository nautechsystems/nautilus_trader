use std::sync::Arc;

use nautilus_common::messages::data::{SubscribeCustomData, UnsubscribeCustomData};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::data::DataType;
use rstest::rstest;

use super::{super::*, support::*};
use crate::common::consts::POLYMARKET_CLIENT_ID;

#[rstest]
fn reset_cancels_old_generation_and_clears_connection_state() {
    let mut client = make_client_for_reset_test();
    let old_token = client.cancellation_token.clone();

    let instrument_id = InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET");
    client.active_quote_subs.insert(instrument_id);
    client.active_delta_subs.insert(instrument_id);
    client.active_trade_subs.insert(instrument_id);
    client.ws_open_tokens.insert(Ustr::from("0xCOND-0xTOKEN"));
    client
        .new_market_inflight_keys
        .insert("btc-updown-5m-1".to_string(), ());
    client
        .pending_snapshot_after_tick_change
        .insert(instrument_id);
    client
        .pending_auto_loads
        .lock()
        .expect("pending_auto_loads mutex poisoned")
        .insert(instrument_id);
    client.auto_load_scheduled.store(true, Ordering::Release);

    client
        .reset()
        .expect("reset should succeed for in-memory state");

    assert!(old_token.is_cancelled());
    assert!(!client.cancellation_token.is_cancelled());

    assert!(client.active_quote_subs.is_empty());
    assert!(client.active_delta_subs.is_empty());
    assert!(client.active_trade_subs.is_empty());
    assert!(client.ws_open_tokens.is_empty());
    assert!(client.new_market_inflight_keys.is_empty());
    assert!(client.pending_snapshot_after_tick_change.is_empty());
    assert!(
        client
            .pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned")
            .is_empty()
    );
    assert!(!client.auto_load_scheduled.load(Ordering::Acquire));
}

#[rstest]
fn new_market_fetch_concurrency_clamps_zero_to_one() {
    let client = make_client_with_fetch_concurrency(0);
    assert_eq!(client.new_market_fetch_semaphore.available_permits(), 1);
    assert_eq!(client.config.new_market_fetch_max_concurrency, 1);
}

#[rstest]
fn new_market_fetch_concurrency_clamps_high_value_to_cap() {
    let client = make_client_with_fetch_concurrency(1_000);
    assert_eq!(
        client.new_market_fetch_semaphore.available_permits(),
        NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP,
    );
    assert_eq!(
        client.config.new_market_fetch_max_concurrency,
        NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP,
    );
}

#[rstest]
fn reset_replaces_new_market_inflight_keys_generation() {
    let mut client = make_client_for_reset_test();
    let old_inflight_keys = client.new_market_inflight_keys.clone();

    old_inflight_keys.insert("cond:0xold".to_string(), ());
    client.reset().expect("reset should succeed");

    client
        .new_market_inflight_keys
        .insert("cond:0xold".to_string(), ());
    old_inflight_keys.remove("cond:0xold");

    assert!(
        client.new_market_inflight_keys.contains_key("cond:0xold"),
        "old-generation guard cleanup should not remove reset-generation dedupe keys",
    );
    assert!(
        !Arc::ptr_eq(&old_inflight_keys, &client.new_market_inflight_keys),
        "reset should replace in-flight dedupe map generation",
    );
}

#[rstest]
fn subscribe_unsupported_custom_data_is_ignored() {
    let mut client = make_client_for_reset_test();
    let data_type = DataType::new("UnsupportedPolymarketCustomData", None, None);

    client
        .subscribe(SubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsupported custom data subscribe should be ignored");

    assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
}

#[rstest]
fn unsubscribe_unsupported_custom_data_is_ignored() {
    let mut client = make_client_for_reset_test();
    let data_type = DataType::new("UnsupportedPolymarketCustomData", None, None);

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsupported custom data unsubscribe should be ignored");

    assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
}

#[rstest]
fn subscribe_custom_rtds_reuses_single_wire_subscription_for_same_symbol() {
    let mut client = make_client_for_reset_test();
    let crypto_upper = rtds_crypto_data_type("BTCUSDT");
    let crypto_lower = rtds_crypto_data_type("btcusdt");

    client
        .subscribe(SubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            crypto_upper,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("first RTDS subscribe");
    client
        .subscribe(SubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            crypto_lower,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("second RTDS subscribe");

    assert_eq!(client.rtds_feed.tracked_subscription_count(), 1);
    assert_eq!(
        client
            .rtds_feed
            .tracked_data_type_count("crypto_prices:btcusdt"),
        2,
    );
}

#[rstest]
fn unsubscribe_custom_rtds_last_reference_removes_wire_subscription() {
    let mut client = make_client_for_reset_test();
    let equity_data_type = rtds_equity_data_type("AAPL");

    client
        .subscribe(SubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            equity_data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("RTDS subscribe");

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            equity_data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("RTDS unsubscribe");

    assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
}

#[rstest]
fn reset_replaces_rtds_feed_generation() {
    let mut client = make_client_for_reset_test();
    let old_feed = client.rtds_feed.clone();
    let data_type = rtds_crypto_data_type("btcusdt");

    client
        .subscribe(SubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("RTDS subscribe");

    assert_eq!(old_feed.tracked_subscription_count(), 1);

    client.reset().expect("reset should succeed");

    assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
    assert_eq!(
        old_feed.tracked_subscription_count(),
        1,
        "old-generation RTDS state should remain isolated from the reset generation",
    );
}
