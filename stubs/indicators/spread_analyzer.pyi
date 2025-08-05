from collections import deque

from stubs.indicators.base.indicator import Indicator
from stubs.model.data import QuoteTick
from stubs.model.identifiers import InstrumentId

class SpreadAnalyzer(Indicator):
    """
    Provides various spread analysis metrics.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the tick updates.
    capacity : int
        The max length for the internal `QuoteTick` deque (determines averages).

    Raises
    ------
    ValueError
        If `capacity` is not positive (> 0).
    """

    instrument_id: InstrumentId
    capacity: int
    current: float
    average: float

    _spreads: deque

    def __init__(self, instrument_id: InstrumentId, capacity: int) -> None: ...
    def handle_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the analyzer with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        Raises
        ------
        ValueError
            If `tick.instrument_id` does not equal the analyzers instrument ID.

        """
    def _reset(self) -> None: ...
