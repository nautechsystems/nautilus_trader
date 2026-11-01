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

use nautilus_common::{
    enums::SerializationEncoding,
    msgbus::{
        BusMessage, BusMessageSource, MessageBusExternalEgress, current_external_source,
        get_message_bus, process_external_typed_message, publish_bar, publish_deltas,
        publish_depth, publish_funding_rate, publish_index_price, publish_instrument,
        publish_mark_price, publish_option_greeks, publish_quote, publish_trade,
        republish_external_message, set_message_bus, subscribe_any, subscribe_bars,
        subscribe_book_deltas, subscribe_book_depth, subscribe_funding_rates,
        subscribe_index_prices, subscribe_instruments, subscribe_mark_prices,
        subscribe_option_greeks, subscribe_quotes, subscribe_trades, subscriber_count_deltas,
        subscriber_count_depth,
        switchboard::{
            get_bars_topic, get_book_deltas_topic, get_book_depth_topic, get_funding_rate_topic,
            get_index_price_topic, get_instrument_topic, get_mark_price_topic,
            get_option_greeks_topic, get_quotes_topic, get_trades_topic,
        },
        unsubscribe_any,
    },
};
use rust_decimal::Decimal;
use serde_json::to_value;

use super::*;

#[derive(Default)]
struct CapturingEgress(Rc<RefCell<Vec<BusMessage>>>);

impl MessageBusExternalEgress for CapturingEgress {
    fn is_closed(&self) -> bool {
        false
    }

    fn publish(&self, message: BusMessage) {
        self.0.borrow_mut().push(message);
    }

    fn close(&mut self) {}
}

#[fixture]
fn data_engine_external(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let _ = stub_msgbus;

    let config = DataEngineConfig {
        external_clients: Some(vec!["EXTERNAL".into(), "OTHER".into()]),
        ..Default::default()
    };

    let engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, Some(config))));
    DataEngine::register_msgbus_handlers(&engine);
    engine
}

fn external_message<T: 'static>(
    value: &T,
    topic: MStr<Topic>,
    encoding: SerializationEncoding,
    publish: fn(MStr<Topic>, &T),
) -> BusMessage {
    let consumer = get_message_bus();
    set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
    let egress = CapturingEgress::default();
    let messages = Rc::clone(&egress.0);
    get_message_bus()
        .borrow_mut()
        .set_external_egress(Box::new(egress), encoding);
    publish(topic, value);
    assert_eq!(messages.borrow().len(), 1);
    let mut message = messages.borrow_mut().pop().unwrap();
    set_message_bus(consumer);
    message.source = Some(BusMessageSource {
        publisher_id: "PUBLISHER-001".into(),
        instance_id: UUID4::new(),
        stream: "market-data".into(),
        sequence: 17,
    });

    message
}

