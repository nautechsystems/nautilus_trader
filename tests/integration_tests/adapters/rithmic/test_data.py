"""Tests for Rithmic data client."""

from datetime import datetime, timedelta, timezone
from types import SimpleNamespace

import pytest

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig, RithmicEnvironment
from nautilus_trader.adapters.rithmic.data import RithmicLiveDataClient, RITHMIC_VENUE
from nautilus_trader.model.data import BarAggregation, BarSpecification, BarType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId


class _FakeCache:
    def __init__(self, instruments):
        self._instruments = instruments
        self.added_currencies = []

    def instrument(self, instrument_id):
        return self._instruments.get(instrument_id)

    def add_currency(self, currency):
        self.added_currencies.append(currency)


class _FakeProvider:
    def __init__(self, instruments, load_callback=None):
        self._instruments = instruments
        self._load_callback = load_callback
        self.load_calls = []
        self.load_all_calls = []
        self.initialize_calls = []

    def find(self, instrument_id):
        return self._instruments.get(instrument_id)

    def get_all(self):
        return self._instruments.copy()

    async def load_async(self, instrument_id, filters=None):
        self.load_calls.append((instrument_id, filters))
        if self._load_callback is not None:
            self._load_callback(instrument_id, filters)

    async def load_all_async(self, filters=None):
        self.load_all_calls.append(filters)

    async def initialize(self, reload=False):
        self.initialize_calls.append(reload)


class _DummyDataClient:
    _resolve_rithmic_symbol = RithmicLiveDataClient._resolve_rithmic_symbol
    _lookup_instrument = RithmicLiveDataClient._lookup_instrument
    _resolve_exchange = RithmicLiveDataClient._resolve_exchange
    _publish_instrument_to_data_engine = RithmicLiveDataClient._publish_instrument_to_data_engine
    _send_all_instruments_to_data_engine = RithmicLiveDataClient._send_all_instruments_to_data_engine
    _ensure_instrument_loaded = RithmicLiveDataClient._ensure_instrument_loaded
    _warn_unsupported = RithmicLiveDataClient._warn_unsupported
    _bar_subscription_key = RithmicLiveDataClient._bar_subscription_key
    _subscribe_bars = RithmicLiveDataClient._subscribe_bars
    _unsubscribe_bars = RithmicLiveDataClient._unsubscribe_bars
    _request_quote_ticks = RithmicLiveDataClient._request_quote_ticks
    _resolve_bar_type_name = RithmicLiveDataClient._resolve_bar_type_name
    _convert_time_bars = RithmicLiveDataClient._convert_time_bars
    _bar_timestamp_from_response = RithmicLiveDataClient._bar_timestamp_from_response
    _fallback_bar_timestamp = RithmicLiveDataClient._fallback_bar_timestamp
    _handle_quote_tick = RithmicLiveDataClient._handle_quote_tick
    _handle_trade_tick = RithmicLiveDataClient._handle_trade_tick
    _handle_live_bar = RithmicLiveDataClient._handle_live_bar
    _request_bars = RithmicLiveDataClient._request_bars

    def __init__(self, cache=None, instrument_provider=None, config=None):
        self._cache = cache or _FakeCache({})
        self._instrument_provider = instrument_provider or _FakeProvider({})
        self.handled = []
        self._log = _FakeLog()
        self._config = config or SimpleNamespace(enable_history=True)
        self._rust_client = None
        self._bar_subscriptions = {}

    def _handle_data(self, data):
        self.handled.append(data)

    def _handle_bars(self, bar_type, bars, *_args, **_kwargs):
        self.handled.append(("bars", bar_type, bars))


class _FakeLog:
    def __init__(self):
        self.warnings = []

    def info(self, message):
        return None

    def error(self, message):
        return None

    def warning(self, message):
        self.warnings.append(message)


