from typing import Optional

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


def make_symbol(
    event_id: str, market_id: str, selection_id: str, selection_handicap: Optional[float]
) -> Symbol:
    def _clean(s):
        return str(s).replace(" ", "").replace(":", "")

    value: str = "".join(
        [_clean(k) for k in (event_id, market_id, selection_id, selection_handicap)]
    )
    assert len(value) <= 32, f"Symbol too long ({len(value)}): '{value}'"
    return Symbol(value)


def betfair_instrument_id(
    event_id: str, market_id: str, selection_id: str, selection_handicap: Optional[float]
) -> InstrumentId:
    symbol = make_symbol(event_id, market_id, selection_id, selection_handicap)
    return InstrumentId(symbol=symbol, venue=BETFAIR_VENUE)
