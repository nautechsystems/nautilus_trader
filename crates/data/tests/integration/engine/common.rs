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

#[fixture]
pub(super) fn client_id() -> ClientId {
    ClientId::test_default()
}

#[fixture]
pub(super) fn venue() -> Venue {
    Venue::test_default()
}

#[fixture]
pub(super) fn clock() -> Rc<RefCell<VirtualClock>> {
    Rc::new(RefCell::new(VirtualClock::new()))
}

#[fixture]
pub(super) fn cache() -> Rc<RefCell<Cache>> {
    Rc::new(RefCell::new(Cache::default()))
}

#[fixture]
pub(super) fn stub_msgbus() -> Rc<RefCell<MessageBus>> {
    MessageBus::new(TraderId::test_default(), UUID4::new(), None, None).register_message_bus()
}

#[fixture]
pub(super) fn data_engine(
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));

    let data_engine_clone = Rc::clone(&data_engine);

    let handler = TypedIntoHandler::from(move |cmd: DataCommand| {
        data_engine_clone.borrow_mut().execute(cmd);
    });

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::register_data_command_endpoint(endpoint, handler);

    data_engine
}

#[fixture]
pub(super) fn data_client(
    client_id: ClientId,
    venue: Venue,
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<VirtualClock>>,
) -> DataClientAdapter {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    DataClientAdapter::new(client_id, Some(venue), true, true, client)
}

pub(super) fn dispatch_data(data_engine: &mut DataEngine, data: Data, borrowed: bool) {
    let count = data_engine.data_count();

    if borrowed {
        data_engine.process_data_ref(DataRef::from(&data));
    } else {
        data_engine.process_data(data);
    }

    assert_eq!(data_engine.data_count(), count + 1);
}

// Registers a mock data client for tests
pub(super) fn register_mock_client(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
    routing: Option<Venue>,
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
    data_engine: &mut DataEngine,
) {
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(recorder)),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.register_client(adapter, routing);
}

pub(super) fn register_failing_subscribe_client(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
    failure: MockSubscribeFailure,
    data_engine: &mut DataEngine,
) {
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(recorder)),
    )
    .with_subscribe_failure(failure);
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.register_client(adapter, None);
}

pub(super) struct FailingRequestDataClient {
    client_id: ClientId,
    venue: Option<Venue>,
    error_message: String,
}

impl FailingRequestDataClient {
    pub(super) fn new(
        client_id: ClientId,
        venue: Option<Venue>,
        error_message: impl Into<String>,
    ) -> Self {
        Self {
            client_id,
            venue,
            error_message: error_message.into(),
        }
    }
}

impl DataClient for FailingRequestDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        self.venue
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn is_disconnected(&self) -> bool {
        false
    }

    fn request_quotes(&self, _request: RequestQuotes) -> anyhow::Result<()> {
        anyhow::bail!("{}", self.error_message)
    }

    fn request_trades(&self, _request: RequestTrades) -> anyhow::Result<()> {
        anyhow::bail!("{}", self.error_message)
    }

    fn request_bars(&self, _request: RequestBars) -> anyhow::Result<()> {
        anyhow::bail!("{}", self.error_message)
    }

    fn request_book_deltas(&self, _request: RequestBookDeltas) -> anyhow::Result<()> {
        anyhow::bail!("{}", self.error_message)
    }

    fn request_option_chain_reference_price(
        &self,
        _request: RequestOptionChainReferencePrice,
    ) -> anyhow::Result<()> {
        anyhow::bail!("{}", self.error_message)
    }
}

pub(super) fn parent_params() -> Params {
    let mut params = Params::new();
    params.insert(PARAMS_IS_PARENT.to_string(), json!(true));
    params
}

pub(super) fn client_subscription_params(params: Params) -> Params {
    #[cfg(feature = "streaming")]
    {
        let mut params = params;
        params.insert("start_ns".to_string(), Value::Null);
        params
    }

    #[cfg(not(feature = "streaming"))]
    params
}

#[cfg(feature = "streaming")]
pub(super) struct CatalogTempDir(PathBuf);

