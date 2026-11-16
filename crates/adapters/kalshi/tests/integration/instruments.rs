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

use std::{str::FromStr, time::Duration};

use nautilus_common::messages::{
    DataEvent,
    data::{DataResponse, RequestInstrument, RequestInstruments},
};
use nautilus_core::{
    UUID4, UnixNanos, datetime::try_datetime_to_unix_nanos, params::Params, time::AtomicTime,
};
use nautilus_kalshi::{
    common::{
        consts::{KALSHI_CLIENT_ID, KALSHI_VENUE},
        parse::{KalshiPriceGrid, parse_instrument_id, parse_ticker},
    },
    config::KalshiDataClientConfig,
    http::{
        client::KalshiHttpClient,
        models::{Event, Field, FixedPoint, Market, PriceRange},
        parse::{parse_instrument, parse_instrument_close},
    },
    providers::KalshiInstrumentProvider,
};
use nautilus_model::{
    enums::{AssetClass, InstrumentCloseType},
    identifiers::{ClientId, InstrumentId},
    instruments::{
        BinaryOption, Instrument, InstrumentAny,
        tick_scheme::{TickScheme, get_tick_scheme},
    },
    types::{Currency, Price, Quantity},
};
use nautilus_network::dst::{net::TcpListener, task, time};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

#[cfg(all(feature = "simulation", madsim))]
use super::dst as peer;
#[cfg(not(all(feature = "simulation", madsim)))]
use super::http as peer;

#[rstest]
#[case("KXBTC15M-26OCT162345-45")]
#[case("Native-Mixed-Case")]
#[case("Market-YES")]
#[case("Market-NO")]
fn symbol_roundtrip_preserves_native_identity(#[case] ticker: &str) {
    let id = parse_instrument_id(ticker).unwrap();
    assert_eq!(id.to_string(), format!("{ticker}.KALSHI"));
    assert_eq!(parse_ticker(id).unwrap(), ticker);
}

#[rstest]
#[case("")]
#[case("KALSHI.MARKET")]
#[case("MARKET.KALSHI")]
#[case(" MARKET")]
#[case("MARKET/YES")]
#[case("MARKET:yes")]
#[case("Märkét")]
#[case("MARKET-1..25")]
#[case("MARKET-.5")]
#[case("MARKET-1.")]
fn malformed_ticker_is_rejected(#[case] ticker: &str) {
    assert!(parse_instrument_id(ticker).is_err());
}

#[rstest]
fn captured_decimal_strike_ticker_roundtrips() {
    let (market, event) = definition("decimal_strike");
    let id = parse_instrument_id(&market.ticker).unwrap();
    assert_eq!(market.ticker, "KXBTCD-26OCT1717-T91749.99");
    assert_eq!(id.to_string(), "KXBTCD-26OCT1717-T91749.99.KALSHI");
    assert_eq!(parse_ticker(id).unwrap(), market.ticker);
    assert_eq!(event.collateral_return_type.as_ref(), "DIRECNET");
    assert_eq!(
        parse_instrument(&market, &event, UnixNanos::from(1))
            .unwrap_err()
            .to_string(),
        "unsupported market economics or family"
    );
}

#[rstest]
fn foreign_and_ambiguous_instruments_are_rejected() {
    for text in ["MARKET.OTHER", "MARKET.YES.KALSHI"] {
        assert!(parse_ticker(InstrumentId::from_str(text).unwrap()).is_err());
    }

    assert_ne!(
        parse_instrument_id("Native-Mixed").unwrap(),
        parse_instrument_id("NATIVE-MIXED").unwrap()
    );
}

#[rstest]
#[case("whole", "0.01", "0.02", "0.025", 99)]
#[case("subcent", "0.001", "0.123", "0.1234", 999)]
#[case("half", "0.005", "0.125", "0.126", 199)]
#[case("nonuniform", "0.001", "0.105", "0.103", 259)]
#[case("float_boundary", "0.01", "0.09", "0.095", 9)]
fn exact_grid_and_registered_navigation_agree(
    #[case] name: &str,
    #[case] increment: &str,
    #[case] valid: &str,
    #[case] invalid: &str,
    #[case] count: usize,
) {
    let ranges: Vec<PriceRange> =
        serde_json::from_value(fixtures()["grids"][name].clone()).unwrap();
    let grid = KalshiPriceGrid::new(&ranges).unwrap();
    let registered = grid.register().unwrap();

    let TickScheme::Tiered(scheme) = get_tick_scheme(registered.as_str()).unwrap() else {
        panic!("expected tiered scheme")
    };

    assert_eq!(
        grid.increment(),
        Price::from_decimal_dp(decimal(increment), 6).unwrap()
    );
    assert!(grid.contains(decimal(valid)));
    assert!(!grid.contains(decimal(invalid)));
    assert!(!grid.contains(Decimal::ZERO));
    assert!(!grid.contains(Decimal::ONE));
    assert_eq!(scheme.tick_count(), count);

    for tick in scheme.ticks() {
        assert!(grid.contains(tick.as_decimal()));
    }

    assert_eq!(grid.register().unwrap(), registered);
}

#[rstest]
#[case("empty")]
#[case("overlap")]
#[case("negative")]
#[case("zero_step")]
#[case("excess_precision")]
fn invalid_grids_are_rejected(#[case] name: &str) {
    let ranges: Vec<PriceRange> =
        serde_json::from_value(fixtures()["invalid_grids"][name].clone()).unwrap();
    assert!(KalshiPriceGrid::new(&ranges).is_err());
}

