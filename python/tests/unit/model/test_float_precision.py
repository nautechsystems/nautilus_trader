# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test float conversion precision boundaries.
"""

from collections.abc import Callable
from decimal import Decimal

import pytest
from tests.providers import TestInstrumentProvider

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountBalance
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountState
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarginAccount
from nautilus_trader.model import MarginBalance
from nautilus_trader.model import Money
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import OrderType
from nautilus_trader.model import OwnBookOrder
from nautilus_trader.model import Position
from nautilus_trader.model import PositionId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import VenueOrderId


AUDUSD_SIM = TestInstrumentProvider.audusd_sim()
USD = Currency.from_str("USD")
FLOAT_PRECISION = 16
ABOVE_FLOAT_PRECISION_CASES = [
    pytest.param(17, id="precision-17"),
    pytest.param(18, id="precision-18"),
]
BALANCE_PRECISION_CASES = [
    pytest.param(16, id="precision-16"),
    pytest.param(17, id="precision-17"),
    pytest.param(18, id="precision-18"),
]


def _fixed(value: str, precision: int) -> str:
    whole, _, fraction = value.partition(".")
    return f"{whole}.{fraction.ljust(precision, '0')}"


def _price(value: str, precision: int) -> Price:
    return Price.from_str(_fixed(value, precision))


def _quantity(value: str, precision: int) -> Quantity:
    return Quantity.from_str(_fixed(value, precision))


def _currency(precision: int) -> Currency:
    return Currency(
        code=f"TST{precision}",
        precision=precision,
        iso4217=0,
        name=f"Test {precision}dp",
        currency_type=CurrencyType.CRYPTO,
    )


def _money(value: str, precision: int) -> Money:
    return Money.from_decimal(Decimal(value), _currency(precision))


def _own_order(side: OrderSide, price: Price, size: Quantity) -> OwnBookOrder:
    return OwnBookOrder(
        trader_id=TraderId("TRADER-001"),
        client_order_id=ClientOrderId("O-001"),
        side=side,
        price=price,
        size=size,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        status=OrderStatus.ACCEPTED,
        ts_last=0,
        ts_accepted=0,
        ts_submitted=0,
        ts_init=0,
    )


def _book_order(
    side: OrderSide,
    price: str,
    price_precision: int,
    size_precision: int,
    order_id: int = 1,
) -> BookOrder:
    return BookOrder(
        side,
        _price(price, price_precision),
        _quantity("2", size_precision),
        order_id,
    )


def _book(price_precision: int, size_precision: int) -> OrderBook:
    book = OrderBook(instrument_id=AUDUSD_SIM.id, book_type=BookType.L3_MBO)
    bid = _book_order(OrderSide.BUY, "1.5", price_precision, size_precision, 1)
    ask = _book_order(OrderSide.SELL, "2.5", price_precision, size_precision, 2)
    book.add(bid, flags=0, sequence=1, ts_event=1)
    book.add(ask, flags=0, sequence=2, ts_event=2)
    return book


def _fill(
    order_side: OrderSide,
    last_px: Price,
    last_qty: Quantity,
    trade_id: str,
) -> OrderFilled:
    return OrderFilled(
        trader_id=TraderId("TESTER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=AUDUSD_SIM.id,
        client_order_id=ClientOrderId(f"O-{trade_id}"),
        venue_order_id=VenueOrderId(f"V-{trade_id}"),
        account_id=AccountId("SIM-001"),
        trade_id=TradeId(trade_id),
        order_side=order_side,
        order_type=OrderType.MARKET,
        last_qty=last_qty,
        last_px=last_px,
        currency=USD,
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        position_id=PositionId("P-001"),
        commission=None,
    )


def _long_position() -> Position:
    fill = _fill(OrderSide.BUY, Price.from_str("1.50000"), Quantity.from_int(2), "T-1")
    return Position(instrument=AUDUSD_SIM, fill=fill)


def _margin_account() -> MarginAccount:
    state = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.MARGIN,
        balances=[
            AccountBalance(
                total=Money.from_str("1000.00 USD"),
                locked=Money.from_str("0.00 USD"),
                free=Money.from_str("1000.00 USD"),
            ),
        ],
        margins=[],
        is_reported=True,
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
        base_currency=USD,
    )
    return MarginAccount(state, calculate_account_state=False)


def _position_open(precision: int) -> object:
    fill = _fill(OrderSide.BUY, _price("1.5", precision), _quantity("2", precision), "T-1")
    return Position(instrument=AUDUSD_SIM, fill=fill).avg_px_open


def _position_apply(precision: int) -> object:
    position = _long_position()
    fill = _fill(OrderSide.BUY, _price("2.5", precision), _quantity("2", precision), "T-2")
    position.apply(fill)
    return position.avg_px_open


def _position_values() -> dict:
    values = _long_position().to_dict()

    # `to_dict` and `from_dict` use different layouts
    values["id"] = values.pop("position_id")
    values["commissions"] = {}
    values["is_currency_pair"] = True
    values["instrument_class"] = "SPOT"
    return values


def _position_from_dict_quantity(precision: int) -> object:
    values = _position_values()
    values["quantity"] = _fixed("2", precision)
    return Position.from_dict(values).quantity


def _position_from_dict_fill(precision: int) -> object:
    values = _position_values()
    values["events"][0]["last_px"] = _fixed("1.5", precision)
    return Position.from_dict(values).quantity


def _margin_calculate_pnls(precision: int) -> object:
    fill = _fill(OrderSide.SELL, _price("2.5", precision), _quantity("2", precision), "T-2")
    return _margin_account().calculate_pnls(AUDUSD_SIM, fill, _long_position())


BOOK_DIMENSIONS = {"price": (18, 16), "size": (16, 18)}
BOOK_VARIANTS = [
    (
        "book-order-exposure",
        lambda pp, sp: _book_order(OrderSide.BUY, "1.5", pp, sp).exposure(),
        3.0,
        {"price", "size"},
    ),
    (
        "book-order-signed-size",
        lambda pp, sp: _book_order(OrderSide.SELL, "1.5", pp, sp).signed_size(),
        -2.0,
        {"size"},
    ),
    (
        "own-book-order-exposure",
        lambda pp, sp: _own_order(OrderSide.BUY, _price("1.5", pp), _quantity("2", sp)).exposure(),
        3.0,
        {"price", "size"},
    ),
    (
        "own-book-order-signed-size",
        lambda pp, sp: _own_order(
            OrderSide.SELL,
            _price("1.5", pp),
            _quantity("2", sp),
        ).signed_size(),
        -2.0,
        {"size"},
    ),
    ("book-level-size", lambda pp, sp: _book(pp, sp).bids()[0].size(), 2.0, {"size"}),
    (
        "book-level-exposure",
        lambda pp, sp: _book(pp, sp).bids()[0].exposure(),
        3.0,
        {"price", "size"},
    ),
    ("order-book-spread", lambda pp, sp: _book(pp, sp).spread(), 1.0, {"price"}),
    ("order-book-midpoint", lambda pp, sp: _book(pp, sp).midpoint(), 2.0, {"price"}),
    (
        "order-book-quantity-for-price",
        lambda pp, sp: _book(pp, sp).get_quantity_for_price(_price("2.5", pp), OrderSide.BUY),
        2.0,
        {"size"},
    ),
    (
        "order-book-avg-px-qty-for-exposure",
        lambda pp, sp: _book(pp, sp).get_avg_px_qty_for_exposure(
            _quantity("5", FLOAT_PRECISION),
            OrderSide.BUY,
        ),
        (2.5, 2.0, 2.5),
        {"price", "size"},
    ),
]
BOOK_CONVERTED_CASES = [
    pytest.param(call, *BOOK_DIMENSIONS[dimension], id=f"{name}-{dimension}")
    for name, call, _, converted in BOOK_VARIANTS
    for dimension in sorted(converted)
]
BOOK_UNCONVERTED_CASES = [
    pytest.param(call, *BOOK_DIMENSIONS[dimension], expected, id=f"{name}-{dimension}")
    for name, call, expected, converted in BOOK_VARIANTS
    for dimension in sorted(BOOK_DIMENSIONS.keys() - converted)
]
FLOAT_VARIANTS = [
    ("price-float", lambda p: float(_price("1.5", p)), 1.5),
    ("price-as-double", lambda p: _price("1.5", p).as_double(), 1.5),
    ("quantity-float", lambda p: float(_quantity("1.5", p)), 1.5),
    ("quantity-as-double", lambda p: _quantity("1.5", p).as_double(), 1.5),
    ("money-float", lambda p: float(_money("1.5", p)), 1.5),
    ("money-as-double", lambda p: _money("1.5", p).as_double(), 1.5),
    *(
        (name, lambda p, call=call: call(p, p), expected)
        for name, call, expected, _ in BOOK_VARIANTS
    ),
    ("position-new", _position_open, 1.5),
    ("position-apply", _position_apply, 2.0),
    (
        "position-unrealized-pnl",
        lambda p: _long_position().unrealized_pnl(_price("2.5", p)),
        Money.from_str("2.00 USD"),
    ),
    (
        "position-total-pnl",
        lambda p: _long_position().total_pnl(_price("2.5", p)),
        Money.from_str("2.00 USD"),
    ),
    (
        "position-calculate-pnl",
        lambda p: _long_position().calculate_pnl(1.5, 2.5, _quantity("2", p)),
        Money.from_str("2.00 USD"),
    ),
    (
        "position-from-dict-quantity",
        _position_from_dict_quantity,
        Quantity.from_str("2.0000000000000000"),
    ),
    ("position-from-dict-fill", _position_from_dict_fill, Quantity.from_int(2)),
    ("margin-account-calculate-pnls", _margin_calculate_pnls, [Money.from_str("2.00 USD")]),
]
FLOAT_SUCCESS_CASES = [
    pytest.param(call, expected, id=name) for name, call, expected in FLOAT_VARIANTS
]
FLOAT_CALL_CASES = [pytest.param(call, id=name) for name, call, _ in FLOAT_VARIANTS]


@pytest.mark.parametrize(("call", "expected"), FLOAT_SUCCESS_CASES)
def test_float_variant_succeeds_at_float_precision(
    call: Callable[[int], object],
    expected: object,
) -> None:
    """
    Test float variant succeeds at float precision.
    """
    result = call(FLOAT_PRECISION)

    assert result == expected


@pytest.mark.parametrize("precision", ABOVE_FLOAT_PRECISION_CASES)
@pytest.mark.parametrize("call", FLOAT_CALL_CASES)
def test_float_variant_rejects_precision_above_float_precision(
    call: Callable[[int], object],
    precision: int,
) -> None:
    """
    Test float variant rejects precision above float precision.
    """
    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        call(precision)

    assert str(exc_info.value) == (
        f"Fixed-point precision {precision} exceeds maximum float precision 16"
    )


@pytest.mark.parametrize(("call", "price_precision", "size_precision"), BOOK_CONVERTED_CASES)
def test_book_variant_rejects_converted_value_above_float_precision(
    call: Callable[[int, int], object],
    price_precision: int,
    size_precision: int,
) -> None:
    """
    Test book variant rejects converted value above float precision.
    """
    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        call(price_precision, size_precision)

    assert str(exc_info.value) == "Fixed-point precision 18 exceeds maximum float precision 16"


@pytest.mark.parametrize(
    ("call", "price_precision", "size_precision", "expected"),
    BOOK_UNCONVERTED_CASES,
)
def test_book_variant_accepts_unconverted_value_above_float_precision(
    call: Callable[[int, int], object],
    price_precision: int,
    size_precision: int,
    expected: object,
) -> None:
    """
    Test book variant accepts unconverted value above float precision.
    """
    result = call(price_precision, size_precision)

    assert result == expected


@pytest.mark.parametrize("precision", ABOVE_FLOAT_PRECISION_CASES)
def test_avg_px_qty_for_exposure_rejects_target_above_float_precision(precision: int) -> None:
    """
    Test avg px qty for exposure rejects target above float precision.
    """
    book = _book(FLOAT_PRECISION, FLOAT_PRECISION)

    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        book.get_avg_px_qty_for_exposure(_quantity("5", precision), OrderSide.BUY)

    assert str(exc_info.value) == (
        f"Fixed-point precision {precision} exceeds maximum float precision 16"
    )


def test_one_sided_book_spread_and_midpoint_skip_float_conversion() -> None:
    """
    Test one-sided book spread and midpoint skip float conversion.
    """
    book = OrderBook(instrument_id=AUDUSD_SIM.id, book_type=BookType.L3_MBO)
    book.add(_book_order(OrderSide.BUY, "1.5", 18, 18), flags=0, sequence=1, ts_event=1)

    spread = book.spread()
    midpoint = book.midpoint()

    assert spread is None
    assert midpoint is None


@pytest.mark.parametrize("precision", BALANCE_PRECISION_CASES)
def test_account_balance_to_dict_formats_exact_amounts(precision: int) -> None:
    """
    Test account balance to dict formats exact amounts.
    """
    total = _fixed("1234567.123456789012345678"[: 8 + precision], precision)
    balance = AccountBalance(
        total=_money(total, precision),
        locked=_money("0", precision),
        free=_money(total, precision),
    )

    values = balance.to_dict()

    assert values == {
        "type": "AccountBalance",
        "total": total,
        "locked": _fixed("0.", precision),
        "free": total,
        "currency": f"TST{precision}",
    }


@pytest.mark.parametrize("precision", BALANCE_PRECISION_CASES)
def test_margin_balance_to_dict_formats_exact_amounts(precision: int) -> None:
    """
    Test margin balance to dict formats exact amounts.
    """
    initial = _fixed("1234567.123456789012345678"[: 8 + precision], precision)
    maintenance = _fixed("7654321.876543210987654321"[: 8 + precision], precision)
    instrument_id = InstrumentId.from_str("AUD/USD.SIM")
    balance = MarginBalance(
        _money(initial, precision),
        _money(maintenance, precision),
        instrument_id,
    )

    values = balance.to_dict()

    assert values == {
        "type": "MarginBalance",
        "initial": initial,
        "maintenance": maintenance,
        "currency": f"TST{precision}",
        "instrument_id": "AUD/USD.SIM",
    }
