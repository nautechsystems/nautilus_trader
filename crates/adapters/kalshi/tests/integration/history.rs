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

use std::{cell::RefCell, num::NonZeroUsize, rc::Rc, time::Duration};

use nautilus_common::{
    cache::Cache,
    clients::DataClient,
    clock::{Clock, VirtualClock},
    factories::DataClientFactory,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent,
        data::{DataResponse, RequestBars, RequestQuotes, RequestTrades},
    },
    msgbus::MessageBus,
};
use nautilus_core::{UUID4, UnixNanos, params::Params};
use nautilus_data::engine::DataEngine;
use nautilus_kalshi::{
    common::{enums::CandlestickInterval, parse::parse_instrument_id},
    config::KalshiDataClientConfig,
    data::KalshiDataClient,
    factories::KalshiDataClientFactory,
    http::{
        client::KalshiHttpClient,
        models::Market,
        parse::parse_instrument,
        query::{CandlesticksQuery, TradesQuery},
    },
};
use nautilus_model::{
    data::BarType, enums::AggressorSide, identifiers::ClientId, instruments::InstrumentAny,
};
use rstest::rstest;
use serde_json::Value;
use tokio::{io::AsyncWriteExt, net::TcpListener};

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn trades_collect_live_then_full_archive_with_one_fixed_cutoff(#[case] archived_only: bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut f = fixtures();
        if archived_only { f["cutoff"]["trades_created_ts"] = "2023-11-14T22:16:00Z".into(); }
        let mut replies = vec![(200, f["cutoff"].clone())];
        if !archived_only { replies.push((200, f["trades"].clone())); }
        replies.push((200, f["trades"].clone()));
        let (config, peer) = serve(replies).await;
        let http = KalshiHttpClient::new(&config).unwrap();
        let query = trade_query(1700000100);
        let trades = http.get_trades_range(query, 2).await.unwrap();
        assert_eq!(trades, vec![serde_json::from_value(f["trade"].clone()).unwrap()]);
        let requests = peer.await.unwrap();
        let mut expected = vec!["GET /trade-api/v2/historical/cutoff HTTP/1.1".to_owned()];
        let query = format!("limit=1000&ticker=KXBTC15M-26OCT162345-45&min_ts=1700000000&max_ts={}&is_block_trade=false", 1700000100);

        if !archived_only { expected.push(format!("GET /trade-api/v2/markets/trades?{query} HTTP/1.1")); }
        expected.push(format!("GET /trade-api/v2/historical/trades?{query} HTTP/1.1"));
        assert_eq!(requests, expected);
    }).await.unwrap();
}

