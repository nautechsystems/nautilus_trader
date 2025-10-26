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

use std::time::Duration;

use futures_util::StreamExt;
use nautilus_model::{
    enums::{OrderSide, OrderType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_okx::{
    common::enums::{OKXInstrumentType, OKXTradeMode},
    http::OKXHttpClient,
    websocket::client::OKXWebSocketClient,
};
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let rest_client = OKXHttpClient::from_env().unwrap();

    let inst_type = OKXInstrumentType::Swap;
    let instruments = rest_client.request_instruments(inst_type, None).await?;

    let mut ws_client = OKXWebSocketClient::from_env().unwrap();
    ws_client.initialize_instruments_cache(instruments.clone());
    ws_client.connect().await?;

    // Subscribe to execution channels: orders and account updates
    ws_client.subscribe_orders(inst_type).await?;
    // ws_client.subscribe_account().await?;

    // Wait briefly to ensure subscriptions are active
    tokio::time::sleep(Duration::from_secs(1)).await;

    let trader_id = TraderId::from("TRADER-001");
    let strategy_id = StrategyId::from("SCALPER-001");
    let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
    let client_order_id = ClientOrderId::from("O20250711001");
    let order_side = OrderSide::Buy;
    let order_type = OrderType::Market;
    let quantity = Quantity::from("0.01");

    let resp = ws_client
        .submit_order(
            trader_id,
            strategy_id,
            instrument_id,
            OKXTradeMode::Isolated,
            client_order_id,
            order_side,
            order_type,
            quantity,
            None, // time_in_force
            None, // price
            None, // trigger_price
            None, // post_only
            None, // reduce_only
            None, // quote_quantity
            None, // position_side
        )
        .await;

    match resp {
        Ok(resp) => tracing::debug!("{resp:?}"),
        Err(e) => tracing::error!("{e:?}"),
    }

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let stream = ws_client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    loop {
        tokio::select! {
            Some(data) = stream.next() => {
                tracing::debug!("{data:?}");
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                ws_client.close().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}