#[rstest]
#[case("live")]
#[case("closed")]
#[case("archived")]
#[case("settled")]
fn captured_definitions_preserve_all_instrument_fields(#[case] name: &str) {
    let (market, event) = definition(name);
    let received = UnixNanos::from(1_999_999_999_111_222_333u64);
    let instrument = parse_instrument(&market, &event, received).unwrap();
    assert_eq!(
        instrument.id.to_string(),
        format!("{}.KALSHI", market.ticker)
    );
    assert_eq!(instrument.raw_symbol.as_str(), market.ticker);
    assert_eq!(instrument.asset_class, AssetClass::Alternative);
    assert_eq!(instrument.currency, Currency::USD());
    assert_eq!(
        instrument.activation_ns.as_u64(),
        u64::try_from(market.open_time.as_nanosecond()).unwrap()
    );
    assert_eq!(
        instrument.expiration_ns.as_u64(),
        u64::try_from(market.latest_expiration_time.as_nanosecond()).unwrap()
    );
    assert_eq!(instrument.price_precision, 6);
    assert_eq!(instrument.size_precision, 2);
    assert_eq!(
        instrument.price_increment,
        KalshiPriceGrid::new(&market.price_ranges)
            .unwrap()
            .increment()
    );
    assert_eq!(instrument.size_increment, Quantity::from("0.01"));
    assert_eq!(instrument.min_quantity, Some(Quantity::from("0.01")));
    assert_eq!(instrument.event_id.unwrap().as_str(), market.event_ticker);
    assert_eq!(instrument.outcome.unwrap().as_str(), "YES");
    assert_eq!(
        instrument.description.unwrap().as_str(),
        market.yes_sub_title
    );
    assert_eq!(instrument.max_quantity, None);
    assert_eq!(instrument.min_notional, None);
    assert_eq!(instrument.max_notional, None);
    assert_eq!(instrument.min_price, None);
    assert_eq!(instrument.max_price, None);
    assert_eq!(instrument.next_bid_price(0.0, 0), None);
    assert_eq!(instrument.margin_init, Decimal::ZERO);
    assert_eq!(instrument.margin_maint, Decimal::ZERO);
    assert_eq!(
        instrument.tick_scheme,
        Some(
            KalshiPriceGrid::new(&market.price_ranges)
                .unwrap()
                .register()
                .unwrap()
        )
    );
    let info = instrument.info.as_ref().unwrap();
    assert_eq!(info.len(), 2);
    assert_eq!(info["market"], serde_json::to_value(&market).unwrap());
    let mut retained_event = event;
    retained_event.markets = Field::Missing;
    assert_eq!(
        info["event"],
        serde_json::to_value(&retained_event).unwrap()
    );

    let expected_event = match market.updated_time {
        Field::Value(t) => UnixNanos::from(u64::try_from(t.as_nanosecond()).unwrap()),
        _ => received,
    };

    assert_eq!(instrument.ts_event, expected_event);
    assert_eq!(instrument.ts_init, received);
    let persisted: BinaryOption =
        serde_json::from_value(serde_json::to_value(&instrument).unwrap()).unwrap();
    let retained: Market =
        serde_json::from_value(persisted.info.unwrap()["market"].clone()).unwrap();
    assert_eq!(
        KalshiPriceGrid::new(&retained.price_ranges)
            .unwrap()
            .register()
            .unwrap(),
        instrument.tick_scheme.unwrap()
    );
}

#[rstest]
fn grid_changes_keep_storage_precision_and_old_response_prices() {
    let (mut market, event) = definition("live");
    let first = parse_instrument(&market, &event, UnixNanos::from(11)).unwrap();
    market.price_ranges = serde_json::from_value(fixtures()["grids"]["whole"].clone()).unwrap();
    market.price_level_structure = "unrecognized_label".parse().unwrap();
    market.updated_time = Field::Missing;
    market.settlement_floor_dollars = Field::Value(FixedPoint(Decimal::ZERO));
    let second = parse_instrument(&market, &event, UnixNanos::from(22)).unwrap();
    assert_eq!(first.price_precision, 6);
    assert_eq!(second.price_precision, 6);
    assert_ne!(first.tick_scheme, second.tick_scheme);
    assert_eq!(second.ts_event, UnixNanos::from(22));
    assert_eq!(
        Price::from_decimal_dp(decimal("0.123456"), second.price_precision)
            .unwrap()
            .as_decimal(),
        decimal("0.123456")
    );
}

#[rstest]
#[case("scalar")]
#[case("notional")]
#[case("floor")]
#[case("bounds")]
#[case("netting")]
#[case("unknown_netting")]
#[case("mve_collection")]
#[case("mve_legs")]
#[case("event_mismatch")]
#[case("shard_mismatch")]
#[case("missing_shard")]
#[case("bad_ticker")]
fn unsupported_and_inconsistent_metadata_is_rejected(#[case] mutation: &str) {
    let mut value = fixtures()["definitions"]["live"].clone();
    match mutation {
        "scalar" => value["market"]["market_type"] = "scalar".into(),
        "notional" => value["market"]["notional_value_dollars"] = "2.0000".into(),
        "floor" => value["market"]["settlement_floor_dollars"] = "0.1000".into(),
        "bounds" => value["market"]["settlement_bounds_type"] = "custom".into(),
        "netting" => value["event"]["collateral_return_type"] = "DIRECNET".into(),
        "unknown_netting" => value["event"]["collateral_return_type"] = "future".into(),
        "mve_collection" => value["market"]["mve_collection_ticker"] = "COLLECTION".into(),
        "mve_legs" => value["market"]["mve_selected_legs"] = fixtures()["selected_legs"].clone(),
        "event_mismatch" => value["event"]["event_ticker"] = "OTHER-EVENT".into(),
        "shard_mismatch" => value["event"]["exchange_index"] = 7.into(),
        "missing_shard" => {
            value["market"]
                .as_object_mut()
                .unwrap()
                .remove("exchange_index");
        }
        "bad_ticker" => value["market"]["ticker"] = "MARKET.KALSHI".into(),
        _ => unreachable!(),
    }

    let market: Market = serde_json::from_value(value["market"].clone()).unwrap();
    let event: Event = serde_json::from_value(value["event"].clone()).unwrap();
    assert!(parse_instrument(&market, &event, UnixNanos::from(1)).is_err());
}

