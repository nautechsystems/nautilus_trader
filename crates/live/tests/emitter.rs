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

//! Integration tests for `ExecutionEventEmitter`.
//!
//! These tests exercise the emitter through the crate's public API, so they also pin which
//! methods are public. Internal state tests are in the in-module tests in emitter.rs.

use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_static};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::AccountType,
    events::AccountState,
    identifiers::{AccountId, TraderId},
    types::{AccountBalance, Currency, Money},
};
use rstest::rstest;

fn create_emitter() -> ExecutionEventEmitter {
    ExecutionEventEmitter::new(
        get_atomic_clock_static(),
        TraderId::from("TRADER-001"),
        AccountId::from("SIM-001"),
        AccountType::Cash,
        None,
    )
}

fn create_account_state(ts_init: UnixNanos) -> AccountState {
    let usd = Currency::USD();
    AccountState::new(
        AccountId::from("SIM-001"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::new(1_500.0, usd),
            Money::new(250.0, usd),
            Money::new(1_250.0, usd),
        )],
        Vec::new(),
        true,
        UUID4::new(),
        UnixNanos::from(123_456_789),
        ts_init,
        Some(usd),
    )
}

#[rstest]
fn test_try_send_account_state_preserves_caller_built_state() {
    let mut emitter = create_emitter();
    let ts_init = UnixNanos::from(987_654_321);
    let state = create_account_state(ts_init);

    let error = emitter.try_send_account_state(state.clone()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Cannot send account state: sender not initialized"
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);

    emitter.try_send_account_state(state.clone()).unwrap();

    // `AccountState::PartialEq` compares identity fields only, so pin each field.
    match rx.try_recv() {
        Ok(ExecutionEvent::Account(received)) => {
            assert_eq!(received.account_id, state.account_id);
            assert_eq!(received.account_type, state.account_type);
            assert_eq!(received.event_id, state.event_id);
            assert_eq!(received.balances, state.balances);
            assert_eq!(received.margins, state.margins);
            assert_eq!(received.is_reported, state.is_reported);
            assert_eq!(received.base_currency, state.base_currency);
            assert_eq!(received.ts_event, state.ts_event);
            assert_eq!(received.ts_init, ts_init);
            assert_eq!(received.info, state.info);
        }
        other => panic!("expected an account state event, was {other:?}"),
    }
}
