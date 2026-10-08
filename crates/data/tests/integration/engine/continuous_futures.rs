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
fn test_continuous_future_request_adjusts_external_bars_across_transitions(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let minute = |value: u64| value * 60_000_000_000;
    let clock = data_engine_clock_at(minute(3));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");
    let esu = add_es_contract(&cache, "ESU24.GLBX", "ESU24");

    let venue = Venue::from("GLBX");
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

    let target_bar_type = BarType::from("ES.GLBX-1-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL");
    let parent_id = UUID4::new();
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": minute(2),
                "pre_instrument_id": esh.to_string(),
                "post_instrument_id": esm.to_string(),
                "pre_price": "100.00",
                "post_price": "95.00"
            },
            {
                "transition_time_ns": minute(3),
                "pre_instrument_id": esm.to_string(),
                "post_instrument_id": esu.to_string(),
                "pre_price": "110.00",
                "post_price": "105.00"
            }
        ]
    }));
    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-external-bars")));
    msgbus::register_response_handler(&parent_id, response_handler);

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(minute(1)).to_datetime_utc()),
        Some(UnixNanos::from(minute(3)).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let child = recorded_bars_request(&recorder, 0);
    let parent_id_str = parent_id.to_string();
    assert_eq!(
        child.bar_type,
        BarType::from("ESH24.GLBX-1-MINUTE-LAST-EXTERNAL")
    );
    assert_eq!(
        child
            .params
            .as_ref()
            .and_then(|params| params.get_str("continuous_future_parent_request_id")),
        Some(parent_id_str.as_str()),
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![make_bar(
            child.bar_type,
            "100.00",
            "101.00",
            "99.00",
            "100.50",
            1,
            minute(1),
        )],
        None,
        None,
        UnixNanos::from(minute(1)),
        child.params,
    )));
    assert_eq!(
        cache
            .borrow()
            .bar(&target_bar_type.standard())
            .map(|bar| bar.open),
        Some(Price::from("90.00"))
    );

    let child = recorded_bars_request(&recorder, 1);
    assert_eq!(
        child.bar_type,
        BarType::from("ESM24.GLBX-1-MINUTE-LAST-EXTERNAL")
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![make_bar(
            child.bar_type,
            "96.00",
            "97.00",
            "95.50",
            "96.50",
            2,
            minute(2),
        )],
        None,
        None,
        UnixNanos::from(minute(2)),
        child.params,
    )));
    assert_eq!(
        cache
            .borrow()
            .bar(&target_bar_type.standard())
            .map(|bar| bar.open),
        Some(Price::from("91.00"))
    );

    let child = recorded_bars_request(&recorder, 2);
    assert_eq!(
        child.bar_type,
        BarType::from("ESU24.GLBX-1-MINUTE-LAST-EXTERNAL")
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![make_bar(
            child.bar_type,
            "106.00",
            "107.00",
            "105.50",
            "106.50",
            3,
            minute(3),
        )],
        None,
        None,
        UnixNanos::from(minute(3)),
        child.params,
    )));

    let cached_bar = cache
        .borrow()
        .bar(&target_bar_type.standard())
        .copied()
        .unwrap();
    assert_eq!(cached_bar.open, Price::from("106.00"));
    assert_eq!(cached_bar.high, Price::from("107.00"));
    assert_eq!(cached_bar.low, Price::from("105.50"));
    assert_eq!(cached_bar.close, Price::from("106.50"));
    assert_eq!(cached_bar.volume, Quantity::from(3));
    let responses = response_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(response_data_count(&responses[0]), Some(3));
}

#[rstest]
fn test_continuous_future_request_ignores_time_range_generator_for_segments(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    // Each segment child requests its whole window, so the parent never advances on a partial
    // time-range window.
    let _ = stub_msgbus;
    let minute = |value: u64| value * 60_000_000_000;
    let clock = data_engine_clock_at(minute(3));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
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

    let params = params_from_json(json!({
        "time_range_generator": "",
        "durations_seconds": [10],
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": minute(2),
                "pre_instrument_id": esh.to_string(),
                "post_instrument_id": esm.to_string(),
                "pre_price": "100.00",
                "post_price": "95.00"
            }
        ]
    }));

    let request = RequestBars::new(
        BarType::from("ES.GLBX-1-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL"),
        Some(UnixNanos::from(minute(1)).to_datetime_utc()),
        Some(UnixNanos::from(minute(3)).to_datetime_utc()),
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let child = recorded_bars_request(&recorder, 0);
    assert_eq!(
        child.start.map(|dt| dt.as_nanosecond()),
        Some(i128::from(minute(1)))
    );
    assert_eq!(
        child.end.map(|dt| dt.as_nanosecond()),
        Some(i128::from(minute(2) - 1))
    );
    assert!(
        child
            .params
            .as_ref()
            .is_some_and(|params| !params.contains_key("time_range_generator"))
    );
    assert_eq!(data_engine.time_range_pipeline_count(), 0);
}

