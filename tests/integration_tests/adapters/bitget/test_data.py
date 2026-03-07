# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import asyncio
from types import SimpleNamespace
from unittest.mock import patch

from nautilus_trader.adapters.bitget.constants import BITGET_DEFAULT_PRODUCTS
from nautilus_trader.adapters.bitget.data import BitgetDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId


def test_bitget_data_client_is_market_data_client() -> None:
    assert issubclass(BitgetDataClient, LiveMarketDataClient)


def test_handle_ws_reconnect_schedules_work_on_event_loop_thread() -> None:
    calls: list[object] = []

    class DummyLoop:
        def call_soon_threadsafe(self, callback):
            calls.append(callback)

    dummy = SimpleNamespace(
        _loop=DummyLoop(),
        _on_ws_reconnect=lambda: None,
    )

    BitgetDataClient._handle_ws_reconnect(dummy)  # type: ignore[arg-type]

    assert calls == [dummy._on_ws_reconnect]


def test_on_ws_reconnect_recovers_books_before_resubscribing() -> None:
    created: list[tuple[str, object]] = []
    instrument_id = SimpleNamespace(value="BTCUSDT-SPOT")

    async def recover_order_book(_instrument_id) -> None:
        return None

    async def subscribe_trade_ticks(_instrument_id) -> None:
        return None

    def warning(*_args, **_kwargs) -> None:
        return None

    def create_task(coro, log_msg):
        created.append((log_msg, coro))
        coro.close()
        return object()

    dummy = SimpleNamespace(
        _log=SimpleNamespace(warning=warning),
        _book_states={("SPOT", "BTCUSDT"): object()},
        _active_trade_subs=[instrument_id],
        _active_book_subs=[instrument_id],
        _ticker_instruments={},
        _bar_instruments={},
        _subscribe_trade_ticks_by_id=subscribe_trade_ticks,
        _recover_order_book=recover_order_book,
        create_task=create_task,
        _ws_tasks=set(),
    )

    BitgetDataClient._on_ws_reconnect(dummy)  # type: ignore[arg-type]

    assert dummy._book_states == {}
    assert [log_msg for log_msg, _ in created] == [
        "bitget:resubscribe_trade:BTCUSDT-SPOT",
        "bitget:recover_book:BTCUSDT-SPOT",
    ]


def test_on_ws_reconnect_resubscribes_ticker_and_bar_channels() -> None:
    created: list[str] = []
    instrument_id = SimpleNamespace(value="BTCUSDT-PERP.BITGET")

    async def resubscribe_ticker(_instrument_id) -> None:
        return None

    async def resubscribe_bar(_instrument_id, _channel) -> None:
        return None

    def create_task(coro, log_msg):
        created.append(log_msg)
        coro.close()
        return object()

    dummy = SimpleNamespace(
        _log=SimpleNamespace(warning=lambda *_args, **_kwargs: None),
        _book_states={},
        _active_trade_subs=[],
        _active_book_subs=[],
        _ticker_instruments={instrument_id.value: instrument_id},
        _bar_instruments={(instrument_id.value, "candle1m"): instrument_id},
        _resubscribe_ticker=resubscribe_ticker,
        _resubscribe_bar=resubscribe_bar,
        create_task=create_task,
        _ws_tasks=set(),
    )

    BitgetDataClient._on_ws_reconnect(dummy)  # type: ignore[arg-type]

    assert created == [
        "bitget:resubscribe_ticker:BTCUSDT-PERP.BITGET",
        "bitget:resubscribe_bar:BTCUSDT-PERP.BITGET:candle1m",
    ]


