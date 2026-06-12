use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use axum::{
    Router,
    extract::{RawQuery, State},
    response::Json,
    routing::get,
};
use rstest::rstest;
use serde_json::Value;

use super::{super::*, support::*};
use crate::websocket::messages::{MarketWsMessage, PolymarketNewMarket};

fn make_new_market(slug: &str, active: bool) -> MarketWsMessage {
    make_new_market_with_ids(
        slug,
        &format!("cond-{slug}"),
        &format!("cond-{slug}"),
        active,
    )
}

fn make_new_market_with_condition(slug: &str, condition_id: &str, active: bool) -> MarketWsMessage {
    make_new_market_with_ids(slug, condition_id, condition_id, active)
}

fn make_new_market_with_ids(
    slug: &str,
    market: &str,
    condition_id: &str,
    active: bool,
) -> MarketWsMessage {
    MarketWsMessage::NewMarket(Box::new(PolymarketNewMarket {
        id: format!("id-{slug}"),
        question: format!("Will {slug} settle true?"),
        market: Ustr::from(market),
        slug: slug.to_string(),
        description: format!("desc-{slug}"),
        assets_ids: vec![format!("yes-{slug}"), format!("no-{slug}")],
        outcomes: vec!["Yes".to_string(), "No".to_string()],
        timestamp: "1700000003000".to_string(),
        tags: vec![],
        condition_id: condition_id.to_string(),
        active,
        clob_token_ids: vec![format!("yes-{slug}"), format!("no-{slug}")],
        order_price_min_tick_size: None,
        group_item_title: None,
        event_message: None,
    }))
}

fn gamma_market_fixture_value() -> Value {
    serde_json::from_str(include_str!("../../../test_data/gamma_market.json"))
        .expect("gamma market fixture json")
}

#[derive(Clone, Default)]
struct NewMarketFetchTestServerState {
    total_requests: Arc<AtomicUsize>,
    inflight_requests: Arc<AtomicUsize>,
    max_inflight_requests: Arc<AtomicUsize>,
    seen_condition_ids: Arc<StdMutex<Vec<Option<String>>>>,
    seen_slugs: Arc<StdMutex<Vec<Option<String>>>>,
    empty_then_success_condition_id: Arc<StdMutex<Option<String>>>,
    empty_then_success_payload: Arc<StdMutex<Option<Value>>>,
    per_condition_requests: Arc<StdMutex<AHashMap<String, usize>>>,
    response_delay_ms: u64,
}

fn query_param(raw_query: Option<String>, key: &str) -> Option<String> {
    let raw = raw_query?;
    raw.split('&').find_map(|pair| {
        let mut parts = pair.splitn(2, '=');
        let pair_key = parts.next().unwrap_or("");
        if pair_key != key {
            return None;
        }
        Some(parts.next().unwrap_or("").to_string())
    })
}

async fn handle_new_market_gamma_markets(
    RawQuery(raw_query): RawQuery,
    State(state): State<NewMarketFetchTestServerState>,
) -> Json<Value> {
    state.total_requests.fetch_add(1, Ordering::SeqCst);
    let inflight = state.inflight_requests.fetch_add(1, Ordering::SeqCst) + 1;
    let condition_id = query_param(raw_query.clone(), "condition_ids");
    let slug = query_param(raw_query, "slug");

    state
        .seen_condition_ids
        .lock()
        .expect("seen_condition_ids mutex poisoned")
        .push(condition_id.clone());
    state
        .seen_slugs
        .lock()
        .expect("seen_slugs mutex poisoned")
        .push(slug);

    loop {
        let prev = state.max_inflight_requests.load(Ordering::SeqCst);
        if inflight <= prev {
            break;
        }

        if state
            .max_inflight_requests
            .compare_exchange(prev, inflight, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            break;
        }
    }

    if state.response_delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(state.response_delay_ms)).await;
    }

    let response = if let Some(ref cid) = condition_id {
        let next_count = {
            let mut counts = state
                .per_condition_requests
                .lock()
                .expect("per_condition_requests mutex poisoned");
            let next = counts.get(cid).copied().unwrap_or(0) + 1;
            counts.insert(cid.clone(), next);
            next
        };

        let target_cid = state
            .empty_then_success_condition_id
            .lock()
            .expect("empty_then_success_condition_id mutex poisoned")
            .clone();

        if target_cid.as_deref() == Some(cid.as_str()) && next_count >= 2 {
            state
                .empty_then_success_payload
                .lock()
                .expect("empty_then_success_payload mutex poisoned")
                .clone()
                .unwrap_or_else(|| serde_json::json!([]))
        } else {
            serde_json::json!([])
        }
    } else {
        serde_json::json!([])
    };

    state.inflight_requests.fetch_sub(1, Ordering::SeqCst);
    Json(response)
}

