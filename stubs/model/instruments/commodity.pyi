from decimal import Decimal
from typing import Any

from nautilus_trader.model.enums import AssetClass
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class Commodity(Instrument):

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        quote_currency: Currency,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        ts_event: int,
        ts_init: int,
        base_currency: Currency | None = None,
        lot_size: Quantity | None = None,
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
        info: dict[Any, Any] | None = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: dict[Any, Any]) -> Commodity: ...
    @staticmethod
    def to_dict(obj: Commodity) -> dict[str, Any]: ...
    @staticmethod
    def from_pyo3(pyo3_instrument: Any) -> Commodity: ...
