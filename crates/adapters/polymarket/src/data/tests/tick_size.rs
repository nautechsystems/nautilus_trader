use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Data as NautilusData, QuoteTick},
    enums::BookType,
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use rstest::rstest;

use super::{super::*, support::*};
use crate::{
    common::enums::PolymarketOrderSide,
    websocket::messages::{
        MarketWsMessage, PolymarketBookLevel, PolymarketBookSnapshot, PolymarketQuote,
        PolymarketQuotes, PolymarketTickSizeChange,
    },
};

fn level(price: &str, size: &str) -> PolymarketBookLevel {
    PolymarketBookLevel {
        price: price.to_string(),
        size: size.to_string(),
    }
}

fn make_snapshot(market: &str, asset_id: &str, prices: &[(&str, &str)]) -> MarketWsMessage {
    let mid = prices.len() / 2;
    let bids = prices[..mid].iter().map(|(p, s)| level(p, s)).collect();
    let asks = prices[mid..].iter().map(|(p, s)| level(p, s)).collect();
    MarketWsMessage::Book(PolymarketBookSnapshot {
        market: Ustr::from(market),
        asset_id: Ustr::from(asset_id),
        bids,
        asks,
        timestamp: "1700000000000".to_string(),
    })
}

fn make_tick_change(market: &str, asset_id: &str, old: &str, new: &str) -> MarketWsMessage {
    MarketWsMessage::TickSizeChange(PolymarketTickSizeChange {
        market: Ustr::from(market),
        asset_id: Ustr::from(asset_id),
        new_tick_size: new.to_string(),
        old_tick_size: old.to_string(),
        timestamp: "1700000001000".to_string(),
    })
}

fn make_price_change(market: &str, asset_id: &str, price: &str, size: &str) -> MarketWsMessage {
    MarketWsMessage::PriceChange(PolymarketQuotes {
        market: Ustr::from(market),
        price_changes: vec![PolymarketQuote {
            asset_id: Ustr::from(asset_id),
            price: price.to_string(),
            side: PolymarketOrderSide::Buy,
            size: size.to_string(),
            hash: String::new(),
            best_bid: None,
            best_ask: None,
        }],
        timestamp: "1700000002000".to_string(),
    })
}

#[rstest]
fn tick_size_change_clears_book_and_marks_pending() {
    let asset_id_str = "0xTOKEN";
    let token_ustr = Ustr::from(asset_id_str);
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.001"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);

    let prior_quote = QuoteTick::new(
        instrument_id,
        Price::from("0.504"),
        Price::from("0.506"),
        Quantity::from("5.00"),
        Quantity::from("8.00"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    ctx.last_quotes.insert(instrument_id, prior_quote);

    let snap = make_snapshot(
        market,
        asset_id_str,
        &[
            ("0.501", "10"),
            ("0.504", "5"),
            ("0.506", "8"),
            ("0.509", "12"),
        ],
    );
    PolymarketDataClient::handle_market_message(snap, &ctx);
    assert!(ctx.order_books.contains_key(&instrument_id));

    while data_rx.try_recv().is_ok() {}

    let change = make_tick_change(market, asset_id_str, "0.001", "0.01");
    PolymarketDataClient::handle_market_message(change, &ctx);

    assert!(!ctx.order_books.contains_key(&instrument_id));
    assert!(ctx.last_quotes.contains_key(&instrument_id));
    assert!(
        ctx.pending_snapshot_after_tick_change
            .contains(&instrument_id)
    );

    let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
    assert_eq!(meta.price_precision, 2);

    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
        "expected rebuilt instrument event, found: {events:?}",
    );
    assert!(
        !events.iter().any(|e| matches!(e, DataEvent::Data(_))),
        "tick size change must not emit Data events: {events:?}",
    );
}

#[rstest]
fn pending_drops_price_change_until_snapshot() {
    let asset_id_str = "0xTOKEN2";
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.01"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);
    ctx.pending_snapshot_after_tick_change.insert(instrument_id);

    let pc = make_price_change(market, asset_id_str, "0.50", "20");
    PolymarketDataClient::handle_market_message(pc, &ctx);

    assert!(!ctx.order_books.contains_key(&instrument_id));
    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        events.is_empty(),
        "price_change while pending must not emit any events: {events:?}",
    );

    let snap = make_snapshot(
        market,
        asset_id_str,
        &[("0.45", "5"), ("0.49", "10"), ("0.51", "8"), ("0.55", "12")],
    );
    PolymarketDataClient::handle_market_message(snap, &ctx);

    assert!(
        !ctx.pending_snapshot_after_tick_change
            .contains(&instrument_id)
    );
    assert!(ctx.order_books.contains_key(&instrument_id));
}

