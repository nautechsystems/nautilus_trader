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

use std::env;

use nautilus_bitmex::http::{
    client::BitmexHttpClient,
    parse::parse_instrument_any,
    query::{
        GetExecutionParamsBuilder, GetOrderParamsBuilder, GetPositionParamsBuilder,
        GetTradeParamsBuilder,
    },
};
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{identifiers::InstrumentId, instruments::any::InstrumentAny};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let api_key = env::var("BITMEX_API_KEY").expect("environment variable should be set");
    let api_secret = env::var("BITMEX_API_SECRET").expect("environment variable should be set");
    let client = BitmexHttpClient::new(None, Some(api_key), Some(api_secret), false, None);

    match client.get_instruments(false).await {
        Ok(resp) => {
            tracing::debug!("{:?}", resp);
            let ts_init = get_atomic_clock_realtime().get_time_ns();
            let mut instruments: Vec<InstrumentAny> = Vec::new();
            for def in resp {
                tracing::debug!("Parsing {def:?}");
                if let Some(inst) = parse_instrument_any(&def, ts_init) {
                    instruments.push(inst);
                } else {
                    tracing::warn!(
                        "Did not parse: symbol={}, type={}",
                        def.symbol,
                        def.instrument_type,
                    );
                }
            }
        }
        Err(e) => tracing::error!("{e:?}"),
    }

    let instrument_id = InstrumentId::from("XBTUSD.BITMEX");

    match client.get_instrument(&instrument_id.symbol).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    let resp = client.get_wallet().await;
    match resp {
        Ok(instrument) => tracing::debug!("{:?}", instrument),
        Err(e) => tracing::error!("{e:?}"),
    }

    let params = GetOrderParamsBuilder::default()
        .symbol("XBTUSD".to_string())
        .build()?;
    match client.get_orders(params).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    let params = GetTradeParamsBuilder::default()
        .symbol("XBTUSD".to_string())
        .build()?;
    match client.get_trades(params).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    let params = GetExecutionParamsBuilder::default()
        .symbol("XBTUSD".to_string())
        .build()?;
    match client.get_executions(params).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    let params = GetPositionParamsBuilder::default().build()?;
    match client.get_positions(params).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    Ok(())
}