#[rstest]
#[case::quotes(true)]
#[case::trades(false)]
fn test_external_ticks_update_cache_before_publication(
    #[case] quotes: bool,
    #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
    encoding: SerializationEncoding,
    #[values(false, true)] processors: bool,
    #[values(false, true)] reset: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    if reset {
        data_engine_external.borrow_mut().reset();
    }

    let cache = Rc::clone(data_engine_external.borrow().cache());
    let observed = Rc::new(RefCell::new(Vec::new()));

    let quote = QuoteTick {
        bid_price: Price::from("1.23456"),
        ask_price: Price::from("1.23478"),
        bid_size: Quantity::from(12),
        ask_size: Quantity::from(34),
        ts_event: 123.into(),
        ts_init: 456.into(),
        ..Default::default()
    };

    let trade = TradeTick {
        price: Price::from("1.23467"),
        size: Quantity::from(56),
        ts_event: 234.into(),
        ts_init: 567.into(),
        ..Default::default()
    };

    let message = if quotes {
        let topic = get_quotes_topic(quote.instrument_id);

        let handler = TypedHandler::from({
            let cache = Rc::clone(&cache);
            let observed = Rc::clone(&observed);
            move |received: &QuoteTick| {
                assert_eq!(*received, quote);
                assert_eq!(cache.borrow().quote(&quote.instrument_id), Some(&quote));
                observed.borrow_mut().push(current_external_source());
            }
        });

        subscribe_quotes(topic.into(), handler, None);
        external_message(&quote, topic, encoding, publish_quote)
    } else {
        let topic = get_trades_topic(trade.instrument_id);

        let handler = TypedHandler::from({
            let cache = Rc::clone(&cache);
            let observed = Rc::clone(&observed);
            move |received: &TradeTick| {
                assert_eq!(*received, trade);
                assert_eq!(cache.borrow().trade(&trade.instrument_id), Some(&trade));
                observed.borrow_mut().push(current_external_source());
            }
        });

        subscribe_trades(topic.into(), handler, None);
        external_message(&trade, topic, encoding, publish_trade)
    };

    let egress = CapturingEgress::default();
    let publications = Rc::clone(&egress.0);
    {
        let bus = get_message_bus();
        let mut bus = bus.borrow_mut();
        bus.add_streaming_type(message.payload_type);
        bus.set_external_egress(Box::new(egress), encoding);
    }

    if processors {
        process_external_typed_message(&message, &mut |_, _| {
            panic!("market data bypasses typed stream processors")
        })
        .unwrap();
    } else {
        republish_external_message(&message).unwrap();
    }

    assert_eq!(*observed.borrow(), vec![message.source]);
    assert_eq!(data_engine_external.borrow().data_count(), 1);
    assert!(publications.borrow().is_empty());
    assert_eq!(current_external_source(), None);

    if quotes {
        let topic = get_quotes_topic(quote.instrument_id);
        publish_quote(topic, &quote);
    } else {
        let topic = get_trades_topic(trade.instrument_id);
        publish_trade(topic, &trade);
    }

    assert_eq!(*observed.borrow(), vec![message.source, None]);
    assert_eq!(publications.borrow().len(), 1);
}

#[rstest]
fn test_external_instrument_updates_cache_before_publication(
    #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
    encoding: SerializationEncoding,
    data_engine_external: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
) {
    let cache = Rc::clone(data_engine_external.borrow().cache());
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let topic = get_instrument_topic(instrument.id());
    let (handler, saver) = get_typed_message_saving_handler::<InstrumentAny>(None);
    subscribe_instruments(topic.into(), handler, None);

    let checked = TypedHandler::from({
        let expected = instrument.clone();
        move |received: &InstrumentAny| {
            assert_eq!(*received, expected);
            assert_eq!(cache.borrow().instrument(&expected.id()), Some(&expected));
        }
    });

    subscribe_instruments(topic.into(), checked, Some(20));
    let message = external_message(&instrument, topic, encoding, publish_instrument);
    get_message_bus()
        .borrow_mut()
        .add_streaming_type(BusPayloadType::Instrument);

    republish_external_message(&message).unwrap();

    assert_eq!(saver.get_messages(), vec![instrument]);
    assert_eq!(data_engine_external.borrow().data_count(), 1);
}

#[rstest]
#[case(None)]
#[case(Some(Vec::new()))]
fn test_external_canonical_topic_without_external_clients_keeps_raw_forwarding(
    #[case] external_clients: Option<Vec<ClientId>>,
    #[values(false, true)] external_exec_client: bool,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    if external_exec_client {
        cache.borrow_mut().add_external_exec_client("EXEC".into());
    }

    let config = DataEngineConfig {
        external_clients,
        ..Default::default()
    };

    let engine = Rc::new(RefCell::new(DataEngine::new(
        clock,
        Rc::clone(&cache),
        Some(config),
    )));
    DataEngine::register_msgbus_handlers(&engine);

    let quote = QuoteTick {
        bid_price: Price::from("1.11001"),
        ask_price: Price::from("1.11003"),
        bid_size: Quantity::from(11),
        ask_size: Quantity::from(33),
        ts_event: 123.into(),
        ts_init: 456.into(),
        ..Default::default()
    };

    let topic = get_quotes_topic(quote.instrument_id);
    let message = external_message(&quote, topic, SerializationEncoding::Json, publish_quote);
    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    subscribe_quotes(topic.into(), handler, None);
    stub_msgbus
        .borrow_mut()
        .add_streaming_type(BusPayloadType::QuoteTick);

    republish_external_message(&message).unwrap();

    assert_eq!(saver.get_messages(), vec![quote]);
    assert_eq!(cache.borrow().quote(&quote.instrument_id), None);
    assert_eq!(engine.borrow().data_count(), 0);
}