#[cfg(feature = "streaming")]
impl CatalogTempDir {
    pub(super) fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nautilus-data-engine-{label}-{}", UUID4::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(feature = "streaming")]
impl Drop for CatalogTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "streaming")]
pub(super) fn write_custom_catalog_file(
    catalog_dir: &CatalogTempDir,
    catalog: &ParquetDataCatalog,
    type_name: &str,
    identifier: Option<&str>,
    start_timestamp: u64,
    end_timestamp: u64,
) {
    let directory = catalog
        .make_path_custom_data(type_name, identifier)
        .unwrap();
    let directory_path = catalog_dir.path().join(directory);
    std::fs::create_dir_all(&directory_path).unwrap();

    let filename = timestamps_to_filename(
        UnixNanos::from(start_timestamp),
        UnixNanos::from(end_timestamp),
    );
    std::fs::write(directory_path.join(filename), b"").unwrap();
}

#[cfg(feature = "streaming")]
pub(super) fn recorded_subscribe_command(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> SubscribeCommand {
    let recorded = recorder.borrow();

    let DataCommand::Subscribe(command) = &recorded[0] else {
        panic!("expected subscribe command");
    };

    command.clone()
}

pub(super) fn make_es_future(instrument_id: &str, symbol: &str) -> FuturesContract {
    FuturesContract::builder()
        .instrument_id(InstrumentId::from(instrument_id))
        .raw_symbol(Symbol::from(symbol))
        .asset_class(AssetClass::Index)
        .exchange(Ustr::from("XCME"))
        .underlying(Ustr::from("ES"))
        .activation_ns(UnixNanos::default())
        .expiration_ns(UnixNanos::from(2_000_000_000_000_000_000u64))
        .currency(Currency::USD())
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

pub(super) fn add_es_contract(
    cache: &Rc<RefCell<Cache>>,
    instrument_id: &str,
    symbol: &str,
) -> InstrumentId {
    let instrument = make_es_future(instrument_id, symbol);
    let instrument_id = instrument.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(instrument))
        .unwrap();
    instrument_id
}

pub(super) fn params_from_json(value: Value) -> Params {
    serde_json::from_value(value).unwrap()
}

pub(super) fn make_bar(
    bar_type: BarType,
    open: &str,
    high: &str,
    low: &str,
    close: &str,
    volume: u64,
    ts: u64,
) -> Bar {
    Bar::new(
        bar_type,
        Price::from(open),
        Price::from(high),
        Price::from(low),
        Price::from(close),
        Quantity::from(volume),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

pub(super) fn make_trade(
    instrument_id: InstrumentId,
    price: &str,
    size: u64,
    trade_id: &str,
    ts: u64,
) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from(price),
        Quantity::from(size),
        AggressorSide::Buy,
        TradeId::new(trade_id),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

pub(super) fn make_quote(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

pub(super) fn response_data_count(response: &BarsResponse) -> Option<u64> {
    response
        .params
        .as_ref()
        .and_then(|params| params.get("data_count"))
        .and_then(Value::as_u64)
}

pub(super) fn data_engine_clock_at(now: u64) -> Rc<RefCell<dyn Clock>> {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    clock
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<VirtualClock>()
        .unwrap()
        .advance_time(UnixNanos::from(now), true);
    clock
}

pub(super) fn execute_book_snapshot_subscribe(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    interval_ms: NonZeroUsize,
) {
    let subscribe = SubscribeBookSnapshots::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        interval_ms,
        None,
        None,
    );

    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
            subscribe,
        )));
}

pub(super) fn execute_book_snapshot_unsubscribe(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    interval_ms: NonZeroUsize,
) {
    let unsubscribe = UnsubscribeBookSnapshots::new(
        instrument_id,
        interval_ms,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    data_engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookSnapshots(
            unsubscribe,
        )));
}

pub(super) fn process_book_delta(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
) {
    let delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
    let deltas = Box::new(OrderBookDeltas::new(instrument_id, vec![delta]));
    data_engine
        .borrow_mut()
        .process_data(Data::BookDeltas(deltas));
}

pub(super) fn advance_clock_and_dispatch(clock: &Rc<RefCell<VirtualClock>>, advance_ns: u64) {
    let to_time_ns = clock.borrow().timestamp_ns().as_u64() + advance_ns;
    let events = clock.borrow_mut().advance_time(to_time_ns.into(), true);
    let handlers = clock.borrow().match_handlers(events);

    for handler in handlers {
        handler.callback.call(handler.event);
    }
}