#[rstest]
fn test_continuous_future_request_inserts_history_behind_newer_live_bar(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let minute = |value: u64| value * 60_000_000_000;
    let clock = data_engine_clock_at(minute(3));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
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

    let target_bar_type = BarType::from("ES.GLBX-1-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL");

    // a live bar is cached under the standard bar type before the requested history
    let standard = target_bar_type.standard();
    data_engine.process_data(Data::Bar(make_bar(
        standard,
        "108.00",
        "108.00",
        "108.00",
        "108.00",
        1,
        minute(3),
    )));
    assert_eq!(
        cache.borrow().bar(&standard).map(|bar| bar.ts_event),
        Some(UnixNanos::from(minute(3))),
    );

    let parent_id = UUID4::new();
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": minute(2),
                "pre_instrument_id": esh.to_string(),
                "post_instrument_id": esm.to_string(),
                "pre_price": "100.00",
                "post_price": "95.00"
            }
        ]
    }));

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(minute(0)).to_datetime_utc()),
        Some(UnixNanos::from(minute(2)).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let child = recorded_bars_request(&recorder, 0);
    assert_eq!(
        child.bar_type,
        BarType::from("ESH24.GLBX-1-MINUTE-LAST-EXTERNAL")
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![
            make_bar(
                child.bar_type,
                "100.00",
                "100.00",
                "100.00",
                "100.00",
                1,
                minute(0),
            ),
            make_bar(
                child.bar_type,
                "101.00",
                "101.00",
                "101.00",
                "101.00",
                1,
                minute(1),
            ),
        ],
        None,
        None,
        UnixNanos::from(minute(1)),
        child.params,
    )));

    // the requested history is inserted behind the newer live bar
    let stamps: Vec<_> = cache
        .borrow()
        .bars(&standard)
        .map(|bars| bars.iter().map(|bar| bar.ts_event).collect())
        .unwrap_or_default();
    assert_eq!(
        stamps,
        vec![
            UnixNanos::from(minute(3)),
            UnixNanos::from(minute(1)),
            UnixNanos::from(minute(0)),
        ],
    );
}

#[rstest]
fn test_continuous_future_request_applies_ratio_to_external_bars(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let minute = |value: u64| value * 60_000_000_000;
    let clock = data_engine_clock_at(minute(2));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
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

    let target_bar_type = BarType::from("ES.GLBX-1-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL");
    let parent_id = UUID4::new();
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_RATIO",
        "continuous_future_transitions": [
            {
                "transition_time_ns": minute(2),
                "pre_instrument_id": esh.to_string(),
                "post_instrument_id": esm.to_string(),
                "pre_price": "100.00",
                "post_price": "50.00"
            }
        ]
    }));
    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-ratio-bars")));
    msgbus::register_response_handler(&parent_id, response_handler);

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(minute(1)).to_datetime_utc()),
        Some(UnixNanos::from(minute(2)).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let child = recorded_bars_request(&recorder, 0);
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![make_bar(
            child.bar_type,
            "100.00",
            "101.00",
            "99.00",
            "100.50",
            1,
            minute(1),
        )],
        None,
        None,
        UnixNanos::from(minute(1)),
        child.params,
    )));
    assert_eq!(
        cache
            .borrow()
            .bar(&target_bar_type.standard())
            .map(|bar| bar.open),
        Some(Price::from("50.00"))
    );

    let child = recorded_bars_request(&recorder, 1);
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![make_bar(
            child.bar_type,
            "55.00",
            "56.00",
            "54.50",
            "55.50",
            2,
            minute(2),
        )],
        None,
        None,
        UnixNanos::from(minute(2)),
        child.params,
    )));

    let cached_bar = cache
        .borrow()
        .bar(&target_bar_type.standard())
        .copied()
        .unwrap();
    assert_eq!(cached_bar.open, Price::from("55.00"));
    assert_eq!(cached_bar.close, Price::from("55.50"));
    let responses = response_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(response_data_count(&responses[0]), Some(2));
}

