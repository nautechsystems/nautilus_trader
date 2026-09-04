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
"""
Test position behavior.
"""

from __future__ import annotations

import re
from decimal import Decimal

import pytest
from tests.providers import TestInstrumentProvider

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import CryptoPerpetual
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentClass
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import Money
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderType
from nautilus_trader.model import Position
from nautilus_trader.model import PositionAdjusted
from nautilus_trader.model import PositionAdjustmentType
from nautilus_trader.model import PositionId
from nautilus_trader.model import PositionSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import Symbol
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import VenueOrderId


USD = Currency.from_str("USD")
AUDUSD_SIM = TestInstrumentProvider.audusd_sim()
GBPUSD_SIM = TestInstrumentProvider.gbpusd_sim()


def _make_fill(
    instrument: object = None,
    order_side: object = OrderSide.BUY,
    last_px: object = "1.00001",
    last_qty: object = 100_000,
    position_id: object = "P-123456",
    client_order_id: ClientOrderId = "O-20210410-022422-001-001-1",
    venue_order_id: VenueOrderId = "1",
    trade_id: object = "E-20210410-022422-001-001-1",
    commission: object = "2.00 USD",
    currency: object = None,
    event_id: object = None,
    ts_event: object = 0,
) -> object:
    if instrument is None:
        instrument = AUDUSD_SIM
    if currency is None:
        currency = instrument.quote_currency
    if event_id is None:
        event_id = UUID4()
    return OrderFilled(
        trader_id=TraderId("TESTER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId(client_order_id),
        venue_order_id=VenueOrderId(venue_order_id),
        account_id=AccountId("SIM-000"),
        trade_id=TradeId(trade_id),
        order_side=order_side,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_str(str(last_qty)),
        last_px=Price.from_str(last_px),
        currency=currency,
        liquidity_side=LiquiditySide.TAKER,
        event_id=event_id,
        ts_event=ts_event,
        ts_init=0,
        reconciliation=False,
        position_id=None if position_id is None else PositionId(position_id),
        commission=Money.from_str(commission),
    )


@pytest.fixture
def long_position() -> object:
    """
    Long position.
    """
    fill = _make_fill(order_side=OrderSide.BUY)
    return Position(instrument=AUDUSD_SIM, fill=fill)


@pytest.fixture
def short_position() -> object:
    """
    Short position.
    """
    fill = _make_fill(order_side=OrderSide.SELL)
    return Position(instrument=AUDUSD_SIM, fill=fill)


def test_position_rejects_missing_position_id() -> None:
    """
    Test position construction rejects a fill without a position ID.
    """
    fill = _make_fill(position_id=None)

    with pytest.raises(ValueError, match=re.escape("`fill.position_id` was None")) as exc_info:
        Position(instrument=AUDUSD_SIM, fill=fill)

    assert str(exc_info.value) == "`fill.position_id` was None"


def test_position_rejects_instrument_mismatch() -> None:
    """
    Test position construction rejects an instrument mismatch.
    """
    fill = _make_fill()
    expected_error = (
        "'instrument.id()' value of GBP/USD.SIM was not equal to "
        "'fill.instrument_id' value of AUD/USD.SIM"
    )

    with pytest.raises(ValueError, match=re.escape(expected_error)) as exc_info:
        Position(instrument=GBPUSD_SIM, fill=fill)

    assert str(exc_info.value) == expected_error


def test_order_filled_rejects_unspecified_side_before_position_construction() -> None:
    """
    Test an unspecified side is rejected before position construction.
    """
    with pytest.raises(TypeError) as exc_info:
        _make_fill(order_side=OrderSide.NO_ORDER_SIDE)

    assert str(exc_info.value) == "'None' is not an instance of 'OrderSide'"


@pytest.mark.parametrize(
    ("instrument", "position_id", "expected_error"),
    [
        (
            GBPUSD_SIM,
            "P-123456",
            "'self.instrument_id' value of AUD/USD.SIM was not equal to "
            "'fill.instrument_id' value of GBP/USD.SIM",
        ),
        (AUDUSD_SIM, None, "`fill.position_id` was None"),
        (
            AUDUSD_SIM,
            "P-654321",
            "'self.id' value of P-123456 was not equal to 'fill.position_id' value of P-654321",
        ),
    ],
    ids=["instrument-mismatch", "missing-position-id", "position-mismatch"],
)
def test_position_apply_rejects_invalid_fill_identity_without_mutation(
    instrument: object,
    position_id: object,
    expected_error: str,
) -> None:
    """
    Test invalid fill identity is rejected before position mutation.
    """
    position = Position(instrument=AUDUSD_SIM, fill=_make_fill())
    fill = _make_fill(
        instrument=instrument,
        position_id=position_id,
        trade_id="E-20210410-022422-001-001-2",
    )
    state_before = position.to_dict()

    with pytest.raises(ValueError, match=re.escape(expected_error)) as exc_info:
        position.apply(fill)

    assert str(exc_info.value) == expected_error
    assert position.to_dict() == state_before


def test_position_apply_rejects_duplicate_trade_without_mutation() -> None:
    """
    Test an ordinary duplicate trade is rejected before position mutation.
    """
    fill = _make_fill()
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    state_before = position.to_dict()
    expected_error = "`fill.trade_id` already contained in `trade_ids`"

    with pytest.raises(ValueError, match=re.escape(expected_error)) as exc_info:
        position.apply(fill)

    assert str(exc_info.value) == expected_error
    assert position.to_dict() == state_before


def test_position_long_properties(long_position: object) -> None:
    """
    Test position long properties.
    """
    last = Price.from_str("1.00050")

    assert long_position.instrument_id == AUDUSD_SIM.id
    assert long_position.account_id == AccountId("SIM-000")
    assert long_position.instrument_class == InstrumentClass.SPOT
    assert long_position.is_spot_currency is True
    assert long_position.symbol == AUDUSD_SIM.id.symbol
    assert long_position.venue == AUDUSD_SIM.id.venue
    assert long_position.opening_order_id == ClientOrderId("O-20210410-022422-001-001-1")
    assert long_position.closing_order_id is None
    assert long_position.quantity == Quantity.from_int(100_000)
    assert long_position.peak_qty == Quantity.from_int(100_000)
    assert long_position.signed_qty == 100_000.0
    assert long_position.entry == OrderSide.BUY
    assert long_position.side == PositionSide.LONG
    assert long_position.ts_opened == 0
    assert long_position.ts_last == 0
    assert long_position.duration_ns == 0
    assert long_position.avg_px_open == 1.00001
    assert long_position.event_count == 1
    assert long_position.id == PositionId("P-123456")
    assert long_position.is_long
    assert not long_position.is_short
    assert long_position.is_open
    assert not long_position.is_closed
    assert long_position.realized_return == 0
    assert long_position.realized_pnl == Money(-2.00, USD)
    assert long_position.unrealized_pnl(last) == Money(49.00, USD)
    assert long_position.total_pnl(last) == Money(47.00, USD)
    assert long_position.commissions() == [Money(2.00, USD)]


def test_position_short_properties(short_position: object) -> None:
    """
    Test position short properties.
    """
    last = Price.from_str("1.00050")

    assert short_position.quantity == Quantity.from_int(100_000)
    assert short_position.signed_qty == -100_000.0
    assert short_position.side == PositionSide.SHORT
    assert short_position.avg_px_open == 1.00001
    assert short_position.event_count == 1
    assert short_position.id == PositionId("P-123456")
    assert not short_position.is_long
    assert short_position.is_short
    assert short_position.is_open
    assert not short_position.is_closed
    assert short_position.realized_return == 0
    assert short_position.realized_pnl == Money(-2.00, USD)
    assert short_position.unrealized_pnl(last) == Money(-49.00, USD)
    assert short_position.total_pnl(last) == Money(-51.00, USD)
    assert short_position.commissions() == [Money(2.00, USD)]


def test_position_str_and_repr(long_position: object) -> None:
    """
    Test position str and repr.
    """
    assert str(long_position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"
    assert repr(long_position) == "Position(LONG 100_000 AUD/USD.SIM, id=P-123456)"


def test_position_equality(long_position: object) -> None:
    """
    Test position equality.
    """
    assert long_position == long_position


def test_position_events_and_ids(long_position: object) -> None:
    """
    Test position events and ids.
    """
    assert len(long_position.events()) == 1
    assert long_position.client_order_ids() == [ClientOrderId("O-20210410-022422-001-001-1")]
    assert long_position.venue_order_ids() == [VenueOrderId("1")]
    assert long_position.trade_ids() == [TradeId("E-20210410-022422-001-001-1")]
    assert long_position.last_trade_id == TradeId("E-20210410-022422-001-001-1")


def test_position_to_dict(long_position: object) -> None:
    """
    Test position to dict.
    """
    d = long_position.to_dict()

    assert d["type"] == "Position"
    assert d["instrument_id"] == "AUD/USD.SIM"
    assert d["side"] == "LONG"
    assert d["entry"] == "BUY"
    assert d["quantity"] == "100000"
    assert d["avg_px_open"] == 1.00001
    assert d["realized_pnl"] == "-2.00 USD"


def test_position_partial_fill_long() -> None:
    """
    Test position partial fill long.
    """
    fill = _make_fill(
        order_side=OrderSide.BUY,
        last_qty=50_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00048")

    assert position.quantity == Quantity.from_int(50_000)
    assert position.peak_qty == Quantity.from_int(50_000)
    assert position.side == PositionSide.LONG
    assert position.avg_px_open == 1.00001
    assert position.is_open
    assert position.unrealized_pnl(last) == Money(23.50, USD)
    assert repr(position) == "Position(LONG 50_000 AUD/USD.SIM, id=P-123456)"


def test_position_close_long() -> None:
    """
    Test position close long.
    """
    fill1 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00001",
        commission="3.00 USD",
        ts_event=1_000_000_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)

    fill2 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00011",
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
        commission="0.00 USD",
        ts_event=2_000_000_000,
    )
    position.apply(fill2)
    last = Price.from_str("1.00050")

    assert position.quantity == Quantity.zero()
    assert position.side == PositionSide.FLAT
    assert position.ts_opened == 1_000_000_000
    assert position.duration_ns == 1_000_000_000
    assert position.avg_px_open == 1.00001
    assert position.avg_px_close == 1.00011
    assert position.ts_closed == 2_000_000_000
    assert position.event_count == 2
    assert not position.is_long
    assert not position.is_short
    assert not position.is_open
    assert position.is_closed
    assert position.realized_pnl == Money(7.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(7.00, USD)
    assert repr(position) == "Position(FLAT AUD/USD.SIM, id=P-123456)"


def test_position_close_short() -> None:
    """
    Test position close short.
    """
    fill1 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00010",
        ts_event=1_000_000_000,
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)

    fill2 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00000",
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
        ts_event=2_000_000_000,
    )
    position.apply(fill2)

    assert position.side == PositionSide.FLAT
    assert position.is_closed
    assert position.avg_px_open == 1.00010
    assert position.avg_px_close == 1.00000