#[rstest]
fn tick_size_change_noop_preserves_book_and_quote() {
    let asset_id_str = "0xTOKEN_NOOP";
    let token_ustr = Ustr::from(asset_id_str);
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.01"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);

    let snap = make_snapshot(
        market,
        asset_id_str,
        &[("0.50", "10"), ("0.54", "5"), ("0.56", "8"), ("0.59", "12")],
    );
    PolymarketDataClient::handle_market_message(snap, &ctx);
    let book_ts_before = ctx
        .order_books
        .get(&instrument_id)
        .expect("book entry")
        .ts_last;

    while data_rx.try_recv().is_ok() {}

    let change = make_tick_change(market, asset_id_str, "0.01", "0.01");
    PolymarketDataClient::handle_market_message(change, &ctx);

    let book_after = ctx.order_books.get(&instrument_id).expect("book entry");
    assert_eq!(book_after.ts_last, book_ts_before);
    assert!(
        !ctx.pending_snapshot_after_tick_change
            .contains(&instrument_id)
    );
    let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
    assert_eq!(meta.price_precision, 2);
    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        events.is_empty(),
        "no-op tick change must not emit events: {events:?}",
    );
}

#[rstest]
fn tick_size_change_same_precision_different_value_triggers_epoch() {
    let asset_id_str = "0xTOKEN_VALUE";
    let token_ustr = Ustr::from(asset_id_str);
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.005"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);
    ctx.order_books.insert(
        instrument_id,
        OrderBook::new(instrument_id, BookType::L2_MBP),
    );

    let change = make_tick_change(market, asset_id_str, "0.005", "0.001");
    PolymarketDataClient::handle_market_message(change, &ctx);

    assert!(!ctx.order_books.contains_key(&instrument_id));
    assert!(
        ctx.pending_snapshot_after_tick_change
            .contains(&instrument_id)
    );
    let meta = ctx.token_meta.get(&token_ustr).expect("token_meta");
    assert_eq!(meta.price_precision, 3);

    let rebuilt = ctx
        .instruments
        .load()
        .get(&instrument_id)
        .cloned()
        .expect("rebuilt instrument");
    assert_eq!(rebuilt.price_increment(), Price::from("0.001"));

    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
        "expected rebuilt instrument event, found: {events:?}",
    );
}

#[rstest]
fn tick_size_change_does_not_mark_pending_for_trade_only_sub() {
    let asset_id_str = "0xTOKEN6";
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.001"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_trade_subs.insert(instrument_id);

    let change = make_tick_change(market, asset_id_str, "0.001", "0.01");
    PolymarketDataClient::handle_market_message(change, &ctx);

    assert!(
        !ctx.pending_snapshot_after_tick_change
            .contains(&instrument_id)
    );
    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        events.iter().any(|e| matches!(e, DataEvent::Instrument(_))),
        "instrument update must still be emitted: {events:?}",
    );
}

