from decimal import Decimal
from typing import Any

import pandas as pd

from nautilus_trader.model.enums import AssetClass
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class OptionSpread(Instrument):

    exchange: str | None
    underlying: str
    strategy_type: str
    activation_ns: int
    expiration_ns: int

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        underlying: str,
        strategy_type: str,
        activation_ns: int,
        expiration_ns: int,
        ts_event: int,
        ts_init: int,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        exchange: str | None = None,
        info: dict[Any, Any] | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    @property
    def activation_utc(self) -> pd.Timestamp: ...
    @property
    def expiration_utc(self) -> pd.Timestamp: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OptionSpread: ...
    @staticmethod
    def to_dict(obj: OptionSpread) -> dict[str, Any]: ...
    @staticmethod
    def from_pyo3(pyo3_instrument: NautilusPyO3OptionSpread) -> OptionSpread: ...