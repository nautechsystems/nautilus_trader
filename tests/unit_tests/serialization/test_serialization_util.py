import pandas as pd
import pytest

from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.serialization.arrow.util import camel_to_snake_case
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import is_nautilus_class


@pytest.mark.parametrize(
    "cls, is_nautilus",
    [
        (OrderBookData, True),
        (TradeTick, True),
        (OrderAccepted, True),
        (pd.DataFrame, False),
    ],
)
def test_is_custom_data(cls, is_nautilus):
    assert is_nautilus_class(cls) is is_nautilus


@pytest.mark.parametrize(
    "s, expected",
    [
        ("BSPOrderBookDelta", "bsp_order_book_delta"),
        ("OrderBookData", "order_book_data"),
        ("TradeTick", "trade_tick"),
    ],
)
def test_camel_to_snake_case(s, expected):
    assert camel_to_snake_case(s) == expected


@pytest.mark.parametrize(
    "s, expected",
    [
        ("Instrument\\ID:hello", "Instrument-ID-hello"),
    ],
)
def test_clean_key(s, expected):
    assert clean_key(s) == expected


@pytest.mark.parametrize(
    "s, expected",
    [
        (TradeTick, "trade_tick"),
        (OrderBookData, "order_book_data"),
        (pd.DataFrame, "genericdata_data_frame"),
    ],
)
def test_class_to_filename(s, expected):
    assert class_to_filename(s) == expected
