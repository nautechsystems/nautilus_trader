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

use super::*;

#[rstest]
fn test_request_scoped_bar_aggregator_runs_alongside_live_subscription(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let live_subscribe = DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(live_subscribe);

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buy,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.process_data(Data::Trade(make_trade(1_000, "live-1")));
    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(
        request.clone(),
    )));

    assert_eq!(
        recorder.borrow().last(),
        Some(&DataCommand::Request(RequestCommand::Trades(request))),
    );

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_trade(2_000, "historical-1")],
        None,
        None,
        UnixNanos::from(2_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
        "request-scoped aggregator must process the historical response",
    );

    data_engine.process_data(Data::Trade(make_trade(3_000, "live-2")));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(3_000)),
        "live aggregator must remain subscribed after request cleanup",
    );
}

#[rstest]
fn test_request_scoped_quote_bar_aggregators_handle_multiple_bar_types(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let one_tick = BarType::from(format!("{instrument_id}-1-TICK-BID-INTERNAL").as_str());
    let two_tick = BarType::from(format!("{instrument_id}-2-TICK-BID-INTERNAL").as_str());
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [one_tick.to_string(), two_tick.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();

    let request = RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));

    let make_quote = |ts: u64, bid: &str| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from("0.65010"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_quote(1_000, "0.65000"), make_quote(2_000, "0.65001")],
        None,
        None,
        UnixNanos::from(2_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&one_tick).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
    assert_eq!(
        cache.borrow().bar(&two_tick).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
}

#[rstest]
fn test_request_scoped_bar_aggregation_deduplicates_bar_types(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-3-TICK-BID-INTERNAL").as_str());

    let params = || -> Params {
        serde_json::from_value(json!({
        "bar_types": [bar_type.to_string(), bar_type.to_string()],
        "update_subscriptions": false,
        }))
        .unwrap()
    };

    let request_id = UUID4::new();

    let request = RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));

    let make_quote = |ts: u64, bid: &str| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from("0.65010"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_quote(1_000, "0.65000"), make_quote(2_000, "0.65001")],
        None,
        None,
        UnixNanos::from(2_000),
        Some(params()),
    )));

    assert_eq!(cache.borrow().bar(&bar_type), None);

    let request_id = UUID4::new();

    let request = RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));

    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![
            make_quote(1_000, "0.65000"),
            make_quote(2_000, "0.65001"),
            make_quote(3_000, "0.65002"),
        ],
        None,
        None,
        UnixNanos::from(3_000),
        Some(params()),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(3_000)),
    );
}

#[rstest]
fn test_request_scoped_bar_aggregation_does_not_publish_to_live_topic(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    let trade = TradeTick::new(
        instrument_id,
        Price::from("0.65000"),
        Quantity::from("1000"),
        AggressorSide::Buy,
        TradeId::new("historical-1"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![trade],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
    assert!(saver.get_messages().is_empty());
}

#[rstest]
fn test_request_scoped_time_bar_aggregation_handles_trade_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-SECOND-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buy,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![
            make_trade(0, "historical-1"),
            make_trade(1_000_000_000, "historical-2"),
        ],
        None,
        None,
        UnixNanos::from(1_000_000_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000_000_000)),
    );
    assert!(saver.get_messages().is_empty());
}

#[rstest]
fn test_request_scoped_composite_bar_aggregator_handles_bar_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite =
        BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL@1-TICK-EXTERNAL").as_str());
    let source = composite.composite();
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [composite.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();

    let request = RequestBars::new(
        source,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Bars(request)));

    let bar = Bar::new(
        source,
        Price::from("0.65000"),
        Price::from("0.65000"),
        Price::from("0.65000"),
        Price::from("0.65000"),
        Quantity::from("1000"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        request_id,
        client_id,
        source,
        vec![bar],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    // Aggregated bars are cached under the standard bar type (v1 parity)
    assert_eq!(
        cache
            .borrow()
            .bar(&composite.standard())
            .map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
}

#[rstest]
fn test_update_subscriptions_request_aggregator_can_be_started_live_after_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buy,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_trade(1_000, "historical-1")],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert!(saver.get_messages().is_empty());

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
        SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    data_engine.process_data(Data::Trade(make_trade(2_000, "live-1")));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].ts_event, UnixNanos::from(2_000));
}

