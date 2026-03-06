import pandas as pd
import pytest

from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.events import OrderAccepted


@pytest.mark.parametrize(
    ("cls", "is_nautilus"),
    [
        (OrderBookDelta, True),
        (TradeTick, True),
        (OrderAccepted, True),
        (BetfairStartingPrice, False),  # BetfairStartingPrice is an adapter specific type
        (BetfairTicker, False),  # BetfairTicker is an adapter specific type
        (pd.DataFrame, False),
    ],
)
def test_is_nautilus_class(cls, is_nautilus):
    # Arrange, Act, Assert
    assert is_nautilus_class(cls=cls) is is_nautilus