def test_position_partial_fills_then_close() -> None:
    """
    Test position partial fills then close.
    """
    fill1 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00001",
        last_qty=50_000,
        trade_id="E1",
    )
    fill2 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00002",
        last_qty=50_000,
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    position.apply(fill2)

    assert position.quantity == Quantity.from_int(100_000)
    assert position.side == PositionSide.SHORT
    assert position.avg_px_open == 1.000015
    assert position.event_count == 2
    assert position.commissions() == [Money(4.00, USD)]

    fill3 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00001",
        client_order_id="O-20210410-022422-001-001-3",
        venue_order_id="3",
        trade_id="E3",
    )
    position.apply(fill3)

    assert position.side == PositionSide.FLAT
    assert position.is_closed


def test_position_no_change() -> None:
    """
    Test position no change.
    """
    fill1 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.0",
        last_qty=50_000,
        trade_id="E1",
    )
    fill2 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.0",
        last_qty=50_000,
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    position.apply(fill2)
    last = Price.from_str("1.00050")

    assert position.side == PositionSide.FLAT
    assert position.is_closed
    assert position.avg_px_open == 1.0
    assert position.avg_px_close == 1.0
    assert position.realized_return == 0
    assert position.realized_pnl == Money(-4.00, USD)
    assert position.unrealized_pnl(last) == Money(0, USD)
    assert position.total_pnl(last) == Money(-4.00, USD)
    assert position.commissions() == [Money(4.00, USD)]