#[rstest]
fn test_continuous_future_request_preserves_bar_type_chain(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock = data_engine_clock_at(80);
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
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

    let bar_type_1 = BarType::from("ES.GLBX-2-TICK-LAST-INTERNAL@1-TICK-EXTERNAL");
    let bar_type_2 = BarType::from("ES.GLBX-4-TICK-LAST-INTERNAL@2-TICK-INTERNAL");
    let parent_id = UUID4::new();
    let params = params_from_json(json!({
        "bar_types": [bar_type_1.to_string(), bar_type_2.to_string()],
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": 50,
                "pre_instrument_id": esh.to_string(),
                "post_instrument_id": esm.to_string(),
                "pre_price": "103.00",
                "post_price": "95.00"
            }
        ]
    }));
    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-chain-bars")));
    msgbus::register_response_handler(&parent_id, response_handler);

    let request = RequestBars::new(
        bar_type_2,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(80).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let child = recorded_bars_request(&recorder, 0);
    assert_eq!(
        child.bar_type,
        BarType::from("ESH24.GLBX-1-TICK-LAST-EXTERNAL")
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![
            make_bar(child.bar_type, "100.00", "100.00", "100.00", "100.00", 1, 1),
            make_bar(child.bar_type, "101.00", "101.00", "101.00", "101.00", 1, 2),
            make_bar(child.bar_type, "102.00", "102.00", "102.00", "102.00", 1, 3),
            make_bar(child.bar_type, "103.00", "103.00", "103.00", "103.00", 1, 4),
        ],
        None,
        None,
        UnixNanos::from(4),
        child.params,
    )));

    let child = recorded_bars_request(&recorder, 1);
    assert_eq!(
        child.bar_type,
        BarType::from("ESM24.GLBX-1-TICK-LAST-EXTERNAL")
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        child.request_id,
        client_id,
        child.bar_type,
        vec![
            make_bar(child.bar_type, "95.00", "95.00", "95.00", "95.00", 1, 51),
            make_bar(child.bar_type, "96.00", "96.00", "96.00", "96.00", 1, 52),
            make_bar(child.bar_type, "97.00", "97.00", "97.00", "97.00", 1, 53),
            make_bar(child.bar_type, "98.00", "98.00", "98.00", "98.00", 1, 54),
        ],
        None,
        None,
        UnixNanos::from(54),
        child.params,
    )));

    // Aggregated bars are cached under the standard bar type (v1 parity)
    let first_level = cache.borrow().bar(&bar_type_1.standard()).copied().unwrap();
    assert_eq!(first_level.open, Price::from("97.00"));
    assert_eq!(first_level.close, Price::from("98.00"));
    assert_eq!(first_level.volume, Quantity::from(2));
    let second_level = cache.borrow().bar(&bar_type_2.standard()).copied().unwrap();
    assert_eq!(second_level.open, Price::from("92.00"));
    assert_eq!(second_level.high, Price::from("98.00"));
    assert_eq!(second_level.low, Price::from("92.00"));
    assert_eq!(second_level.close, Price::from("98.00"));
    assert_eq!(second_level.volume, Quantity::from(8));
    let responses = response_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(response_data_count(&responses[0]), Some(8));
}

#[rstest]
fn test_continuous_future_request_uses_quote_tick_source(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock = data_engine_clock_at(20);
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
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

    let target_bar_type = BarType::from("ES.GLBX-2-TICK-BID-INTERNAL");
    let parent_id = UUID4::new();
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": 10,
                "pre_instrument_id": esh.to_string(),
                "post_instrument_id": esm.to_string(),
                "pre_price": "100.00",
                "post_price": "110.00"
            }
        ]
    }));
    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-quote-bars")));
    msgbus::register_response_handler(&parent_id, response_handler);

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(20).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let child = recorded_quotes_request(&recorder, 0);
    let parent_id_str = parent_id.to_string();
    assert_eq!(child.instrument_id, esh);
    assert_eq!(
        child
            .params
            .as_ref()
            .and_then(|params| params.get_str("continuous_future_parent_request_id")),
        Some(parent_id_str.as_str()),
    );
    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        child.request_id,
        client_id,
        child.instrument_id,
        vec![make_quote(child.instrument_id, "100.00", "100.25", 1)],
        None,
        None,
        UnixNanos::from(1),
        child.params,
    )));

    let child = recorded_quotes_request(&recorder, 1);
    assert_eq!(child.instrument_id, esm);
    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        child.request_id,
        client_id,
        child.instrument_id,
        vec![make_quote(child.instrument_id, "111.00", "111.25", 11)],
        None,
        None,
        UnixNanos::from(11),
        child.params,
    )));

    let cached_bar = cache.borrow().bar(&target_bar_type).copied().unwrap();
    assert_eq!(cached_bar.open, Price::from("110.00"));
    assert_eq!(cached_bar.close, Price::from("111.00"));
    assert_eq!(cached_bar.volume, Quantity::from(2));
    let responses = response_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(response_data_count(&responses[0]), Some(2));
}

