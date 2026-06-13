use std::{
    sync::{Arc, Mutex as StdMutex},
    time::Duration as StdDuration,
};

use nautilus_common::{live::runner::replace_data_event_sender, messages::DataResponse};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{Data as NautilusData, DataType},
    enums::{AssetClass, OrderSide, PositionSide},
    events::{PositionEvent, PositionOpened},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TraderId,
    },
    instruments::BinaryOption,
    types::{Currency, Price, Quantity},
};
use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
use serde_json::Value;

use super::super::*;
use crate::{
    config::PolymarketDataClientConfig,
    http::{clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient},
    websocket::{
        client::{PolymarketWebSocketClient, WsSubscriptionHandle},
        handler::HandlerCommand,
    },
};

pub(super) fn make_handle() -> (
    WsSubscriptionHandle,
    tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
    (WsSubscriptionHandle::from_sender(tx), rx)
}

pub(super) type ActiveSet = Arc<AtomicSet<InstrumentId>>;
pub(super) type OpenTokens = Arc<AtomicSet<Ustr>>;
pub(super) type WsMutex = Arc<tokio::sync::Mutex<()>>;

pub(super) fn make_state() -> (ActiveSet, ActiveSet, ActiveSet, OpenTokens, WsMutex) {
    (
        Arc::new(AtomicSet::new()),
        Arc::new(AtomicSet::new()),
        Arc::new(AtomicSet::new()),
        Arc::new(AtomicSet::new()),
        Arc::new(tokio::sync::Mutex::new(())),
    )
}

pub(super) fn is_resolve_response(event: &DataEvent) -> bool {
    matches!(event, DataEvent::Response(DataResponse::Data(_)))
}

pub(super) fn count_instrument_close_events(events: &[DataEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, DataEvent::Data(NautilusData::InstrumentClose(_))))
        .count()
}

pub(super) fn rtds_crypto_data_type(symbol: &str) -> DataType {
    let mut metadata = Params::new();
    metadata.insert("symbol".to_string(), Value::String(symbol.to_string()));
    DataType::new("PolymarketRtdsCryptoPrice", Some(metadata), None)
}

pub(super) fn rtds_equity_data_type(symbol: &str) -> DataType {
    let mut metadata = Params::new();
    metadata.insert("symbol".to_string(), Value::String(symbol.to_string()));
    DataType::new("PolymarketRtdsEquityPrice", Some(metadata), None)
}

pub(super) async fn collect_events_until<F>(
    data_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    timeout: StdDuration,
    mut done: F,
) -> Vec<DataEvent>
where
    F: FnMut(&[DataEvent]) -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    let mut events = Vec::new();

    loop {
        while let Ok(event) = data_rx.try_recv() {
            events.push(event);
        }

        if done(&events) || tokio::time::Instant::now() >= deadline {
            break;
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        let wait_for = remaining.min(StdDuration::from_millis(100));
        if let Ok(Some(event)) = tokio::time::timeout(wait_for, data_rx.recv()).await {
            events.push(event);
        }
    }

    events
}

pub(super) fn instrument_id() -> InstrumentId {
    InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET")
}

pub(super) fn token_ustr() -> Ustr {
    Ustr::from("0xCOND-0xTOKEN")
}

