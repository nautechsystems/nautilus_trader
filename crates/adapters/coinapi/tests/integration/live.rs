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

#![cfg(not(all(feature = "simulation", madsim)))]

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    time::Duration,
};

use jiff::Timestamp;
use nautilus_coinapi::{
    common::symbol::VenueSymbol,
    config::{CoinApiDataClientConfig, CoinApiInstrumentSelection},
    data::CoinApiDataClient,
    http::{
        client::{CoinApiHttpClient, CoinApiHttpConfig},
        query::{BookQuery, ExchangesQuery, HistoryQuery, OhlcvQuery, SymbolsQuery, TradesQuery},
    },
    websocket::{
        client::{CoinApiWebSocketClient, CoinApiWebSocketConfig},
        messages::{Channel, CoinApiWsMessage, DataType, Subscription},
    },
};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent, DataResponse,
        data::{
            RequestBars, RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestQuotes,
            RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth, SubscribeQuotes,
            SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{UUID4, UnixNanos, string::secret::SecretString};
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::{BarType, Data},
    enums::{AssetClass, BookAction, BookType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::retry::RetryConfig;
use rstest::rstest;
use ustr::Ustr;

const PRICE_PRECISION: u8 = 3;

#[tokio::test]
#[ignore = "uses CoinAPI credits and requires COINAPI_API_KEY"]
async fn bounded_rest_smoke() {
    let key =
        SecretString::from(std::env::var("COINAPI_API_KEY").expect("COINAPI_API_KEY required"));

    let client = CoinApiHttpClient::new(
        key,
        CoinApiHttpConfig {
            credit_budget: 7,
            retry: RetryConfig {
                max_retries: 0,
                max_elapsed_ms: Some(30_000),
                ..RetryConfig::default()
            },
            ..CoinApiHttpConfig::default()
        },
    )
    .unwrap();

    let symbol = "COINBASE_SPOT_BTC_USD";
    let start = Timestamp::now().as_second() - 86_400;

    let range = HistoryQuery {
        time_start: Timestamp::from_second(start).unwrap(),
        time_end: Timestamp::from_second(start + 60).unwrap(),
        limit: 2,
    };

    let exchanges = client
        .get_exchanges(&ExchangesQuery {
            filter_exchange_id: Some("COINBASE".into()),
        })
        .await
        .unwrap();

    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].exchange_id.as_str(), "COINBASE");
    println!("Exchange metadata passed");

    let symbols = client
        .get_symbols(
            "COINBASE",
            &SymbolsQuery {
                filter_symbol_id: Some(symbol.into()),
                ..SymbolsQuery::default()
            },
        )
        .await
        .unwrap();

    assert!(symbols.iter().any(
        |value| value.symbol_id.as_str() == symbol && value.exchange_id.as_str() == "COINBASE"
    ));
    println!("Active symbol metadata passed");

    let book = client
        .get_book(
            symbol,
            &BookQuery {
                limit_levels: Some(2),
            },
        )
        .await
        .unwrap();

    assert_eq!(book.symbol_id.as_str(), symbol);
    assert!(!book.asks.is_empty() && book.asks.len() <= 2);
    assert!(!book.bids.is_empty() && book.bids.len() <= 2);
    println!("Current L2 book passed");

    let book = client
        .get_book_l3(
            symbol,
            &BookQuery {
                limit_levels: Some(2),
            },
        )
        .await
        .unwrap();

    assert_eq!(book.symbol_id.as_str(), symbol);
    let ask_prices: HashSet<_> = book.asks.iter().map(|level| level.price).collect();
    let bid_prices: HashSet<_> = book.bids.iter().map(|level| level.price).collect();
    assert!(!ask_prices.is_empty() && ask_prices.len() <= 2);
    assert!(!bid_prices.is_empty() && bid_prices.len() <= 2);
    assert!(
        book.asks
            .iter()
            .chain(&book.bids)
            .all(|level| level.id.is_some())
    );
    println!("Current L3 book passed");

    let quotes = client.get_quotes(symbol, &range).await.unwrap();
    assert!(!quotes.is_empty() && quotes.len() <= 2);
    println!("Historical quotes passed, {} records", quotes.len());

    let trades = client
        .get_trades(
            symbol,
            &TradesQuery {
                range: range.clone(),
                include_id: Some(true),
            },
        )
        .await
        .unwrap();

    assert!(!trades.is_empty() && trades.len() <= 2);
    println!("Historical trades passed, {} records", trades.len());

    let bars = client
        .get_ohlcv(
            symbol,
            &OhlcvQuery {
                period_id: "1MIN".into(),
                range,
            },
        )
        .await
        .unwrap();

    assert!(!bars.is_empty() && bars.len() <= 2);
    println!("Historical OHLCV passed, {} records", bars.len());
    assert_eq!(client.credits_remaining(), 0);
    println!("Seven requests completed");
}