#[rstest]
fn test_external_ingress_uses_existing_cache_registrations(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    cache
        .borrow_mut()
        .add_external_data_client("EXTERNAL".into());

    let engine = Rc::new(RefCell::new(DataEngine::new(
        clock,
        Rc::clone(&cache),
        None,
    )));
    DataEngine::register_msgbus_handlers(&engine);

    let quote = QuoteTick {
        bid_price: Price::from("1.22001"),
        ask_price: Price::from("1.22005"),
        bid_size: Quantity::from(22),
        ask_size: Quantity::from(55),
        ts_event: 234.into(),
        ts_init: 567.into(),
        ..Default::default()
    };

    let topic = get_quotes_topic(quote.instrument_id);
    let message = external_message(&quote, topic, SerializationEncoding::Json, publish_quote);
    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    subscribe_quotes(topic.into(), handler, None);
    stub_msgbus
        .borrow_mut()
        .add_streaming_type(BusPayloadType::QuoteTick);

    republish_external_message(&message).unwrap();

    assert_eq!(saver.get_messages(), vec![quote]);
    assert_eq!(cache.borrow().quote(&quote.instrument_id), Some(&quote));
    assert_eq!(engine.borrow().data_count(), 1);
}

#[rstest]
#[case("application.quotes")]
#[case("data.quotes.SIM.EURUSD")]
#[case("data.quotes.SIM.AUDUSD.extra")]
fn test_external_application_topic_keeps_raw_forwarding(
    data_engine_external: Rc<RefCell<DataEngine>>,
    #[case] topic: &str,
) {
    let cache = Rc::clone(data_engine_external.borrow().cache());
    let quote = QuoteTick::default();
    let topic: MStr<Topic> = topic.into();
    let message = external_message(&quote, topic, SerializationEncoding::Json, publish_quote);
    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    subscribe_quotes(topic.into(), handler, None);
    get_message_bus()
        .borrow_mut()
        .add_streaming_type(BusPayloadType::QuoteTick);

    republish_external_message(&message).unwrap();

    assert_eq!(saver.get_messages(), vec![quote]);
    assert_eq!(cache.borrow().quote(&quote.instrument_id), None);
    assert_eq!(data_engine_external.borrow().data_count(), 0);
}

fn book_command(
    id: InstrumentId,
    client_id: ClientId,
    depth: bool,
    managed: bool,
) -> SubscribeCommand {
    if depth {
        SubscribeCommand::BookDepth(SubscribeBookDepth::new(
            id,
            BookType::L2_MBP,
            Some(client_id),
            None,
            UUID4::new(),
            1.into(),
            None,
            managed,
            None,
            None,
        ))
    } else {
        SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            id,
            BookType::L2_MBP,
            Some(client_id),
            None,
            UUID4::new(),
            1.into(),
            None,
            managed,
            None,
            None,
        ))
    }
}

#[rstest]
fn test_external_book_owners_and_replays_preserve_updater_lifetime(
    #[values(false, true)] depth: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let id = stub_deltas().instrument_id;
    let client_id = ClientId::from("EXTERNAL");
    let first = book_command(id, client_id, depth, false);
    let second = book_command(id, client_id, depth, true);
    let topic = if depth {
        get_book_depth_topic(id)
    } else {
        get_book_deltas_topic(id)
    };

    let subscribers = |topic| {
        if depth {
            subscriber_count_depth(topic)
        } else {
            subscriber_count_deltas(topic)
        }
    };

    let mut engine = data_engine_external.borrow_mut();

    engine.execute_subscribe(first.clone()).unwrap();
    assert_eq!(subscribers(topic), 0);
    engine.execute_subscribe(second.clone()).unwrap();
    engine.execute_subscribe(second.clone()).unwrap();
    assert_eq!(subscribers(topic), 1);

    let unsubscribe = first.into_unsubscribe(UUID4::new(), 2.into(), None);
    engine.execute_unsubscribe(&unsubscribe).unwrap();
    assert_eq!(subscribers(topic), 1);
    let unsubscribe = second.into_unsubscribe(UUID4::new(), 3.into(), None);
    engine.execute_unsubscribe(&unsubscribe).unwrap();
    assert_eq!(subscribers(topic), 0);

    let rejoin = book_command(id, client_id, depth, true);
    engine.execute_subscribe(rejoin.clone()).unwrap();
    assert_eq!(subscribers(topic), 1);
    let unsubscribe = rejoin.into_unsubscribe(UUID4::new(), 4.into(), None);
    engine.execute_unsubscribe(&unsubscribe).unwrap();
    assert_eq!(subscribers(topic), 0);
}