def test_request_order_book_snapshot_fetches_and_emits_data() -> None:
    emitted: list[object] = []
    request_calls: list[tuple[str, object, object]] = []
    instrument_id = SimpleNamespace(value="BTCUSDT-SPOT")
    instrument = SimpleNamespace(raw_symbol=SimpleNamespace(value="BTCUSDT"))

    class DummyHttpClient:
        async def request_order_book_snapshot(self, symbol, product_type, actual_instrument):
            request_calls.append((symbol, product_type, actual_instrument))
            return "capsule"

    class DummyProvider:
        def find(self, actual_instrument_id):
            assert actual_instrument_id is instrument_id
            return instrument

    dummy = SimpleNamespace(
        _instrument_provider=DummyProvider(),
        _instrument_product_type=lambda actual_instrument: "SPOT",
        _http_client=DummyHttpClient(),
        _handle_data=emitted.append,
        _product_types={"SPOT"},
        _log=SimpleNamespace(warning=lambda *_args, **_kwargs: None),
    )
    request = SimpleNamespace(instrument_id=instrument_id)

    with patch("nautilus_trader.adapters.bitget.data.capsule_to_data", lambda capsule: f"decoded:{capsule}"):
        asyncio.run(BitgetDataClient._request_order_book_snapshot(dummy, request))  # type: ignore[arg-type]

    assert request_calls == [("BTCUSDT", "SPOT", instrument)]
    assert emitted == ["decoded:capsule"]