#[rstest]
fn test_continuous_future_request_start_after_end_emits_empty_parent_response(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock = data_engine_clock_at(20);
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_instrument_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_instrument_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
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

    let parent_id = UUID4::new();
    let target_bar_type = BarType::from("ES.GLBX-2-TICK-LAST-INTERNAL");
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": 10,
                "pre_instrument_id": pre_instrument_id.to_string(),
                "post_instrument_id": post_instrument_id.to_string(),
                "pre_price": "100.00",
                "post_price": "110.00"
            }
        ]
    }));
    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-empty-bounds")));
    msgbus::register_response_handler(&parent_id, response_handler);

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(20).to_datetime_utc()),
        Some(UnixNanos::from(10).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let responses = response_saver.get_messages();
    assert!(recorder.borrow().is_empty());
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].correlation_id, parent_id);
    assert!(responses[0].data.is_empty());
    assert_eq!(responses[0].start, Some(UnixNanos::from(20)));
    assert_eq!(responses[0].end, Some(UnixNanos::from(10)));
    assert_eq!(response_data_count(&responses[0]), None);
}

#[rstest]
fn test_continuous_future_request_walks_segments_and_applies_adjustments(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    clock
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<VirtualClock>()
        .unwrap()
        .advance_time(UnixNanos::from(20), true);
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let pre_instrument = make_es_future("ESH24.GLBX", "ESH24");
    let post_instrument = make_es_future("ESM24.GLBX", "ESM24");
    let pre_instrument_id = pre_instrument.id;
    let post_instrument_id = post_instrument.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(pre_instrument))
        .unwrap();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(post_instrument))
        .unwrap();

    let venue = Venue::from("GLBX");
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

    let parent_id = UUID4::new();
    let target_bar_type = BarType::from("ES.GLBX-2-TICK-LAST-INTERNAL");

    let params = || -> Params {
        serde_json::from_value(json!({
            "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
            "continuous_future_transitions": [
                {
                    "transition_time_ns": 10,
                    "pre_instrument_id": pre_instrument_id.to_string(),
                    "post_instrument_id": post_instrument_id.to_string(),
                    "pre_price": "100.00",
                    "post_price": "110.00"
                }
            ]
        }))
        .unwrap()
    };

    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-future-response")));
    msgbus::register_response_handler(&parent_id, response_handler);

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(20).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params()),
    );

    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let first_child = recorded_trades_request(&recorder, 0);
    assert_eq!(first_child.instrument_id, pre_instrument_id);
    assert_eq!(first_child.start.map(|dt| dt.as_nanosecond()), Some(0));
    assert_eq!(first_child.end.map(|dt| dt.as_nanosecond()), Some(9));
    let first_child_params_ref = first_child.params.as_ref().unwrap();
    let parent_id_str = parent_id.to_string();
    assert_eq!(
        first_child_params_ref.get_str("continuous_future_parent_request_id"),
        Some(parent_id_str.as_str()),
    );
    assert!(!first_child_params_ref.contains_key("continuous_future_transitions"));
    assert!(!first_child_params_ref.contains_key("bar_types"));
    let mut first_response_params = first_child.params.clone().unwrap();
    first_response_params.insert("data_count".to_string(), json!(7));

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        first_child.request_id,
        client_id,
        pre_instrument_id,
        vec![make_trade(pre_instrument_id, "100.00", 1, "pre-1", 1)],
        Some(UnixNanos::from(0)),
        Some(UnixNanos::from(9)),
        UnixNanos::from(1),
        Some(first_response_params),
    )));

    assert!(response_saver.get_messages().is_empty());
    assert_eq!(recorder.borrow().len(), 2);

    let second_child = recorded_trades_request(&recorder, 1);
    assert_eq!(second_child.instrument_id, post_instrument_id);
    assert_eq!(second_child.start.map(|dt| dt.as_nanosecond()), Some(10));
    assert_eq!(second_child.end.map(|dt| dt.as_nanosecond()), Some(20));
    let mut second_response_params = second_child.params.clone().unwrap();
    second_response_params.insert("data_count".to_string(), json!(8));
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        second_child.request_id,
        client_id,
        post_instrument_id,
        vec![make_trade(post_instrument_id, "111.00", 1, "post-1", 11)],
        Some(UnixNanos::from(10)),
        Some(UnixNanos::from(20)),
        UnixNanos::from(11),
        Some(second_response_params),
    )));

    let cached_bar = cache.borrow().bar(&target_bar_type).copied().unwrap();
    assert_eq!(cached_bar.open, Price::from("110.00"));
    assert_eq!(cached_bar.close, Price::from("111.00"));
    assert_eq!(cached_bar.volume, Quantity::from(2));
    let target_instrument = cache
        .borrow()
        .instrument(&target_bar_type.instrument_id())
        .cloned()
        .unwrap();

    let InstrumentAny::FuturesContract(target_instrument) = target_instrument else {
        panic!("Expected synthesized futures contract");
    };

    assert_eq!(target_instrument.id, target_bar_type.instrument_id());
    assert_eq!(
        target_instrument.raw_symbol,
        target_bar_type.instrument_id().symbol
    );
    assert_eq!(target_instrument.activation_ns, UnixNanos::default());
    assert_eq!(target_instrument.expiration_ns, UnixNanos::default());

    let responses = response_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].correlation_id, parent_id);
    assert!(responses[0].data.is_empty());
    assert_eq!(response_data_count(&responses[0]), Some(15));

    let second_parent_id = UUID4::new();

    let second_request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(20).to_datetime_utc()),
        None,
        Some(client_id),
        second_parent_id,
        UnixNanos::default(),
        Some(params()),
    );

    data_engine
        .execute_request(RequestCommand::Bars(second_request))
        .unwrap();

    assert_eq!(recorder.borrow().len(), 3);

    match recorder.borrow()[2].clone() {
        DataCommand::Request(RequestCommand::Trades(request)) => {
            assert_eq!(request.instrument_id, pre_instrument_id);
        }
        other => panic!("Expected repeated continuous future child request, was {other:?}"),
    }
}

