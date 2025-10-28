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

use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_okx::{
    common::enums::{OKXInstrumentType, OKXPositionMode},
    http::client::OKXHttpClient,
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let mut client = OKXHttpClient::from_env().unwrap();
    let account_id = AccountId::new("OKX-001");

    let inst_type = OKXInstrumentType::Swap;
    let instruments = client.request_instruments(inst_type, None).await?;
    client.add_instruments(instruments);

    // Set position mode
    let resp = client.set_position_mode(OKXPositionMode::NetMode).await;
    match resp {
        Ok(msg) => tracing::debug!("{:?}", msg),
        Err(e) => tracing::error!("{e:?}"),
    }

    // Request account state
    let resp = client.request_account_state(account_id).await;
    match resp {
        Ok(account_state) => tracing::debug!("{:?}", account_state),
        Err(e) => tracing::error!("{e:?}"),
    }

    // Request orders history
    let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");

    let result = client
        .request_order_status_reports(
            account_id,
            None,
            Some(instrument_id),
            None,
            None,
            false,
            None,
        )
        .await;
    match result {
        Ok(reports) => tracing::debug!("{:?}", reports),
        Err(e) => tracing::error!("{e:?}"),
    }

    let instrument_type = OKXInstrumentType::Swap;

    let result = client
        .request_position_status_reports(account_id, Some(instrument_type), None)
        .await;
    match result {
        Ok(reports) => tracing::debug!("{:?}", reports),
        Err(e) => tracing::error!("{e:?}"),
    }

    Ok(())

    // let params = GetOrderParamsBuilder::default()
    //     .symbol("XBTUSD".to_string())
    //     .build()?;
    // match client.get_orders(params).await {
    //     Ok(resp) => tracing::debug!("{:?}", resp),
    //     Err(e) => tracing::error!("{e:?}"),
    // }
    //
    // Ok(())
}