def test_position_multiple_fills_long() -> None:
    """
    Test position multiple fills long.
    """
    fill1 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00000",
        last_qty=50_000,
        trade_id="E1",
    )
    fill2 = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00010",
        last_qty=50_000,
        client_order_id="O-20210410-022422-001-001-2",
        venue_order_id="2",
        trade_id="E2",
    )
    fill3 = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00020",
        client_order_id="O-20210410-022422-001-001-3",
        venue_order_id="3",
        trade_id="E3",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill1)
    position.apply(fill2)

    assert position.quantity == Quantity.from_int(100_000)
    assert position.avg_px_open == 1.00005
    assert position.is_long
    assert position.is_open

    position.apply(fill3)

    assert position.side == PositionSide.FLAT
    assert position.is_closed
    assert position.avg_px_close == 1.00020


def test_position_pnl_long_win() -> None:
    """
    Test position pnl long win.
    """
    fill = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00000",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00010")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(10.00, USD)


def test_position_pnl_long_loss() -> None:
    """
    Test position pnl long loss.
    """
    fill = _make_fill(
        order_side=OrderSide.BUY,
        last_px="1.00010",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00000")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(-10.00, USD)


def test_position_pnl_short_win() -> None:
    """
    Test position pnl short win.
    """
    fill = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00010",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00000")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(10.00, USD)