pub(super) fn create_snapshot_test_engine(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let _ =
        MessageBus::new(TraderId::test_default(), UUID4::new(), None, None).register_message_bus();

    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));
    let data_engine_clone = Rc::clone(&data_engine);

    let handler = TypedIntoHandler::from(move |cmd: DataCommand| {
        data_engine_clone.borrow_mut().execute(cmd);
    });

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::register_data_command_endpoint(endpoint, handler);

    data_engine
}

pub(super) fn make_crypto_option(
    symbol: &str,
    underlying_str: &str,
    settlement_str: &str,
    strike: &str,
    kind: OptionKind,
    expiration_ns: UnixNanos,
) -> InstrumentAny {
    use nautilus_model::{
        identifiers::Symbol,
        instruments::CryptoOption,
        types::{Currency, Money, Quantity},
    };

    let instrument_id = InstrumentId::from(symbol);
    let raw_symbol = Symbol::from(symbol.split('.').next().unwrap_or(symbol));
    let underlying = Currency::from(underlying_str);
    let quote = Currency::USD();
    let settlement = Currency::from(settlement_str);
    let activation = UnixNanos::from(1_671_696_000_000_000_000u64);

    InstrumentAny::CryptoOption(
        CryptoOption::builder()
            .instrument_id(instrument_id)
            .raw_symbol(raw_symbol)
            .underlying(underlying)
            .quote_currency(quote)
            .settlement_currency(settlement)
            .is_inverse(false)
            .option_kind(kind)
            .strike_price(Price::from(strike))
            .activation_ns(activation)
            .expiration_ns(expiration_ns)
            .price_precision(3)
            .size_precision(1)
            .price_increment(Price::from("0.001"))
            .size_increment(Quantity::from("0.1"))
            .multiplier(Quantity::from(1))
            .lot_size(Quantity::from(1))
            .max_quantity(Quantity::from("9000.0"))
            .min_quantity(Quantity::from("0.1"))
            .min_notional(Money::new(10.00, Currency::USD()))
            .ts_event(0.into())
            .ts_init(0.into())
            .build()
            .unwrap(),
    )
}

/// Creates a data engine that shares the provided cache and clock.
pub(super) fn make_option_chain_engine(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));
    DataEngine::register_msgbus_handlers(&data_engine);

    data_engine
}

pub(super) fn synthetic_index() -> (SyntheticInstrument, InstrumentId, InstrumentId) {
    let component_a = InstrumentId::from("BTC-USD.SIM");
    let component_b = InstrumentId::from("ETH-USD.SIM");
    let synthetic = synthetic_index_with_components("BTC-ETH-INDEX", component_a, component_b);

    (synthetic, component_a, component_b)
}

