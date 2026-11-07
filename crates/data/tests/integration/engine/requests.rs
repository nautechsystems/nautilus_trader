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

use nautilus_model::instruments::stubs::equity_aapl;

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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
#[case::live_subscription_retained(false)]
#[case::live_subscription_released(true)]
fn test_request_bar_aggregation_after_unsubscribe(
    #[case] unsubscribe_before_response: bool,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    const SECOND: u64 = 1_000_000_000;

    // Zero timer starts mean now, so use a nonzero five-minute boundary
    let open_ns = 1_700_000_100 * SECOND;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let instrument = equity_aapl();
    let venue = instrument.id().venue;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::Equity(instrument))
        .unwrap();
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let source_type = BarType::from("AAPL.XNAS-1-MINUTE-LAST-EXTERNAL");
    let composite = BarType::from("AAPL.XNAS-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL");
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [composite.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();
    let (response_handler, response_saver) =
        get_typed_message_saving_handler::<DataResponseEnvelope>(None);
    msgbus::register_response_handler(&request_id, response_handler);
    data_engine
        .execute_request(RequestCommand::Bars(RequestBars::new(
            source_type,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::default(),
            Some(params.clone()),
        )))
        .unwrap();

    let (handler, live_saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(composite.standard());
    msgbus::subscribe_bars(topic.into(), handler.clone(), None);
    data_engine
        .execute_subscribe(SubscribeCommand::Bars(SubscribeBars::new(
            composite,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )))
        .unwrap();

    let unsubscribe = UnsubscribeCommand::Bars(UnsubscribeBars::new(
        composite,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));

    if unsubscribe_before_response {
        // Release the exact-topic handler before the command, as DataActorCore does
        msgbus::unsubscribe_bars(topic.into(), &handler);
        data_engine.execute_unsubscribe(&unsubscribe).unwrap();
    }

    let subscriptions_before_response = data_engine.subscribed_bars();
    let source_unsubscribed = recorder.borrow().iter().any(|command| {
        matches!(
            command,
            DataCommand::Unsubscribe(UnsubscribeCommand::Bars(cmd))
                if cmd.single_bar_type().ok() == Some(source_type)
        )
    });
    let source: Vec<_> = (0..5)
        .map(|minute| {
            let close_time = UnixNanos::from(open_ns + (minute + 1) * 60 * SECOND);
            Bar::new(
                source_type,
                Price::from(format!("{}.00", 100 + minute).as_str()),
                Price::from(format!("{}.00", 102 + minute).as_str()),
                Price::from(format!("{}.00", 99 + minute).as_str()),
                Price::from(format!("{}.00", 101 + minute).as_str()),
                Quantity::from("10"),
                close_time,
                close_time,
            )
        })
        .collect();
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        request_id,
        client_id,
        source_type,
        source.clone(),
        None,
        None,
        UnixNanos::from(open_ns + 300 * SECOND),
        Some(params),
    )));

    // Capture the results before normal teardown can affect their observability
    let cached_source = cache.borrow().bars(&source_type).unwrap_or_default();
    let aggregated = cache
        .borrow()
        .bars(&composite.standard())
        .unwrap_or_default();
    let responses = response_saver.get_messages();
    let handler_consumed = stub_msgbus
        .borrow()
        .get_response_handler(&request_id)
        .is_none();
    let subscriptions_after_response = data_engine.subscribed_bars();

    if !unsubscribe_before_response {
        msgbus::unsubscribe_bars(topic.into(), &handler);
        data_engine.execute_unsubscribe(&unsubscribe).unwrap();
    }
    data_engine.stop();
    data_engine.dispose();

    assert_eq!(source_unsubscribed, unsubscribe_before_response);
    assert_eq!(cached_source.len(), source.len());
    assert!(source.iter().all(|bar| cached_source.contains(bar)));
    assert_eq!(responses.len(), 1);
    assert!(responses[0].completed());
    assert!(responses[0].failure_reason().is_none());
    let Some(DataResponse::Bars(response)) = responses[0].response() else {
        panic!("Expected the original historical bars response");
    };
    assert_eq!(response.data.as_ref(), source.as_slice());
    assert!(handler_consumed);
    assert!(live_saver.get_messages().is_empty());

    if unsubscribe_before_response {
        assert!(subscriptions_before_response.is_empty());
        assert!(subscriptions_after_response.is_empty());
    } else {
        assert!(subscriptions_before_response.contains(&composite.standard()));
        assert!(subscriptions_after_response.contains(&composite.standard()));
    }

    assert_eq!(
        aggregated,
        vec![Bar::new(
            composite.standard(),
            Price::from("100.00"),
            Price::from("106.00"),
            Price::from("99.00"),
            Price::from("105.00"),
            Quantity::from("50"),
            UnixNanos::from(1_700_000_400_000_000_000),
            UnixNanos::from(1_700_000_400_000_000_000),
        )],
    );
}

