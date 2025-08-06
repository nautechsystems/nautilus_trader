from datetime import datetime
from decimal import Decimal
from typing import Any

from nautilus_trader.core.nautilus_pyo3 import BetSide
from nautilus_trader.model.enums import OrderSide
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class BettingInstrument(Instrument):

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
    def from_dict(values: dict) -> BettingInstrument: ...
    @staticmethod
    def to_dict(obj: BettingInstrument) -> dict[str, Any]: ...
    def notional_value(
        self, quantity: Quantity, price: Price, use_quote_for_inverse: bool = False
    ) -> Money: ...


def make_symbol(
    market_id: str,
    selection_id: int,
    selection_handicap: float,
) -> Symbol: ...
def null_handicap() -> float: ...
def order_side_to_bet_side(order_side: OrderSide) -> BetSide: ...