#[rstest]
fn test_continuous_future_request_cleans_up_after_first_dispatch_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock = data_engine_clock_at(20);
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_instrument_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_instrument_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let failing_client =
        FailingRequestDataClient::new(client_id, Some(venue), "request dispatch failed");
    let adapter =
        DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(failing_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let target_bar_type = BarType::from("ES.GLBX-2-TICK-LAST-INTERNAL");

    let params = || {
        params_from_json(json!({
            "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
            "continuous_future_transitions": [
                {
                    "transition_time_ns": 10,
                    "pre_instrument_id": pre_instrument_id.to_string(),
                    "post_instrument_id": post_instrument_id.to_string(),
                    "pre_price": "100.00",
                    "post_price": "110.00"
                }
            ]
        }))
    };

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(20).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params()),
    );
    let result = data_engine.execute_request(RequestCommand::Bars(request));

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("request dispatch failed")
    );

    data_engine.deregister_client(&client_id);
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

    let retry = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(20).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params()),
    );
    data_engine
        .execute_request(RequestCommand::Bars(retry))
        .unwrap();

    let child = recorded_trades_request(&recorder, 0);
    assert_eq!(recorder.borrow().len(), 1);
    assert_eq!(child.instrument_id, pre_instrument_id);
}

#[rstest]
fn test_continuous_future_request_emits_parent_response_on_later_dispatch_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock = data_engine_clock_at(20);
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_instrument_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_instrument_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
    let mut data_engine = DataEngine::new(clock, cache, None);
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::new(RefCell::new(Cache::default())),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let parent_id = UUID4::new();
    let target_bar_type = BarType::from("ES.GLBX-2-TICK-LAST-INTERNAL");
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": 10,
                "pre_instrument_id": pre_instrument_id.to_string(),
                "post_instrument_id": post_instrument_id.to_string(),
                "pre_price": "100.00",
                "post_price": "110.00"
            }
        ]
    }));
    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-dispatch-error")));
    msgbus::register_response_handler(&parent_id, response_handler);

    let request = RequestBars::new(
        target_bar_type,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(20).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    );
    data_engine
        .execute_request(RequestCommand::Bars(request))
        .unwrap();

    let child = recorded_trades_request(&recorder, 0);
    data_engine.deregister_client(&client_id);
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        child.request_id,
        client_id,
        child.instrument_id,
        vec![make_trade(child.instrument_id, "100.00", 1, "pre-1", 1)],
        Some(UnixNanos::from(0)),
        Some(UnixNanos::from(9)),
        UnixNanos::from(1),
        child.params,
    )));

    let responses = response_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].correlation_id, parent_id);
    assert!(responses[0].data.is_empty());
    assert_eq!(response_data_count(&responses[0]), Some(1));
    assert_eq!(recorder.borrow().len(), 1);
}

#[rstest]
fn test_continuous_future_params_require_request_bars(
    audusd_sim: CurrencyPair,
    client_id: ClientId,
) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let params: Params = serde_json::from_value(json!({
        "continuous_future_transitions": []
    }))
    .unwrap();

    let request = RequestTrades::new(
        audusd_sim.id,
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
    assert!(result.unwrap_err().to_string().contains("RequestBars"));
}