#[rstest]
fn captured_netting_event_is_rejected() {
    let (market, event) = definition("unsupported");
    assert_eq!(
        parse_instrument(&market, &event, UnixNanos::from(1))
            .unwrap_err()
            .to_string(),
        "unsupported market economics or family"
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn consecutive_requests_emit_fresh_correlated_definitions() {
    Box::pin(time::timeout(Duration::from_secs(10), async {
        let values = fixtures();
        let original = values["definitions"]["live"].clone();
        let ticker = original["market"]["ticker"].as_str().unwrap().to_owned();
        let (listener, address) = peer::bind_peer(18380).await;
        let ticker_peer = ticker.clone();
        let event_peer = original["event"]["event_ticker"].as_str().unwrap().to_owned();
        let mut series_expected = values["series"]["series"].clone();
        series_expected["fee_multiplier"] = "1".into();
        series_expected.as_object_mut().unwrap().remove("product_metadata");

        let server = task::spawn(async move {
            let mut requests = Vec::new();

            for round in 0..2 {
                let mut market = original["market"].clone();
                market["yes_sub_title"] = format!("Fresh outcome {round}").into();
                market["price_ranges"] = values["grids"][if round == 0 {"subcent"} else {"whole"}].clone();
                for body in [serde_json::json!({"market": market}), serde_json::json!({"event": original["event"], "markets": []}), values["series"].clone()] {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let request = peer::read_headers(&mut stream).await;
                    requests.push(request.lines().next().unwrap().to_owned());
                    let body = serde_json::to_string(&body).unwrap();
                    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
                }
            }

            assert_eq!(requests[0], format!("GET /trade-api/v2/markets/{ticker_peer} HTTP/1.1"));
            assert_eq!(requests[0],requests[3]);

            for offset in [0, 3] {
                assert_eq!(requests[offset + 1], format!("GET /trade-api/v2/events/{event_peer}?exclude_markets=true HTTP/1.1"));
                assert_eq!(requests[offset + 2], "GET /trade-api/v2/series/KXBTC15M HTTP/1.1");
            }

            requests
        });

        let config = KalshiDataClientConfig { base_url_http: Some(format!("http://{address}/trade-api/v2").into()), ..Default::default() };
        let http = KalshiHttpClient::new(&config).unwrap();
        let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(1_999_999_999_000_000_001u64))));
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let provider = KalshiInstrumentProvider::new(http, clock, sender.into());
        let id = parse_instrument_id(&ticker).unwrap();
        let mut schemes = Vec::new();

        for round in 0..2 {
            let now = UnixNanos::from(1_999_999_999_000_000_001u64 + round);
            clock.set_time(now);

            let params = Params::new([
                ("exchange_index".into(), 2.into()),
                ("series_ticker".into(), "KXBTC15M".into()),
            ].into());

            let request_id = UUID4::new();
            let request = RequestInstrument::new(id, None, None, Some(ClientId::new("DISCOVERY-CLIENT")), request_id, UnixNanos::from(7), Some(params.clone()));
            provider.request_instrument(request).await.unwrap();
            let DataEvent::Response(DataResponse::Instrument(response)) = receiver.try_recv().unwrap() else { panic!("expected instrument response") };
            let InstrumentAny::BinaryOption(instrument) = response.data else { panic!("expected binary option") };
            assert_eq!(response.correlation_id, request_id);
            assert_eq!(response.client_id, ClientId::new("DISCOVERY-CLIENT"));
            assert_eq!(response.instrument_id, id);
            assert_eq!(response.start, None); assert_eq!(response.end, None);
            assert_eq!(response.params, Some(params));
            assert_eq!(response.ts_init, now);
            assert_eq!(instrument.ts_init, now);
            assert_eq!(instrument.description.unwrap().as_str(), format!("Fresh outcome {round}"));
            assert_eq!(instrument.price_increment.as_decimal(), decimal(if round==0 {"0.001"} else {"0.01"}));
            assert_eq!(instrument.price_precision, 6);
            assert_eq!(instrument.info.as_ref().unwrap()["series"], series_expected);
            schemes.push(instrument.tick_scheme);
        }

        assert_ne!(schemes[0],schemes[1]);
        assert!(receiver.try_recv().is_err());
        assert_eq!(server.await.unwrap().len(),6);
    })).await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn paginated_bulk_requests_refresh_and_apply_filters() {
    Box::pin(time::timeout(Duration::from_secs(10), async {
        let values = fixtures();
        let original = values["definitions"]["live"].clone();
        let ticker = original["market"]["ticker"].as_str().unwrap().to_owned();
        let event_ticker = original["market"]["event_ticker"].as_str().unwrap().to_owned();
        let (listener, address) = peer::bind_peer(18381).await;
        let event_peer = event_ticker.clone();
        let ticker_peer = ticker.clone();

        let server = task::spawn(async move {
            let mut requests = Vec::new();

            for round in 0..3 {
                let mut market = original["market"].clone();
                market["yes_sub_title"] = format!("Bulk outcome {round}").into();
                if round == 2 { market["status"] = "finalized".into(); }
                let mut scalar = market.clone(); scalar["ticker"] = "EXCLUDED-SCALAR".into(); scalar["market_type"] = "scalar".into();
                let mut other = market.clone(); other["ticker"] = "OTHER-SHARD".into(); other["exchange_index"] = 7.into();

                for body in [serde_json::json!({"markets":[market,scalar],"cursor":"next"}),serde_json::json!({"markets":[other],"cursor":""}),serde_json::json!({"event":original["event"],"markets":[]}),values["series"].clone()] {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    requests.push(peer::read_headers(&mut stream).await.lines().next().unwrap().to_owned());
                    let body = serde_json::to_string(&body).unwrap();
                    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
                }
            }

            for round in 0..3 {
                let first = &requests[round*4]; let second = &requests[round*4+1];
                if round < 2 {
                    assert!(first.contains("event_ticker="));
                    assert!(first.contains(&event_peer));
                    assert!(first.contains("series_ticker=KXBTC15M"));
                    assert!(first.contains("status=open"));
                    assert!(first.contains("mve_filter=exclude"));
                } else {
                    assert!(first.starts_with("GET /trade-api/v2/historical/markets?"));
                    assert!(!first.contains("event_ticker="));
                    assert!(!first.contains("series_ticker="));
                    assert!(!first.contains("mve_filter="));
                }

                assert!(!first.contains("cursor="));
                assert!(second.contains("cursor=next"));
                assert!(first.contains(&format!("tickers={ticker_peer}%2COTHER-TICKER")));
                assert_eq!(requests[round*4+2], format!("GET /trade-api/v2/events/{event_peer}?exclude_markets=true HTTP/1.1"));
                assert_eq!(requests[round*4+3], "GET /trade-api/v2/series/KXBTC15M HTTP/1.1");
            }

            requests
        });

        let config = KalshiDataClientConfig {base_url_http:Some(format!("http://{address}/trade-api/v2").into()),..Default::default()};
        let clock = Box::leak(Box::new(AtomicTime::new(false,UnixNanos::from(1_999_999_999_000_000_000u64))));
        let (sender,mut receiver)=tokio::sync::mpsc::unbounded_channel();
        let provider=KalshiInstrumentProvider::new(KalshiHttpClient::new(&config).unwrap(),clock,sender.into());
        let start=jiff::Timestamp::from_str("2026-10-17T00:00:00Z").unwrap();
        let end=jiff::Timestamp::from_str("2026-10-18T00:00:00Z").unwrap();

        for round in 0..3 {
            let params = Params::new([
                ("event_ticker".into(), event_ticker.clone().into()),
                ("series_ticker".into(), "KXBTC15M".into()),
                ("exchange_index".into(), 2.into()),
                ("status".into(), if round == 2 { "settled" } else { "open" }.into()),
                ("historical".into(), (round == 2).into()),
                ("tickers".into(), format!("{ticker},OTHER-TICKER").into()),
            ].into());

            let request_id=UUID4::new();
            provider.request_instruments(RequestInstruments::new(Some(start),Some(end),Some(*KALSHI_CLIENT_ID),Some(*KALSHI_VENUE),request_id,UnixNanos::from(1),Some(params.clone()))).await.unwrap();
            let DataEvent::Response(DataResponse::Instruments(response))=receiver.try_recv().unwrap() else {panic!("expected instruments response")};
            assert_eq!(response.correlation_id,request_id);assert_eq!(response.client_id,*KALSHI_CLIENT_ID);assert_eq!(response.venue,*KALSHI_VENUE);
            assert_eq!(response.start,Some(UnixNanos::from(1_792_195_200_000_000_000u64)));assert_eq!(response.end,Some(UnixNanos::from(1_792_281_600_000_000_000u64)));
            assert_eq!(response.ts_init,clock.get_time_ns());assert_eq!(response.params,Some(params));assert_eq!(response.data.len(),1);
            let InstrumentAny::BinaryOption(instrument)=&response.data[0] else {panic!("expected binary option")};
            assert_eq!(instrument.id,parse_instrument_id(&ticker).unwrap());assert_eq!(instrument.description.unwrap().as_str(),format!("Bulk outcome {round}"));
        }

        assert!(receiver.try_recv().is_err());assert_eq!(server.await.unwrap().len(),12);
    })).await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn archive_lookup_uses_explicit_selection_or_live_404() {
    Box::pin(time::timeout(Duration::from_secs(10), async {
        for historical in [false, true] {
            let values = fixtures();
            let definition = &values["definitions"]["live"];
            let ticker = definition["market"]["ticker"].as_str().unwrap().to_owned();
            let (listener, address) = peer::bind_peer(18382).await;
            let mut responses = Vec::new();
            if !historical {
                responses.push((404, Value::Null));
            }

            responses.extend([
                (200, serde_json::json!({"market":definition["market"]})),
                (
                    200,
                    serde_json::json!({"event":definition["event"],"markets":[]}),
                ),
                (200, values["series"].clone()),
            ]);

            let server = task::spawn(serve_responses(listener, responses));

            let config = KalshiDataClientConfig {
                base_url_http: Some(format!("http://{address}/trade-api/v2").into()),
                ..Default::default()
            };

            let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(31))));
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

            let provider = KalshiInstrumentProvider::new(
                KalshiHttpClient::new(&config).unwrap(),
                clock,
                sender.into(),
            );

            let params = Params::new([("historical".into(), historical.into())].into());
            let id = parse_instrument_id(&ticker).unwrap();
            let request_id = UUID4::new();
            provider
                .request_instrument(RequestInstrument::new(
                    id,
                    None,
                    None,
                    None,
                    request_id,
                    UnixNanos::from(17),
                    Some(params.clone()),
                ))
                .await
                .unwrap();

            let DataEvent::Response(DataResponse::Instrument(response)) =
                receiver.try_recv().unwrap()
            else {
                panic!("expected archive response")
            };

            assert_eq!(response.correlation_id, request_id);
            assert_eq!(response.instrument_id, id);
            assert_eq!(response.params, Some(params));
            assert_eq!(response.ts_init, UnixNanos::from(31));
            let requests = server.await.unwrap();
            let offset = usize::from(!historical);
            if !historical {
                assert_eq!(
                    requests[0],
                    format!("GET /trade-api/v2/markets/{ticker} HTTP/1.1")
                );
            }

            assert_eq!(
                requests[offset],
                format!("GET /trade-api/v2/historical/markets/{ticker} HTTP/1.1")
            );
        }
    }))
    .await
    .unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn failed_requests_never_emit_partial_or_mismatched_definitions() {
    Box::pin(time::timeout(Duration::from_secs(10), async {
        for scenario in [
            "budget",
            "identity",
            "filters",
            "created_filter",
            "updated_filter",
            "close_filter",
            "settled_filter",
            "http",
        ] {
            let values = fixtures();
            let definition = &values["definitions"]["live"];
            let ticker = definition["market"]["ticker"].as_str().unwrap().to_owned();
            let mut market = definition["market"].clone();

            let (responses, expected) = match scenario {
                "budget" => (
                    vec![(200, serde_json::json!({"markets":[market],"cursor":"next"}))],
                    "page budget exhausted",
                ),
                "identity" => {
                    market["ticker"] = "WRONG-MARKET".into();

                    (
                        vec![(200, serde_json::json!({"market":market}))],
                        "requested market identity mismatch",
                    )
                }
                "filters" | "created_filter" | "updated_filter" | "close_filter"
                | "settled_filter" => (
                    vec![
                        (200, serde_json::json!({"market":market})),
                        (
                            200,
                            serde_json::json!({"event":definition["event"],"markets":[]}),
                        ),
                        (200, values["series"].clone()),
                    ],
                    "does not satisfy request filters",
                ),
                "http" => (vec![(400, Value::Null)], "HTTP 400"),
                _ => unreachable!(),
            };

            let (listener, address) = peer::bind_peer(18383).await;
            let server = task::spawn(serve_responses(listener, responses));

            let config = KalshiDataClientConfig {
                base_url_http: Some(format!("http://{address}/trade-api/v2").into()),
                ..Default::default()
            };

            let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(41))));
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

            let provider = KalshiInstrumentProvider::new(
                KalshiHttpClient::new(&config).unwrap(),
                clock,
                sender.into(),
            );

            let mut params = vec![
                ("max_pages".into(), 1.into()),
                ("series_ticker".into(), "WRONG-SERIES".into()),
            ];

            if scenario.ends_with("_filter") {
                params.push(("series_ticker".into(), "KXBTC15M".into()));

                let key = match scenario {
                    "created_filter" => "min_created_ts",
                    "updated_filter" => "min_updated_ts",
                    "close_filter" => "min_close_ts",
                    "settled_filter" => "min_settled_ts",
                    _ => unreachable!(),
                };

                params.push((key.into(), 2_000_000_000i64.into()));
            }

            let params = Params::new(params.into_iter().collect());

            let result = if scenario == "budget" {
                provider
                    .request_instruments(RequestInstruments::new(
                        None,
                        None,
                        None,
                        Some(*KALSHI_VENUE),
                        UUID4::new(),
                        UnixNanos::from(1),
                        Some(params),
                    ))
                    .await
            } else {
                provider
                    .request_instrument(RequestInstrument::new(
                        parse_instrument_id(&ticker).unwrap(),
                        None,
                        None,
                        None,
                        UUID4::new(),
                        UnixNanos::from(1),
                        Some(params),
                    ))
                    .await
            };

            assert!(
                result.unwrap_err().to_string().contains(expected),
                "scenario {scenario}"
            );
            assert!(
                receiver.try_recv().is_err(),
                "unexpected emitted response for {scenario}"
            );
            server.await.unwrap();
        }
    }))
    .await
    .unwrap();
}