#[rstest]
#[case("https://api-emea.coinapi.io/v1/")]
#[case("https://api-apac.coinapi.io/v1/")]
#[case("https://api-ncsa.coinapi.io/v1/")]
#[tokio::test]
#[ignore = "uses CoinAPI credits and requires COINAPI_API_KEY"]
async fn bounded_rest_regional_smoke(#[case] base_url: &str) {
    let key =
        SecretString::from(std::env::var("COINAPI_API_KEY").expect("COINAPI_API_KEY required"));

    let client = CoinApiHttpClient::new(
        key,
        CoinApiHttpConfig {
            base_url: base_url.into(),
            credit_budget: 1,
            retry: RetryConfig {
                max_retries: 0,
                max_elapsed_ms: Some(30_000),
                ..RetryConfig::default()
            },
            ..CoinApiHttpConfig::default()
        },
    )
    .unwrap();

    let exchanges = client
        .get_exchanges(&ExchangesQuery {
            filter_exchange_id: Some("COINBASE".into()),
        })
        .await
        .unwrap();

    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].exchange_id.as_str(), "COINBASE");
    assert_eq!(client.credits_remaining(), 0);
    println!("Regional REST passed: {base_url}");
}

#[tokio::test]
#[ignore = "uses CoinAPI credits and requires COINAPI_API_KEY"]
async fn bounded_instrument_smoke() {
    let key =
        SecretString::from(std::env::var("COINAPI_API_KEY").expect("COINAPI_API_KEY required"));

    let client = CoinApiHttpClient::new(
        key,
        CoinApiHttpConfig {
            credit_budget: 1,
            retry: RetryConfig {
                max_retries: 0,
                max_elapsed_ms: Some(30_000),
                ..RetryConfig::default()
            },
            ..CoinApiHttpConfig::default()
        },
    )
    .unwrap();

    let instruments = client
        .request_instruments(
            "KRAKENFTS",
            &SymbolsQuery {
                filter_symbol_id: Some("KRAKENFTS_PERP_BTC_USD".into()),
                ..SymbolsQuery::default()
            },
            &HashMap::new(),
            UnixNanos::from(1),
        )
        .await
        .unwrap();

    let find = |id: &str| {
        instruments
            .iter()
            .find(|parsed| parsed.instrument.id() == InstrumentId::from(id))
            .unwrap_or_else(|| panic!("{id} not loaded"))
    };

    let inverse = &find("PI_XBTUSD.KRAKEN").instrument;
    let linear = &find("PF_XBTUSD.KRAKEN").instrument;

    assert!(inverse.is_inverse());
    assert_eq!(inverse.settlement_currency().code, "BTC");
    assert!(!linear.is_inverse());
    assert_eq!(linear.settlement_currency().code, "USD");

    for parsed in &instruments {
        let venue_symbol = VenueSymbol::from_native(parsed.instrument.id()).unwrap();
        assert_eq!(venue_symbol.exchange_id, "KRAKENFTS");
        assert_eq!(venue_symbol.symbol, parsed.instrument.raw_symbol().inner());
        println!(
            "{} -> {} {:?} tick {} lot {} multiplier {}",
            parsed.symbol_id,
            parsed.instrument.id(),
            venue_symbol.symbol_type,
            parsed.instrument.price_increment(),
            parsed.instrument.size_increment(),
            parsed.instrument.multiplier(),
        );
    }

    assert_eq!(client.credits_remaining(), 0);
    println!(
        "Instrument loading passed, {} instruments",
        instruments.len()
    );
}