#[rstest]
#[case("conflict")]
#[case("cursor")]
#[case("budget")]
#[case("identity")]
#[case("filter")]
#[case("source")]
#[tokio::test]
async fn incomplete_trade_collections_fail(#[case] scenario: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let mut page = f["trades"].clone();
        let mut replies = vec![(200, f["cutoff"].clone())];

        let expected = match scenario {
            "conflict" => {
                let mut other = page["trades"][0].clone();
                other["count_fp"] = "9.75".into();
                page["trades"].as_array_mut().unwrap().push(other);
                "conflicting duplicate trade"
            }
            "identity" => {
                page["trades"][0]["ticker"] = "WRONG".into();
                "trade does not match requested filters"
            }
            "filter" => {
                page["trades"][0]["is_block_trade"] = true.into();
                "trade does not match requested filters"
            }
            "cursor" => {
                page["cursor"] = "again".into();
                "repeated cursor"
            }
            "budget" => {
                page["cursor"] = "next".into();
                "trade history page budget exhausted"
            }
            "source" => "HTTP 400",
            _ => unreachable!(),
        };

        replies.push((if scenario == "source" { 400 } else { 200 }, page.clone()));
        if scenario == "cursor" {
            replies.push((200, page));
        }

        let (config, peer) = serve(replies).await;
        let http = KalshiHttpClient::new(&config).unwrap();
        let error = http
            .get_trades_range(
                trade_query(1700000100),
                if scenario == "budget" { 1 } else { 2 },
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case("live")]
#[case("archived")]
#[case("handoff")]
#[tokio::test]
async fn candles_route_by_market_settlement_and_normalize_archive(#[case] scenario: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let mut market = f["market"]["market"].clone();
        let mut replies = vec![(200, f["cutoff"].clone())];

        if scenario != "live" {
            market["status"] = "finalized".into();
            market["settlement_ts"] = if scenario == "archived" {
                "2023-11-14T22:14:00Z"
            } else {
                "2023-11-14T22:16:00Z"
            }
            .into();
        }

        if scenario != "archived" {
            replies.push((200, f["candles"].clone()));
        }

        if scenario != "live" {
            replies.push((200, f["archived_candles"].clone()));
        }

        let (config, peer) = serve(replies).await;
        let market: Market = serde_json::from_value(market).unwrap();
        let http = KalshiHttpClient::new(&config).unwrap();

        let query = CandlesticksQuery {
            start_ts: 1700000040,
            end_ts: 1700000100,
            period_interval: CandlestickInterval::Minute,
        };

        let candles = http
            .get_candlesticks_range(&market, "KXBTC15M", &query)
            .await
            .unwrap();
        assert_eq!(candles.len(), 1);
        let mut expected = f["candles"]["candlesticks"][0].clone();
        if scenario == "archived" {
            expected["price"]
                .as_object_mut()
                .unwrap()
                .remove("min_dollars");
            expected["price"]
                .as_object_mut()
                .unwrap()
                .remove("max_dollars");
        }

        assert_eq!(serde_json::to_value(&candles[0]).unwrap(), expected);
        let requests = peer.await.unwrap();
        assert_eq!(requests[0], "GET /trade-api/v2/historical/cutoff HTTP/1.1");
        let q = "start_ts=1700000040&end_ts=1700000100&period_interval=1";
        let mut expected = vec![requests[0].clone()];
        if scenario != "archived" {
            expected.push(format!(
                "GET /trade-api/v2/series/KXBTC15M/markets/{}/candlesticks?{q} HTTP/1.1",
                market.ticker
            ));
        }

        if scenario != "live" {
            expected.push(format!(
                "GET /trade-api/v2/historical/markets/{}/candlesticks?{q} HTTP/1.1",
                market.ticker
            ));
        }

        assert_eq!(requests, expected);
    })
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn trade_requests_preserve_exact_window_limit_correlation_and_fresh_context(
    #[case] archived: bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f=fixtures();
        let mut newer=f["trade"].clone();newer["trade_id"]="newer".into();newer["created_time"]="2023-11-14T22:14:00.123456790Z".into();newer["taker_book_side"]="bid".into();newer["taker_outcome_side"]="yes".into();
        let mut boundary=f["trade"].clone();boundary["trade_id"]="end-boundary".into();boundary["created_time"]="2023-11-14T22:14:00.123456791Z".into();
        let mut before=f["trade"].clone();before["trade_id"]="before".into();before["created_time"]="2023-11-14T22:14:00.123456788Z".into();
        let mut replies=Vec::new();
        for revision in 0..2 {
            if archived {replies.push((404,Value::Null));}
            let mut market=f["market"].clone();market["market"]["price_level_structure"]=if revision==0 {"cent"} else {"deci_cent"}.into();
            replies.extend([(200,market),(200,f["event"].clone()),(200,f["series"].clone()),(200,f["cutoff"].clone())]);
            let mut page=f["trades"].clone();page["trades"]=vec![newer.clone(),boundary.clone(),f["trade"].clone(),before.clone()].into();
            replies.push((200,page));replies.push((200,f["trades"].clone()));
        }

        let (config,peer)=serve(replies).await;
        let (sender,mut events)=tokio::sync::mpsc::unbounded_channel();
        let id=parse_instrument_id(f["trade"]["ticker"].as_str().unwrap()).unwrap();
        let mut client=KalshiDataClient::new("LOCAL".into(),config,vec![id],sender.into()).unwrap();

        for limit in [Some(NonZeroUsize::new(2).unwrap()),None] {
            let start="2023-11-14T22:14:00.123456789Z".parse().unwrap();let end="2023-11-14T22:14:00.123456791Z".parse().unwrap();

            let params = Params::new([
                ("max_pages".into(), 2.into()),
                ("bar_types".into(), vec![format!("{id}-5-MINUTE-LAST-INTERNAL")].into()),
                ("update_subscriptions".into(), true.into()),
                ("skip_first_non_full_bar".into(), false.into()),
                ("time_zone".into(), "UTC".into()),
                ("durations_seconds".into(), vec![60, 300].into()),
                ("point_data".into(), false.into()),
                ("data_count".into(), 3.into()),
                ("subscription_name".into(), "history".into()),
            ].into());

            let request=RequestTrades::new(id,Some(start),Some(end),limit,Some("ROUTED".into()),UUID4::new(),UnixNanos::from(7),Some(params.clone()));
            let correlation=request.request_id;
            DataClient::request_trades(&client,request).unwrap();
            let DataEvent::Response(DataResponse::Trades(response))=events.recv().await.unwrap() else {panic!("expected trades response")};
            assert_eq!(response.correlation_id,correlation);assert_eq!(response.client_id,ClientId::from("ROUTED"));assert_eq!(response.instrument_ids,vec![id]);assert_eq!(response.params,Some(params));
            assert_eq!(response.start,Some(UnixNanos::from(1700000040123456789)));assert_eq!(response.end,Some(UnixNanos::from(1700000040123456791)));
            assert_eq!(response.data.len(),if limit.is_some() {2} else {3});

            for (index,tick) in response.data.iter().enumerate() {
                assert_eq!(tick.instrument_id,id);assert_eq!(tick.price.as_decimal(),"0.123456".parse().unwrap());assert_eq!(tick.price.precision,6);assert_eq!(tick.size.as_decimal(),"2.75".parse().unwrap());assert_eq!(tick.size.precision,2);
                assert_eq!(tick.aggressor_side,if index==1 {AggressorSide::Buy} else {AggressorSide::Sell});
                assert_eq!(tick.trade_id.as_str(),["trade-a","newer","end-boundary"][index]);assert_eq!(tick.ts_event,UnixNanos::from(1700000040123456789+index as u64));assert_eq!(tick.ts_init,tick.ts_event);
            }
        }

        client.disconnect().await.unwrap();
        let requests=peer.await.unwrap();
        assert_eq!(requests.iter().filter(|s| s.contains("historical/cutoff")).count(),2);
        assert_eq!(requests.iter().filter(|s| s.contains("/events/")).count(),2);
        assert_eq!(requests.iter().filter(|s| s.contains("/series/")).count(),2);
        let expected="limit=1000&ticker=KXBTC15M-26OCT162345-45&min_ts=1700000039&max_ts=1700000041&is_block_trade=false";
        assert_eq!(requests.iter().filter(|s| s.contains(expected)).count(),4);

        if archived {assert_eq!(requests.iter().filter(|s|s.contains("/historical/markets/")).count(),2);}
        assert!(events.try_recv().is_err());
    }).await.unwrap();
}