#[rstest]
fn test_external_managed_books_apply_before_callbacks(
    #[values(false, true)] depth: bool,
    #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
    encoding: SerializationEncoding,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let cache = Rc::clone(data_engine_external.borrow().cache());
    let deltas = stub_deltas();
    let book_depth = stub_depth10();
    let id = if depth {
        book_depth.instrument_id
    } else {
        deltas.instrument_id
    };

    let command = book_command(id, "EXTERNAL".into(), depth, true);
    data_engine_external
        .borrow_mut()
        .execute_subscribe(command)
        .unwrap();
    let mut expected = OrderBook::new(id, BookType::L2_MBP);
    let observed = Rc::new(RefCell::new(0));

    let message = if depth {
        expected.apply_depth(&book_depth).unwrap();
        let topic = get_book_depth_topic(id);

        let handler = TypedHandler::from({
            let observed = Rc::clone(&observed);
            move |received: &OrderBookDepth| {
                assert_eq!(*received, book_depth);
                let cache = cache.borrow();
                let actual = cache.order_book(&id).unwrap();
                assert_eq!(
                    actual.to_deltas(10.into(), 20.into()),
                    expected.to_deltas(10.into(), 20.into())
                );
                *observed.borrow_mut() += 1;
            }
        });

        subscribe_book_depth(topic.into(), handler, None);
        external_message(&stub_depth10(), topic, encoding, publish_depth)
    } else {
        expected.apply_deltas(&deltas).unwrap();
        let topic = get_book_deltas_topic(id);
        let message = external_message(&deltas, topic, encoding, publish_deltas);

        let handler = TypedHandler::from({
            let observed = Rc::clone(&observed);
            move |received: &OrderBookDeltas| {
                assert_eq!(*received, deltas);
                let cache = cache.borrow();
                let actual = cache.order_book(&id).unwrap();
                assert_eq!(
                    actual.to_deltas(10.into(), 20.into()),
                    expected.to_deltas(10.into(), 20.into())
                );
                *observed.borrow_mut() += 1;
            }
        });

        subscribe_book_deltas(topic.into(), handler, None);
        message
    };

    republish_external_message(&message).unwrap();

    assert_eq!(*observed.borrow(), 1);
    assert_eq!(data_engine_external.borrow().data_count(), 1);
}

#[rstest]
fn test_external_managed_books_reject_conflicting_clients_without_retaining_owner(
    #[values(false, true)] depth: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let id = stub_deltas().instrument_id;
    let first = book_command(id, "EXTERNAL".into(), depth, true);
    let rejected = book_command(id, "OTHER".into(), depth, true);
    let mut engine = data_engine_external.borrow_mut();
    engine.execute_subscribe(first.clone()).unwrap();

    let error = engine.execute_subscribe(rejected.clone()).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "Conflicting book subscription for {id}: shared book sources must use the same client, book type, depth, and parameters"
        )
    );
    let unsubscribe = first.into_unsubscribe(UUID4::new(), 2.into(), None);
    engine.execute_unsubscribe(&unsubscribe).unwrap();
    engine.execute_subscribe(rejected).unwrap();

    assert_eq!(
        subscriber_count_depth(get_book_depth_topic(id)),
        usize::from(depth)
    );
    assert_eq!(
        subscriber_count_deltas(get_book_deltas_topic(id)),
        usize::from(!depth)
    );
}