#[rstest]
#[case("venue_equal", false, 0, 0, true)]
#[case("before", false, -1, -1, false)]
#[case("after", false, 1, 1, false)]
#[case("inverted", false, 1, -1, false)]
#[case("fallback_equal", true, 0, 0, true)]
#[case("fallback_before", true, -1, -1, false)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn request_time_windows_are_inclusive_and_use_timestamp_fallback(
    #[case] name: &str,
    #[case] missing: bool,
    #[case] start_delta: i128,
    #[case] end_delta: i128,
    #[case] included: bool,
    #[values(false, true)] bulk: bool,
) {
    Box::pin(time::timeout(Duration::from_secs(10), async {
        let values = fixtures();
        let definition = &values["definitions"]["live"];
        let mut market = definition["market"].clone();
        let now = UnixNanos::from(1_999_999_999_000_000_001u64);
        let timestamp = if missing {
            i128::from(now.as_u64())
        } else {
            jiff::Timestamp::from_str(market["updated_time"].as_str().unwrap())
                .unwrap()
                .as_nanosecond()
        };

        if missing {
            market.as_object_mut().unwrap().remove("updated_time");
        }

        let start = jiff::Timestamp::from_nanosecond(timestamp + start_delta).unwrap();
        let end = jiff::Timestamp::from_nanosecond(timestamp + end_delta).unwrap();
        let id = parse_instrument_id(market["ticker"].as_str().unwrap()).unwrap();

        let first_body = if bulk {
            serde_json::json!({"markets":[market],"cursor":""})
        } else {
            serde_json::json!({"market":market})
        };

        let mut responses = vec![
            (200, first_body),
            (
                200,
                serde_json::json!({"event":definition["event"],"markets":[]}),
            ),
            (200, values["series"].clone()),
        ];

        if name == "inverted" {
            responses.clear();
        }

        let (listener, address) = peer::bind_peer(18384).await;
        let server = task::spawn(serve_responses(listener, responses));

        let config = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{address}/trade-api/v2").into()),
            ..Default::default()
        };

        let clock = Box::leak(Box::new(AtomicTime::new(false, now)));
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let provider = KalshiInstrumentProvider::new(
            KalshiHttpClient::new(&config).unwrap(),
            clock,
            sender.into(),
        );

        let request_id = UUID4::new();

        let result = if bulk {
            provider
                .request_instruments(RequestInstruments::new(
                    Some(start),
                    Some(end),
                    None,
                    Some(*KALSHI_VENUE),
                    request_id,
                    UnixNanos::from(1),
                    None,
                ))
                .await
        } else {
            provider
                .request_instrument(RequestInstrument::new(
                    id,
                    Some(start),
                    Some(end),
                    None,
                    request_id,
                    UnixNanos::from(1),
                    None,
                ))
                .await
        };

        let error = match (bulk, name, included) {
            (_, "inverted", _) => Some("invalid update interval"),
            (false, _, false) => Some("definition outside requested update interval"),
            _ => None,
        };

        if let Some(expected) = error {
            assert_eq!(result.unwrap_err().to_string(), expected, "{name}");
            assert!(
                receiver.try_recv().is_err(),
                "unexpected response for {name}"
            );
            server.await.unwrap();
            return;
        }

        result.unwrap();

        let instruments = match receiver.try_recv().unwrap() {
            DataEvent::Response(DataResponse::Instrument(response)) => {
                assert_eq!(response.correlation_id, request_id);
                assert_eq!(response.client_id, *KALSHI_CLIENT_ID);
                assert_eq!(response.instrument_id, id);
                assert_eq!(
                    response.start,
                    Some(UnixNanos::from(
                        u64::try_from(start.as_nanosecond()).unwrap()
                    ))
                );
                assert_eq!(
                    response.end,
                    Some(UnixNanos::from(u64::try_from(end.as_nanosecond()).unwrap()))
                );
                assert_eq!(response.ts_init, now);
                assert_eq!(response.params, None);
                vec![response.data]
            }
            DataEvent::Response(DataResponse::Instruments(response)) => {
                assert_eq!(response.correlation_id, request_id);
                assert_eq!(response.client_id, *KALSHI_CLIENT_ID);
                assert_eq!(response.venue, *KALSHI_VENUE);
                assert_eq!(
                    response.start,
                    Some(UnixNanos::from(
                        u64::try_from(start.as_nanosecond()).unwrap()
                    ))
                );
                assert_eq!(
                    response.end,
                    Some(UnixNanos::from(u64::try_from(end.as_nanosecond()).unwrap()))
                );
                assert_eq!(response.ts_init, now);
                assert_eq!(response.params, None);
                response.data
            }
            _ => panic!("expected definition response"),
        };

        assert_eq!(instruments.len(), usize::from(included), "{name}");

        for instrument in instruments {
            let InstrumentAny::BinaryOption(instrument) = instrument else {
                panic!("expected binary option")
            };

            assert_eq!(instrument.id, id);
            assert_eq!(
                instrument.ts_event,
                UnixNanos::from(u64::try_from(timestamp).unwrap())
            );
            assert_eq!(instrument.ts_init, now);
        }

        assert!(receiver.try_recv().is_err());
        server.await.unwrap();
    }))
    .await
    .unwrap();
}

