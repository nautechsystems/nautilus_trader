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

//! Account application through external message bus ingress.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::VirtualClock,
    enums::SerializationEncoding,
    msgbus::{
        self, BusMessage, BusMessageSource, BusPayloadType, MessageBus, MessageBusExternalEgress,
        TypedHandler,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::Account,
    enums::AccountType,
    events::AccountState,
    types::{AccountBalance, Currency, Money},
};
use nautilus_portfolio::Portfolio;
use rstest::rstest;

#[derive(Default)]
struct CapturingEgress {
    messages: Rc<RefCell<Vec<BusMessage>>>,
    closed: bool,
}

impl MessageBusExternalEgress for CapturingEgress {
    fn is_closed(&self) -> bool {
        self.closed
    }

    fn publish(&self, message: BusMessage) {
        self.messages.borrow_mut().push(message);
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

fn account_state(account_type: AccountType, account_id: &str, amount: &str) -> AccountState {
    AccountState::new(
        account_id.into(),
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
}

fn external_messages(
    encoding: SerializationEncoding,
    states: &[AccountState],
) -> (Vec<BusMessage>, Rc<RefCell<Vec<BusMessage>>>) {
    msgbus::set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
    let egress = CapturingEgress::default();
    let captured = Rc::clone(&egress.messages);
    msgbus::get_message_bus()
        .borrow_mut()
        .set_external_egress(Box::new(egress), encoding);
    let messages = states
        .iter()
        .map(|state| {
            msgbus::publish_account_state(
                format!("events.account.{}", state.account_id).into(),
                state,
            );
            let mut captured = captured.borrow_mut();
            assert_eq!(captured.len(), 1);
            let mut message = captured.pop().unwrap();
            message.source = Some(BusMessageSource {
                publisher_id: "PUBLISHER-001".into(),
                instance_id: UUID4::new(),
                stream: "accounts".into(),
                sequence: 1,
            });
            message
        })
        .collect();
    msgbus::get_message_bus()
        .borrow_mut()
        .add_streaming_type(BusPayloadType::AccountState);
    (messages, captured)
}

#[rstest]
#[case::cash(AccountType::Cash)]
#[case::margin(AccountType::Margin)]
fn test_external_account_applies_before_publication(
    #[case] account_type: AccountType,
    #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
    encoding: SerializationEncoding,
    #[values(false, true)] with_processors: bool,
) {
    let states = [
        account_state(account_type, "SIM-001", "100 USD"),
        account_state(account_type, "SIM-002", "200 USD"),
        account_state(account_type, "SIM-001", "0 USD"),
    ];
    let (mut messages, egress) = external_messages(encoding, &states);

    // Account publication uses the account identity, including for nonstandard ingress topics
    for message in &mut messages {
        message.topic = "external.accounts".into();
    }
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut portfolio = Portfolio::new(
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::clone(&cache),
        None,
    );
    let observed = Rc::new(RefCell::new(Vec::new()));
    let handler = TypedHandler::from({
        let observed = Rc::clone(&observed);
        let cache = Rc::clone(&cache);
        move |event: &AccountState| {
            let cache = cache.borrow();
            let account = cache.account(&event.account_id).unwrap();
            assert_eq!(account.last_event().unwrap(), *event);
            assert_eq!(account.account_type(), event.account_type);
            assert_eq!(
                account.balances().into_values().collect::<Vec<_>>(),
                event.balances
            );
            observed.borrow_mut().push((
                event.event_id,
                account.events().len(),
                msgbus::current_external_source(),
            ));
        }
    });
    msgbus::subscribe_account_state("events.account.*".into(), handler, Some(20));
    let scoped = Rc::new(RefCell::new(Vec::new()));
    let scoped_handler = TypedHandler::from({
        let scoped = Rc::clone(&scoped);
        move |event: &AccountState| scoped.borrow_mut().push(event.event_id)
    });
    msgbus::subscribe_account_state("events.account.SIM-001".into(), scoped_handler, None);

    for message in &messages {
        if with_processors {
            msgbus::process_external_typed_message(message, &mut |_, _| {
                panic!("AccountState bypasses typed stream processors")
            })
            .unwrap();
        } else {
            msgbus::republish_external_message(message).unwrap();
        }
    }

    let expected: Vec<_> = states
        .iter()
        .zip(&messages)
        .zip([1, 1, 2])
        .map(|((state, message), count)| (state.event_id, count, message.source))
        .collect();
    assert_eq!(*observed.borrow(), expected);
    assert_eq!(
        *scoped.borrow(),
        vec![states[0].event_id, states[2].event_id]
    );
    assert!(egress.borrow().is_empty(), "external inputs must not echo");
    assert_eq!(msgbus::current_external_source(), None);

    // Local updates must still reach external egress after ingress guards are released
    let local = account_state(account_type, "SIM-001", "50 USD");
    portfolio.update_account(&local);
    assert_eq!(egress.borrow().len(), 1);
    assert_eq!(egress.borrow()[0].topic.as_str(), "events.account.SIM-001");
    assert_eq!(
        observed.borrow().last().unwrap(),
        &(local.event_id, 3, None)
    );
}

#[rstest]
#[case::cash_update(AccountType::Cash, true)]
#[case::wallet_initial(AccountType::Wallet, false)]
#[case::wallet_update(AccountType::Wallet, true)]
fn test_external_rejected_account_is_not_published(
    #[case] account_type: AccountType,
    #[case] existing_account: bool,
    #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
    encoding: SerializationEncoding,
) {
    let initial = account_state(account_type, "SIM-001", "100 USD");
    let rejected = account_state(account_type, "SIM-001", "-1 USD");
    let (messages, egress) = external_messages(encoding, &[rejected]);
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut portfolio = Portfolio::new(
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::clone(&cache),
        None,
    );

    if existing_account {
        portfolio.update_account(&initial);
        assert_eq!(
            egress
                .borrow()
                .iter()
                .filter(|message| message.payload_type == BusPayloadType::AccountState)
                .count(),
            1,
        );
        egress.borrow_mut().clear();
    }
    let received = Rc::new(RefCell::new(Vec::new()));
    let handler = TypedHandler::from({
        let received = Rc::clone(&received);
        move |event: &AccountState| received.borrow_mut().push(event.clone())
    });
    msgbus::subscribe_account_state("events.account.*".into(), handler, Some(20));
    msgbus::republish_external_message(&messages[0]).unwrap();

    assert!(received.borrow().is_empty());
    assert!(egress.borrow().is_empty());
    assert_eq!(msgbus::current_external_source(), None);
    let cache = cache.borrow();

    if existing_account {
        let account = cache.account(&initial.account_id).unwrap();
        assert_eq!(account.last_event(), Some(initial));
        assert_eq!(account.events().len(), 1);
    } else {
        assert!(cache.account(&initial.account_id).is_none());
    }
}
