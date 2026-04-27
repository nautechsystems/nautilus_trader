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

//! Connects to the Bybit public WebSocket feed and streams specified market data topics.
//! Useful when manually validating the Rust WebSocket client implementation.

use std::error::Error;

use futures_util::StreamExt;
use nautilus_bybit::{
    common::enums::{BybitEnvironment, BybitProductType},
    websocket::{client::BybitWebSocketClient, messages::BybitWsMessage},
};
use nautilus_network::websocket::TransportBackend;
use tokio::{pin, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        None,
        20,
        TransportBackend::default(),
        None,
    );
    client.connect().await?;

    client
        .subscribe(vec![
            "orderbook.1.BTCUSDT".to_string(),
            "publicTrade.BTCUSDT".to_string(),
            "tickers.BTCUSDT".to_string(),
        ])
        .await?;

    let stream = client.stream();
    let shutdown = signal::ctrl_c();
    pin!(stream);
    pin!(shutdown);

    log::info!("Streaming Bybit market data; press Ctrl+C to exit");

    loop {
        tokio::select! {
            Some(event) = stream.next() => {
                match event {
                    BybitWsMessage::Orderbook(msg) => {
                        log::info!(
                            "orderbook: topic={}, type={}, count={}",
                            msg.topic, msg.msg_type, msg.data.b.len() + msg.data.a.len()
                        );
                    }
                    BybitWsMessage::Trade(msg) => {
                        for trade in &msg.data {
                            log::info!(
                                "trade: symbol={}, price={}, size={}, side={:?}",
                                trade.s, trade.p, trade.v, trade.taker_side
                            );
                        }
                    }
                    BybitWsMessage::Kline(msg) => {
                        for kline in &msg.data {
                            log::info!(
                                "kline: topic={}, close={}, confirm={}",
                                msg.topic, kline.close, kline.confirm
                            );
                        }
                    }
                    BybitWsMessage::TickerLinear(msg) => {
                        log::info!(
                            "ticker linear: symbol={}, last_price={:?}, mark_price={:?}",
                            msg.data.symbol, msg.data.last_price, msg.data.mark_price
                        );
                    }
                    BybitWsMessage::TickerOption(msg) => {
                        log::info!(
                            "ticker option: symbol={}, bid={}, ask={}",
                            msg.data.symbol, msg.data.bid_price, msg.data.ask_price
                        );
                    }
                    BybitWsMessage::Error(err) => {
                        log::error!("WebSocket error: code={}, message={}", err.code, err.message);
                    }
                    BybitWsMessage::Reconnected => {
                        log::warn!("WebSocket reconnected");
                    }
                    BybitWsMessage::Auth(result) => {
                        log::info!("Auth result: success={:?}", result.success);
                    }
                    _ => {
                        log::trace!("Other message received");
                    }
                }
            }
            _ = &mut shutdown => {
                log::info!("Received Ctrl+C, closing connection");
                client.close().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}
