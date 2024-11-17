from typing import Dict, List

from nautilus_trader.core.data import Data
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.model.objects import Price

class SyntheticInstrument(Data):
    """
    Represents a synthetic instrument with prices derived from component instruments using a
    formula.

    The `id` for the synthetic will become {symbol}.{SYNTH}.

    Parameters
    ----------
    symbol : Symbol
        The symbol for the synethic instrument.
    price_precision : int
        The price precision for the synthetic instrument.
    components : List[InstrumentId]
        The component instruments for the synthetic instrument.
    formula : str
        The derivation formula for the synthetic instrument.
    ts_event : int
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : int
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `price_precision` is greater than 9.
    OverflowError
        If `price_precision` is negative (< 0).
    ValueError
        If the `components` list does not contain at least 2 instrument IDs.
    ValueError
        If the `formula` is not a valid string.
    ValueError
        If the `formula` is not a valid expression.

    Warnings
    --------
    All component instruments should already be defined and exist in the cache prior to defining
    a new synthetic instrument.
    """

    id: InstrumentId

    def __init__(
        self,
        symbol: Symbol,
        price_precision: int,
        components: List[InstrumentId],
        formula: str,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def components(self) -> List[InstrumentId]: ...
    @property
    def formula(self) -> str: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    def change_formula(self, formula: str) -> None: ...
    def calculate(self, inputs: List[float]) -> Price: ...
    @staticmethod
    def from_dict(values: Dict[str, object]) -> "SyntheticInstrument": ...
    @staticmethod
    def to_dict(obj: "SyntheticInstrument") -> Dict[str, object]: ...
