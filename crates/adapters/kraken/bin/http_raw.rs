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

use nautilus_kraken::http::client::KrakenRawHttpClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .finish(),
    )?;

    tracing::info!("Kraken HTTP client example");

    let client = KrakenRawHttpClient::default();

    tracing::info!("Fetching server time...");
    let server_time = client.get_server_time().await?;
    tracing::info!("Server time: {:?}", server_time);

    tracing::info!("Fetching system status...");
    let status = client.get_system_status().await?;
    tracing::info!("System status: {:?}", status);

    tracing::info!("Fetching asset pairs for BTC/USD...");
    let pairs = client
        .get_asset_pairs(Some(vec!["XBTUSDT".to_string()]))
        .await?;
    tracing::info!("Asset pairs count: {}", pairs.len());

    tracing::info!("Fetching ticker for BTC/USD...");
    let ticker = client.get_ticker(vec!["XBTUSDT".to_string()]).await?;
    tracing::info!("Ticker count: {}", ticker.len());

    tracing::info!("HTTP client example completed successfully");

    Ok(())
}