#[rstest]
fn test_subscribe_continuous_future_bars_dispatches_child_trade_subscription(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, test_clock, recorder) =
        register_continuous_future_subscription_engine(cache.clone(), 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");
    let parent_id = UUID4::new();
    let params = continuous_future_transitions_params(10, pre_id, post_id);

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("XNAS")),
        parent_id,
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    assert_eq!(recorder.borrow().len(), 1);

    let DataCommand::Subscribe(SubscribeCommand::Trades(child)) = recorder.borrow()[0].clone()
    else {
        panic!(
            "expected child SubscribeTrades, was {:?}",
            recorder.borrow()[0]
        );
    };

    assert_eq!(child.instrument_id, pre_id);
    assert_eq!(child.venue, Some(Venue::from("GLBX")));
    assert_eq!(child.correlation_id, Some(parent_id));
    let child_params = child.params.as_ref().unwrap();
    assert!(!child_params.contains_key("continuous_future_transitions"));
    assert!(!child_params.contains_key("continuous_future_adjustment_mode"));
    assert!(!child_params.contains_key("bar_types"));

    // Continuous instrument was synthesized into the cache
    assert!(
        cache
            .borrow()
            .instrument(&target_bar_type.instrument_id())
            .is_some()
    );

    // Timer scheduled for the upcoming transition
    let timer_names: Vec<String> = test_clock
        .borrow()
        .timer_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert!(
        timer_names
            .iter()
            .any(|name| name.starts_with("continuous-future-roll:")),
        "expected continuous-future-roll timer, found {timer_names:?}"
    );
}

#[rstest]
fn test_subscribe_continuous_future_bars_external_uses_bar_source(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, _test_clock, recorder) =
        register_continuous_future_subscription_engine(cache, 0);

    let target_bar_type = BarType::from("ES.GLBX-1-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL");
    let params = continuous_future_transitions_params(10, pre_id, post_id);

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("GLBX")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    assert_eq!(recorder.borrow().len(), 1);

    let DataCommand::Subscribe(SubscribeCommand::Bars(child)) = recorder.borrow()[0].clone() else {
        panic!(
            "expected child SubscribeBars, was {:?}",
            recorder.borrow()[0]
        );
    };

    assert_eq!(
        child.bar_type,
        BarType::from("ESH24.GLBX-1-MINUTE-LAST-EXTERNAL")
    );
}

#[rstest]
fn test_continuous_future_subscription_transition_swaps_source(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, test_clock, recorder) =
        register_continuous_future_subscription_engine(cache.clone(), 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");
    let transition_ns = 10u64;
    let params = continuous_future_transitions_params(transition_ns, pre_id, post_id);

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("GLBX")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));
    assert_eq!(recorder.borrow().len(), 1);

    // Trade in the pre segment publishes a bar adjusted by the BACKWARD_SPREAD offset,
    // post_price - pre_price = +5.
    data_engine
        .borrow_mut()
        .process_data(Data::Trade(make_trade(pre_id, "100.00", 1, "pre-1", 1)));
    let pre_bar = cache
        .borrow()
        .bar(&target_bar_type)
        .copied()
        .expect("expected pre-transition bar in cache");
    assert_eq!(pre_bar.open, Price::from("105.00"));

    let events = test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(transition_ns), true);
    let handlers = test_clock.borrow().match_handlers(events);
    for handler in handlers {
        handler.callback.call(handler.event);
    }

    // Recorder now has unsub(pre) and sub(post)
    assert_eq!(recorder.borrow().len(), 3);

    let DataCommand::Unsubscribe(UnsubscribeCommand::Trades(unsub)) = recorder.borrow()[1].clone()
    else {
        panic!(
            "expected child UnsubscribeTrades, was {:?}",
            recorder.borrow()[1]
        );
    };

    assert_eq!(unsub.instrument_id, pre_id);

    let DataCommand::Subscribe(SubscribeCommand::Trades(sub2)) = recorder.borrow()[2].clone()
    else {
        panic!(
            "expected child SubscribeTrades, was {:?}",
            recorder.borrow()[2]
        );
    };

    assert_eq!(sub2.instrument_id, post_id);

    // Trade in the post segment now publishes a bar with no adjustment for the final
    // segment, BACKWARD_SPREAD cumulative offset is zero.
    data_engine
        .borrow_mut()
        .process_data(Data::Trade(make_trade(post_id, "110.00", 1, "post-1", 11)));
    let post_bar = cache.borrow().bar(&target_bar_type).copied().unwrap();
    assert_eq!(post_bar.open, Price::from("110.00"));
}

#[rstest]
fn test_unsubscribe_continuous_future_bars_tears_down_subscription(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, test_clock, recorder) =
        register_continuous_future_subscription_engine(cache, 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");
    let params = continuous_future_transitions_params(10, pre_id, post_id);

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("XNAS")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params.clone()),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));
    assert_eq!(recorder.borrow().len(), 1);

    let unsub = UnsubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("XNAS")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(unsub)));

    assert_eq!(recorder.borrow().len(), 2);

    let DataCommand::Unsubscribe(UnsubscribeCommand::Trades(child)) = recorder.borrow()[1].clone()
    else {
        panic!(
            "expected child UnsubscribeTrades, was {:?}",
            recorder.borrow()[1]
        );
    };

    assert_eq!(child.instrument_id, pre_id);
    assert_eq!(child.venue, Some(Venue::from("GLBX")));

    let leftover_roll_timers = test_clock
        .borrow()
        .timer_names()
        .into_iter()
        .filter(|name| name.starts_with("continuous-future-roll:"))
        .count();
    assert_eq!(leftover_roll_timers, 0);
}

