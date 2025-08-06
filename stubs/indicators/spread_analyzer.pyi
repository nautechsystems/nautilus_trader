from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import QuoteTick
from stubs.model.identifiers import InstrumentId

class SpreadAnalyzer(Indicator):

    instrument_id: InstrumentId
    capacity: int
    current: float
    average: float

    _spreads: deque

    def __init__(self, instrument_id: InstrumentId, capacity: int) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def _reset(self) -> None: ...
