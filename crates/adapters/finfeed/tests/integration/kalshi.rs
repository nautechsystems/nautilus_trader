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

//! FinFeed outcome definitions enriched by public Kalshi metadata.

use std::{sync::Arc, time::Duration};

use jiff::Timestamp;
use nautilus_common::{
    clients::DataClient,
    messages::{
        DataEvent, DataResponse,
        data::{RequestBookSnapshot, SubscribeQuotes, UnsubscribeQuotes},
    },
};
use nautilus_core::{UUID4, UnixNanos, string::secret::SecretString};
use nautilus_finfeed::{
    common::symbol::to_instrument_id,
    config::FinFeedDataClientConfig,
    data::FinFeedDataClient,
    http::{client::FinFeedHttpClient, error::Error},
    instruments::{FinFeedInstrumentProvider, FinFeedInstrumentSelection},
    loaders::FinFeedDataLoader,
};
use nautilus_model::{
    data::Data,
    enums::{AssetClass, CurrencyType},
    identifiers::ClientId,
    instruments::{
        Instrument,
        tick_scheme::{TickScheme, get_tick_scheme},
    },
    types::{Currency, Price},
};
use nautilus_network::dst::{task, time};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, json};

use super::{data::response, dst::start_peer, polling::timed_peer};

const MARKET: &str = include_str!("../../test_data/kalshi-market.json");
const NATIVE_MARKET: &str = include_str!("../../test_data/kalshi-native-market.json");
const NATIVE_EVENT: &str = include_str!("../../test_data/kalshi-native-event.json");
const EXCHANGES: &str = include_str!("../../test_data/exchanges.json");

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn conflicting_finfeed_exchange_rejects_before_public_metadata_request() {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market["exchange"] = json!({"exchange_id": "POLYMARKET"});
    let (peer, mut http) = start_peer(vec![(200, "", market.to_string())]).await;
    http.kalshi_metadata_url = Some("http://127.0.0.1:1/trade-api/v2".into());
    let loader = FinFeedDataLoader::with_http_config(Some("test-api-key".into()), http);
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap();

    let error = loader.request_instrument(id).await.unwrap_err();

    assert!(matches!(
        error.downcast_ref::<Error>(),
        Some(Error::Definition {
            field: "exchange_id",
            reason: "conflicts with nested exchange",
            ..
        })
    ));
    assert_eq!(
        loader.instrument(id).unwrap_err().to_string(),
        format!("No loaded instrument {id}"),
    );
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case("Yes", "YES")]
#[case("No", "NO")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn public_metadata_defines_every_field_without_forwarding_credentials(
    #[case] outcome: &str,
    #[case] suffix: &str,
) {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    let id = format!("TEST-KALSHI-30_{suffix}");
    market["market_id"] = id.clone().into();
    market["outcome_name"] = outcome.into();
    let (native_peer, native_http) =
        start_peer(vec![(200, "", NATIVE_MARKET), (200, "", NATIVE_EVENT)]).await;
    let (peer, mut http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", market.to_string()),
    ])
    .await;
    http.kalshi_metadata_url = Some(native_http.base_url);

    let http = Arc::new(
        FinFeedHttpClient::new(Some(SecretString::from("finfeed-only-secret")), http).unwrap(),
    );

    let mut provider = FinFeedInstrumentProvider::new(Arc::clone(&http));
    let ts_init = UnixNanos::from(1_800_000_000_123_456_789);

    let instruments = provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "KALSHI".into(),
                market_ids: vec![id.as_str().into()],
            }],
            ts_init,
        )
        .await
        .unwrap();

    let instrument = &instruments[0];
    let info = instrument.info.as_ref().unwrap();
    let timestamp =
        |value: &str| UnixNanos::from(value.parse::<Timestamp>().unwrap().as_nanosecond() as u64);

    assert_eq!(instruments.len(), 1);
    assert_eq!(instrument.id, to_instrument_id("KALSHI", &id).unwrap());
    assert_eq!(instrument.raw_symbol.as_str(), id);
    assert_eq!(instrument.asset_class, AssetClass::Alternative);
    assert_eq!(instrument.currency, Currency::USD());
    assert_eq!(instrument.currency.code.as_str(), "USD");
    assert_eq!(instrument.currency.precision, 2);
    assert_eq!(instrument.currency.iso4217, 840);
    assert_eq!(instrument.currency.name.as_str(), "United States dollar");
    assert_eq!(instrument.currency.currency_type, CurrencyType::Fiat);
    assert_eq!(instrument.activation_ns, timestamp("2026-01-02T03:04:05Z"));
    assert_eq!(instrument.expiration_ns, timestamp("2030-01-03T05:06:07Z"));
    assert_eq!(instrument.price_precision, 6);
    assert_eq!(instrument.size_precision, 2);
    assert_eq!(
        instrument.price_increment.as_decimal(),
        "0.001".parse::<Decimal>().unwrap()
    );
    assert_eq!(
        instrument.size_increment.as_decimal(),
        "0.01".parse::<Decimal>().unwrap()
    );
    assert_eq!(instrument.min_quantity, Some(instrument.size_increment));
    assert_eq!(instrument.max_quantity, None);
    assert_eq!(instrument.min_notional, None);
    assert_eq!(instrument.max_notional, None);
    assert_eq!(instrument.min_price, None);
    assert_eq!(instrument.max_price, None);
    assert_eq!(instrument.margin_init, Decimal::ZERO);
    assert_eq!(instrument.margin_maint, Decimal::ZERO);
    assert_eq!(instrument.event_id.unwrap().as_str(), "TEST-KALSHI");
    assert_eq!(instrument.outcome.unwrap().as_str(), outcome);
    assert_eq!(
        instrument.description.unwrap().as_str(),
        "Authored FinFeed Kalshi definition with public metadata enrichment."
    );
    assert_eq!(instrument.ts_event, timestamp("2026-11-10T06:02:00Z"));
    assert_eq!(instrument.ts_init, ts_init);
    assert_eq!(info.get_str("provider"), Some("FINFEED"));
    assert_eq!(info.get_str("metadata_source"), Some("KALSHI"));
    assert_eq!(info.get_str("market_id"), Some(id.as_str()));
    assert_eq!(info.get_str("status"), Some("Open"));
    assert_eq!(info.get_str("activation_source"), Some("Kalshi.open_time"));
    assert_eq!(
        info.get_str("expiration_source"),
        Some("Kalshi.latest_expiration_time")
    );
    assert_eq!(info["close_time"], json!("2029-12-30T04:05:06Z"));
    assert_eq!(info["market"]["ticker"], json!("TEST-KALSHI-30"));
    assert_eq!(info["event"]["event_ticker"], json!("TEST-KALSHI"));
    assert_eq!(provider.get(instrument.id), Some(instrument));
    assert_eq!(http.attempts_remaining(), 98);

    let TickScheme::Tiered(grid) =
        get_tick_scheme(instrument.tick_scheme.unwrap().as_str()).unwrap()
    else {
        panic!("expected tiered grid")
    };

    assert_eq!(grid.ticks().len(), 279);

    for request in native_peer.await.unwrap() {
        assert!(!request.contains("finfeed-only-secret"));
        assert!(!request.to_lowercase().contains("authorization:"));
        assert!(!request.to_lowercase().contains("kalshi-access"));
    }

    assert_eq!(peer.await.unwrap().len(), 2);
}

