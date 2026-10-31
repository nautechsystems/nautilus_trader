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

use std::{cell::RefCell, net::SocketAddr, rc::Rc};

use nautilus_common::{
    cache::Cache, live::runner::set_exec_event_sender, messages::ExecutionEvent,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType},
    events::AccountState,
    identifiers::{AccountId, TraderId},
    types::{AccountBalance, Money},
};

use super::adapter::{
    common::{
        consts::{AX_CLIENT_ID, AX_VENUE},
        enums::AxEnvironment,
    },
    config::AxExecutionClientConfig,
    execution::AxExecutionClient,
};

pub(crate) fn setup_exec_channel() -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(sender);
    receiver
}

pub(crate) fn create_test_exec_config(addr: SocketAddr) -> AxExecutionClientConfig {
    AxExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
        environment: AxEnvironment::Sandbox,
        base_url_http: Some(format!("http://{addr}")),
        base_url_orders: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/orders/ws")),
        ..Default::default()
    }
}

pub(crate) fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    AxExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("AX-001");
    let client_id = *AX_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *AX_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);
    let rx = setup_exec_channel();
    let client = AxExecutionClient::new(core, config).expect("Failed to create exec client");

    (client, rx, cache)
}

pub(crate) fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("100000.50 USD"),
            Money::from("0 USD"),
            Money::from("100000.50 USD"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

pub(crate) fn drain_rx(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>) {
    while rx.try_recv().is_ok() {}
}
