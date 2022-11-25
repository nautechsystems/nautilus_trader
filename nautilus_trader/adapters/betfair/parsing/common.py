from typing import Optional

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


def make_symbol(
    market_id: str,
    selection_id: str,
    selection_handicap: Optional[str],
) -> Symbol:
    """
    Make symbol

    >>> make_symbol(market_id="1.201070830", selection_id="123456", selection_handicap=None)
    Symbol('1.201070830|123456|None')
    """

    def _clean(s):
        return str(s).replace(" ", "").replace(":", "")

    value: str = "|".join(
        [_clean(k) for k in (market_id, selection_id, selection_handicap)],
    )
    assert len(value) <= 32, f"Symbol too long ({len(value)}): '{value}'"
    return Symbol(value)


def betfair_instrument_id(
    market_id: str,
    selection_id: str,
    selection_handicap: Optional[str],
) -> InstrumentId:
    """
    Create an instrument id from betfair fields

    >>> betfair_instrument_id(market_id="1.201070830", selection_id="123456", selection_handicap=None)
    InstrumentId('1.201070830|123456|None.BETFAIR')

    """
    PyCondition.not_empty(market_id, "market_id")
    symbol = make_symbol(market_id, selection_id, selection_handicap)
    return InstrumentId(symbol=symbol, venue=BETFAIR_VENUE)
