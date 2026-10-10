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

//! Non-mutating live validation against the Derive v3 public testnet API.

use std::time::Duration;

use nautilus_common::providers::InstrumentProvider;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_derive::{
    common::{consts::REST_URL_TESTNET, enums::DeriveEnvironment},
    http::DeriveHttpClient,
    providers::DeriveInstrumentProvider,
    websocket::{
        DeriveWebSocketClient, DeriveWsMessage, parse_ticker_msg, parse_ticker_quote,
        parse_ticker_quote_from_rest,
    },
};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::TransportBackend;

#[tokio::test]
#[ignore = "requires Derive public testnet network access"]
async fn test_public_v3_discovery_and_market_data() {
    let validation = async {
        let http = DeriveHttpClient::new(REST_URL_TESTNET, Some(15), None, None).unwrap();
        let mut provider =
            DeriveInstrumentProvider::with_expired(http.clone(), vec!["ETH".to_string()], true);
        provider.load_all(None).await.unwrap();
        let perp_id = InstrumentId::from("ETH-PERP.DERIVE");
        let spot_id = InstrumentId::from("ETH-USDC.DERIVE");
        let perp = provider.store().find(&perp_id).unwrap();
        let spot = provider.store().find(&spot_id).unwrap();
        assert!(matches!(perp, InstrumentAny::CryptoPerpetual(_)));
        assert!(matches!(spot, InstrumentAny::CurrencyPair(_)));

        let option = provider
            .store()
            .list_all()
            .into_iter()
            .find(|instrument| {
                matches!(instrument, InstrumentAny::CryptoOption(_))
                    && instrument.info().unwrap().get("is_active")
                        == Some(&serde_json::Value::Bool(true))
            })
            .unwrap();

        let option_name = option.id().symbol.to_string();
        let definition = http.get_instrument(&option_name).await.unwrap();
        assert_eq!(definition.instrument_name.as_str(), option_name);
        assert_eq!(
            definition.base_asset_address.as_str(),
            option
                .info()
                .unwrap()
                .get_str("base_asset_address")
                .unwrap()
        );
        assert_eq!(
            definition.base_asset_sub_id.as_str(),
            option.info().unwrap().get_str("base_asset_sub_id").unwrap()
        );

        let ticker = http.get_ticker("ETH-PERP").await.unwrap();
        let quote = parse_ticker_quote_from_rest(
            &ticker,
            perp.price_precision(),
            perp.size_precision(),
            UnixNanos::from(1),
        )
        .unwrap();
        assert_eq!(quote.instrument_id, perp_id);
        assert_eq!(quote.bid_price.as_decimal(), ticker.best_bid_price);
        assert_eq!(quote.ask_price.as_decimal(), ticker.best_ask_price);
        assert_eq!(quote.bid_size.as_decimal(), ticker.best_bid_amount);
        assert_eq!(quote.ask_size.as_decimal(), ticker.best_ask_amount);
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(ticker.timestamp as u64 * 1_000_000)
        );
        assert!(ticker.funding_rate.is_some());
        assert!(ticker.stats.is_some());
        let option_ticker = http.get_ticker(&option_name).await.unwrap();
        assert!(option_ticker.option_pricing.is_some());
        let spot_ticker = http.get_ticker("ETH-USDC").await.unwrap();
        assert_eq!(spot_ticker.funding_rate, None);
        assert!(spot_ticker.option_pricing.is_none());

        let end_ms = (get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000) as i64;
        let trades = http
            .get_trade_history("ETH-PERP", Some(end_ms - 86_400_000), Some(end_ms), 1, 10)
            .await
            .unwrap();
        let funding = http
            .get_funding_rate_history(
                "ETH-PERP",
                Some(end_ms - 86_400_000),
                Some(end_ms),
                Some(3600),
            )
            .await
            .unwrap();
        let candles = http
            .get_candles("ETH-PERP", end_ms / 1000 - 86_400, end_ms / 1000, 3600)
            .await
            .unwrap();
        assert!(!funding.funding_rate_history.is_empty());

        let mut ws = DeriveWebSocketClient::new(
            None,
            DeriveEnvironment::Testnet,
            TransportBackend::default(),
            None,
        );
        ws.connect().await.unwrap();
        let mut events = ws.take_event_receiver().unwrap();
        ws.subscribe_ticker("ETH-PERP", "1000").await.unwrap();

        let payload = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let DeriveWsMessage::Subscription(payload) = events.recv().await.unwrap() {
                    break payload;
                }
            }
        })
        .await
        .unwrap();

        let ticker_msg = parse_ticker_msg(&payload).unwrap();
        let quote = parse_ticker_quote(
            &ticker_msg,
            perp.price_precision(),
            perp.size_precision(),
            UnixNanos::from(2),
        )
        .unwrap();
        assert_eq!(quote.instrument_id, perp_id);
        assert_eq!(
            quote.ts_event,
            UnixNanos::from(ticker_msg.data.timestamp() as u64 * 1_000_000)
        );
        ws.disconnect().await.unwrap();
        println!(
            "Derive public testnet: {} instruments; REST perp/option/spot tickers; {} trades, {} funding samples, {} candles; WS slim ticker parsed",
            provider.store().count(),
            trades.trades.len(),
            funding.funding_rate_history.len(),
            candles.len()
        );
    };

    tokio::time::timeout(Duration::from_secs(90), validation)
        .await
        .expect("public testnet validation exceeds 90 seconds");
}