#[rstest]
#[case("LAST",["0.130001","0.220010","0.120009","0.210011"])]
#[case("BID",["0.100001","0.150003","0.090002","0.140004"])]
#[case("ASK",["0.200005","0.250007","0.190006","0.240008"])]
#[tokio::test]
async fn bar_requests_map_each_series_exactly_and_keep_zero_volume(
    #[case] price_type: &str,
    #[case] prices: [&str; 4],
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let mut candles = f["candles"].clone();
        let mut before = candles["candlesticks"][0].clone();
        before["end_period_ts"] = 1699999980_i64.into();
        let mut after = candles["candlesticks"][0].clone();
        after["end_period_ts"] = 1700000100_i64.into();
        candles["candlesticks"]
            .as_array_mut()
            .unwrap()
            .extend([after, before]);
        let (mut config, peer) = serve(vec![
            (200, f["market"].clone()),
            (200, f["event"].clone()),
            (200, f["series"].clone()),
            (200, f["cutoff"].clone()),
            (200, candles),
        ])
        .await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let id = parse_instrument_id(f["trade"]["ticker"].as_str().unwrap()).unwrap();
        config.instrument_ids = vec![id];
        replace_data_event_sender(sender);
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
        let mut client = KalshiDataClientFactory
            .create("LOCAL", &config, cache.into(), clock)
            .unwrap();
        let bar_type: BarType = format!("{id}-1-MINUTE-{price_type}-EXTERNAL")
            .parse()
            .unwrap();

        let request = RequestBars::new(
            bar_type,
            Some("2023-11-14T22:14:00Z".parse().unwrap()),
            Some("2023-11-14T22:14:59.999999999Z".parse().unwrap()),
            None,
            Some("ROUTED".into()),
            UUID4::new(),
            UnixNanos::from(7),
            None,
        );

        let correlation = request.request_id;
        client.request_bars(request).unwrap();

        let DataEvent::Response(DataResponse::Bars(response)) = events.recv().await.unwrap() else {
            panic!("expected bars response")
        };

        assert_eq!(response.correlation_id, correlation);
        assert_eq!(response.client_id, ClientId::from("ROUTED"));
        assert_eq!(response.bar_type, bar_type);
        assert_eq!(response.params, None);
        assert_eq!(response.start, Some(UnixNanos::from(1700000040000000000)));
        assert_eq!(response.end, Some(UnixNanos::from(1700000099999999999)));
        assert_eq!(response.data.len(), 1);
        let bar = &response.data[0];
        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(
            [
                bar.open.as_decimal(),
                bar.high.as_decimal(),
                bar.low.as_decimal(),
                bar.close.as_decimal()
            ],
            prices.map(|v| v.parse().unwrap())
        );
        assert_eq!(bar.volume.as_decimal(), rust_decimal::Decimal::ZERO);
        assert_eq!(bar.volume.precision, 2);
        assert_eq!(bar.ts_event, UnixNanos::from(1700000040000000000));
        assert_eq!(bar.ts_init, bar.ts_event);
        client.disconnect().await.unwrap();
        let requests = peer.await.unwrap();
        assert_eq!(requests.len(), 5);
        assert!(requests[4].contains("start_ts=1700000040&end_ts=1700000099&period_interval=1"));
    })
    .await
    .unwrap();
}

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../../test_data/history.json")).unwrap()
}

