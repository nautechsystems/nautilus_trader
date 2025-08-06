from typing import Any

from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class IndexInstrument(Instrument):

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        currency: Currency,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        ts_event: int,
        ts_init: int,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> Instrument: ...
    @staticmethod
    def to_dict(obj: Instrument) -> dict[str, Any]: ...
    @staticmethod
    def from_pyo3(pyo3_instrument: Any) -> IndexInstrument: ...