#[rstest]
fn test_update_subscriptions_request_aggregator_can_subscribe_before_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
        SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buy,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_trade(1_000, "historical-1")],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
    assert!(saver.get_messages().is_empty());

    data_engine.process_data(Data::Trade(make_trade(2_000, "live-1")));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].ts_event, UnixNanos::from(2_000));
}

#[rstest]
fn test_request_bar_aggregation_rejects_running_update_subscription_aggregator(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
        SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );

    let result = data_engine.execute_request(RequestCommand::Trades(request));

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already running"));
}

#[rstest]
fn test_request_bar_aggregation_rejects_external_bar_type(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-EXTERNAL").as_str());
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );

    let result = data_engine.execute_request(RequestCommand::Trades(request));

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("must be internally aggregated")
    );
}

#[rstest]
fn test_request_bar_aggregation_cleans_up_after_dispatch_failure(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );

    let result = data_engine.execute_request(RequestCommand::Trades(request.clone()));

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no client found"));

    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    data_engine
        .execute_request(RequestCommand::Trades(request))
        .unwrap();

    let trade = TradeTick::new(
        instrument_id,
        Price::from("0.65000"),
        Quantity::from("1000"),
        AggressorSide::Buy,
        TradeId::new("historical-1"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![trade],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
}

#[rstest]
fn test_request_bar_aggregation_reset_clears_pending_aggregators(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());

    let params = || -> Params {
        serde_json::from_value(json!({
            "bar_types": [bar_type.to_string()],
            "update_subscriptions": false,
        }))
        .unwrap()
    };

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params()),
    );
    data_engine
        .execute_request(RequestCommand::Trades(request))
        .unwrap();

    data_engine.reset();

    let request_id = UUID4::new();

    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params()),
    );
    data_engine
        .execute_request(RequestCommand::Trades(request))
        .unwrap();

    let trade = TradeTick::new(
        instrument_id,
        Price::from("0.65000"),
        Quantity::from("1000"),
        AggressorSide::Buy,
        TradeId::new("historical-1"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![trade],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params()),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
}

