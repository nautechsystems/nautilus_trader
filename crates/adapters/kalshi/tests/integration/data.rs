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

use std::{collections::BTreeMap, num::NonZeroUsize, sync::atomic::Ordering, time::Duration};

use nautilus_common::{
    clients::DataClient,
    messages::{
        DataEvent,
        data::{
            DataResponse, RequestBookSnapshot, RequestInstrument, RequestInstruments,
            SubscribeBookDeltas, SubscribeInstrumentClose, SubscribeInstrumentStatus,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, SubscriptionError,
            UnsubscribeBookDeltas, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{UUID4, UnixNanos, params::Params};
use nautilus_kalshi::{
    common::{
        consts::{KALSHI_CLIENT_ID, KALSHI_VENUE},
        parse::parse_instrument_id,
    },
    data::KalshiDataClient,
};
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::Data,
    enums::{BookType, InstrumentCloseType, MarketStatusAction, RecordFlag},
    identifiers::{ClientId, InstrumentId},
    instruments::{BinaryOption, Instrument, InstrumentAny},
};
use rust_decimal::Decimal;
use serde_json::Value;

use crate::data_peer::Peer;

#[tokio::test]
async fn lifecycle_streams_selected_types_and_shares_demand_across_reconnect() {
    tokio::time::timeout(Duration::from_secs(12), async {
        let mut peer = Peer::new().await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let ids = ids();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            ids.clone(),
            sender.into(),
        )
        .unwrap();
        client.subscribe_quotes(quotes(ids[0])).unwrap();
        client.subscribe_quotes(quotes(ids[0])).unwrap();
        client
            .subscribe_trades(trades(ids[0]).with_instrument_ids(ids.clone()))
            .unwrap();

        client.connect().await.unwrap();
        assert!(client.is_connected());
        client.connect().await.unwrap();
        assert_eq!(peer.requests.load(Ordering::SeqCst), 12);

        for id in &ids {
            assert_eq!(definition(&mut events).await.id, *id);
        }

        let mut channels = Vec::new();

        for _ in 0..3 {
            let command = peer.command().await;
            assert_eq!(command["cmd"], "subscribe");
            let channel = command["params"]["channels"][0]
                .as_str()
                .unwrap()
                .to_owned();

            if channel != "market_lifecycle_v2" {
                assert_eq!(
                    command["params"]["market_tickers"],
                    serde_json::json!([ids[0].symbol.as_str(), ids[1].symbol.as_str()])
                );
            }

            channels.push(channel);
        }

        channels.sort();
        assert_eq!(channels, vec!["market_lifecycle_v2", "ticker", "trade"]);

        peer.emit("ticker", ids[0], false);

        let DataEvent::Data(Data::Quote(tick)) = events.recv().await.unwrap() else {
            panic!("expected quote");
        };

        assert_eq!(tick.instrument_id, ids[0]);
        assert_eq!(tick.bid_price.as_decimal(), "0.123456".parse().unwrap());
        assert_eq!(tick.ask_price.as_decimal(), "0.789123".parse().unwrap());
        assert_eq!(tick.bid_size.as_decimal(), "3.25".parse().unwrap());
        assert_eq!(tick.ask_size.as_decimal(), "6.75".parse().unwrap());
        assert_eq!(tick.ts_event, UnixNanos::from(1_700_000_123_789_000_000));
        assert!(tick.ts_init > UnixNanos::from(1));
        peer.emit("ticker", ids[1], false);
        peer.emit("trade", ids[0], true);
        peer.emit("trade", ids[1], false);

        let DataEvent::Data(Data::Trade(tick)) = events.recv().await.unwrap() else {
            panic!("expected requested trade");
        };

        assert_eq!(tick.instrument_id, ids[1]);
        assert_eq!(tick.size.as_decimal(), "7.25".parse().unwrap());
        assert!(events.try_recv().is_err());

        client.subscribe_quotes(quotes(ids[1])).unwrap();
        peer.emit("ticker", ids[1], false);

        let DataEvent::Data(Data::Quote(tick)) = events.recv().await.unwrap() else {
            panic!("expected quote for later demand");
        };

        assert_eq!(tick.instrument_id, ids[1]);
        assert!(peer.commands.try_recv().is_err());

        let unsubscribe = UnsubscribeQuotes::new(
            ids[0],
            Some(*KALSHI_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::from(4),
            None,
            None,
        );

        client.unsubscribe_quotes(&unsubscribe).unwrap();
        peer.emit("ticker", ids[0], false);
        assert!(matches!(
            events.recv().await.unwrap(),
            DataEvent::Data(Data::Quote(_))
        ));
        assert!(peer.commands.try_recv().is_err());
        client.unsubscribe_quotes(&unsubscribe).unwrap();
        peer.emit("ticker", ids[1], false);

        let DataEvent::Data(Data::Quote(tick)) = events.recv().await.unwrap() else {
            panic!("expected remaining market demand");
        };

        assert_eq!(tick.instrument_id, ids[1]);
        assert!(peer.commands.try_recv().is_err());
        client
            .unsubscribe_quotes(&UnsubscribeQuotes::new(
                ids[1],
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(5),
                None,
                None,
            ))
            .unwrap();
        assert_eq!(peer.command().await["cmd"], "unsubscribe");
        client.subscribe_quotes(quotes(ids[0])).unwrap();
        assert_eq!(peer.command().await["cmd"], "subscribe");
        peer.emit("ticker", ids[0], false);
        assert!(matches!(
            events.recv().await.unwrap(),
            DataEvent::Data(Data::Quote(_))
        ));

        peer.revision.store(1, Ordering::SeqCst);
        peer.early.store(true, Ordering::SeqCst);
        peer.actions.send(("close".into(), Value::Null)).unwrap();
        client.stop().unwrap();
        client.connect().await.unwrap();

        for _ in 0..3 {
            assert_eq!(peer.command().await["cmd"], "subscribe");
        }

        for id in &ids {
            let instrument = definition(&mut events).await;
            assert_eq!(instrument.id, *id);
            assert_eq!(instrument.description.unwrap().as_str(), "revision-1");
        }

        assert!(matches!(events.recv().await.unwrap(), DataEvent::Data(_)));
        peer.emit("trade", ids[0], false);
        assert!(matches!(
            events.recv().await.unwrap(),
            DataEvent::Data(Data::Trade(_))
        ));
        client.stop().unwrap();
        client.stop().unwrap();
        assert!(client.is_disconnected());
        client.disconnect().await.unwrap();
        client.connect().await.unwrap();

        for id in &ids {
            assert_eq!(definition(&mut events).await.id, *id);
        }

        for _ in 0..3 {
            assert_eq!(peer.command().await["cmd"], "subscribe");
        }

        client.reset().unwrap();
        client.disconnect().await.unwrap();
        client.connect().await.unwrap();

        while let Ok(command) = peer.commands.try_recv() {
            if command["cmd"] == "subscribe" {
                assert_eq!(command["params"]["channels"][0], "market_lifecycle_v2");
            }
        }

        client.dispose().unwrap();
        client.disconnect().await.unwrap();
        assert!(client.connect().await.is_err());
        assert!(client.subscribe_quotes(quotes(ids[0])).is_err());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn explicit_requests_refresh_context_and_preserve_correlation_filters_and_bounds() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let peer = Peer::new().await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let ids = ids();
        let client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            ids.clone(),
            sender.into(),
        )
        .unwrap();

        let params = Params::new(
            [
                ("status".into(), "open".into()),
                ("exchange_index".into(), 2.into()),
            ]
            .into(),
        );

        let start = jiff::Timestamp::from_second(0).unwrap();
        let end = jiff::Timestamp::from_second(2_000_000_000).unwrap();

        for revision in [7, 8] {
            peer.revision.store(revision, Ordering::SeqCst);
            let request_id = UUID4::new();

            let request = RequestInstrument::new(
                ids[0],
                Some(start),
                Some(end),
                Some("KALSHI-REQUEST".into()),
                request_id,
                UnixNanos::from(9),
                Some(params.clone()),
            );

            client.request_instrument(request).unwrap();
            assert_eq!(
                definition(&mut events).await.description.unwrap().as_str(),
                format!("revision-{revision}")
            );

            let DataEvent::Response(DataResponse::Instrument(response)) =
                events.recv().await.unwrap()
            else {
                panic!("expected correlated instrument");
            };

            assert_eq!(response.correlation_id, request_id);
            assert_eq!(response.client_id, "KALSHI-REQUEST".into());
            assert_eq!(response.instrument_id, ids[0]);
            assert_eq!(response.start, Some(UnixNanos::from(0)));
            assert_eq!(
                response.end,
                Some(UnixNanos::from(2_000_000_000_000_000_000))
            );
            assert_eq!(response.params, Some(params.clone()));

            let InstrumentAny::BinaryOption(instrument) = response.data else {
                panic!("expected binary option");
            };

            assert_eq!(
                instrument.description.unwrap().as_str(),
                format!("revision-{revision}")
            );
        }

        let request_id = UUID4::new();
        client
            .request_instruments(RequestInstruments::new(
                Some(start),
                Some(end),
                Some("KALSHI-REQUEST".into()),
                None,
                request_id,
                UnixNanos::from(10),
                Some(params.clone()),
            ))
            .unwrap();

        assert_eq!(definition(&mut events).await.id, ids[1]);

        let DataEvent::Response(DataResponse::Instruments(response)) = events.recv().await.unwrap()
        else {
            panic!("expected correlated instruments");
        };

        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.client_id, "KALSHI-REQUEST".into());
        assert_eq!(response.venue.as_str(), "KALSHI");
        assert_eq!(response.start, Some(UnixNanos::from(0)));
        assert_eq!(
            response.end,
            Some(UnixNanos::from(2_000_000_000_000_000_000))
        );
        assert_eq!(response.params, Some(params));
        assert_eq!(response.data.len(), 2);
        assert_eq!(peer.requests.load(Ordering::SeqCst), 9);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn failed_bootstrap_canceled_connect_and_idle_closed_consumer_release_sessions() {
    tokio::time::timeout(Duration::from_secs(12), async {
        let mut peer = Peer::new().await;
        let (sender, events) = tokio::sync::mpsc::unbounded_channel();
        let id = ids()[0];
        let mut client = KalshiDataClient::new(*KALSHI_CLIENT_ID, peer.config.clone(), vec![id], sender.into()).unwrap();
        peer.revision.store(99, Ordering::SeqCst);
        assert!(client.connect().await.is_err());
        assert!(client.is_disconnected());
        assert!(peer.commands.try_recv().is_err());
        peer.revision.store(0, Ordering::SeqCst);
        client.subscribe_trades(trades(id)).unwrap();
        peer.confirm.store(false, Ordering::SeqCst);
        {
            let connect = client.connect();
            tokio::pin!(connect);
            tokio::select! {
                result = &mut connect => panic!("connect finished before unconfirmed command: {result:?}"),
                command = peer.command() => { assert_eq!(command["cmd"], "subscribe"); },
            }
        }

        client.disconnect().await.unwrap();
        peer.confirm.store(true, Ordering::SeqCst);
        client.connect().await.unwrap();

        while peer.commands.try_recv().is_ok() {}
        drop(events);

        loop {
            if peer.commands.recv().await.unwrap()["cmd"] == "closed" { break; }
        }

        assert!(client.is_disconnected());
        client.disconnect().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn blocked_command_is_canceled_without_emitting_a_response() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let peer = Peer::new().await;
        peer.revision.store(100, Ordering::SeqCst);
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let id = ids()[0];
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        let request =
            RequestInstrument::new(id, None, None, None, UUID4::new(), UnixNanos::from(1), None);
        client.request_instrument(request.clone()).unwrap();

        while peer.requests.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }

        client.stop().unwrap();
        tokio::time::timeout(Duration::from_secs(1), client.disconnect())
            .await
            .unwrap()
            .unwrap();
        assert!(events.try_recv().is_err());
        assert!(client.request_instrument(request).is_err());
        assert_eq!(peer.requests.load(Ordering::SeqCst), 1);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unsupported_subscriptions_and_invalid_bootstrap_selection_fail_explicitly() {
    let peer = Peer::new().await;
    let (sender, _events) = tokio::sync::mpsc::unbounded_channel();
    let id = ids()[0];
    assert_eq!(
        KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![],
            sender.clone().into()
        )
        .unwrap_err()
        .to_string(),
        "bootstrap markets must not be empty"
    );
    assert_eq!(
        KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id, id],
            sender.clone().into()
        )
        .unwrap_err()
        .to_string(),
        "duplicate bootstrap market"
    );
    let mut client = KalshiDataClient::new(
        *KALSHI_CLIENT_ID,
        peer.config.clone(),
        vec![id],
        sender.into(),
    )
    .unwrap();
    assert_eq!(
        client
            .subscribe_quotes(quotes(ids()[1]))
            .unwrap_err()
            .to_string(),
        "instrument is not a selected bootstrap market"
    );

    let command = SubscribeMarkPrices::new(
        id,
        Some(*KALSHI_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::from(1),
        None,
        None,
    );

    assert_eq!(
        client
            .subscribe_mark_prices(command)
            .unwrap_err()
            .to_string(),
        "Kalshi does not support subscribe_mark_prices"
    );
    assert_eq!(peer.requests.load(Ordering::SeqCst), 0);
}

fn fixture_books() -> Value {
    serde_json::from_str(include_str!("../../test_data/books.json")).unwrap()
}

fn snapshot_oracle() -> (BTreeMap<Decimal, Decimal>, BTreeMap<Decimal, Decimal>) {
    let snapshot = fixture_books()["snapshot"].clone();
    let mut bids = BTreeMap::new();
    let mut asks = BTreeMap::new();

    for row in snapshot["yes_dollars_fp"].as_array().unwrap() {
        bids.insert(
            row[0].as_str().unwrap().parse().unwrap(),
            row[1].as_str().unwrap().parse().unwrap(),
        );
    }

    for row in snapshot["no_dollars_fp"].as_array().unwrap() {
        asks.insert(
            row[0].as_str().unwrap().parse().unwrap(),
            row[1].as_str().unwrap().parse().unwrap(),
        );
    }

    (bids, asks)
}

fn ids() -> Vec<InstrumentId> {
    vec![
        parse_instrument_id("KXBTC15M-26OCT162345-45").unwrap(),
        parse_instrument_id("MARKET-B").unwrap(),
    ]
}

fn quotes(id: InstrumentId) -> SubscribeQuotes {
    SubscribeQuotes::new(
        id,
        Some(*KALSHI_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::from(2),
        None,
        None,
    )
}

fn trades(id: InstrumentId) -> SubscribeTrades {
    SubscribeTrades::new(
        id,
        Some(*KALSHI_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::from(3),
        None,
        None,
    )
}

async fn definition(events: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> BinaryOption {
    let DataEvent::Data(Data::Instrument(instrument)) = events.recv().await.unwrap() else {
        panic!("expected bootstrap definition before streaming");
    };

    let InstrumentAny::BinaryOption(instrument) = *instrument else {
        panic!("expected bootstrap definition before streaming");
    };

    instrument
}

#[rstest::rstest]
#[case::configured(None, None, Some(0))]
#[case::explicit(Some(1), None, Some(1))]
#[case::min_updated(None, Some("min_updated_ts"), None)]
#[case::max_updated(None, Some("max_updated_ts"), None)]
#[case::page_limit(None, Some("max_pages"), Some(0))]
#[case::status(None, Some("status"), Some(0))]
#[case::historical(None, Some("historical"), Some(0))]
#[tokio::test]
async fn bulk_instrument_requests_respect_configured_and_explicit_selection(
    #[case] requested: Option<usize>,
    #[case] filter: Option<&str>,
    #[case] expected: Option<usize>,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let peer = Peer::new().await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let instruments = ids();
        let client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![instruments[0]],
            sender.into(),
        )
        .unwrap();

        let params = requested
            .map(|index| {
                Params::new([("tickers".into(), instruments[index].symbol.as_str().into())].into())
            })
            .or_else(|| {
                filter.map(|filter| {
                    let value = match filter {
                        "min_updated_ts" => serde_json::json!(0),
                        "max_updated_ts" => serde_json::json!(2_000_000_000_i64),
                        "max_pages" => serde_json::json!(1),
                        "status" => serde_json::json!("open"),
                        "historical" => serde_json::json!(true),
                        _ => unreachable!(),
                    };

                    Params::new([(filter.into(), value)].into())
                })
            });

        let request_id = UUID4::new();
        client
            .request_instruments(RequestInstruments::new(
                None,
                None,
                None,
                Some(*KALSHI_VENUE),
                request_id,
                UnixNanos::from(1),
                params.clone(),
            ))
            .unwrap();

        let response = loop {
            match events.recv().await.unwrap() {
                DataEvent::Response(DataResponse::Instruments(response)) => break response,
                DataEvent::Data(Data::Instrument(_)) => {}
                event => panic!("unexpected instrument response event: {event:?}"),
            }
        };

        let paths = peer.request_paths.lock().unwrap();
        let path = paths
            .iter()
            .find(|path| path.contains("/markets?"))
            .unwrap();

        if let Some(index) = expected {
            assert!(path.contains(&format!("tickers={}", instruments[index].symbol)));
        } else {
            assert!(!path.contains("tickers="));
        }

        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.params, params);
        let mut actual: Vec<_> = response.data.iter().map(InstrumentAny::id).collect();
        actual.sort();
        let mut expected = expected.map_or(instruments.clone(), |index| vec![instruments[index]]);
        expected.sort();
        assert_eq!(actual, expected);
    })
    .await
    .unwrap();
}

#[rstest::rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn venue_routed_requests_survive_connect_and_use_configured_source(#[case] bulk: bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut peer = Peer::new().await;
        // The held request must outlive quota-limited lifecycle recovery
        peer.config.http_timeout_secs = 10;
        peer.config.ws_timeout_secs = 5;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let selected = ids();
        let configured_id = "KALSHI-ALT".into();
        let mut client = KalshiDataClient::new(
            configured_id,
            peer.config.clone(),
            selected.clone(),
            sender.into(),
        )
        .unwrap();
        peer.revision.store(100, Ordering::SeqCst);
        let request_id = UUID4::new();

        if bulk {
            client
                .request_instruments(RequestInstruments::new(
                    None,
                    None,
                    None,
                    Some(*KALSHI_VENUE),
                    request_id,
                    UnixNanos::from(1),
                    None,
                ))
                .unwrap();
        } else {
            client
                .request_instrument(RequestInstrument::new(
                    selected[0],
                    None,
                    None,
                    None,
                    request_id,
                    UnixNanos::from(2),
                    None,
                ))
                .unwrap();
        }

        while peer.requests.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        peer.revision.store(0, Ordering::SeqCst);
        client.connect().await.unwrap();

        for id in &selected {
            assert_eq!(definition(&mut events).await.id, *id);
        }

        peer.release.notify_waiters();

        loop {
            match events.recv().await.unwrap() {
                DataEvent::Response(DataResponse::Instrument(response)) => {
                    assert!(!bulk);
                    assert_eq!(response.client_id, configured_id);
                    assert_eq!(response.correlation_id, request_id);
                    assert_eq!(response.instrument_id, selected[0]);
                    break;
                }
                DataEvent::Response(DataResponse::Instruments(response)) => {
                    assert!(bulk);
                    assert_eq!(response.client_id, configured_id);
                    assert_eq!(response.correlation_id, request_id);
                    assert_eq!(response.venue, *KALSHI_VENUE);
                    assert_eq!(response.data.len(), selected.len());
                    break;
                }
                DataEvent::Data(Data::Instrument(_)) => {}
                event => panic!("unexpected request event {event:?}"),
            }
        }

        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn failed_websocket_connect_leaves_instrument_request_admission_open() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut peer = Peer::new().await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        peer.config.private_key = Some("invalid-test-key".into());
        let id = ids()[0];
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        assert!(client.connect().await.is_err());
        assert_eq!(definition(&mut events).await.id, id);
        peer.revision.store(1, Ordering::SeqCst);
        let request_id = UUID4::new();
        client
            .request_instrument(RequestInstrument::new(
                id,
                None,
                None,
                None,
                request_id,
                UnixNanos::from(1),
                None,
            ))
            .unwrap();
        assert_eq!(definition(&mut events).await.id, id);

        let DataEvent::Response(DataResponse::Instrument(response)) = events.recv().await.unwrap()
        else {
            panic!("expected instrument response after failed connect");
        };

        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.client_id, *KALSHI_CLIENT_ID);
        assert!(client.is_disconnected());
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn book_stream_recovers_gaps_without_false_multi_market_or_control_gaps() {
    tokio::time::timeout(Duration::from_secs(12), async {
        let mut peer = Peer::new().await;
        let mut ids = ids();
        ids.reverse();
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            ids.clone(),
            sender.into(),
        )
        .unwrap();
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);

        let members = vec![
            ids[0],
            parse_instrument_id("UNKNOWN-MARKET").unwrap(),
            ids[1],
        ];
        let error = client
            .subscribe_book_deltas(SubscribeBookDeltas::new(
                members,
                BookType::L2_MBP,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                true,
                None,
                None,
            ))
            .unwrap_err()
            .downcast::<SubscriptionError>()
            .unwrap();
        assert_eq!(error.instrument_ids_accepted, ids);
        assert_eq!(error.bar_types_accepted, vec![]);
        assert_eq!(
            error.source.to_string(),
            "instrument is not a selected bootstrap market"
        );

        for id in &ids {
            checker.open(*id);
        }

        client.connect().await.unwrap();
        let lifecycle = peer.command().await;
        assert_eq!(lifecycle["params"]["channels"][0], "market_lifecycle_v2");
        let command = peer.command().await;
        assert_eq!(command["params"]["use_yes_price"], true);
        assert_eq!(
            command["params"]["market_tickers"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let mut snapshots = 0;
        let (bids, asks) = snapshot_oracle();

        while snapshots < 2 {
            if let DataEvent::Data(Data::BookDeltas(batch)) = events.recv().await.unwrap() {
                checker.apply(&batch).unwrap();
                checker
                    .verify(batch.instrument_id, 100, &bids, &asks)
                    .unwrap();
                snapshots += 1;
            }
        }

        let stale = fixture_books()["steps"][0].clone();
        peer.actions
            .send((
                "raw".into(),
                serde_json::json!({
                    "type": "orderbook_delta",
                    "sid": 99,
                    "seq": 1,
                    "msg": stale
                }),
            ))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(events.try_recv().is_err());
        assert!(peer.commands.try_recv().is_err());

        let frame =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["stale_market_update"]
                .clone();
        peer.actions.send(("raw".into(), frame)).unwrap();
        let fixture: Value =
            serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();

        for seq in [2, 1, 4] {
            let mut payload = fixture["steps"][0].clone();
            payload["market_ticker"] = ids[0].symbol.as_str().into();
            peer.actions
                .send((
                    "raw".into(),
                    serde_json::json!({"type":"orderbook_delta","sid":2,"seq":seq,"msg":payload}),
                ))
                .unwrap();
        }

        let DataEvent::Data(Data::BookDeltas(batch)) = events.recv().await.unwrap() else {
            panic!("expected one incremental");
        };

        assert_eq!(batch.sequence, 4);
        checker.apply(&batch).unwrap();
        assert!(peer.commands.try_recv().is_err());
        let mut payload = fixture["steps"][1].clone();
        payload["market_ticker"] = ids[1].symbol.as_str().into();
        peer.actions
            .send((
                "raw".into(),
                serde_json::json!({"type":"orderbook_delta","sid":2,"seq":6,"msg":payload}),
            ))
            .unwrap();
        let mut replacements = 0;
        while replacements < 2 {
            let command = peer.command().await;
            assert_eq!(command["cmd"], "update_subscription");
            assert_eq!(command["params"]["action"], "get_snapshot");

            let DataEvent::Data(Data::BookDeltas(batch)) = events.recv().await.unwrap() else {
                panic!("expected replacement snapshot");
            };

            assert!(RecordFlag::F_SNAPSHOT.matches(batch.flags));
            checker.apply(&batch).unwrap();
            checker
                .verify(batch.instrument_id, 100, &bids, &asks)
                .unwrap();
            replacements += 1;
        }

        client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                ids.clone(),
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(2),
                None,
                None,
            ))
            .unwrap();

        for id in ids {
            checker.close(id);
        }

        let command = peer.command().await;
        assert_eq!(command["cmd"], "unsubscribe");
        assert!(events.try_recv().is_err());
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unsupported_book_options_fail_before_io() {
    let peer = Peer::new().await;
    let (sender, _events) = tokio::sync::mpsc::unbounded_channel();
    let id = ids()[0];
    let mut client = KalshiDataClient::new(
        *KALSHI_CLIENT_ID,
        peer.config.clone(),
        vec![id],
        sender.into(),
    )
    .unwrap();

    for (book_type, depth) in [
        (BookType::L1_MBP, None),
        (BookType::L3_MBO, None),
        (BookType::L2_MBP, NonZeroUsize::new(1)),
    ] {
        let command = SubscribeBookDeltas::new(
            id,
            book_type,
            Some(*KALSHI_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::from(1),
            depth,
            true,
            None,
            None,
        );

        assert_eq!(
            client
                .subscribe_book_deltas(command)
                .unwrap_err()
                .to_string(),
            "Kalshi streams full L2 market-by-price books only"
        );
    }

    let params = Params::new([("unsupported".into(), true.into())].into());

    let command = SubscribeBookDeltas::new(
        id,
        BookType::L2_MBP,
        Some(*KALSHI_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::from(1),
        None,
        true,
        None,
        Some(params.clone()),
    );

    assert_eq!(
        client
            .subscribe_book_deltas(command)
            .unwrap_err()
            .to_string(),
        "Kalshi book subscription parameters are unsupported"
    );

    let request = RequestBookSnapshot::new(
        id,
        NonZeroUsize::new(101),
        None,
        UUID4::new(),
        UnixNanos::from(2),
        None,
    );

    assert_eq!(
        client
            .request_book_snapshot(request)
            .unwrap_err()
            .to_string(),
        "Kalshi REST book depth exceeds 100"
    );

    let request = RequestBookSnapshot::new(
        id,
        None,
        None,
        UUID4::new(),
        UnixNanos::from(2),
        Some(params),
    );

    assert_eq!(
        client
            .request_book_snapshot(request)
            .unwrap_err()
            .to_string(),
        "Kalshi book request parameters are unsupported"
    );
    assert_eq!(peer.requests.load(Ordering::SeqCst), 0);
}

#[rstest::rstest]
#[case::full(None)]
#[case::maximum(NonZeroUsize::new(100))]
#[tokio::test]
async fn rest_book_is_an_exact_correlated_response_without_stream_output(
    #[case] depth: Option<NonZeroUsize>,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let peer = Peer::new().await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let id = ids()[0];
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        let correlation = UUID4::new();
        let response_client = ClientId::new("RESPONSE-CLIENT");

        let request = RequestBookSnapshot::new(
            id,
            depth,
            Some(response_client),
            correlation,
            UnixNanos::from(17),
            Some(Params::default()),
        );

        client.request_book_snapshot(request).unwrap();

        let DataEvent::Response(DataResponse::Book(response)) = events.recv().await.unwrap() else {
            panic!("expected book response");
        };

        let bids: Vec<_> = response.data.bids_as_map(None).into_iter().collect();
        let asks: Vec<_> = response.data.asks_as_map(None).into_iter().collect();
        assert_eq!(response.correlation_id, correlation);
        assert_eq!(response.client_id, response_client);
        assert_eq!(response.instrument_id, id);
        assert_eq!(response.start, None);
        assert_eq!(response.end, None);
        assert_eq!(response.params, Some(Params::default()));
        assert_eq!(response.data.instrument_id, id);
        assert_eq!(response.data.book_type, BookType::L2_MBP);
        assert_eq!(response.data.sequence, 0);
        assert_eq!(response.data.ts_last, response.ts_init);
        assert_eq!(response.data.update_count, 3);
        assert_eq!(
            bids,
            vec![("0.123456".parse().unwrap(), "7.25".parse().unwrap())]
        );
        assert_eq!(
            asks,
            vec![("0.713456".parse().unwrap(), "9.75".parse().unwrap())]
        );
        let book_paths: Vec<_> = peer
            .request_paths
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.contains("/orderbook?"))
            .cloned()
            .collect();
        assert_eq!(
            book_paths,
            vec![format!(
                "/trade-api/v2/markets/{}/orderbook?depth={}",
                id.symbol,
                depth.map_or(0, NonZeroUsize::get)
            )]
        );
        assert!(events.try_recv().is_err());
        assert!(!client.is_connected());
        client.dispose().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn lifecycle_acknowledgment_recovers_a_settlement_missed_during_bootstrap() {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut peer = Peer::new().await;
        peer.confirm.store(false, Ordering::SeqCst);
        let id = ids()[0];
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(*KALSHI_CLIENT_ID, peer.config.clone(), vec![id], sender.into()).unwrap();
        client.subscribe_instrument_status(SubscribeInstrumentStatus::new(id, Some(*KALSHI_CLIENT_ID), None, UUID4::new(), UnixNanos::from(1), None, None)).unwrap();
        client.subscribe_instrument_close(SubscribeInstrumentClose::new(id, Some(*KALSHI_CLIENT_ID), None, UUID4::new(), UnixNanos::from(1), None, None)).unwrap();
        {
            let connect = client.connect(); tokio::pin!(connect);
            let command = tokio::select! {
                result = &mut connect => panic!("connect returned before subscription acknowledgment: {result:?}"),
                command = peer.command() => command,
            };
            assert_eq!(command["params"]["channels"][0], "market_lifecycle_v2");
            let values: Value = serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
            let mut final_market = values["definitions"]["settled"]["market"].clone();
            final_market["event_ticker"] = values["definitions"]["live"]["market"]["event_ticker"].clone();
            final_market["updated_time"] = "2026-10-17T03:45:02.123456Z".into();
            final_market["settlement_value_dollars"] = "0.330001".into();
            final_market["settlement_ts"] = "2026-10-17T03:45:01.987654Z".into();
            *peer.market.lock().unwrap() = Some(final_market);
            peer.actions.send(("raw".into(), serde_json::json!({"type":"subscribed","id":command["id"],"msg":{"channel":"market_lifecycle_v2","sid":1}}))).unwrap();
            connect.await.unwrap();
        }

        let mut closes = Vec::new(); let mut statuses = Vec::new(); let mut definitions = Vec::new();

        while let Ok(event) = events.try_recv() {
            match event {
                DataEvent::Data(Data::InstrumentClose(close)) => closes.push(close),
                DataEvent::Data(Data::InstrumentStatus(status)) => statuses.push(status),
                DataEvent::Data(Data::Instrument(instrument)) => {
                    let InstrumentAny::BinaryOption(instrument) = *instrument else {
                        panic!("unexpected lifecycle output");
                    };

                    definitions.push(instrument);
                }
                _ => panic!("unexpected lifecycle output"),
            }
        }

        assert_eq!(definitions.len(), 2);
        assert_eq!(definitions[1].id, id);
        assert_eq!(definitions[1].info.as_ref().unwrap()["market"]["status"], "finalized");
        assert_eq!(closes.len(), 1);
        assert_eq!(closes[0].instrument_id, id);
        assert_eq!(closes[0].close_price.as_decimal(), "0.330001".parse().unwrap());
        assert_eq!(closes[0].close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(closes[0].ts_event.as_u64(), 1_792_208_701_987_654_000);
        assert_eq!(statuses.last().unwrap().action, MarketStatusAction::NotAvailableForTrading);
        assert_eq!(statuses.last().unwrap().reason.unwrap().as_str(), "finalized");
        client.disconnect().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn lifecycle_notifications_filter_global_markets_and_preserve_newer_stream_state() {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut peer = Peer::new().await;
        let values: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market = values["definitions"]["live"]["market"].clone();
        market["close_time"] = "2099-01-01T00:00:00Z".into();
        market["latest_expiration_time"] = "2099-01-02T00:00:00Z".into();
        *peer.market.lock().unwrap() = Some(market);
        let id = ids()[0];
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        client
            .subscribe_instrument_status(SubscribeInstrumentStatus::new(
                id,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            ))
            .unwrap();
        client.connect().await.unwrap();
        assert_eq!(
            peer.command().await["params"]["channels"][0],
            "market_lifecycle_v2"
        );

        while events.try_recv().is_ok() {}
        let frames: Value =
            serde_json::from_str(include_str!("../../test_data/lifecycle.json")).unwrap();
        let mut ignored = frames["frames"][2].clone();
        ignored["msg"]["market_ticker"] = "KXMVE-EXCLUDED".into();
        ignored["sid"] = 1.into();
        ignored["seq"] = 1.into();
        ignored["type"] = "market_lifecycle_v2".into();
        peer.actions.send(("raw".into(), ignored)).unwrap();
        let mut pause = frames["frames"][2].clone();
        pause["seq"] = 2.into();
        pause["sid"] = 1.into();
        pause["type"] = "market_lifecycle_v2".into();
        peer.actions.send(("raw".into(), pause.clone())).unwrap();

        let DataEvent::Data(Data::Instrument(_)) = events.recv().await.unwrap() else {
            panic!()
        };

        let DataEvent::Data(Data::InstrumentStatus(status)) = events.recv().await.unwrap() else {
            panic!()
        };

        assert_eq!(status.instrument_id, id);
        assert_eq!(status.action, MarketStatusAction::Pause);
        assert_eq!(status.reason.unwrap().as_str(), "inactive");
        assert_eq!(status.is_trading, Some(false));
        assert_eq!(status.is_quoting, Some(false));
        assert_eq!(
            status.ts_event.as_u64(),
            pause["sending_ts_ms"].as_u64().unwrap() * 1_000_000
        );
        peer.actions.send(("raw".into(), pause)).unwrap();
        peer.market.lock().unwrap().as_mut().unwrap()["status"] = "inactive".into();
        let correlation = UUID4::new();
        client
            .request_instrument(RequestInstrument::new(
                id,
                None,
                None,
                None,
                correlation,
                UnixNanos::from(7),
                None,
            ))
            .unwrap();

        match events.recv().await.unwrap() {
            DataEvent::Response(DataResponse::Instrument(response)) => {
                assert_eq!(response.correlation_id, correlation);
            }
            event => panic!("duplicate or stale lifecycle evidence emitted: {event:?}"),
        }

        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest::rstest]
#[case::sequence_gap(false)]
#[case::reconnect(true)]
#[tokio::test]
async fn missed_lifecycle_settlement_recovers_from_rest(#[case] reconnect: bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut peer = Peer::new().await;
        let values: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market = values["definitions"]["live"]["market"].clone();
        market["close_time"] = "2099-01-01T00:00:00Z".into();
        market["latest_expiration_time"] = "2099-01-02T00:00:00Z".into();
        *peer.market.lock().unwrap() = Some(market.clone());
        let id = ids()[0];
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        client
            .subscribe_instrument_close(SubscribeInstrumentClose::new(
                id,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            ))
            .unwrap();
        client.connect().await.unwrap();
        assert_eq!(
            peer.command().await["params"]["channels"][0],
            "market_lifecycle_v2"
        );
        assert!(matches!(
            events.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));
        assert!(events.try_recv().is_err());
        market["status"] = "finalized".into();
        market["updated_time"] = "2026-10-17T03:45:02.123456Z".into();
        market["settlement_value_dollars"] = "0.67".into();
        market["settlement_ts"] = "2026-10-17T03:45:01.987654Z".into();
        *peer.market.lock().unwrap() = Some(market);
        if reconnect {
            peer.actions.send(("close".into(), Value::Null)).unwrap();
            client.stop().unwrap();
            client.connect().await.unwrap();
            assert_eq!(
                peer.command().await["params"]["channels"][0],
                "market_lifecycle_v2"
            );
        } else {
            let frames: Value =
                serde_json::from_str(include_str!("../../test_data/lifecycle.json")).unwrap();

            for seq in [1, 3] {
                let mut frame = frames["frames"][2].clone();
                frame["sid"] = 1.into();
                frame["seq"] = seq.into();
                frame["type"] = "market_lifecycle_v2".into();
                frame["msg"]["market_ticker"] = "UNSELECTED-MARKET".into();
                peer.actions.send(("raw".into(), frame)).unwrap();
            }
        }

        let definition = definition(&mut events).await;

        let DataEvent::Data(Data::InstrumentClose(close)) = events.recv().await.unwrap() else {
            panic!()
        };

        assert_eq!(definition.id, id);
        assert_eq!(
            definition.info.as_ref().unwrap()["market"]["status"],
            "finalized"
        );
        assert_eq!(close.instrument_id, id);
        assert_eq!(close.close_price.as_decimal(), "0.67".parse().unwrap());
        assert_eq!(close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(close.ts_event.as_u64(), 1_792_208_701_987_654_000);
        assert!(events.try_recv().is_err());
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest::rstest]
#[case::late_final(true, false)]
#[case::bounded_non_final(false, false)]
#[case::cancel(false, true)]
#[tokio::test]
async fn settlement_notification_rechecks_rest_until_final_or_bounded_stop(
    #[case] finalize: bool,
    #[case] cancel: bool,
) {
    tokio::time::timeout(Duration::from_secs(12), async {
        let peer = Peer::new().await;
        let values: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market = values["definitions"]["live"]["market"].clone();
        market["status"] = "determined".into();
        market["updated_time"] = "2026-10-17T03:30:00Z".into();
        *peer.market.lock().unwrap() = Some(market.clone());
        let id = ids()[0];
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        client
            .subscribe_instrument_close(SubscribeInstrumentClose::new(
                id,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            ))
            .unwrap();

        if cancel {
            client.subscribe_quotes(quotes(id)).unwrap();
        }

        client.connect().await.unwrap();
        let baseline = peer.requests.load(Ordering::SeqCst);
        assert_eq!(definition(&mut events).await.id, id);
        assert!(events.try_recv().is_err());
        let frames: Value =
            serde_json::from_str(include_str!("../../test_data/lifecycle.json")).unwrap();
        let mut frame = frames["frames"][2].clone();
        frame["type"] = "market_lifecycle_v2".into();
        frame["sid"] = 1.into();
        frame["seq"] = 1.into();
        frame["msg"]["market_ticker"] = id.symbol.as_str().into();
        frame["msg"]["event_type"] = "settled".into();
        peer.actions.send(("raw".into(), frame)).unwrap();
        while peer.request_paths.lock().unwrap().len() < baseline + 3 {
            tokio::task::yield_now().await;
        }

        if cancel {
            peer.emit("ticker", id, false);
            assert!(matches!(
                events.recv().await.unwrap(),
                DataEvent::Data(Data::Quote(_))
            ));
            client.disconnect().await.unwrap();
            assert_eq!(peer.requests.load(Ordering::SeqCst), baseline + 3);
        } else if finalize {
            market["status"] = "finalized".into();
            market["updated_time"] = "2026-10-17T03:45:02.123456Z".into();
            market["settlement_value_dollars"] = "0.67".into();
            market["settlement_ts"] = "2026-10-17T03:45:01.987654Z".into();
            *peer.market.lock().unwrap() = Some(market);
            let final_definition = definition(&mut events).await;

            let DataEvent::Data(Data::InstrumentClose(close)) = events.recv().await.unwrap() else {
                panic!("expected final close")
            };

            assert_eq!(
                final_definition.info.as_ref().unwrap()["market"]["status"],
                "finalized"
            );
            assert_eq!(close.instrument_id, id);
            assert_eq!(close.close_price.as_decimal(), "0.67".parse().unwrap());
            assert_eq!(close.ts_event.as_u64(), 1_792_208_701_987_654_000);
            assert_eq!(close.close_type, InstrumentCloseType::ContractExpired);
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert_eq!(peer.requests.load(Ordering::SeqCst), baseline + 6);
            assert!(events.try_recv().is_err());
            client.disconnect().await.unwrap();
        } else {
            tokio::time::sleep(Duration::from_secs(8)).await;
            assert_eq!(peer.requests.load(Ordering::SeqCst), baseline + 15);
            assert!(events.try_recv().is_err());
            client.disconnect().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn reconnect_fails_explicitly_when_fresh_metadata_is_unavailable() {
    tokio::time::timeout(Duration::from_secs(8), async {
        let peer = Peer::new().await;
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market = fixtures["definitions"]["live"]["market"].clone();
        market["updated_time"] = "2026-10-17T03:45:02Z".into();
        *peer.market.lock().unwrap() = Some(market.clone());
        let id = parse_instrument_id("KXBTC15M-26OCT170345-45").unwrap();
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        client.connect().await.unwrap();
        client.disconnect().await.unwrap();
        assert_eq!(definition(&mut events).await.id, id);
        assert!(events.try_recv().is_err());
        market["updated_time"] = "2026-10-17T03:45:01Z".into();
        *peer.market.lock().unwrap() = Some(market);
        assert_eq!(
            client.connect().await.unwrap_err().to_string(),
            "fresh instrument metadata is unavailable during recovery"
        );
        assert!(client.is_disconnected());
        client
            .subscribe_instrument_status(SubscribeInstrumentStatus::new(
                id,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(2),
                None,
                None,
            ))
            .unwrap();
        assert!(
            events.try_recv().is_err(),
            "failed recovery must not retain metadata for immediate status publication"
        );
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest::rstest]
#[case::quotes(true)]
#[case::trades(false)]
#[tokio::test]
async fn market_batches_preserve_demand_and_report_partial_acceptance(#[case] quotes: bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        let peer = Peer::new().await;
        let selected = ids();
        let first = selected[0];
        let second = selected[1];
        let unknown = parse_instrument_id("MARKET-C").unwrap();
        let members = vec![first, unknown, second, first];
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            selected.clone(),
            sender.into(),
        )
        .unwrap();

        let result = if quotes {
            client.subscribe_trades(trades(second)).unwrap();
            client.subscribe_quotes(SubscribeQuotes::new(
                members,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            ))
        } else {
            client.subscribe_quotes(self::quotes(second)).unwrap();
            client.subscribe_trades(SubscribeTrades::new(
                members,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            ))
        };

        let error = result.unwrap_err().downcast::<SubscriptionError>().unwrap();
        assert_eq!(error.instrument_ids_accepted, selected);
        assert_eq!(error.bar_types_accepted, vec![]);
        assert_eq!(
            error.source.to_string(),
            "instrument is not a selected bootstrap market"
        );

        client.connect().await.unwrap();

        for id in &selected {
            assert_eq!(definition(&mut events).await.id, *id);
        }

        let channel = if quotes { "ticker" } else { "trade" };
        let other_channel = if quotes { "trade" } else { "ticker" };

        for id in &selected {
            peer.emit(channel, *id, false);

            match events.recv().await.unwrap() {
                DataEvent::Data(Data::Quote(tick)) if quotes => assert_eq!(tick.instrument_id, *id),
                DataEvent::Data(Data::Trade(tick)) if !quotes => {
                    assert_eq!(tick.instrument_id, *id);
                }
                event => panic!("unexpected event: {event:?}"),
            }
        }

        peer.emit(other_channel, first, false);
        peer.emit(other_channel, second, false);

        match events.recv().await.unwrap() {
            DataEvent::Data(Data::Trade(tick)) if quotes => assert_eq!(tick.instrument_id, second),
            DataEvent::Data(Data::Quote(tick)) if !quotes => assert_eq!(tick.instrument_id, second),
            event => panic!("unexpected event: {event:?}"),
        }

        if quotes {
            client
                .unsubscribe_quotes(&UnsubscribeQuotes::new(
                    selected.clone(),
                    Some(*KALSHI_CLIENT_ID),
                    None,
                    UUID4::new(),
                    UnixNanos::from(2),
                    None,
                    None,
                ))
                .unwrap();
        } else {
            client
                .unsubscribe_trades(&UnsubscribeTrades::new(
                    selected.clone(),
                    Some(*KALSHI_CLIENT_ID),
                    None,
                    UUID4::new(),
                    UnixNanos::from(2),
                    None,
                    None,
                ))
                .unwrap();
        }

        for id in &selected {
            peer.emit(channel, *id, false);
        }

        // The other channel proves that all preceding frames were consumed without publication
        peer.emit(other_channel, second, false);

        match events.recv().await.unwrap() {
            DataEvent::Data(Data::Trade(tick)) if quotes => assert_eq!(tick.instrument_id, second),
            DataEvent::Data(Data::Quote(tick)) if !quotes => assert_eq!(tick.instrument_id, second),
            event => panic!("unexpected event after unsubscribe: {event:?}"),
        }

        assert!(events.try_recv().is_err());
        client.disconnect().await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest::rstest]
#[case::status_subscribe(false, false)]
#[case::status_unsubscribe(false, true)]
#[case::close_subscribe(true, false)]
#[case::close_unsubscribe(true, true)]
#[tokio::test]
async fn lifecycle_batches_report_partial_acceptance_and_preserve_demand(
    #[case] close: bool,
    #[case] unsubscribe: bool,
) {
    tokio::time::timeout(Duration::from_secs(8), async {
        let peer = Peer::new().await;
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market = fixtures["definitions"]["settled"]["market"].clone();
        market["event_ticker"] = fixtures["definitions"]["live"]["event"]["event_ticker"].clone();
        *peer.market.lock().unwrap() = Some(market);
        let selected = vec![
            parse_instrument_id("MARKET-A").unwrap(),
            parse_instrument_id("MARKET-B").unwrap(),
        ];
        let unknown = parse_instrument_id("MARKET-C").unwrap();
        let members = vec![selected[0], unknown, selected[1]];
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            peer.config.clone(),
            selected.clone(),
            sender.into(),
        )
        .unwrap();

        if unsubscribe {
            if close {
                client
                    .subscribe_instrument_close(SubscribeInstrumentClose::new(
                        selected.clone(),
                        Some(*KALSHI_CLIENT_ID),
                        None,
                        UUID4::new(),
                        UnixNanos::from(1),
                        None,
                        None,
                    ))
                    .unwrap();
            } else {
                client
                    .subscribe_instrument_status(SubscribeInstrumentStatus::new(
                        selected.clone(),
                        Some(*KALSHI_CLIENT_ID),
                        None,
                        UUID4::new(),
                        UnixNanos::from(1),
                        None,
                        None,
                    ))
                    .unwrap();
            }
        }

        let result = match (close, unsubscribe) {
            (false, false) => client.subscribe_instrument_status(SubscribeInstrumentStatus::new(
                members,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            )),
            (true, false) => client.subscribe_instrument_close(SubscribeInstrumentClose::new(
                members,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            )),
            (false, true) => {
                client.unsubscribe_instrument_status(&UnsubscribeInstrumentStatus::new(
                    members,
                    Some(*KALSHI_CLIENT_ID),
                    None,
                    UUID4::new(),
                    UnixNanos::from(2),
                    None,
                    None,
                ))
            }
            (true, true) => client.unsubscribe_instrument_close(&UnsubscribeInstrumentClose::new(
                members,
                Some(*KALSHI_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::from(2),
                None,
                None,
            )),
        };

        let error = result.unwrap_err().downcast::<SubscriptionError>().unwrap();
        client.connect().await.unwrap();
        client.disconnect().await.unwrap();

        let mut received = Vec::new();

        while let Ok(event) = events.try_recv() {
            match event {
                DataEvent::Data(Data::InstrumentStatus(status)) => {
                    assert!(!close);
                    received.push(status.instrument_id);
                }
                DataEvent::Data(Data::InstrumentClose(value)) => {
                    assert!(close);
                    received.push(value.instrument_id);
                }
                DataEvent::Data(Data::Instrument(_)) => {}
                event => panic!("unexpected event: {event:?}"),
            }
        }

        assert_eq!(error.instrument_ids_accepted, selected);
        assert_eq!(error.bar_types_accepted, vec![]);
        assert_eq!(
            error.source.to_string(),
            "instrument is not a selected bootstrap market"
        );
        assert_eq!(received, if unsubscribe { vec![] } else { selected });
    })
    .await
    .unwrap();
}
