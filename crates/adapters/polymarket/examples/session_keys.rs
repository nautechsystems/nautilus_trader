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

//! Owner-operated Deposit Wallet session administration.
//!
//! Run `cargo run -p nautilus-polymarket --example polymarket-session-keys -- list`.
//! Use `authorize ADDRESS` or `revoke ADDRESS` to change access.
//! Credentials are supplied through the Polymarket owner and Builder environment variables.

use nautilus_core::string::secret::SecretString;
use nautilus_polymarket::session::{PolymarketSessionKeyClient, PolymarketSessionKeyClientConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        matches!(args.as_slice(), [action] if action == "list")
            || matches!(args.as_slice(), [action, _] if action == "authorize" || action == "revoke"),
        "Usage: session_keys list | authorize ADDRESS | revoke ADDRESS"
    );

    let client = PolymarketSessionKeyClient::new(PolymarketSessionKeyClientConfig {
        private_key: secret("POLYMARKET_PK")?,
        api_key: secret("POLYMARKET_API_KEY")?,
        api_secret: secret("POLYMARKET_API_SECRET")?,
        passphrase: secret("POLYMARKET_PASSPHRASE")?,
        builder_api_key: secret("POLYMARKET_BUILDER_API_KEY")?,
        builder_api_secret: secret("POLYMARKET_BUILDER_API_SECRET")?,
        builder_passphrase: secret("POLYMARKET_BUILDER_PASSPHRASE")?,
        funder: std::env::var("POLYMARKET_FUNDER")?,
        base_url_http: None,
        base_url_relayer: None,
        proxy_url: None,
    })?;

    match args[0].as_str() {
        "list" => println!("{:?}", client.list_session_keys().await?),
        "authorize" => println!("{:?}", client.authorize_session_key(&args[1]).await?),
        "revoke" => {
            client.revoke_session_key(&args[1]).await?;
            println!("Session removed from active registry");
        }
        _ => unreachable!(),
    }

    Ok(())
}

fn secret(name: &str) -> anyhow::Result<SecretString> {
    Ok(std::env::var(name)?.into())
}
