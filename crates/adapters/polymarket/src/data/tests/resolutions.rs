use std::{net::SocketAddr, sync::Arc, time::Duration as StdDuration};

use ahash::AHashMap;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
};
use nautilus_common::{
    live::runner::replace_data_event_sender,
    messages::{DataResponse, data::RequestCustomData},
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{CustomData as ModelCustomData, Data as NautilusData, DataType},
    enums::InstrumentCloseType,
    identifiers::{ClientId, PositionId},
    types::{Price, Quantity},
};
use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
use rstest::rstest;
use serde_json::Value;

use super::{super::*, support::*};
use crate::{
    common::consts::POLYMARKET_CLIENT_ID,
    config::PolymarketDataClientConfig,
    http::{clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient},
    resolve::{
        PolymarketResolveRequestSummaryData, RESOLVE_REQUEST_TYPE_NAME, ResolveBatchErrorMode,
        fetch_and_apply_resolutions_by_condition_ids, pause_resolve_watch_entries,
        update_resolve_watchlist_from_position_event, upsert_resolve_watch_entry_from_instrument,
    },
    websocket::{
        client::PolymarketWebSocketClient,
        messages::{MarketWsMessage, PolymarketMarketResolved},
    },
};

fn make_market_resolved(
    condition_id: &str,
    winner_asset_id: &str,
    loser_asset_id: &str,
) -> MarketWsMessage {
    MarketWsMessage::MarketResolved(PolymarketMarketResolved {
        id: "resolved-1".to_string(),
        market: Ustr::from(condition_id),
        assets_ids: vec![winner_asset_id.to_string(), loser_asset_id.to_string()],
        winning_asset_id: winner_asset_id.to_string(),
        winning_outcome: "Yes".to_string(),
        timestamp: "1700000004000".to_string(),
        tags: vec![],
    })
}

fn make_gamma_market_value_with_outcome_prices(
    condition_id: &str,
    clob_token_ids: &str,
    outcome_prices: Option<&str>,
    closed: Option<bool>,
    accepting_orders: Option<bool>,
) -> Value {
    let mut value = serde_json::json!({
        "id": "1557558",
        "conditionId": condition_id,
        "questionID": "0xquestion",
        "clobTokenIds": clob_token_ids,
        "outcomes": "[\"Yes\",\"No\"]",
        "question": "Will test pass?",
        "description": null,
        "startDate": null,
        "endDate": null,
        "active": false,
        "closed": closed,
        "acceptingOrders": accepting_orders,
        "enableOrderBook": false,
        "slug": "test-market",
        "events": []
    });

    if let Some(outcome_prices) = outcome_prices {
        value["outcomePrices"] = serde_json::Value::String(outcome_prices.to_string());
    }

    value
}

fn make_clob_market_value(
    condition_id: &str,
    winner_token_id: &str,
    loser_token_id: &str,
    closed: bool,
) -> Value {
    serde_json::json!({
        "condition_id": condition_id,
        "closed": closed,
        "tokens": [
            {"token_id": winner_token_id, "outcome": "Yes", "winner": true},
            {"token_id": loser_token_id, "outcome": "No", "winner": false}
        ]
    })
}

#[derive(Clone, Default)]
struct TestServerState {
    gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    clob_market_by_condition: Arc<tokio::sync::Mutex<AHashMap<String, Value>>>,
}

async fn handle_gamma_markets(State(state): State<TestServerState>) -> Json<Value> {
    let body = state
        .gamma_response
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| serde_json::json!([]));
    Json(body)
}

async fn handle_clob_market(
    State(state): State<TestServerState>,
    Path(condition_id): Path<String>,
) -> (StatusCode, Json<Value>) {
    let body = state.clob_market_by_condition.lock().await;
    if let Some(value) = body.get(&condition_id) {
        (StatusCode::OK, Json(value.clone()))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":"market not found"})),
        )
    }
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr");
    let router = Router::new()
        .route("/markets", get(handle_gamma_markets))
        .route("/markets/{condition_id}", get(handle_clob_market))
        .with_state(state);

    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
    addr
}

