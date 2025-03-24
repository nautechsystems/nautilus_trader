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

use std::collections::HashMap;

use futures_util::StreamExt;
use nautilus_coinbase_intx::{
    http::client::CoinbaseIntxHttpClient, websocket::client::CoinbaseIntxWebSocketClient,
};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;
use ustr::Ustr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let client = CoinbaseIntxHttpClient::from_env().unwrap();

    // Cache instruments first (required for correct websocket message parsing)
    let resp = client.request_instruments().await?;
    let mut instruments: HashMap<Ustr, InstrumentAny> = HashMap::new();
    for inst in resp {
        instruments.insert(inst.raw_symbol().inner(), inst);
    }

    let mut client = CoinbaseIntxWebSocketClient::default();
    client.connect(instruments).await?;

    let instrument_id = InstrumentId::from("BTC-PERP.COINBASE_INTX");

    // client.subscribe_instruments(vec![instrument_id]).await?;
    // client.subscribe_risk(vec![instrument_id]).await?;
    // client.subscribe_funding(vec![instrument_id]).await?;
    // client.subscribe_trades(vec![instrument_id]).await?;
    // client.subscribe_quotes(vec![instrument_id]).await?;
    client.subscribe_order_book(vec![instrument_id]).await?;

    // let bar_type = BarType::from("ETH-PERP.COINBASE_INTX-1-MINUTE-LAST-EXTERNAL");
    // client.subscribe_bars(bar_type).await?;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let stream = client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    loop {
        tokio::select! {
            Some(data) = stream.next() => {
                tracing::debug!("Received from stream: {data:?}");
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                client.close().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}
