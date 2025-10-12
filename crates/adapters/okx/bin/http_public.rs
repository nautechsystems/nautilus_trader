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

use nautilus_model::{data::BarType, identifiers::InstrumentId};
use nautilus_okx::{common::enums::OKXInstrumentType, http::client::OKXHttpClient};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let mut client = OKXHttpClient::from_env().unwrap();

    // Request instruments
    let inst_type = OKXInstrumentType::Swap;
    let instruments = client.request_instruments(inst_type, None).await?;
    client.add_instruments(instruments);

    let inst_type = OKXInstrumentType::Spot;
    let instruments = client.request_instruments(inst_type, None).await?;
    client.add_instruments(instruments);

    // Request mark price
    let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");

    let resp = client.request_mark_price(instrument_id).await;
    match resp {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    // Request index price
    let instrument_id = InstrumentId::from("BTC-USDT.OKX");

    let resp = client.request_index_price(instrument_id).await;
    match resp {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    // Request trades
    let resp = client.request_trades(instrument_id, None, None, None).await;
    match resp {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    // Request bars
    let bar_type = BarType::from("BTC-USDT-SWAP.OKX-1-MINUTE-LAST-EXTERNAL");

    let resp = client.request_bars(bar_type, None, None, None).await;
    match resp {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    // let params = GetPositionTiersParamsBuilder::default()
    //     .instrument_type(OKXInstrumentType::Swap)
    //     .trade_mode(OKXTradeMode::Isolated)
    //     .instrument_family("BTC-USD")
    //     .build()?;
    // match client.http_get_position_tiers(params).await {
    //     Ok(resp) => tracing::debug!("{:?}", resp),
    //     Err(e) => tracing::error!("{e:?}"),
    // }
    //

    //
    // let params = GetPositionsParamsBuilder::default()
    //     .instrument_type(OKXInstrumentType::Swap)
    //     .build()?;
    // match client.http_get_positions(params).await {
    //     Ok(resp) => tracing::debug!("{:?}", resp),
    //     Err(e) => tracing::error!("{e:?}"),
    // }
    //
    // let params = GetPositionsHistoryParamsBuilder::default()
    //     .instrument_type(OKXInstrumentType::Swap)
    //     .build()?;
    // match client.http_get_position_history(params).await {
    //     Ok(resp) => tracing::debug!("{:?}", resp),
    //     Err(e) => tracing::error!("{e:?}"),
    // }

    Ok(())
}