#[rstest]
#[case::zero_start_yes("YES", None)]
#[case::zero_start_no("NO", None)]
#[case::nonzero_start_yes("YES", Some("0.000001"))]
#[case::nonzero_start_no("NO", Some("0.000001"))]
#[case::wide_step_yes("YES", Some("0.000002"))]
#[case::wide_step_no("NO", Some("0.000002"))]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn public_metadata_preserves_single_tick_bands_at_six_decimals(
    #[case] outcome: &str,
    #[case] singleton_step: Option<&str>,
) {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market["market_id"] = format!("TEST-KALSHI-30_{outcome}").into();
    market["outcome_name"] = if outcome == "YES" { "Yes" } else { "No" }.into();
    let mut native: Value = serde_json::from_str(NATIVE_MARKET).unwrap();

    native["market"]["price_ranges"] = if let Some(step) = singleton_step {
        json!([{"start":"0.999999", "end":"1.000000", "step":step}])
    } else {
        json!([
            {"start":"0.000000", "end":"0.000002", "step":"0.000001"},
            {"start":"0.000002", "end":"1.000000", "step":"0.249999"},
        ])
    };

    let (native_peer, native_http) = start_peer(vec![
        (200, "", native.to_string()),
        (200, "", NATIVE_EVENT.to_owned()),
    ])
    .await;
    let (peer, mut http) = start_peer(vec![(200, "", market.to_string())]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let loader = FinFeedDataLoader::with_http_config(Some("test-api-key".into()), http);
    let id = to_instrument_id("KALSHI", &format!("TEST-KALSHI-30_{outcome}")).unwrap();

    let instrument = loader.request_instrument(id).await.unwrap();

    let TickScheme::Tiered(grid) =
        get_tick_scheme(instrument.tick_scheme.unwrap().as_str()).unwrap()
    else {
        panic!("expected exact tiered grid")
    };

    let (expected, increment) = match (singleton_step.is_some(), outcome) {
        (false, "YES") => (
            vec![
                "0.000001", "0.000002", "0.250001", "0.500000", "0.749999", "0.999998",
            ],
            "0.000001",
        ),
        (false, "NO") => (
            vec![
                "0.000002", "0.250001", "0.500000", "0.749999", "0.999998", "0.999999",
            ],
            "0.000001",
        ),
        (true, "YES") => (vec!["0.999999"], "0.000001"),
        (true, "NO") => (vec!["0.000001"], "0.000001"),
        _ => unreachable!(),
    };

    let expected = expected.into_iter().map(Price::from).collect::<Vec<_>>();

    assert_eq!(grid.ticks(), expected);
    assert_eq!(grid.precision(), 6);
    assert_eq!(instrument.price_increment, Price::from(increment));
    assert_eq!(native_peer.await.unwrap().len(), 2);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case::yes("YES", "0.030000", 0)]
#[case::no("NO", "0.010000", 2)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn public_metadata_derives_outcome_quantum_for_three_cent_grid(
    #[case] outcome: &str,
    #[case] increment: &str,
    #[case] offset: i64,
) {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market["market_id"] = format!("TEST-KALSHI-30_{outcome}").into();
    market["outcome_name"] = if outcome == "YES" { "Yes" } else { "No" }.into();
    let mut native: Value = serde_json::from_str(NATIVE_MARKET).unwrap();
    native["market"]["price_ranges"] = json!([
        {"start":"0.0000", "end":"1.0000", "step":"0.0300"},
    ]);
    let (native_peer, native_http) = start_peer(vec![
        (200, "", native.to_string()),
        (200, "", NATIVE_EVENT.to_owned()),
    ])
    .await;
    let (peer, mut http) = start_peer(vec![(200, "", market.to_string())]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let loader = FinFeedDataLoader::with_http_config(Some("test-api-key".into()), http);
    let id = to_instrument_id("KALSHI", &format!("TEST-KALSHI-30_{outcome}")).unwrap();

    let instrument = loader.request_instrument(id).await.unwrap();

    let TickScheme::Tiered(grid) =
        get_tick_scheme(instrument.tick_scheme.unwrap().as_str()).unwrap()
    else {
        panic!("expected exact tiered grid")
    };

    let expected = (1..=33)
        .map(|n| Decimal::new(3 * n - offset, 2))
        .collect::<Vec<_>>();

    assert_eq!(
        grid.ticks()
            .iter()
            .map(Price::as_decimal)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(instrument.price_increment, Price::from(increment));
    assert_eq!(native_peer.await.unwrap().len(), 2);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case("market", "ticker", json!("WRONG-TICKER"), "identity")]
#[case("market", "event_ticker", json!("WRONG-EVENT"), "identity")]
#[case("event", "event_ticker", json!("WRONG-EVENT"), "identity")]
#[case("market", "notional_value_dollars", json!("2.0000"), "Kalshi metadata")]
#[case("market", "market_type", json!("scalar"), "Kalshi metadata")]
#[case("market", "settlement_bounds_type", json!("fractional"), "Kalshi metadata")]
#[case("market", "settlement_floor_dollars", json!("0.5000"), "Kalshi metadata")]
#[case("market", "mve_collection_ticker", json!("COLLECTION"), "Kalshi metadata")]
#[case("market", "mve_selected_legs", json!([{}]), "Kalshi metadata")]
#[case("event", "collateral_return_type", json!("netted"), "Kalshi metadata")]
#[case("market", "price_ranges", json!([]), "Kalshi metadata")]
#[case("market", "price_ranges", json!([{"start":"0", "end":"1", "step":"1"}]), "Kalshi metadata")]
#[case("range", "start", json!("-0.0100"), "Kalshi metadata")]
#[case("range", "end", json!("0.2000"), "Kalshi metadata")]
#[case("range", "step", json!("0.0000"), "Kalshi metadata")]
#[case("range", "step", json!("0.0000001"), "Kalshi metadata")]
#[case("range", "step", json!("-0.1000"), "Kalshi metadata")]
#[case("market", "latest_expiration_time", json!("2025-01-01T00:00:00Z"), "Kalshi metadata")]
#[case("market", "open_time", json!("1960-01-01T00:00:00Z"), "Kalshi metadata")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_public_metadata_does_not_replace_registered_definition(
    #[case] scope: &str,
    #[case] field: &str,
    #[case] value: Value,
    #[case] failed_field: &str,
) {
    let mut native: Value = serde_json::from_str(NATIVE_MARKET).unwrap();
    let mut event: Value = serde_json::from_str(NATIVE_EVENT).unwrap();
    let target = match scope {
        "event" => &mut event["event"],
        "range" => &mut native["market"]["price_ranges"][0],
        _ => &mut native["market"],
    };

    target[field] = value;

    let (native_peer, native_http) = start_peer(vec![
        (200, "", NATIVE_MARKET.to_owned()),
        (200, "", NATIVE_EVENT.to_owned()),
        (200, "", native.to_string()),
        (200, "", event.to_string()),
    ])
    .await;
    let (peer, mut http) = start_peer(vec![(200, "", MARKET), (200, "", MARKET)]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let loader = FinFeedDataLoader::with_http_config(Some("test-api-key".into()), http);
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap();
    let before = loader.request_instrument(id).await.unwrap();
    let error = loader.request_instrument(id).await.unwrap_err();

    assert!(error.to_string().contains(failed_field), "{error}");
    assert_eq!(
        serde_json::to_value(loader.instrument(id).unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(native_peer.await.unwrap().len(), 4);
    assert_eq!(peer.await.unwrap().len(), 2);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn archived_metadata_is_used_only_after_a_missing_live_market() {
    let (native_peer, native_http) = start_peer(vec![
        (404, "", "missing live market"),
        (200, "", NATIVE_MARKET),
        (200, "", NATIVE_EVENT),
    ])
    .await;
    let (peer, mut http) = start_peer(vec![(200, "", MARKET)]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let loader = FinFeedDataLoader::with_http_config(Some("test-api-key".into()), http);
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap();
    let instrument = loader.request_instrument(id).await.unwrap();
    let requests = native_peer.await.unwrap();

    assert_eq!(instrument.id, id);
    assert!(requests[0].starts_with("GET /v1/markets/TEST-KALSHI-30 HTTP/1.1"));
    assert!(requests[1].starts_with("GET /v1/historical/markets/TEST-KALSHI-30 HTTP/1.1"));
    assert!(requests[2].starts_with("GET /v1/events/TEST-KALSHI HTTP/1.1"));
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn public_metadata_failure_is_identified_without_response_content() {
    let (native_peer, native_http) = start_peer(vec![(403, "", "private response sentinel")]).await;
    let (peer, mut http) = start_peer(vec![(200, "", MARKET)]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let http = Arc::new(FinFeedHttpClient::new(Some("test-api-key".into()), http).unwrap());
    let mut provider = FinFeedInstrumentProvider::new(http);
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap();
    let error = provider
        .request_instrument(id, UnixNanos::from(17))
        .await
        .unwrap_err();

    assert!(matches!(error, Error::KalshiMetadata));
    assert_eq!(error.to_string(), "Public Kalshi metadata request failed");
    assert!(!format!("{error:?}").contains("private response sentinel"));
    assert_eq!(provider.get(id), None);
    assert_eq!(native_peer.await.unwrap().len(), 1);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn public_metadata_retries_share_pacing_without_spending_finfeed_allowance() {
    let (native_peer, native_http) = start_peer(vec![
        (503, "", NATIVE_MARKET),
        (200, "", NATIVE_MARKET),
        (200, "", NATIVE_EVENT),
    ])
    .await;
    let (peer, mut config) = start_peer(vec![(200, "", MARKET)]).await;
    config.kalshi_metadata_url = Some(native_http.base_url);
    config.attempt_budget = 1;
    config.min_request_interval_ms = 20;
    let http = Arc::new(FinFeedHttpClient::new(Some("test-api-key".into()), config).unwrap());
    let mut provider = FinFeedInstrumentProvider::new(Arc::clone(&http));
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap();

    let started = time::Instant::now();
    let instrument = provider
        .request_instrument(id, UnixNanos::from(43))
        .await
        .unwrap();
    let elapsed = started.elapsed();

    let requests = native_peer.await.unwrap();

    assert_eq!(instrument.id, id);
    assert!(elapsed >= Duration::from_millis(60), "elapsed {elapsed:?}");
    assert_eq!(http.attempts_remaining(), 0);
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /v1/markets/TEST-KALSHI-30 HTTP/1.1"));
    assert!(requests[1].starts_with("GET /v1/markets/TEST-KALSHI-30 HTTP/1.1"));
    assert!(requests[2].starts_with("GET /v1/events/TEST-KALSHI HTTP/1.1"));

    for request in requests {
        assert!(!request.to_lowercase().contains("authorization:"));
        assert!(!request.to_lowercase().contains("x-referrer:"));
    }

    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_interrupts_public_metadata_and_sends_no_event_lookup() {
    let (native_peer, native_http, mut observed) =
        timed_peer(vec![(200, NATIVE_MARKET, Duration::from_secs(60))]).await;
    let (peer, mut config) = start_peer(vec![(200, "", MARKET)]).await;
    config.kalshi_metadata_url = Some(native_http.base_url);
    let http = Arc::new(FinFeedHttpClient::new(Some("test-api-key".into()), config).unwrap());
    let mut provider = FinFeedInstrumentProvider::new(Arc::clone(&http));
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap();
    let ts_init = UnixNanos::from(23);
    let pending = task::spawn(async move { provider.request_instrument(id, ts_init).await });
    assert_eq!(observed.recv().await.unwrap().0, 0);
    http.cancel();
    let result = time::timeout(Duration::from_secs(1), pending)
        .await
        .unwrap()
        .unwrap();

    assert!(matches!(result, Err(Error::KalshiMetadata)));
    assert!(observed.try_recv().is_err());
    native_peer.abort();
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn mixed_venue_reconnect_keeps_books_and_quotes_on_finfeed() {
    let (native_peer, native_http) = start_peer(vec![
        (200, "", NATIVE_MARKET),
        (200, "", NATIVE_EVENT),
        (200, "", NATIVE_MARKET),
        (200, "", NATIVE_EVENT),
    ])
    .await;
    let mut replies = Vec::new();
    for _ in 0..2 {
        replies.extend([
            (200, "", EXCHANGES),
            (
                200,
                "",
                include_str!("../../test_data/polymarket-market.json"),
            ),
            (200, "", MARKET),
            (200, "", include_str!("../../test_data/kalshi-book.json")),
            (
                200,
                "",
                include_str!("../../test_data/kalshi-activity.json"),
            ),
        ]);
    }

    let (peer, mut http) = start_peer(replies).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some("test-api-key".into()),
        http,
        instruments: vec![
            FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec!["TEST-OUTCOME_NO".into()],
            },
            FinFeedInstrumentSelection {
                exchange_id: "KALSHI".into(),
                market_ids: vec!["TEST-KALSHI-30_YES".into()],
            },
        ],
        poll_interval_ms: 86_400_000,
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap();

    for _ in 0..2 {
        client.connect().await.unwrap();

        for expected in [
            to_instrument_id("POLYMARKET", "TEST-OUTCOME_NO").unwrap(),
            id,
        ] {
            let DataEvent::Data(Data::Instrument(definition)) = receiver.try_recv().unwrap() else {
                panic!("expected definition")
            };

            assert_eq!(definition.id(), expected);
        }

        let correlation = UUID4::new();
        client
            .request_book_snapshot(RequestBookSnapshot::new(
                id,
                None,
                Some(client.client_id()),
                correlation,
                UnixNanos::from(31),
                None,
            ))
            .unwrap();

        let DataResponse::Book(book) = response(&mut receiver).await else {
            panic!("expected book response")
        };

        assert_eq!(book.correlation_id, correlation);
        assert_eq!(book.instrument_id, id);
        assert_eq!(book.data.instrument_id, id);
        assert_eq!(
            book.data.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![(
                "0.21".parse::<Decimal>().unwrap(),
                "13.25".parse::<Decimal>().unwrap()
            )]
        );
        assert_eq!(
            book.data.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![(
                "0.38".parse::<Decimal>().unwrap(),
                "29.75".parse::<Decimal>().unwrap()
            )]
        );
        client
            .subscribe_quotes(SubscribeQuotes::new(
                id,
                Some(client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::from(37),
                None,
                None,
            ))
            .unwrap();

        let DataEvent::Data(Data::Quote(quote)) =
            time::timeout(Duration::from_secs(3), receiver.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("expected FinFeed quote")
        };

        assert_eq!(quote.instrument_id, id);
        assert_eq!(quote.bid_price.precision, 6);
        assert_eq!(quote.ask_price.precision, 6);
        assert_eq!(quote.bid_size.precision, 2);
        assert_eq!(quote.ask_size.precision, 2);
        assert_eq!(quote.ts_event, UnixNanos::from(1_790_820_184_000_000_300));
        assert_eq!(quote.ts_init, UnixNanos::from(1_790_820_184_000_000_400));
        assert_eq!(
            quote.bid_price.as_decimal(),
            "0.27".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            quote.ask_price.as_decimal(),
            "0.39".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            quote.bid_size.as_decimal(),
            "17.25".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            quote.ask_size.as_decimal(),
            "23.75".parse::<Decimal>().unwrap()
        );
        client
            .unsubscribe_quotes(&UnsubscribeQuotes::new(
                id,
                Some(client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::from(41),
                None,
                None,
            ))
            .unwrap();
        client.disconnect().await.unwrap();
        assert!(client.is_disconnected());
        assert!(receiver.try_recv().is_err());
    }

    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 10);
    assert_eq!(native_peer.await.unwrap().len(), 4);

    for index in [3, 8] {
        assert!(
            requests[index]
                .starts_with("GET /v1/orderbook/KALSHI/TEST-KALSHI-30_YES/current HTTP/1.1")
        );
    }

    for index in [4, 9] {
        assert!(
            requests[index].starts_with(
                "GET /v1/activity/KALSHI/TEST-KALSHI-30_YES/latest?limit=100 HTTP/1.1"
            )
        );
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn no_outcome_uses_the_complementary_nonuniform_grid() {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market["market_id"] = "TEST-KALSHI-30_NO".into();
    market["outcome_name"] = "No".into();
    let mut native: Value = serde_json::from_str(NATIVE_MARKET).unwrap();
    native["market"]["price_ranges"] = json!([
        {"start":"0.0000", "end":"0.5000", "step":"0.0100"},
        {"start":"0.5000", "end":"1.0000", "step":"0.0200"},
    ]);
    let (native_peer, native_http) = start_peer(vec![
        (200, "", native.to_string()),
        (200, "", NATIVE_EVENT.to_owned()),
    ])
    .await;
    let (peer, mut http) = start_peer(vec![(200, "", market.to_string())]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let loader = FinFeedDataLoader::with_http_config(Some("test-api-key".into()), http);
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_NO").unwrap();
    let instrument = loader.request_instrument(id).await.unwrap();

    let TickScheme::Tiered(grid) =
        get_tick_scheme(instrument.tick_scheme.unwrap().as_str()).unwrap()
    else {
        panic!("expected complementary grid")
    };

    let expected = (1..25)
        .map(|n| Decimal::new(n * 2, 2))
        .chain((50..100).map(|n| Decimal::new(n, 2)))
        .collect::<Vec<_>>();

    assert_eq!(
        grid.ticks()
            .iter()
            .map(|price| price.as_decimal())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(instrument.outcome.unwrap().as_str(), "No");
    assert_eq!(native_peer.await.unwrap().len(), 2);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn no_outcome_preserves_non_aligned_band_ticks() {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market["market_id"] = "TEST-KALSHI-30_NO".into();
    market["outcome_name"] = "No".into();
    let mut native: Value = serde_json::from_str(NATIVE_MARKET).unwrap();
    native["market"]["price_ranges"] = json!([
        {"start":"0.0000", "end":"0.1000", "step":"0.0300"},
        {"start":"0.1000", "end":"1.0000", "step":"0.0100"},
    ]);
    let (native_peer, native_http) = start_peer(vec![
        (200, "", native.to_string()),
        (200, "", NATIVE_EVENT.to_owned()),
    ])
    .await;
    let (peer, mut http) = start_peer(vec![(200, "", market.to_string())]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let http = Arc::new(FinFeedHttpClient::new(Some("test-api-key".into()), http).unwrap());
    let mut provider = FinFeedInstrumentProvider::new(http);
    let id = to_instrument_id("KALSHI", "TEST-KALSHI-30_NO").unwrap();

    let instrument = provider
        .request_instrument(id, UnixNanos::from(47))
        .await
        .unwrap();

    let TickScheme::Tiered(grid) =
        get_tick_scheme(instrument.tick_scheme.unwrap().as_str()).unwrap()
    else {
        panic!("expected complementary grid")
    };

    let expected = (1..=90)
        .chain([91, 94, 97])
        .map(|n| Decimal::new(n, 2))
        .collect::<Vec<_>>();

    assert_eq!(
        grid.ticks()
            .iter()
            .map(Price::as_decimal)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(instrument.price_increment, Price::from("0.010000"));
    assert_eq!(provider.get(id), Some(&instrument));
    assert_eq!(native_peer.await.unwrap().len(), 2);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case("active", "Closed", "Closed")]
#[case("closed", "Open", "Closed")]
#[case("finalized", "Open", "Resolved")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn terminal_metadata_from_either_source_is_retained(
    #[case] native_status: &str,
    #[case] finfeed_status: &str,
    #[case] expected_status: &str,
) {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market["status"] = finfeed_status.into();
    let mut native: Value = serde_json::from_str(NATIVE_MARKET).unwrap();
    native["market"]["status"] = native_status.into();
    let (native_peer, native_http) = start_peer(vec![
        (200, "", native.to_string()),
        (200, "", NATIVE_EVENT.to_owned()),
    ])
    .await;
    let (peer, mut http) = start_peer(vec![(200, "", market.to_string())]).await;
    http.kalshi_metadata_url = Some(native_http.base_url);
    let loader = FinFeedDataLoader::with_http_config(Some("test-api-key".into()), http);
    let instrument = loader
        .request_instrument(to_instrument_id("KALSHI", "TEST-KALSHI-30_YES").unwrap())
        .await
        .unwrap();
    let info = instrument.info.unwrap();

    assert_eq!(info.get_str("status"), Some(expected_status));
    assert_eq!(info.get_str("finfeed_status"), Some(finfeed_status));
    assert_eq!(info["market"]["status"], json!(native_status));
    assert_eq!(native_peer.await.unwrap().len(), 2);
    assert_eq!(peer.await.unwrap().len(), 1);
}
