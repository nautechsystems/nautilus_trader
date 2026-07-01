# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import datetime as dt
import inspect
from decimal import Decimal

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import ComponentState
from nautilus_trader.common import CustomData
from nautilus_trader.common import DataActor
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.common import Signal
from nautilus_trader.common import TimeEvent
from nautilus_trader.core import UUID4
from nautilus_trader.model import ActorId
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import Block
from nautilus_trader.model import Blockchain
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import Chain
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType
from nautilus_trader.model import Dex
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import OptionChainSlice
from nautilus_trader.model import OptionGreeks
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Pool
from nautilus_trader.model import PoolFeeCollect
from nautilus_trader.model import PoolFlash
from nautilus_trader.model import PoolLiquidityUpdate
from nautilus_trader.model import PoolLiquidityUpdateType
from nautilus_trader.model import PoolSwap
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Token
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from tests.providers import TestInstrumentProvider
from tests.unit.common.actor import TestActor
from tests.unit.common.actor import TestActorConfig


LIFECYCLE_METHODS = [
    "start",
    "stop",
    "resume",
    "reset",
    "dispose",
    "degrade",
    "fault",
]

HOOK_METHODS = [
    "on_start",
    "on_stop",
    "on_resume",
    "on_reset",
    "on_dispose",
    "on_degrade",
    "on_fault",
]

TYPED_CALLBACKS = [
    ("on_time_event", "time_event"),
    ("on_data", "custom_data"),
    ("on_signal", "signal"),
    ("on_instrument", "instrument"),
    ("on_quote", "quote"),
    ("on_trade", "trade"),
    ("on_bar", "bar"),
    ("on_book_deltas", "book_deltas"),
    ("on_book", "book"),
    ("on_mark_price", "mark_price"),
    ("on_index_price", "index_price"),
    ("on_funding_rate", "funding_rate"),
    ("on_instrument_status", "instrument_status"),
    ("on_instrument_close", "instrument_close"),
    ("on_option_greeks", "option_greeks"),
    ("on_option_chain", "option_chain"),
    ("on_block", "block"),
    ("on_pool", "pool"),
    ("on_pool_swap", "pool_swap"),
    ("on_pool_liquidity_update", "pool_liquidity_update"),
    ("on_pool_fee_collect", "pool_fee_collect"),
    ("on_pool_flash", "pool_flash"),
]

HISTORICAL_CALLBACKS = [
    ("on_historical_data", "historical_data"),
    ("on_historical_quotes", "historical_quotes"),
    ("on_historical_trades", "historical_trades"),
    ("on_historical_funding_rates", "historical_funding_rates"),
    ("on_historical_bars", "historical_bars"),
    ("on_historical_mark_prices", "historical_mark_prices"),
    ("on_historical_index_prices", "historical_index_prices"),
]

NO_PARAMETERS = ()
STATE_PARAMETERS = ("state",)