#[tokio::test]
#[ignore = "uses CoinAPI credits and requires COINAPI_API_KEY"]
async fn bounded_tradfi_instrument_smoke() {
    let key =
        SecretString::from(std::env::var("COINAPI_API_KEY").expect("COINAPI_API_KEY required"));

    let client = CoinApiHttpClient::new(
        key,
        CoinApiHttpConfig {
            credit_budget: 1,
            retry: RetryConfig {
                max_retries: 0,
                max_elapsed_ms: Some(30_000),
                ..RetryConfig::default()
            },
            ..CoinApiHttpConfig::default()
        },
    )
    .unwrap();

    let instruments = client
        .request_instruments(
            "BINANCEFTS",
            &SymbolsQuery {
                filter_symbol_id: Some("BINANCEFTS_PERP_XAU_USDT".into()),
                ..SymbolsQuery::default()
            },
            &HashMap::new(),
            UnixNanos::from(1),
        )
        .await
        .unwrap();

    let parsed = instruments
        .iter()
        .find(|parsed| parsed.instrument.id() == InstrumentId::from("XAUUSDT-PERP.BINANCE"))
        .expect("XAUUSDT-PERP.BINANCE not loaded");

    assert!(matches!(
        parsed.instrument,
        InstrumentAny::PerpetualContract(_)
    ));
    assert_eq!(parsed.instrument.asset_class(), AssetClass::Commodity);
    assert_eq!(parsed.instrument.settlement_currency().code, "USDT");
    assert_eq!(client.credits_remaining(), 0);
    println!(
        "{} -> {} {:?} tick {} lot {}",
        parsed.symbol_id,
        parsed.instrument.id(),
        parsed.instrument.asset_class(),
        parsed.instrument.price_increment(),
        parsed.instrument.size_increment(),
    );
}

#[tokio::test]
#[ignore = "uses CoinAPI WebSocket data and requires COINAPI_API_KEY"]
async fn bounded_websocket_smoke() {
    let key =
        SecretString::from(std::env::var("COINAPI_API_KEY").expect("COINAPI_API_KEY required"));
    let (client, mut rx) =
        CoinApiWebSocketClient::new(key, CoinApiWebSocketConfig::default()).unwrap();

    let trades = Channel {
        data_type: DataType::Trade,
        period_id: None,
    };

    let ohlcv = Channel {
        data_type: DataType::Ohlcv,
        period_id: Some(Ustr::from("1SEC")),
    };

    let btc = Ustr::from("COINBASE_SPOT_BTC_USD");
    let eth = Ustr::from("COINBASE_SPOT_ETH_USD");
    let subscription = |channel, symbol_id| Subscription { channel, symbol_id };

    client.subscribe(subscription(trades, btc)).await.unwrap();
    next_matching(
        &mut rx,
        |m| matches!(m, CoinApiWsMessage::Trade(t) if t.symbol_id == btc),
    )
    .await;
    println!("BTC trade received");

    client.subscribe(subscription(trades, eth)).await.unwrap();
    next_matching(
        &mut rx,
        |m| matches!(m, CoinApiWsMessage::Trade(t) if t.symbol_id == eth),
    )
    .await;
    println!("ETH trade received after scope replacement");

    client.subscribe(subscription(ohlcv, btc)).await.unwrap();
    next_matching(&mut rx, |m| {
        matches!(m, CoinApiWsMessage::Ohlcv(bar) if bar.symbol_id == btc && bar.period_id == "1SEC")
    })
    .await;

    println!("OHLCV 1SEC received on the shared connection");

    let quotes = Channel {
        data_type: DataType::Quote,
        period_id: None,
    };

    client.subscribe(subscription(quotes, btc)).await.unwrap();
    next_matching(&mut rx, |message| {
        let CoinApiWsMessage::Quote(quote) = message else {
            return false;
        };

        if quote.symbol_id != btc {
            return false;
        }

        assert!(quote.bid_price > rust_decimal::Decimal::ZERO);
        assert!(quote.bid_price < quote.ask_price);
        assert!(quote.bid_size >= rust_decimal::Decimal::ZERO);
        assert!(quote.ask_size >= rust_decimal::Decimal::ZERO);
        true
    })
    .await;

    println!("BTC quote received on the shared connection");

    client.unsubscribe(subscription(trades, btc)).await.unwrap();

    // Messages routed before the scope changed can still be queued
    let grace = tokio::time::Instant::now() + Duration::from_secs(1);
    while tokio::time::timeout_at(grace, rx.recv()).await.is_ok() {}

    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while let Ok(Some(message)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        assert!(
            !matches!(&message, CoinApiWsMessage::Trade(t) if t.symbol_id == btc),
            "BTC trade after unsubscribe"
        );
        assert!(
            !matches!(
                message,
                CoinApiWsMessage::Error(_) | CoinApiWsMessage::Reconnected
            ),
            "unexpected {message:?}"
        );
    }

    println!("No BTC trades after unsubscribe");

    client.unsubscribe(subscription(trades, eth)).await.unwrap();
    client.close().await;
    println!("Channels closed");
}

