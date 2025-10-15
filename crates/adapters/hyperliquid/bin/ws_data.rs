// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{env, error::Error, time::Duration};

use nautilus_hyperliquid::{
    common::consts::ws_url,
    websocket::{
        client::HyperliquidWebSocketInnerClient,
        messages::{HyperliquidWsMessage, SubscriptionRequest},
    },
};
use tokio::{pin, signal, time::sleep};
use tracing::level_filters::LevelFilter;
use ustr::Ustr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let log_level = env::var("LOG_LEVEL")
        .unwrap_or_else(|_| "INFO".to_string())
        .parse::<LevelFilter>()
        .unwrap_or(LevelFilter::INFO);

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .compact()
        .init();

    let args: Vec<String> = env::args().collect();
    let subscription_type = args.get(1).map_or("all", String::as_str);
    let symbol = args.get(2).map_or("BTC", String::as_str);
    let testnet = args.get(3).is_some_and(|s| s == "testnet");

    tracing::info!("Starting Hyperliquid WebSocket test");
    tracing::info!("Subscription type: {subscription_type}");
    tracing::info!("Symbol: {symbol}");
    tracing::info!("Network: {}", if testnet { "testnet" } else { "mainnet" });

    let url = ws_url(testnet);
    let mut client = HyperliquidWebSocketInnerClient::connect(url).await?;

    // Give the connection a moment to stabilize
    sleep(Duration::from_millis(500)).await;

    let coin = Ustr::from(symbol);
    tracing::info!("Using symbol: {symbol}");

    match subscription_type {
        "trades" => {
            tracing::info!("Subscribing to trades for {symbol}");
            let subscription = SubscriptionRequest::Trades { coin };
            client.ws_subscribe(subscription).await?;
        }
        "book" | "l2book" | "orderbook" => {
            tracing::info!("Subscribing to L2 book for {symbol}");
            let subscription = SubscriptionRequest::L2Book {
                coin,
                n_sig_figs: None,
                mantissa: None,
            };
            client.ws_subscribe(subscription).await?;
        }
        "candles" | "klines" => {
            tracing::info!("Subscribing to candles for {symbol}");
            let subscription = SubscriptionRequest::Candle {
                coin,
                interval: "1m".to_string(),
            };
            client.ws_subscribe(subscription).await?;
        }
        "allmids" => {
            tracing::info!("Subscribing to all mids");
            let subscription = SubscriptionRequest::AllMids { dex: None };
            client.ws_subscribe(subscription).await?;
        }
        "bbo" => {
            tracing::info!("Subscribing to best bid/offer for {symbol}");
            let subscription = SubscriptionRequest::Bbo { coin };
            client.ws_subscribe(subscription).await?;
        }
        "all" => {
            tracing::info!("Subscribing to all available data types for {symbol}");

            tracing::info!("- Subscribing to trades");
            let subscription = SubscriptionRequest::Trades { coin };
            if let Err(e) = client.ws_subscribe(subscription).await {
                tracing::error!("Failed to subscribe to trades: {e}");
            } else {
                tracing::info!("  ✓ Trades subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to L2 book");
            let subscription = SubscriptionRequest::L2Book {
                coin,
                n_sig_figs: None,
                mantissa: None,
            };
            if let Err(e) = client.ws_subscribe(subscription).await {
                tracing::error!("Failed to subscribe to L2 book: {e}");
            } else {
                tracing::info!("  ✓ L2 book subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to best bid/offer");
            let subscription = SubscriptionRequest::Bbo { coin };
            if let Err(e) = client.ws_subscribe(subscription).await {
                tracing::error!("Failed to subscribe to BBO: {e}");
            } else {
                tracing::info!("  ✓ BBO subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to candles");
            let subscription = SubscriptionRequest::Candle {
                coin,
                interval: "1m".to_string(),
            };
            if let Err(e) = client.ws_subscribe(subscription).await {
                tracing::error!("Failed to subscribe to candles: {e}");
            } else {
                tracing::info!("  ✓ Candles subscription successful");
            }
        }
        _ => {
            tracing::error!("Unknown subscription type: {subscription_type}");
            tracing::info!("Available types: trades, book, candles, allmids, bbo, all");
            tracing::info!("Usage: {} <subscription_type> <symbol> [testnet]", args[0]);
            tracing::info!("Example: {} trades BTC testnet", args[0]);
            return Ok(());
        }
    }

    tracing::info!("Subscriptions completed, waiting for data...");
    tracing::info!("Press CTRL+C to stop");

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    // Use a flag to track if we should close
    let mut should_close = false;
    let mut message_count = 0u64;

    loop {
        tokio::select! {
            event = client.ws_next_event() => {
                match event {
                    Some(HyperliquidWsMessage::Trades { data }) => {
                        message_count += 1;
                        tracing::info!(
                            "[Message #{message_count}] Trade update: {} trades",
                            data.len()
                        );
                        for trade in &data {
                            tracing::debug!(
                                coin = %trade.coin,
                                side = %trade.side,
                                px = %trade.px,
                                sz = %trade.sz,
                                time = trade.time,
                                tid = trade.tid,
                                "trade"
                            );
                        }
                    }
                    Some(HyperliquidWsMessage::L2Book { data }) => {
                        message_count += 1;
                        tracing::info!(
                            "[Message #{message_count}] L2 book update: coin={}, levels={}",
                            data.coin,
                            data.levels.len()
                        );
                        tracing::debug!(
                            coin = %data.coin,
                            time = data.time,
                            bids = data.levels[0].len(),
                            asks = data.levels[1].len(),
                            "L2 book"
                        );
                    }
                    Some(HyperliquidWsMessage::Bbo { data }) => {
                        message_count += 1;
                        tracing::info!(
                            "[Message #{message_count}] BBO update: coin={}",
                            data.coin
                        );
                        let bid = data.bbo[0].as_ref().map_or("None", |l| l.px.as_str());
                        let ask = data.bbo[1].as_ref().map_or("None", |l| l.px.as_str());
                        tracing::debug!(
                            coin = %data.coin,
                            bid = bid,
                            ask = ask,
                            time = data.time,
                            "BBO"
                        );
                    }
                    Some(HyperliquidWsMessage::AllMids { data }) => {
                        message_count += 1;
                        tracing::info!(
                            "[Message #{message_count}] All mids update: {} coins",
                            data.mids.len()
                        );
                        for (coin, mid) in &data.mids {
                            tracing::debug!(coin = %coin, mid = %mid, "mid price");
                        }
                    }
                    Some(HyperliquidWsMessage::Candle { data }) => {
                        message_count += 1;
                        tracing::info!("[Message #{message_count}] Candle update");
                        tracing::debug!(
                            symbol = %data.s,
                            interval = %data.i,
                            time = data.t,
                            open = %data.o,
                            high = %data.h,
                            low = %data.l,
                            close = %data.c,
                            volume = %data.v,
                            trades = data.n,
                            "candle data"
                        );
                    }
                    Some(HyperliquidWsMessage::SubscriptionResponse { data: sub_data }) => {
                        tracing::info!(
                            "Subscription response received: method={}, type={:?}",
                            sub_data.method,
                            sub_data.subscription
                        );
                    }
                    Some(HyperliquidWsMessage::Pong) => {
                        tracing::trace!("Received pong");
                    }
                    Some(event) => {
                        message_count += 1;
                        tracing::debug!("[Message #{message_count}] Other message: {event:?}");
                    }
                    None => {
                        tracing::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }
            _ = &mut sigint => {
                tracing::info!("Received CTRL+C, closing connection...");
                should_close = true;
                break;
            }
        }
    }

    if should_close {
        tracing::info!("Total messages received: {message_count}");
        client.ws_disconnect().await?;
        tracing::info!("Connection closed successfully");
    }

    Ok(())
}
