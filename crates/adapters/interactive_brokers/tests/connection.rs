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

use std::{env, time::Duration};

use ibapi::client::Client;
use nautilus_interactive_brokers::common::consts::{
    DEFAULT_CLIENT_ID, DEFAULT_HOST, DEFAULT_TWS_PORT,
};
use rstest::rstest;

fn live_ib_tests_enabled() -> bool {
    env::var("NAUTILUS_IB_LIVE_TESTS").is_ok_and(|value| value == "1")
}

fn ib_host() -> String {
    env::var("NAUTILUS_IB_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string())
}

fn ib_port() -> u16 {
    env::var("NAUTILUS_IB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_TWS_PORT)
}

fn ib_client_id() -> i32 {
    env::var("NAUTILUS_IB_CLIENT_ID")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_CLIENT_ID + 90)
}

#[rstest]
#[tokio::test]
async fn test_paper_tws_managed_accounts_smoke() {
    if !live_ib_tests_enabled() {
        eprintln!("Skipping live IB smoke test because NAUTILUS_IB_LIVE_TESTS is not 1");
        return;
    }

    let address = format!("{}:{}", ib_host(), ib_port());
    let client_id = ib_client_id();

    let client = tokio::time::timeout(
        Duration::from_secs(15),
        Client::connect(&address, client_id),
    )
    .await
    .unwrap_or_else(|_| panic!("Timed out connecting to paper TWS/Gateway at {address}"))
    .unwrap_or_else(|e| panic!("Failed to connect to paper TWS/Gateway at {address}: {e}"));

    let accounts = tokio::time::timeout(Duration::from_secs(15), client.managed_accounts())
        .await
        .expect("Timed out requesting managed accounts")
        .expect("Failed to request managed accounts");

    if let Ok(expected_accounts) = env::var("NAUTILUS_IB_EXPECTED_ACCOUNTS") {
        let expected_accounts: Vec<String> = expected_accounts
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect();
        assert_eq!(accounts, expected_accounts);
    } else {
        assert!(!accounts.is_empty());
    }
}