#[rstest]
#[case(DataType::Book5, 5)]
#[case(DataType::Book20, 20)]
#[case(DataType::Book50, 50)]
#[tokio::test]
#[ignore = "uses CoinAPI WebSocket data and requires COINAPI_API_KEY"]
async fn bounded_depth_smoke(#[case] data_type: DataType, #[case] depth: usize) {
    let key =
        SecretString::from(std::env::var("COINAPI_API_KEY").expect("COINAPI_API_KEY required"));
    let (client, mut rx) =
        CoinApiWebSocketClient::new(key, CoinApiWebSocketConfig::default()).unwrap();
    let symbol_id = Ustr::from("COINBASE_SPOT_BTC_USD");

    let subscription = Subscription {
        channel: Channel {
            data_type,
            period_id: None,
        },
        symbol_id,
    };

    client.subscribe(subscription).await.unwrap();
    next_matching(&mut rx, |message| {
        let CoinApiWsMessage::Book(book) = message else {
            return false;
        };

        if book.symbol_id != symbol_id || book.data_type != data_type {
            return false;
        }

        assert_eq!((book.bids.len(), book.asks.len()), (depth, depth));
        assert!(
            book.bids
                .windows(2)
                .all(|pair| pair[0].price > pair[1].price)
        );
        assert!(
            book.asks
                .windows(2)
                .all(|pair| pair[0].price < pair[1].price)
        );
        assert!(book.bids[0].price < book.asks[0].price);
        assert!(book.bids.iter().chain(&book.asks).all(|level| {
            level.price > rust_decimal::Decimal::ZERO && level.size > rust_decimal::Decimal::ZERO
        }));
        true
    })
    .await;

    client.unsubscribe(subscription).await.unwrap();
    client.close().await;
    println!("Fixed-depth stream passed: {data_type:?}, {depth} levels per side");
}