#[rstest]
fn test_continuous_future_subscription_idempotent_resubscribe(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, _test_clock, recorder) =
        register_continuous_future_subscription_engine(cache, 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");
    let params = continuous_future_transitions_params(10, pre_id, post_id);
    let venue = Venue::from("GLBX");

    let subscribe = || {
        let sub = SubscribeBars::new(
            target_bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(params.clone()),
        );
        data_engine
            .borrow_mut()
            .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));
    };

    let unsubscribe = || {
        let unsub = UnsubscribeBars::new(
            target_bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(params.clone()),
        );
        data_engine
            .borrow_mut()
            .execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(unsub)));
    };

    subscribe();
    unsubscribe();
    subscribe();

    assert_eq!(recorder.borrow().len(), 3);

    let kinds: Vec<&'static str> = recorder
        .borrow()
        .iter()
        .map(|cmd| match cmd {
            DataCommand::Subscribe(SubscribeCommand::Trades(_)) => "sub-trades",
            DataCommand::Unsubscribe(UnsubscribeCommand::Trades(_)) => "unsub-trades",
            other => panic!("unexpected child command {other:?}"),
        })
        .collect();

    assert_eq!(kinds, vec!["sub-trades", "unsub-trades", "sub-trades"]);
}

#[rstest]
fn test_continuous_future_subscription_rejects_bar_types_param(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, test_clock, recorder) =
        register_continuous_future_subscription_engine(cache, 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": 10,
                "pre_instrument_id": pre_id.to_string(),
                "post_instrument_id": post_id.to_string(),
                "pre_price": "100.00",
                "post_price": "105.00"
            }
        ],
        "bar_types": ["ES.GLBX-1-TICK-LAST-INTERNAL"],
    }));

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("GLBX")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    assert!(recorder.borrow().is_empty());
    let roll_timers = test_clock
        .borrow()
        .timer_names()
        .into_iter()
        .filter(|name| name.starts_with("continuous-future-roll:"))
        .count();
    assert_eq!(roll_timers, 0);
}

#[rstest]
fn test_continuous_future_subscription_walks_multiple_transitions(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");
    let esu = add_es_contract(&cache, "ESU24.GLBX", "ESU24");

    let (data_engine, test_clock, recorder) =
        register_continuous_future_subscription_engine(cache, 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");
    let params = params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": 10,
                "pre_instrument_id": esh.to_string(),
                "post_instrument_id": esm.to_string(),
                "pre_price": "100.00",
                "post_price": "105.00"
            },
            {
                "transition_time_ns": 20,
                "pre_instrument_id": esm.to_string(),
                "post_instrument_id": esu.to_string(),
                "pre_price": "110.00",
                "post_price": "115.00"
            }
        ]
    }));

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("GLBX")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let fire_timers = |clock: &Rc<RefCell<VirtualClock>>, to_ns: u64| {
        let events = clock
            .borrow_mut()
            .advance_time(UnixNanos::from(to_ns), true);
        let handlers = clock.borrow().match_handlers(events);
        for handler in handlers {
            handler.callback.call(handler.event);
        }
    };

    fire_timers(&test_clock, 10);
    fire_timers(&test_clock, 20);

    let kinds: Vec<&'static str> = recorder
        .borrow()
        .iter()
        .map(|cmd| match cmd {
            DataCommand::Subscribe(SubscribeCommand::Trades(_)) => "sub",
            DataCommand::Unsubscribe(UnsubscribeCommand::Trades(_)) => "unsub",
            other => panic!("unexpected child command {other:?}"),
        })
        .collect();

    assert_eq!(kinds, vec!["sub", "unsub", "sub", "unsub", "sub"]);

    let ids: Vec<InstrumentId> = recorder
        .borrow()
        .iter()
        .map(|cmd| match cmd {
            DataCommand::Subscribe(SubscribeCommand::Trades(c)) => c.instrument_id,
            DataCommand::Unsubscribe(UnsubscribeCommand::Trades(c)) => c.instrument_id,
            _ => unreachable!(),
        })
        .collect();

    assert_eq!(ids, vec![esh, esh, esm, esm, esu]);

    // No more transition timers remain after the last roll
    let leftover_roll_timers = test_clock
        .borrow()
        .timer_names()
        .into_iter()
        .filter(|name| name.starts_with("continuous-future-roll:"))
        .count();
    assert_eq!(leftover_roll_timers, 0);
}