pub(super) fn stub_instrument(
    raw_symbol: &str,
    price_increment: Price,
    size_increment: Quantity,
) -> InstrumentAny {
    let price_precision = price_increment.precision;
    let size_precision = size_increment.precision;
    InstrumentAny::BinaryOption(BinaryOption::new(
        InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str()),
        Symbol::new(raw_symbol),
        AssetClass::Alternative,
        Currency::pUSD(),
        UnixNanos::default(),
        UnixNanos::from(u64::MAX),
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

pub(super) fn make_ws_ctx_with_gamma_base_url(
    gamma_base_url: &str,
) -> (
    WsMessageContext,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    let gamma_client = PolymarketGammaHttpClient::new(
        Some(gamma_base_url.to_string()),
        2,
        RetryConfig {
            max_retries: 0,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(2_000),
            immediate_first: true,
            max_elapsed_ms: Some(2_000),
        },
    )
    .expect("gamma client");
    let clob_public_client =
        PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 5)
            .expect("clob client");

    let ctx = WsMessageContext {
        clock: get_atomic_clock_realtime(),
        data_sender: data_tx,
        token_meta: Arc::new(DashMap::new()),
        instruments: Arc::new(AtomicMap::new()),
        gamma_client,
        clob_public_client,
        filters: vec![],
        order_books: Arc::new(DashMap::new()),
        last_quotes: Arc::new(DashMap::new()),
        active_quote_subs: Arc::new(AtomicSet::new()),
        active_delta_subs: Arc::new(AtomicSet::new()),
        active_trade_subs: Arc::new(AtomicSet::new()),
        resolve_poll_watchlist: Arc::new(AtomicMap::new()),
        resolve_watch_apply_mutex: Arc::new(StdMutex::new(())),
        pending_snapshot_after_tick_change: Arc::new(AtomicSet::new()),
        new_market_inflight_keys: Arc::new(DashMap::new()),
        new_market_fetch_semaphore: Arc::new(tokio::sync::Semaphore::new(
            PolymarketDataClientConfig::default().new_market_fetch_max_concurrency,
        )),
        subscribe_new_markets: false,
        new_market_filter: None,
        cancellation_token: CancellationToken::new(),
    };

    (ctx, data_rx)
}

pub(super) fn make_ws_ctx() -> (
    WsMessageContext,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    make_ws_ctx_with_gamma_base_url("http://localhost")
}

pub(super) fn make_client_for_reset_test() -> PolymarketDataClient {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let gamma = PolymarketGammaHttpClient::new(
        Some("http://localhost".to_string()),
        1,
        RetryConfig::default(),
    )
    .expect("gamma client");
    let clob = PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 1)
        .expect("clob client");
    let data_api = PolymarketDataApiHttpClient::new(Some("http://localhost".to_string()), 1)
        .expect("data api client");
    let ws = PolymarketWebSocketClient::new_market(
        Some("ws://localhost/ws/market".to_string()),
        false,
        TransportBackend::default(),
    );

    PolymarketDataClient::new(
        ClientId::from("POLY-TEST"),
        PolymarketDataClientConfig::default(),
        gamma,
        clob,
        data_api,
        ws,
    )
}

pub(super) fn make_client_with_fetch_concurrency(concurrency: usize) -> PolymarketDataClient {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let gamma = PolymarketGammaHttpClient::new(
        Some("http://localhost".to_string()),
        1,
        RetryConfig::default(),
    )
    .expect("gamma client");
    let clob = PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 1)
        .expect("clob client");
    let data_api = PolymarketDataApiHttpClient::new(Some("http://localhost".to_string()), 1)
        .expect("data api client");
    let ws = PolymarketWebSocketClient::new_market(
        Some("ws://localhost/ws/market".to_string()),
        false,
        TransportBackend::default(),
    );

    let config = PolymarketDataClientConfig {
        new_market_fetch_max_concurrency: concurrency,
        ..PolymarketDataClientConfig::default()
    };

    PolymarketDataClient::new(
        ClientId::from("POLY-TEST"),
        config,
        gamma,
        clob,
        data_api,
        ws,
    )
}

pub(super) fn seed_instrument(
    ctx: &WsMessageContext,
    raw_symbol: &str,
    price_increment: Price,
    size_increment: Quantity,
) -> InstrumentAny {
    let inst = stub_instrument(raw_symbol, price_increment, size_increment);
    cache_instrument(&ctx.instruments, &ctx.token_meta, &inst);
    inst
}