#[rstest]
#[case("1969-12-31T23:59:59Z")]
#[case("3000-01-01T00:00:00Z")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unrepresentable_request_bounds_fail_before_transport(
    #[case] timestamp: &str,
    #[values(false, true)] start_bound: bool,
    #[values(false, true)] bulk: bool,
) {
    let timestamp = jiff::Timestamp::from_str(timestamp).unwrap();
    let (start, end) = if start_bound {
        (Some(timestamp), None)
    } else {
        (None, Some(timestamp))
    };

    let config = KalshiDataClientConfig {
        base_url_http: Some("http://127.0.0.1:1/trade-api/v2".into()),
        ..Default::default()
    };

    let http = KalshiHttpClient::new(&config).unwrap();
    http.cancel();
    let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(31))));
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let provider = KalshiInstrumentProvider::new(http, clock, sender.into());
    let result = if bulk {
        provider
            .request_instruments(RequestInstruments::new(
                start,
                end,
                None,
                Some(*KALSHI_VENUE),
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .await
    } else {
        provider
            .request_instrument(RequestInstrument::new(
                parse_instrument_id("VALID-MARKET").unwrap(),
                start,
                end,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .await
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        if start_bound {
            "invalid request start bound"
        } else {
            "invalid request end bound"
        }
    );
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case("event")]
#[case("series")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn context_identity_mismatches_never_emit(
    #[case] mismatch: &str,
    #[values(false, true)] bulk: bool,
) {
    Box::pin(time::timeout(Duration::from_secs(10), async {
        let values = fixtures();
        let definition = &values["definitions"]["live"];
        let ticker = definition["market"]["ticker"].as_str().unwrap();
        let event_ticker = definition["market"]["event_ticker"].as_str().unwrap();
        let mut event = definition["event"].clone();
        let mut series = values["series"].clone();

        if mismatch == "event" {
            event["event_ticker"] = "WRONG-EVENT".into();
        } else {
            series["series"]["ticker"] = "WRONG-SERIES".into();
        }

        let first = if bulk {
            serde_json::json!({"markets":[definition["market"]],"cursor":""})
        } else {
            serde_json::json!({"market":definition["market"]})
        };

        let mut responses = vec![
            (200, first),
            (200, serde_json::json!({"event":event,"markets":[]})),
        ];

        if !bulk || mismatch == "series" {
            responses.push((200, series));
        }

        let (listener, address) = peer::bind_peer(18385).await;
        let server = task::spawn(serve_responses(listener, responses));

        let config = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{address}/trade-api/v2").into()),
            ..Default::default()
        };

        let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(31))));
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let provider = KalshiInstrumentProvider::new(
            KalshiHttpClient::new(&config).unwrap(),
            clock,
            sender.into(),
        );

        let result = if bulk {
            provider
                .request_instruments(RequestInstruments::new(
                    None,
                    None,
                    None,
                    Some(*KALSHI_VENUE),
                    UUID4::new(),
                    UnixNanos::from(1),
                    None,
                ))
                .await
        } else {
            provider
                .request_instrument(RequestInstrument::new(
                    parse_instrument_id(ticker).unwrap(),
                    None,
                    None,
                    None,
                    UUID4::new(),
                    UnixNanos::from(1),
                    None,
                ))
                .await
        };

        let expected = match (bulk, mismatch) {
            (_, "series") => "requested series identity mismatch",
            (true, _) => "requested event identity mismatch",
            (false, _) => "market and event identity mismatch",
        };

        assert_eq!(result.unwrap_err().to_string(), expected);
        assert!(receiver.try_recv().is_err());
        let requests = server.await.unwrap();
        assert_eq!(
            requests[1],
            format!("GET /trade-api/v2/events/{event_ticker}?exclude_markets=true HTTP/1.1")
        );

        if requests.len() == 3 {
            assert_eq!(requests[2], "GET /trade-api/v2/series/KXBTC15M HTTP/1.1");
        }
    }))
    .await
    .unwrap();
}