#[rstest]
fn test_continuous_future_subscription_warns_on_unknown_unsubscribe(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let _ = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let _ = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, _test_clock, recorder) =
        register_continuous_future_subscription_engine(cache, 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");

    let unsub = UnsubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("GLBX")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(unsub)));

    // Standard bar unsubscribe path is taken; no continuous-future subscription state
    // existed so no child unsubscribe-trades is recorded.
    assert!(
        !recorder
            .borrow()
            .iter()
            .any(|cmd| matches!(cmd, DataCommand::Unsubscribe(UnsubscribeCommand::Trades(_))))
    );
}

#[rstest]
fn test_continuous_future_subscription_uses_quote_source_for_non_last_price_type(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let (data_engine, _test_clock, recorder) =
        register_continuous_future_subscription_engine(cache, 0);

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-BID-INTERNAL");
    let params = continuous_future_transitions_params(10, pre_id, post_id);

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(Venue::from("GLBX")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    assert_eq!(recorder.borrow().len(), 1);

    let DataCommand::Subscribe(SubscribeCommand::Quotes(child)) = recorder.borrow()[0].clone()
    else {
        panic!(
            "expected child SubscribeQuotes, was {:?}",
            recorder.borrow()[0]
        );
    };

    assert_eq!(child.instrument_id, pre_id);
}

#[rstest]
fn test_continuous_future_subscription_rejected_when_roller_missing(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let pre_id = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let post_id = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let engine_clock: Rc<RefCell<dyn Clock>> = test_clock.clone();
    let mut data_engine = DataEngine::new(engine_clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let venue = Venue::from("GLBX");
    register_mock_client(
        test_clock.clone(),
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let target_bar_type = BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL");
    let params = continuous_future_transitions_params(10, pre_id, post_id);

    let sub = SubscribeBars::new(
        target_bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    let result = data_engine.execute_subscribe(SubscribeCommand::Bars(sub));

    let err = result.expect_err("expected subscribe to fail without a roller");
    assert!(
        err.to_string().contains("roller is not initialized"),
        "unexpected error: {err}"
    );
    assert!(recorder.borrow().is_empty());

    let roll_timers = test_clock
        .borrow()
        .timer_names()
        .into_iter()
        .filter(|name| name.starts_with("continuous-future-roll:"))
        .count();
    assert_eq!(roll_timers, 0);
}

fn recorded_bars_request(recorder: &Rc<RefCell<Vec<DataCommand>>>, index: usize) -> RequestBars {
    match recorder.borrow()[index].clone() {
        DataCommand::Request(RequestCommand::Bars(request)) => request,
        other => panic!("Expected child bar request, was {other:?}"),
    }
}

fn recorded_trades_request(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
    index: usize,
) -> RequestTrades {
    match recorder.borrow()[index].clone() {
        DataCommand::Request(RequestCommand::Trades(request)) => request,
        other => panic!("Expected child trade request, was {other:?}"),
    }
}

fn recorded_quotes_request(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
    index: usize,
) -> RequestQuotes {
    match recorder.borrow()[index].clone() {
        DataCommand::Request(RequestCommand::Quotes(request)) => request,
        other => panic!("Expected child quote request, was {other:?}"),
    }
}

fn continuous_future_transitions_params(
    transition_time_ns: u64,
    pre_id: InstrumentId,
    post_id: InstrumentId,
) -> Params {
    params_from_json(json!({
        "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
        "continuous_future_transitions": [
            {
                "transition_time_ns": transition_time_ns,
                "pre_instrument_id": pre_id.to_string(),
                "post_instrument_id": post_id.to_string(),
                "pre_price": "100.00",
                "post_price": "105.00"
            }
        ]
    }))
}

#[expect(
    clippy::type_complexity,
    reason = "test setup returns coupled engine, clock, and command recorder handles"
)]
fn register_continuous_future_subscription_engine(
    cache: Rc<RefCell<Cache>>,
    initial_ns: u64,
) -> (
    Rc<RefCell<DataEngine>>,
    Rc<RefCell<VirtualClock>>,
    Rc<RefCell<Vec<DataCommand>>>,
) {
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(initial_ns), true);
    let engine_clock: Rc<RefCell<dyn Clock>> = test_clock.clone();

    let data_engine = Rc::new(RefCell::new(DataEngine::new(
        engine_clock,
        cache.clone(),
        None,
    )));
    DataEngine::register_msgbus_handlers(&data_engine);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let client_id = ClientId::test_default();
    let venue = Venue::from("GLBX");
    let client = MockDataClient::new_with_recorder(
        test_clock.clone(),
        cache,
        client_id,
        Some(venue),
        Some(recorder.clone()),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.borrow_mut().register_client(adapter, None);

    (data_engine, test_clock, recorder)
}
