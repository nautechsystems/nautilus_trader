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

use nautilus_bitmex::{
    enums::{ExecInstruction, OrderType, Side},
    http::{
        client::BitmexHttpClient,
        query::{DeleteOrderParamsBuilder, GetOrderParamsBuilder, PostOrderParamsBuilder},
    },
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let api_key = env::var("BITMEX_API_KEY").expect("environment variable should be set");
    let api_secret = env::var("BITMEX_API_SECRET").expect("environment variable should be set");
    let client = BitmexHttpClient::new(None, Some(api_key), Some(api_secret), false, None);

    let cl_ord_id = "2024-12-03-002";

    let params = PostOrderParamsBuilder::default()
        .symbol("XBTUSD".to_string())
        .cl_ord_id(cl_ord_id)
        .ord_type(OrderType::Limit)
        .side(Side::Sell)
        .order_qty(100_u32)
        .price(100_000.0)
        .exec_inst(vec![ExecInstruction::ParticipateDoNotInitiate])
        .build()?;
    match client.place_order(params).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    let params = DeleteOrderParamsBuilder::default()
        .cl_ord_id(vec![cl_ord_id.to_string()])
        .build()?;
    match client.cancel_orders(params).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    let params = GetOrderParamsBuilder::default()
        .symbol("XBTUSD".to_string())
        .build()?;
    match client.get_orders(params).await {
        Ok(resp) => tracing::debug!("{:?}", resp),
        Err(e) => tracing::error!("{e:?}"),
    }

    Ok(())
}
