from datetime import datetime
from decimal import Decimal
from typing import Any

from nautilus_trader.core.nautilus_pyo3 import BetSide
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol

class BettingInstrument(Instrument):
    """
    Represents an instrument in a betting market.
    """

    event_type_id: int
    event_type_name: str
    competition_id: int
    competition_name: str
    event_id: int
    event_name: str
    event_country_code: str
    event_open_date: datetime
    betting_type: str
    market_id: str
    market_name: str
    market_type: str
    market_start_time: datetime
    selection_id: int
    selection_name: str
    selection_handicap: float
    min_price: Price

    def __init__(
        self,
        venue_name: str,
        event_type_id: int,
        event_type_name: str,
        competition_id: int,
        competition_name: str,
        event_id: int,
        event_name: str,
        event_country_code: str,
        event_open_date: datetime,
        betting_type: str,
        market_id: str,
        market_name: str,
        market_start_time: datetime,
        market_type: str,
        selection_id: int,
        selection_name: str,
        currency: str,
        selection_handicap: float,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_notional: Money | None = None,
        min_notional: Money | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        tick_scheme_name: str | None = None,
        info: dict | None = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: dict) -> BettingInstrument:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        BettingInstrument

        """
        ...
    @staticmethod
    def to_dict(obj: BettingInstrument) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    def notional_value(
        self, quantity: Quantity, price: Price, use_quote_for_inverse: bool = False
    ) -> Money: ...


def make_symbol(
    market_id: str,
    selection_id: int,
    selection_handicap: float,
) -> Symbol:
    """
    Make symbol.

    >>> make_symbol(market_id="1.201070830", selection_id=123456, selection_handicap=null_handicap())
    Symbol('1-201070830-123456-None')

    """
    ...


def null_handicap() -> float: ...
def order_side_to_bet_side(order_side: OrderSide) -> BetSide: ...