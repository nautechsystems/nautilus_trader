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

//! Data-client bootstrap, historical responses, and instrument refresh behavior.

use std::{num::NonZeroUsize, time::Duration};

use indexmap::IndexMap;
use nautilus_common::{
    clients::DataClient,
    messages::{DataEvent, DataResponse, data::*},
};
use nautilus_core::{
    Params, UUID4, UnixNanos, string::secret::SecretString, time::get_atomic_clock_realtime,
};
use nautilus_finfeed::{
    config::FinFeedDataClientConfig, data::FinFeedDataClient, http::client::FinFeedHttpConfig,
    instruments::FinFeedInstrumentSelection,
};
use nautilus_model::{
    data::{Bar, BarType, Data, QuoteTick, TradeTick},
    enums::{AggressorSide, BookType, RecordFlag},
    identifiers::{ClientId, InstrumentId, TradeId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use nautilus_network::dst::{task, time};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::json;
use tokio::io::AsyncWriteExt;

use super::{
    dst::{assert_request, start_peer},
    polling::{connected_selection, event, timed_peer},
};

const MARKET: &str = include_str!("../../test_data/polymarket-market.json");
const EXCHANGES: &str = include_str!("../../test_data/exchanges.json");
const MARKET_ID: &str = "TEST-OUTCOME_NO";

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bootstrap_disconnect_and_reconnect_reload_definitions() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", EXCHANGES),
        (200, "", MARKET),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http,
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![MARKET_ID.into()],
        }],
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    assert!(client.is_disconnected());

    for _ in 0..2 {
        client.connect().await.unwrap();
        assert!(client.is_connected());

        let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
            panic!("expected definition before readiness")
        };

        assert_eq!(instrument.id().symbol.as_str(), MARKET_ID);
        assert_eq!(instrument.id().venue.as_str(), "FINFEED_POLYMARKET");
        assert_eq!(instrument.price_precision(), 4);
        assert_eq!(instrument.size_precision(), 6);
        client.connect().await.unwrap();
        assert!(receiver.try_recv().is_err());
        client.disconnect().await.unwrap();
        assert!(client.is_disconnected());
    }

    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("GET /v1/exchanges HTTP/1.1"));
    assert!(requests[2].starts_with("GET /v1/exchanges HTTP/1.1"));
    assert_eq!(requests[1].lines().next(), requests[3].lines().next());
}

const TRADES: &str = include_str!("../../test_data/client-trades.json");
const QUOTES: &str = include_str!("../../test_data/client-quotes.json");
const BARS: &str = include_str!("../../test_data/client-bars.json");
const BOOK: &str = include_str!("../../test_data/polymarket-book.json");
const ACTIVITY_MARKET: &str = include_str!("../../test_data/activity-market.json");
const ACTIVITY_MARKET_ID: &str = "TEST-ACTIVITY_NO";

fn instrument_id() -> InstrumentId {
    nautilus_finfeed::common::symbol::to_instrument_id("POLYMARKET", MARKET_ID).unwrap()
}

fn config(http: FinFeedHttpConfig) -> FinFeedDataClientConfig {
    FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http,
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![MARKET_ID.into()],
        }],
        ..Default::default()
    }
}

fn params() -> Params {
    serde_json::from_value(json!({"purpose":"correlation-check","number":73})).unwrap()
}

