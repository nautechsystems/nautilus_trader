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

#[cfg(feature = "streaming")]
use super::*;

#[cfg(feature = "streaming")]
#[rstest]
fn test_continuous_future_request_serves_segments_from_catalog(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    // With every segment covered by the catalog, the client is never asked and the parent
    // response completes from catalog legs alone.
    let _ = stub_msgbus;
    let minute = |value: u64| value * 60_000_000_000;
    let clock = data_engine_clock_at(minute(3));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let esh = add_es_contract(&cache, "ESH24.GLBX", "ESH24");
    let esm = add_es_contract(&cache, "ESM24.GLBX", "ESM24");

    let venue = Venue::from("GLBX");
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

    let catalog_dir = CatalogTempDir::new("continuous-future-segments");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    let esh_bar_type = BarType::from("ESH24.GLBX-1-MINUTE-LAST-EXTERNAL");
    let esm_bar_type = BarType::from("ESM24.GLBX-1-MINUTE-LAST-EXTERNAL");

    for bar in [
        make_bar(
            esh_bar_type,
            "100.00",
            "101.00",
            "99.00",
            "100.50",
            1,
            minute(1),
        ),
        make_bar(
            esm_bar_type,
            "96.00",
            "97.00",
            "95.50",
            "96.50",
            2,
            minute(2),
        ),
    ] {
        catalog
            .write_to_parquet(
                &[bar],
                Some(UnixNanos::default()),
                Some(UnixNanos::from(minute(10))),
                None,
            )
            .unwrap();
    }

    data_engine.register_catalog(Box::new(catalog), None);

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
            }
        ]
    }));
    let (response_handler, response_saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("continuous-catalog-bars")));
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

    let cached_bar = cache
        .borrow()
        .bar(&target_bar_type.standard())
        .copied()
        .unwrap();
    let responses = response_saver.get_messages();
    assert!(recorder.borrow().is_empty());
    assert_eq!(responses.len(), 1);
    assert_eq!(response_data_count(&responses[0]), Some(2));
    assert_eq!(cached_bar.open, Price::from("96.00"));
    assert_eq!(cached_bar.close, Price::from("96.50"));
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_quotes_from_catalog(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_quote_catalog(&mut data_engine, audusd_sim.id, 1_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let SubscribeCommand::Quotes(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected quotes subscribe");
    };

    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(1_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_quotes_preserves_existing_start_ns(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_quote_catalog(&mut data_engine, audusd_sim.id, 1_000);
    let params: Params = serde_json::from_value(json!({"start_ns": 42})).unwrap();
    let correlation_id = UUID4::new();

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        Some(params),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let SubscribeCommand::Quotes(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected quotes subscribe");
    };

    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(42)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_quotes_sets_null_without_catalog_hit(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_empty_catalog(&mut data_engine, "empty-quotes");
    let correlation_id = UUID4::new();

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let SubscribeCommand::Quotes(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected quotes subscribe");
    };

    let null_value = json!(null);
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get("start_ns")),
        Some(&null_value)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_trades_from_catalog(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_trade_catalog(&mut data_engine, audusd_sim.id, 2_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Trades(sub)));

    let SubscribeCommand::Trades(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected trades subscribe");
    };

    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(2_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_external_bars_from_catalog(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
    let _catalog_dir = register_bar_catalog(&mut data_engine, bar_type, 3_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let SubscribeCommand::Bars(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected bars subscribe");
    };

    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(3_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_skips_internal_bars(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");
    let _catalog_dir = register_bar_catalog(&mut data_engine, bar_type, 4_000);
    let command_id = UUID4::new();

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        command_id,
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let SubscribeCommand::Trades(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, command_id)
    else {
        panic!("expected source trades subscribe");
    };

    assert_eq!(recorded.instrument_id, bar_type.instrument_id());
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get("start_ns")),
        Some(&json!(null))
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_from_catalog(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let data_type = DataType::new("CustomFeed", None, Some("SIM//AUDUSD".to_string()));
    let _catalog_dir = register_custom_catalog(&mut data_engine, &data_type, 5_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };

    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(5_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_sets_null_without_catalog_hit(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let data_type = DataType::new("CustomFeed", None, Some("SIM//MISSING".to_string()));
    let _catalog_dir = register_empty_catalog(&mut data_engine, "empty-custom");
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };

    let null_value = json!(null);
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get("start_ns")),
        Some(&null_value)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_without_identifier_merges_catalog_intervals(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let type_name = "CustomFeed";
    let catalog_dir = CatalogTempDir::new("custom-no-identifier");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    write_custom_catalog_file(
        &catalog_dir,
        &catalog,
        type_name,
        Some("SIM//AUDUSD"),
        1_000,
        10_000,
    );
    write_custom_catalog_file(
        &catalog_dir,
        &catalog,
        type_name,
        Some("SIM//EURUSD"),
        5_000,
        6_000,
    );
    data_engine.register_catalog(Box::new(catalog), None);
    let data_type = DataType::new(type_name, None, None);
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };

    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(10_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_preserves_existing_start_ns(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let data_type = DataType::new("CustomFeed", None, Some("SIM//AUDUSD".to_string()));
    let _catalog_dir = register_custom_catalog(&mut data_engine, &data_type, 6_000);
    let params: Params = serde_json::from_value(json!({"start_ns": 42})).unwrap();
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        Some(params),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };

    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(42)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_preserves_command_metadata(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let metadata = serde_json::from_value(json!({
        "instrument_id": "IGNORED.SIM",
        "source": "metadata",
    }))
    .unwrap();

    let data_type = DataType::new(
        "CustomMetadataFeed",
        Some(metadata),
        Some("SIM//METADATA".to_string()),
    );
    let _catalog_dir = register_custom_catalog(&mut data_engine, &data_type, 7_000);
    let command_id = UUID4::new();
    let ts_init = UnixNanos::from(123);
    let correlation_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({"source": "params"})).unwrap();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        command_id,
        ts_init,
        Some(correlation_id),
        Some(params),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };

    assert_eq!(recorded.client_id, Some(client_id));
    assert_eq!(recorded.venue, Some(venue));
    assert_eq!(recorded.data_type.type_name(), data_type.type_name());
    assert_eq!(recorded.data_type.metadata(), data_type.metadata());
    assert_eq!(recorded.data_type.identifier(), data_type.identifier());
    assert_eq!(recorded.command_id, command_id);
    assert_eq!(recorded.ts_init, ts_init);
    assert_eq!(recorded.correlation_id, Some(correlation_id));
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(7_001)
    );
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_str("source")),
        Some("params")
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_time_range_pipeline_child_uses_catalog_client_fanin(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_quote_catalog_with_quotes(
        &mut data_engine,
        "time-range-split-quotes",
        &[split_quote(instrument_id, 1_500_000_000)],
        Some((1_000_000_000, 1_500_000_000)),
    );
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("time-range-split-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [2],
    }))
    .unwrap();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(5_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "first time-range child should split into one client leg"
    );
    assert_eq!(data_engine.request_pipeline_count(), 1);
    assert_eq!(data_engine.time_range_pipeline_count(), 1);

    data_engine.response(time_range_quote_response(
        &recorded[0],
        instrument_id,
        client_id,
        1,
        vec![split_quote(instrument_id, 2_500_000_000)],
    ));

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(
        recorded.len(),
        2,
        "next time-range child should be issued after catalog/client fan-in"
    );
    assert_eq!(
        recorded[1].start.map(|dt| dt.as_nanosecond()),
        Some(3_000_000_001)
    );
    assert_eq!(
        recorded[1].end.map(|dt| dt.as_nanosecond()),
        Some(5_000_000_000)
    );

    data_engine.response(time_range_quote_response(
        &recorded[1],
        instrument_id,
        client_id,
        0,
        Vec::new(),
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(data_engine.request_pipeline_count(), 0);
    assert_eq!(data_engine.time_range_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_quotes_catalog_only_serves_from_disk(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_quote_catalog_with_quotes(
        &mut data_engine,
        "catalog-only",
        &[
            split_quote(instrument_id, 1_000),
            split_quote(instrument_id, 2_000),
        ],
        Some((1_000, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("catalog-only")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_quotes_client_only_when_catalog_has_no_data(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "empty-quotes-only");

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].start.map(|d| d.as_nanosecond()), Some(1_000));
    assert_eq!(recorded[0].end.map(|d| d.as_nanosecond()), Some(3_000));
    assert_eq!(data_engine.request_pipeline_count(), 1);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_quotes_catalog_plus_client_split(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_quote_catalog_with_quotes(
        &mut data_engine,
        "split-quotes",
        &[split_quote(instrument_id, 1_500)],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("split-quotes-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let parent_limit = NonZeroUsize::new(50).unwrap();
    let sentinel_params: Params = serde_json::from_value(json!({"feed_tag": "alpha"})).unwrap();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        Some(parent_limit),
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(sentinel_params),
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "expected one client leg for the missing interval"
    );
    let client_start = recorded[0].start.map_or(0, |d| d.as_nanosecond());
    let client_end = recorded[0].end.map_or(0, |d| d.as_nanosecond());
    assert!(
        client_start > 1_500,
        "client leg should start after the catalog coverage ends (was {client_start})"
    );
    assert_eq!(client_end, 3_000);
    assert_eq!(
        recorded[0].limit,
        Some(parent_limit),
        "with_dates_for_pipeline must carry the parent limit to each leg"
    );
    assert_eq!(
        recorded[0]
            .params
            .as_ref()
            .and_then(|p| p.get("feed_tag"))
            .and_then(Value::as_str),
        Some("alpha"),
        "with_dates_for_pipeline must carry parent params to each leg"
    );

    let leg_request_id = recorded[0].request_id;
    data_engine.response(leg_quotes_response(
        leg_request_id,
        instrument_id,
        client_id,
        vec![split_quote(instrument_id, 2_500)],
        recorded[0]
            .start
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        recorded[0]
            .end
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_500, 2_500]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_quotes_skip_catalog_data_param_honored(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_quote_catalog_with_quotes(
        &mut data_engine,
        "skip-catalog",
        &[split_quote(instrument_id, 1_500)],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let params: Params = serde_json::from_value(json!({"skip_catalog_data": true})).unwrap();
    let parent_id = UUID4::new();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(recorded.len(), 1, "skip flag should bypass catalog leg");
    assert_eq!(
        recorded[0].start.map(|d| d.as_nanosecond()),
        Some(1_000),
        "client leg should cover the full parent window when catalog is skipped"
    );
    assert_eq!(recorded[0].end.map(|d| d.as_nanosecond()), Some(3_000));
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_quotes_no_client_and_no_catalog_data_emits_empty(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "empty-no-client");

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("empty-no-client")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_bars_catalog_lookup_uses_bar_type_identifier(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", audusd_sim.id).as_str());
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_bar_catalog_with_bars(
        &mut data_engine,
        "bars-by-bar-type",
        &[split_bar(bar_type, 2_000)],
        Some((1_000, 3_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("bars-by-bar-type")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Bars(RequestBars::new(
        bar_type,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].bar_type, bar_type);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|b| b.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![2_000]);
    assert_eq!(received[0].data[0].bar_type, bar_type);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_trades_catalog_plus_client_split(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_trade_catalog_with_trades(
        &mut data_engine,
        "split-trades",
        &[split_trade(instrument_id, 1_500, "T-1")],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<TradesResponse>(Some(Ustr::from("split-trades-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Trades(RequestTrades::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_trades(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "expected one client leg for trades split"
    );
    let leg_request_id = recorded[0].request_id;

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        leg_request_id,
        client_id,
        instrument_id,
        vec![split_trade(instrument_id, 2_500, "T-2")],
        recorded[0]
            .start
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        recorded[0]
            .end
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        UnixNanos::default(),
        None,
    )));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|t| t.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_500, 2_500]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_quotes_dispatches_straight_to_client_with_no_catalog_registered(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let original_start = UnixNanos::from(1_000).to_datetime_utc();
    let original_end = UnixNanos::from(3_000).to_datetime_utc();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(original_start),
        Some(original_end),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "no-catalog path must dispatch a single direct client request"
    );
    assert_eq!(
        recorded[0].request_id, parent_id,
        "no-catalog path must preserve the parent request id (no pipeline rebinding)"
    );
    assert_eq!(recorded[0].start, Some(original_start));
    assert_eq!(recorded[0].end, Some(original_end));
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_pipeline_count_resets_after_catalog_split_fanin(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_quote_catalog_with_quotes(
        &mut data_engine,
        "pipeline-reset",
        &[split_quote(instrument_id, 1_500)],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(recorded.len(), 1);

    data_engine.response(leg_quotes_response(
        recorded[0].request_id,
        instrument_id,
        client_id,
        vec![split_quote(instrument_id, 2_500)],
        recorded[0]
            .start
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        recorded[0]
            .end
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
    ));

    assert_eq!(data_engine.request_pipeline_count(), 0);
    assert_eq!(data_engine.pending_join_request_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_bars_catalog_plus_client_split(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", audusd_sim.id).as_str());
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_bar_catalog_with_bars(
        &mut data_engine,
        "bars-split",
        &[split_bar(bar_type, 1_500)],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("bars-split-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Bars(RequestBars::new(
        bar_type,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded: Vec<RequestBars> = recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Bars(req)) => Some(req.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].bar_type, bar_type,
        "client leg must preserve the parent bar_type"
    );

    data_engine.response(DataResponse::Bars(BarsResponse::new(
        recorded[0].request_id,
        client_id,
        bar_type,
        vec![split_bar(bar_type, 2_500)],
        recorded[0]
            .start
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        recorded[0]
            .end
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        UnixNanos::default(),
        None,
    )));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].bar_type, bar_type);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|b| b.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_500, 2_500]);
    assert_eq!(received[0].data[0].bar_type, bar_type);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_funding_rates_catalog_only_serves_from_disk(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_funding_catalog_with_rates(
        &mut data_engine,
        "funding-catalog-only",
        &[
            split_funding_rate(instrument_id, 1_000, "0.0001"),
            split_funding_rate(instrument_id, 2_000, "0.0002"),
        ],
        Some((1_000, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<FundingRatesResponse>(Some(Ustr::from("funding-catalog-only")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::FundingRates(RequestFundingRates::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|rate| rate.ts_init.as_u64())
        .collect();

    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(ts_inits, vec![1_000, 2_000]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_funding_rates_catalog_plus_client_split(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_funding_catalog_with_rates(
        &mut data_engine,
        "funding-split",
        &[split_funding_rate(instrument_id, 1_500, "0.0001")],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<FundingRatesResponse>(Some(Ustr::from("funding-split-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::FundingRates(RequestFundingRates::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_funding_rates(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "expected one client leg for the missing interval"
    );

    data_engine.response(DataResponse::FundingRates(FundingRatesResponse::new(
        recorded[0].request_id,
        client_id,
        instrument_id,
        vec![split_funding_rate(instrument_id, 2_500, "0.0002")],
        recorded[0].start.map(datetime_to_unix_nanos_for_test),
        recorded[0].end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        None,
    )));

    let received = saver.get_messages();
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|rate| rate.ts_init.as_u64())
        .collect();

    assert_eq!(received.len(), 1);
    assert_eq!(ts_inits, vec![1_500, 2_500]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_funding_rates_no_client_no_catalog_emits_empty(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "funding-empty");

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<FundingRatesResponse>(Some(Ustr::from("funding-empty")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::FundingRates(RequestFundingRates::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_funding_rates_dispatches_straight_to_client_with_no_catalog_registered(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let req = RequestCommand::FundingRates(RequestFundingRates::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_funding_rates(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].request_id, parent_id);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_custom_data_catalog_only_serves_from_disk(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = InstrumentId::from("RUST.TEST");
    let data_type = rust_test_custom_data_type("RUST.TEST");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_custom_catalog_with_data(
        &mut data_engine,
        "custom-catalog-only",
        vec![
            split_custom(data_type.clone(), instrument_id, 1_000, 1.0),
            split_custom(data_type.clone(), instrument_id, 2_000, 2.0),
        ],
        Some((1_000, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<CustomDataResponse>(Some(Ustr::from("custom-catalog-only")));
    msgbus::register_response_handler(&parent_id, handler);
    let params: Params = serde_json::from_value(json!({"source": "params"})).unwrap();

    let req = RequestCommand::Data(RequestCustomData::new(
        client_id,
        data_type.clone(),
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    let data = custom_response_payload(&received[0]);

    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].data_type, data_type);
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get_bool("update_catalog")),
        Some(false)
    );
    assert_eq!(custom_values(&data), vec![1.0, 2.0]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_custom_data_without_identifier_catalog_only_serves_from_disk(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = InstrumentId::from("RUST.TEST");

    let data_type = DataType::new(
        "RustTestCustomData",
        Some(serde_json::from_value(json!({"source": "catalog-test"})).unwrap()),
        None,
    );
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_custom_catalog_with_data(
        &mut data_engine,
        "custom-catalog-no-identifier",
        vec![
            split_custom(data_type.clone(), instrument_id, 1_000, 1.0),
            split_custom(data_type.clone(), instrument_id, 2_000, 2.0),
        ],
        Some((1_000, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) = get_any_saving_handler::<CustomDataResponse>(Some(Ustr::from(
        "custom-catalog-no-identifier",
    )));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Data(RequestCustomData::new(
        client_id,
        data_type.clone(),
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    let data = custom_response_payload(&received[0]);

    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].data_type, data_type);
    assert_eq!(custom_values(&data), vec![1.0, 2.0]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_custom_data_catalog_plus_client_split(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = InstrumentId::from("RUST.TEST");
    let data_type = rust_test_custom_data_type("RUST.TEST");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_custom_catalog_with_data(
        &mut data_engine,
        "custom-split",
        vec![split_custom(data_type.clone(), instrument_id, 1_500, 1.5)],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<CustomDataResponse>(Some(Ustr::from("custom-split-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Data(RequestCustomData::new(
        client_id,
        data_type.clone(),
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_data(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "expected one client leg for the missing interval"
    );

    data_engine.response(DataResponse::Data(CustomDataResponse::new(
        recorded[0].request_id,
        client_id,
        Some(venue),
        data_type.clone(),
        split_custom(data_type, instrument_id, 2_500, 2.5),
        recorded[0].start.map(datetime_to_unix_nanos_for_test),
        recorded[0].end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        None,
    )));

    let received = saver.get_messages();
    let data = custom_response_payload(&received[0]);

    assert_eq!(received.len(), 1);
    assert_eq!(custom_values(&data), vec![1.5, 2.5]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_custom_data_no_client_no_catalog_emits_empty(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let data_type = rust_test_custom_data_type("RUST.TEST");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "custom-empty");

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<CustomDataResponse>(Some(Ustr::from("custom-empty")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Data(RequestCustomData::new(
        client_id,
        data_type,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    let data = custom_response_payload(&received[0]);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(data.is_empty());
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_custom_data_dispatches_straight_to_client_with_no_catalog_registered(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let data_type = rust_test_custom_data_type("RUST.TEST");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let req = RequestCommand::Data(RequestCustomData::new(
        client_id,
        data_type,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_data(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].request_id, parent_id);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_instruments_no_client_no_catalog_emits_empty(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "instruments-empty");

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<InstrumentsResponse>(Some(Ustr::from("instruments-empty")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Instruments(RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_instrument_catalog_uses_latest_record(
    mut audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let mut earlier = audusd_sim.clone();
    earlier.ts_init = UnixNanos::from(1_000);
    audusd_sim.ts_init = UnixNanos::from(2_000);
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_instrument_catalog_with_instruments(
        &mut data_engine,
        "instrument-latest",
        vec![
            InstrumentAny::CurrencyPair(earlier),
            InstrumentAny::CurrencyPair(audusd_sim),
        ],
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<InstrumentResponse>(Some(Ustr::from("instrument-latest")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Instrument(RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].instrument_id, instrument_id);
    assert_eq!(received[0].data.ts_init(), UnixNanos::from(2_000));
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_instruments_catalog_applies_only_last(
    mut audusd_sim: CurrencyPair,
    mut gbpusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let mut audusd_earlier = audusd_sim.clone();
    audusd_earlier.ts_init = UnixNanos::from(1_000);
    audusd_sim.ts_init = UnixNanos::from(2_000);
    gbpusd_sim.ts_init = UnixNanos::from(3_000);
    let audusd_id = audusd_sim.id;
    let gbpusd_id = gbpusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_instrument_catalog_with_instruments(
        &mut data_engine,
        "instruments-only-last",
        vec![
            InstrumentAny::CurrencyPair(audusd_earlier),
            InstrumentAny::CurrencyPair(audusd_sim),
            InstrumentAny::CurrencyPair(gbpusd_sim),
        ],
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<InstrumentsResponse>(Some(Ustr::from("instruments-only-last")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Instruments(RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    let mut ids_and_timestamps: Vec<(InstrumentId, u64)> = received[0]
        .data
        .iter()
        .map(|instrument| (instrument.id(), instrument.ts_init().as_u64()))
        .collect();
    ids_and_timestamps.sort_by_key(|(id, _)| id.to_string());

    assert_eq!(received.len(), 1);
    assert_eq!(
        ids_and_timestamps,
        vec![(audusd_id, 2_000), (gbpusd_id, 3_000)]
    );
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
#[case::later_id_after_clock(3_000, 20_000_000_000)]
#[case::earlier_id_after_clock(20_000_000_000, 3_000)]
fn test_request_instruments_catalog_result_independent_of_id_order(
    mut audusd_sim: CurrencyPair,
    mut gbpusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
    #[case] audusd_ts_init: u64,
    #[case] gbpusd_ts_init: u64,
) {
    // The query has no end bound, so a definition stamped after the clock is still returned
    let _ = stub_msgbus;
    audusd_sim.ts_init = UnixNanos::from(audusd_ts_init);
    gbpusd_sim.ts_init = UnixNanos::from(gbpusd_ts_init);
    let audusd_id = audusd_sim.id;
    let gbpusd_id = gbpusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_instrument_catalog_with_instruments(
        &mut data_engine,
        "instruments-id-order",
        vec![
            InstrumentAny::CurrencyPair(audusd_sim),
            InstrumentAny::CurrencyPair(gbpusd_sim),
        ],
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<InstrumentsResponse>(Some(Ustr::from("instruments-id-order")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Instruments(RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    let ids: Vec<InstrumentId> = received[0]
        .data
        .iter()
        .map(|instrument| instrument.id())
        .collect();

    assert_eq!(received.len(), 1);
    assert_eq!(ids, vec![audusd_id, gbpusd_id]);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_instrument_dispatches_straight_to_client_with_no_catalog_registered(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let req = RequestCommand::Instrument(RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_instrument(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].request_id, parent_id);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_instruments_dispatches_straight_to_client_with_no_catalog_registered(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let req = RequestCommand::Instruments(RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_instruments(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].request_id, parent_id);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_instrument_force_update_dispatches_to_client_with_catalog_registered(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_instrument_catalog_with_instruments(
        &mut data_engine,
        "instrument-force-update",
        vec![InstrumentAny::CurrencyPair(audusd_sim)],
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let params: Params = serde_json::from_value(json!({"force_instrument_update": true})).unwrap();
    let parent_id = UUID4::new();
    let req = RequestCommand::Instrument(RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_instrument(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].request_id, parent_id);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_instruments_update_catalog_dispatches_to_client_with_catalog_registered(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_instrument_catalog_with_instruments(
        &mut data_engine,
        "instruments-update-catalog",
        vec![InstrumentAny::CurrencyPair(audusd_sim)],
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let params: Params = serde_json::from_value(json!({"update_catalog": true})).unwrap();
    let parent_id = UUID4::new();
    let req = RequestCommand::Instruments(RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_instruments(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].request_id, parent_id);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_subscription_name_param_disables_now_clamping(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    // Clock at 1_000; the request asks for data up to 5_000. Without the
    // subscription_name bypass, bound_request_dates clamps end to 1_000.
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 1_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "subscription-name");

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let params: Params = serde_json::from_value(json!({"subscription_name": "feed-a"})).unwrap();
    let parent_id = UUID4::new();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        Some(UnixNanos::from(5_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_quotes(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].start.map(|d| d.as_nanosecond()),
        Some(2_000),
        "subscription_name must bypass start clamping"
    );
    assert_eq!(
        recorded[0].end.map(|d| d.as_nanosecond()),
        Some(5_000),
        "subscription_name must bypass end clamping"
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_book_deltas_catalog_only_serves_from_disk(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-catalog-only",
        &[
            split_delta(instrument_id, 1_000),
            split_delta(instrument_id, 2_000),
        ],
        Some((1_000, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-catalog-only")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|d| d.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_book_deltas_catalog_plus_client_split(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-split",
        &[split_delta(instrument_id, 1_500)],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-split-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_book_deltas(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "expected one client leg for the missing interval"
    );
    assert_eq!(recorded[0].instrument_id, instrument_id);

    let leg_request_id = recorded[0].request_id;
    data_engine.response(DataResponse::BookDeltas(BookDeltasResponse::new(
        leg_request_id,
        client_id,
        instrument_id,
        vec![split_delta(instrument_id, 2_500)],
        recorded[0]
            .start
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        recorded[0]
            .end
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        UnixNanos::default(),
        None,
    )));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|d| d.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_500, 2_500]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_book_deltas_no_client_no_catalog_emits_empty(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "deltas-empty");

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-empty")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_book_depth_catalog_only_serves_from_disk(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_depth_catalog_with_depths(
        &mut data_engine,
        "depth-catalog-only",
        &[
            book_depth_at(instrument_id, 1_000),
            book_depth_at(instrument_id, 2_000),
        ],
        Some((1_000, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDepthResponse>(Some(Ustr::from("depth-catalog-only")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDepth(RequestBookDepth::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|d| d.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_book_depth_catalog_plus_client_split(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let _catalog_dir = register_depth_catalog_with_depths(
        &mut data_engine,
        "depth-split",
        &[book_depth_at(instrument_id, 1_500)],
        Some((1_000, 1_500)),
    );

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let mock_client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDepthResponse>(Some(Ustr::from("depth-split-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let parent_depth = NonZeroUsize::new(10).unwrap();
    let req = RequestCommand::BookDepth(RequestBookDepth::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(parent_depth),
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_request_book_depth(&recorder);
    assert_eq!(
        recorded.len(),
        1,
        "expected one client leg for the missing interval"
    );
    assert_eq!(recorded[0].instrument_id, instrument_id);
    assert_eq!(
        recorded[0].depth,
        Some(parent_depth),
        "with_dates_for_pipeline must carry the parent depth to each leg"
    );

    let leg_request_id = recorded[0].request_id;
    data_engine.response(DataResponse::BookDepth(BookDepthResponse::new(
        leg_request_id,
        client_id,
        instrument_id,
        vec![book_depth_at(instrument_id, 2_500)],
        recorded[0]
            .start
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        recorded[0]
            .end
            .map(|d| UnixNanos::from(u64::try_from(d.as_nanosecond().max(0)).unwrap_or(0))),
        UnixNanos::default(),
        None,
    )));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|d| d.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_500, 2_500]);
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_book_depth_no_client_no_catalog_emits_empty(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_empty_catalog(&mut data_engine, "depth-empty");

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDepthResponse>(Some(Ustr::from("depth-empty")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDepth(RequestBookDepth::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[cfg(feature = "streaming")]
fn register_empty_catalog(data_engine: &mut DataEngine, label: &str) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_quote_catalog(
    data_engine: &mut DataEngine,
    instrument_id: InstrumentId,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("quotes");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog
        .write_to_parquet(
            &[QuoteTick::new(
                instrument_id,
                Price::from("1.0000"),
                Price::from("1.0001"),
                Quantity::from(1),
                Quantity::from(1),
                UnixNanos::from(last_timestamp),
                UnixNanos::from(last_timestamp),
            )],
            None,
            None,
            None,
        )
        .unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_trade_catalog(
    data_engine: &mut DataEngine,
    instrument_id: InstrumentId,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("trades");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog
        .write_to_parquet(
            &[TradeTick::new(
                instrument_id,
                Price::from("1.0000"),
                Quantity::from(1),
                AggressorSide::Buy,
                TradeId::new("T-1"),
                UnixNanos::from(last_timestamp),
                UnixNanos::from(last_timestamp),
            )],
            None,
            None,
            None,
        )
        .unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_bar_catalog(
    data_engine: &mut DataEngine,
    bar_type: BarType,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("bars");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog
        .write_to_parquet(
            &[Bar::new(
                bar_type,
                Price::from("1.0000"),
                Price::from("1.0001"),
                Price::from("0.9999"),
                Price::from("1.0000"),
                Quantity::from(1),
                UnixNanos::from(last_timestamp),
                UnixNanos::from(last_timestamp),
            )],
            None,
            None,
            None,
        )
        .unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_custom_catalog(
    data_engine: &mut DataEngine,
    data_type: &DataType,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("custom");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    write_custom_catalog_file(
        &catalog_dir,
        &catalog,
        data_type.type_name(),
        data_type.identifier(),
        last_timestamp,
        last_timestamp,
    );

    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_recording_client(
    data_engine: &mut DataEngine,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) -> Rc<RefCell<Vec<DataCommand>>> {
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(clock, cache, client_id, venue, None, &recorder, data_engine);
    recorder
}

#[cfg(feature = "streaming")]
fn recorded_subscribe_command_with_correlation(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
    correlation_id: UUID4,
) -> SubscribeCommand {
    let command = recorded_subscribe_command(recorder);
    assert_eq!(command.correlation_id(), Some(correlation_id));
    command
}

#[cfg(feature = "streaming")]
fn register_bar_catalog_with_bars(
    data_engine: &mut DataEngine,
    label: &str,
    bars: &[Bar],
    interval: Option<(u64, u64)>,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);

    let (start, end) = match interval {
        Some((s, e)) => (Some(UnixNanos::from(s)), Some(UnixNanos::from(e))),
        None => (None, None),
    };

    catalog.write_to_parquet(bars, start, end, None).unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn split_bar(bar_type: BarType, ts: u64) -> Bar {
    make_bar(bar_type, "1.0000", "1.0001", "0.9999", "1.0000", 1, ts)
}

#[cfg(feature = "streaming")]
fn recorded_request_quotes(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> Vec<RequestQuotes> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Quotes(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn recorded_request_trades(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> Vec<RequestTrades> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Trades(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn recorded_request_funding_rates(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestFundingRates> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::FundingRates(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn recorded_request_data(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> Vec<RequestCustomData> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Data(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn recorded_request_instrument(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> Vec<RequestInstrument> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Instrument(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn recorded_request_instruments(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestInstruments> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Instruments(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn split_funding_rate(instrument_id: InstrumentId, ts: u64, rate: &str) -> FundingRateUpdate {
    FundingRateUpdate::new(
        instrument_id,
        rate.parse().unwrap(),
        None,
        None,
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

#[cfg(feature = "streaming")]
fn register_funding_catalog_with_rates(
    data_engine: &mut DataEngine,
    label: &str,
    rates: &[FundingRateUpdate],
    interval: Option<(u64, u64)>,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);

    let (start, end) = match interval {
        Some((s, e)) => (Some(UnixNanos::from(s)), Some(UnixNanos::from(e))),
        None => (None, None),
    };

    catalog.write_to_parquet(rates, start, end, None).unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn rust_test_custom_data_type(identifier: &str) -> DataType {
    DataType::new(
        "RustTestCustomData",
        Some(serde_json::from_value(json!({"source": "catalog-test"})).unwrap()),
        Some(identifier.to_string()),
    )
}

#[cfg(feature = "streaming")]
fn split_custom(
    data_type: DataType,
    instrument_id: InstrumentId,
    ts: u64,
    value: f64,
) -> CustomData {
    CustomData::new(
        std::sync::Arc::new(RustTestCustomData {
            instrument_id,
            value,
            flag: value > 1.0,
            ts_event: UnixNanos::from(ts),
            ts_init: UnixNanos::from(ts),
        }),
        data_type,
    )
}

#[cfg(feature = "streaming")]
fn register_custom_catalog_with_data(
    data_engine: &mut DataEngine,
    label: &str,
    data: Vec<CustomData>,
    interval: Option<(u64, u64)>,
) -> CatalogTempDir {
    ensure_engine_custom_data_registered();
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);

    let (start, end) = match interval {
        Some((s, e)) => (Some(UnixNanos::from(s)), Some(UnixNanos::from(e))),
        None => (None, None),
    };

    catalog
        .write_custom_data_batch(data, start, end, None)
        .unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn custom_response_payload(resp: &CustomDataResponse) -> Vec<CustomData> {
    resp.data
        .as_ref()
        .downcast_ref::<Vec<CustomData>>()
        .expect("custom response payload should be Vec<CustomData>")
        .clone()
}

#[cfg(feature = "streaming")]
fn custom_values(data: &[CustomData]) -> Vec<f64> {
    data.iter()
        .map(|custom| {
            custom
                .data
                .as_any()
                .downcast_ref::<RustTestCustomData>()
                .expect("custom payload should be RustTestCustomData")
                .value
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn register_instrument_catalog_with_instruments(
    data_engine: &mut DataEngine,
    label: &str,
    instruments: Vec<InstrumentAny>,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog.write_instruments(instruments).unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn recorded_request_book_deltas(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestBookDeltas> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::BookDeltas(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "streaming")]
fn register_depth_catalog_with_depths(
    data_engine: &mut DataEngine,
    label: &str,
    depths: &[OrderBookDepth],
    interval: Option<(u64, u64)>,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);

    let (start, end) = match interval {
        Some((s, e)) => (Some(UnixNanos::from(s)), Some(UnixNanos::from(e))),
        None => (None, None),
    };

    catalog.write_to_parquet(depths, start, end, None).unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn recorded_request_book_depth(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> Vec<RequestBookDepth> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::BookDepth(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}
