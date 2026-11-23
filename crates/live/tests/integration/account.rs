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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::VirtualClock,
    messages::ExecutionEvent,
    msgbus::{self, MessageBus, TypedHandler},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::runner::AsyncRunner;
use nautilus_model::{
    accounts::Account,
    enums::AccountType,
    events::AccountState,
    identifiers::{AccountId, TraderId},
    types::{AccountBalance, Currency, Money},
};
use nautilus_portfolio::Portfolio;
use rstest::rstest;

#[rstest]
#[case::cash(AccountType::Cash)]
#[case::margin(AccountType::Margin)]
fn test_account_event_publishes_after_portfolio_application(#[case] account_type: AccountType) {
    // Native logging tests use nextest's per-test process isolation.
    struct AccountLogCapture(std::sync::Mutex<Vec<String>>);

    impl log::Log for AccountLogCapture {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() == log::Level::Info
                && metadata.target() == "nautilus_portfolio::portfolio"
        }

        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                self.0.lock().unwrap().push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    static LOGS: AccountLogCapture = AccountLogCapture(std::sync::Mutex::new(Vec::new()));
    log::set_logger(&LOGS).unwrap();
    log::set_max_level(log::LevelFilter::Info);

    msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::new(
        TraderId::from("TESTER-001"),
        UUID4::new(),
        None,
        None,
    ))));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let _portfolio = Portfolio::new(clock, cache.clone(), None);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let received = observed.clone();
    let source = cache.clone();
    let handler = TypedHandler::from(move |event: &AccountState| {
        let cache = source.borrow();
        let account = cache.account(&event.account_id).unwrap();
        assert_eq!(account.last_event().unwrap().event_id, event.event_id);
        assert_eq!(account.account_type(), event.account_type);
        assert_eq!(
            account.balances().into_values().collect::<Vec<_>>(),
            event.balances
        );
        received
            .borrow_mut()
            .push((event.event_id, account.events().len()));
    });

    // Subscribers must see applied state regardless of their topic priority
    msgbus::subscribe_account_state("events.account.*".into(), handler.clone(), Some(20));
    let scoped = Rc::new(RefCell::new(Vec::new()));
    let scoped_received = scoped.clone();
    let scoped_handler = TypedHandler::from(move |event: &AccountState| {
        scoped_received.borrow_mut().push(event.event_id);
    });

    msgbus::subscribe_account_state(
        "events.account.SIM-001".into(),
        scoped_handler.clone(),
        None,
    );
    let mut expected = Vec::new();
    let mut expected_scoped = Vec::new();

    for (index, (account_id, total, locked, free, history_len)) in [
        ("SIM-001", "100 USD", "20 USD", "80 USD", 1),
        ("SIM-002", "200 USD", "50 USD", "150 USD", 1),
        ("SIM-001", "90 USD", "10 USD", "80 USD", 2),
        ("SIM-001", "0 USD", "0 USD", "0 USD", 3),
    ]
    .into_iter()
    .enumerate()
    {
        let event = AccountState::new(
            AccountId::from(account_id),
            account_type,
            vec![AccountBalance::new(
                Money::from(total),
                Money::from(locked),
                Money::from(free),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(index as u64 + 1),
            UnixNanos::from(index as u64 + 1),
            Some(Currency::USD()),
        );

        expected.push((event.event_id, history_len));
        if account_id == "SIM-001" {
            expected_scoped.push(event.event_id);
        }

        let expected_log = format!("Updated {event}");
        AsyncRunner::handle_exec_event(ExecutionEvent::Account(event));
        assert_eq!(
            LOGS.0
                .lock()
                .unwrap()
                .iter()
                .filter(|line| **line == expected_log)
                .count(),
            1
        );
    }

    assert_eq!(*observed.borrow(), expected);
    assert_eq!(*scoped.borrow(), expected_scoped);
    let cache = cache.borrow();
    assert_eq!(
        cache
            .account(&AccountId::from("SIM-001"))
            .unwrap()
            .events()
            .len(),
        3
    );
    assert_eq!(
        cache
            .account(&AccountId::from("SIM-002"))
            .unwrap()
            .events()
            .len(),
        1
    );
    msgbus::unsubscribe_account_state("events.account.*".into(), &handler);
    msgbus::unsubscribe_account_state("events.account.SIM-001".into(), &scoped_handler);
}

#[rstest]
#[case::cash_update(AccountType::Cash, true)]
#[case::wallet_initial(AccountType::Wallet, false)]
#[case::wallet_update(AccountType::Wallet, true)]
fn test_rejected_account_event_is_not_published(
    #[case] account_type: AccountType,
    #[case] existing_account: bool,
) {
    msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut portfolio = Portfolio::new(
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        None,
    );

    let state = |amount: &str| {
        AccountState::new(
            AccountId::from("SIM-001"),
            account_type,
            vec![AccountBalance::new(
                Money::from(amount),
                Money::zero(Currency::USD()),
                Money::from(amount),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            (account_type != AccountType::Wallet).then_some(Currency::USD()),
        )
    };

    let initial = state("100.00 USD");
    if existing_account {
        portfolio.update_account(&initial);
    }

    let received = Rc::new(RefCell::new(Vec::new()));

    let handler = TypedHandler::from({
        let received = received.clone();
        move |event: &AccountState| received.borrow_mut().push(event.clone())
    });

    msgbus::subscribe_account_state("events.account.*".into(), handler.clone(), None);
    AsyncRunner::handle_exec_event(ExecutionEvent::Account(state("-1.00 USD")));
    assert!(received.borrow().is_empty());
    let cache = cache.borrow();

    if existing_account {
        assert_eq!(
            cache.account(&initial.account_id).unwrap().last_event(),
            Some(initial)
        );
    } else {
        assert!(cache.account(&initial.account_id).is_none());
    }

    msgbus::unsubscribe_account_state("events.account.*".into(), &handler);
}