#[rstest]
#[case("venue", "instrument request venue must be KALSHI")]
#[case("cursor", "instrument discovery must start at the first page")]
#[case("tickers", "invalid Kalshi ticker")]
#[case("interval", "invalid update interval")]
#[case("interval_single", "invalid update interval")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_discovery_requests_fail_before_transport(
    #[case] scenario: &str,
    #[case] expected: &str,
) {
    let config = KalshiDataClientConfig {
        base_url_http: Some("http://127.0.0.1:1/trade-api/v2".into()),
        ..Default::default()
    };

    let http = KalshiHttpClient::new(&config).unwrap();
    http.cancel();
    let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(31))));
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let provider = KalshiInstrumentProvider::new(http, clock, sender.into());
    let mut params = Vec::new();

    match scenario {
        "cursor" => {
            params.push(("cursor".into(), "continuation".into()));
        }
        "tickers" => {
            params.push(("tickers".into(), "VALID,BAD..TICKER".into()));
        }
        _ => {}
    }

    let params = Params::new(params.into_iter().collect());

    let (start, end) = if scenario.starts_with("interval") {
        (
            Some(jiff::Timestamp::from_nanosecond(2).unwrap()),
            Some(jiff::Timestamp::from_nanosecond(1).unwrap()),
        )
    } else {
        (None, None)
    };

    let venue = if scenario == "venue" {
        "OTHER".into()
    } else {
        *KALSHI_VENUE
    };

    let result = if scenario == "interval_single" {
        provider
            .request_instrument(RequestInstrument::new(
                parse_instrument_id("VALID-MARKET").unwrap(),
                start,
                end,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                Some(params),
            ))
            .await
    } else {
        provider
            .request_instruments(RequestInstruments::new(
                start,
                end,
                None,
                Some(venue),
                UUID4::new(),
                UnixNanos::from(1),
                Some(params),
            ))
            .await
    };

    assert_eq!(result.unwrap_err().to_string(), expected);
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case("tickers")]
#[case("event_ticker")]
#[case("series_ticker")]
#[case("combined")]
#[case("none")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn archived_discovery_uses_one_wire_filter(#[case] selection: &str) {
    Box::pin(time::timeout(Duration::from_secs(10), async {
        let values = fixtures();
        let definition = &values["definitions"]["live"];
        let ticker = definition["market"]["ticker"].as_str().unwrap();
        let event = definition["event"]["event_ticker"].as_str().unwrap();
        let (listener, address) = peer::bind_peer(18386).await;
        let responses = vec![
            (
                200,
                serde_json::json!({"markets":[definition["market"]],"cursor":""}),
            ),
            (
                200,
                serde_json::json!({"event":definition["event"],"markets":[]}),
            ),
            (200, values["series"].clone()),
        ];

        let server = task::spawn(serve_responses(listener, responses));

        let config = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{address}/trade-api/v2").into()),
            ..Default::default()
        };

        let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(31))));
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let provider = KalshiInstrumentProvider::new(
            KalshiHttpClient::new(&config).unwrap(),
            clock,
            sender.into(),
        );

        let mut params = vec![("historical".into(), true.into())];

        for (field, value) in [
            ("tickers", ticker),
            ("event_ticker", event),
            ("series_ticker", "KXBTC15M"),
        ] {
            if selection == field || selection == "combined" {
                params.push((field.into(), value.into()));
            }
        }

        let params = Params::new(params.into_iter().collect());

        let request_id = UUID4::new();
        provider
            .request_instruments(RequestInstruments::new(
                None,
                None,
                None,
                Some(*KALSHI_VENUE),
                request_id,
                UnixNanos::from(1),
                Some(params.clone()),
            ))
            .await
            .unwrap();

        let DataEvent::Response(DataResponse::Instruments(response)) = receiver.try_recv().unwrap()
        else {
            panic!("expected archived instruments response")
        };

        let (field, value) = match selection {
            "tickers" | "combined" => ("tickers", ticker),
            "event_ticker" => ("event_ticker", event),
            "series_ticker" => ("series_ticker", "KXBTC15M"),
            _ => ("mve_filter", "exclude"),
        };

        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.params, Some(params));
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id(), parse_instrument_id(ticker).unwrap());
        let requests = server.await.unwrap();
        assert_eq!(
            requests[0],
            format!("GET /trade-api/v2/historical/markets?{field}={value} HTTP/1.1")
        );
        assert_eq!(
            requests[1],
            format!("GET /trade-api/v2/events/{event}?exclude_markets=true HTTP/1.1")
        );
        assert_eq!(requests[2], "GET /trade-api/v2/series/KXBTC15M HTTP/1.1");
    }))
    .await
    .unwrap();
}