fn trade_query(end: i64) -> TradesQuery {
    TradesQuery {
        ticker: Some("KXBTC15M-26OCT162345-45".into()),
        min_ts: Some(1700000000),
        max_ts: Some(end),
        limit: Some(1000),
        is_block_trade: Some(false),
        ..Default::default()
    }
}

async fn serve(
    replies: Vec<(u16, Value)>,
) -> (KalshiDataClientConfig, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let config = KalshiDataClientConfig {
        base_url_http: Some(format!("http://{address}/trade-api/v2").into()),
        ..Default::default()
    };

    let peer = tokio::spawn(async move {
        let mut requests = Vec::new();

        for (status, body) in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = super::http_request::read_headers(&mut stream).await;
            requests.push(request.lines().next().unwrap().to_owned());
            let body = serde_json::to_string(&body).unwrap();
            stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        }

        requests
    });

    (config, peer)
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn trade_pagination_and_empty_history_cover_moving_handoff(#[case] empty: bool) {
    tokio::time::timeout(Duration::from_secs(5),async {
        let f=fixtures();
        let mut cutoff=f["cutoff"].clone();cutoff["trades_created_ts"]="2023-11-14T22:13:00Z".into();
        let mut page=f["trades"].clone();page["cursor"]="next-page".into();
        let mut second=f["trades"].clone();second["trades"][0]["trade_id"]="trade-b".into();

        let replies=if empty {vec![(200,cutoff),(200,f["empty_trades"].clone()),(200,f["empty_trades"].clone())]} else {vec![(200,cutoff),(200,page),(200,second),(200,f["trades"].clone())]};
        let (config,peer)=serve(replies).await;
        let result=KalshiHttpClient::new(&config).unwrap().get_trades_range(trade_query(1700000100),2).await.unwrap();

        let expected=if empty {Vec::new()}else{let a=serde_json::from_value(f["trade"].clone()).unwrap();let mut b=f["trade"].clone();b["trade_id"]="trade-b".into();vec![a,serde_json::from_value(b).unwrap()]};
        assert_eq!(result,expected);
        let requests=peer.await.unwrap();
        assert_eq!(requests.iter().filter(|v|v.contains("/cutoff")).count(),1);
        assert_eq!(requests.last().unwrap(),"GET /trade-api/v2/historical/trades?limit=1000&ticker=KXBTC15M-26OCT162345-45&min_ts=1700000000&max_ts=1700000100&is_block_trade=false HTTP/1.1");

        if !empty {assert_eq!(requests[2],"GET /trade-api/v2/markets/trades?limit=1000&cursor=next-page&ticker=KXBTC15M-26OCT162345-45&min_ts=1700000000&max_ts=1700000100&is_block_trade=false HTTP/1.1");}
    }).await.unwrap();
}