macro_rules! cached_external_data {
    ($name:ident, $ty:ty, $value:expr, $key:ident, $topic:ident, $publish:ident, $subscribe:ident, $cached:ident, $payload:ident) => {
        #[rstest]
        fn $name(
            #[values(SerializationEncoding::Json, SerializationEncoding::MsgPack)]
            encoding: SerializationEncoding,
            data_engine_external: Rc<RefCell<DataEngine>>,
        ) {
            let value: $ty = $value;
            let topic = $topic(value.$key);
            let message = external_message(&value, topic, encoding, $publish);
            let cache = Rc::clone(data_engine_external.borrow().cache());
            let observed = Rc::new(RefCell::new(Vec::new()));
            let handler = TypedHandler::from({
                let observed = Rc::clone(&observed);
                move |received: &$ty| {
                    let cached = cache.borrow().$cached(&received.$key).cloned().unwrap();
                    assert_eq!(cached, value);
                    assert_eq!(cached.ts_event, value.ts_event);
                    assert_eq!(cached.ts_init, value.ts_init);
                    observed.borrow_mut().push(*received);
                }
            });
            $subscribe(topic.into(), handler, None);
            get_message_bus()
                .borrow_mut()
                .add_streaming_type(BusPayloadType::$payload);

            republish_external_message(&message).unwrap();

            assert_eq!(*observed.borrow(), vec![value]);
            assert_eq!(data_engine_external.borrow().data_count(), 1);
        }
    };
}

cached_external_data!(
    test_external_bar_cache,
    Bar,
    Bar {
        ts_event: 123.into(),
        ts_init: 456.into(),
        ..Default::default()
    },
    bar_type,
    get_bars_topic,
    publish_bar,
    subscribe_bars,
    bar,
    Bar
);
cached_external_data!(
    test_external_mark_price_cache,
    MarkPriceUpdate,
    MarkPriceUpdate::new(
        "AUDUSD.SIM".into(),
        Price::from("1.23456"),
        123.into(),
        456.into()
    ),
    instrument_id,
    get_mark_price_topic,
    publish_mark_price,
    subscribe_mark_prices,
    mark_price,
    MarkPriceUpdate
);
cached_external_data!(
    test_external_index_price_cache,
    IndexPriceUpdate,
    IndexPriceUpdate::new(
        "AUDUSD.SIM".into(),
        Price::from("1.34567"),
        234.into(),
        567.into()
    ),
    instrument_id,
    get_index_price_topic,
    publish_index_price,
    subscribe_index_prices,
    index_price,
    IndexPriceUpdate
);
cached_external_data!(
    test_external_funding_rate_cache,
    FundingRateUpdate,
    FundingRateUpdate::new(
        "AUDUSD.SIM".into(),
        Decimal::new(123, 7),
        Some(480),
        Some(999.into()),
        345.into(),
        678.into()
    ),
    instrument_id,
    get_funding_rate_topic,
    publish_funding_rate,
    subscribe_funding_rates,
    funding_rate,
    FundingRateUpdate
);
cached_external_data!(
    test_external_option_greeks_cache,
    OptionGreeks,
    OptionGreeks {
        instrument_id: "AUDUSD.SIM".into(),
        convention: GreeksConvention::PriceAdjusted,
        greeks: OptionGreekValues {
            delta: 0.125,
            gamma: 0.25,
            vega: 0.375,
            theta: -0.5,
            rho: 0.625
        },
        mark_iv: Some(0.125),
        bid_iv: Some(0.25),
        ask_iv: Some(0.375),
        underlying_price: Some(123.0),
        open_interest: Some(456.0),
        ts_event: 456.into(),
        ts_init: 789.into(),
    },
    instrument_id,
    get_option_greeks_topic,
    publish_option_greeks,
    subscribe_option_greeks,
    option_greeks,
    OptionGreeks
);