#[rstest]
#[case::created_then_queryable(true)]
#[case::unavailable_archive_is_not_closure(false)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn newly_created_market_lookup_retries_with_a_finite_budget(#[case] available: bool) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let values = fixtures();
        let definition = &values["definitions"]["live"];
        let id = parse_instrument_id(definition["market"]["ticker"].as_str().unwrap()).unwrap();
        let (listener, address) = peer::bind_peer(18393).await;

        let attempts = if available { 1 } else { 4 };
        let mut responses = vec![(404, Value::Null); attempts * 2];
        if available {
            responses.extend([
                (200, serde_json::json!({"market":definition["market"]})),
                (
                    200,
                    serde_json::json!({"event":definition["event"],"markets":[]}),
                ),
                (200, values["series"].clone()),
            ]);
        }

        let server = task::spawn(serve_responses(listener, responses));

        let config = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{address}/trade-api/v2").into()),
            ..Default::default()
        };

        let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(41))));
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let provider = KalshiInstrumentProvider::new(
            KalshiHttpClient::new(&config).unwrap(),
            clock,
            sender.into(),
        );

        let correlation = UUID4::new();
        let result = provider
            .request_instrument(RequestInstrument::new(
                id,
                None,
                None,
                None,
                correlation,
                UnixNanos::from(1),
                None,
            ))
            .await;

        if available {
            result.unwrap();

            let DataEvent::Response(DataResponse::Instrument(response)) =
                receiver.try_recv().unwrap()
            else {
                panic!()
            };

            assert_eq!(response.instrument_id, id);
            assert_eq!(response.correlation_id, correlation);
            assert_eq!(response.ts_init, UnixNanos::from(41));
        } else {
            assert!(result.is_err());
        }

        assert!(receiver.try_recv().is_err());
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), if available { 5 } else { 8 });

        for pair in requests[..attempts * 2].as_chunks::<2>().0 {
            assert_eq!(
                pair[0],
                format!("GET /trade-api/v2/markets/{} HTTP/1.1", id.symbol)
            );
            assert_eq!(
                pair[1],
                format!(
                    "GET /trade-api/v2/historical/markets/{} HTTP/1.1",
                    id.symbol
                )
            );
        }
    }))
    .await
    .unwrap();
}