fn create_test_client(
    addr: SocketAddr,
) -> (
    PolymarketDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(tx);

    let base_url = format!("http://{addr}");
    let gamma = PolymarketGammaHttpClient::new(Some(base_url.clone()), 5, RetryConfig::default())
        .expect("gamma client");
    let clob_public =
        PolymarketClobPublicClient::new(Some(base_url.clone()), 5).expect("clob client");
    let data_api =
        PolymarketDataApiHttpClient::new(Some(base_url.clone()), 5).expect("data api client");
    let ws = PolymarketWebSocketClient::new_market(
        Some(format!("ws://{addr}/ws/market")),
        false,
        TransportBackend::default(),
    );

    let config = PolymarketDataClientConfig {
        base_url_http: Some(base_url.clone()),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        base_url_gamma: Some(base_url.clone()),
        base_url_data_api: Some(base_url),
        resolve_poll_enabled: false,
        ..PolymarketDataClientConfig::default()
    };

    let client = PolymarketDataClient::new(
        *POLYMARKET_CLIENT_ID,
        config,
        gamma,
        clob_public,
        data_api,
        ws,
    );

    (client, rx)
}

#[rstest]
fn market_resolved_emits_grouped_close_and_removes_watch_entry() {
    let (ctx, mut data_rx) = make_ws_ctx();
    let expiration_ns = UnixNanos::from(1_000_000_000);
    let yes = seed_instrument_with_context(
        &ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            market_slug: Some("btc-updown-5m"),
            market_id: Some("1778973900"),
            condition_id: Some("0xCOND-BTC"),
            expiration_ns: Some(expiration_ns),
        },
    );
    let no = seed_instrument_with_context(
        &ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            market_slug: Some("btc-updown-5m"),
            market_id: Some("1778973900"),
            condition_id: Some("0xCOND-BTC"),
            expiration_ns: Some(expiration_ns),
        },
    );

    update_resolve_watchlist_from_position_event(
        &ctx.resolve_poll_watchlist,
        &ctx.instruments,
        &stub_position_opened_event(yes.id()),
    );
    update_resolve_watchlist_from_position_event(
        &ctx.resolve_poll_watchlist,
        &ctx.instruments,
        &stub_position_opened_event(no.id()),
    );

    PolymarketDataClient::handle_market_message(
        make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO"),
        &ctx,
    );

    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    let statuses = events
        .iter()
        .filter(|event| matches!(event, DataEvent::InstrumentStatus(_)))
        .count();
    assert_eq!(statuses, 2);

    let mut yes_close = None;
    let mut no_close = None;

    for event in events {
        if let DataEvent::Data(NautilusData::InstrumentClose(close)) = event {
            if close.instrument_id == yes.id() {
                yes_close = Some(close);
            } else if close.instrument_id == no.id() {
                no_close = Some(close);
            }
        }
    }

    let yes_close = yes_close.expect("expected yes close");
    let no_close = no_close.expect("expected no close");
    assert_eq!(yes_close.close_type, InstrumentCloseType::ContractExpired);
    assert_eq!(no_close.close_type, InstrumentCloseType::ContractExpired);
    assert_eq!(
        yes_close.close_price.as_decimal(),
        rust_decimal::Decimal::ONE
    );
    assert_eq!(
        no_close.close_price.as_decimal(),
        rust_decimal::Decimal::ZERO
    );
    assert!(
        !ctx.resolve_poll_watchlist
            .contains_key(&"0xCOND-BTC".to_string())
    );
}

