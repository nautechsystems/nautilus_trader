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
    cache::CacheView,
    clients::ExecutionClient,
    clock::Clock,
    enums::Environment,
    factories::{ClientConfig, ExecutionClientFactory},
    messages::execution::{SubmitOrder, TradingCommand},
    msgbus::{
        self, MessagingSwitchboard, stubs::get_typed_into_message_saving_handler, switchboard,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::stubs::StubExecutionClient;
use nautilus_live::{builder::LiveNodeBuilder, config::RoutingConfig};
use nautilus_model::{
    accounts::{AccountAny, CashAccount},
    data::InstrumentClose,
    enums::{AccountType, InstrumentCloseType, OmsType, OrderSide, OrderType},
    events::{AccountState, OrderDeniedReason, OrderEventAny},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TraderId, Venue},
    instruments::{
        Instrument, InstrumentAny,
        stubs::{audusd_sim, binary_option},
    },
    orders::{Order, OrderTestBuilder},
    stubs::TestDefault,
    types::{AccountBalance, Money, Price, Quantity},
};
use rstest::rstest;
use ustr::Ustr;

#[rstest]
#[case::single(1, false, false, false)]
#[case::explicit(2, true, false, false)]
#[case::default(2, false, true, false)]
#[case::explicit_over_default(2, true, true, false)]
#[case::single_empty_venues(1, false, false, true)]
#[case::default_empty_venues(2, false, true, true)]
fn test_execution_client_routing_and_instruments(
    #[case] count: usize,
    #[case] explicit: bool,
    #[case] default: bool,
    #[case] empty_venues: bool,
    #[values(false, true)] reverse: bool,
) {
    let clients: Vec<_> = (0..count)
        .map(|i| {
            StubExecutionClient::new(
                ClientId::new(format!("CLIENT-{i}")),
                AccountId::new(format!("ACCOUNT-{i}")),
                Venue::from("SIM"),
                OmsType::Netting,
                None,
            )
        })
        .collect();

    let mut builder = LiveNodeBuilder::new(TraderId::test_default(), Environment::Live).unwrap();
    let mut indices: Vec<_> = (0..count).collect();
    if reverse {
        indices.reverse();
    }

    for i in indices {
        builder = builder
            .add_exec_client_with_routing(
                Some(format!("client-{i}")),
                Box::new(RoutingClientFactory(clients[i].clone())),
                Box::new(RoutingClientConfig),
                RoutingConfig {
                    default: default && i == 0,
                    venues: if empty_venues {
                        Some(vec![])
                    } else {
                        (explicit && i == 1).then(|| vec!["SIM".to_string()])
                    },
                },
            )
            .unwrap();
    }

    let node = builder.build().unwrap();
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    msgbus::publish_instrument(
        switchboard::get_instrument_topic(instrument.id()),
        &instrument,
    );
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .quantity(Quantity::from(1))
        .build();
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    node.kernel()
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let engine = node.kernel().exec_engine.borrow();
    engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
        &order,
        TraderId::test_default(),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    )));
    let routed = engine.get_clients_for_orders(std::slice::from_ref(&order));
    let explicit_index = count - 1 - usize::from(explicit);
    let explicit_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-EXPLICIT"))
        .quantity(Quantity::from(2))
        .build();
    node.kernel()
        .cache
        .borrow_mut()
        .add_order(explicit_order.clone(), None, None, false)
        .unwrap();
    engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
        &explicit_order,
        TraderId::test_default(),
        Some(clients[explicit_index].client_id()),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )));

    assert_eq!(engine.client_ids().len(), count);
    assert_eq!(routed.len(), 1);
    assert_eq!(
        routed[0].client_id(),
        clients[usize::from(explicit)].client_id()
    );

    for (i, client) in clients.iter().enumerate() {
        let mut expected_orders = if i == usize::from(explicit) {
            vec![order.client_order_id()]
        } else {
            vec![]
        };

        if i == explicit_index {
            expected_orders.push(explicit_order.client_order_id());
        }

        assert_eq!(*client.submitted_order_ids().borrow(), expected_orders);
        assert_eq!(
            *client.received_instruments().borrow(),
            vec![instrument.clone()]
        );
    }
}