#[derive(Clone, Copy, Default)]
pub(super) struct SeedInstrumentContext<'a> {
    pub(super) market_slug: Option<&'a str>,
    pub(super) market_id: Option<&'a str>,
    pub(super) condition_id: Option<&'a str>,
    pub(super) expiration_ns: Option<UnixNanos>,
}

pub(super) fn seed_instrument_with_context(
    ctx: &WsMessageContext,
    raw_symbol: &str,
    price_increment: Price,
    size_increment: Quantity,
    seed_ctx: SeedInstrumentContext<'_>,
) -> InstrumentAny {
    let mut inst = stub_instrument(raw_symbol, price_increment, size_increment);
    if let InstrumentAny::BinaryOption(ref mut binary) = inst {
        if let Some(expiration_ns) = seed_ctx.expiration_ns {
            binary.expiration_ns = expiration_ns;
        }

        let mut info = Params::new();
        info.insert(
            "token_id".to_string(),
            serde_json::Value::String(raw_symbol.to_string()),
        );

        if let Some(market_slug) = seed_ctx.market_slug {
            info.insert(
                "market_slug".to_string(),
                serde_json::Value::String(market_slug.to_string()),
            );
        }

        if let Some(market_id) = seed_ctx.market_id {
            info.insert(
                "market_id".to_string(),
                serde_json::Value::String(market_id.to_string()),
            );
        }

        if let Some(condition_id) = seed_ctx.condition_id {
            info.insert(
                "condition_id".to_string(),
                serde_json::Value::String(condition_id.to_string()),
            );
        }

        binary.info = Some(info);
    }

    cache_instrument(&ctx.instruments, &ctx.token_meta, &inst);
    inst
}

pub(super) fn stub_position_opened_event_with_position_id(
    instrument_id: InstrumentId,
    position_id: &str,
) -> PositionEvent {
    PositionEvent::PositionOpened(PositionOpened {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("STRATEGY-001"),
        instrument_id,
        position_id: PositionId::new(position_id),
        account_id: AccountId::from("ACCOUNT-001"),
        opening_order_id: ClientOrderId::from("ENTRY-1"),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 1.0,
        quantity: Quantity::from("1"),
        last_qty: Quantity::from("1"),
        last_px: Price::from("0.75"),
        currency: Currency::pUSD(),
        avg_px_open: 0.75,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(1),
        ts_init: UnixNanos::from(1),
    })
}

pub(super) fn stub_position_opened_event(instrument_id: InstrumentId) -> PositionEvent {
    stub_position_opened_event_with_position_id(instrument_id, "P-1")
}

pub(super) fn make_client_ws_ctx(client: &PolymarketDataClient) -> WsMessageContext {
    WsMessageContext {
        clock: client.clock,
        data_sender: client.data_sender.clone(),
        token_meta: client.token_meta.clone(),
        instruments: client.instruments.clone(),
        gamma_client: client.provider.http_client().clone(),
        clob_public_client: client.clob_public_client.clone(),
        filters: client.provider.filters(),
        order_books: client.order_books.clone(),
        last_quotes: client.last_quotes.clone(),
        active_quote_subs: client.active_quote_subs.clone(),
        active_delta_subs: client.active_delta_subs.clone(),
        active_trade_subs: client.active_trade_subs.clone(),
        resolve_poll_watchlist: client.resolve_poll_watchlist.clone(),
        resolve_watch_apply_mutex: client.resolve_watch_apply_mutex.clone(),
        pending_snapshot_after_tick_change: client.pending_snapshot_after_tick_change.clone(),
        new_market_inflight_keys: client.new_market_inflight_keys.clone(),
        new_market_fetch_semaphore: client.new_market_fetch_semaphore.clone(),
        subscribe_new_markets: client.config.subscribe_new_markets,
        new_market_filter: client.config.new_market_filter.clone(),
        cancellation_token: client.cancellation_token.clone(),
    }
}