#[rstest]
fn test_external_book_batch_preserves_valid_members_after_conflict(
    #[values(false, true)] depth: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let fresh_id = InstrumentId::from("FRESH.SIM");
    let conflict_id = InstrumentId::from("OCCUPIED.SIM");
    let occupied = book_command(conflict_id, "OTHER".into(), depth, true);
    let mut engine = data_engine_external.borrow_mut();
    engine.execute_subscribe(occupied.clone()).unwrap();
    let first = book_command(fresh_id, "EXTERNAL".into(), depth, true);
    let batch = first
        .with_instrument_ids(vec![fresh_id, conflict_id])
        .unwrap();
    let (handler, saver) = get_any_saving_handler::<SubscribeCommand>(None);
    subscribe_any("commands.data.EXTERNAL".into(), handler, None);

    let error = engine.execute_subscribe(batch).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!(
            "Conflicting book subscription for {conflict_id}: shared book sources must use the same client, book type, depth, and parameters"
        )
    );
    assert_eq!(
        to_value(saver.get_messages()).unwrap(),
        to_value([&first]).unwrap()
    );
    assert!(engine.has_book_subscription(&fresh_id));

    let unsubscribe = occupied.into_unsubscribe(UUID4::new(), 2.into(), None);
    engine.execute_unsubscribe(&unsubscribe).unwrap();
    let retry = book_command(conflict_id, "EXTERNAL".into(), depth, true);
    engine.execute_subscribe(retry.clone()).unwrap();

    assert_eq!(
        to_value(saver.get_messages()).unwrap(),
        to_value([first, retry]).unwrap()
    );
}

#[rstest]
fn test_external_untracked_book_unsubscribe_still_forwards(
    #[values(false, true)] depth: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let command = book_command("UNTRACKED.SIM".into(), "EXTERNAL".into(), depth, true)
        .into_unsubscribe(UUID4::new(), 123.into(), None);
    let (handler, saver) = get_any_saving_handler::<UnsubscribeCommand>(None);
    subscribe_any("commands.data.EXTERNAL".into(), handler, None);

    data_engine_external
        .borrow_mut()
        .execute_unsubscribe(&command)
        .unwrap();

    assert_eq!(
        to_value(saver.get_messages()).unwrap(),
        to_value([command]).unwrap()
    );
}

#[rstest]
#[case::rejected_owner(false)]
#[case::replayed_release(true)]
fn test_external_unmatched_book_release_preserves_surviving_owner(
    #[case] replay: bool,
    #[values(false, true)] depth: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let id = InstrumentId::from("SURVIVOR.SIM");
    let surviving = book_command(id, "EXTERNAL".into(), depth, true);
    let mut other = book_command(id, "EXTERNAL".into(), depth, true);
    if !replay {
        match &mut other {
            SubscribeCommand::BookDeltas(command) => command.depth = NonZeroUsize::new(7),
            SubscribeCommand::BookDepth(command) => command.depth = NonZeroUsize::new(7),
            _ => unreachable!(),
        }
    }

    let mut engine = data_engine_external.borrow_mut();
    engine.execute_subscribe(surviving.clone()).unwrap();
    let (handler, saver) = get_any_saving_handler::<UnsubscribeCommand>(None);
    subscribe_any("commands.data.EXTERNAL".into(), handler, None);
    let other_id = other.command_id();
    let release = other
        .clone()
        .into_unsubscribe(UUID4::new(), 123.into(), Some(other_id));

    if replay {
        engine.execute_subscribe(other).unwrap();
        engine.execute_unsubscribe(&release).unwrap();
    } else {
        let error = engine.execute_subscribe(other).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "Conflicting book subscription for {id}: shared book sources must use the same client, book type, depth, and parameters"
            )
        );
    }

    engine.execute_unsubscribe(&release).unwrap();

    assert_eq!(saver.get_messages(), Vec::<UnsubscribeCommand>::new());
    assert!(engine.has_book_subscription(&id));
    assert_eq!(
        subscriber_count_depth(get_book_depth_topic(id)),
        usize::from(depth)
    );
    assert_eq!(
        subscriber_count_deltas(get_book_deltas_topic(id)),
        usize::from(!depth)
    );

    let surviving_id = surviving.command_id();
    let final_release = surviving.into_unsubscribe(UUID4::new(), 456.into(), Some(surviving_id));
    engine.execute_unsubscribe(&final_release).unwrap();

    assert_eq!(
        to_value(saver.get_messages()).unwrap(),
        to_value([final_release]).unwrap()
    );
    assert!(!engine.has_book_subscription(&id));
    assert_eq!(subscriber_count_depth(get_book_depth_topic(id)), 0);
    assert_eq!(subscriber_count_deltas(get_book_deltas_topic(id)), 0);
}

