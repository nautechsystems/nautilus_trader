// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_adapters::tardis::{enums::Exchange, http::client::TardisHttpClient};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let client = TardisHttpClient::new(None, None, None).unwrap();

    // Tardis instrument definitions
    let resp = client.instruments_info(Exchange::Bitmex).await;
    println!("Received: {resp:?}");

    let resp = client.instrument_info(Exchange::Bitmex, "ETHUSDT").await;
    println!("Received: {resp:?}");

    // Nautilus instrument definitions
    let resp = client.instruments(Exchange::Bitmex).await;
    println!("Received: {resp:?}");

    let resp = client.instrument(Exchange::Bitmex, "ETHUSDT").await;
    println!("Received: {resp:?}");
}