#[rstest]
fn test_execute_request_data(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestCustomData {
        client_id,
        data_type: DataType::new("X", None, None),
        start: None,
        end: None,
        limit: None,
        request_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    };

    let cmd = DataCommand::Request(RequestCommand::Data(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_instrument(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestInstrument::new(
        audusd_sim.id,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Request(RequestCommand::Instrument(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_instruments(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Request(RequestCommand::Instruments(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_book_snapshot(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestBookSnapshot::new(
        audusd_sim.id,
        None, // depth
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::BookSnapshot(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_quotes(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestQuotes::new(
        audusd_sim.id,
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::Quotes(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_trades(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestTrades::new(
        audusd_sim.id,
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::Trades(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_funding_rates(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestFundingRates::new(
        audusd_sim.id,
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::FundingRates(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_bars(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestBars::new(
        BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL"),
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::Bars(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_order_book_depth(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestBookDepth::new(
        audusd_sim.id,
        None,                                 // start
        None,                                 // end
        None,                                 // limit
        Some(NonZeroUsize::new(10).unwrap()), // depth
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::BookDepth(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_defi_request_pool_snapshot(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let request = RequestPoolSnapshot::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request));
    data_engine.execute(cmd.clone());

    // Verify command was forwarded to the client
    assert_eq!(recorder.borrow().len(), 1);
    assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&cmd));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_setup_pool_updater_requests_snapshot(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd.clone());

    // Verify two commands were recorded:
    // 1. The SubscribePool command (forwarded to client first)
    // 2. The RequestPoolSnapshot command (automatically sent by setup_pool_updater after)
    let recorded = recorder.borrow();
    assert_eq!(
        recorded.len(),
        2,
        "Expected 2 commands (SubscribePool and RequestPoolSnapshot)"
    );

    // First command should be the SubscribePool (forwarded before snapshot request)
    assert_eq!(recorded[0], cmd);

    // Second command should be RequestPoolSnapshot
    match &recorded[1] {
        DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request)) => {
            assert_eq!(request.instrument_id, instrument_id);
            assert_eq!(request.client_id, Some(client_id));
        }
        _ => panic!(
            "Expected second command to be RequestPoolSnapshot, was: {:?}",
            recorded[1]
        ),
    }
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_snapshot_request_routing_by_client_id(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    // Register two clients
    let client_id_1 = ClientId::from("CLIENT1");
    let venue_1 = Venue::from("VENUE1");
    let recorder_1: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id_1,
        venue_1,
        None,
        &recorder_1,
        &mut data_engine,
    );

    let client_id_2 = ClientId::from("CLIENT2");
    let venue_2 = Venue::from("VENUE2");
    let recorder_2: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id_2,
        venue_2,
        None,
        &recorder_2,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    // Request snapshot with specific client_id
    let request = RequestPoolSnapshot::new(
        instrument_id,
        Some(client_id_1),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request));
    data_engine.execute(cmd.clone());

    // Verify request was routed to CLIENT1 only
    assert_eq!(recorder_1.borrow().len(), 1);
    assert_eq!(recorder_1.borrow()[0], cmd);
    assert_eq!(recorder_2.borrow().len(), 0);
}

#[rstest]
fn test_unsubscribe_book_snapshots_removes_only_requested_interval(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    let fast_interval_ms = NonZeroUsize::new(100).unwrap();
    let slow_interval_ms = NonZeroUsize::new(200).unwrap();
    let fast_topic = switchboard::get_book_snapshots_topic(audusd_sim.id, fast_interval_ms);
    let slow_topic = switchboard::get_book_snapshots_topic(audusd_sim.id, slow_interval_ms);
    let (fast_handler, fast_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    let (slow_handler, slow_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(fast_topic.into(), fast_handler, None);
    msgbus::subscribe_book_snapshots(slow_topic.into(), slow_handler, None);

    execute_book_snapshot_subscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        fast_interval_ms,
    );
    execute_book_snapshot_subscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        slow_interval_ms,
    );
    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 500_000_000);

    let fast_count = fast_saver.get_messages().len();
    let slow_count = slow_saver.get_messages().len();

    execute_book_snapshot_unsubscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        fast_interval_ms,
    );

    assert_eq!(recorder.borrow().len(), 1);

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 500_000_000);

    assert_eq!(fast_saver.get_messages().len(), fast_count);
    assert!(slow_saver.get_messages().len() > slow_count);

    execute_book_snapshot_unsubscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        slow_interval_ms,
    );

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 2);
    assert!(matches!(
        &recorded[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd)) if cmd.instrument_id == audusd_sim.id
    ));
    drop(recorded);

    let slow_count = slow_saver.get_messages().len();
    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 500_000_000);
    assert_eq!(slow_saver.get_messages().len(), slow_count);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_trades_with_bar_types_param_sets_up_aggregation_through_streaming_path(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let _catalog_dir = register_trade_catalog_with_trades(
        &mut data_engine,
        "agg-trades",
        &[split_trade(instrument_id, 2_000, "agg-1")],
        Some((1_000, 2_000)),
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();

    let parent_id = UUID4::new();
    let req = RequestCommand::Trades(RequestTrades::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
        "request-scoped aggregator must consume the catalog-sourced trade",
    );
    assert_eq!(data_engine.request_pipeline_count(), 0);
}