def test_position_pnl_short_loss() -> None:
    """
    Test position pnl short loss.
    """
    fill = _make_fill(
        order_side=OrderSide.SELL,
        last_px="1.00000",
        commission="0.00 USD",
    )
    position = Position(instrument=AUDUSD_SIM, fill=fill)
    last = Price.from_str("1.00010")

    pnl = position.unrealized_pnl(last)

    assert pnl == Money(-10.00, USD)


def test_position_inverse_pnl_and_notional_value() -> None:
    """
    Test position inverse pnl and notional value.
    """
    instrument = _inverse_perpetual()
    fill = _make_fill(
        instrument=instrument,
        order_side=OrderSide.SELL,
        last_px="10000.00",
        last_qty=100_000,
        commission="0.00000000 BTC",
        currency=instrument.settlement_currency,
    )
    position = Position(instrument=instrument, fill=fill)

    pnl = position.calculate_pnl(
        avg_px_open=10_000.00,
        avg_px_close=11_000.00,
        quantity=Quantity.from_int(100_000),
    )

    assert pnl == Money(-0.90909091, Currency.from_str("BTC"))
    assert position.unrealized_pnl(Price.from_str("11000.00")) == pnl
    assert position.notional_value(Price.from_str("11000.00")) == Money(
        9.09090909,
        Currency.from_str("BTC"),
    )