LIFECYCLE_HOOK_SIGNATURES = [
    ("on_start", NO_PARAMETERS),
    ("on_stop", NO_PARAMETERS),
    ("on_resume", NO_PARAMETERS),
    ("on_reset", NO_PARAMETERS),
    ("on_dispose", NO_PARAMETERS),
    ("on_degrade", NO_PARAMETERS),
    ("on_fault", NO_PARAMETERS),
]
SAVE_LOAD_HOOK_SIGNATURES = [
    ("on_save", NO_PARAMETERS),
    ("on_load", STATE_PARAMETERS),
]
DATA_CALLBACK_SIGNATURES = [
    ("on_time_event", ("event",)),
    ("on_data", ("data",)),
    ("on_signal", ("signal",)),
    ("on_instrument", ("instrument",)),
    ("on_quote", ("quote",)),
    ("on_trade", ("trade",)),
    ("on_bar", ("bar",)),
    ("on_book_deltas", ("deltas",)),
    ("on_book", ("book",)),
    ("on_mark_price", ("mark_price",)),
    ("on_index_price", ("index_price",)),
    ("on_funding_rate", ("funding_rate",)),
    ("on_instrument_status", ("status",)),
    ("on_instrument_close", ("close",)),
    ("on_option_greeks", ("greeks",)),
    ("on_option_chain", ("slice",)),
]
HISTORICAL_CALLBACK_SIGNATURES = [
    ("on_historical_data", ("data",)),
    ("on_historical_quotes", ("quotes",)),
    ("on_historical_trades", ("trades",)),
    ("on_historical_funding_rates", ("funding_rates",)),
    ("on_historical_bars", ("bars",)),
    ("on_historical_mark_prices", ("mark_prices",)),
    ("on_historical_index_prices", ("index_prices",)),
]
DEFI_CALLBACK_SIGNATURES = [
    ("on_block", ("block",)),
    ("on_pool", ("pool",)),
    ("on_pool_swap", ("swap",)),
    ("on_pool_liquidity_update", ("update",)),
    ("on_pool_fee_collect", ("update",)),
    ("on_pool_flash", ("flash",)),
]
CALLBACK_SIGNATURES = (
    LIFECYCLE_HOOK_SIGNATURES
    + SAVE_LOAD_HOOK_SIGNATURES
    + DATA_CALLBACK_SIGNATURES
    + HISTORICAL_CALLBACK_SIGNATURES
    + DEFI_CALLBACK_SIGNATURES
)

DATA_SUBSCRIPTION_PARAMETERS = ("data_type", "client_id", "params")
DATA_REQUEST_PARAMETERS = ("data_type", "client_id", "start", "end", "limit", "params")
VENUE_SUBSCRIPTION_PARAMETERS = ("venue", "client_id", "params")
VENUE_REQUEST_PARAMETERS = ("venue", "start", "end", "client_id", "params")
INSTRUMENT_SUBSCRIPTION_PARAMETERS = ("instrument_id", "client_id", "params")
BOOK_DELTAS_SUBSCRIPTION_PARAMETERS = (
    "instrument_id",
    "book_type",
    "depth",
    "client_id",
    "managed",
    "params",
)
BOOK_INTERVAL_SUBSCRIPTION_PARAMETERS = (
    "instrument_id",
    "book_type",
    "interval_ms",
    "depth",
    "client_id",
    "params",
)
BOOK_INTERVAL_UNSUBSCRIBE_PARAMETERS = ("instrument_id", "interval_ms", "client_id", "params")
BAR_SUBSCRIPTION_PARAMETERS = ("bar_type", "client_id", "params")
BLOCK_SUBSCRIPTION_PARAMETERS = ("chain", "client_id", "params")
OPTION_CHAIN_SUBSCRIPTION_PARAMETERS = (
    "series_id",
    "strike_range",
    "snapshot_interval_ms",
    "client_id",
    "params",
)
INSTRUMENT_REQUEST_PARAMETERS = ("instrument_id", "start", "end", "client_id", "params")
BOOK_SNAPSHOT_REQUEST_PARAMETERS = ("instrument_id", "depth", "client_id", "params")
BOOK_DELTAS_REQUEST_PARAMETERS = ("instrument_id", "start", "end", "limit", "client_id", "params")
BOOK_DEPTH_REQUEST_PARAMETERS = (
    "instrument_id",
    "start",
    "end",
    "limit",
    "depth",
    "client_id",
    "params",
)
INSTRUMENT_HISTORY_REQUEST_PARAMETERS = (
    "instrument_id",
    "start",
    "end",
    "limit",
    "client_id",
    "params",
)
BAR_REQUEST_PARAMETERS = ("bar_type", "start", "end", "limit", "client_id", "params")
OPTION_CHAIN_UNSUBSCRIBE_PARAMETERS = ("series_id", "client_id")

