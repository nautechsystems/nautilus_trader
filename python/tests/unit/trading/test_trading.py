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

from nautilus_trader.common import ComponentState
from nautilus_trader.common import CustomData
from nautilus_trader.common import Signal
from nautilus_trader.common import TimeEvent
from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import DataType
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Money
from nautilus_trader.model import OptionChainSlice
from nautilus_trader.model import OptionGreeks
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import OrderAccepted
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderCanceled
from nautilus_trader.model import OrderCancelRejected
from nautilus_trader.model import OrderDenied
from nautilus_trader.model import OrderEmulated
from nautilus_trader.model import OrderExpired
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderInitialized
from nautilus_trader.model import OrderModifyRejected
from nautilus_trader.model import OrderPendingCancel
from nautilus_trader.model import OrderPendingUpdate
from nautilus_trader.model import OrderRejected
from nautilus_trader.model import OrderReleased
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderSubmitted
from nautilus_trader.model import OrderTriggered
from nautilus_trader.model import OrderType
from nautilus_trader.model import OrderUpdated
from nautilus_trader.model import Position
from nautilus_trader.model import PositionChanged
from nautilus_trader.model import PositionClosed
from nautilus_trader.model import PositionId
from nautilus_trader.model import PositionOpened
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import VenueOrderId
from nautilus_trader.trading import ForexSession
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from nautilus_trader.trading import fx_local_from_utc
from nautilus_trader.trading import fx_next_end
from nautilus_trader.trading import fx_next_start
from nautilus_trader.trading import fx_prev_end
from nautilus_trader.trading import fx_prev_start
from tests.providers import TestInstrumentProvider
from tests.unit.common.actor import TestStrategy


def test_strategy_default_construction():
    strategy = Strategy()

    assert strategy.trader_id is None
    assert strategy.strategy_id is not None
    assert strategy.state() == ComponentState.PRE_INITIALIZED
    assert strategy.is_ready() is False
    assert strategy.is_running() is False
    assert strategy.is_stopped() is False
    assert strategy.is_disposed() is False
    assert strategy.is_degraded() is False
    assert strategy.is_faulted() is False


def test_strategy_construction_with_config():
    config = StrategyConfig(
        StrategyId("S-001"),
        "001",
        None,
        None,
        False,
        False,
        False,
        0,
        3,
        TimeInForce.GTC,
        False,
        True,
        True,
        True,
        True,
        False,
    )
    strategy = Strategy(config)

    assert strategy.strategy_id == StrategyId("S-001")


def test_strategy_clock_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.clock


def test_strategy_cache_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.cache


LIFECYCLE_METHODS = ["start", "stop", "resume", "reset", "dispose", "degrade", "fault"]


@pytest.mark.parametrize("method_name", LIFECYCLE_METHODS)
def test_strategy_lifecycle_methods_reject_pre_initialized(method_name):
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="Invalid state trigger PRE_INITIALIZED"):
        getattr(strategy, method_name)()


def test_strategy_submit_order_signature():
    sig = inspect.signature(Strategy.submit_order)
    params = tuple(sig.parameters)

    assert "order" in params
    assert "position_id" in params
    assert "client_id" in params


def test_strategy_config_defaults():
    config = StrategyConfig(
        None,
        None,
        None,
        None,
        False,
        False,
        False,
        0,
        3,
        TimeInForce.GTC,
        False,
        True,
        True,
        True,
        True,
        False,
    )

    assert config.strategy_id is None
    assert config.order_id_tag is None
    assert config.oms_type is None
    assert config.manage_contingent_orders is False
    assert config.manage_gtd_expiry is False
    assert config.use_uuid_client_order_ids is True
    assert config.use_hyphens_in_client_order_ids is True
    assert config.log_events is True
    assert config.log_commands is True
    assert config.log_rejected_due_post_only_as_warning is False


def test_strategy_config_with_explicit_values():
    config = StrategyConfig(
        StrategyId("S-002"),
        "002",
        None,
        None,
        True,
        True,
        False,
        500,
        5,
        TimeInForce.IOC,
        True,
        False,
        False,
        False,
        False,
        True,
    )

    assert config.strategy_id == StrategyId("S-002")
    assert config.order_id_tag == "002"
    assert config.manage_contingent_orders is True
    assert config.manage_gtd_expiry is True
    assert config.use_uuid_client_order_ids is False
    assert config.use_hyphens_in_client_order_ids is False
    assert config.log_events is False
    assert config.log_commands is False
    assert config.log_rejected_due_post_only_as_warning is True


def test_forex_session_variants():
    variants = list(ForexSession.variants())

    assert len(variants) == 4
    assert ForexSession.from_str("SYDNEY") == ForexSession.SYDNEY
    assert ForexSession.from_str("TOKYO") == ForexSession.TOKYO
    assert ForexSession.from_str("LONDON") == ForexSession.LONDON
    assert ForexSession.from_str("NEW_YORK") == ForexSession.NEW_YORK


