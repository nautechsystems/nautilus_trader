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

//! Explicitly authorized live validation of the wallet-wide cancel-on-disconnect setting.

use std::time::Duration;

use nautilus_derive::{
    common::{credential::DeriveCredential, enums::DeriveEnvironment, urls},
    http::{
        DeriveCredentials, DeriveHttpClient,
        error::DeriveHttpError,
        query::{DeriveGetSubaccountParams, DeriveGetTriggerOrdersParams},
    },
    websocket::{DeriveWebSocketClient, DeriveWsCredentials},
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct AccountState {
    cancel_on_disconnect: bool,
    subaccount_ids: Vec<u64>,
}

#[rstest]
#[case::testnet(DeriveEnvironment::Testnet)]
#[case::mainnet(DeriveEnvironment::Mainnet)]
#[tokio::test]
#[ignore = "mutates wallet-wide COD; requires explicit authorization, credentials, and an idle wallet"]
async fn test_live_cancel_on_disconnect(#[case] environment: DeriveEnvironment) {
    let credential = DeriveCredential::resolve(None, None, None, environment).unwrap();
    let credentials =
        DeriveCredentials::new(credential.wallet_address(), credential.session_key()).unwrap();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(environment),
        credentials,
        Some(10),
        None,
        None,
    )
    .unwrap();
    let account = idle_account(&http, credential.wallet_address())
        .await
        .unwrap();
    assert!(
        !account.cancel_on_disconnect,
        "COD must be disabled before this validation"
    );
    let credentials =
        DeriveWsCredentials::new(credential.wallet_address(), credential.session_key()).unwrap();
    let mut ws_client = DeriveWebSocketClient::with_credentials(
        None,
        environment,
        TransportBackend::default(),
        None,
        credentials,
        None,
        None,
    );
    ws_client.set_request_timeout(Duration::from_secs(10));
    ws_client.connect().await.expect("live login failed");
    let validation: anyhow::Result<()> = async {
        ws_client
            .set_cancel_on_disconnect(true)
            .await
            .map_err(|e| anyhow::anyhow!("enable COD failed: {e}"))?;
        wait_for_cancel_on_disconnect(&http, credential.wallet_address(), true).await?;
        ws_client
            .disconnect()
            .await
            .map_err(|_| anyhow::anyhow!("disconnect failed"))?;
        ws_client
            .connect()
            .await
            .map_err(|_| anyhow::anyhow!("reconnect failed"))?;
        wait_for_cancel_on_disconnect(&http, credential.wallet_address(), true).await?;
        Ok(())
    }
    .await;

    if !ws_client.is_authenticated() {
        ws_client.connect().await.expect("cleanup login failed");
    }

    let cleanup = ws_client.set_cancel_on_disconnect(false).await;
    let disconnected = ws_client.disconnect().await;
    let final_account = wait_for_cancel_on_disconnect(&http, credential.wallet_address(), false)
        .await
        .expect("final idle-wallet check failed");
    assert!(
        !final_account.cancel_on_disconnect,
        "COD cleanup did not persist"
    );
    assert_eq!(final_account.subaccount_ids, account.subaccount_ids);
    cleanup.expect("disable COD failed");
    disconnected.expect("cleanup disconnect failed");
    validation.unwrap();
    println!(
        "{environment:?}: enable, reconnect persistence, disable, and idle-wallet checks passed"
    );
}

async fn wait_for_cancel_on_disconnect(
    http: &DeriveHttpClient,
    wallet: &str,
    enabled: bool,
) -> anyhow::Result<AccountState> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let account = idle_account(http, wallet).await?;
            if account.cancel_on_disconnect == enabled {
                return Ok(account);
            }

            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!("account did not reflect acknowledged COD setting within 30 seconds")
    })?
}

async fn idle_account(http: &DeriveHttpClient, wallet: &str) -> anyhow::Result<AccountState> {
    let account: AccountState = http
        .send_private("private/get_account", &json!({"wallet": wallet}))
        .await
        .map_err(|e| match e {
            DeriveHttpError::JsonRpc { code, .. } => {
                anyhow::anyhow!("account read failed: JSON-RPC {code}")
            }
            DeriveHttpError::Http { status, .. } => {
                anyhow::anyhow!("account read failed: HTTP {status}")
            }
            _ => anyhow::anyhow!("account read failed"),
        })?;

    anyhow::ensure!(
        !account.subaccount_ids.is_empty(),
        "wallet has no subaccounts"
    );

    for subaccount_id in &account.subaccount_ids {
        let params = DeriveGetSubaccountParams::new(*subaccount_id);
        let subaccount = http
            .get_subaccount(&params)
            .await
            .map_err(|_| anyhow::anyhow!("subaccount read failed"))?;
        anyhow::ensure!(subaccount.open_orders.is_empty(), "wallet has open orders");
        anyhow::ensure!(subaccount.positions.is_empty(), "wallet has positions");
        let params = DeriveGetTriggerOrdersParams::new(*subaccount_id);
        let orders = http
            .get_trigger_orders(&params)
            .await
            .map_err(|_| anyhow::anyhow!("trigger order read failed"))?;
        anyhow::ensure!(orders.orders.is_empty(), "wallet has trigger orders");
    }

    Ok(account)
}
