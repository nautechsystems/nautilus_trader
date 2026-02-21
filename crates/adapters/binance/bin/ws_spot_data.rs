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

//! Test binary for Binance Spot WebSocket SBE market data streams.
//!
//! Tests trades and quotes (best bid/ask) streams against the live Binance SBE endpoint.
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin binance-spot-ws-data --package nautilus-binance
//! ```
//!
//! # Environment Variables
//!
//! Ed25519 authentication is **required** for SBE streams:
//! - `BINANCE_API_KEY`: Ed25519 API key (required)
//! - `BINANCE_API_SECRET`: Ed25519 private key in PEM format (required)

use futures_util::StreamExt;
use nautilus_binance::{
    common::{enums::BinanceEnvironment, sbe::stream::mantissa_to_f64},
    spot::{
        http::client::BinanceSpotHttpClient,
        websocket::streams::{
            client::BinanceSpotWebSocketClient,
            messages::{BinanceSpotWsMessage, NautilusSpotDataWsMessage},
            parse::{MarketDataMessage, decode_market_data},
        },
    },
};
use nautilus_model::data::Data;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    // Read credentials from environment (required for SBE streams)
    let api_key = std::env::var("BINANCE_API_KEY").ok();
    let api_secret = std::env::var("BINANCE_API_SECRET").ok();

    if api_key.is_none() || api_secret.is_none() {
        log::error!("Ed25519 credentials required for Binance SBE streams");
        log::error!("Set BINANCE_API_KEY and BINANCE_API_SECRET environment variables");
        anyhow::bail!("Missing required Ed25519 credentials");
    }
    log::info!("Using Ed25519 authentication for SBE streams");

    log::info!("Fetching instruments from Binance Spot API...");
    let http_client = BinanceSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None, // api_key (not needed for public endpoints)
        None, // api_secret
        None, // base_url_override
        None, // recv_window
        None, // timeout_secs
        None, // proxy_url
    )?;

    let instruments = http_client.request_instruments().await?;
    log::info!("Parsed instruments: count={}", instruments.len());

    log::info!("Creating Binance Spot WebSocket client...");
    let mut ws_client = BinanceSpotWebSocketClient::new(
        None, // url (default SBE endpoint)
        api_key, api_secret, None, // heartbeat
    )?;

    ws_client.cache_instruments(instruments);

    log::info!("Connecting to Binance Spot SBE WebSocket...");
    ws_client.connect().await?;

    // Subscribe to trades and quotes for BTC and ETH
    let streams = vec![
        "btcusdt@trade".to_string(),
        "ethusdt@trade".to_string(),
        "btcusdt@bestBidAsk".to_string(),
        "ethusdt@bestBidAsk".to_string(),
    ];
    log::info!("Subscribing to streams: {streams:?}");
    ws_client.subscribe(streams).await?;

    log::info!("Listening for messages (Ctrl+C to stop)...");

    let stream = ws_client.stream();
    tokio::pin!(stream);

    let mut message_count = 0u64;
    let mut trade_count = 0u64;
    let mut quote_count = 0u64;
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                message_count += 1;

                match msg {
                    BinanceSpotWsMessage::Data(data_msg) => match data_msg {
                        NautilusSpotDataWsMessage::Data(data_vec) => {
                            for data in &data_vec {
                                match data {
                                    Data::Trade(trade) => {
                                        trade_count += 1;
                                        log::info!(
                                            "Trade: msg={message_count}, instrument={}, price={}, size={}, side={:?}, trade_id={}",
                                            trade.instrument_id,
                                            trade.price,
                                            trade.size,
                                            trade.aggressor_side,
                                            trade.trade_id
                                        );
                                    }
                                    Data::Quote(quote) => {
                                        quote_count += 1;
                                        log::info!(
                                            "Quote: msg={message_count}, instrument={}, bid={}, ask={}, bid_size={}, ask_size={}",
                                            quote.instrument_id,
                                            quote.bid_price,
                                            quote.ask_price,
                                            quote.bid_size,
                                            quote.ask_size
                                        );
                                    }
                                    _ => {
                                        log::debug!("Other data: msg={message_count}, data={data:?}");
                                    }
                                }
                            }
                        }
                        NautilusSpotDataWsMessage::Deltas(deltas) => {
                            log::info!(
                                "OrderBook deltas: msg={message_count}, instrument={}, num_deltas={}",
                                deltas.instrument_id,
                                deltas.deltas.len()
                            );
                        }
                        NautilusSpotDataWsMessage::RawBinary(data) => {
                            match decode_and_display_sbe(&data) {
                                Ok(()) => {}
                                Err(e) => {
                                    log::warn!(
                                        "Raw binary (decode failed): msg={message_count}, len={}, error={e}",
                                        data.len()
                                    );
                                }
                            }
                        }
                        NautilusSpotDataWsMessage::RawJson(json) => {
                            log::debug!("Raw JSON: msg={message_count}, json={json}");
                        }
                        NautilusSpotDataWsMessage::Instrument(inst) => {
                            log::info!("Instrument: {inst:?}");
                        }
                    },
                    BinanceSpotWsMessage::Error(err) => {
                        log::error!("WebSocket error: code={}, msg={}", err.code, err.msg);
                    }
                    BinanceSpotWsMessage::Reconnected => {
                        log::warn!("WebSocket reconnected");
                    }
                }

                if message_count.is_multiple_of(50) {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let rate = message_count as f64 / elapsed;
                    log::info!(
                        "Statistics: messages={message_count}, trades={trade_count}, quotes={quote_count}, elapsed_secs={elapsed:.1}, rate={rate:.1}/s"
                    );
                }
            }
            _ = tokio::signal::ctrl_c() => {
                log::info!("Received Ctrl+C, shutting down...");
                break;
            }
        }
    }

    ws_client.close().await?;

    let elapsed = start_time.elapsed().as_secs_f64();
    let avg_rate = message_count as f64 / elapsed;
    log::info!(
        "Final statistics: total_messages={message_count}, trades={trade_count}, quotes={quote_count}, elapsed_secs={elapsed:.1}, avg_rate={avg_rate:.1}/s"
    );

    Ok(())
}