def _make_instrument(value: str, exchange: str = "CME", currency_code: str = "USD"):
    return SimpleNamespace(
        id=InstrumentId.from_str(value),
        exchange=exchange,
        info={"exchange": exchange},
        currency=SimpleNamespace(code=currency_code),
        price_precision=2,
        size_precision=0,
    )


def _make_bar_type(aggregation: BarAggregation = BarAggregation.MINUTE, step: int = 1):
    spec = BarSpecification(step, aggregation, PriceType.LAST)
    return BarType(InstrumentId.from_str("ESZ4.RITHMIC"), spec)


# Note: These tests require mocking the NautilusTrader infrastructure
# Full integration tests would require a running NautilusTrader instance

class TestRithmicLiveDataClient:
    """Tests for RithmicLiveDataClient."""

    def test_venue(self):
        # Verify venue constant
        assert RITHMIC_VENUE.value == "RITHMIC"

    def test_resolve_rithmic_symbol_strips_exchange_suffix(self):
        client = _DummyDataClient()
        instrument_id = InstrumentId.from_str("ESZ4.CME.RITHMIC")

        assert client._resolve_rithmic_symbol(instrument_id) == "ESZ4"

    def test_resolve_exchange_from_symbol_suffix(self):
        client = _DummyDataClient()

        exchange = client._resolve_exchange(InstrumentId.from_str("ESZ4.CME.RITHMIC"))
        assert exchange == "CME"

    def test_lookup_instrument_uses_normalized_id(self):
        instrument = object()
        client = _DummyDataClient(
            cache=_FakeCache({InstrumentId.from_str("ESZ4.RITHMIC"): instrument}),
        )

        found = client._lookup_instrument(InstrumentId.from_str("ESZ4.CME.RITHMIC"))
        assert found is instrument

    def test_publish_instrument_to_data_engine_adds_currency(self):
        instrument = _make_instrument("ESZ4.RITHMIC")
        cache = _FakeCache({})
        client = _DummyDataClient(cache=cache)

        client._publish_instrument_to_data_engine(instrument)

        assert client.handled == [instrument]
        assert cache.added_currencies == [instrument.currency]

    def test_send_all_instruments_to_data_engine_publishes_provider_snapshot(self):
        instrument = _make_instrument("ESZ4.RITHMIC")
        client = _DummyDataClient(
            instrument_provider=_FakeProvider({instrument.id: instrument}),
        )

        client._send_all_instruments_to_data_engine()

        assert client.handled == [instrument]

    @pytest.mark.asyncio
    async def test_ensure_instrument_loaded_publishes_loaded_instrument(self):
        instrument = _make_instrument("ESZ4.RITHMIC")
        normalized_id = instrument.id

        def load_callback(_instrument_id, _filters):
            provider._instruments[normalized_id] = instrument

        provider = _FakeProvider({}, load_callback=load_callback)
        client = _DummyDataClient(
            cache=_FakeCache({}),
            instrument_provider=provider,
        )

        await client._ensure_instrument_loaded(
            InstrumentId.from_str("ESZ4.CME.RITHMIC"),
            "CME",
        )

        assert provider.load_calls == [
            (
                InstrumentId.from_str("ESZ4.CME.RITHMIC"),
                {"exchange": "CME"},
            )
        ]
        assert client.handled == [instrument]

    @pytest.mark.asyncio
    async def test_subscribe_bars_requires_history_enabled(self):
        client = _DummyDataClient(config=SimpleNamespace(enable_history=False))
        client._rust_client = object()
        command = SimpleNamespace(
            bar_type=BarType.from_str("ESZ4.RITHMIC-1-MINUTE-LAST-EXTERNAL"),
            params={"exchange": "CME"},
        )

        with pytest.raises(RuntimeError, match="history is disabled"):
            await client._subscribe_bars(command)

    @pytest.mark.asyncio
    async def test_subscribe_bars_registers_live_subscription(self):
        instrument = _make_instrument("ESZ4.RITHMIC")

        class _FakeRustClient:
            def __init__(self):
                self.calls = []

            async def subscribe_bars(self, symbol, exchange, bar_type_name, bar_period):
                self.calls.append(("subscribe", symbol, exchange, bar_type_name, bar_period))

            async def unsubscribe_bars(self, symbol, exchange, bar_type_name, bar_period):
                self.calls.append(("unsubscribe", symbol, exchange, bar_type_name, bar_period))

        client = _DummyDataClient(cache=_FakeCache({instrument.id: instrument}))
        client._rust_client = _FakeRustClient()
        command = SimpleNamespace(
            bar_type=BarType.from_str("ESZ4.RITHMIC-1-MINUTE-LAST-EXTERNAL"),
            params={"exchange": "CME"},
        )

        await client._subscribe_bars(command)

        assert client._rust_client.calls == [("subscribe", "ESZ4", "CME", "MinuteBar", 1)]
        assert client._bar_subscriptions == {
            ("ESZ4", "CME", "MinuteBar", 1): command.bar_type,
        }

        await client._unsubscribe_bars(command)

        assert client._rust_client.calls[-1] == ("unsubscribe", "ESZ4", "CME", "MinuteBar", 1)
        assert client._bar_subscriptions == {}

    @pytest.mark.asyncio
    async def test_request_quote_ticks_warns_when_unsupported(self):
        client = _DummyDataClient()

        await client._request_quote_ticks(object())

        assert client._log.warnings == ["Historical quote tick requests not implemented for Rithmic"]

    @pytest.mark.parametrize(
        ("aggregation", "expected"),
        [
            (BarAggregation.SECOND, "SecondBar"),
            (BarAggregation.MINUTE, "MinuteBar"),
            (BarAggregation.DAY, "DailyBar"),
            (BarAggregation.WEEK, "WeeklyBar"),
        ],
    )
    def test_resolve_bar_type_name_supported_aggregations(self, aggregation, expected):
        client = _DummyDataClient()
        bar_type = _make_bar_type(aggregation)

        assert client._resolve_bar_type_name(bar_type) == expected

    def test_resolve_bar_type_name_unsupported_aggregation(self):
        client = _DummyDataClient()
        bar_type = _make_bar_type(BarAggregation.HOUR)

        with pytest.raises(ValueError):
            client._resolve_bar_type_name(bar_type)

    def test_convert_time_bars_corrects_invalid_ohlc(self):
        instrument = _make_instrument("ESZ4.RITHMIC")
        cache = _FakeCache({instrument.id: instrument})
        client = _DummyDataClient(cache=cache)
        bar_type = _make_bar_type(BarAggregation.MINUTE)
        response = SimpleNamespace(
            open_price=100.0,
            high_price=90.0,
            low_price=110.0,
            close_price=95.0,
            volume=10,
            period="42",
        )

        bars = client._convert_time_bars(bar_type, [response], instrument.id)

        assert len(bars) == 1
        bar = bars[0]
        assert float(bar.open) == 100.0
        assert float(bar.high) == 110.0
        assert float(bar.low) == 90.0
        assert float(bar.close) == 95.0
        assert float(bar.volume) == 10.0
        assert bar.ts_event == 42 * 1_000_000_000
        assert client._log.warnings == [
            f"Corrected invalid OHLC data for {instrument.id} at {42 * 1_000_000_000}"
        ]

    def test_bar_timestamp_from_response_parses_seconds(self):
        client = _DummyDataClient()
        bar_type = _make_bar_type(BarAggregation.SECOND, step=5)
        response = SimpleNamespace(period="7")

        ts = client._bar_timestamp_from_response(response, bar_type)

        assert ts == 7 * 1_000_000_000

    def test_bar_timestamp_from_response_prefers_marker(self):
        client = _DummyDataClient()
        bar_type = _make_bar_type(BarAggregation.MINUTE, step=1)
        response = SimpleNamespace(marker=1_700_000_000, period="1")

        ts = client._bar_timestamp_from_response(response, bar_type)

        assert ts == 1_700_000_000 * 1_000_000_000

    def test_bar_timestamp_from_response_falls_back_on_parse_error(self):
        client = _DummyDataClient()
        bar_type = _make_bar_type(BarAggregation.SECOND, step=5)
        response = SimpleNamespace(period="not-a-number")

        ts = client._bar_timestamp_from_response(response, bar_type)

        assert ts == 5 * 1_000_000_000
        assert client._log.warnings == ["Could not parse bar period 'not-a-number' as timestamp"]

    def test_handle_quote_tick_uses_default_precision_without_instrument(self):
        client = _DummyDataClient()
        tick = SimpleNamespace(
            symbol="ESZ4",
            bid_price=4300.25,
            ask_price=4300.75,
            bid_size=1,
            ask_size=2,
            ts_event=123,
            ts_init=123,
        )

        client._handle_quote_tick(tick)

        published = client.handled[0]
        assert published.instrument_id == InstrumentId.from_str("ESZ4.RITHMIC")
        assert float(published.bid_price) == 4300.25
        assert float(published.ask_price) == 4300.75
        assert float(published.bid_size) == 1.0
        assert float(published.ask_size) == 2.0
        assert client._log.warnings == [
            "No instrument found for ESZ4.RITHMIC, using default precision"
        ]

    def test_handle_trade_tick_maps_aggressor_and_precisions(self):
        instrument = _make_instrument("ESZ4.RITHMIC")
        cache = _FakeCache({instrument.id: instrument})
        client = _DummyDataClient(cache=cache)
        tick = SimpleNamespace(
            symbol="ESZ4",
            price=4300.25,
            size=3,
            aggressor_side="SELL",
            trade_id="t1",
            ts_event=456,
            ts_init=456,
        )

        client._handle_trade_tick(tick)

        published = client.handled[0]
        assert published.instrument_id == instrument.id
        assert float(published.price) == 4300.25
        assert float(published.size) == 3.0
        assert published.aggressor_side.name == "SELLER"
        assert client._log.warnings == []

    def test_handle_live_bar_publishes_matching_bar(self):
        instrument = _make_instrument("ESZ4.RITHMIC")
        cache = _FakeCache({instrument.id: instrument})
        client = _DummyDataClient(cache=cache)
        bar_type = BarType.from_str("ESZ4.RITHMIC-1-MINUTE-LAST-EXTERNAL")
        client._bar_subscriptions[("ESZ4", "CME", "MinuteBar", 1)] = bar_type

        client._handle_live_bar(
            SimpleNamespace(
                symbol="ESZ4",
                exchange="CME",
                bar_kind="MinuteBar",
                bar_period=1,
                open_price=4300.0,
                high_price=4301.0,
                low_price=4299.5,
                close_price=4300.5,
                volume=12,
                ts_event=1_700_000_000_000_000_000,
                ts_init=1_700_000_000_100_000_000,
            )
        )

        published = client.handled[0]
        assert published.bar_type == bar_type
        assert float(published.open) == 4300.0
        assert float(published.high) == 4301.0
        assert float(published.low) == 4299.5
        assert float(published.close) == 4300.5
        assert float(published.volume) == 12.0
        assert published.ts_event == 1_700_000_000_000_000_000
        assert published.ts_init == 1_700_000_000_100_000_000

    @pytest.mark.asyncio
    async def test_request_bars_requires_start_before_end(self):
        client = _DummyDataClient()
        client._rust_client = object()
        bar_type = _make_bar_type()
        request = SimpleNamespace(
            bar_type=bar_type,
            start=datetime(2024, 1, 1, tzinfo=timezone.utc),
            end=datetime(2024, 1, 1, tzinfo=timezone.utc),
            params={"exchange": "CME"},
            id="req-1",
        )

        with pytest.raises(ValueError, match="Start must be earlier than end"):
            await client._request_bars(request)

    @pytest.mark.asyncio
    async def test_request_bars_requires_history_enabled(self):
        client = _DummyDataClient(config=SimpleNamespace(enable_history=False))
        request = SimpleNamespace(
            bar_type=_make_bar_type(),
            start=datetime(2024, 1, 1, tzinfo=timezone.utc),
            end=datetime(2024, 1, 1, tzinfo=timezone.utc) + timedelta(minutes=1),
            params={"exchange": "CME"},
            id="req-history-disabled",
        )

        with pytest.raises(RuntimeError, match="history is disabled"):
            await client._request_bars(request)

    @pytest.mark.asyncio
    async def test_request_bars_converts_responses(self):
        instrument = _make_instrument("ESZ4.RITHMIC")

        def load_callback(_instrument_id, _filters):
            provider._instruments[instrument.id] = instrument

        provider = _FakeProvider({}, load_callback=load_callback)

        class _FakeRustClient:
            def __init__(self, responses):
                self._responses = responses
                self.calls = []

            async def request_bars(self, symbol, exchange, bar_type_name, bar_period, start_sec, end_sec):
                self.calls.append((symbol, exchange, bar_type_name, bar_period, start_sec, end_sec))
                return self._responses

        responses = [
            SimpleNamespace(
                open_price=10.0,
                high_price=11.0,
                low_price=9.5,
                close_price=10.5,
                volume=7,
                period="100",
            )
        ]

        client = _DummyDataClient(cache=_FakeCache({}), instrument_provider=provider)
        client._rust_client = _FakeRustClient(responses)

        bar_type = _make_bar_type(BarAggregation.MINUTE, step=1)
        request = SimpleNamespace(
            bar_type=bar_type,
            start=datetime(2024, 1, 1, tzinfo=timezone.utc),
            end=datetime(2024, 1, 1, tzinfo=timezone.utc) + timedelta(minutes=5),
            params={"exchange": "CME"},
            id="req-2",
            start_time=None,
            end_time=None,
        )

        await client._request_bars(request)

        # Verify the Rust client was invoked with resolved symbol/exchange and period
        assert client._rust_client.calls == [
            ("ESZ4", "CME", "MinuteBar", 1, 1704067200, 1704067500),
        ]

        # Verify bars were converted and published
        assert len(client.handled) == 2  # instrument published then bars
        _, published_bar_type, bars = client.handled[-1]
        assert published_bar_type == bar_type
        assert len(bars) == 1
        bar = bars[0]
        assert float(bar.open) == 10.0
        assert float(bar.high) == 11.0
        assert float(bar.low) == 9.5
        assert float(bar.close) == 10.5
        assert float(bar.volume) == 7.0
        assert bar.ts_event == 100 * 1_000_000_000

    # Integration tests would look like:
    #
    # @pytest.fixture
    # def data_client(self, event_loop, msgbus, cache, clock):
    #     config = RithmicDataClientConfig(
    #         environment=RithmicEnvironment.DEMO,
    #         username="test_user",
    #         password="test_pass",
    #         system_name="test_system",
    #     )
    #     return RithmicLiveDataClient(
    #         loop=event_loop,
    #         client_id="RITHMIC-DATA",
    #         msgbus=msgbus,
    #         cache=cache,
    #         clock=clock,
    #         config=config,
    #     )
    #
    # @pytest.mark.asyncio
    # async def test_connect(self, data_client):
    #     await data_client._connect()
    #     # Verify connection state
    #
    # @pytest.mark.asyncio
    # async def test_subscribe_quotes(self, data_client):
    #     await data_client._connect()
    #     instrument_id = InstrumentId.from_str("ESZ4.RITHMIC")
    #     await data_client._subscribe_quote_ticks(instrument_id)
    #     # Verify subscription