#[rstest]
#[case("404")]
#[case("conflict")]
#[case("identity")]
#[case("daily_offset")]
#[case("missing_settlement")]
#[tokio::test]
async fn candle_handoff_and_malformed_sources_are_explicit(#[case] scenario: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let mut market = f["market"]["market"].clone();
        market["status"] = "finalized".into();
        market["settlement_ts"] = "2023-11-14T22:16:00Z".into();
        let mut live = f["candles"].clone();
        let mut archive = f["archived_candles"].clone();
        match scenario {
            "conflict" => archive["candlesticks"][0]["volume"] = "1.00".into(),
            "identity" => live["ticker"] = "WRONG".into(),
            "daily_offset" => {
                live["candlesticks"][0]["end_period_ts"] = 1786852800_i64.into();
                archive["candlesticks"][0]["end_period_ts"] = 1786852800_i64.into();
            }
            "missing_settlement" => market["settlement_ts"] = Value::Null,
            _ => {}
        }

        let mut replies = vec![(200, f["cutoff"].clone())];
        if scenario != "missing_settlement" {
            replies.push((if scenario == "404" { 404 } else { 200 }, live));
        }

        if !matches!(scenario, "identity" | "missing_settlement") {
            replies.push((200, archive));
        }

        let (config, peer) = serve(replies).await;
        let market = serde_json::from_value(market).unwrap();

        let query = if scenario == "daily_offset" {
            CandlesticksQuery {
                start_ts: 1786852800,
                end_ts: 1786939200,
                period_interval: CandlestickInterval::Day,
            }
        } else {
            CandlesticksQuery {
                start_ts: 1700000040,
                end_ts: 1700000100,
                period_interval: CandlestickInterval::Minute,
            }
        };

        let result = KalshiHttpClient::new(&config)
            .unwrap()
            .get_candlesticks_range(&market, "KXBTC15M", &query)
            .await;

        let expected = match scenario {
            "404" => None,
            "conflict" => Some("conflicting duplicate candle"),
            "identity" => Some("candle market identity mismatch"),
            "daily_offset" => None,
            "missing_settlement" => Some("finalized market has no settlement timestamp"),
            _ => unreachable!(),
        };

        if let Some(message) = expected {
            assert_eq!(
                result.unwrap_err().to_string(),
                format!("parse error: {message}")
            );
        } else {
            let candles = result.unwrap();
            assert_eq!(candles.len(), 1);
            assert_eq!(
                candles[0].end_period_ts,
                if scenario == "daily_offset" {
                    1786852800
                } else {
                    1700000040
                }
            );
        }

        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn bar_requests_cover_archive_limit_params_and_future_partial(#[case] partial: bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let mut market = f["market"].clone();
        let mut cutoff = f["cutoff"].clone();
        let mut candles = if partial {
            f["candles"].clone()
        } else {
            f["archived_candles"].clone()
        };

        let mut replies = Vec::new();

        if partial {
            let ts = jiff::Timestamp::now().as_second() / 60 * 60 + 3600;
            candles["candlesticks"][0]["end_period_ts"] = ts.into();
            for key in [
                "open_dollars",
                "high_dollars",
                "low_dollars",
                "close_dollars",
            ] {
                candles["candlesticks"][0]["price"][key] = Value::Null;
            }

            candles["candlesticks"][0]["volume_fp"] = "12.00".into();
        } else {
            replies.push((404, Value::Null));
            market["market"]["settlement_ts"] = "2023-11-14T22:14:00Z".into();
            market["market"]["status"] = "finalized".into();
            let mut newer = candles["candlesticks"][0].clone();
            newer["end_period_ts"] = 1700000100_i64.into();
            candles["candlesticks"]
                .as_array_mut()
                .unwrap()
                .insert(0, newer);
            cutoff["market_settled_ts"] = "2023-11-14T22:15:00Z".into();
        }

        replies.extend([
            (200, market),
            (200, f["event"].clone()),
            (200, f["series"].clone()),
            (200, cutoff),
            (200, candles),
        ]);
        let (config, peer) = serve(replies).await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let id = parse_instrument_id(f["trade"]["ticker"].as_str().unwrap()).unwrap();
        let mut client =
            KalshiDataClient::new("LOCAL".into(), config, vec![id], sender.into()).unwrap();
        let bar_type = format!("{id}-1-MINUTE-LAST-EXTERNAL").parse().unwrap();

        let params = Params::new(
            [
                ("max_pages".into(), 3.into()),
                ("update_catalog".into(), true.into()),
                ("skip_catalog_data".into(), false.into()),
                ("filter_expr".into(), "price > 0".into()),
                ("only_last".into(), false.into()),
                ("from_day_start".into(), true.into()),
            ]
            .into(),
        );

        let correlation = UUID4::new();
        client
            .request_bars(RequestBars::new(
                bar_type,
                Some("2023-11-14T22:14:00Z".parse().unwrap()),
                Some(
                    if partial {
                        "2099-01-01T00:00:00Z"
                    } else {
                        "2023-11-14T22:15:00Z"
                    }
                    .parse()
                    .unwrap(),
                ),
                NonZeroUsize::new(1),
                None,
                correlation,
                UnixNanos::from(1),
                Some(params.clone()),
            ))
            .unwrap();

        let Some(DataEvent::Response(DataResponse::Bars(response))) = events.recv().await else {
            panic!("expected bars response")
        };

        assert_eq!(response.correlation_id, correlation);
        assert_eq!(response.client_id, ClientId::from("LOCAL"));
        assert_eq!(response.params, Some(params));
        assert_eq!(response.bar_type, bar_type);

        if partial {
            assert_eq!(response.data.as_ref(), &[]);
        } else {
            assert_eq!(response.data.len(), 1);
            assert_eq!(
                response.data[0].ts_event,
                UnixNanos::from(1700000040000000000)
            );
            assert_eq!(
                response.data[0].open.as_decimal(),
                "0.130001".parse().unwrap()
            );
        }

        client.disconnect().await.unwrap();
        let requests = peer.await.unwrap();
        assert_eq!(requests.len(), if partial { 5 } else { 6 });
        assert_eq!(
            requests.last().unwrap().contains("/historical/markets/"),
            !partial
        );
    })
    .await
    .unwrap();
}