#[rstest]
#[case::default_accepts_venue(true, false)]
#[case::default_rejects_venue(false, false)]
#[case::explicit_route_takes_precedence(true, true)]
fn test_execution_instruments_follow_default_broker_routing(
    #[case] broker_accepts_venue: bool,
    #[case] explicit_route: bool,
) {
    let mut broker = StubExecutionClient::new(
        ClientId::from("BROKER"),
        AccountId::from("BROKER-001"),
        Venue::from("BROKER"),
        OmsType::Netting,
        None,
    );

    if broker_accepts_venue {
        broker = broker.with_handles_all_order_venues();
    }

    let other = StubExecutionClient::new(
        ClientId::from("OTHER"),
        AccountId::from("OTHER-001"),
        Venue::from("OTHER"),
        OmsType::Netting,
        None,
    );

    let node = LiveNodeBuilder::new(TraderId::test_default(), Environment::Live)
        .unwrap()
        .add_exec_client_with_routing(
            Some("broker".into()),
            Box::new(RoutingClientFactory(broker.clone())),
            Box::new(RoutingClientConfig),
            RoutingConfig {
                default: true,
                venues: None,
            },
        )
        .unwrap()
        .add_exec_client_with_routing(
            Some("other".into()),
            Box::new(RoutingClientFactory(other.clone())),
            Box::new(RoutingClientConfig),
            RoutingConfig {
                default: false,
                venues: explicit_route.then(|| vec!["XCME".into()]),
            },
        )
        .unwrap()
        .build()
        .unwrap();

    let mut instrument = audusd_sim();
    instrument.id = InstrumentId::from("AUDUSD.XCME");
    let instrument = InstrumentAny::CurrencyPair(instrument);

    let engine = node.kernel().exec_engine.borrow_mut();
    node.kernel().data_engine.borrow_mut().process(&instrument);
    drop(engine);

    let broker_expected = if broker_accepts_venue && !explicit_route {
        vec![instrument.clone()]
    } else {
        vec![]
    };

    let other_expected = if explicit_route {
        vec![instrument]
    } else {
        vec![]
    };

    assert_eq!(*broker.received_instruments().borrow(), broker_expected);
    assert_eq!(*other.received_instruments().borrow(), other_expected);
}

#[rstest]
fn test_execution_client_routing_selects_risk_check_account() {
    let clients: Vec<_> = (0..2)
        .map(|i| {
            StubExecutionClient::new(
                ClientId::new(format!("CLIENT-{i}")),
                AccountId::new(format!("ACCOUNT-{i}")),
                Venue::from("SIM"),
                OmsType::Netting,
                None,
            )
        })
        .collect();

    let mut builder = LiveNodeBuilder::new(TraderId::test_default(), Environment::Live).unwrap();
    for (i, client) in clients.iter().enumerate() {
        builder = builder
            .add_exec_client_with_routing(
                Some(format!("client-{i}")),
                Box::new(RoutingClientFactory(client.clone())),
                Box::new(RoutingClientConfig),
                RoutingConfig {
                    default: i == 0,
                    venues: None,
                },
            )
            .unwrap();
    }

    let node = builder.build().unwrap();
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100))
        .price(Price::from("1.00000"))
        .build();
    let explicit_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-EXPLICIT"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from(200))
        .price(Price::from("1.00000"))
        .build();

    {
        let mut cache = node.kernel().cache.borrow_mut();
        cache.add_instrument(instrument).unwrap();
        cache
            .add_account(cash_account(clients[0].account_id(), "1000000 USD"))
            .unwrap();
        cache
            .add_account(cash_account(clients[1].account_id(), "150 USD"))
            .unwrap();
        cache.add_order(order.clone(), None, None, false).unwrap();
        cache
            .add_order(explicit_order.clone(), None, None, false)
            .unwrap();
    }

    let (command_handler, commands) = get_typed_into_message_saving_handler::<TradingCommand>(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        command_handler,
    );
    let (event_handler, events) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(
        MessagingSwitchboard::exec_engine_process(),
        event_handler,
    );

    let mut risk_engine = node.kernel().risk_engine.borrow_mut();
    risk_engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
        &order,
        TraderId::test_default(),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    )));
    risk_engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
        &explicit_order,
        TraderId::test_default(),
        Some(clients[1].client_id()),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )));

    let forwarded: Vec<_> = commands
        .get_messages()
        .iter()
        .map(|command| match command {
            TradingCommand::SubmitOrder(cmd) => (cmd.client_order_id, cmd.client_id),
            other => panic!("Unexpected command {other:?}"),
        })
        .collect();

    let events = events.get_messages();
    assert_eq!(forwarded, vec![(order.client_order_id(), None)]);
    assert_eq!(events.len(), 1);

    let OrderEventAny::Denied(denied) = &events[0] else {
        panic!("Expected OrderDenied")
    };

    assert_eq!(denied.client_order_id, explicit_order.client_order_id());
    assert_eq!(
        denied.reason,
        Ustr::from(
            &OrderDeniedReason::NotionalExceedsFreeBalance {
                free_balance: Money::from("150 USD"),
                notional: Money::from("200 USD"),
            }
            .to_string()
        )
    );
}

