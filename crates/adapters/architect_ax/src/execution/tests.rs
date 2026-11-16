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

#[path = "../../tests/integration/exec_client.rs"]
mod exec_client;
#[path = "../../tests/integration/common/execution.rs"]
mod execution;
#[path = "../../tests/integration/replacement.rs"]
mod replacement;
#[path = "../../tests/integration/common/server.rs"]
mod server;

use std::time::Duration;

use nautilus_common::testing::wait_until_async;

use super::AxExecutionClient;
use crate as adapter;

async fn wait_for_pending_tasks(client: &AxExecutionClient) {
    wait_until_async(
        || async { client.pending_tasks.all_finished() },
        Duration::from_secs(5),
    )
    .await;
}

async fn wait_for_order_commands(client: &AxExecutionClient, state: &server::TestServerState) {
    wait_for_pending_tasks(client).await;
    let request_id = client.ws_orders.get_open_orders().await.unwrap();
    wait_until_async(
        || async {
            state
                .get_messages()
                .await
                .iter()
                .any(|message| message["t"] == "o" && message["rid"].as_i64() == Some(request_id))
        },
        Duration::from_secs(5),
    )
    .await;
}