#[rstest]
fn unsupported_requests_fail_without_network() {
    let id = parse_instrument_id("TEST").unwrap();
    let (sender, _) = tokio::sync::mpsc::unbounded_channel();

    let client = KalshiDataClient::new(
        "LOCAL".into(),
        KalshiDataClientConfig {
            base_url_http: Some("http://127.0.0.1:1".into()),
            ..Default::default()
        },
        vec![id],
        sender.into(),
    )
    .unwrap();

    let now = UnixNanos::from(1);
    let result = client.request_quotes(RequestQuotes::new(
        id,
        None,
        None,
        None,
        None,
        UUID4::new(),
        now,
        None,
    ));
    assert_eq!(
        result.unwrap_err().to_string(),
        "Kalshi does not provide historical quotes"
    );
    let result = client.request_trades(RequestTrades::new(
        id,
        None,
        None,
        None,
        None,
        UUID4::new(),
        now,
        None,
    ));
    assert_eq!(
        result.unwrap_err().to_string(),
        "Kalshi history requires a start bound"
    );
    let bar_type: BarType = format!("{id}-5-MINUTE-LAST-EXTERNAL").parse().unwrap();
    let result = client.request_bars(RequestBars::new(
        bar_type,
        None,
        None,
        None,
        None,
        UUID4::new(),
        now,
        None,
    ));
    assert_eq!(
        result.unwrap_err().to_string(),
        "unsupported Kalshi candle interval"
    );
}