NOW_UTC = dt.datetime(2024, 6, 15, 12, 0, 0, tzinfo=dt.UTC)


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_next_start_returns_future_datetime(session):
    result = fx_next_start(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result > NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_next_end_returns_future_datetime(session):
    result = fx_next_end(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result > NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_prev_start_returns_past_datetime(session):
    result = fx_prev_start(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result < NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_prev_end_returns_past_datetime(session):
    result = fx_prev_end(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result < NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_local_from_utc_returns_string(session):
    result = fx_local_from_utc(session, NOW_UTC)

    assert isinstance(result, str)
    assert "2024" in result


HOOK_METHODS = [
    "on_start",
    "on_stop",
    "on_resume",
    "on_reset",
    "on_dispose",
    "on_degrade",
    "on_fault",
]

DATA_CALLBACKS = [
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
    ("on_historical_data", "historical_data"),
    ("on_historical_quotes", "historical_quotes"),
    ("on_historical_trades", "historical_trades"),
    ("on_historical_funding_rates", "historical_funding_rates"),
    ("on_historical_bars", "historical_bars"),
    ("on_historical_mark_prices", "historical_mark_prices"),
    ("on_historical_index_prices", "historical_index_prices"),
]

ORDER_CALLBACKS = [
    ("on_order_initialized", "order_initialized"),
    ("on_order_denied", "order_denied"),
    ("on_order_emulated", "order_emulated"),
    ("on_order_released", "order_released"),
    ("on_order_submitted", "order_submitted"),
    ("on_order_rejected", "order_rejected"),
    ("on_order_accepted", "order_accepted"),
    ("on_order_expired", "order_expired"),
    ("on_order_triggered", "order_triggered"),
    ("on_order_pending_update", "order_pending_update"),
    ("on_order_pending_cancel", "order_pending_cancel"),
    ("on_order_modify_rejected", "order_modify_rejected"),
    ("on_order_cancel_rejected", "order_cancel_rejected"),
    ("on_order_updated", "order_updated"),
    ("on_order_canceled", "order_canceled"),
    ("on_order_filled", "order_filled"),
]

POSITION_CALLBACKS = [
    ("on_position_opened", "position_opened"),
    ("on_position_changed", "position_changed"),
    ("on_position_closed", "position_closed"),
]


def _make_recording_method(method_name):
    def method(self, *args):
        self.calls.append((method_name, args))

    return method


def _create_recording_strategy_type():
    attrs = {}

    for method_name in HOOK_METHODS:
        attrs[method_name] = _make_recording_method(method_name)

    for method_name, _sample_name in DATA_CALLBACKS + ORDER_CALLBACKS + POSITION_CALLBACKS:
        attrs[method_name] = _make_recording_method(method_name)

    return type("RecordingStrategy", (TestStrategy,), attrs)


RecordingStrategy = _create_recording_strategy_type()


@pytest.fixture
def strategy():
    return Strategy()


@pytest.fixture
def recording_strategy():
    strategy = RecordingStrategy()
    strategy.calls = []
    return strategy


@pytest.fixture
def strategy_sample_objects():
    instrument = TestInstrumentProvider.audusd_sim()
    quote = _make_quote(instrument.id)
    trade = _make_trade(instrument.id)
    bar = _make_bar(instrument.id)
    book_deltas = _make_book_deltas(instrument.id)
    option_greeks = _make_option_greeks()
    option_chain = _make_option_chain()
    time_event = TimeEvent("timer", UUID4(), 1, 2)
    custom_data = CustomData(DataType("X"), [1, 2], 3, 4)
    signal = Signal("sig", "value", 1, 2)
    mark_price = MarkPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    index_price = IndexPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    funding_rate = FundingRateUpdate(instrument.id, Decimal("0.0001"), 1, 2, interval=480)
    instrument_status = InstrumentStatus(instrument.id, MarketStatusAction.TRADING, 1, 2)
    instrument_close = InstrumentClose(
        instrument.id,
        Price.from_str("1.00000"),
        InstrumentCloseType.END_OF_SESSION,
        1,
        2,
    )
    order_events = _make_order_events(instrument)
    position_events = _make_position_events(instrument)

    return {
        "time_event": time_event,
        "custom_data": custom_data,
        "signal": signal,
        "instrument": instrument,
        "quote": quote,
        "trade": trade,
        "bar": bar,
        "book_deltas": book_deltas,
        "book": OrderBook(instrument.id, BookType.L2_MBP),
        "mark_price": mark_price,
        "index_price": index_price,
        "funding_rate": funding_rate,
        "instrument_status": instrument_status,
        "instrument_close": instrument_close,
        "option_greeks": option_greeks,
        "option_chain": option_chain,
        "historical_data": custom_data,
        "historical_quotes": [quote],
        "historical_trades": [trade],
        "historical_funding_rates": [funding_rate],
        "historical_bars": [bar],
        "historical_mark_prices": [mark_price],
        "historical_index_prices": [index_price],
        **order_events,
        **position_events,
    }


@pytest.mark.parametrize("method_name", HOOK_METHODS)
def test_strategy_lifecycle_hooks_can_be_overridden(recording_strategy, method_name):
    assert getattr(recording_strategy, method_name)() is None

    assert recording_strategy.calls[-1] == (method_name, ())


@pytest.mark.parametrize(
    ("method_name", "sample_name"),
    DATA_CALLBACKS + ORDER_CALLBACKS + POSITION_CALLBACKS,
)
def test_strategy_callbacks_accept_runtime_objects(
    strategy,
    strategy_sample_objects,
    method_name,
    sample_name,
):
    assert getattr(strategy, method_name)(strategy_sample_objects[sample_name]) is None


@pytest.mark.parametrize(
    ("method_name", "sample_name"),
    DATA_CALLBACKS + ORDER_CALLBACKS + POSITION_CALLBACKS,
)
def test_strategy_overridden_callbacks_receive_runtime_objects(
    recording_strategy,
    strategy_sample_objects,
    method_name,
    sample_name,
):
    payload = strategy_sample_objects[sample_name]

    assert getattr(recording_strategy, method_name)(payload) is None

    call_name, call_args = recording_strategy.calls[-1]
    assert call_name == method_name
    assert call_args == (payload,)
    assert call_args[0] is payload


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
        Quantity.from_int(100),
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


def _make_order_events(instrument):
    trader_id = TraderId("TRADER-001")
    strategy_id = StrategyId("S-001")
    account_id = AccountId("SIM-001")
    client_order_id = ClientOrderId("O-001")
    venue_order_id = VenueOrderId("V-001")

    return {
        "order_initialized": OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            quantity=Quantity.from_int(100_000),
            time_in_force=TimeInForce.GTC,
            post_only=False,
            reduce_only=False,
            quote_quantity=False,
            reconciliation=False,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_denied": OrderDenied(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            reason="denied",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_emulated": OrderEmulated(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_released": OrderReleased(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            released_price=Price.from_str("1.00020"),
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_submitted": OrderSubmitted(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_rejected": OrderRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            reason="rejected",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
        ),
        "order_accepted": OrderAccepted(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
        ),
        "order_expired": OrderExpired(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_triggered": OrderTriggered(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_pending_update": OrderPendingUpdate(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
        ),
        "order_pending_cancel": OrderPendingCancel(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
        ),
        "order_modify_rejected": OrderModifyRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            reason="modify rejected",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_cancel_rejected": OrderCancelRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            reason="cancel rejected",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_updated": OrderUpdated(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            quantity=Quantity.from_int(150_000),
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
            price=Price.from_str("1.00030"),
        ),
        "order_canceled": OrderCanceled(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_filled": _make_order_filled_event(
            instrument,
            client_order_id="O-001",
            venue_order_id="V-001",
            trade_id="T-001",
            order_side=OrderSide.BUY,
            last_qty=100_000,
            last_px="1.00010",
            ts_event=10,
        ),
    }


def _make_position_events(instrument):
    position_id = "P-101"
    opening_fill = _make_order_filled_event(
        instrument,
        client_order_id="O-101",
        venue_order_id="V-101",
        trade_id="T-101",
        order_side=OrderSide.BUY,
        last_qty=100_000,
        last_px="1.00010",
        position_id=position_id,
        ts_event=10,
    )
    position = Position(instrument=instrument, fill=opening_fill)
    position_opened = PositionOpened.create(position, opening_fill, UUID4(), 11)

    change_fill = _make_order_filled_event(
        instrument,
        client_order_id="O-102",
        venue_order_id="V-102",
        trade_id="T-102",
        order_side=OrderSide.BUY,
        last_qty=50_000,
        last_px="1.00020",
        position_id=position_id,
        ts_event=12,
    )
    position.apply(change_fill)
    position_changed = PositionChanged.create(position, change_fill, UUID4(), 13)

    closing_fill = _make_order_filled_event(
        instrument,
        client_order_id="O-103",
        venue_order_id="V-103",
        trade_id="T-103",
        order_side=OrderSide.SELL,
        last_qty=150_000,
        last_px="1.00030",
        position_id=position_id,
        ts_event=14,
    )
    position.apply(closing_fill)
    position_closed = PositionClosed.create(position, closing_fill, UUID4(), 15)

    return {
        "position_opened": position_opened,
        "position_changed": position_changed,
        "position_closed": position_closed,
    }


def _make_order_filled_event(
    instrument,
    client_order_id,
    venue_order_id,
    trade_id,
    order_side,
    last_qty,
    last_px,
    ts_event,
    position_id=None,
):
    return OrderFilled(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId(client_order_id),
        venue_order_id=VenueOrderId(venue_order_id),
        account_id=AccountId("SIM-001"),
        trade_id=TradeId(trade_id),
        order_side=order_side,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(last_qty),
        last_px=Price.from_str(last_px),
        currency=instrument.quote_currency,
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=ts_event,
        ts_init=ts_event + 1,
        reconciliation=False,
        position_id=PositionId(position_id) if position_id is not None else None,
        commission=Money.from_str("2.00 USD"),
    )
