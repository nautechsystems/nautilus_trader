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

use std::{cell::RefCell, net::SocketAddr, rc::Rc, time::Duration};

use nautilus_common::{
    cache::Cache, clients::ExecutionClient, live::runner::set_exec_event_sender,
    messages::ExecutionEvent,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType},
    events::{AccountState, OrderEventAny},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
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
    create_test_execution_client_from_config(create_test_exec_config(addr))
}

pub(crate) fn create_test_execution_client_from_config(
    config: AxExecutionClientConfig,
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

pub(crate) async fn collect_order_events(
    client: &AxExecutionClient,
    state: &super::server::TestServerState,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<ExecutionEvent> {
    let client_order_id = ClientOrderId::new(format!("CID-FENCE-{}", UUID4::new()));
    let venue_order_id = VenueOrderId::new(format!("OID-FENCE-{}", UUID4::new()));
    client.register_external_order(
        client_order_id,
        venue_order_id,
        InstrumentId::from("EURUSD-PERP.AX"),
        StrategyId::from("S-001"),
        UnixNanos::default(),
    );
    let mut canceled = super::server::load_test_data("ws_order_canceled.json");
    canceled["o"]["oid"] = serde_json::json!(venue_order_id.as_str());
    state.order_injected_events.lock().await.push(canceled);

    // The marker follows earlier frames through the same socket and execution stream
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();

        loop {
            let event = rx.recv().await.expect("execution event channel open");

            if matches!(&event, ExecutionEvent::Order(OrderEventAny::Canceled(event))
                if event.client_order_id == client_order_id)
            {
                return events;
            }

            events.push(event);
        }
    })
    .await
    .expect("order event marker should arrive")
}