#[rstest]
#[case::single(vec!["SYNTH.SYNTH"])]
#[case::synthetic_first(vec!["SYNTH.SYNTH", "AUDUSD.SIM"])]
#[case::synthetic_last(vec!["AUDUSD.SIM", "SYNTH.SYNTH"])]
fn test_external_synthetic_books_preserve_forwarding_and_ownership(
    #[case] ids: Vec<&str>,
    #[values(false, true)] depth: bool,
    #[values(false, true)] managed: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let ids: Vec<InstrumentId> = ids.into_iter().map(InstrumentId::from).collect();
    let command = book_command(ids[0], "EXTERNAL".into(), depth, managed)
        .with_instrument_ids(ids.clone())
        .unwrap();
    let acquisition_id = command.command_id();
    let (handler, subscribes) = get_any_saving_handler::<SubscribeCommand>(None);
    subscribe_any("commands.data.EXTERNAL".into(), handler.clone(), None);
    let mut engine = data_engine_external.borrow_mut();

    engine.execute_subscribe(command.clone()).unwrap();
    engine.execute_subscribe(command.clone()).unwrap();

    assert_eq!(
        to_value(subscribes.get_messages()).unwrap(),
        to_value([&command]).unwrap()
    );

    let payload_type = if depth {
        BusPayloadType::OrderBookDepth
    } else {
        BusPayloadType::OrderBookDeltas
    };

    assert!(get_message_bus().borrow().is_streaming_type(payload_type));

    for id in &ids {
        assert!(engine.has_book_subscription(id));
        assert_eq!(
            subscriber_count_depth(get_book_depth_topic(*id)),
            usize::from(depth && managed)
        );
        assert_eq!(
            subscriber_count_deltas(get_book_deltas_topic(*id)),
            usize::from(!depth && managed)
        );
        let actual = engine
            .cache()
            .borrow()
            .order_book(id)
            .map(|book| book.to_deltas(10.into(), 20.into()));

        let expected = managed.then(|| {
            let book = OrderBook::new(*id, BookType::L2_MBP);
            book.to_deltas(10.into(), 20.into())
        });

        assert_eq!(actual, expected);
    }

    unsubscribe_any("commands.data.EXTERNAL".into(), &handler);
    let (handler, unsubscribes) = get_any_saving_handler::<UnsubscribeCommand>(None);
    subscribe_any("commands.data.EXTERNAL".into(), handler, None);
    let release = command.into_unsubscribe(UUID4::new(), 123.into(), Some(acquisition_id));
    engine.execute_unsubscribe(&release).unwrap();

    assert_eq!(
        to_value(unsubscribes.get_messages()).unwrap(),
        to_value([release]).unwrap()
    );

    for id in &ids {
        assert!(!engine.has_book_subscription(id));
        assert_eq!(subscriber_count_depth(get_book_depth_topic(*id)), 0);
        assert_eq!(subscriber_count_deltas(get_book_deltas_topic(*id)), 0);
    }
}

#[rstest]
fn test_local_synthetic_books_remain_unsupported(
    #[values(false, true)] depth: bool,
    #[values(false, true)] managed: bool,
    data_engine_external: Rc<RefCell<DataEngine>>,
) {
    let id = InstrumentId::from("SYNTH.SYNTH");
    let command = book_command(id, "LOCAL".into(), depth, managed);
    let mut engine = data_engine_external.borrow_mut();

    let error = engine.execute_subscribe(command).unwrap_err();

    let payload = if depth {
        "OrderBookDepth"
    } else {
        "OrderBookDelta"
    };

    assert_eq!(
        error.to_string(),
        format!("Cannot subscribe for synthetic instrument `{payload}` data")
    );
    assert!(!engine.has_book_subscription(&id));
    assert_eq!(subscriber_count_depth(get_book_depth_topic(id)), 0);
    assert_eq!(subscriber_count_deltas(get_book_deltas_topic(id)), 0);
    assert!(engine.cache().borrow().order_book(&id).is_none());
}