#[rstest]
fn test_execution_client_keeps_native_route_with_extra_venue(
    #[values(false, true)] other_native_client: bool,
) {
    let client = StubExecutionClient::new(
        ClientId::from("CLIENT"),
        AccountId::from("CLIENT-001"),
        Venue::from("SIM"),
        OmsType::Netting,
        None,
    )
    .with_handles_all_order_venues();

    let other_client = StubExecutionClient::new(
        ClientId::from("OTHER_CLIENT"),
        AccountId::from("OTHER_CLIENT-002"),
        Venue::from("OTHER"),
        OmsType::Netting,
        None,
    );

    let mut builder = LiveNodeBuilder::new(TraderId::test_default(), Environment::Live)
        .unwrap()
        .add_exec_client_with_routing(
            Some("client".to_string()),
            Box::new(RoutingClientFactory(client.clone())),
            Box::new(RoutingClientConfig),
            RoutingConfig {
                default: false,
                venues: Some(vec!["OTHER".to_string(), "OTHER".to_string()]),
            },
        )
        .unwrap();

    if other_native_client {
        builder = builder
            .add_exec_client_with_routing(
                Some("other-client".to_string()),
                Box::new(RoutingClientFactory(other_client.clone())),
                Box::new(RoutingClientConfig),
                RoutingConfig::default(),
            )
            .unwrap();
    }

    let node = builder.build().unwrap();

    let native = audusd_sim();
    let mut other = native.clone();
    other.id.venue = Venue::from("OTHER");

    let instruments = [
        InstrumentAny::CurrencyPair(native),
        InstrumentAny::CurrencyPair(other),
    ];

    for instrument in &instruments {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new(format!("O-{}", instrument.id().venue)))
            .quantity(Quantity::from(1))
            .build();
        let engine = node.kernel().exec_engine.borrow();
        node.kernel()
            .cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
            &order,
            TraderId::test_default(),
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        )));
        let routed = engine.get_clients_for_orders(&[order]);
        assert_eq!(
            routed
                .iter()
                .map(|client| client.client_id())
                .collect::<Vec<_>>(),
            vec![client.client_id()]
        );
        drop(engine);
        msgbus::publish_instrument(
            switchboard::get_instrument_topic(instrument.id()),
            instrument,
        );
    }

    assert_eq!(*client.received_instruments().borrow(), instruments);
    assert_eq!(
        *client.submitted_order_ids().borrow(),
        vec![ClientOrderId::from("O-SIM"), ClientOrderId::from("O-OTHER")]
    );
    assert_eq!(*other_client.submitted_order_ids().borrow(), vec![]);

    let expected_other = if other_native_client {
        vec![instruments[1].clone()]
    } else {
        vec![]
    };

    assert_eq!(
        *other_client.received_instruments().borrow(),
        expected_other
    );
}