def test_send_all_instruments_to_data_engine_respects_product_type_filter() -> None:
    emitted: list[object] = []
    cached: list[object] = []
    spot_instrument = SimpleNamespace(id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDT")))
    futures_instrument = SimpleNamespace(id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDT-PERP")))

    class DummyProvider:
        def currencies(self):
            return {"USDT": "usdt"}

        def get_all(self):
            return {
                "spot": spot_instrument,
                "futures": futures_instrument,
            }

    dummy = SimpleNamespace(
        _cache=SimpleNamespace(add_currency=cached.append),
        _instrument_provider=DummyProvider(),
        _handle_data=emitted.append,
        _product_types={"SPOT"},
        _instrument_product_type=lambda instrument: "SPOT" if instrument is spot_instrument else "USDT-FUTURES",
    )

    BitgetDataClient._send_all_instruments_to_data_engine(dummy)  # type: ignore[arg-type]

    assert cached == ["usdt"]
    assert emitted == [spot_instrument]


def test_request_order_book_depth_delegates_to_snapshot_request() -> None:
    calls: list[object] = []
    request = SimpleNamespace(instrument_id=SimpleNamespace(value="BTCUSDT-SPOT"))

    async def request_order_book_snapshot(actual_request) -> None:
        calls.append(actual_request)

    dummy = SimpleNamespace(
        _request_order_book_snapshot=request_order_book_snapshot,
        _log=SimpleNamespace(warning=lambda *_args, **_kwargs: None),
    )

    asyncio.run(BitgetDataClient._request_order_book_depth(dummy, request))  # type: ignore[arg-type]

    assert calls == [request]


def test_request_order_book_deltas_delegates_to_snapshot_request() -> None:
    calls: list[object] = []
    request = SimpleNamespace(instrument_id=SimpleNamespace(value="BTCUSDT-SPOT"))

    async def request_order_book_snapshot(actual_request) -> None:
        calls.append(actual_request)

    dummy = SimpleNamespace(
        _request_order_book_snapshot=request_order_book_snapshot,
        _log=SimpleNamespace(warning=lambda *_args, **_kwargs: None),
    )

    asyncio.run(BitgetDataClient._request_order_book_deltas(dummy, request))  # type: ignore[arg-type]

    assert calls == [request]


def test_request_instrument_respects_product_type_filter() -> None:
    emitted: list[object] = []
    instrument_id = SimpleNamespace(value="BTCUSDT-PERP.BITGET")
    instrument = SimpleNamespace(id=instrument_id)

    class DummyProvider:
        def find(self, actual_instrument_id):
            assert actual_instrument_id is instrument_id
            return instrument

    dummy = SimpleNamespace(
        _instrument_provider=DummyProvider(),
        _handle_data=emitted.append,
        _product_types={"SPOT"},
        _instrument_product_type=lambda _instrument: "USDT-FUTURES",
    )
    request = SimpleNamespace(instrument_id=instrument_id)

    asyncio.run(BitgetDataClient._request_instrument(dummy, request))  # type: ignore[arg-type]

    assert emitted == []


def test_request_order_book_snapshot_respects_product_type_filter() -> None:
    emitted: list[object] = []
    request_calls: list[tuple[str, object, object]] = []
    instrument_id = SimpleNamespace(value="BTCUSDT-PERP.BITGET")
    instrument = SimpleNamespace(raw_symbol=SimpleNamespace(value="BTCUSDT"), id=instrument_id)

    class DummyHttpClient:
        async def request_order_book_snapshot(self, symbol, product_type, actual_instrument):
            request_calls.append((symbol, product_type, actual_instrument))
            return "capsule"

    class DummyProvider:
        def find(self, actual_instrument_id):
            assert actual_instrument_id is instrument_id
            return instrument

    dummy = SimpleNamespace(
        _instrument_provider=DummyProvider(),
        _instrument_product_type=lambda _instrument: "USDT-FUTURES",
        _http_client=DummyHttpClient(),
        _handle_data=emitted.append,
        _product_types={"SPOT"},
        _log=SimpleNamespace(warning=lambda *_args, **_kwargs: None),
    )
    request = SimpleNamespace(instrument_id=instrument_id)

    with patch("nautilus_trader.adapters.bitget.data.capsule_to_data", lambda capsule: f"decoded:{capsule}"):
        asyncio.run(BitgetDataClient._request_order_book_snapshot(dummy, request))  # type: ignore[arg-type]

    assert request_calls == []
    assert emitted == []


def test_connect_uses_bitget_websocket_config_helper() -> None:
    captured_configs: list[object] = []
    captured_helper_args: list[tuple[object, object, object]] = []

    class DummyProvider:
        async def initialize(self):
            return None

    class DummyWebSocketClient:
        @staticmethod
        async def connect(*, loop_, config, handler, post_reconnection):
            captured_configs.append(config)
            return object()

    dummy = SimpleNamespace(
        _instrument_provider=DummyProvider(),
        _rebuild_instrument_index=lambda: None,
        _send_all_instruments_to_data_engine=lambda: None,
        _config=SimpleNamespace(
            base_url_ws_public="wss://public.example",
            retry_delay_initial_ms=None,
            retry_delay_max_ms=None,
        ),
        _environment=nautilus_pyo3.BitgetEnvironment.DEMO,
        _loop=object(),
        _handle_ws_message=lambda _raw: None,
        _handle_ws_reconnect=lambda: None,
        _log=SimpleNamespace(info=lambda *_args, **_kwargs: None),
        _update_instruments_interval_mins=None,
        _update_instruments_task=None,
        _ws_client=None,
    )

    with patch(
        "nautilus_trader.adapters.bitget.data.nautilus_pyo3.WebSocketClient",
        DummyWebSocketClient,
    ), patch(
        "nautilus_trader.adapters.bitget.data.nautilus_pyo3.BitgetWebSocketClient.websocket_config",
        lambda self, base_url, retry_delay_initial_ms, retry_delay_max_ms: (
            captured_helper_args.append((base_url, retry_delay_initial_ms, retry_delay_max_ms))
            or SimpleNamespace(
                url=base_url,
                headers=[],
                heartbeat=30,
                heartbeat_msg="rust-ping",
                reconnect_timeout_ms=10_000,
                reconnect_delay_initial_ms=retry_delay_initial_ms or 2_000,
                reconnect_delay_max_ms=retry_delay_max_ms or 30_000,
            )
        ),
        create=True,
    ), patch(
        "nautilus_trader.adapters.bitget.data.nautilus_pyo3.WebSocketConfig",
        lambda **kwargs: SimpleNamespace(**kwargs),
    ):
        asyncio.run(BitgetDataClient._connect(dummy))  # type: ignore[arg-type]

    assert captured_helper_args == [("wss://public.example", None, None)]
    assert captured_configs[0].url == "wss://public.example"
    assert captured_configs[0].heartbeat_msg == "rust-ping"


def test_default_products_include_all_bitget_product_families() -> None:
    assert BITGET_DEFAULT_PRODUCTS == (
        nautilus_pyo3.BitgetProductType.SPOT,
        nautilus_pyo3.BitgetProductType.USDT_FUTURES,
        nautilus_pyo3.BitgetProductType.COIN_FUTURES,
        nautilus_pyo3.BitgetProductType.USDC_FUTURES,
    )


def test_instrument_product_type_uses_settlement_currency() -> None:
    spot = SimpleNamespace(
        id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDT")),
        quote_currency=SimpleNamespace(code="USDT"),
        settlement_currency=None,
    )
    usdt_perp = SimpleNamespace(
        id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDT-PERP")),
        base_currency=SimpleNamespace(code="BTC"),
        quote_currency=SimpleNamespace(code="USDT"),
        settlement_currency=SimpleNamespace(code="USDT"),
    )
    usdc_perp = SimpleNamespace(
        id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDC-PERP")),
        base_currency=SimpleNamespace(code="BTC"),
        quote_currency=SimpleNamespace(code="USDC"),
        settlement_currency=SimpleNamespace(code="USDC"),
    )
    coin_perp = SimpleNamespace(
        id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSD-PERP")),
        base_currency=SimpleNamespace(code="BTC"),
        quote_currency=SimpleNamespace(code="USD"),
        settlement_currency=SimpleNamespace(code="BTC"),
    )

    dummy = SimpleNamespace(
        _currency_code=BitgetDataClient._currency_code,
        _is_delivery_symbol=BitgetDataClient._is_delivery_symbol,
    )

    assert BitgetDataClient._instrument_product_type(dummy, spot) == nautilus_pyo3.BitgetProductType.SPOT  # type: ignore[arg-type]
    assert BitgetDataClient._instrument_product_type(dummy, usdt_perp) == nautilus_pyo3.BitgetProductType.USDT_FUTURES  # type: ignore[arg-type]
    assert BitgetDataClient._instrument_product_type(dummy, usdc_perp) == nautilus_pyo3.BitgetProductType.USDC_FUTURES  # type: ignore[arg-type]
    assert BitgetDataClient._instrument_product_type(dummy, coin_perp) == nautilus_pyo3.BitgetProductType.COIN_FUTURES  # type: ignore[arg-type]


def test_request_bars_fetches_and_emits_sorted_bars() -> None:
    instrument_id = InstrumentId.from_str("BTCUSDT.BITGET")
    instrument = SimpleNamespace(
        id=instrument_id,
        raw_symbol=SimpleNamespace(value="BTCUSDT"),
    )
    emitted: list[tuple[object, list[object], object, object, object]] = []

    class DummyHttpClient:
        async def request_bars(self, product_type, symbol, granularity, start_time, end_time, limit):
            assert product_type == "SPOT"
            assert symbol == "BTCUSDT"
            assert granularity == "1m"
            assert limit == 2
            return '[["1700000000000","1","2","0.5","1.5","10"],["1700000060000","1.5","2.5","1.2","2.0","12"]]'

    bar_type = SimpleNamespace(
        instrument_id=instrument_id,
        spec=SimpleNamespace(
            is_time_aggregated=lambda: True,
            price_type=PriceType.LAST,
            aggregation=BarAggregation.MINUTE,
            step=1,
        ),
        is_internally_aggregated=lambda: False,
    )
    dummy = SimpleNamespace(
        _instrument_provider=SimpleNamespace(find=lambda actual_id: instrument if actual_id == instrument_id else None),
        _instrument_product_type=lambda _instrument: "SPOT",
        _product_types={"SPOT"},
        _http_client=DummyHttpClient(),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _handle_bars=lambda bt, bars, request_id, start, end, params: emitted.append((bt, bars, request_id, start, end)),
        _bitget_bar_interval=BitgetDataClient._bitget_bar_interval,
        _datetime_to_millis=lambda value: None if value is None else value,
        _product_type_key=BitgetDataClient._product_type_key,
        _log=SimpleNamespace(error=lambda *_args, **_kwargs: None),
    )
    request = SimpleNamespace(
        bar_type=bar_type,
        start=None,
        end=None,
        limit=2,
        id="REQ-1",
        params=None,
    )

    with patch(
        "nautilus_trader.adapters.bitget.data.nautilus_pyo3.BitgetWebSocketClient.parse_bars",
        lambda payload, actual_instrument: ["bar-2", "bar-1"],
        create=True,
    ), patch(
        "nautilus_trader.adapters.bitget.data.capsule_to_data",
        lambda capsule: SimpleNamespace(ts_event=2 if capsule == "bar-2" else 1),
    ):
        asyncio.run(BitgetDataClient._request_bars(dummy, request))  # type: ignore[arg-type]

    assert emitted[0][0] is bar_type
    assert [bar.ts_event for bar in emitted[0][1]] == [1, 2]


def test_request_funding_rates_uses_history_endpoint_and_filters_window() -> None:
    instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BITGET")
    instrument = SimpleNamespace(
        id=instrument_id,
        raw_symbol=SimpleNamespace(value="BTCUSDT"),
        base_currency=SimpleNamespace(code="BTC"),
        quote_currency=SimpleNamespace(code="USDT"),
        settlement_currency=SimpleNamespace(code="USDT"),
    )
    captured: list[tuple[object, list[object], object, object, object]] = []

    class DummyHttpClient:
        async def request_funding_rate_history(self, product_type, symbol, cursor, limit):
            assert product_type == nautilus_pyo3.BitgetProductType.USDT_FUTURES
            assert symbol == "BTCUSDT"
            assert cursor == 1
            assert limit == 2
            return (
                '[{"symbol":"BTCUSDT","fundingRate":"0.0001","fundingTime":"1700000000000"},'
                '{"symbol":"BTCUSDT","fundingRate":"0.0002","fundingTime":"1700003600000"}]'
            )

    dummy = SimpleNamespace(
        _instrument_provider=SimpleNamespace(find=lambda actual_id: instrument if actual_id == instrument_id else None),
        _instrument_product_type=lambda _instrument: nautilus_pyo3.BitgetProductType.USDT_FUTURES,
        _product_types=(nautilus_pyo3.BitgetProductType.USDT_FUTURES,),
        _http_client=DummyHttpClient(),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _handle_funding_rates=lambda instrument_id_, rates, request_id, start, end, params: captured.append((instrument_id_, rates, request_id, start, end)),
        _datetime_to_millis=lambda value: value,
        _parse_timestamp_ms=BitgetDataClient._parse_timestamp_ms,
        _is_spot_product_type=lambda product_type: False,
        _log=SimpleNamespace(error=lambda *_args, **_kwargs: None, warning=lambda *_args, **_kwargs: None),
    )
    request = SimpleNamespace(
        instrument_id=instrument_id,
        start=1_699_999_999_999,
        end=1_700_003_600_000,
        limit=2,
        id="REQ-2",
        params=None,
    )

    asyncio.run(BitgetDataClient._request_funding_rates(dummy, request))  # type: ignore[arg-type]

    assert captured[0][0] == instrument_id
    assert [str(rate.rate) for rate in captured[0][1]] == ["0.0001", "0.0002"]
    assert [rate.ts_event for rate in captured[0][1]] == [1_700_000_000_000_000_000, 1_700_003_600_000_000_000]