#[rstest]
fn test_request_bar_aggregation_after_resubscribe(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();
    data_engine
        .execute_request(RequestCommand::Trades(RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::default(),
            Some(params.clone()),
        )))
        .unwrap();
    let subscribe = || {
        SubscribeCommand::Bars(SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
    };
    let unsubscribe = || {
        UnsubscribeCommand::Bars(UnsubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
    };
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
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler.clone(), None);
    data_engine.execute_subscribe(subscribe()).unwrap();

    // The released subscription's buffered trade must not reappear after resubscribing
    data_engine.process_data(Data::Trade(make_trade(2_000, "released-live")));
    msgbus::unsubscribe_bars(topic.into(), &handler);
    data_engine.execute_unsubscribe(&unsubscribe()).unwrap();
    let released_subscriptions = data_engine.subscribed_bars();

    msgbus::subscribe_bars(topic.into(), handler.clone(), None);
    data_engine.execute_subscribe(subscribe()).unwrap();
    let resubscribed = data_engine.subscribed_bars();
    data_engine.process_data(Data::Trade(make_trade(2_500, "new-buffered-live")));
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_trade(1_000, "historical")],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));
    let historical_bars = cache.borrow().bars(&bar_type).unwrap_or_default();
    let historical_messages = saver.get_messages();
    let active_after_response = data_engine.subscribed_bars();
    data_engine.process_data(Data::Trade(make_trade(3_000, "resubscribed-live")));
    let live_messages = saver.get_messages();

    msgbus::unsubscribe_bars(topic.into(), &handler);
    data_engine.execute_unsubscribe(&unsubscribe()).unwrap();
    data_engine.stop();
    data_engine.dispose();

    let historical_bar = Bar::new(
        bar_type,
        Price::from("0.65000"),
        Price::from("0.65000"),
        Price::from("0.65000"),
        Price::from("0.65000"),
        Quantity::from("1000"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    assert!(released_subscriptions.is_empty());
    assert!(resubscribed.contains(&bar_type));
    assert!(active_after_response.contains(&bar_type));
    assert_eq!(historical_bars.len(), 2);
    assert!(historical_bars.contains(&historical_bar));
    assert!(historical_bars.contains(&Bar {
        ts_event: UnixNanos::from(2_500),
        ts_init: UnixNanos::from(2_500),
        ..historical_bar
    }));
    assert!(historical_messages.is_empty());
    assert_eq!(
        live_messages,
        vec![Bar {
            ts_event: UnixNanos::from(3_000),
            ts_init: UnixNanos::from(3_000),
            ..historical_bar
        }]
    );
}

#[rstest]
fn test_request_bar_aggregation_failure_after_unsubscribe(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();
    let request = |request_id| {
        RequestCommand::Trades(RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::default(),
            Some(params.clone()),
        ))
    };
    let failed_id = UUID4::new();
    let (response_handler, response_saver) =
        get_typed_message_saving_handler::<DataResponseEnvelope>(None);
    msgbus::register_response_handler(&failed_id, response_handler.clone());
    data_engine.execute_request(request(failed_id)).unwrap();
    let (handler, _) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler.clone(), None);
    data_engine
        .execute_subscribe(SubscribeCommand::Bars(SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )))
        .unwrap();
    msgbus::unsubscribe_bars(topic.into(), &handler);
    data_engine
        .execute_unsubscribe(&UnsubscribeCommand::Bars(UnsubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )))
        .unwrap();
    let subscriptions_before_failure = data_engine.subscribed_bars();
    data_engine.request_failed(DataRequestFailed {
        request_id: failed_id,
        reason: "Historical data unavailable".to_owned(),
        response: None,
    });
    let failed_responses = response_saver.get_messages();
    let subscriptions_after_failure = data_engine.subscribed_bars();
    let bars_after_failure = cache.borrow().bar_count(&bar_type);

    // A new request for the same bar type must still complete after the failure
    let next_id = UUID4::new();
    msgbus::register_response_handler(&next_id, response_handler);
    data_engine.execute_request(request(next_id)).unwrap();
    let trade = TradeTick::new(
        instrument_id,
        Price::from("0.65000"),
        Quantity::from("1000"),
        AggressorSide::Buy,
        TradeId::new("next-history"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        next_id,
        client_id,
        instrument_id,
        vec![trade],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));
    let responses = response_saver.get_messages();
    let bars = cache.borrow().bars(&bar_type).unwrap_or_default();
    data_engine.stop();
    data_engine.dispose();

    assert!(subscriptions_before_failure.is_empty());
    assert!(subscriptions_after_failure.is_empty());
    assert_eq!(bars_after_failure, 0);
    assert_eq!(failed_responses.len(), 1);
    assert_eq!(failed_responses[0].correlation_id(), &failed_id);
    assert!(failed_responses[0].completed());
    assert_eq!(
        failed_responses[0].failure_reason(),
        Some("Historical data unavailable")
    );
    assert!(failed_responses[0].response().is_none());
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[1].correlation_id(), &next_id);
    assert!(responses[1].completed());
    assert!(responses[1].failure_reason().is_none());
    assert_eq!(
        bars,
        vec![Bar::new(
            bar_type,
            Price::from("0.65000"),
            Price::from("0.65000"),
            Price::from("0.65000"),
            Price::from("0.65000"),
            Quantity::from("1000"),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        )]
    );
}