#[tokio::test]
#[ignore = "uses CoinAPI credits and WebSocket data and requires COINAPI_API_KEY"]
async fn bounded_data_client_smoke() {
    let _log_guard =
        nautilus_common::testing::init_logger_for_testing(Some(log::LevelFilter::Debug)).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(tx);

    let client_id = ClientId::new("COINAPI");
    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let bar_type = BarType::from("BTC-USD.COINBASE-1-SECOND-LAST-EXTERNAL");

    // Leaving api_key unset resolves COINAPI_API_KEY
    let config = CoinApiDataClientConfig::builder()
        .instruments(vec![CoinApiInstrumentSelection {
            exchange_id: "COINBASE".into(),
            query: SymbolsQuery {
                filter_symbol_id: Some("COINBASE_SPOT_BTC_USD".into()),
                ..SymbolsQuery::default()
            },
            instrument_ids: vec![instrument_id],
        }])
        .price_precisions(HashMap::from([(instrument_id, PRICE_PRECISION)]))
        .credit_budget(3)
        .max_retries(0)
        .build();

    let mut client = CoinApiDataClient::new(client_id, config).unwrap();
    client.connect().await.unwrap();

    let Some(DataEvent::Instrument(instrument)) = rx.recv().await else {
        panic!("expected instrument");
    };

    assert_eq!(instrument.id(), instrument_id);
    assert_eq!(instrument.price_precision(), PRICE_PRECISION);
    println!(
        "Loaded {} tick {} lot {}",
        instrument.id(),
        instrument.price_increment(),
        instrument.size_increment()
    );

    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_bars(SubscribeBars::new(
            bar_type,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let mut trades = 0;
    let mut bars = 0;
    let mut quotes = 0;
    tokio::time::timeout(Duration::from_secs(30), async {
        while trades == 0 || bars < 2 || quotes == 0 {
            match rx.recv().await.expect("data stream open") {
                DataEvent::Data(Data::Trade(trade)) => {
                    assert_eq!(trade.instrument_id, instrument_id);

                    if trades == 0 {
                        println!("First trade {trade:?}");
                    }

                    trades += 1;
                }
                DataEvent::Data(Data::Bar(bar)) => {
                    assert_eq!(bar.bar_type, bar_type);
                    assert!(bar.high >= bar.low);
                    assert_eq!(
                        [
                            bar.open.precision,
                            bar.high.precision,
                            bar.low.precision,
                            bar.close.precision,
                        ],
                        [PRICE_PRECISION; 4]
                    );
                    println!("Bar {bar:?}");
                    bars += 1;
                }
                DataEvent::Data(Data::Quote(quote)) => {
                    assert_eq!(quote.instrument_id, instrument_id);
                    assert!(quote.bid_price < quote.ask_price);
                    assert_eq!(
                        [quote.bid_price.precision, quote.ask_price.precision],
                        [PRICE_PRECISION; 2]
                    );

                    if quotes == 0 {
                        println!("First quote {quote:?}");
                    }

                    quotes += 1;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    })
    .await
    .expect("quotes, trades, and two bars within 30 seconds");

    println!("Received {quotes} quotes, {trades} trades, and {bars} bars");

    check_instrument_refresh(&client, &mut rx, client_id, &instrument).await;

    client
        .unsubscribe_trades(&UnsubscribeTrades::new(
            instrument_id,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    // Trades routed before the scope changed can still be queued
    let grace = tokio::time::Instant::now() + Duration::from_secs(1);
    while tokio::time::timeout_at(grace, rx.recv()).await.is_ok() {}

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        assert!(
            matches!(event, DataEvent::Data(Data::Bar(_) | Data::Quote(_))),
            "unexpected {event:?} after trade unsubscribe"
        );
    }

    println!("No trades after unsubscribe");

    client
        .unsubscribe_quotes(&UnsubscribeQuotes::new(
            instrument_id,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .unsubscribe_bars(&UnsubscribeBars::new(
            bar_type,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let grace = tokio::time::Instant::now() + Duration::from_secs(1);
    while tokio::time::timeout_at(grace, rx.recv()).await.is_ok() {}

    assert!(
        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .is_err()
    );
    println!("All streams quiet after unsubscribe");

    client.disconnect().await.unwrap();
}

async fn check_instrument_refresh(
    client: &CoinApiDataClient,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    client_id: ClientId,
    instrument: &InstrumentAny,
) {
    let instrument_id = instrument.id();

    let request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::default(),
            None,
        ))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let DataEvent::Response(DataResponse::Instrument(response)) =
                rx.recv().await.expect("data stream open")
            {
                assert_eq!(response.correlation_id, request_id);
                assert_eq!(response.data.id(), instrument.id());
                assert_eq!(
                    response.data.price_increment(),
                    instrument.price_increment()
                );
                assert_eq!(response.data.size_increment(), instrument.size_increment());
                break;
            }
        }

        loop {
            if let DataEvent::Data(Data::Trade(trade)) = rx.recv().await.expect("data stream open")
            {
                assert_eq!(trade.instrument_id, instrument_id);
                break;
            }
        }
    })
    .await
    .expect("instrument refresh and subsequent trade within 30 seconds");

    println!("Metadata refreshed while streaming, trade received afterward");

    let request_id = UUID4::new();
    client
        .request_instruments(RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(instrument_id.venue),
            request_id,
            UnixNanos::default(),
            None,
        ))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let DataEvent::Response(DataResponse::Instruments(response)) =
                rx.recv().await.expect("data stream open")
            {
                assert_eq!(response.correlation_id, request_id);
                assert_eq!(response.client_id, client_id);
                assert_eq!(response.venue, Venue::new("COINBASE"));
                assert_eq!(response.data.len(), 1);
                assert_eq!(response.data[0].id(), instrument_id);
                assert_eq!(
                    response.data[0].price_increment(),
                    instrument.price_increment()
                );
                assert_eq!(
                    response.data[0].size_increment(),
                    instrument.size_increment()
                );
                break;
            }
        }
    })
    .await
    .expect("plural instrument refresh within 30 seconds");

    println!("Plural instrument request passed while streaming");
}

// Streams each book channel in turn, since a connection can list only one book type
#[tokio::test]
#[ignore = "uses CoinAPI credits and WebSocket data and requires COINAPI_API_KEY"]
async fn bounded_book_smoke() {
    let _log_guard =
        nautilus_common::testing::init_logger_for_testing(Some(log::LevelFilter::Info)).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(tx);

    let client_id = ClientId::new("COINAPI");
    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");

    // Leaving api_key unset resolves COINAPI_API_KEY
    let config = CoinApiDataClientConfig::builder()
        .instruments(vec![CoinApiInstrumentSelection {
            exchange_id: "COINBASE".into(),
            query: SymbolsQuery {
                filter_symbol_id: Some("COINBASE_SPOT_BTC_USD".into()),
                ..SymbolsQuery::default()
            },
            instrument_ids: vec![instrument_id],
        }])
        .credit_budget(1)
        .max_retries(0)
        .build();

    let mut client = CoinApiDataClient::new(client_id, config).unwrap();
    client.connect().await.unwrap();

    let Some(DataEvent::Instrument(instrument)) = rx.recv().await else {
        panic!("expected instrument");
    };

    assert_eq!(instrument.id(), instrument_id);

    for book_type in [BookType::L2_MBP, BookType::L3_MBO] {
        check_book_deltas(&mut client, &mut rx, client_id, instrument_id, book_type).await;
    }

    for requested_depth in [5, 20, 50] {
        check_book_depth(
            &mut client,
            &mut rx,
            client_id,
            instrument_id,
            requested_depth,
        )
        .await;
    }

    client.disconnect().await.unwrap();
}

async fn check_book_deltas(
    client: &mut CoinApiDataClient,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    client_id: ClientId,
    instrument_id: InstrumentId,
    book_type: BookType,
) {
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            book_type,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .unwrap();

    let mut checker = BookStreamChecker::new(book_type, true);
    checker.open(instrument_id);
    let mut snapshots = 0;
    let mut updates = 0;

    // Waits for the snapshot, then checks every batch for five seconds; the deadline is checked
    // on every batch because a backlog never leaves the receiver waiting
    let mut until = tokio::time::Instant::now() + Duration::from_secs(60);
    while tokio::time::Instant::now() < until
        && let Ok(event) = tokio::time::timeout_at(until, rx.recv()).await
    {
        let DataEvent::Data(Data::BookDeltas(deltas)) = event.expect("data stream open") else {
            continue;
        };

        checker.apply(&deltas).unwrap();

        if deltas.deltas[0].action == BookAction::Clear {
            if snapshots == 0 {
                until = tokio::time::Instant::now() + Duration::from_secs(5);
            }

            snapshots += 1;
        } else {
            updates += 1;
        }
    }

    let book = checker.book(instrument_id).unwrap();
    println!(
        "{book_type}: {snapshots} snapshots, {updates} updates, best bid {:?}, best ask {:?}, \
         {} bid and {} ask levels",
        book.best_bid_price(),
        book.best_ask_price(),
        book.bids(None).count(),
        book.asks(None).count()
    );
    assert!(snapshots >= 1, "no {book_type} snapshot");
    assert!(updates > 0, "no {book_type} updates");

    client
        .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
            instrument_id,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    // Batches emitted before the unsubscribe can still be queued for the next checker
    let grace = tokio::time::Instant::now() + Duration::from_secs(1);
    while tokio::time::timeout_at(grace, rx.recv()).await.is_ok() {}
}

async fn check_book_depth(
    client: &mut CoinApiDataClient,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    client_id: ClientId,
    instrument_id: InstrumentId,
    requested_depth: usize,
) {
    client
        .subscribe_book_depth(SubscribeBookDepth::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            NonZeroUsize::new(requested_depth),
            false,
            None,
            None,
        ))
        .unwrap();

    let mut depths = 0;
    tokio::time::timeout(Duration::from_secs(60), async {
        while depths < 3 {
            let DataEvent::Data(Data::BookDepth(depth)) =
                rx.recv().await.expect("data stream open")
            else {
                continue;
            };

            assert_eq!(depth.instrument_id, instrument_id);
            assert_eq!(
                (depth.bids.len(), depth.asks.len()),
                (requested_depth, requested_depth)
            );
            assert!(
                depth
                    .bids
                    .windows(2)
                    .all(|pair| pair[0].price > pair[1].price)
            );
            assert!(
                depth
                    .asks
                    .windows(2)
                    .all(|pair| pair[0].price < pair[1].price)
            );
            assert!(depth.bids[0].price < depth.asks[0].price);

            if depths == 0 {
                println!("Depth {requested_depth}: {depth:?}");
            }

            depths += 1;
        }
    })
    .await
    .expect("three depth snapshots within 60 seconds");

    client
        .unsubscribe_book_depth(&UnsubscribeBookDepth::new(
            instrument_id,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let grace = tokio::time::Instant::now() + Duration::from_secs(1);
    while tokio::time::timeout_at(grace, rx.recv()).await.is_ok() {}
}

// Restarts a book pair on the open connection, first as the whole scope, then beside another symbol
#[tokio::test]
#[ignore = "uses CoinAPI WebSocket data and requires COINAPI_API_KEY"]
async fn bounded_book_resubscribe_smoke() {
    let key =
        SecretString::from(std::env::var("COINAPI_API_KEY").expect("COINAPI_API_KEY required"));
    let (client, mut rx) =
        CoinApiWebSocketClient::new(key, CoinApiWebSocketConfig::default()).unwrap();

    let channel = |data_type| Channel {
        data_type,
        period_id: None,
    };

    let book = Subscription {
        channel: channel(DataType::Book),
        symbol_id: Ustr::from("COINBASE_SPOT_BTC_USD"),
    };

    let eth_trades = Subscription {
        channel: channel(DataType::Trade),
        symbol_id: Ustr::from("COINBASE_SPOT_ETH_USD"),
    };

    // A restart shows as a snapshot at sequence one on the same connection; next_matching also
    // fails on a venue error
    let restarted = |message: &CoinApiWsMessage| {
        assert!(
            !matches!(message, CoinApiWsMessage::Reconnected),
            "restart reconnected"
        );
        matches!(message, CoinApiWsMessage::Book(b) if b.is_snapshot == Some(true) && b.sequence == 1)
    };

    client.subscribe(book).await.unwrap();
    next_matching(&mut rx, restarted).await;
    next_matching(
        &mut rx,
        |m| matches!(m, CoinApiWsMessage::Book(b) if b.is_snapshot == Some(false) && b.sequence > 1),
    )
    .await;
    println!("Book streaming as the whole scope");

    client.resubscribe(book, || {}).await.unwrap();
    next_matching(&mut rx, restarted).await;
    println!("Book restarted at sequence 1 on the same connection");

    client.subscribe(eth_trades).await.unwrap();
    next_matching(
        &mut rx,
        |m| matches!(m, CoinApiWsMessage::Book(b) if b.is_snapshot == Some(false) && b.sequence > 1),
    )
    .await;
    client.resubscribe(book, || {}).await.unwrap();
    next_matching(&mut rx, restarted).await;
    println!("Book restarted at sequence 1 beside ETH trades");

    client.close().await;
}

async fn next_matching(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<CoinApiWsMessage>,
    predicate: impl Fn(&CoinApiWsMessage) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let message = rx.recv().await.expect("message stream open");
            assert!(
                !matches!(message, CoinApiWsMessage::Error(_)),
                "venue error {message:?}"
            );

            if predicate(&message) {
                return;
            }
        }
    })
    .await
    .expect("expected message within 20 seconds");
}

