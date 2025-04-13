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
    num::NonZeroUsize,
    rc::Rc,
};

use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    messages::data::{
        DataCommand, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10,
        SubscribeBookSnapshots, SubscribeCommand, SubscribeData, SubscribeIndexPrices,
        SubscribeInstrument, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
        UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookSnapshots, UnsubscribeCommand,
        UnsubscribeData, UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeMarkPrices,
        UnsubscribeQuotes, UnsubscribeTrades,
    },
    msgbus::{
        self, MessageBus,
        handler::ShareableMessageHandler,
        stubs::{get_message_saving_handler, get_saved_messages},
        switchboard::{self, MessagingSwitchboard},
    },
    testing::init_logger_for_testing,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarType, Data, DataType, OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10,
        QuoteTick, TradeTick,
        prices::{IndexPriceUpdate, MarkPriceUpdate},
        stubs::{stub_delta, stub_deltas, stub_depth10},
    },
    enums::{BookType, PriceType},
    identifiers::{ClientId, TraderId, Venue},
    instruments::{CurrencyPair, Instrument, InstrumentAny, stubs::audusd_sim},
    types::Price,
};
use rstest::*;

use crate::{
    client::DataClientAdapter,
    engine::{DataEngine, SubscriptionCommandHandler},
    mocks::MockDataClient,
};

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
fn stub_msgbus() -> Rc<RefCell<MessageBus>> {
    MessageBus::new(TraderId::default(), UUID4::new(), None, None).register_message_bus()
}

#[fixture]
fn data_engine(
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let data_engine = DataEngine::new(clock, cache, None);
    Rc::new(RefCell::new(data_engine))
}

#[fixture]
fn data_client(
    client_id: ClientId,
    venue: Venue,
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<TestClock>>,
) -> DataClientAdapter {
    let client = Box::new(MockDataClient::new(cache, client_id, venue));
    DataClientAdapter::new(client_id, venue, true, true, client, clock)
}

#[rstest]
fn test_execute_subscribe_custom_data(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let data_type = DataType::new(stringify!(String), None);
    let cmd = SubscribeData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Data(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_custom_data()
            .contains(&data_type)
    );

    let cmd = UnsubscribeData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Data(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_custom_data()
            .contains(&data_type)
    );
}

#[rstest]
fn test_execute_subscribe_book_deltas(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_book_deltas()
            .contains(&audusd_sim.id)
    );

    let cmd = UnsubscribeBookDeltas::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_book_deltas()
            .contains(&audusd_sim.id)
    );
}

#[ignore = "Attempt to subtract with overflow"]
#[rstest]
fn test_execute_subscribe_book_snapshots(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeBookSnapshots::new(
        audusd_sim.id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        NonZeroUsize::new(1_000).unwrap(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookSnapshots(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_book_snapshots()
            .contains(&audusd_sim.id)
    );

    let cmd = UnsubscribeBookSnapshots::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::BookSnapshots(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_book_snapshots()
            .contains(&audusd_sim.id)
    );
}

#[rstest]
fn test_execute_subscribe_instrument(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeInstrument::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Instrument(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_instruments()
            .contains(&audusd_sim.id)
    );

    let cmd = UnsubscribeInstrument::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Instrument(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_instruments()
            .contains(&audusd_sim.id)
    );
}

#[rstest]
fn test_execute_subscribe_quotes(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_quotes()
            .contains(&audusd_sim.id)
    );

    let cmd = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_quotes()
            .contains(&audusd_sim.id)
    );
}

#[rstest]
fn test_execute_subscribe_trades(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Trades(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_trades()
            .contains(&audusd_sim.id)
    );

    let cmd = UnsubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Trades(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_trades()
            .contains(&audusd_sim.id)
    );
}

#[rstest]
fn test_execute_subscribe_bars(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    init_logger_for_testing(None).unwrap(); // TODO: Remove once initial development completed

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.borrow_mut().process(&audusd_sim as &dyn Any);

    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");

    let cmd = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Bars(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(data_engine.borrow().subscribed_bars().contains(&bar_type));

    let cmd = UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Bars(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert_eq!(audusd_sim.id(), bar_type.instrument_id());
    assert!(!data_engine.borrow().subscribed_bars().contains(&bar_type));
}

#[rstest]
fn test_execute_subscribe_mark_prices(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_mark_prices()
            .contains(&audusd_sim.id)
    );

    let cmd = UnsubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_mark_prices()
            .contains(&audusd_sim.id)
    );
}

