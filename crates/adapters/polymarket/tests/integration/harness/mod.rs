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

//! Engine-wired seam harness for Polymarket live execution tests.

use std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use nautilus_common::clients::ExecutionClient;
use nautilus_core::{UUID4, UnixNanos};
pub(crate) use nautilus_live::testing::invariants;
use nautilus_live::{ExecutionClientCore, testing::ExecutionHarness};
use nautilus_model::{
    accounts::{AccountAny, cash::CashAccount},
    data::QuoteTick,
    enums::{AccountType, AssetClass, OmsType, OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId},
    instruments::{BinaryOption, InstrumentAny},
    orders::{OrderAny, OrderTestBuilder},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_polymarket::{
    common::consts::{POLYMARKET_CLIENT_ID, POLYMARKET_PRICE_PRECISION, POLYMARKET_VENUE},
    execution::PolymarketExecutionClient,
};
use rust_decimal::Decimal;
use serde_json::json;
use ustr::Ustr;

use crate::mock_venue::{
    TEST_CONDITION_ID, TEST_TOKEN_ID, TestServerState, execution_config, start_mock_server,
};

pub(crate) const ACCOUNT_ID: &str = "POLYMARKET-001";
pub(crate) const INSTRUMENT_ID: &str = "TEST-TOKEN.POLYMARKET";
pub(crate) const STRATEGY_ID: &str = "S-001";
pub(crate) const TRADER_ID: &str = "TESTER-001";

pub(crate) struct Harness {
    execution: ExecutionHarness,
    pub(crate) mock_state: TestServerState,
}

impl Harness {
    pub(crate) async fn build() -> Self {
        let trader_id = TraderId::from(TRADER_ID);
        let account_id = AccountId::from(ACCOUNT_ID);
        let instrument = instrument();
        let execution =
            ExecutionHarness::new(trader_id, *POLYMARKET_CLIENT_ID, account_id, instrument);
        add_account(&execution, account_id);

        let mock_state = TestServerState::default();
        mock_state.configure_default_order_success().await;
        let addr = start_mock_server(mock_state.clone()).await;
        let core = ExecutionClientCore::new(
            trader_id,
            *POLYMARKET_CLIENT_ID,
            *POLYMARKET_VENUE,
            OmsType::Netting,
            account_id,
            AccountType::Cash,
            None,
            execution.cache().clone(),
        );
        let mut client = PolymarketExecutionClient::new(core, execution_config(addr)).unwrap();
        let cached_instrument = execution
            .cache()
            .borrow()
            .instrument(&execution.instrument_id())
            .unwrap()
            .clone();
        client.on_instrument(cached_instrument);
        client.start().unwrap();
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
        }
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

pub(crate) fn instrument() -> InstrumentAny {
    let instrument_id = InstrumentId::from(INSTRUMENT_ID);
    let info = serde_json::from_value(json!({
        "condition_id": TEST_CONDITION_ID,
        "token_id": TEST_TOKEN_ID,
    }))
    .unwrap();
    let instrument = BinaryOption::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(TEST_TOKEN_ID))
        .asset_class(AssetClass::Alternative)
        .currency(Currency::pUSD())
        .activation_ns(UnixNanos::default())
        .expiration_ns(UnixNanos::default())
        .price_precision(POLYMARKET_PRICE_PRECISION)
        .size_precision(4)
        .price_increment(Price::from("0.0001"))
        .size_increment(Quantity::from("0.0001"))
        .outcome(Ustr::from("Yes"))
        .taker_fee(Decimal::ZERO)
        .info(info)
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    InstrumentAny::BinaryOption(instrument)
}

pub(crate) fn limit_order(instrument_id: InstrumentId, client_order_id: &str) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from(TRADER_ID))
        .strategy_id(StrategyId::from(STRATEGY_ID))
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(OrderSide::Buy)
        .price(Price::from("0.5000"))
        .quantity(Quantity::from("100.0000"))
        .time_in_force(TimeInForce::Gtc)
        .build()
}

pub(crate) fn quote(instrument_id: InstrumentId) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from("0.4900"),
        Price::from("0.5100"),
        Quantity::from("100.0000"),
        Quantity::from("100.0000"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn add_account(execution: &ExecutionHarness, account_id: AccountId) {
    let state = AccountState::new(
        account_id,
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000.0 pUSD"),
            Money::from("0 pUSD"),
            Money::from("1000.0 pUSD"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );
    execution
        .cache()
        .borrow_mut()
        .add_account(AccountAny::Cash(CashAccount::new(state, true, false)))
        .unwrap();
}