#[tokio::test]
#[ignore = "uses CoinAPI credits and requires COINAPI_API_KEY"]
async fn bounded_history_smoke() {
    let _log_guard =
        nautilus_common::testing::init_logger_for_testing(Some(log::LevelFilter::Debug)).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(tx);

    let client_id = ClientId::new("COINAPI");
    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let bar_type = BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL");

    // Leaving api_key unset resolves COINAPI_API_KEY
    let config = CoinApiDataClientConfig::builder()
        .instruments(vec![CoinApiInstrumentSelection {
            exchange_id: "COINBASE".into(),
            query: SymbolsQuery {
                filter_symbol_id: Some("COINBASE_SPOT_BTC_USD".into()),
                ..SymbolsQuery::default()
            },
            instrument_ids: vec![instrument_id],
        }])
        .price_precisions(HashMap::from([(instrument_id, PRICE_PRECISION)]))
        .credit_budget(5)
        .max_retries(0)
        .build();

    let mut client = CoinApiDataClient::new(client_id, config).unwrap();
    client.connect().await.unwrap();

    let Some(DataEvent::Instrument(instrument)) = rx.recv().await else {
        panic!("expected instrument");
    };

    assert_eq!(instrument.id(), instrument_id);

    let start = Timestamp::from_second(Timestamp::now().as_second() / 60 * 60 - 86_400).unwrap();
    let end = start + jiff::SignedDuration::from_mins(3);
    let limit = NonZeroUsize::new(3);
    let request_ids = [UUID4::new(), UUID4::new(), UUID4::new(), UUID4::new()];

    client
        .request_book_snapshot(RequestBookSnapshot::new(
            instrument_id,
            NonZeroUsize::new(2),
            Some(client_id),
            request_ids[0],
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    client
        .request_quotes(RequestQuotes::new(
            instrument_id,
            Some(start),
            Some(end),
            limit,
            Some(client_id),
            request_ids[1],
            UnixNanos::default(),
            None,
        ))
        .unwrap();
    client
        .request_trades(RequestTrades::new(
            instrument_id,
            Some(start),
            Some(end),
            limit,
            Some(client_id),
            request_ids[2],
            UnixNanos::default(),
            None,
        ))
        .unwrap();
    client
        .request_bars(RequestBars::new(
            bar_type,
            Some(start),
            Some(end),
            limit,
            Some(client_id),
            request_ids[3],
            UnixNanos::default(),
            None,
        ))
        .unwrap();
    let mut remaining = HashSet::from(request_ids);
    tokio::time::timeout(Duration::from_secs(60), async {
        while !remaining.is_empty() {
            let DataEvent::Response(mut response) = rx.recv().await.expect("data channel open")
            else {
                panic!("expected historical response");
            };

            assert!(
                remaining.remove(response.correlation_id()),
                "unexpected response ID"
            );
            response.trim_to_bounds();

            match response {
                DataResponse::Book(response) => {
                    assert_eq!(response.correlation_id, request_ids[0]);
                    assert_eq!(response.client_id, client_id);
                    let book = response.data;
                    assert_eq!(book.instrument_id, instrument_id);
                    assert!(book.best_bid_price() < book.best_ask_price());
                    println!(
                        "Book bid {:?} ask {:?} ts_last {}",
                        book.best_bid_price(),
                        book.best_ask_price(),
                        book.ts_last
                    );
                }
                DataResponse::Quotes(response) => {
                    assert_eq!(response.correlation_id, request_ids[1]);
                    assert_eq!(response.client_id, client_id);
                    assert_eq!(response.instrument_id, instrument_id);
                    assert_eq!(response.data.len(), 3);

                    for quote in &response.data {
                        assert_eq!(quote.instrument_id, instrument_id);
                        assert!(quote.bid_price < quote.ask_price);
                        assert_eq!(
                            [quote.bid_price.precision, quote.ask_price.precision],
                            [PRICE_PRECISION; 2]
                        );
                        assert_eq!(quote.ts_init, quote.ts_event);
                    }

                    println!("Quotes {:?}", response.data);
                }
                DataResponse::Trades(response) => {
                    assert_eq!(response.correlation_id, request_ids[2]);
                    assert_eq!(response.client_id, client_id);
                    assert_eq!(response.instrument_id, instrument_id);
                    assert_eq!(response.data.len(), 3);

                    for trade in &response.data {
                        assert_eq!(trade.instrument_id, instrument_id);
                        assert_eq!(trade.price.precision, PRICE_PRECISION);
                        assert_eq!(trade.ts_init, trade.ts_event);
                    }

                    println!("Trades {:?}", response.data);
                }
                DataResponse::Bars(response) => {
                    assert_eq!(response.correlation_id, request_ids[3]);
                    assert_eq!(response.client_id, client_id);
                    assert_eq!(response.bar_type, bar_type);
                    assert!(
                        !response.data.is_empty(),
                        "historical bars must reach the consumer"
                    );
                    assert!(response.data.len() <= 3);

                    for bar in &response.data {
                        assert_eq!(bar.bar_type, bar_type);
                        assert!(bar.high >= bar.open && bar.high >= bar.close);
                        assert!(bar.low <= bar.open && bar.low <= bar.close);
                        assert_eq!(
                            [
                                bar.open.precision,
                                bar.high.precision,
                                bar.low.precision,
                                bar.close.precision
                            ],
                            [PRICE_PRECISION; 4]
                        );
                        assert_eq!(bar.ts_init, bar.ts_event);
                    }

                    println!(
                        "Bars ({} of 3 at instrument precision) {:?}",
                        response.data.len(),
                        response.data
                    );
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    })
    .await
    .expect("four historical responses within 60 seconds");

    client.disconnect().await.unwrap();
}
