# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Tests for ``LmexLiveMarketDataClient`` message routing and parsing.

No live API calls or real NautilusTrader framework objects are constructed.
The data client methods under test are called on a lightweight stub that
carries only the attributes each method actually touches.
"""

from __future__ import annotations

import types
from unittest.mock import MagicMock, patch

import msgspec
import pytest


# ---------------------------------------------------------------------------
# Stub factory
# ---------------------------------------------------------------------------


def _make_stub(ws_trade_msg: bytes | None = None) -> types.SimpleNamespace:
    """
    Build a minimal namespace that satisfies all attribute accesses made by
    ``_handle_msg``, ``_handle_trade_msg``, and ``_parse_orderbook_snapshot``.

    The class-level msgspec decoders are copied onto the stub so the real
    decode logic runs (no mocking of JSON parsing).
    """
    from nautilus_trader.adapters.lmex.constants import (
        LMEX_WS_TOPIC_ORDERBOOK,
        LMEX_WS_TOPIC_TRADES,
        LMEX_WS_TOPIC_NOTIFICATIONS,
    )
    from nautilus_trader.adapters.lmex.data import LmexLiveMarketDataClient
    from nautilus_trader.adapters.lmex.schemas.ws import (
        LmexWsMsg,
        LmexWsTradeMsg,
        LmexWsOrderBookMsg,
    )

    stub = types.SimpleNamespace()

    # Real decoders from the class (class-level attributes in data.py)
    stub._dec_envelope = msgspec.json.Decoder(LmexWsMsg)
    stub._dec_trade = msgspec.json.Decoder(LmexWsTradeMsg)
    stub._dec_orderbook = msgspec.json.Decoder(LmexWsOrderBookMsg)

    # Logger mock
    stub._log = MagicMock()

    # Clock mock
    stub._clock = MagicMock()
    stub._clock.timestamp_ns.return_value = 1_779_786_372_223_000_000

    # Cache mock
    stub._cache = MagicMock()

    # Track published data
    stub._published: list = []

    def _handle_data(obj):
        stub._published.append(obj)

    stub._handle_data = _handle_data

    # Bind the real methods from the class
    stub._handle_msg = LmexLiveMarketDataClient._handle_msg.__get__(stub, type(stub))
    stub._handle_trade_msg = LmexLiveMarketDataClient._handle_trade_msg.__get__(
        stub, type(stub)
    )
    stub._handle_orderbook_msg = LmexLiveMarketDataClient._handle_orderbook_msg.__get__(
        stub, type(stub)
    )
    stub._parse_orderbook_snapshot = (
        LmexLiveMarketDataClient._parse_orderbook_snapshot.__get__(stub, type(stub))
    )

    return stub


def _make_mock_instrument(price_precision: int = 1, size_precision: int = 5):
    """Return a mock NT instrument with the given precisions."""
    instrument = MagicMock()
    instrument.price_precision = price_precision
    instrument.size_precision = size_precision
    return instrument


# ---------------------------------------------------------------------------
# _handle_msg routing tests
# ---------------------------------------------------------------------------


class TestHandleMsgRouting:
    """Tests for WS message dispatch in ``_handle_msg``."""

    def test_trade_msg_dispatches_to_handle_trade(self, ws_trade_msg: bytes) -> None:
        """A ``tradeHistoryApi`` message dispatches to ``_handle_trade_msg``."""
        stub = _make_stub()
        # Give cache an instrument so the trade handler can proceed
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)
        stub._cache.instrument.return_value = _make_mock_instrument()

        stub._handle_msg(ws_trade_msg)

        # Two trades in the fixture → _handle_data called twice
        assert len(stub._published) == 2
        stub._log.warning.assert_not_called()

    def test_subscribe_ack_logs_debug_not_warning(
        self, ws_subscribe_ack: bytes
    ) -> None:
        """A subscribe ack is logged at debug level and does not emit a warning."""
        stub = _make_stub()
        stub._handle_msg(ws_subscribe_ack)

        stub._log.debug.assert_called_once()
        stub._log.warning.assert_not_called()
        assert len(stub._published) == 0

    def test_unknown_topic_logs_debug(self) -> None:
        """A message with an unrecognised topic is logged at debug level."""
        stub = _make_stub()
        raw = b'{"topic":"unknownTopic:BTC-USD","data":[]}'
        stub._handle_msg(raw)

        stub._log.debug.assert_called()
        stub._log.warning.assert_not_called()

    def test_invalid_json_logs_warning(self) -> None:
        """Unparseable bytes trigger a warning with no exception propagation."""
        stub = _make_stub()
        stub._handle_msg(b"not valid json {{{")

        stub._log.warning.assert_called_once()
        assert len(stub._published) == 0

    def test_uncached_instrument_logs_warning(self, ws_trade_msg: bytes) -> None:
        """Trade for instrument not in cache logs a warning and publishes nothing."""
        stub = _make_stub()
        stub._cache.instrument.return_value = None  # instrument absent

        stub._handle_msg(ws_trade_msg)

        stub._log.warning.assert_called()
        assert len(stub._published) == 0


# ---------------------------------------------------------------------------
# _handle_trade_msg field correctness
# ---------------------------------------------------------------------------


class TestHandleTradeMsg:
    """Tests for trade tick field values from WS trade messages."""

    def test_trade_tick_aggressor_side_sell(self, ws_trade_msg: bytes) -> None:
        """First datum (side=SELL) produces AggressorSide.SELLER."""
        from nautilus_trader.model.enums import AggressorSide

        stub = _make_stub()
        stub._cache.instrument.return_value = _make_mock_instrument(
            price_precision=1, size_precision=5
        )

        stub._handle_msg(ws_trade_msg)

        first_tick = stub._published[0]
        assert first_tick.aggressor_side == AggressorSide.SELLER

    def test_trade_tick_aggressor_side_buy(self, ws_trade_msg: bytes) -> None:
        """Second datum (side=BUY) produces AggressorSide.BUYER."""
        from nautilus_trader.model.enums import AggressorSide

        stub = _make_stub()
        stub._cache.instrument.return_value = _make_mock_instrument(
            price_precision=1, size_precision=5
        )

        stub._handle_msg(ws_trade_msg)

        second_tick = stub._published[1]
        assert second_tick.aggressor_side == AggressorSide.BUYER

    def test_trade_tick_price_value(self, ws_trade_msg: bytes) -> None:
        """First tick price matches the fixture value 76653.1."""
        stub = _make_stub()
        stub._cache.instrument.return_value = _make_mock_instrument(
            price_precision=1, size_precision=5
        )

        stub._handle_msg(ws_trade_msg)

        first_tick = stub._published[0]
        assert float(first_tick.price) == pytest.approx(76653.1, rel=1e-5)

    def test_trade_tick_size_value(self, ws_trade_msg: bytes) -> None:
        """First tick size matches the fixture value 0.0145."""
        stub = _make_stub()
        stub._cache.instrument.return_value = _make_mock_instrument(
            price_precision=1, size_precision=5
        )

        stub._handle_msg(ws_trade_msg)

        first_tick = stub._published[0]
        assert float(first_tick.size) == pytest.approx(0.0145, rel=1e-4)

    def test_trade_tick_trade_id(self, ws_trade_msg: bytes) -> None:
        """First tick trade_id matches fixture tradeId 31626447."""
        stub = _make_stub()
        stub._cache.instrument.return_value = _make_mock_instrument(
            price_precision=1, size_precision=5
        )

        stub._handle_msg(ws_trade_msg)

        first_tick = stub._published[0]
        assert first_tick.trade_id.value == "31626447"

    def test_trade_tick_ts_event(self, ws_trade_msg: bytes) -> None:
        """ts_event is millis_to_nanos of fixture timestamp 1779786372223."""
        from nautilus_trader.core.datetime import millis_to_nanos

        stub = _make_stub()
        stub._cache.instrument.return_value = _make_mock_instrument(
            price_precision=1, size_precision=5
        )

        stub._handle_msg(ws_trade_msg)

        first_tick = stub._published[0]
        assert first_tick.ts_event == millis_to_nanos(1779786372223)

    def test_trade_tick_count(self, ws_trade_msg: bytes) -> None:
        """Two data entries produce exactly two TradeTick objects."""
        stub = _make_stub()
        stub._cache.instrument.return_value = _make_mock_instrument(
            price_precision=1, size_precision=5
        )

        stub._handle_msg(ws_trade_msg)

        assert len(stub._published) == 2


# ---------------------------------------------------------------------------
# _parse_orderbook_snapshot
# ---------------------------------------------------------------------------


class TestParseOrderbookSnapshot:
    """Tests for ``_parse_orderbook_snapshot`` → ``OrderBookDeltas``."""

    def _make_entry(self, price: str, size: str):
        """Return a minimal entry object matching LmexOrderBookEntry API."""
        obj = MagicMock()
        obj.price = price
        obj.size = size
        return obj

    def test_snapshot_produces_correct_delta_count(self) -> None:
        """2 bids + 2 asks → 4 deltas."""
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        stub = _make_stub()
        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)

        buy_quote = [self._make_entry("76700.0", "0.5"), self._make_entry("76690.0", "1.0")]
        sell_quote = [self._make_entry("76710.0", "0.3"), self._make_entry("76720.0", "0.8")]

        deltas = stub._parse_orderbook_snapshot(
            "BTC-USD",
            buy_quote,
            sell_quote,
            instrument_id=iid,
            price_precision=1,
            size_precision=5,
            ts_event=1_000_000,
            ts_init=1_000_001,
        )

        assert len(deltas.deltas) == 4

    def test_snapshot_bid_side(self) -> None:
        """Bid entries produce OrderSide.BUY deltas."""
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.enums import OrderSide
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        stub = _make_stub()
        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)

        deltas = stub._parse_orderbook_snapshot(
            "BTC-USD",
            [self._make_entry("76700.0", "0.5")],
            [],
            instrument_id=iid,
            price_precision=1,
            size_precision=5,
            ts_event=0,
            ts_init=0,
        )

        assert deltas.deltas[0].order.side == OrderSide.BUY

    def test_snapshot_ask_side(self) -> None:
        """Ask entries produce OrderSide.SELL deltas."""
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.enums import OrderSide
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        stub = _make_stub()
        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)

        deltas = stub._parse_orderbook_snapshot(
            "BTC-USD",
            [],
            [self._make_entry("76710.0", "0.3")],
            instrument_id=iid,
            price_precision=1,
            size_precision=5,
            ts_event=0,
            ts_init=0,
        )

        assert deltas.deltas[0].order.side == OrderSide.SELL

    def test_zero_size_produces_delete_action(self) -> None:
        """A level with size=0 generates a ``BookAction.DELETE`` delta."""
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.enums import BookAction
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        stub = _make_stub()
        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)

        deltas = stub._parse_orderbook_snapshot(
            "BTC-USD",
            [self._make_entry("76700.0", "0")],
            [],
            instrument_id=iid,
            price_precision=1,
            size_precision=5,
            ts_event=0,
            ts_init=0,
        )

        assert deltas.deltas[0].action == BookAction.DELETE

    def test_first_delta_has_snapshot_flag(self) -> None:
        """The first delta must carry ``RecordFlag.F_SNAPSHOT``."""
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.enums import RecordFlag
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        stub = _make_stub()
        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)

        deltas = stub._parse_orderbook_snapshot(
            "BTC-USD",
            [self._make_entry("76700.0", "0.5"), self._make_entry("76690.0", "1.0")],
            [self._make_entry("76710.0", "0.3")],
            instrument_id=iid,
            price_precision=1,
            size_precision=5,
            ts_event=0,
            ts_init=0,
        )

        # First delta: snapshot flag set
        assert deltas.deltas[0].flags & RecordFlag.F_SNAPSHOT
        # Subsequent deltas: flag not set
        assert not (deltas.deltas[1].flags & RecordFlag.F_SNAPSHOT)