/// Decode and display raw SBE binary data.
fn decode_and_display_sbe(data: &[u8]) -> anyhow::Result<()> {
    match decode_market_data(data)? {
        MarketDataMessage::Trades(event) => {
            for trade in &event.trades {
                let price = mantissa_to_f64(trade.price_mantissa, event.price_exponent);
                let qty = mantissa_to_f64(trade.qty_mantissa, event.qty_exponent);
                let side = if trade.is_buyer_maker { "SELL" } else { "BUY" };
                let ts = chrono::DateTime::from_timestamp_micros(event.transact_time_us)
                    .map_or_else(
                        || "?".to_string(),
                        |dt| dt.format("%H:%M:%S%.6f").to_string(),
                    );

                log::info!(
                    "Trade (raw SBE): symbol={}, side={side}, price={price:.2}, qty={qty:.6}, id={}, time={ts}",
                    event.symbol,
                    trade.id
                );
            }
        }
        MarketDataMessage::BestBidAsk(event) => {
            let bid = mantissa_to_f64(event.bid_price_mantissa, event.price_exponent);
            let ask = mantissa_to_f64(event.ask_price_mantissa, event.price_exponent);
            let bid_size = mantissa_to_f64(event.bid_qty_mantissa, event.qty_exponent);
            let ask_size = mantissa_to_f64(event.ask_qty_mantissa, event.qty_exponent);
            let ts = chrono::DateTime::from_timestamp_micros(event.event_time_us).map_or_else(
                || "?".to_string(),
                |dt| dt.format("%H:%M:%S%.6f").to_string(),
            );

            log::info!(
                "Quote (raw SBE): symbol={}, bid={bid:.2}, ask={ask:.2}, bid_size={bid_size:.6}, ask_size={ask_size:.6}, time={ts}",
                event.symbol
            );
        }
        MarketDataMessage::DepthSnapshot(event) => {
            log::info!(
                "Depth snapshot (raw SBE): symbol={}, bids={}, asks={}",
                event.symbol,
                event.bids.len(),
                event.asks.len()
            );
        }
        MarketDataMessage::DepthDiff(event) => {
            log::info!(
                "Depth diff (raw SBE): symbol={}, bids={}, asks={}, first_update_id={}, last_update_id={}",
                event.symbol,
                event.bids.len(),
                event.asks.len(),
                event.first_book_update_id,
                event.last_book_update_id
            );
        }
    }

    Ok(())
}