#[rstest]
fn duplicate_market_resolved_after_watch_removal_is_a_noop() {
    let (ctx, mut data_rx) = make_ws_ctx();
    let yes = seed_instrument_with_context(
        &ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-BTC"),
            expiration_ns: Some(UnixNanos::from(1_000_000_000)),
            ..SeedInstrumentContext::default()
        },
    );

    update_resolve_watchlist_from_position_event(
        &ctx.resolve_poll_watchlist,
        &ctx.instruments,
        &stub_position_opened_event(yes.id()),
    );

    let resolved = make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO");
    PolymarketDataClient::handle_market_message(resolved.clone(), &ctx);
    let _ = std::iter::from_fn(|| data_rx.try_recv().ok()).collect::<Vec<_>>();

    PolymarketDataClient::handle_market_message(resolved, &ctx);
    assert!(data_rx.try_recv().is_err());
}

#[rstest]
fn market_resolved_emit_failure_merges_watch_entry_back() {
    let (ctx, data_rx) = make_ws_ctx();
    let expiration_ns = UnixNanos::from(1_000_000_000);
    let yes = seed_instrument_with_context(
        &ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            market_slug: Some("btc-updown-5m"),
            market_id: Some("1778973900"),
            condition_id: Some("0xCOND-BTC"),
            expiration_ns: Some(expiration_ns),
        },
    );
    let no = seed_instrument_with_context(
        &ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            market_slug: Some("btc-updown-5m"),
            market_id: Some("1778973900"),
            condition_id: Some("0xCOND-BTC"),
            expiration_ns: Some(expiration_ns),
        },
    );

    update_resolve_watchlist_from_position_event(
        &ctx.resolve_poll_watchlist,
        &ctx.instruments,
        &stub_position_opened_event(yes.id()),
    );
    update_resolve_watchlist_from_position_event(
        &ctx.resolve_poll_watchlist,
        &ctx.instruments,
        &stub_position_opened_event(no.id()),
    );

    drop(data_rx);

    PolymarketDataClient::handle_market_message(
        make_market_resolved("0xCOND-BTC", "0xTOKEN_YES", "0xTOKEN_NO"),
        &ctx,
    );

    let watchlist = ctx.resolve_poll_watchlist.load();
    let entry = watchlist
        .get("0xCOND-BTC")
        .expect("expected watch entry restored after emit failure");
    assert_eq!(entry.tracked.len(), 2);
}

#[rstest]
#[tokio::test]
async fn request_data_manual_fallback_resolves_paused_entries() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([
        make_gamma_market_value_with_outcome_prices(
            "0xCOND-REQ",
            "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        )
    ]));
    let addr = start_mock_server(state).await;
    let (client, mut data_rx) = create_test_client(addr);
    let ws_ctx = make_client_ws_ctx(&client);

    let expiration_ns = UnixNanos::from(1_000_000_000);
    let inst_yes = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    let inst_no = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );

    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_yes,
        PositionId::new("P-1"),
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_no,
        PositionId::new("P-2"),
    );
    pause_resolve_watch_entries(&client.resolve_poll_watchlist, &["0xCOND-REQ".to_string()]);

    let request = RequestCustomData::new(
        ClientId::from("POLYMARKET"),
        DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_data(request).expect("request_data");

    wait_until_async(
        || async {
            !client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-REQ".to_string())
        },
        StdDuration::from_secs(5),
    )
    .await;

    let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
        events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
    })
    .await;

    assert!(
        events.iter().any(is_resolve_response),
        "expected custom data response, received: {events:?}"
    );
    let response = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Response(DataResponse::Data(response)) => Some(response),
            _ => None,
        })
        .expect("expected custom data response");
    let custom = response
        .data
        .as_ref()
        .downcast_ref::<ModelCustomData>()
        .expect("expected CustomData response payload");
    assert_eq!(custom.data_type.type_name(), RESOLVE_REQUEST_TYPE_NAME);
    let summary = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketResolveRequestSummaryData>()
        .expect("expected resolve summary payload");
    assert_eq!(
        summary.emitted_condition_ids,
        vec!["0xCOND-REQ".to_string()]
    );
    let closes = count_instrument_close_events(&events);
    assert_eq!(closes, 2);
}

