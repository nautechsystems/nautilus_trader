from decimal import Decimal
from typing import Any

from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class CryptoPerpetual(Instrument):

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        is_inverse: bool,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        ts_event: int,
        ts_init: int,
        multiplier: Quantity = ...,
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
        info: dict = ...,
    ) -> None: ...
    def get_base_currency(self) -> Currency: ...
    def get_settlement_currency(self) -> Currency: ...
    @staticmethod
    def from_dict(values: dict) -> CryptoPerpetual: ...
    @staticmethod
    def to_dict(obj: CryptoPerpetual) -> dict[str, Any]: ...
    @staticmethod
    def from_pyo3(pyo3_instrument: Any) -> CryptoPerpetual: ...