#[rstest]
#[case::ambiguous(
    false,
    false,
    false,
    "Multiple execution clients for venue SIM: configure an explicit venue route or default client"
)]
#[case::duplicate_id(true, false, false, "Client already registered with ID CLIENT-0")]
#[case::duplicate_default(false, true, false, "default client already registered")]
#[case::duplicate_route(false, false, true, "cannot re-route")]
fn test_execution_client_routing_rejects_ambiguity(
    #[case] duplicate_id: bool,
    #[case] default: bool,
    #[case] explicit: bool,
    #[case] expected: &str,
) {
    let mut builder = LiveNodeBuilder::new(TraderId::test_default(), Environment::Live).unwrap();

    for i in 0..2 {
        let id = if duplicate_id { 0 } else { i };

        let client = StubExecutionClient::new(
            ClientId::new(format!("CLIENT-{id}")),
            AccountId::new(format!("ACCOUNT-{i}")),
            Venue::from("SIM"),
            OmsType::Netting,
            None,
        );

        builder = builder
            .add_exec_client_with_routing(
                Some(format!("client-{i}")),
                Box::new(RoutingClientFactory(client)),
                Box::new(RoutingClientConfig),
                RoutingConfig {
                    default,
                    venues: explicit.then(|| vec!["SIM".to_string()]),
                },
            )
            .unwrap();
    }

    let error = builder.build().unwrap_err().to_string();
    assert!(error.contains(expected), "Unexpected error: {error}");
}

fn cash_account(account_id: AccountId, free: &str) -> AccountAny {
    let state = AccountState::new(
        account_id,
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from(free),
            Money::from("0 USD"),
            Money::from(free),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    AccountAny::Cash(CashAccount::new(state, false, false))
}

#[rstest]
#[case::venue(false, true)]
#[case::self_settling_venue(true, false)]
fn test_execution_engine_settles_instrument_closes_unless_venue_settles_itself(
    #[case] self_settling: bool,
    #[case] expected_settled: bool,
) {
    let instrument = InstrumentAny::BinaryOption(binary_option());

    let mut client = StubExecutionClient::new(
        ClientId::from("CLIENT"),
        AccountId::from("CLIENT-001"),
        instrument.id().venue,
        OmsType::Netting,
        None,
    );

    if self_settling {
        client = client.with_settles_contract_expirations();
    }

    let node = LiveNodeBuilder::new(TraderId::test_default(), Environment::Live)
        .unwrap()
        .add_exec_client_with_routing(
            Some("client".to_string()),
            Box::new(RoutingClientFactory(client)),
            Box::new(RoutingClientConfig),
            RoutingConfig::default(),
        )
        .unwrap()
        .build()
        .unwrap();
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let close = InstrumentClose::new(
        instrument.id(),
        Price::from("1.000"),
        InstrumentCloseType::ContractExpired,
        UnixNanos::from(300),
        UnixNanos::from(301),
    );

    msgbus::publish_any(
        switchboard::get_instrument_close_topic(instrument.id()),
        &close,
    );

    assert_eq!(
        node.kernel()
            .cache
            .borrow()
            .instrument_close(&instrument.id()),
        expected_settled.then_some(&close)
    );
}

#[derive(Debug)]
struct RoutingClientFactory(StubExecutionClient);

impl ExecutionClientFactory for RoutingClientFactory {
    fn create(
        &self,
        _trader_id: TraderId,
        _name: &str,
        _config: &dyn ClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        Ok(Box::new(self.0.clone().with_instrument_subscription(cache)))
    }

    fn name(&self) -> &'static str {
        "routing"
    }

    fn config_type(&self) -> &'static str {
        "RoutingClientConfig"
    }
}

#[derive(Debug)]
struct RoutingClientConfig;

impl ClientConfig for RoutingClientConfig {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