#[rstest]
#[tokio::test]
async fn request_data_manual_fallback_with_auto_poll_disabled_resolves_expired_entries() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([
        make_gamma_market_value_with_outcome_prices(
            "0xCOND-REQ",
            "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        )
    ]));
    let addr = start_mock_server(state).await;
    let (client, mut data_rx) = create_test_client(addr);
    let ws_ctx = make_client_ws_ctx(&client);

    let expiration_ns = UnixNanos::from(
        client
            .clock
            .get_time_ns()
            .as_u64()
            .saturating_sub(60_000_000_000),
    );
    let inst_yes = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    let inst_no = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );

    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_yes,
        PositionId::new("P-1"),
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_no,
        PositionId::new("P-2"),
    );

    let request = RequestCustomData::new(
        ClientId::from("POLYMARKET"),
        DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_data(request).expect("request_data");

    wait_until_async(
        || async {
            !client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-REQ".to_string())
        },
        StdDuration::from_secs(5),
    )
    .await;

    let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
        events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
    })
    .await;

    let closes = count_instrument_close_events(&events);
    assert_eq!(closes, 2);
}

#[rstest]
#[tokio::test]
async fn request_data_manual_fallback_uses_clob_when_gamma_is_not_strict() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([
        make_gamma_market_value_with_outcome_prices(
            "0xCOND-REQ",
            "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
            Some("[\"0.58\",\"0.42\"]"),
            Some(true),
            Some(false),
        )
    ]));
    state.clob_market_by_condition.lock().await.insert(
        "0xCOND-REQ".to_string(),
        make_clob_market_value("0xCOND-REQ", "0xTOKEN_YES", "0xTOKEN_NO", true),
    );

    let addr = start_mock_server(state).await;
    let (client, mut data_rx) = create_test_client(addr);
    let ws_ctx = make_client_ws_ctx(&client);

    let expiration_ns = UnixNanos::from(
        client
            .clock
            .get_time_ns()
            .as_u64()
            .saturating_sub(60_000_000_000),
    );
    let inst_yes = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    let inst_no = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );

    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_yes,
        PositionId::new("P-1"),
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_no,
        PositionId::new("P-2"),
    );

    let request = RequestCustomData::new(
        ClientId::from("POLYMARKET"),
        DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_data(request).expect("request_data");

    wait_until_async(
        || async {
            !client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-REQ".to_string())
        },
        StdDuration::from_secs(5),
    )
    .await;

    let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
        events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 2
    })
    .await;

    let response = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Response(DataResponse::Data(response)) => Some(response),
            _ => None,
        })
        .expect("expected custom data response");
    let custom = response
        .data
        .as_ref()
        .downcast_ref::<ModelCustomData>()
        .expect("expected CustomData response payload");
    let summary = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketResolveRequestSummaryData>()
        .expect("expected resolve summary payload");
    assert_eq!(summary.resolved_markets, 1);
    assert_eq!(summary.skipped_non_binary_markets, 1);
    assert_eq!(summary.clob_fallback_successes, 1);
    assert_eq!(
        summary.emitted_condition_ids,
        vec!["0xCOND-REQ".to_string()]
    );

    let closes = count_instrument_close_events(&events);
    assert_eq!(closes, 2);
}

