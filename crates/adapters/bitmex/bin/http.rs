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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::env;

use nautilus_bitmex::http::client::BitmexHttpClient;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let api_key = env::var("BITMEX_API_KEY").expect("environment variable should be set");
    let api_secret = env::var("BITMEX_API_SECRET").expect("environment variable should be set");
    let client = BitmexHttpClient::new(
        None,
        Some(api_key),
        Some(api_secret),
        false,
        None,
        None, // max_retries
        None, // retry_delay_ms
        None, // retry_delay_max_ms
    )
    .expect("Failed to create HTTP client");

    Ok(())
}