#[rstest]
fn pending_persists_when_snapshot_has_corrupt_level() {
    let asset_id_str = "0xTOKEN7";

    let (ctx, _data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.01"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);
    ctx.active_quote_subs.insert(instrument_id);
    ctx.pending_snapshot_after_tick_change.insert(instrument_id);

    let snap = MarketWsMessage::Book(PolymarketBookSnapshot {
        market: Ustr::from("0xMARKET"),
        asset_id: Ustr::from(asset_id_str),
        bids: vec![level("not-a-number", "1"), level("0.49", "10")],
        asks: vec![level("0.51", "8"), level("0.55", "12")],
        timestamp: "1700000000000".to_string(),
    });
    PolymarketDataClient::handle_market_message(snap, &ctx);

    assert!(
        ctx.pending_snapshot_after_tick_change
            .contains(&instrument_id)
    );
    assert!(!ctx.order_books.contains_key(&instrument_id));
}

#[rstest]
fn price_change_emits_delta_when_not_pending() {
    let asset_id_str = "0xTOKEN10";
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.01"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);
    ctx.order_books.insert(
        instrument_id,
        OrderBook::new(instrument_id, BookType::L2_MBP),
    );

    let pc = make_price_change(market, asset_id_str, "0.50", "20");
    PolymarketDataClient::handle_market_message(pc, &ctx);

    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DataEvent::Data(NautilusData::Deltas(_)))),
        "delta must be emitted on the not-pending happy path: {events:?}",
    );

    let book = ctx.order_books.get(&instrument_id).expect("book entry");
    assert_eq!(book.best_bid_price(), Some(Price::from("0.50")));
    assert_eq!(book.best_bid_size(), Some(Quantity::from("20.00")));
}

#[rstest]
fn quote_path_open_during_pending_window() {
    let asset_id_str = "0xTOKEN8";
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.01"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);
    ctx.active_quote_subs.insert(instrument_id);
    ctx.pending_snapshot_after_tick_change.insert(instrument_id);

    let prior = QuoteTick::new(
        instrument_id,
        Price::from("0.49"),
        Price::from("0.51"),
        Quantity::from("100.00"),
        Quantity::from("75.00"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    ctx.last_quotes.insert(instrument_id, prior);

    let pc = MarketWsMessage::PriceChange(PolymarketQuotes {
        market: Ustr::from(market),
        price_changes: vec![PolymarketQuote {
            asset_id: Ustr::from(asset_id_str),
            price: "0.50".to_string(),
            side: PolymarketOrderSide::Buy,
            size: "20".to_string(),
            hash: String::new(),
            best_bid: Some("0.50".to_string()),
            best_ask: Some("0.52".to_string()),
        }],
        timestamp: "1700000003000".to_string(),
    });
    PolymarketDataClient::handle_market_message(pc, &ctx);

    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DataEvent::Data(NautilusData::Deltas(_)))),
        "delta must be dropped while pending: {events:?}",
    );
    let emitted_quote = events
        .iter()
        .find_map(|e| match e {
            DataEvent::Data(NautilusData::Quote(q)) => Some(q),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected quote event, found: {events:?}"));
    assert_eq!(emitted_quote.bid_size, Quantity::from("20.00"));
    assert_eq!(emitted_quote.ask_size, Quantity::from("75.00"));
}

#[rstest]
fn pending_persists_when_snapshot_fails_to_seed() {
    let asset_id_str = "0xTOKEN5";
    let market = "0xMARKET";

    let (ctx, mut data_rx) = make_ws_ctx();
    let inst = seed_instrument(
        &ctx,
        asset_id_str,
        Price::from("0.01"),
        Quantity::from("0.01"),
    );
    let instrument_id = inst.id();
    ctx.active_delta_subs.insert(instrument_id);
    ctx.pending_snapshot_after_tick_change.insert(instrument_id);

    let empty = MarketWsMessage::Book(PolymarketBookSnapshot {
        market: Ustr::from(market),
        asset_id: Ustr::from(asset_id_str),
        bids: vec![],
        asks: vec![],
        timestamp: "1700000000000".to_string(),
    });
    PolymarketDataClient::handle_market_message(empty, &ctx);

    assert!(
        ctx.pending_snapshot_after_tick_change
            .contains(&instrument_id)
    );
    let events: Vec<DataEvent> = std::iter::from_fn(|| data_rx.try_recv().ok()).collect();
    assert!(
        !events.iter().any(|e| matches!(e, DataEvent::Data(_))),
        "empty snapshot must not emit Data events: {events:?}",
    );
}