#[rstest]
#[tokio::test]
async fn resolve_fallback_clob_success_after_gamma_error_does_not_mark_failed() {
    let state = TestServerState::default();
    state.clob_market_by_condition.lock().await.insert(
        "0xCOND-REQ".to_string(),
        make_clob_market_value("0xCOND-REQ", "0xTOKEN_YES", "0xTOKEN_NO", true),
    );

    let addr = start_mock_server(state).await;
    let (client, _data_rx) = create_test_client(addr);
    let ws_ctx = make_client_ws_ctx(&client);

    let expiration_ns = UnixNanos::from(
        client
            .clock
            .get_time_ns()
            .as_u64()
            .saturating_sub(60_000_000_000),
    );
    let inst_yes = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    let inst_no = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_yes,
        PositionId::new("P-1"),
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_no,
        PositionId::new("P-2"),
    );

    let failing_gamma = PolymarketGammaHttpClient::new(
        Some("http://127.0.0.1:1".to_string()),
        1,
        RetryConfig {
            max_retries: 0,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(200),
            immediate_first: true,
            max_elapsed_ms: Some(200),
        },
    )
    .expect("gamma client");

    let stats = fetch_and_apply_resolutions_by_condition_ids(
        &failing_gamma,
        &client.clob_public_client,
        &ws_ctx.resolve_context(),
        &["0xCOND-REQ".to_string()],
        ResolveBatchErrorMode::StopOnFirstError,
    )
    .await;

    assert_eq!(stats.resolved_markets, 1);
    assert_eq!(stats.clob_fallback_successes, 1);
    assert_eq!(stats.emitted_condition_ids, vec!["0xCOND-REQ".to_string()]);
    assert!(stats.failed_condition_ids.is_empty());
    assert_eq!(stats.error, None);
}

#[rstest]
#[tokio::test]
async fn request_data_explicit_multiple_condition_ids_resolves_all_requested_conditions() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([
        make_gamma_market_value_with_outcome_prices(
            "0xCOND-A",
            "[\"0xA_YES\",\"0xA_NO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        ),
        make_gamma_market_value_with_outcome_prices(
            "0xCOND-B",
            "[\"0xB_YES\",\"0xB_NO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        )
    ]));
    let addr = start_mock_server(state).await;
    let (client, mut data_rx) = create_test_client(addr);
    let ws_ctx = make_client_ws_ctx(&client);

    let expiration_ns = UnixNanos::from(1_000_000_000);
    let instruments = [
        seed_instrument_with_context(
            &ws_ctx,
            "0xA_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-A"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        ),
        seed_instrument_with_context(
            &ws_ctx,
            "0xA_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-A"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        ),
        seed_instrument_with_context(
            &ws_ctx,
            "0xB_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-B"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        ),
        seed_instrument_with_context(
            &ws_ctx,
            "0xB_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                condition_id: Some("0xCOND-B"),
                expiration_ns: Some(expiration_ns),
                ..SeedInstrumentContext::default()
            },
        ),
    ];

    for instrument in &instruments {
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            instrument,
            PositionId::new("P-1"),
        );
    }

    let mut params = Params::new();
    params.insert(
        "condition_ids".to_string(),
        serde_json::json!(["0xCOND-A", "0xCOND-B"]),
    );
    let request = RequestCustomData::new(
        ClientId::from("POLYMARKET"),
        DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );
    client.request_data(request).expect("request_data");

    wait_until_async(
        || async {
            !client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-A".to_string())
                && !client
                    .resolve_poll_watchlist
                    .contains_key(&"0xCOND-B".to_string())
        },
        StdDuration::from_secs(5),
    )
    .await;

    let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
        events.iter().any(is_resolve_response) && count_instrument_close_events(events) >= 4
    })
    .await;

    let response = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Response(DataResponse::Data(response)) => Some(response),
            _ => None,
        })
        .expect("expected custom data response");
    let custom = response
        .data
        .as_ref()
        .downcast_ref::<ModelCustomData>()
        .expect("expected CustomData response payload");
    let summary = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketResolveRequestSummaryData>()
        .expect("expected resolve summary payload");
    assert_eq!(
        summary.requested_condition_ids,
        vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]
    );
    assert_eq!(summary.resolved_markets, 2);
    assert_eq!(
        summary.emitted_condition_ids,
        vec!["0xCOND-A".to_string(), "0xCOND-B".to_string()]
    );

    let closes = count_instrument_close_events(&events);
    assert_eq!(closes, 4);
}

