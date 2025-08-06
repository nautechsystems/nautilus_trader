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

class BinaryOption(Instrument):

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        activation_ns: int,
        expiration_ns: int,
        ts_event: int,
        ts_init: int,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        outcome: str | None = None,
        description: str | None = None,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    @property
    def activation_utc(self) -> pd.Timestamp: ...
    @property
    def expiration_utc(self) -> pd.Timestamp: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> BinaryOption: ...
    @staticmethod
    def to_dict(obj: BinaryOption) -> dict[str, Any]: ...
