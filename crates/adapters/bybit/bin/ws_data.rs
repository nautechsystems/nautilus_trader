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
    websocket::{client::BybitWebSocketClient, messages::NautilusWsMessage},
};
use nautilus_model::data::Data;
use tokio::{pin, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        None,
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
                    NautilusWsMessage::Data(data_vec) => {
                        log::info!("data update: count={}", data_vec.len());
                        for data in data_vec {
                            match data {
                                Data::Trade(tick) => {
                                    log::info!("trade: instrument={}, price={}, size={}", tick.instrument_id, tick.price, tick.size);
                                }
                                Data::Quote(quote) => {
                                    log::info!("quote: instrument={}, bid={}, ask={}", quote.instrument_id, quote.bid_price, quote.ask_price);
                                }
                                Data::Bar(bar) => {
                                    log::info!("bar: bar_type={}, close={}", bar.bar_type, bar.close);
                                }
                                _ => {
                                    log::debug!("other data type");
                                }
                            }
                        }
                    }
                    NautilusWsMessage::Deltas(deltas) => {
                        log::info!("orderbook deltas: instrument={}, sequence={}", deltas.instrument_id, deltas.sequence);
                    }
                    NautilusWsMessage::FundingRates(rates) => {
                        log::info!("funding rate updates: count={}", rates.len());
                        for rate in rates {
                            log::info!(
                                "funding rate: instrument={}, rate={}, next_funding={:?}",
                                rate.instrument_id, rate.rate, rate.next_funding_ns
                            );
                        }
                    }
                    NautilusWsMessage::OrderStatusReports(reports) => {
                        log::info!("order status reports: count={}", reports.len());
                        for report in reports {
                            log::info!(
                                "order status report: instrument={}, client_order_id={:?}, venue_order_id={:?}, status={:?}",
                                report.instrument_id, report.client_order_id, report.venue_order_id, report.order_status
                            );
                        }
                    }
                    NautilusWsMessage::FillReports(reports) => {
                        log::info!("fill reports: count={}", reports.len());
                        for report in reports {
                            log::info!(
                                "fill report: instrument={}, client_order_id={:?}, venue_order_id={:?}, last_qty={}, last_px={}",
                                report.instrument_id, report.client_order_id, report.venue_order_id, report.last_qty, report.last_px
                            );
                        }
                    }
                    NautilusWsMessage::PositionStatusReport(report) => {
                        log::info!("position status report: instrument={}, quantity={}", report.instrument_id, report.quantity);
                    }
                    NautilusWsMessage::AccountState(state) => {
                        log::info!("account state: account_id={}, balances={}", state.account_id, state.balances.len());
                    }
                    NautilusWsMessage::OrderRejected(event) => {
                        log::warn!("order rejected: trader_id={}, client_order_id={}, reason={}", event.trader_id, event.client_order_id, event.reason);
                    }
                    NautilusWsMessage::OrderCancelRejected(event) => {
                        log::warn!("order cancel rejected: trader_id={}, client_order_id={}, reason={}", event.trader_id, event.client_order_id, event.reason);
                    }
                    NautilusWsMessage::OrderModifyRejected(event) => {
                        log::warn!("order modify rejected: trader_id={}, client_order_id={}, reason={}", event.trader_id, event.client_order_id, event.reason);
                    }
                    NautilusWsMessage::Error(err) => {
                        log::error!("websocket error: code={}, message={}", err.code, err.message);
                    }
                    NautilusWsMessage::Reconnected => {
                        log::warn!("WebSocket reconnected");
                    }
                    NautilusWsMessage::Authenticated => {
                        log::info!("Authenticated successfully");
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