#[rstest]
#[tokio::test]
async fn request_data_explicit_invalid_selector_does_not_fallback_to_watchlist() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([
        make_gamma_market_value_with_outcome_prices(
            "0xCOND-REQ",
            "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        )
    ]));
    let addr = start_mock_server(state).await;
    let (client, mut data_rx) = create_test_client(addr);
    let ws_ctx = make_client_ws_ctx(&client);

    let expiration_ns = UnixNanos::from(1_000_000_000);
    let inst_yes = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    let inst_no = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-REQ"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_yes,
        PositionId::new("P-1"),
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_no,
        PositionId::new("P-2"),
    );
    pause_resolve_watch_entries(&client.resolve_poll_watchlist, &["0xCOND-REQ".to_string()]);

    let mut params = Params::new();
    params.insert(
        "instrument_ids".to_string(),
        serde_json::json!(["BTCUSDT-PERP.BINANCE"]),
    );
    let request = RequestCustomData::new(
        ClientId::from("POLYMARKET"),
        DataType::new(RESOLVE_REQUEST_TYPE_NAME, None, None),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );
    client.request_data(request).expect("request_data");

    let events = collect_events_until(&mut data_rx, StdDuration::from_secs(2), |events| {
        events.iter().any(is_resolve_response)
    })
    .await;

    let response = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Response(DataResponse::Data(response)) => Some(response),
            _ => None,
        })
        .expect("expected custom data response");
    let custom = response
        .data
        .as_ref()
        .downcast_ref::<ModelCustomData>()
        .expect("expected CustomData response payload");
    let summary = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketResolveRequestSummaryData>()
        .expect("expected resolve summary payload");
    assert!(!summary.used_watchlist_fallback);
    assert_eq!(summary.requested_condition_ids, Vec::<String>::new());
    assert!(summary.error.is_some());

    let closes = count_instrument_close_events(&events);
    assert_eq!(closes, 0);
    assert!(
        client
            .resolve_poll_watchlist
            .contains_key(&"0xCOND-REQ".to_string())
    );
}

#[rstest]
#[tokio::test]
async fn resolve_poll_task_emits_grouped_close_for_expired_watch_entries() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([
        make_gamma_market_value_with_outcome_prices(
            "0xCOND-POLL",
            "[\"0xTOKEN_YES\",\"0xTOKEN_NO\"]",
            Some("[\"1\",\"0\"]"),
            Some(true),
            Some(false),
        )
    ]));
    let addr = start_mock_server(state).await;
    let (mut client, mut data_rx) = create_test_client(addr);
    client.config.resolve_poll_enabled = true;
    client.config.resolve_poll_interval_secs = 1;
    client.config.resolve_poll_grace_secs = 0;
    client.config.resolve_poll_max_wait_secs = 300;

    let ws_ctx = make_client_ws_ctx(&client);
    let expiration_ns = UnixNanos::from(
        client
            .clock
            .get_time_ns()
            .as_u64()
            .saturating_sub(1_000_000_000),
    );
    let inst_yes = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_YES",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-POLL"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    let inst_no = seed_instrument_with_context(
        &ws_ctx,
        "0xTOKEN_NO",
        Price::from("0.001"),
        Quantity::from("0.01"),
        SeedInstrumentContext {
            condition_id: Some("0xCOND-POLL"),
            expiration_ns: Some(expiration_ns),
            ..SeedInstrumentContext::default()
        },
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_yes,
        PositionId::new("P-1"),
    );
    upsert_resolve_watch_entry_from_instrument(
        &client.resolve_poll_watchlist,
        &inst_no,
        PositionId::new("P-2"),
    );

    client.spawn_resolve_poll_task();

    wait_until_async(
        || async {
            !client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-POLL".to_string())
        },
        StdDuration::from_secs(5),
    )
    .await;

    client.cancellation_token.cancel();
    client
        .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
        .await;

    let events = collect_events_until(&mut data_rx, StdDuration::from_secs(1), |events| {
        count_instrument_close_events(events) >= 2
    })
    .await;
    let closes = count_instrument_close_events(&events);

    assert_eq!(closes, 2);
    assert!(
        !client
            .resolve_poll_watchlist
            .contains_key(&"0xCOND-POLL".to_string())
    );
}