#[rstest]
fn test_request_bar_aggregation_chain_after_unsubscribe(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    const SECOND: u64 = 1_000_000_000;
    let _ = stub_msgbus;

    // Both historical intervals start at the same nonzero ten-minute boundary
    let open_ns = 1_700_000_400 * SECOND;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let instrument = equity_aapl();
    let venue = instrument.id().venue;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::Equity(instrument))
        .unwrap();
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let source_type = BarType::from("AAPL.XNAS-1-MINUTE-LAST-EXTERNAL");
    let five_minute = BarType::from("AAPL.XNAS-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL");
    let ten_minute = BarType::from("AAPL.XNAS-10-MINUTE-LAST-INTERNAL@5-MINUTE-INTERNAL");
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [five_minute.to_string(), ten_minute.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();
    let (response_handler, response_saver) =
        get_typed_message_saving_handler::<DataResponseEnvelope>(None);
    msgbus::register_response_handler(&request_id, response_handler);
    data_engine
        .execute_request(RequestCommand::Bars(RequestBars::new(
            source_type,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::default(),
            Some(params.clone()),
        )))
        .unwrap();
    let (handler, live_saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(ten_minute.standard());
    msgbus::subscribe_bars(topic.into(), handler.clone(), None);

    for bar_type in [five_minute, ten_minute] {
        data_engine
            .execute_subscribe(SubscribeCommand::Bars(SubscribeBars::new(
                bar_type,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )))
            .unwrap();
    }

    // Releasing the only public consumer also releases its internal live source
    msgbus::unsubscribe_bars(topic.into(), &handler);
    data_engine
        .execute_unsubscribe(&UnsubscribeCommand::Bars(UnsubscribeBars::new(
            ten_minute,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )))
        .unwrap();
    let subscriptions_before_response = data_engine.subscribed_bars();
    let source_unsubscribed = recorder.borrow().iter().any(|command| {
        matches!(
            command,
            DataCommand::Unsubscribe(UnsubscribeCommand::Bars(cmd))
                if cmd.single_bar_type().ok() == Some(source_type)
        )
    });
    let source: Vec<_> = (0..10)
        .map(|minute| {
            let close_time = UnixNanos::from(open_ns + (minute + 1) * 60 * SECOND);
            Bar::new(
                source_type,
                Price::from(format!("{}.00", 100 + minute).as_str()),
                Price::from(format!("{}.00", 102 + minute).as_str()),
                Price::from(format!("{}.00", 99 + minute).as_str()),
                Price::from(format!("{}.00", 101 + minute).as_str()),
                Quantity::from("10"),
                close_time,
                close_time,
            )
        })
        .collect();
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        request_id,
        client_id,
        source_type,
        source.clone(),
        None,
        None,
        UnixNanos::from(open_ns + 600 * SECOND),
        Some(params),
    )));
    let cached_source = cache.borrow().bars(&source_type).unwrap_or_default();
    let five_minute_bars = cache
        .borrow()
        .bars(&five_minute.standard())
        .unwrap_or_default();
    let ten_minute_bars = cache
        .borrow()
        .bars(&ten_minute.standard())
        .unwrap_or_default();
    let subscriptions_after_response = data_engine.subscribed_bars();
    let responses = response_saver.get_messages();
    data_engine.stop();
    data_engine.dispose();

    let expected_bar = |bar_type, prices: [&str; 4], volume: &str, timestamp: u64| {
        Bar::new(
            bar_type,
            Price::from(prices[0]),
            Price::from(prices[1]),
            Price::from(prices[2]),
            Price::from(prices[3]),
            Quantity::from(volume),
            UnixNanos::from(timestamp),
            UnixNanos::from(timestamp),
        )
    };
    assert!(subscriptions_before_response.is_empty());
    assert!(subscriptions_after_response.is_empty());
    assert!(source_unsubscribed);
    assert_eq!(cached_source.len(), 10);
    assert!(source.iter().all(|bar| cached_source.contains(bar)));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].correlation_id(), &request_id);
    assert!(responses[0].completed());
    assert!(responses[0].failure_reason().is_none());
    let Some(DataResponse::Bars(response)) = responses[0].response() else {
        panic!("Expected the original historical bars response");
    };
    assert_eq!(response.data.as_ref(), source.as_slice());
    assert!(live_saver.get_messages().is_empty());
    assert_eq!(five_minute_bars.len(), 2);
    assert!(five_minute_bars.contains(&expected_bar(
        five_minute.standard(),
        ["100.00", "106.00", "99.00", "105.00"],
        "50",
        1_700_000_700_000_000_000,
    )));
    assert!(five_minute_bars.contains(&expected_bar(
        five_minute.standard(),
        ["105.00", "111.00", "104.00", "110.00"],
        "50",
        1_700_001_000_000_000_000,
    )));
    assert_eq!(
        ten_minute_bars,
        vec![expected_bar(
            ten_minute.standard(),
            ["100.00", "111.00", "99.00", "110.00"],
            "100",
            1_700_001_000_000_000_000,
        )]
    );
}

