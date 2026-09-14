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

//! Engine-wired seam harness for Betfair live execution tests.
//!
//! The adapter-neutral engine, routing, and assertion support lives behind the `nautilus-live`
//! `test-support` feature. This module supplies the Betfair client, mock venue, order builders, and
//! stream feeder.

use std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use nautilus_betfair::{
    common::consts::{BETFAIR_CLIENT_ID, BETFAIR_VENUE},
    config::BetfairExecutionClientConfig,
    execution::BetfairExecutionClient,
};
use nautilus_common::clients::ExecutionClient;
use nautilus_core::UnixNanos;
pub(crate) use nautilus_live::testing::{RoutedKind, invariants};
use nautilus_live::{ExecutionClientCore, testing::ExecutionHarness};
use nautilus_model::{
    data::QuoteTick,
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::{InstrumentAny, stubs::betting},
    orders::{OrderAny, builder::OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::mpsc::UnboundedSender};

use crate::common::{
    MockState, accept_and_activate, create_test_http_client, load_fixture, load_json_fixture,
    plain_stream_config, start_mock_http, start_mock_stream, test_credential,
};

pub(crate) const STRATEGY_ID: &str = "S-001";
const TRADER_ID: &str = "TESTER-001";

pub(crate) struct Harness {
    execution: ExecutionHarness,
    pub(crate) mock_state: MockState,
    pub(crate) feeder: StreamFeeder,
}

impl Harness {
    pub(crate) async fn build() -> Self {
        let trader_id = TraderId::from(TRADER_ID);
        let account_id = AccountId::from("BETFAIR-001");
        let instrument = InstrumentAny::Betting(betting());
        let execution =
            ExecutionHarness::new(trader_id, *BETFAIR_CLIENT_ID, account_id, instrument);
        let (addr, mock_state) = start_mock_http().await;
        let (stream_port, listener) = start_mock_stream().await;

        let core = ExecutionClientCore::new(
            trader_id,
            *BETFAIR_CLIENT_ID,
            *BETFAIR_VENUE,
            OmsType::Netting,
            account_id,
            AccountType::Betting,
            None,
            execution.cache().clone(),
        );
        let mut client = BetfairExecutionClient::new(
            core,
            create_test_http_client(addr),
            test_credential(),
            plain_stream_config(stream_port),
            BetfairExecutionClientConfig::default(),
            Currency::GBP(),
        );
        client.start().unwrap();

        let feeder = StreamFeeder::spawn(listener);
        client.connect().await.unwrap();
        nautilus_common::testing::wait_until_async(
            || async { client.is_connected() },
            Duration::from_secs(2),
        )
        .await;
        execution.register_client(Box::new(client)).unwrap();

        Self {
            execution,
            mock_state,
            feeder,
        }
    }

    pub(crate) fn override_betting_result(&self, method: &str, fixture_rel_path: &str) {
        let fixture = load_fixture(fixture_rel_path);
        let value: serde_json::Value = serde_json::from_str(&fixture).unwrap();
        self.mock_state
            .betting_overrides
            .lock()
            .insert(method.to_string(), value["result"].clone());
    }
}

impl Deref for Harness {
    type Target = ExecutionHarness;

    fn deref(&self) -> &Self::Target {
        &self.execution
    }
}

impl DerefMut for Harness {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.execution
    }
}

pub(crate) fn limit_order(instrument_id: &InstrumentId, client_order_id: &str) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from(TRADER_ID))
        .strategy_id(StrategyId::from(STRATEGY_ID))
        .instrument_id(*instrument_id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(OrderSide::Buy)
        .price(Price::from("3.0"))
        .quantity(Quantity::from("10.0"))
        .time_in_force(TimeInForce::Gtc)
        .build()
}

pub(crate) fn quote(instrument_id: &InstrumentId, bid: &str, ask: &str) -> QuoteTick {
    QuoteTick::new(
        *instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

pub(crate) struct StreamFeeder {
    tx: UnboundedSender<String>,
}

impl StreamFeeder {
    fn spawn(listener: TcpListener) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let (_reader, mut write_half) = accept_and_activate(&listener).await;

            while let Some(frame) = rx.recv().await {
                write_half
                    .write_all(format!("{}\r\n", frame.trim()).as_bytes())
                    .await
                    .unwrap();
            }
        });
        Self { tx }
    }

    pub(crate) fn feed(&self, fixture_rel_path: &str) {
        let mut frame = load_json_fixture(fixture_rel_path);
        frame["id"] = 2.into();
        self.tx.send(frame.to_string()).unwrap();
    }
}