async fn serve_responses(listener: TcpListener, responses: Vec<(u16, Value)>) -> Vec<String> {
    let mut requests = Vec::new();

    for (status, body) in responses {
        let (mut stream, _) = listener.accept().await.unwrap();
        requests.push(
            peer::read_headers(&mut stream)
                .await
                .lines()
                .next()
                .unwrap()
                .to_owned(),
        );
        let body = serde_json::to_string(&body).unwrap();
        stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    }

    requests
}

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap()
}

#[rstest]
#[case("settled", "1.000000")]
#[case("archived", "0.000000")]
fn final_close_uses_exact_authoritative_fields(#[case] name: &str, #[case] price: &str) {
    let (market, event) = definition(name);
    let received = UnixNanos::from(1_900_000_000_123_456_789);
    let close = parse_instrument_close(&market, &event, received)
        .unwrap()
        .unwrap();
    assert_eq!(close.instrument_id.symbol.as_str(), market.ticker);
    assert_eq!(close.instrument_id.venue.as_str(), "KALSHI");
    assert_eq!(close.close_price, Price::from(price));
    assert_eq!(close.close_type, InstrumentCloseType::ContractExpired);
    assert_eq!(
        close.ts_event,
        try_datetime_to_unix_nanos(match market.settlement_ts {
            Field::Value(time) => time,
            _ => panic!(),
        })
        .unwrap()
    );
    assert_eq!(close.ts_init, received);
}

#[rstest]
#[case("initialized")]
#[case("active")]
#[case("inactive")]
#[case("closed")]
#[case("determined")]
#[case("disputed")]
#[case("amended")]
fn nonfinal_states_never_supply_close(#[case] state: &str) {
    let (mut market, event) = definition("settled");
    market.status = state.parse().unwrap();
    assert_eq!(
        parse_instrument_close(&market, &event, UnixNanos::from(1)).unwrap(),
        None
    );
}

#[rstest]
#[case("missing_value")]
#[case("missing_time")]
#[case("negative")]
#[case("over_one")]
#[case("precision")]
#[case("scalar")]
#[case("floor")]
#[case("mve")]
fn invalid_final_evidence_fails_explicitly(#[case] invalid: &str) {
    let (mut market, event) = definition("settled");
    match invalid {
        "missing_value" => market.settlement_value_dollars = Field::Missing,
        "missing_time" => market.settlement_ts = Field::Null,
        "negative" | "over_one" | "precision" => {
            let value = match invalid {
                "negative" => "-0.01",
                "over_one" => "1.01",
                _ => "0.1234567",
            };

            market.settlement_value_dollars = Field::Value(FixedPoint(value.parse().unwrap()));
        }
        "scalar" => market.market_type = "scalar".parse().unwrap(),
        "floor" => market.settlement_bounds_type = "floor".parse().unwrap(),
        "mve" => market.mve_collection_ticker = Field::Value("MVE-COLLECTION".into()),
        _ => panic!(),
    }

    assert!(parse_instrument_close(&market, &event, UnixNanos::from(1)).is_err());
}

fn definition(name: &str) -> (Market, Event) {
    let value = &fixtures()["definitions"][name];

    (
        serde_json::from_value(value["market"].clone()).unwrap(),
        serde_json::from_value(value["event"].clone()).unwrap(),
    )
}

fn decimal(value: &str) -> Decimal {
    Decimal::from_str_exact(value).unwrap()
}