#[tokio::test]
async fn bounded_past_history_survives_data_engine_response_and_reaches_cache() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let f = fixtures();
        let mut replies = vec![
            (200, f["market"].clone()),
            (200, f["event"].clone()),
            (200, f["series"].clone()),
            (200, f["cutoff"].clone()),
            (200, f["trades"].clone()),
            (200, f["empty_trades"].clone()),
        ];
        replies.extend([
            (200, f["market"].clone()),
            (200, f["event"].clone()),
            (200, f["series"].clone()),
            (200, f["cutoff"].clone()),
            (200, f["candles"].clone()),
        ]);
        let (config, peer) = serve(replies).await;
        let id = parse_instrument_id(f["trade"]["ticker"].as_str().unwrap()).unwrap();
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut client =
            KalshiDataClient::new("LOCAL".into(), config, vec![id], sender.into()).unwrap();
        let _bus =
            MessageBus::new("TRADER-001".into(), UUID4::new(), None, None).register_message_bus();

        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));

        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
        let market = serde_json::from_value(f["market"]["market"].clone()).unwrap();
        let event = serde_json::from_value(f["event"]["event"].clone()).unwrap();
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::BinaryOption(instrument))
            .unwrap();
        let start = Some("2023-11-14T22:14:00Z".parse().unwrap());
        let end = Some("2023-11-14T22:15:00Z".parse().unwrap());
        client
            .request_trades(RequestTrades::new(
                id,
                start,
                end,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
            ))
            .unwrap();

        let Some(DataEvent::Response(DataResponse::Trades(response))) = events.recv().await else {
            panic!("expected trades response")
        };

        let expected_trades = response.data.to_vec();
        data_engine.response(DataResponse::Trades(response));
        assert_eq!(cache.borrow().trades(&id), Some(expected_trades));
        let bar_type: BarType = format!("{id}-1-MINUTE-LAST-EXTERNAL").parse().unwrap();
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

        let Some(DataEvent::Response(DataResponse::Bars(response))) = events.recv().await else {
            panic!("expected bars response")
        };

        let expected_bars = response.data.to_vec();
        data_engine.response(DataResponse::Bars(response));
        assert_eq!(cache.borrow().bars(&bar_type), Some(expected_bars));
        client.disconnect().await.unwrap();
        assert_eq!(peer.await.unwrap().len(), 11);
    })
    .await
    .unwrap();
}

#[rstest]
#[case("empty_live")]
#[case("both_missing")]
#[tokio::test]
async fn settled_empty_window_distinguishes_live_success_from_missing_sources(
    #[case] scenario: &str,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        let f = fixtures();
        let mut market = f["market"]["market"].clone();
        market["status"] = "finalized".into();
        market["settlement_ts"] = "2023-11-14T22:16:00Z".into();
        let mut empty = f["candles"].clone();
        empty["candlesticks"] = Vec::<Value>::new().into();
        let (config, peer) = serve(vec![
            (200, f["cutoff"].clone()),
            (if scenario == "empty_live" { 200 } else { 404 }, empty),
            (404, Value::Null),
        ])
        .await;

        let result = KalshiHttpClient::new(&config)
            .unwrap()
            .get_candlesticks_range(
                &serde_json::from_value(market).unwrap(),
                "KXBTC15M",
                &CandlesticksQuery {
                    start_ts: 1700000040,
                    end_ts: 1700000100,
                    period_interval: CandlestickInterval::Minute,
                },
            )
            .await;

        if scenario == "empty_live" {
            assert_eq!(result.unwrap(), Vec::new());
        } else {
            assert_eq!(result.unwrap_err().to_string(), "HTTP 404");
        }

        assert_eq!(peer.await.unwrap().len(), 3);
    })
    .await
    .unwrap();
}
