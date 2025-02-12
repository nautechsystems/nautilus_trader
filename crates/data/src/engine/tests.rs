// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Tests module for `DataEngine`.

use std::{
    any::Any,
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use indexmap::indexmap;
use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    messages::data::{Action, SubscriptionCommand},
    msgbus::{
        handler::ShareableMessageHandler,
        stubs::{get_message_saving_handler, get_saved_messages},
        switchboard::MessagingSwitchboard,
        MessageBus,
    },
    testing::init_logger_for_testing,
};
use nautilus_core::{UnixNanos, UUID4};
use nautilus_model::{
    data::{
        stubs::{stub_delta, stub_deltas, stub_depth10},
        Bar, BarType, Data, DataType, OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10,
        QuoteTick, TradeTick,
    },
    enums::BookType,
    identifiers::{ClientId, TraderId, Venue},
    instruments::{stubs::audusd_sim, CurrencyPair, InstrumentAny},
};
use rstest::*;

use crate::{
    client::DataClientAdapter,
    engine::{DataEngine, SubscriptionCommandHandler},
    mocks::MockDataClient,
};

#[fixture]
fn trader_id() -> TraderId {
    TraderId::default()
}

#[fixture]
fn client_id() -> ClientId {
    ClientId::default()
}

#[fixture]
fn venue() -> Venue {
    Venue::default()
}

#[fixture]
fn clock() -> Rc<RefCell<TestClock>> {
    Rc::new(RefCell::new(TestClock::new()))
}

#[fixture]
fn cache() -> Rc<RefCell<Cache>> {
    // Ensure there is only ever one instance of the cache *per test*
    thread_local! {
        static CACHE: OnceCell<Rc<RefCell<Cache>>> = const { OnceCell::new() };
    }
    Rc::new(RefCell::new(Cache::default()))
}

#[fixture]
fn msgbus(trader_id: TraderId) -> Rc<RefCell<MessageBus>> {
    // Ensure there is only ever one instance of the message bus *per test*
    thread_local! {
        static MSGBUS: OnceCell<Rc<RefCell<MessageBus>>> = const { OnceCell::new() };
    }

    MSGBUS.with(|cell| {
        cell.get_or_init(|| {
            Rc::new(RefCell::new(MessageBus::new(
                trader_id,
                UUID4::new(),
                None,
                None,
            )))
        })
        .clone()
    })
}

#[fixture]
fn switchboard(msgbus: Rc<RefCell<MessageBus>>) -> MessagingSwitchboard {
    msgbus.borrow().switchboard.clone()
}

#[fixture]
fn data_engine(
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
) -> Rc<RefCell<DataEngine>> {
    let data_engine = DataEngine::new(clock, cache, msgbus, None);
    Rc::new(RefCell::new(data_engine))
}

#[fixture]
fn data_client(
    client_id: ClientId,
    venue: Venue,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    clock: Rc<RefCell<TestClock>>,
) -> DataClientAdapter {
    let client = Box::new(MockDataClient::new(cache, msgbus, client_id, venue));
    DataClientAdapter::new(client_id, venue, true, true, client, clock)
}

#[rstest]
fn test_execute_subscribe_custom_data(
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let data_type = DataType::new(stringify!(String), None);
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine
        .borrow()
        .subscribed_custom_data()
        .contains(&data_type));

    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Unsubscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(!data_engine
        .borrow()
        .subscribed_custom_data()
        .contains(&data_type));
}

#[rstest]
fn test_execute_subscribe_order_book_deltas(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
        "book_type".to_string() => BookType::L3_MBO.to_string(),
        "managed".to_string() => "true".to_string(),
    };
    let data_type = DataType::new(stringify!(OrderBookDelta), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine
        .borrow()
        .subscribed_order_book_deltas()
        .contains(&audusd_sim.id));

    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Unsubscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(!data_engine
        .borrow()
        .subscribed_order_book_deltas()
        .contains(&audusd_sim.id));
}

#[rstest]
fn test_execute_subscribe_order_book_snapshots(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
        "book_type".to_string() => BookType::L2_MBP.to_string(),
        "managed".to_string() => "true".to_string(),
    };
    let data_type = DataType::new(stringify!(OrderBookDeltas), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine
        .borrow()
        .subscribed_order_book_snapshots()
        .contains(&audusd_sim.id));

    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Unsubscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(!data_engine
        .borrow()
        .subscribed_order_book_snapshots()
        .contains(&audusd_sim.id));
}

#[rstest]
fn test_execute_subscribe_instrument(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
    };
    let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine
        .borrow()
        .subscribed_instruments()
        .contains(&audusd_sim.id));

    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Unsubscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(!data_engine
        .borrow()
        .subscribed_instruments()
        .contains(&audusd_sim.id));
}

#[rstest]
fn test_execute_subscribe_quote_ticks(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
    };
    let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine
        .borrow()
        .subscribed_quote_ticks()
        .contains(&audusd_sim.id));

    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Unsubscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(!data_engine
        .borrow()
        .subscribed_quote_ticks()
        .contains(&audusd_sim.id));
}