#[rstest]
fn test_update_subscriptions_request_aggregator_replays_live_quotes_after_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    const SEC: u64 = 1_000_000_000;
    let t0 = 1_700_000_040 * SEC;

    let _ = stub_msgbus;
    let virtual_clock = Rc::new(RefCell::new(VirtualClock::new()));
    virtual_clock
        .borrow_mut()
        .set_time(UnixNanos::from(t0 + 30 * SEC));
    let clock: Rc<RefCell<dyn Clock>> = virtual_clock;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-MINUTE-MID-INTERNAL").as_str());
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();

    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(
        RequestQuotes::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::from(t0 + 30 * SEC),
            Some(params.clone()),
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
        SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(t0 + 30 * SEC),
            None,
            None,
        ),
    )));

    let make_quote = |ts: u64, bid: &str, ask: &str| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    // A stale quote and a quote after the history end arrive while the response is in flight
    data_engine.process_data(Data::Quote(make_quote(t0 - 2 * SEC, "0.64000", "0.64010")));
    data_engine.process_data(Data::Quote(make_quote(t0 + 29 * SEC, "0.66000", "0.66010")));
    assert!(cache.borrow().bar(&bar_type).is_none());

    let history: Vec<QuoteTick> = (1..=30)
        .map(|i| make_quote(t0 - 300 * SEC + i * 10 * SEC - 1, "0.65000", "0.65010"))
        .collect();
    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        history,
        None,
        None,
        UnixNanos::from(t0 + 30 * SEC),
        Some(params),
    )));

    // The held quote closes the last historical bar and the stale quote is not applied
    let cache_ref = cache.borrow();
    let bars = cache_ref.bars(&bar_type).unwrap();
    assert_eq!(bars.len(), 5);
    assert_eq!(bars[0].ts_event, UnixNanos::from(t0));
    assert_eq!(bars[0].close, Price::from("0.65005"));
    assert_eq!(bars[0].low, Price::from("0.65005"));
}