@pytest.mark.parametrize(
    ("opening_side", "flipping_side", "expected_side", "expected_quantity"),
    [
        (OrderSide.SELL, OrderSide.BUY, PositionSide.LONG, Quantity.from_str("0.499")),
        (OrderSide.BUY, OrderSide.SELL, PositionSide.SHORT, Quantity.from_str("0.501")),
    ],
)
def test_position_flip_applies_full_base_currency_commission(
    opening_side: object,
    flipping_side: object,
    expected_side: object,
    expected_quantity: object,
) -> None:
    """
    Test position flip applies full base currency commission.
    """
    instrument = TestInstrumentProvider.btcusdt_binance()
    opening_fill = _make_fill(
        instrument=instrument,
        order_side=opening_side,
        last_px="50000.00",
        last_qty="1.0",
        commission="0.00 USDT",
    )
    position = Position(instrument=instrument, fill=opening_fill)
    flipping_fill = _make_fill(
        instrument=instrument,
        order_side=flipping_side,
        last_px="50000.00",
        last_qty="1.5",
        client_order_id="O-2",
        venue_order_id="2",
        trade_id="E-2",
        commission="0.001 BTC",
        currency=instrument.base_currency,
        event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2"),
    )

    position.apply(flipping_fill)

    assert position.side == expected_side
    assert position.quantity == expected_quantity
    assert position.adjustments() == [
        PositionAdjusted(
            trader_id=flipping_fill.trader_id,
            strategy_id=flipping_fill.strategy_id,
            instrument_id=flipping_fill.instrument_id,
            position_id=PositionId("P-123456"),
            account_id=flipping_fill.account_id,
            adjustment_type=PositionAdjustmentType.COMMISSION,
            quantity_change=Decimal("-0.001"),
            pnl_change=None,
            reason=str(flipping_fill.client_order_id),
            event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff3"),
            ts_event=flipping_fill.ts_event,
            ts_init=flipping_fill.ts_init,
        ),
    ]


def test_position_adjustment_dict_roundtrip_preserves_optional_values() -> None:
    """
    Test position adjustment dict roundtrip preserves optional values.
    """
    adjustment = PositionAdjusted(
        trader_id=TraderId("TESTER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=AUDUSD_SIM.id,
        position_id=PositionId("P-123456"),
        account_id=AccountId("SIM-000"),
        adjustment_type=PositionAdjustmentType.FUNDING,
        quantity_change=None,
        pnl_change=Money(-5.50, USD),
        reason="funding_payment",
        event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2"),
        ts_event=1_000_000_000,
        ts_init=2_000_000_000,
    )

    values = adjustment.to_dict()
    restored = PositionAdjusted.from_dict(values)

    assert values == {
        "type": "PositionAdjusted",
        "trader_id": "TESTER-001",
        "strategy_id": "S-001",
        "instrument_id": "AUD/USD.SIM",
        "position_id": "P-123456",
        "account_id": "SIM-000",
        "adjustment_type": "FUNDING",
        "quantity_change": None,
        "pnl_change": "-5.50 USD",
        "reason": "funding_payment",
        "event_id": "91762096-b188-49ea-8562-8d8a4cc22ff2",
        "ts_event": 1_000_000_000,
        "ts_init": 2_000_000_000,
    }
    assert restored == adjustment


def test_position_purge_events_removes_matching_adjustment() -> None:
    """
    Test position purge events removes matching adjustment.
    """
    instrument = TestInstrumentProvider.btcusdt_binance()
    first = _make_fill(
        instrument=instrument,
        last_px="50000.00",
        last_qty="1.0",
        client_order_id="O-1",
        venue_order_id="1",
        trade_id="E-1",
        commission="0.001 BTC",
        currency=instrument.base_currency,
    )
    second = _make_fill(
        instrument=instrument,
        last_px="51000.00",
        last_qty="2.0",
        client_order_id="O-2",
        venue_order_id="2",
        trade_id="E-2",
        commission="0.002 BTC",
        currency=instrument.base_currency,
    )
    position = Position(instrument=instrument, fill=first)
    position.apply(second)

    position.purge_events_for_order(first.client_order_id)

    assert position.events() == [second]
    assert [adjustment.quantity_change for adjustment in position.adjustments()] == [
        Decimal("-0.002"),
    ]


def _inverse_perpetual() -> object:
    return CryptoPerpetual(
        instrument_id=InstrumentId.from_str("XBTUSD-PERP.BITMEX"),
        raw_symbol=Symbol("XBTUSD"),
        base_currency=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("BTC"),
        is_inverse=True,
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