#[rstest]
fn test_execute_subscribe_trade_ticks(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
    };
    let data_type = DataType::new(stringify!(TradeTick), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine
        .borrow()
        .subscribed_trade_ticks()
        .contains(&audusd_sim.id));

    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Unsubscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(!data_engine
        .borrow()
        .subscribed_trade_ticks()
        .contains(&audusd_sim.id));
}

#[rstest]
fn test_execute_subscribe_bars(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    init_logger_for_testing(None); // TODO: Remove once initial development completed

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.borrow_mut().process(&audusd_sim as &dyn Any);

    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");
    let metadata = indexmap! {
        "bar_type".to_string() => bar_type.to_string(),
    };
    let data_type = DataType::new(stringify!(Bar), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type.clone(),
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine.borrow().subscribed_bars().contains(&bar_type));

    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Unsubscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert_eq!(audusd_sim.id(), bar_type.instrument_id());
    assert!(!data_engine.borrow().subscribed_bars().contains(&bar_type));
}

#[rstest]
fn test_process_instrument(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id().to_string(),
    };
    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);

    let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<InstrumentAny>(None);
    {
        let mut msgbus = msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_instrument_topic(audusd_sim.id());
        msgbus.subscribe(topic, handler.clone(), None);
    }

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process(&audusd_sim as &dyn Any);
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<InstrumentAny>(handler);

    assert_eq!(
        cache.instrument(&audusd_sim.id()),
        Some(audusd_sim.clone()).as_ref()
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&audusd_sim));
}

#[rstest]
fn test_process_order_book_delta(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
        "book_type".to_string() => BookType::L3_MBO.to_string(),
        "managed".to_string() => "true".to_string(),
    };
    let data_type = DataType::new(stringify!(OrderBookDelta), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

    let delta = stub_delta();
    let handler = get_message_saving_handler::<OrderBookDeltas>(None);
    {
        let mut msgbus = msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_deltas_topic(delta.instrument_id);
        msgbus.subscribe(topic, handler.clone(), None);
    }

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Delta(delta));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDeltas>(handler);

    assert_eq!(messages.len(), 1);
}

#[rstest]
fn test_process_order_book_deltas(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
        "book_type".to_string() => BookType::L3_MBO.to_string(),
        "managed".to_string() => "true".to_string(),
    };
    let data_type = DataType::new(stringify!(OrderBookDeltas), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

    // TODO: Using FFI API wrapper temporarily until Cython gone
    let deltas = OrderBookDeltas_API::new(stub_deltas());
    let handler = get_message_saving_handler::<OrderBookDeltas>(None);
    {
        let mut msgbus = msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_deltas_topic(deltas.instrument_id);
        msgbus.subscribe(topic, handler.clone(), None);
    }

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Deltas(deltas.clone()));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDeltas>(handler);

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&deltas));
}

#[rstest]
fn test_process_order_book_depth10(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
        "book_type".to_string() => BookType::L3_MBO.to_string(),
        "managed".to_string() => "true".to_string(),
    };
    let data_type = DataType::new(stringify!(OrderBookDepth10), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

    let depth = stub_depth10();
    let handler = get_message_saving_handler::<OrderBookDepth10>(None);
    {
        let mut msgbus = msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_depth_topic(depth.instrument_id);
        msgbus.subscribe(topic, handler.clone(), None);
    }

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::from(depth));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDepth10>(handler);

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&depth));
}

#[rstest]
fn test_process_quote_tick(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
    };
    let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

    let quote = QuoteTick::default();
    let handler = get_message_saving_handler::<QuoteTick>(None);
    {
        let mut msgbus = msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_quotes_topic(quote.instrument_id);
        msgbus.subscribe(topic, handler.clone(), None);
    }

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Quote(quote));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<QuoteTick>(handler);

    assert_eq!(cache.quote(&quote.instrument_id), Some(quote).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&quote));
}

#[rstest]
fn test_process_trade_tick(
    audusd_sim: CurrencyPair,
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let metadata = indexmap! {
        "instrument_id".to_string() => audusd_sim.id.to_string(),
    };
    let data_type = DataType::new(stringify!(TradeTick), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

    let trade = TradeTick::default();
    let handler = get_message_saving_handler::<TradeTick>(None);
    {
        let mut msgbus = msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_trades_topic(trade.instrument_id);
        msgbus.subscribe(topic, handler.clone(), None);
    }

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Trade(trade));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<TradeTick>(handler);

    assert_eq!(cache.trade(&trade.instrument_id), Some(trade).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&trade));
}

#[rstest]
fn test_process_bar(
    msgbus: Rc<RefCell<MessageBus>>,
    switchboard: MessagingSwitchboard,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let bar = Bar::default();
    let metadata = indexmap! {
        "bar_type".to_string() => bar.bar_type.to_string(),
    };
    let data_type = DataType::new(stringify!(Bar), Some(metadata));
    let cmd = SubscriptionCommand::new(
        client_id,
        venue,
        data_type,
        Action::Subscribe,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let endpoint = switchboard.data_engine_execute;
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus.borrow_mut().register(endpoint, handler);
    msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<Bar>(None);
    {
        let mut msgbus = msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_bars_topic(bar.bar_type);
        msgbus.subscribe(topic, handler.clone(), None);
    }

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Bar(bar));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<Bar>(handler);

    assert_eq!(cache.bar(&bar.bar_type), Some(bar).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&bar));
}