#[rstest]
fn test_update_subscriptions_request_aggregator_replays_live_trades_after_response(
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-2-TICK-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();

    data_engine.execute(DataCommand::Request(RequestCommand::Trades(
        RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::default(),
            Some(params.clone()),
        ),
    )));
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

    let make_trade = |ts: u64, price: &str, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from(price),
            Quantity::from("1000"),
            AggressorSide::Buy,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    // A live trade arrives while the response is in flight and must not displace the history
    data_engine.process_data(Data::Trade(make_trade(5_000, "0.67000", "live-1")));
    assert!(cache.borrow().bar(&bar_type).is_none());
    assert!(saver.get_messages().is_empty());

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![
            make_trade(1_000, "0.65000", "historical-1"),
            make_trade(2_000, "0.65010", "historical-2"),
            make_trade(3_000, "0.65020", "historical-3"),
            make_trade(4_000, "0.65030", "historical-4"),
        ],
        None,
        None,
        UnixNanos::from(5_000),
        Some(params),
    )));

    {
        let cache_ref = cache.borrow();
        let bars = cache_ref.bars(&bar_type).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, Price::from("0.65030"));
        assert_eq!(bars[1].close, Price::from("0.65010"));
    }
    assert!(saver.get_messages().is_empty());

    // The held live trade opens the next bar, which completes with the next live trade
    data_engine.process_data(Data::Trade(make_trade(6_000, "0.67010", "live-2")));

    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].open, Price::from("0.67000"));
    assert_eq!(messages[0].close, Price::from("0.67010"));
    assert_eq!(messages[0].ts_event, UnixNanos::from(6_000));
}

#[rstest]
#[case::composite_first(true)]
#[case::source_first(false)]
fn test_update_subscriptions_request_chain_replays_live_quotes_before_dependents_go_live(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
    #[case] composite_first: bool,
) {
    const SEC: u64 = 1_000_000_000;
    // Aligned to a 5-minute boundary
    let boundary = 1_700_000_100 * SEC;

    let _ = stub_msgbus;
    let virtual_clock = Rc::new(RefCell::new(VirtualClock::new()));
    virtual_clock
        .borrow_mut()
        .set_time(UnixNanos::from(boundary + 10 * SEC));
    let clock: Rc<RefCell<dyn Clock>> = virtual_clock;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let source = BarType::from(format!("{instrument_id}-1-MINUTE-MID-INTERNAL").as_str());
    let composite =
        BarType::from(format!("{instrument_id}-5-MINUTE-MID-INTERNAL@1-MINUTE-INTERNAL").as_str());
    let bar_types = if composite_first {
        vec![composite, source]
    } else {
        vec![source, composite]
    };

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": bar_types.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "update_subscriptions": true,
    }))
    .unwrap();

    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(
        RequestQuotes::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::from(boundary + 10 * SEC),
            Some(params.clone()),
        ),
    )));

    for bar_type in bar_types {
        data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
            SubscribeBars::new(
                bar_type,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(boundary + 10 * SEC),
                None,
                None,
            ),
        )));
    }

    let make_quote = |ts: u64| {
        QuoteTick::new(
            instrument_id,
            Price::from("0.65000"),
            Price::from("0.65010"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    // Two live quotes straddling the 5-minute boundary arrive while the response is in flight
    data_engine.process_data(Data::Quote(make_quote(boundary - 30 * SEC)));
    data_engine.process_data(Data::Quote(make_quote(boundary + 5 * SEC)));
    assert!(cache.borrow().bar(&composite.standard()).is_none());

    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        Vec::new(),
        None,
        None,
        UnixNanos::from(boundary + 10 * SEC),
        Some(params),
    )));

    // The held quotes replay through the source before the composite goes live
    let cache_ref = cache.borrow();
    let source_bar = cache_ref.bar(&source.standard()).unwrap();
    assert_eq!(source_bar.ts_event, UnixNanos::from(boundary));
    let composite_bar = cache_ref.bar(&composite.standard()).unwrap();
    assert_eq!(composite_bar.ts_event, UnixNanos::from(boundary));
    assert_eq!(composite_bar.close, Price::from("0.65005"));
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
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
        Rc::clone(&cache),
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
        Rc::clone(&clock),
        Rc::clone(&cache),
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
    let data_engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd)) if cmd.single_instrument_id().unwrap() == audusd_sim.id
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
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
