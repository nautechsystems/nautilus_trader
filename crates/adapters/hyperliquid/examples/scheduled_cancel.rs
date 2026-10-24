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

//! Arms, replaces, and clears a scheduled cancellation on an account with no open orders.
//!
//! Uses testnet by default. Pass `mainnet` to use the production account.

use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_hyperliquid::{HyperliquidHttpClient, common::enums::HyperliquidEnvironment};
use nautilus_model::identifiers::AccountId;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let environment = match std::env::args().nth(1).as_deref() {
        None | Some("testnet") => HyperliquidEnvironment::Testnet,
        Some("mainnet") => HyperliquidEnvironment::Mainnet,
        _ => anyhow::bail!("Expected testnet or mainnet"),
    };

    let mut client =
        HyperliquidHttpClient::with_credentials(None, None, None, environment, 60, None)?;

    if client.has_vault_address() {
        client.set_account_address(None)?;
    }

    client.set_account_id(AccountId::from("HYPERLIQUID-001"));
    let user = client.get_account_address()?;
    for instrument in client.request_instruments().await? {
        client.cache_instrument(&instrument);
    }

    let orders = client.request_order_status_reports(&user, None).await?;
    anyhow::ensure!(
        orders.is_empty(),
        "Scheduled-cancel validation requires no open orders"
    );

    let now = get_atomic_clock_realtime().get_time_ms();
    let result = async {
        client.schedule_cancel(Some(now + 60_000)).await?;
        println!("Scheduled cancellation armed");
        client.schedule_cancel(Some(now + 120_000)).await?;
        println!("Scheduled cancellation replaced");
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(e) = client.schedule_cancel(None).await {
        anyhow::bail!("Failed to clear scheduled cancellation: {e}; update result: {result:?}");
    }

    println!("Scheduled cancellation cleared");
    result
}