#[rstest]
fn test_execute_subscribe_index_prices(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeIndexPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::IndexPrices(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        data_engine
            .borrow()
            .subscribed_index_prices()
            .contains(&audusd_sim.id)
    );

    let cmd = UnsubscribeIndexPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Unsubscribe(UnsubscribeCommand::IndexPrices(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);
    data_engine.borrow_mut().run();

    assert!(
        !data_engine
            .borrow()
            .subscribed_index_prices()
            .contains(&audusd_sim.id)
    );
}

#[rstest]
fn test_process_instrument(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);

    let cmd = SubscribeInstrument::new(
        audusd_sim.id(),
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Instrument(cmd));

    msgbus::send(&endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<InstrumentAny>(None);
    let topic = switchboard::get_instrument_topic(audusd_sim.id());
    msgbus::subscribe(topic, handler.clone(), None);

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
fn test_process_book_delta(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let cmd = SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    let delta = stub_delta();
    let handler = get_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(delta.instrument_id);
    msgbus::subscribe(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Delta(delta));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDeltas>(handler);

    assert_eq!(messages.len(), 1);
}

#[rstest]
fn test_process_book_deltas(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let cmd = SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    // TODO: Using FFI API wrapper temporarily until Cython gone
    let deltas = OrderBookDeltas_API::new(stub_deltas());
    let handler = get_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
    msgbus::subscribe(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Deltas(deltas.clone()));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDeltas>(handler);

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&deltas));
}

#[rstest]
fn test_process_book_depth10(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let cmd = SubscribeBookDepth10::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDepth10(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    let depth = stub_depth10();
    let handler = get_message_saving_handler::<OrderBookDepth10>(None);
    let topic = switchboard::get_book_depth10_topic(depth.instrument_id);
    msgbus::subscribe(topic, handler.clone(), None);

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
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let cmd = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    let quote = QuoteTick::default();
    let handler = get_message_saving_handler::<QuoteTick>(None);
    let topic = switchboard::get_quotes_topic(quote.instrument_id);
    msgbus::subscribe(topic, handler.clone(), None);

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
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let cmd = SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Trades(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    let trade = TradeTick::default();
    let handler = get_message_saving_handler::<TradeTick>(None);
    let topic = switchboard::get_trades_topic(trade.instrument_id);
    msgbus::subscribe(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Trade(trade));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<TradeTick>(handler);

    assert_eq!(cache.trade(&trade.instrument_id), Some(trade).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&trade));
}

#[rstest]
fn test_process_mark_price(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let cmd = SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    let mark_price = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let handler = get_message_saving_handler::<MarkPriceUpdate>(None);
    let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
    msgbus::subscribe(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::MarkPriceUpdate(mark_price));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<MarkPriceUpdate>(handler);

    assert_eq!(
        cache.price(&mark_price.instrument_id, PriceType::Mark),
        Some(mark_price.value)
    );
    assert_eq!(
        cache.mark_price(&mark_price.instrument_id),
        Some(&mark_price)
    );
    assert_eq!(
        cache.mark_prices(&mark_price.instrument_id),
        Some(vec![mark_price])
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&mark_price));
}

#[rstest]
fn test_process_index_price(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let cmd = SubscribeIndexPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::IndexPrices(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    let index_price = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let handler = get_message_saving_handler::<IndexPriceUpdate>(None);
    let topic = switchboard::get_index_price_topic(index_price.instrument_id);
    msgbus::subscribe(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::IndexPriceUpdate(index_price));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<IndexPriceUpdate>(handler);

    assert_eq!(
        cache.index_price(&index_price.instrument_id),
        Some(&index_price)
    );
    assert_eq!(
        cache.index_prices(&index_price.instrument_id),
        Some(vec![index_price])
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&index_price));
}

#[rstest]
fn test_process_bar(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let bar = Bar::default();

    let cmd = SubscribeBars::new(
        bar.bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Bars(cmd));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
        id: endpoint,
        engine_ref: data_engine.clone(),
    }));
    msgbus::register(endpoint, handler);
    msgbus::send(&endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar.bar_type);
    msgbus::subscribe(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Bar(bar));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<Bar>(handler);

    assert_eq!(cache.bar(&bar.bar_type), Some(bar).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&bar));
}