pub(super) async fn response(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> DataResponse {
    let mut definitions = Vec::new();

    loop {
        let event = time::timeout(Duration::from_secs(3), receiver.recv())
            .await
            .unwrap()
            .unwrap();

        match event {
            DataEvent::Data(Data::Instrument(instrument)) => definitions.push(*instrument),
            DataEvent::Response(response) => {
                assert_definitions(&response, &definitions);
                return response;
            }
            event => panic!("expected definition or correlated response, received {event:?}"),
        }
    }
}

pub(super) fn assert_definitions(response: &DataResponse, definitions: &[InstrumentAny]) {
    let expected = match response {
        DataResponse::Instrument(response) => std::slice::from_ref(&response.data),
        DataResponse::Instruments(response) => &response.data,
        _ => &[],
    };

    assert_eq!(
        serde_json::to_value(definitions).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn quotes_and_trades_preserve_envelopes_boundaries_order_and_exact_limits() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", QUOTES),
        (200, "", TRADES),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let start = "2026-10-11T00:00:01Z".parse().unwrap();
    let end = "2026-10-11T00:00:03Z".parse().unwrap();
    let start_ns = UnixNanos::from(1_791_676_801_000_000_000);
    let end_ns = UnixNanos::from(1_791_676_803_000_000_000);
    let before = get_atomic_clock_realtime().get_time_ns();
    let quote_id = UUID4::new();
    let trade_id = UUID4::new();
    let requested_client = ClientId::from("REQUESTED");
    client
        .request_quotes(RequestQuotes::new(
            instrument_id(),
            Some(start),
            Some(end),
            NonZeroUsize::new(3),
            Some(requested_client),
            quote_id,
            UnixNanos::from(41),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Quotes(quotes) = response(&mut receiver).await else {
        panic!("expected quotes")
    };

    client
        .request_trades(RequestTrades::new(
            instrument_id(),
            Some(start),
            Some(end),
            NonZeroUsize::new(4),
            Some(requested_client),
            trade_id,
            UnixNanos::from(42),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Trades(trades) = response(&mut receiver).await else {
        panic!("expected trades")
    };

    let after = get_atomic_clock_realtime().get_time_ns();

    for (correlation, client, id, start, end, stamp, actual_params, expected) in [
        (
            quotes.correlation_id,
            quotes.client_id,
            quotes.instrument_ids,
            quotes.start,
            quotes.end,
            quotes.ts_init,
            &quotes.params,
            quote_id,
        ),
        (
            trades.correlation_id,
            trades.client_id,
            trades.instrument_ids,
            trades.start,
            trades.end,
            trades.ts_init,
            &trades.params,
            trade_id,
        ),
    ] {
        assert_eq!(correlation, expected);
        assert_eq!(client, requested_client);
        assert_eq!(id, vec![instrument_id()]);
        assert_eq!(start, Some(start_ns));
        assert_eq!(end, Some(end_ns));
        assert!(stamp >= before && stamp <= after);
        assert_eq!(actual_params, &Some(params()));
    }

    assert_eq!(quotes.data.len(), 3);

    for (index, (bid, ask, bid_qty, ask_qty, event, init)) in [
        ("0.2", "0.3", "5.000005", "6.000006", 0, 1),
        ("0.3", "0.4", "3.000003", "4.000004", 1, 2),
        ("0.31", "0.41", "7.000007", "8.000008", 1, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let value = quotes.data[index];
        assert_eq!(value.instrument_id, instrument_id());
        assert_eq!(
            value.bid_price.as_decimal(),
            bid.parse::<Decimal>().unwrap()
        );
        assert_eq!(
            value.ask_price.as_decimal(),
            ask.parse::<Decimal>().unwrap()
        );
        assert_eq!(
            value.bid_size.as_decimal(),
            bid_qty.parse::<Decimal>().unwrap()
        );
        assert_eq!(
            value.ask_size.as_decimal(),
            ask_qty.parse::<Decimal>().unwrap()
        );
        assert_eq!(
            value.ts_event,
            UnixNanos::from(1_791_676_800_000_000_000 + event * 1_000_000_000)
        );
        assert_eq!(
            value.ts_init,
            UnixNanos::from(1_791_676_800_000_000_000 + init * 1_000_000_000)
        );
    }

    assert_eq!(trades.data.len(), 4);

    for (index, (id, price, qty, second)) in [
        ("trade-2", "0.21", "3.000003", 1),
        ("trade-1", "0.31", "2.000002", 2),
        ("trade-3", "0.32", "4.000004", 2),
        ("trade-4", "0.4", "5.000005", 3),
    ]
    .into_iter()
    .enumerate()
    {
        let value = trades.data[index];
        assert_eq!(value.instrument_id, instrument_id());
        assert_eq!(value.trade_id.as_str(), id);
        assert_eq!(value.price.as_decimal(), price.parse::<Decimal>().unwrap());
        assert_eq!(value.size.as_decimal(), qty.parse::<Decimal>().unwrap());
        assert_eq!(value.aggressor_side, AggressorSide::NoAggressor);
        assert_eq!(
            value.ts_event,
            UnixNanos::from(1_791_676_800_000_000_000 + second * 1_000_000_000)
        );
        assert_eq!(value.ts_init, value.ts_event);
    }

    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(
        requests[2]
            .lines()
            .next()
            .unwrap()
            .ends_with("/history/quotes?date=2026-10-11&limit=10000 HTTP/1.1")
    );
    assert!(
        requests[3]
            .lines()
            .next()
            .unwrap()
            .ends_with("/history/trades?date=2026-10-11&limit=10000 HTTP/1.1")
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn fresh_instrument_requests_and_empty_windows_preserve_every_field() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", MARKET),
        (200, "", MARKET),
        (200, "", QUOTES),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let start = "2026-10-11T00:01:00Z".parse().unwrap();
    let end = "2026-10-11T00:02:00Z".parse().unwrap();
    let request_id = UUID4::new();
    let before = get_atomic_clock_realtime().get_time_ns();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id(),
            Some(start),
            Some(end),
            Some(ClientId::from("REQUESTED")),
            request_id,
            UnixNanos::from(9),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Instrument(result) = response(&mut receiver).await else {
        panic!("expected instrument")
    };

    let after = get_atomic_clock_realtime().get_time_ns();
    assert_eq!(result.correlation_id, request_id);
    assert_eq!(result.client_id, ClientId::from("REQUESTED"));
    assert_eq!(result.instrument_id, instrument_id());
    assert_eq!(
        result.start,
        Some(UnixNanos::from(1_791_676_860_000_000_000))
    );
    assert_eq!(result.end, Some(UnixNanos::from(1_791_676_920_000_000_000)));
    assert_eq!(result.params, Some(params()));
    assert!(result.ts_init >= before && result.ts_init <= after);
    let expected = nautilus_finfeed::http::parse::parse_instrument(
        &serde_json::from_str(MARKET).unwrap(),
        result.data.ts_init(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&result.data).unwrap(),
        serde_json::to_value(InstrumentAny::from(expected)).unwrap()
    );
    let list_id = UUID4::new();
    client
        .request_instruments(RequestInstruments::new(
            Some(start),
            Some(end),
            Some(ClientId::from("REQUESTED")),
            Some(instrument_id().venue),
            list_id,
            UnixNanos::from(10),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Instruments(list) = response(&mut receiver).await else {
        panic!("expected instruments")
    };

    assert_eq!(list.correlation_id, list_id);
    assert_eq!(list.client_id, ClientId::from("REQUESTED"));
    assert_eq!(list.venue, instrument_id().venue);
    assert_eq!(list.data.len(), 1);
    assert_eq!(list.data[0].id(), instrument_id());
    assert_eq!(list.start, result.start);
    assert_eq!(list.end, result.end);
    assert_eq!(list.params, Some(params()));
    assert!(list.ts_init >= after && list.ts_init <= get_atomic_clock_realtime().get_time_ns());
    let empty_id = UUID4::new();
    client
        .request_quotes(RequestQuotes::new(
            instrument_id(),
            Some(start),
            Some(end),
            None,
            None,
            empty_id,
            UnixNanos::from(11),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Quotes(empty) = response(&mut receiver).await else {
        panic!("expected empty quotes")
    };

    assert_eq!(empty.correlation_id, empty_id);
    assert_eq!(empty.client_id, ClientId::from("FINFEED"));
    assert_eq!(empty.instrument_ids, vec![instrument_id()]);
    assert!(empty.data.is_empty());
    assert_eq!(empty.start, result.start);
    assert_eq!(empty.end, result.end);
    assert_eq!(empty.params, Some(params()));
    assert!(empty.ts_init >= after && empty.ts_init <= get_atomic_clock_realtime().get_time_ns());
    let bar_type: BarType = format!("{}-1-MINUTE-LAST-EXTERNAL", instrument_id())
        .parse()
        .unwrap();
    let point_id = UUID4::new();
    client
        .request_bars(RequestBars::new(
            bar_type,
            Some(start),
            Some(start),
            None,
            None,
            point_id,
            UnixNanos::from(12),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Bars(point) = response(&mut receiver).await else {
        panic!("expected empty bars")
    };

    assert_eq!(point.correlation_id, point_id);
    assert_eq!(point.client_id, ClientId::from("FINFEED"));
    assert_eq!(point.bar_type, bar_type);
    assert!(point.data.is_empty());
    assert_eq!(point.start, result.start);
    assert_eq!(point.end, result.start);
    assert_eq!(point.params, Some(params()));
    client.disconnect().await.unwrap();
    assert_eq!(peer.await.unwrap().len(), 5);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn books_and_closed_bars_preserve_complete_envelopes() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", BOOK),
        (200, "", include_str!("../../test_data/periods.json")),
        (200, "", BARS),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let book_id = UUID4::new();
    let before = get_atomic_clock_realtime().get_time_ns();
    client
        .request_book_snapshot(RequestBookSnapshot::new(
            instrument_id(),
            NonZeroUsize::new(2),
            Some(ClientId::from("REQUESTED")),
            book_id,
            UnixNanos::from(8),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Book(book) = response(&mut receiver).await else {
        panic!("expected book")
    };

    assert_eq!(book.correlation_id, book_id);
    assert_eq!(book.client_id, ClientId::from("REQUESTED"));
    assert_eq!(book.instrument_id, instrument_id());
    assert_eq!(book.start, None);
    assert_eq!(book.end, None);
    assert_eq!(book.params, Some(params()));
    assert!(book.ts_init >= before && book.ts_init <= get_atomic_clock_realtime().get_time_ns());
    assert_eq!(book.data.instrument_id, instrument_id());
    assert_eq!(book.data.book_type, BookType::L2_MBP);
    assert_eq!(book.data.sequence, 0);
    assert_eq!(
        book.data.ts_last,
        UnixNanos::from(1_791_763_202_000_000_300)
    );
    assert_eq!(
        book.data.bids_as_map(None).into_iter().collect::<Vec<_>>(),
        vec![
            (
                "0.4".parse::<Decimal>().unwrap(),
                "31.125".parse::<Decimal>().unwrap()
            ),
            (
                "0.39".parse::<Decimal>().unwrap(),
                "32.125".parse::<Decimal>().unwrap()
            )
        ]
    );
    assert_eq!(
        book.data.asks_as_map(None).into_iter().collect::<Vec<_>>(),
        vec![
            (
                "0.6".parse::<Decimal>().unwrap(),
                "41.25".parse::<Decimal>().unwrap()
            ),
            (
                "0.61".parse::<Decimal>().unwrap(),
                "42.25".parse::<Decimal>().unwrap()
            )
        ]
    );
    let bar_type: BarType = format!("{}-1-MINUTE-LAST-EXTERNAL", instrument_id())
        .parse()
        .unwrap();
    let bar_id = UUID4::new();
    client
        .request_bars(RequestBars::new(
            bar_type,
            Some("2020-10-11T00:00:00Z".parse().unwrap()),
            Some("2020-10-11T00:02:30Z".parse().unwrap()),
            NonZeroUsize::new(1),
            Some(ClientId::from("REQUESTED")),
            bar_id,
            UnixNanos::from(9),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Bars(bars) = response(&mut receiver).await else {
        panic!("expected bars")
    };

    assert_eq!(bars.correlation_id, bar_id);
    assert_eq!(bars.client_id, ClientId::from("REQUESTED"));
    assert_eq!(bars.bar_type, bar_type);
    assert_eq!(bars.start, Some(UnixNanos::from(1_602_374_400_000_000_000)));
    assert_eq!(bars.end, Some(UnixNanos::from(1_602_374_550_000_000_000)));
    assert_eq!(bars.params, Some(params()));
    assert!(bars.ts_init >= before && bars.ts_init <= get_atomic_clock_realtime().get_time_ns());
    assert_eq!(bars.data.len(), 1);
    let value = bars.data[0];
    assert_eq!(value.bar_type, bar_type);
    assert_eq!(value.open.as_decimal(), "0.2".parse::<Decimal>().unwrap());
    assert_eq!(value.high.as_decimal(), "0.4".parse::<Decimal>().unwrap());
    assert_eq!(value.low.as_decimal(), "0.1".parse::<Decimal>().unwrap());
    assert_eq!(value.close.as_decimal(), "0.3".parse::<Decimal>().unwrap());
    assert_eq!(
        value.volume.as_decimal(),
        "12.345678".parse::<Decimal>().unwrap()
    );
    assert_eq!(value.ts_event, UnixNanos::from(1_602_374_460_000_000_000));
    assert_eq!(value.ts_init, value.ts_event);
    client.disconnect().await.unwrap();
    assert_eq!(peer.await.unwrap().len(), 5);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn quote_history_preserves_every_tick() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (
            200,
            "",
            include_str!("../../test_data/polymarket-quotes-day.json"),
        ),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http.clone(), vec![MARKET_ID.into()], 60_000).await;
    client
        .request_quotes(RequestQuotes::new(
            instrument_id(),
            Some("2026-10-11T00:00:00Z".parse().unwrap()),
            Some("2026-10-11T01:00:00Z".parse().unwrap()),
            NonZeroUsize::new(3),
            None,
            UUID4::new(),
            UnixNanos::from(17),
            None,
        ))
        .unwrap();

    let DataResponse::Quotes(quotes) = response(&mut receiver).await else {
        panic!("expected history quotes")
    };

    client.disconnect().await.unwrap();

    let expected: Vec<_> = [
        (
            "11.000000",
            "21.000000",
            "2026-10-11T00:01:00.0000001Z",
            "2026-10-11T00:01:00.0000003Z",
        ),
        (
            "12.000000",
            "22.000000",
            "2026-10-11T00:02:00.0000001Z",
            "2026-10-11T00:02:00.0000003Z",
        ),
        (
            "13.000000",
            "23.000000",
            "2026-10-11T00:03:00.0000001Z",
            "2026-10-11T00:03:00.0000003Z",
        ),
    ]
    .into_iter()
    .map(|(bid_qty, ask_qty, entry, recv)| {
        QuoteTick::new_checked(
            instrument_id(),
            Price::from("0.2000"),
            Price::from("0.3000"),
            Quantity::from(bid_qty),
            Quantity::from(ask_qty),
            entry.parse().unwrap(),
            recv.parse().unwrap(),
        )
        .unwrap()
    })
    .collect();

    assert_eq!(&*quotes.data, expected.as_slice());

    for quote in quotes.data.iter() {
        assert_eq!(
            [
                quote.bid_price.precision,
                quote.ask_price.precision,
                quote.bid_size.precision,
                quote.ask_size.precision,
            ],
            [4, 4, 6, 6]
        );
    }

    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert_request(
        &requests[2],
        &format!("/v1/activity/POLYMARKET/{MARKET_ID}/history/quotes?date=2026-10-11&limit=10000"),
        &http,
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn mixed_activity_preserves_every_tick() {
    let market_id = ACTIVITY_MARKET_ID;
    let id = nautilus_finfeed::common::symbol::to_instrument_id("POLYMARKET", market_id).unwrap();
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", ACTIVITY_MARKET),
        (200, "", include_str!("../../test_data/activity-mixed.json")),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http.clone(), vec![market_id.into()], 60_000).await;
    client
        .subscribe_quotes(SubscribeQuotes::new(
            id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(11),
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_trades(SubscribeTrades::new(
            id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(13),
            None,
            None,
        ))
        .unwrap();
    let mut actual = Vec::new();
    for _ in 0..18 {
        actual.push(event(&mut receiver).await);
    }

    client.disconnect().await.unwrap();

    let mut expected: Vec<_> = [
        (
            "101.000000",
            "201.000000",
            "2026-10-14T00:01:00.0000001Z",
            "2026-10-14T00:01:00.0000003Z",
        ),
        (
            "102.000000",
            "202.000000",
            "2026-10-14T00:02:00.0000001Z",
            "2026-10-14T00:02:00.0000003Z",
        ),
        (
            "103.000000",
            "203.000000",
            "2026-10-14T00:03:00.0000001Z",
            "2026-10-14T00:03:00.0000003Z",
        ),
        (
            "104.000000",
            "204.000000",
            "2026-10-14T00:04:00.0000001Z",
            "2026-10-14T00:04:00.0000003Z",
        ),
        (
            "105.000000",
            "205.000000",
            "2026-10-14T00:05:00.0000001Z",
            "2026-10-14T00:05:00.0000003Z",
        ),
        (
            "106.000000",
            "206.000000",
            "2026-10-14T00:06:00.0000001Z",
            "2026-10-14T00:06:00.0000003Z",
        ),
        (
            "107.000000",
            "207.000000",
            "2026-10-14T00:07:00.0000001Z",
            "2026-10-14T00:07:00.0000003Z",
        ),
        (
            "108.000000",
            "208.000000",
            "2026-10-14T00:08:00.0000001Z",
            "2026-10-14T00:08:00.0000003Z",
        ),
        (
            "109.000000",
            "209.000000",
            "2026-10-14T00:09:00.0000001Z",
            "2026-10-14T00:09:00.0000003Z",
        ),
        (
            "110.000000",
            "210.000000",
            "2026-10-14T00:10:00.0000001Z",
            "2026-10-14T00:10:00.0000003Z",
        ),
        (
            "111.000000",
            "211.000000",
            "2026-10-14T00:11:00.0000001Z",
            "2026-10-14T00:11:00.0000003Z",
        ),
        (
            "112.000000",
            "212.000000",
            "2026-10-14T00:12:00.0000001Z",
            "2026-10-14T00:12:00.0000003Z",
        ),
        (
            "113.000000",
            "213.000000",
            "2026-10-14T00:13:00.0000001Z",
            "2026-10-14T00:13:00.0000003Z",
        ),
        (
            "114.000000",
            "214.000000",
            "2026-10-14T00:14:00.0000001Z",
            "2026-10-14T00:14:00.0000003Z",
        ),
        (
            "115.000000",
            "215.000000",
            "2026-10-14T00:15:00.0000001Z",
            "2026-10-14T00:15:00.0000003Z",
        ),
        (
            "116.000000",
            "216.000000",
            "2026-10-14T00:16:00.0000001Z",
            "2026-10-14T00:16:00.0000003Z",
        ),
    ]
    .into_iter()
    .map(|(bid_qty, ask_qty, entry, recv)| {
        Data::Quote(
            QuoteTick::new_checked(
                id,
                Price::from("0.3400"),
                Price::from("0.3600"),
                Quantity::from(bid_qty),
                Quantity::from(ask_qty),
                entry.parse().unwrap(),
                recv.parse().unwrap(),
            )
            .unwrap(),
        )
    })
    .collect();

    for (trade_id, price, size, timestamp) in [
        (
            "authored-activity-1",
            "0.3600",
            "31.000000",
            "2026-10-14T00:00:01.0000001Z",
        ),
        (
            "authored-activity-2",
            "0.3700",
            "32.000000",
            "2026-10-14T00:00:02.0000002Z",
        ),
    ] {
        let timestamp = timestamp.parse().unwrap();
        expected.push(Data::Trade(
            TradeTick::new_checked(
                id,
                Price::from(price),
                Quantity::from(size),
                AggressorSide::NoAggressor,
                TradeId::from(trade_id),
                timestamp,
                timestamp,
            )
            .unwrap(),
        ));
    }

    assert_eq!(actual, expected);
    assert!(receiver.try_recv().is_err());
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert_request(
        &requests[2],
        &format!("/v1/activity/POLYMARKET/{market_id}/latest?limit=100"),
        &http,
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bars_preserve_zero_volume_price_periods() {
    #[cfg(all(feature = "simulation", madsim))]
    let clock = {
        let clock = get_atomic_clock_realtime();
        clock.make_static();
        clock.set_time("2026-10-14T00:03:00Z".parse().unwrap());
        clock
    };

    let market_id = ACTIVITY_MARKET_ID;
    let id = nautilus_finfeed::common::symbol::to_instrument_id("POLYMARKET", market_id).unwrap();
    let bar_type = BarType::from(format!("{id}-1-MINUTE-LAST-EXTERNAL").as_str());
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", ACTIVITY_MARKET),
        (200, "", include_str!("../../test_data/periods.json")),
        (
            200,
            "",
            include_str!("../../test_data/bars-zero-volume.json"),
        ),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http.clone(), vec![market_id.into()], 60_000).await;
    client
        .request_bars(RequestBars::new(
            bar_type,
            Some("2026-10-14T00:00:00Z".parse().unwrap()),
            Some("2026-10-14T00:03:00Z".parse().unwrap()),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(19),
            None,
        ))
        .unwrap();

    let DataResponse::Bars(bars) = response(&mut receiver).await else {
        panic!("expected zero-volume bars")
    };

    client.disconnect().await.unwrap();

    let expected: Vec<_> = [
        "2026-10-14T00:01:00Z",
        "2026-10-14T00:02:00Z",
        "2026-10-14T00:03:00Z",
    ]
    .into_iter()
    .map(|end| {
        let end = end.parse().unwrap();
        Bar::new_checked(
            bar_type,
            Price::from("0.3100"),
            Price::from("0.3400"),
            Price::from("0.3000"),
            Price::from("0.3200"),
            Quantity::from("0.000000"),
            end,
            end,
        )
        .unwrap()
    })
    .collect();

    assert_eq!(&*bars.data, expected.as_slice());

    for bar in bars.data.iter() {
        assert_eq!(
            [
                bar.open.precision,
                bar.high.precision,
                bar.low.precision,
                bar.close.precision,
                bar.volume.precision,
            ],
            [4, 4, 4, 4, 6]
        );
    }

    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 4);
    assert_request(
        &requests[3],
        &format!(
            "/v1/ohlcv/POLYMARKET/{market_id}/history?period_id=1MIN&time_start=2026-10-14T00%3A00%3A00.0000000Z&time_end=2026-10-14T00%3A03%3A00.0000000Z&limit=100000"
        ),
        &http,
    );
    #[cfg(all(feature = "simulation", madsim))]
    clock.make_realtime();
}

#[rstest]
fn invalid_configuration_is_rejected() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        FinFeedDataClient::new(
            ClientId::from("FINFEED"),
            FinFeedDataClientConfig::default(),
            sender.into()
        )
        .is_err()
    );
}

#[rstest]
#[case("quote-rounding")]
#[case("trade-id")]
#[case("trade-identity")]
#[case("trade-zero")]
#[case("history-cap")]
#[case("history-price")]
#[case("history-action")]
#[case("history-empty")]
#[case("book-last-level")]
#[case("book-missing-side")]
#[case("book-duplicate-price")]
#[case("book-crossed")]
#[case("bar-price")]
#[case("bar-duration")]
#[case("http-failure")]
#[case("saturation")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn failed_requests_emit_no_partial_response(#[case] scenario: &str) {
    let mut body: serde_json::Value = serde_json::from_str(match scenario {
        value if value.starts_with("history") => REPLAY,
        value if value.starts_with("book") => BOOK,
        value if value.starts_with("trade") => TRADES,
        value if value.starts_with("bar") => BARS,
        _ => QUOTES,
    })
    .unwrap();

    match scenario {
        "history-cap" => {
            let mut rows = body.as_array().unwrap()[..4].to_vec();
            rows.extend(vec![body[4].clone(); 9996]);
            body = serde_json::Value::Array(rows);
        }
        "history-price" => body[1]["price"] = json!("0.30001"),
        "history-action" => body[11]["update_type"] = json!(99),
        "history-empty" => body = serde_json::Value::Array(Vec::new()),
        "quote-rounding" => body[0]["bid"] = json!("0.12345"),
        "trade-id" => body[0]["id"] = serde_json::Value::Null,
        "trade-identity" => body[0]["market_id"] = json!("wrong-market"),
        "trade-zero" => body[0]["quantity"] = json!("0"),
        "book-last-level" => body["bids"][35]["size"] = json!("1.0000001"),
        "book-missing-side" => body["asks"] = serde_json::Value::Null,
        "book-duplicate-price" => body["bids"][1]["price"] = body["bids"][0]["price"].clone(),
        "book-crossed" => body["bids"][35]["price"] = json!("0.9999"),
        "bar-price" => body[0]["price_high"] = json!("0.01"),
        "bar-duration" => body[0]["time_period_end"] = json!("2020-10-11T00:01:30Z"),
        "saturation" => body = serde_json::Value::Array(vec![body[0].clone(); 10_000]),
        "http-failure" => {}
        _ => panic!("unknown scenario"),
    }

    let mut responses = vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
    ];

    if scenario.starts_with("bar") {
        responses.push((
            200,
            "",
            include_str!("../../test_data/periods.json").to_owned(),
        ));
    }

    responses.push((
        if scenario == "http-failure" { 400 } else { 200 },
        "",
        body.to_string(),
    ));
    let (peer, http) = start_peer(responses).await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();

    let date = if scenario.starts_with("bar") {
        "2020-10-11"
    } else {
        "2026-10-11"
    };

    let start = Some(format!("{date}T00:00:00Z").parse().unwrap());
    let end = Some(format!("{date}T00:02:00Z").parse().unwrap());

    if scenario.starts_with("history") {
        client
            .request_book_depth(RequestBookDepth::new(
                instrument_id(),
                Some("2026-10-11T00:00:01.0000001Z".parse().unwrap()),
                Some("2026-10-11T00:00:02.5000000Z".parse().unwrap()),
                NonZeroUsize::new(1),
                NonZeroUsize::new(1),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .unwrap();
    } else if scenario.starts_with("book") {
        client
            .request_book_snapshot(RequestBookSnapshot::new(
                instrument_id(),
                NonZeroUsize::new(1),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .unwrap();
    } else if scenario.starts_with("trade") {
        client
            .request_trades(RequestTrades::new(
                instrument_id(),
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .unwrap();
    } else if scenario.starts_with("bar") {
        let bar_type: BarType = format!("{}-1-MINUTE-LAST-EXTERNAL", instrument_id())
            .parse()
            .unwrap();
        client
            .request_bars(RequestBars::new(
                bar_type,
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .unwrap();
    } else {
        client
            .request_quotes(RequestQuotes::new(
                instrument_id(),
                start,
                end,
                NonZeroUsize::new(1),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .unwrap();
    }

    peer.await.unwrap();
    assert!(
        time::timeout(Duration::from_millis(200), receiver.recv())
            .await
            .is_err()
    );
    client.disconnect().await.unwrap();
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn disconnect_cancels_inflight_and_queued_requests_before_reconnect(#[case] books: bool) {
    let (listener, address) = super::dst::bind_peer().await;
    let (arrived_tx, arrived_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();

    let peer = task::spawn(async move {
        let mut requests = Vec::new();

        for body in [EXCHANGES, MARKET] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(super::dst::read_request(&mut stream).await);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        }

        let (mut pending, _) = listener.accept().await.unwrap();
        requests.push(super::dst::read_request(&mut pending).await);
        arrived_tx.send(()).unwrap();
        release_rx.await.unwrap();

        let body = if books { REPLAY } else { QUOTES };
        let _ = pending
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await;
        let _ = pending.flush().await;

        for body in [EXCHANGES, MARKET] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(super::dst::read_request(&mut stream).await);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        }

        requests
    });

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/"),
        min_request_interval_ms: 1,
        ..Default::default()
    };

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();

    let request = |client: &FinFeedDataClient| {
        let start = Some("2026-10-11T00:00:01.0000001Z".parse().unwrap());
        let end = Some("2026-10-11T00:00:05Z".parse().unwrap());

        if books {
            client.request_book_deltas(RequestBookDeltas::new(
                instrument_id(),
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
        } else {
            client.request_quotes(RequestQuotes::new(
                instrument_id(),
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
        }
    };

    request(&client).unwrap();
    arrived_rx.await.unwrap();
    request(&client).unwrap();
    time::timeout(Duration::from_secs(4), client.disconnect())
        .await
        .unwrap()
        .unwrap();
    assert!(client.is_disconnected());
    assert!(request(&client).is_err());
    assert!(receiver.try_recv().is_err());
    release_tx.send(()).unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    assert!(client.is_connected());
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests[3].starts_with("GET /v1/exchanges HTTP/1.1"));
    assert_eq!(requests[1].lines().next(), requests[4].lines().next());
    assert!(receiver.try_recv().is_err());
    client.disconnect().await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn missing_instruments_invalid_windows_and_historical_books_fail_before_io() {
    let (peer, http) = start_peer(vec![(200, "", EXCHANGES), (200, "", MARKET)]).await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let start = Some("2026-10-11T00:00:00Z".parse().unwrap());
    let end = Some("2026-10-11T00:02:00Z".parse().unwrap());
    let absent =
        nautilus_finfeed::common::symbol::to_instrument_id("POLYMARKET", "unknown_NO").unwrap();
    assert!(
        client
            .request_trades(RequestTrades::new(
                absent,
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None
            ))
            .is_err()
    );

    let bar_type: BarType = format!("{}-1-MINUTE-LAST-EXTERNAL", instrument_id())
        .parse()
        .unwrap();

    for (start, end, expected) in [
        (None, end, "FinFeed history requires a start time"),
        (end, start, "FinFeed history start exceeds end"),
    ] {
        let results = [
            client.request_quotes(RequestQuotes::new(
                instrument_id(),
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            )),
            client.request_trades(RequestTrades::new(
                instrument_id(),
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            )),
            client.request_bars(RequestBars::new(
                bar_type,
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            )),
            client.request_book_deltas(RequestBookDeltas::new(
                instrument_id(),
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            )),
        ];

        for result in results {
            assert_eq!(result.unwrap_err().to_string(), expected);
        }
    }

    let historical = Some(serde_json::from_value::<Params>(json!({"date":"2026-10-11"})).unwrap());
    assert!(
        client
            .request_book_snapshot(RequestBookSnapshot::new(
                instrument_id(),
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                historical
            ))
            .is_err()
    );
    let l3 = Some(serde_json::from_value::<Params>(json!({"book_type":"L3_MBO"})).unwrap());
    assert!(
        client
            .request_book_snapshot(RequestBookSnapshot::new(
                instrument_id(),
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                l3
            ))
            .is_err()
    );
    assert!(
        client
            .request_book_depth(RequestBookDepth::new(
                instrument_id(),
                None,
                end,
                None,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None
            ))
            .is_err()
    );
    let bid_type: BarType = format!("{}-1-MINUTE-BID-EXTERNAL", instrument_id())
        .parse()
        .unwrap();
    let rejected = client
        .request_bars(RequestBars::new(
            bid_type,
            start,
            start,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
        ))
        .unwrap_err();
    assert_eq!(rejected.to_string(), "FinFeed OHLCV uses last prices");

    for aggregation in ["1-MILLISECOND", "100-TICK", "100-VOLUME", "100-VALUE"] {
        let unsupported: BarType = format!("{}-{aggregation}-LAST-EXTERNAL", instrument_id())
            .parse()
            .unwrap();

        for end in [start, end] {
            let rejected = client
                .request_bars(RequestBars::new(
                    unsupported,
                    start,
                    end,
                    None,
                    None,
                    UUID4::new(),
                    UnixNanos::from(1),
                    None,
                ))
                .unwrap_err();
            assert_eq!(rejected.to_string(), "unsupported FinFeed bar aggregation");
        }
    }

    client.stop().unwrap();
    client.stop().unwrap();
    assert!(client.is_disconnected());
    client.disconnect().await.unwrap();
    assert_eq!(peer.await.unwrap().len(), 2);
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn empty_daily_page_advances_and_point_window_retains_shared_timestamps() {
    let next_day = QUOTES.replace("2026-10-11", "2026-10-12");
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", "[]".to_owned()),
        (200, "", next_day),
        (200, "", TRADES.to_owned()),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    client
        .request_quotes(RequestQuotes::new(
            instrument_id(),
            Some("2026-10-11T00:00:00Z".parse().unwrap()),
            Some("2026-10-12T00:00:03Z".parse().unwrap()),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
        ))
        .unwrap();

    let DataResponse::Quotes(quotes) = response(&mut receiver).await else {
        panic!("expected quotes")
    };

    assert_eq!(quotes.data.len(), 5);
    let point = Some("2026-10-11T00:00:02Z".parse().unwrap());
    client
        .request_trades(RequestTrades::new(
            instrument_id(),
            point,
            point,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
        ))
        .unwrap();

    let DataResponse::Trades(trades) = response(&mut receiver).await else {
        panic!("expected trades")
    };

    assert_eq!(
        trades
            .data
            .iter()
            .map(|trade| trade.trade_id.as_str())
            .collect::<Vec<_>>(),
        ["trade-1", "trade-3"]
    );
    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(
        requests[2]
            .lines()
            .next()
            .unwrap()
            .contains("date=2026-10-11")
    );
    assert!(
        requests[3]
            .lines()
            .next()
            .unwrap()
            .contains("date=2026-10-12")
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn failed_bootstrap_stays_disconnected_and_can_reconnect() {
    let (peer, http) = start_peer(vec![
        (400, "", "[]"),
        (200, "", EXCHANGES),
        (200, "", MARKET),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    assert!(client.connect().await.is_err());
    assert!(client.is_disconnected());
    assert!(receiver.try_recv().is_err());
    client.connect().await.unwrap();
    assert!(client.is_connected());
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    client.disconnect().await.unwrap();
    assert_eq!(peer.await.unwrap().len(), 3);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bootstrap_timeout_and_dropped_connect_leave_no_readiness() {
    for outer_deadline in [false, true] {
        let (peer, http) =
            super::dst::start_peer_delayed(vec![(200, "", EXCHANGES)], Duration::from_secs(2))
                .await;
        let mut config = config(http);
        config.connect_timeout_secs = 1;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut client =
            FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();

        if outer_deadline {
            assert!(
                time::timeout(Duration::from_millis(20), client.connect())
                    .await
                    .is_err()
            );
        } else {
            assert!(
                client
                    .connect()
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("bootstrap timed out")
            );
        }

        assert!(client.is_disconnected());
        assert!(receiver.try_recv().is_err());
        client.disconnect().await.unwrap();
        peer.abort();
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unselected_outcome_refresh_updates_request_parsing_context() {
    let yes_id = MARKET_ID.replace("_NO", "_YES");
    let mut yes: serde_json::Value = serde_json::from_str(MARKET).unwrap();
    yes["market_id"] = json!(yes_id);
    yes["outcome_name"] = json!("Yes");
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", yes.to_string()),
        (200, "", QUOTES.to_owned()),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let id = nautilus_finfeed::common::symbol::to_instrument_id("POLYMARKET", &yes_id).unwrap();
    let request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            id,
            None,
            None,
            Some(ClientId::from("REQUESTED")),
            request_id,
            UnixNanos::from(1),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Instrument(result) = response(&mut receiver).await else {
        panic!("expected fresh definition")
    };

    assert_eq!(result.correlation_id, request_id);
    assert_eq!(result.instrument_id, id);
    assert_eq!(result.data.id(), id);
    assert_eq!(result.data.price_precision(), 4);
    assert_eq!(result.data.size_precision(), 6);
    client
        .request_quotes(RequestQuotes::new(
            id,
            Some("2026-10-11T00:00:00Z".parse().unwrap()),
            Some("2026-10-11T00:00:04Z".parse().unwrap()),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(2),
            None,
        ))
        .unwrap();

    let DataResponse::Quotes(quotes) = response(&mut receiver).await else {
        panic!("expected quotes for refreshed outcome")
    };

    assert_eq!(quotes.instrument_ids, vec![id]);
    assert_eq!(quotes.data.len(), 6);
    assert!(quotes.data.iter().all(|quote| quote.instrument_id == id));
    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests[2].lines().next().unwrap().contains(&yes_id));
    assert!(requests[3].lines().next().unwrap().contains(&yes_id));
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn failed_list_refresh_retains_token_ownership_and_retry_preserves_ad_hoc_definitions(
    #[case] duplicate: bool,
) {
    let alpha = outcome_definition("alpha_NO", "101");
    let beta = outcome_definition("beta_NO", "102");
    let delta = outcome_definition("delta_NO", "104");
    let refreshed_alpha = outcome_definition("alpha_NO", "103");
    let refreshed_beta = outcome_definition("beta_NO", "105");
    let gamma = outcome_definition("gamma_NO", "101");

    let (status, failure) = if duplicate {
        (200, outcome_definition("beta_NO", "104"))
    } else {
        (404, include_str!("../../test_data/empty.json").to_owned())
    };

    let replies = vec![
        (200, EXCHANGES.to_owned()),
        (200, alpha),
        (200, beta),
        (200, delta.clone()),
        (200, refreshed_alpha.clone()),
        (status, failure),
        (200, gamma.clone()),
        (200, delta.clone()),
        (200, refreshed_alpha.clone()),
        (200, refreshed_beta.clone()),
        (200, gamma),
        (200, outcome_definition("epsilon_NO", "104")),
        (200, delta),
    ];
    let (peer, http, mut observed) = timed_peer(
        replies
            .into_iter()
            .map(|(status, body)| (status, body, Duration::ZERO))
            .collect(),
    )
    .await;
    let mut config = config(http);
    config.instruments[0].market_ids = vec!["alpha_NO".into(), "beta_NO".into()];
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    client.connect().await.unwrap();

    for name in ["alpha_NO", "beta_NO"] {
        let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
            panic!("expected startup definition")
        };

        assert_eq!(instrument.id(), outcome_id(name));
    }

    request_outcome(&client, "delta_NO");

    let DataResponse::Instrument(original_delta) = response(&mut receiver).await else {
        panic!("expected ad hoc definition")
    };

    client
        .request_instruments(RequestInstruments::new(
            None,
            None,
            None,
            Some(outcome_id("alpha_NO").venue),
            UUID4::new(),
            UnixNanos::from(0),
            None,
        ))
        .unwrap();

    for expected in 0..=5 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    request_outcome(&client, "gamma_NO");
    assert_eq!(observed.recv().await.unwrap().0, 6);
    request_outcome(&client, "delta_NO");

    let DataResponse::Instrument(retained_delta) = response(&mut receiver).await else {
        panic!("expected only the sentinel after rejected list and duplicate requests")
    };

    assert_eq!(retained_delta.instrument_id, original_delta.instrument_id);
    assert_eq!(retained_delta.data.id(), outcome_id("delta_NO"));

    let retry_id = UUID4::new();
    client
        .request_instruments(RequestInstruments::new(
            None,
            None,
            None,
            Some(outcome_id("alpha_NO").venue),
            retry_id,
            UnixNanos::from(0),
            None,
        ))
        .unwrap();

    let DataResponse::Instruments(retry) = response(&mut receiver).await else {
        panic!("expected successful list retry")
    };

    assert_eq!(retry.correlation_id, retry_id);
    assert_eq!(retry.data.len(), 2);

    for (instrument, source) in retry.data.iter().zip([refreshed_alpha, refreshed_beta]) {
        let expected = nautilus_finfeed::http::parse::parse_instrument(
            &serde_json::from_str(&source).unwrap(),
            instrument.ts_init(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(instrument).unwrap(),
            serde_json::to_value(InstrumentAny::from(expected)).unwrap()
        );
    }

    request_outcome(&client, "gamma_NO");

    let DataResponse::Instrument(released) = response(&mut receiver).await else {
        panic!("expected old token to become available after successful publication")
    };

    assert_eq!(released.instrument_id, outcome_id("gamma_NO"));
    assert_eq!(
        released.data.info().unwrap().get_str("token_id"),
        Some("101")
    );

    request_outcome(&client, "epsilon_NO");

    for expected in 7..=11 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    request_outcome(&client, "delta_NO");

    let DataResponse::Instrument(retained) = response(&mut receiver).await else {
        panic!("expected ad hoc ownership to survive successful list refresh")
    };

    assert_eq!(retained.instrument_id, outcome_id("delta_NO"));
    assert_eq!(
        retained.data.info().unwrap().get_str("token_id"),
        Some("104")
    );
    client.disconnect().await.unwrap();
    assert!(receiver.try_recv().is_err());
    let requests = peer.await.unwrap();
    let paths: Vec<_> = requests
        .iter()
        .map(|request| request.lines().next().unwrap().to_owned())
        .collect();
    let mut expected = vec!["GET /v1/exchanges HTTP/1.1".to_owned()];
    expected.extend(
        [
            "alpha_NO",
            "beta_NO",
            "delta_NO",
            "alpha_NO",
            "beta_NO",
            "gamma_NO",
            "delta_NO",
            "alpha_NO",
            "beta_NO",
            "gamma_NO",
            "epsilon_NO",
            "delta_NO",
        ]
        .map(|name| format!("GET /v1/markets/POLYMARKET/{name} HTTP/1.1")),
    );
    assert_eq!(paths, expected);
}

fn outcome_definition(name: &str, token: &str) -> String {
    let mut market: serde_json::Value = serde_json::from_str(MARKET).unwrap();
    market["market_id"] = name.into();
    let mut tokens: serde_json::Value =
        serde_json::from_str(market["source_specific_data"]["Tokens"].as_str().unwrap()).unwrap();
    tokens[1]["token_id"] = token.into();
    market["source_specific_data"]["Tokens"] = tokens.to_string().into();
    market.to_string()
}

fn outcome_id(name: &str) -> InstrumentId {
    nautilus_finfeed::common::symbol::to_instrument_id("POLYMARKET", name).unwrap()
}

fn request_outcome(client: &FinFeedDataClient, name: &str) {
    client
        .request_instrument(RequestInstrument::new(
            outcome_id(name),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
        ))
        .unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn periods_closing_after_now_never_become_historical_bars() {
    let future = BARS.replace("2020-10-11", "2030-10-11");
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (
            200,
            "",
            include_str!("../../test_data/periods.json").to_owned(),
        ),
        (200, "", future),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let bar_type: BarType = format!("{}-1-MINUTE-LAST-EXTERNAL", instrument_id())
        .parse()
        .unwrap();
    client
        .request_bars(RequestBars::new(
            bar_type,
            Some("2030-10-11T00:00:00Z".parse().unwrap()),
            Some("2030-10-11T00:03:00Z".parse().unwrap()),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
        ))
        .unwrap();

    let DataResponse::Bars(bars) = response(&mut receiver).await else {
        panic!("expected empty bars")
    };

    assert!(bars.data.is_empty());
    client.disconnect().await.unwrap();
    assert_eq!(peer.await.unwrap().len(), 4);
}

const REPLAY: &str = include_str!("../../test_data/book-replay.json");

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn historical_books_preserve_correlation_bounds_and_reconnect_replay() {
    let mut replies = Vec::new();
    for _ in 0..2 {
        replies.extend([
            (200, "", EXCHANGES),
            (200, "", MARKET),
            (200, "", REPLAY),
            (200, "", REPLAY),
            (200, "", REPLAY),
        ]);
    }

    let (peer, http) = start_peer(replies).await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    let start = Some("2026-10-11T00:00:01.0000001Z".parse().unwrap());
    let end = Some("2026-10-11T00:00:05.0000001Z".parse().unwrap());
    let requested = ClientId::from("REQUESTED");

    for _ in 0..2 {
        client.connect().await.unwrap();
        receiver.try_recv().unwrap();
        let request_id = UUID4::new();
        client
            .request_book_deltas(RequestBookDeltas::new(
                instrument_id(),
                start,
                end,
                None,
                Some(requested),
                request_id,
                UnixNanos::from(1),
                Some(params()),
            ))
            .unwrap();

        let DataResponse::BookDeltas(value) = response(&mut receiver).await else {
            panic!("expected book deltas")
        };

        assert_eq!(value.correlation_id, request_id);
        assert_eq!(value.client_id, requested);
        assert_eq!(value.instrument_id, instrument_id());
        assert_eq!(
            value.start,
            nautilus_core::datetime::datetime_to_unix_nanos(start)
        );
        assert_eq!(
            value.end,
            nautilus_core::datetime::datetime_to_unix_nanos(end)
        );
        assert_eq!(value.params, Some(params()));
        assert_eq!(value.data.len(), 15);
        let mut consumer = OrderBook::new(instrument_id(), BookType::L2_MBP);
        for delta in value.data.iter() {
            consumer.apply_delta(delta).unwrap();
        }

        assert_eq!(
            consumer.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.2".parse::<Decimal>().unwrap(), Decimal::from(6))]
        );
        assert_eq!(
            consumer.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.9".parse::<Decimal>().unwrap(), Decimal::from(10))]
        );
        let request_id = UUID4::new();
        client
            .request_book_depth(RequestBookDepth::new(
                instrument_id(),
                start,
                end,
                NonZeroUsize::new(2),
                NonZeroUsize::new(1),
                Some(requested),
                request_id,
                UnixNanos::from(1),
                Some(params()),
            ))
            .unwrap();

        let DataResponse::BookDepth(value) = response(&mut receiver).await else {
            panic!("expected book depth")
        };

        assert_eq!(value.correlation_id, request_id);
        assert_eq!(value.client_id, requested);
        assert_eq!(value.instrument_id, instrument_id());
        assert_eq!(
            value.start,
            nautilus_core::datetime::datetime_to_unix_nanos(start)
        );
        assert_eq!(
            value.end,
            nautilus_core::datetime::datetime_to_unix_nanos(end)
        );
        assert_eq!(value.params, Some(params()));
        assert_eq!(value.data.len(), 2);
        assert_eq!(value.data[0].bids[0].size.as_decimal(), Decimal::from(2));
        assert_eq!(
            value.data[1].bids[0].size.as_decimal(),
            "5.25".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            value.data[1].asks[0].price.as_decimal(),
            "0.65".parse::<Decimal>().unwrap()
        );
        let point = "2026-10-11T00:00:02.5000000Z";
        let mut point_params = IndexMap::clone(&params());
        point_params.insert("end".into(), point.into());
        let point_params = Params::new(point_params);
        let request_id = UUID4::new();
        client
            .request_book_snapshot(RequestBookSnapshot::new(
                instrument_id(),
                NonZeroUsize::new(1),
                Some(requested),
                request_id,
                UnixNanos::from(1),
                Some(point_params.clone()),
            ))
            .unwrap();

        let DataResponse::Book(value) = response(&mut receiver).await else {
            panic!("expected historical book")
        };

        let bound =
            nautilus_core::datetime::datetime_to_unix_nanos(Some(point.parse().unwrap())).unwrap();
        assert_eq!(value.correlation_id, request_id);
        assert_eq!(value.client_id, requested);
        assert_eq!(value.instrument_id, instrument_id());
        assert_eq!(value.start, Some(bound));
        assert_eq!(value.end, Some(bound));
        assert_eq!(value.params, Some(point_params));
        assert_eq!(value.data.ts_last, bound);
        assert_eq!(
            value.data.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![(
                "0.4".parse::<Decimal>().unwrap(),
                "5.25".parse::<Decimal>().unwrap()
            )]
        );
        assert_eq!(
            value.data.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.65".parse::<Decimal>().unwrap(), Decimal::from(8))]
        );
        client.disconnect().await.unwrap();
        assert!(receiver.try_recv().is_err());
    }

    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 10);

    for request in requests
        .iter()
        .filter(|request| request.contains("/history?"))
    {
        assert!(request.contains("time_start=2026-10-11T00%3A00%3A00.0000000Z&time_end=2026-10-11T01%3A00%3A00.0000000Z&limit=10000"));
    }
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn hourly_book_replay_preserves_handoff_and_rejects_overlap(#[case] overlap: bool) {
    let rows: serde_json::Value = serde_json::from_str(REPLAY).unwrap();
    let first = serde_json::Value::Array(rows.as_array().unwrap()[..10].to_vec());
    let mut second = rows.clone();
    for row in second.as_array_mut().unwrap() {
        let time = row["time_exchange"]
            .as_str()
            .unwrap()
            .replace("T00:", "T01:");
        row["time_exchange"] = time.into();
    }

    if overlap {
        second.as_array_mut().unwrap().insert(0, first[9].clone());
    }

    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", first.to_string()),
        (200, "", second.to_string()),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    client
        .request_book_deltas(RequestBookDeltas::new(
            instrument_id(),
            Some("2026-10-11T00:00:01.0000001Z".parse().unwrap()),
            Some("2026-10-11T01:00:02.0000001Z".parse().unwrap()),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
        ))
        .unwrap();
    let requests = peer.await.unwrap();

    if overlap {
        assert!(
            time::timeout(Duration::from_millis(200), receiver.recv())
                .await
                .is_err()
        );
    } else {
        let DataResponse::BookDeltas(value) = response(&mut receiver).await else {
            panic!("expected book deltas")
        };

        let mut book = OrderBook::new(instrument_id(), BookType::L2_MBP);
        for delta in value.data.iter() {
            book.apply_delta(delta).unwrap();
        }

        assert_eq!(
            book.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![
                (
                    "0.4".parse::<Decimal>().unwrap(),
                    "5.25".parse::<Decimal>().unwrap()
                ),
                ("0.3".parse::<Decimal>().unwrap(), Decimal::from(5))
            ]
        );
        assert_eq!(
            book.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![
                ("0.65".parse::<Decimal>().unwrap(), Decimal::from(8)),
                ("0.7".parse::<Decimal>().unwrap(), Decimal::from(7))
            ]
        );
        assert_eq!(
            value
                .data
                .iter()
                .filter(|delta| delta.flags & RecordFlag::F_LAST as u8 != 0)
                .count(),
            6
        );
    }

    client.disconnect().await.unwrap();
    assert!(receiver.try_recv().is_err());
    assert_eq!(requests.len(), 4);
    assert!(requests[2].contains(
        "time_start=2026-10-11T00%3A00%3A00.0000000Z&time_end=2026-10-11T01%3A00%3A00.0000000Z"
    ));
    assert!(requests[3].contains(
        "time_start=2026-10-11T01%3A00%3A00.0000000Z&time_end=2026-10-11T02%3A00%3A00.0000000Z"
    ));
}

#[rstest]
#[case("2019-02-01T00:00:00Z", "2019-03-01T00:00:00Z", "1-MONTH", "1MTH")]
#[case("2020-02-01T00:00:00Z", "2020-03-01T00:00:00Z", "1-MONTH", "1MTH")]
#[case("2020-04-01T00:00:00Z", "2020-05-01T00:00:00Z", "1-MONTH", "1MTH")]
#[case("2020-01-01T00:00:00Z", "2020-02-01T00:00:00Z", "1-MONTH", "1MTH")]
#[case("2019-12-01T00:00:00Z", "2020-03-01T00:00:00Z", "3-MONTH", "3MTH")]
#[case("2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z", "1-YEAR", "1YRS")]
#[case("2019-01-01T00:00:00Z", "2021-01-01T00:00:00Z", "2-YEAR", "2YRS")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn complete_calendar_bars_use_the_full_window(
    #[case] start: &str,
    #[case] end: &str,
    #[case] suffix: &str,
    #[case] period_id: &str,
) {
    let mut records: serde_json::Value = serde_json::from_str(BARS).unwrap();
    let mut row = records[0].clone();
    row["time_period_start"] = start.into();
    row["time_period_end"] = end.into();
    row["time_open"] = start.into();
    row["time_close"] = end.into();
    row["volume_traded"] = json!("3601.4359029999996");
    records = json!([row]);
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (
            200,
            "",
            include_str!("../../test_data/periods.json").to_owned(),
        ),
        (200, "", records.to_string()),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let bar_type = BarType::from(format!("{}-{suffix}-LAST-EXTERNAL", instrument_id()).as_str());
    let request_id = UUID4::new();
    let start = start.parse().unwrap();
    let end = end.parse().unwrap();
    client
        .request_bars(RequestBars::new(
            bar_type,
            Some(start),
            Some(end),
            NonZeroUsize::new(1),
            Some(ClientId::from("CALENDAR")),
            request_id,
            UnixNanos::from(7),
            Some(params()),
        ))
        .unwrap();

    let DataResponse::Bars(result) = response(&mut receiver).await else {
        panic!("expected bars")
    };

    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    let end = nautilus_core::datetime::datetime_to_unix_nanos(Some(end)).unwrap();

    assert_eq!(result.correlation_id, request_id);
    assert_eq!(result.client_id, ClientId::from("CALENDAR"));
    assert_eq!(result.bar_type, bar_type);
    assert_eq!(
        result.start,
        nautilus_core::datetime::datetime_to_unix_nanos(Some(start))
    );
    assert_eq!(result.end, Some(end));
    assert_eq!(result.params, Some(params()));
    assert_eq!(result.data.len(), 1);
    let bar = result.data[0];
    assert_eq!(bar.bar_type, bar_type);
    assert_eq!(bar.open.as_decimal(), Decimal::new(2, 1));
    assert_eq!(bar.high.as_decimal(), Decimal::new(4, 1));
    assert_eq!(bar.low.as_decimal(), Decimal::new(1, 1));
    assert_eq!(bar.close.as_decimal(), Decimal::new(3, 1));
    assert_eq!(bar.volume.as_decimal(), Decimal::new(3_601_435_903, 6));
    assert_eq!(bar.ts_event, end);
    assert_eq!(bar.ts_init, end);
    assert_eq!(requests.len(), 4);
    assert!(requests[3].contains(&format!("period_id={period_id}&")));
    assert!(requests[3].contains("limit=100000 HTTP/1.1"));
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn multi_month_history_crosses_empty_dates_and_keeps_earliest_records(#[case] trades: bool) {
    let start: jiff::Timestamp = "2024-01-01T00:00:02Z".parse().unwrap();
    let end: jiff::Timestamp = "2024-03-01T00:00:02Z".parse().unwrap();
    let first_day = start.as_second().div_euclid(86_400);
    let last_day = end.as_second().div_euclid(86_400);
    let mut responses = vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
    ];

    for day in first_day..=last_day {
        let body = if day == first_day || day == last_day {
            let date = jiff::Timestamp::from_second(day * 86_400)
                .unwrap()
                .to_string();
            let date = &date[..10];
            if trades { TRADES } else { QUOTES }.replace("2026-10-11", date)
        } else {
            "[]".to_owned()
        };

        responses.push((200, "", body));
    }

    let (peer, http) = start_peer(responses).await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    let request_id = UUID4::new();

    if trades {
        client
            .request_trades(RequestTrades::new(
                instrument_id(),
                Some(start),
                Some(end),
                NonZeroUsize::new(2),
                Some(ClientId::from("LONG")),
                request_id,
                UnixNanos::from(7),
                Some(params()),
            ))
            .unwrap();
    } else {
        client
            .request_quotes(RequestQuotes::new(
                instrument_id(),
                Some(start),
                Some(end),
                NonZeroUsize::new(2),
                Some(ClientId::from("LONG")),
                request_id,
                UnixNanos::from(7),
                Some(params()),
            ))
            .unwrap();
    }

    let result = response(&mut receiver).await;
    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    let expected_start = nautilus_core::datetime::datetime_to_unix_nanos(Some(start));
    let expected_end = nautilus_core::datetime::datetime_to_unix_nanos(Some(end));
    let expected = expected_start.unwrap();

    let times = match result {
        DataResponse::Trades(result) => {
            assert_eq!(result.correlation_id, request_id);
            assert_eq!(result.client_id, ClientId::from("LONG"));
            assert_eq!(result.instrument_ids, vec![instrument_id()]);
            assert_eq!(result.start, expected_start);
            assert_eq!(result.end, expected_end);
            assert_eq!(result.params, Some(params()));
            result
                .data
                .iter()
                .map(|value| value.ts_init)
                .collect::<Vec<_>>()
        }
        DataResponse::Quotes(result) => {
            assert_eq!(result.correlation_id, request_id);
            assert_eq!(result.client_id, ClientId::from("LONG"));
            assert_eq!(result.instrument_ids, vec![instrument_id()]);
            assert_eq!(result.start, expected_start);
            assert_eq!(result.end, expected_end);
            assert_eq!(result.params, Some(params()));
            result
                .data
                .iter()
                .map(|value| value.ts_init)
                .collect::<Vec<_>>()
        }
        _ => panic!("expected trades or quotes"),
    };

    assert_eq!(times, vec![expected, expected]);
    assert_eq!(requests.len(), 63);

    for (request, day) in requests[2..].iter().zip(first_day..=last_day) {
        let date = jiff::Timestamp::from_second(day * 86_400)
            .unwrap()
            .to_string();
        assert!(request.contains(&format!("date={}&limit=10000 HTTP/1.1", &date[..10])));
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn oversized_history_preserves_allowance_for_a_smaller_request() {
    let (peer, mut http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", QUOTES),
    ])
    .await;
    http.attempt_budget = 4;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config(http), sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    client
        .request_quotes(RequestQuotes::new(
            instrument_id(),
            Some("2026-10-11T00:00:00Z".parse().unwrap()),
            Some("2027-10-11T00:00:00Z".parse().unwrap()),
            NonZeroUsize::new(1),
            None,
            UUID4::new(),
            UnixNanos::from(7),
            None,
        ))
        .unwrap();
    let request_id = UUID4::new();
    client
        .request_quotes(RequestQuotes::new(
            instrument_id(),
            Some("2026-10-11T00:00:01Z".parse().unwrap()),
            Some("2026-10-11T00:00:03Z".parse().unwrap()),
            NonZeroUsize::new(1),
            None,
            request_id,
            UnixNanos::from(8),
            None,
        ))
        .unwrap();

    let DataResponse::Quotes(result) = response(&mut receiver).await else {
        panic!("expected smaller quote request")
    };

    assert_eq!(result.correlation_id, request_id);
    assert_eq!(result.data.len(), 1);
    let requests = peer.await.unwrap();
    assert!(
        time::timeout(Duration::from_millis(200), receiver.recv())
            .await
            .is_err()
    );
    client.disconnect().await.unwrap();

    assert_eq!(requests.len(), 3);
    assert!(requests[2].contains("date=2026-10-11&limit=10000 HTTP/1.1"));
    assert!(receiver.try_recv().is_err());
}
