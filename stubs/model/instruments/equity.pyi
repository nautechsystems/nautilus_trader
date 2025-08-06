from decimal import Decimal
from typing import Any

from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class Equity(Instrument):

    isin: str | None

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        lot_size: Quantity,
        ts_event: int,
        ts_init: int,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        isin: str | None = None,
        info: dict = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: dict) -> Instrument: ...
    @staticmethod
    def to_dict(obj: Instrument) -> dict[str, Any]: ...
    @staticmethod
    def from_pyo3(pyo3_instrument: Any) -> Equity: ...