pub(super) fn synthetic_index_with_components(
    symbol: &str,
    component_a: InstrumentId,
    component_b: InstrumentId,
) -> SyntheticInstrument {
    let formula = format!("({component_a} + {component_b}) / 2.0");
    SyntheticInstrument::builder()
        .symbol(Symbol::new(symbol))
        .price_precision(2)
        .components(vec![component_a, component_b])
        .formula(&formula)
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

pub(super) fn subscribe_synthetic_quotes_cmd(instrument_id: InstrumentId) -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        instrument_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

pub(super) fn subscribe_synthetic_trades_cmd(instrument_id: InstrumentId) -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::Trades(SubscribeTrades::new(
        instrument_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

pub(super) fn quote_tick(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

pub(super) fn trade_tick(
    instrument_id: InstrumentId,
    price: &str,
    trade_id: &str,
    ts: u64,
) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from(price),
        Quantity::from(1),
        AggressorSide::Buy,
        TradeId::new(trade_id),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

pub(super) fn book_depth_at(instrument_id: InstrumentId, ts: u64) -> OrderBookDepth {
    let mut depth = stub_depth10();
    depth.instrument_id = instrument_id;
    depth.ts_event = UnixNanos::from(ts);
    depth.ts_init = UnixNanos::from(ts);
    depth
}

pub(super) fn pipeline_quote(instrument_id: InstrumentId, ts: u64) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from("1.00000"),
        Price::from("1.00010"),
        Quantity::from("1"),
        Quantity::from("1"),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

pub(super) fn leg_quotes_response(
    request_id: UUID4,
    instrument_id: InstrumentId,
    client_id: ClientId,
    quotes: Vec<QuoteTick>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> DataResponse {
    DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        quotes,
        start,
        end,
        UnixNanos::default(),
        None,
    ))
}

pub(super) fn time_range_quote_response(
    request: &RequestQuotes,
    instrument_id: InstrumentId,
    client_id: ClientId,
    data_count: u64,
    quotes: Vec<QuoteTick>,
) -> DataResponse {
    DataResponse::Quotes(QuotesResponse::new(
        request.request_id,
        client_id,
        instrument_id,
        quotes,
        request.start.map(datetime_to_unix_nanos_for_test),
        request.end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        Some(time_range_data_count_params(data_count)),
    ))
}

pub(super) fn time_range_data_count_params(data_count: u64) -> Params {
    serde_json::from_value(json!({"data_count": data_count})).unwrap()
}

pub(super) fn datetime_to_unix_nanos_for_test(dt: jiff::Timestamp) -> UnixNanos {
    UnixNanos::from(u64::try_from(dt.as_nanosecond().max(0)).unwrap_or(0))
}

pub(super) fn advance_test_clock_to(clock: &Rc<RefCell<dyn Clock>>, ns: u64) {
    clock
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<VirtualClock>()
        .unwrap()
        .advance_time(UnixNanos::from(ns), true);
}

pub(super) fn register_time_range_recorder(
    data_engine: &mut DataEngine,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) -> Rc<RefCell<Vec<DataCommand>>> {
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
    recorder
}

#[cfg(feature = "streaming")]
pub(super) fn register_quote_catalog_with_quotes(
    data_engine: &mut DataEngine,
    label: &str,
    quotes: &[QuoteTick],
    interval: Option<(u64, u64)>,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);

    let (start, end) = match interval {
        Some((s, e)) => (Some(UnixNanos::from(s)), Some(UnixNanos::from(e))),
        None => (None, None),
    };

    catalog.write_to_parquet(quotes, start, end, None).unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
pub(super) fn register_trade_catalog_with_trades(
    data_engine: &mut DataEngine,
    label: &str,
    trades: &[TradeTick],
    interval: Option<(u64, u64)>,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);

    let (start, end) = match interval {
        Some((s, e)) => (Some(UnixNanos::from(s)), Some(UnixNanos::from(e))),
        None => (None, None),
    };

    catalog.write_to_parquet(trades, start, end, None).unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

#[cfg(feature = "streaming")]
pub(super) fn advance_clock_to(clock: &Rc<RefCell<dyn Clock>>, ns: u64) {
    clock
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<VirtualClock>()
        .unwrap()
        .advance_time(UnixNanos::from(ns), true);
}

#[cfg(feature = "streaming")]
pub(super) fn split_quote(instrument_id: InstrumentId, ts: u64) -> QuoteTick {
    make_quote(instrument_id, "1.0000", "1.0001", ts)
}

#[cfg(feature = "streaming")]
pub(super) fn split_trade(instrument_id: InstrumentId, ts: u64, trade_id: &str) -> TradeTick {
    make_trade(instrument_id, "1.0000", 1, trade_id, ts)
}

#[cfg(feature = "streaming")]
pub(super) fn ensure_engine_custom_data_registered() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        ensure_custom_data_registered::<RustTestCustomData>();
    });
}

pub(super) fn split_delta(instrument_id: InstrumentId, ts: u64) -> OrderBookDelta {
    OrderBookDeltaTestBuilder::new(instrument_id)
        .ts_event(UnixNanos::from(ts))
        .ts_init(UnixNanos::from(ts))
        .build()
}

#[cfg(feature = "streaming")]
pub(super) fn register_deltas_catalog_with_deltas(
    data_engine: &mut DataEngine,
    label: &str,
    deltas: &[OrderBookDelta],
    interval: Option<(u64, u64)>,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);

    let (start, end) = match interval {
        Some((s, e)) => (Some(UnixNanos::from(s)), Some(UnixNanos::from(e))),
        None => (None, None),
    };

    catalog.write_to_parquet(deltas, start, end, None).unwrap();
    data_engine.register_catalog(Box::new(catalog), None);
    catalog_dir
}

pub(super) fn delta_with_flag(instrument_id: InstrumentId, ts: u64, flags: u8) -> OrderBookDelta {
    OrderBookDeltaTestBuilder::new(instrument_id)
        .flags(flags)
        .ts_event(UnixNanos::from(ts))
        .ts_init(UnixNanos::from(ts))
        .build()
}

#[fixture]
pub(super) fn managed_book_engine(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) -> Rc<RefCell<DataEngine>> {
    let engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut engine.borrow_mut(),
    );
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    engine
}