async fn start_new_market_test_server(state: NewMarketFetchTestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr");
    let router = Router::new()
        .route("/markets", get(handle_new_market_gamma_markets))
        .with_state(state);

    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
    addr
}

#[rstest]
#[tokio::test]
async fn new_market_dedupes_same_slug_and_cleans_inflight_on_cancel() {
    let state = NewMarketFetchTestServerState::default();
    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(0));

    PolymarketDataClient::handle_market_message(make_new_market("btc-updown-5m-1", true), &ctx);
    PolymarketDataClient::handle_market_message(make_new_market("btc-updown-5m-1", true), &ctx);

    assert_eq!(state.total_requests.load(Ordering::SeqCst), 0);
    assert_eq!(ctx.new_market_inflight_keys.len(), 1);
    assert!(
        ctx.new_market_inflight_keys
            .contains_key("cond:cond-btc-updown-5m-1")
    );

    ctx.cancellation_token.cancel();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        if ctx.new_market_inflight_keys.is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected in-flight key cleanup after cancellation"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[rstest]
#[tokio::test]
async fn new_market_fetches_respect_global_concurrency_cap() {
    let state = NewMarketFetchTestServerState {
        response_delay_ms: 150,
        ..NewMarketFetchTestServerState::default()
    };
    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

    let slug_count = 6usize;
    for idx in 0..slug_count {
        let slug = format!("asset-{idx}-updown-5m-1");
        PolymarketDataClient::handle_market_message(make_new_market(&slug, true), &ctx);
    }

    let expected_requests = slug_count * (1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);

    loop {
        let done = state.total_requests.load(Ordering::SeqCst) >= expected_requests
            && state.inflight_requests.load(Ordering::SeqCst) == 0
            && ctx.new_market_inflight_keys.is_empty();

        if done {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for new market fetch tasks to complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(
        state.total_requests.load(Ordering::SeqCst),
        expected_requests,
    );
    assert_eq!(state.max_inflight_requests.load(Ordering::SeqCst), 1);
}

#[rstest]
#[tokio::test]
async fn new_market_same_slug_can_refetch_after_previous_completion() {
    let state = NewMarketFetchTestServerState {
        response_delay_ms: 50,
        ..NewMarketFetchTestServerState::default()
    };
    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

    let slug = "btc-updown-5m-2";
    let dedupe_key = "cond:cond-btc-updown-5m-2";
    PolymarketDataClient::handle_market_message(make_new_market(slug, true), &ctx);

    let per_fetch_requests = 1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS;
    let deadline_first = tokio::time::Instant::now() + Duration::from_secs(3);

    loop {
        let first_done = state.total_requests.load(Ordering::SeqCst) >= per_fetch_requests
            && state.inflight_requests.load(Ordering::SeqCst) == 0
            && !ctx.new_market_inflight_keys.contains_key(dedupe_key);

        if first_done {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline_first,
            "timed out waiting for first slug fetch to complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    PolymarketDataClient::handle_market_message(make_new_market(slug, true), &ctx);

    let deadline_second = tokio::time::Instant::now() + Duration::from_secs(3);

    loop {
        let second_done = state.total_requests.load(Ordering::SeqCst) >= per_fetch_requests * 2
            && state.inflight_requests.load(Ordering::SeqCst) == 0
            && !ctx.new_market_inflight_keys.contains_key(dedupe_key);

        if second_done {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline_second,
            "timed out waiting for second slug fetch to complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(
        state.total_requests.load(Ordering::SeqCst),
        per_fetch_requests * 2,
    );
}

#[rstest]
#[tokio::test]
async fn new_market_cancellation_during_fetch_cleans_inflight_slug() {
    let state = NewMarketFetchTestServerState {
        response_delay_ms: 500,
        ..NewMarketFetchTestServerState::default()
    };
    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

    let slug = "eth-updown-5m-cancel";
    let dedupe_key = "cond:cond-eth-updown-5m-cancel";
    PolymarketDataClient::handle_market_message(make_new_market(slug, true), &ctx);

    let deadline_started = tokio::time::Instant::now() + Duration::from_secs(2);

    loop {
        let started = state.inflight_requests.load(Ordering::SeqCst) > 0
            && ctx.new_market_inflight_keys.contains_key(dedupe_key);

        if started {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline_started,
            "timed out waiting for in-flight fetch to begin"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    ctx.cancellation_token.cancel();

    let deadline_cleanup = tokio::time::Instant::now() + Duration::from_secs(2);

    loop {
        if ctx.new_market_inflight_keys.is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline_cleanup,
            "expected in-flight key cleanup after cancellation during fetch"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(
        state.max_inflight_requests.load(Ordering::SeqCst) <= 1,
        "fetch concurrency exceeded configured cap during cancellation path"
    );
}

#[rstest]
#[tokio::test]
async fn new_market_dedupes_mixed_slugs_when_condition_id_matches() {
    let state = NewMarketFetchTestServerState::default();
    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(0));

    let condition_id = "0xabc123";
    PolymarketDataClient::handle_market_message(
        make_new_market_with_condition("btc-updown-5m-window-a", condition_id, true),
        &ctx,
    );
    PolymarketDataClient::handle_market_message(
        make_new_market_with_condition("btc-updown-5m-window-b", condition_id, true),
        &ctx,
    );

    assert_eq!(state.total_requests.load(Ordering::SeqCst), 0);
    assert_eq!(ctx.new_market_inflight_keys.len(), 1);
    assert!(
        ctx.new_market_inflight_keys.contains_key("cond:0xabc123"),
        "mixed slug events with same condition_id should dedupe to one in-flight fetch",
    );
}

#[rstest]
#[tokio::test]
async fn new_market_fetch_prefers_condition_id_query_over_slug_query() {
    let state = NewMarketFetchTestServerState::default();
    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

    PolymarketDataClient::handle_market_message(
        make_new_market_with_ids(
            "btc-updown-5m-query-check",
            "0xmarket-condition-query",
            "0xcondition-query",
            true,
        ),
        &ctx,
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

    loop {
        let done = state.total_requests.load(Ordering::SeqCst) >= 1
            && state.inflight_requests.load(Ordering::SeqCst) == 0
            && ctx.new_market_inflight_keys.is_empty();

        if done {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for condition_id query fetch to complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let condition_ids = state
        .seen_condition_ids
        .lock()
        .expect("seen_condition_ids mutex poisoned");
    let slugs = state.seen_slugs.lock().expect("seen_slugs mutex poisoned");
    assert_eq!(
        condition_ids.len(),
        1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS,
    );
    assert_eq!(slugs.len(), 1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS);
    assert!(
        condition_ids
            .iter()
            .all(|cid| cid.as_deref() == Some("0xcondition-query")),
    );
    assert_eq!(
        slugs.iter().filter(|slug| slug.is_none()).count(),
        1 + NEW_MARKET_EMPTY_RECHECK_MAX_ATTEMPTS,
        "condition-aware path should not send slug query for new_market fetch"
    );
}

#[rstest]
#[tokio::test]
async fn new_market_fetch_falls_back_to_slug_when_identifiers_missing() {
    let state = NewMarketFetchTestServerState::default();
    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, _data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

    PolymarketDataClient::handle_market_message(
        make_new_market_with_ids("btc-updown-5m-slug-fallback", "", "", true),
        &ctx,
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

    loop {
        let done = state.total_requests.load(Ordering::SeqCst) >= 1
            && state.inflight_requests.load(Ordering::SeqCst) == 0
            && ctx.new_market_inflight_keys.is_empty();

        if done {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for slug fallback fetch to complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let condition_ids = state
        .seen_condition_ids
        .lock()
        .expect("seen_condition_ids mutex poisoned");
    let slugs = state.seen_slugs.lock().expect("seen_slugs mutex poisoned");
    assert_eq!(condition_ids.len(), 1);
    assert_eq!(slugs.len(), 1);
    assert_eq!(condition_ids[0], None);
    assert_eq!(slugs[0].as_deref(), Some("btc-updown-5m-slug-fallback"));
}

#[rstest]
#[tokio::test]
async fn new_market_condition_empty_then_success_recheck_loads_instrument() {
    let state = NewMarketFetchTestServerState::default();
    let target_condition = "0xcondition-recheck";
    *state
        .empty_then_success_condition_id
        .lock()
        .expect("empty_then_success_condition_id mutex poisoned") =
        Some(target_condition.to_string());
    *state
        .empty_then_success_payload
        .lock()
        .expect("empty_then_success_payload mutex poisoned") =
        Some(serde_json::json!([gamma_market_fixture_value()]));

    let addr = start_new_market_test_server(state.clone()).await;
    let gamma_base_url = format!("http://{addr}");
    let (mut ctx, mut data_rx) = make_ws_ctx_with_gamma_base_url(&gamma_base_url);
    ctx.subscribe_new_markets = true;
    ctx.new_market_fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(1));

    PolymarketDataClient::handle_market_message(
        make_new_market_with_ids(
            "btc-updown-5m-recheck",
            target_condition,
            target_condition,
            true,
        ),
        &ctx,
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let done = state.total_requests.load(Ordering::SeqCst) >= 2
            && state.inflight_requests.load(Ordering::SeqCst) == 0
            && ctx.new_market_inflight_keys.is_empty()
            && !ctx.instruments.load().is_empty();

        if done {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for empty-then-success recheck flow",
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let seen_condition_ids = state
        .seen_condition_ids
        .lock()
        .expect("seen_condition_ids mutex poisoned")
        .clone();
    assert!(
        seen_condition_ids
            .iter()
            .all(|cid| cid.as_deref() == Some(target_condition)),
        "all requests should query target condition_id, saw: {seen_condition_ids:?}",
    );
    assert_eq!(
        state.total_requests.load(Ordering::SeqCst),
        2,
        "single recheck policy should perform exactly two condition fetch attempts",
    );

    let mut emitted_instrument = false;

    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_millis(200), data_rx.recv()).await
    {
        if matches!(event, DataEvent::Instrument(_)) {
            emitted_instrument = true;
            break;
        }
    }
    assert!(
        emitted_instrument,
        "expected emitted DataEvent::Instrument after successful recheck"
    );
}

#[rstest]
fn new_market_dedupe_key_prefers_condition_then_market_then_slug() {
    let MarketWsMessage::NewMarket(mut nm) =
        make_new_market_with_condition("btc-updown-5m-window-a", "0xcond123", true)
    else {
        panic!("expected new_market message");
    };

    assert_eq!(
        PolymarketDataClient::new_market_dedupe_key(&nm),
        "cond:0xcond123"
    );

    nm.condition_id.clear();
    nm.market = Ustr::from("0xmarket456");
    assert_eq!(
        PolymarketDataClient::new_market_dedupe_key(&nm),
        "market:0xmarket456"
    );

    nm.market = Ustr::from("");
    nm.slug = "btc-updown-5m-window-b".to_string();
    assert_eq!(
        PolymarketDataClient::new_market_dedupe_key(&nm),
        "slug:btc-updown-5m-window-b"
    );
}