REGISTRATION_REQUIRED_SIGNATURES = [
    ("subscribe_data", DATA_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instruments", VENUE_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instrument", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_book_deltas", BOOK_DELTAS_SUBSCRIPTION_PARAMETERS),
    ("subscribe_book_at_interval", BOOK_INTERVAL_SUBSCRIPTION_PARAMETERS),
    ("subscribe_quotes", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_trades", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_bars", BAR_SUBSCRIPTION_PARAMETERS),
    ("subscribe_mark_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_index_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_funding_rates", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_option_greeks", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instrument_status", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instrument_close", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_option_chain", OPTION_CHAIN_SUBSCRIPTION_PARAMETERS),
    ("subscribe_blocks", BLOCK_SUBSCRIPTION_PARAMETERS),
    ("subscribe_pool", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_pool_swaps", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_pool_liquidity_updates", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_pool_fee_collects", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_pool_flash_events", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_data", DATA_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_instruments", VENUE_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_instrument", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_book_deltas", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_book_at_interval", BOOK_INTERVAL_UNSUBSCRIBE_PARAMETERS),
    ("unsubscribe_quotes", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_trades", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_bars", BAR_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_mark_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_index_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_funding_rates", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_option_greeks", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_instrument_status", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_instrument_close", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_option_chain", OPTION_CHAIN_UNSUBSCRIBE_PARAMETERS),
    ("unsubscribe_blocks", BLOCK_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_pool", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_pool_swaps", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_pool_liquidity_updates", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_pool_fee_collects", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_pool_flash_events", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("request_data", DATA_REQUEST_PARAMETERS),
    ("request_instrument", INSTRUMENT_REQUEST_PARAMETERS),
    ("request_instruments", VENUE_REQUEST_PARAMETERS),
    ("request_book_snapshot", BOOK_SNAPSHOT_REQUEST_PARAMETERS),
    ("request_book_deltas", BOOK_DELTAS_REQUEST_PARAMETERS),
    ("request_book_depth", BOOK_DEPTH_REQUEST_PARAMETERS),
    ("request_quotes", INSTRUMENT_HISTORY_REQUEST_PARAMETERS),
    ("request_trades", INSTRUMENT_HISTORY_REQUEST_PARAMETERS),
    ("request_funding_rates", INSTRUMENT_HISTORY_REQUEST_PARAMETERS),
    ("request_bars", BAR_REQUEST_PARAMETERS),
]
REMOVED_ORDER_EVENT_METHODS = [
    "on_order_filled",
    "on_order_canceled",
    "subscribe_order_fills",
    "subscribe_order_cancels",
    "unsubscribe_order_fills",
    "unsubscribe_order_cancels",
]
HISTORICAL_REQUEST_DATETIME_CASES = [
    pytest.param("datetime-utc", id="datetime-utc"),
    pytest.param("pandas-timestamp-utc", id="pandas-timestamp-utc"),
    pytest.param("pandas-timestamp-utc-nanos", id="pandas-timestamp-utc-nanos"),
]


def _make_recording_method(method_name):
    def method(self, *args):
        self.calls.append((method_name, args))

    return method


def _create_recording_actor_type():
    attrs = {}

    for method_name in HOOK_METHODS:
        attrs[method_name] = _make_recording_method(method_name)

    for method_name, _sample_name in TYPED_CALLBACKS + HISTORICAL_CALLBACKS:
        attrs[method_name] = _make_recording_method(method_name)

    return type("RecordingActor", (TestActor,), attrs)


RecordingActor = _create_recording_actor_type()


class HistoricalRequestProbeActor(TestActor):
    observed_request_ids = {}
    request_time = dt.datetime(1970, 1, 1, tzinfo=dt.UTC)

    def on_start(self):
        instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        client_id = ClientId("SIM")
        venue = Venue("SIM")
        bar_type = BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL")
        request_time = type(self).request_time

        type(self).observed_request_ids = {
            "data": self.request_data(
                DataType("TestData"),
                client_id,
                start=request_time,
                limit=1,
                params={"kind": "data"},
            ),
            "instrument": self.request_instrument(
                instrument_id,
                start=request_time,
                params={"kind": "instrument"},
            ),
            "instruments": self.request_instruments(
                venue,
                end=request_time,
                params={"kind": "instruments"},
            ),
            "book_deltas": self.request_book_deltas(
                instrument_id,
                start=request_time,
                limit=1,
                params={"kind": "deltas"},
            ),
            "book_depth": self.request_book_depth(
                instrument_id,
                end=request_time,
                limit=2,
                depth=5,
                params={"kind": "depth"},
            ),
            "quotes": self.request_quotes(
                instrument_id,
                start=request_time,
                limit=1,
                params={"kind": "quotes"},
            ),
            "trades": self.request_trades(
                instrument_id,
                end=request_time,
                limit=1,
                params={"kind": "trades"},
            ),
            "funding_rates": self.request_funding_rates(
                instrument_id,
                start=request_time,
                limit=1,
                params={"kind": "funding-rates"},
            ),
            "bars": self.request_bars(
                bar_type,
                end=request_time,
                limit=1,
                params={"kind": "bars"},
            ),
        }


def test_data_actor_pre_registration_surface(actor):
    assert isinstance(actor, DataActor)
    assert actor.log.name == "ACTOR-001"
    assert actor.actor_id == ActorId("ACTOR-001")
    assert actor.trader_id is None
    assert actor.state() == ComponentState.PRE_INITIALIZED
    assert actor.is_ready() is False
    assert actor.is_running() is False
    assert actor.is_stopped() is False
    assert actor.is_degraded() is False
    assert actor.is_faulted() is False
    assert actor.is_disposed() is False

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = actor.clock

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = actor.cache


@pytest.mark.parametrize("method_name", LIFECYCLE_METHODS)
def test_data_actor_lifecycle_methods_reject_pre_initialized_state(actor, method_name):
    with pytest.raises(RuntimeError, match="Invalid state trigger PRE_INITIALIZED"):
        getattr(actor, method_name)()


@pytest.mark.parametrize("method_name", HOOK_METHODS)
def test_data_actor_lifecycle_hooks_are_callable(actor, method_name):
    assert getattr(actor, method_name)() is None


@pytest.mark.parametrize("method_name", HOOK_METHODS)
def test_data_actor_overridden_lifecycle_hooks_are_called(recording_actor, method_name):
    assert getattr(recording_actor, method_name)() is None

    assert recording_actor.calls[-1] == (method_name, ())


@pytest.mark.parametrize(("method_name", "sample_name"), TYPED_CALLBACKS)
def test_data_actor_typed_callbacks_accept_runtime_objects(
    actor,
    sample_objects,
    method_name,
    sample_name,
):
    assert getattr(actor, method_name)(sample_objects[sample_name]) is None


@pytest.mark.parametrize(("method_name", "sample_name"), TYPED_CALLBACKS)
def test_data_actor_overridden_typed_callbacks_receive_runtime_objects(
    recording_actor,
    sample_objects,
    method_name,
    sample_name,
):
    payload = sample_objects[sample_name]

    assert getattr(recording_actor, method_name)(payload) is None

    call_name, call_args = recording_actor.calls[-1]
    assert call_name == method_name
    assert call_args == (payload,)
    assert call_args[0] is payload


def test_data_actor_overridden_pool_swap_callback_exposes_raw_payload(
    recording_actor,
    sample_objects,
):
    payload = sample_objects["pool_swap"]

    assert recording_actor.on_pool_swap(payload) is None

    call_name, call_args = recording_actor.calls[-1]
    assert call_name == "on_pool_swap"
    assert call_args == (payload,)

    swap = call_args[0]
    assert swap is payload
    assert swap.recipient == "0x0000000000000000000000000000000000000005"
    assert swap.amount0 == "1"
    assert swap.amount1 == "-2"
    assert swap.sqrt_price_x96 == "79228162514264337593543950336"
    assert swap.liquidity == "100"
    assert swap.tick == 1


@pytest.mark.parametrize(("method_name", "sample_name"), HISTORICAL_CALLBACKS)
def test_data_actor_historical_callbacks_accept_runtime_objects(
    actor,
    sample_objects,
    method_name,
    sample_name,
):
    assert getattr(actor, method_name)(sample_objects[sample_name]) is None


@pytest.mark.parametrize(("method_name", "sample_name"), HISTORICAL_CALLBACKS)
def test_data_actor_overridden_historical_callbacks_receive_runtime_objects(
    recording_actor,
    sample_objects,
    method_name,
    sample_name,
):
    payload = sample_objects[sample_name]

    assert getattr(recording_actor, method_name)(payload) is None

    call_name, call_args = recording_actor.calls[-1]
    assert call_name == method_name
    assert call_args == (payload,)
    assert call_args[0] is payload


def test_data_actor_shutdown_system_signature_exposes_optional_reason(actor):
    signature = inspect.signature(actor.shutdown_system)
    parameter = signature.parameters["reason"]

    assert list(signature.parameters) == ["reason"]
    assert parameter.default is None


@pytest.mark.parametrize(("method_name", "parameter_names"), CALLBACK_SIGNATURES)
def test_data_actor_callback_methods_expose_expected_signatures(
    actor,
    method_name,
    parameter_names,
):
    signature = inspect.signature(getattr(actor, method_name))

    assert tuple(signature.parameters) == parameter_names


@pytest.mark.parametrize(("method_name", "parameter_names"), REGISTRATION_REQUIRED_SIGNATURES)
def test_data_actor_registration_gated_methods_expose_expected_signatures(
    actor,
    method_name,
    parameter_names,
):
    signature = inspect.signature(getattr(actor, method_name))

    assert tuple(signature.parameters) == parameter_names


@pytest.mark.parametrize("method_name", REMOVED_ORDER_EVENT_METHODS)
def test_data_actor_order_event_methods_are_not_exposed(actor, method_name):
    assert not hasattr(actor, method_name)


@pytest.mark.parametrize("request_time", HISTORICAL_REQUEST_DATETIME_CASES)
def test_data_actor_historical_requests_accept_datetimes_when_registered(request_time):
    HistoricalRequestProbeActor.observed_request_ids = {}
    HistoricalRequestProbeActor.request_time = _historical_request_time(request_time)
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_actor_from_config(
        ImportableActorConfig(
            actor_path="tests.unit.common.test_actor:HistoricalRequestProbeActor",
            config_path="tests.unit.common.actor:TestActorConfig",
            config={"actor_id": "HISTORICAL-REQUEST-ACTOR"},
        ),
    )

    try:
        engine.run()

        assert set(HistoricalRequestProbeActor.observed_request_ids) == {
            "data",
            "instrument",
            "instruments",
            "book_deltas",
            "book_depth",
            "quotes",
            "trades",
            "funding_rates",
            "bars",
        }

        for request_id in HistoricalRequestProbeActor.observed_request_ids.values():
            assert UUID4.from_str(request_id)
    finally:
        engine.dispose()


def _historical_request_time(request_time):
    if request_time == "datetime-utc":
        return dt.datetime(1970, 1, 1, tzinfo=dt.UTC)

    pd = pytest.importorskip("pandas")

    if request_time == "pandas-timestamp-utc":
        return pd.Timestamp("1970-01-01T00:00:00Z")

    if request_time == "pandas-timestamp-utc-nanos":
        return pd.Timestamp(0, unit="ns", tz="UTC")

    raise ValueError(f"Unknown historical request datetime case: {request_time}")


@pytest.fixture
def actor():
    config = TestActorConfig(
        actor_id=ActorId("ACTOR-001"),
        log_events=False,
        log_commands=False,
    )
    return TestActor(config)


@pytest.fixture
def recording_actor():
    config = TestActorConfig(
        actor_id=ActorId("ACTOR-001"),
        log_events=False,
        log_commands=False,
    )
    actor = RecordingActor(config)
    actor.calls = []
    return actor


@pytest.fixture
def sample_objects():
    instrument = TestInstrumentProvider.audusd_sim()
    quote = _make_quote(instrument.id)
    trade = _make_trade(instrument.id)
    bar = _make_bar(instrument.id)
    book_deltas = _make_book_deltas(instrument.id)
    option_greeks = _make_option_greeks()
    option_chain = _make_option_chain()
    time_event = TimeEvent("timer", UUID4(), 5, 6)
    block = _make_block()
    pool = _make_pool()
    custom_data = CustomData(DataType("X"), [1, 2], 3, 4)
    mark_price = MarkPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    index_price = IndexPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    funding_rate = FundingRateUpdate(instrument.id, Decimal("0.0001"), 1, 2, interval=480)

    return {
        "time_event": time_event,
        "custom_data": custom_data,
        "signal": Signal("sig", "value", 1, 2),
        "instrument": instrument,
        "quote": quote,
        "trade": trade,
        "bar": bar,
        "book_deltas": book_deltas,
        "book": OrderBook(instrument.id, BookType.L2_MBP),
        "mark_price": mark_price,
        "index_price": index_price,
        "funding_rate": funding_rate,
        "instrument_status": InstrumentStatus(instrument.id, MarketStatusAction.TRADING, 1, 2),
        "instrument_close": InstrumentClose(
            instrument.id,
            Price.from_str("1.00000"),
            InstrumentCloseType.END_OF_SESSION,
            1,
            2,
        ),
        "option_greeks": option_greeks,
        "option_chain": option_chain,
        "block": block,
        "pool": pool,
        "pool_swap": _make_pool_swap(pool),
        "pool_liquidity_update": _make_pool_liquidity_update(pool),
        "pool_fee_collect": _make_pool_fee_collect(pool),
        "pool_flash": _make_pool_flash(pool),
        "historical_data": [custom_data],
        "historical_quotes": [quote],
        "historical_trades": [trade],
        "historical_funding_rates": [funding_rate],
        "historical_bars": [bar],
        "historical_mark_prices": [mark_price],
        "historical_index_prices": [index_price],
    }


def _make_quote(instrument_id):
    return QuoteTick(
        instrument_id,
        Price.from_str("1.00000"),
        Price.from_str("1.00001"),
        Quantity.from_int(1),
        Quantity.from_int(2),
        1,
        2,
    )


def _make_trade(instrument_id):
    return TradeTick(
        instrument_id,
        Price.from_str("1.00000"),
        Quantity.from_int(10),
        AggressorSide.BUYER,
        TradeId("T-001"),
        1,
        2,
    )


def _make_bar(instrument_id):
    bar_type = BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")
    return Bar(
        bar_type,
        Price.from_str("1.00000"),
        Price.from_str("1.10000"),
        Price.from_str("0.90000"),
        Price.from_str("1.05000"),
        Quantity.from_int(10),
        1,
        2,
    )


def _make_book_deltas(instrument_id):
    bid = BookOrder(OrderSide.BUY, Price.from_str("1.00000"), Quantity.from_int(1), 1)
    ask = BookOrder(OrderSide.SELL, Price.from_str("1.10000"), Quantity.from_int(2), 2)
    delta1 = OrderBookDelta(instrument_id, BookAction.ADD, bid, 0, 1, 1, 2)
    delta2 = OrderBookDelta(instrument_id, BookAction.ADD, ask, 0, 2, 1, 2)
    return OrderBookDeltas(instrument_id, [delta1, delta2])


def _make_option_greeks():
    instrument_id = InstrumentId.from_str("BTC-20240329-50000-C.DERIBIT")
    return OptionGreeks(
        instrument_id,
        0.5,
        0.1,
        0.2,
        -0.3,
        0.05,
        0.6,
        0.55,
        0.65,
        50_000.0,
        42.0,
        3,
        4,
    )


def _make_option_chain():
    series_id = OptionSeriesId.from_expiry("DERIBIT", "BTC", "USD", "2024-03-29")
    return OptionChainSlice(series_id, Price.from_str("50000.0"), 5, 6)


def _make_block():
    return Block(
        Blockchain.BASE,
        "0x1111111111111111111111111111111111111111111111111111111111111111",
        "0x0000000000000000000000000000000000000000000000000000000000000000",
        1,
        "0x0000000000000000000000000000000000000001",
        30_000_000,
        15_000_000,
        7,
    )


def _make_pool():
    chain = Chain(Blockchain.BASE, 8453)
    dex = _make_dex(chain)
    token0 = _make_token0(chain)
    token1 = _make_token1(chain)
    return Pool(
        chain=chain,
        dex=dex,
        address="0x0000000000000000000000000000000000000003",
        pool_identifier="0x0000000000000000000000000000000000000003",
        creation_block=1,
        token0=token0,
        token1=token1,
        fee=500,
        tick_spacing=10,
        ts_init=2,
    )


def _make_pool_swap(pool):
    return PoolSwap(
        chain=pool.chain,
        dex=pool.dex,
        instrument_id=pool.instrument_id,
        pool_identifier=pool.address,
        block=1,
        transaction_hash="0x3333333333333333333333333333333333333333333333333333333333333333",
        transaction_index=0,
        log_index=1,
        timestamp=10,
        sender="0x0000000000000000000000000000000000000004",
        receiver="0x0000000000000000000000000000000000000005",
        amount0="1",
        amount1="-2",
        sqrt_price_x96="79228162514264337593543950336",
        liquidity=100,
        tick=1,
    )


def _make_pool_liquidity_update(pool):
    return PoolLiquidityUpdate(
        chain=pool.chain,
        dex=pool.dex,
        pool_identifier=pool.address,
        instrument_id=pool.instrument_id,
        kind=PoolLiquidityUpdateType.MINT,
        block=1,
        transaction_hash="0x4444444444444444444444444444444444444444444444444444444444444444",
        transaction_index=0,
        log_index=1,
        sender=None,
        owner="0x0000000000000000000000000000000000000004",
        position_liquidity="10",
        amount0="1",
        amount1="2",
        tick_lower=-10,
        tick_upper=10,
        timestamp=10,
    )


def _make_pool_fee_collect(pool):
    return PoolFeeCollect(
        chain=pool.chain,
        dex=pool.dex,
        pool_identifier=pool.address,
        instrument_id=pool.instrument_id,
        block=1,
        transaction_hash="0x5555555555555555555555555555555555555555555555555555555555555555",
        transaction_index=0,
        log_index=1,
        owner="0x0000000000000000000000000000000000000004",
        amount0="1",
        amount1="2",
        tick_lower=-10,
        tick_upper=10,
        timestamp=10,
    )


def _make_pool_flash(pool):
    return PoolFlash(
        chain=pool.chain,
        dex=pool.dex,
        pool_identifier=pool.address,
        instrument_id=pool.instrument_id,
        block=1,
        transaction_hash="0x6666666666666666666666666666666666666666666666666666666666666666",
        transaction_index=0,
        log_index=1,
        sender="0x0000000000000000000000000000000000000004",
        recipient="0x0000000000000000000000000000000000000005",
        amount0="1",
        amount1="2",
        paid0="3",
        paid1="4",
        timestamp=10,
    )


def _make_dex(chain):
    return Dex(
        chain=chain,
        name="UniswapV3",
        factory="0x0000000000000000000000000000000000000fac",
        factory_creation_block=1,
        amm_type="CLAMM",
        pool_created_event="PoolCreated",
        swap_event="Swap",
        mint_event="Mint",
        burn_event="Burn",
        collect_event="Collect",
    )


def _make_token0(chain):
    return Token(
        chain=chain,
        address="0x0000000000000000000000000000000000000001",
        name="USD Coin",
        symbol="USDC",
        decimals=6,
    )


def _make_token1(chain):
    return Token(
        chain=chain,
        address="0x0000000000000000000000000000000000000002",
        name="Wrapped Ether",
        symbol="WETH",
        decimals=18,
    )
