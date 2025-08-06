from decimal import Decimal
from typing import Any

import pandas as pd

from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class CryptoFuture(Instrument):

    underlying: Currency
    settlement_currency: Currency
    activation_ns: int
    expiration_ns: int

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        underlying: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        is_inverse: bool,
        activation_ns: int,
        expiration_ns: int,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        ts_event: int,
        ts_init: int,
        multiplier: Quantity = ...,
        lot_size: Quantity = ...,
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
    def __repr__(self) -> str: ...
    def get_base_currency(self) -> Currency: ...
    def get_settlement_currency(self) -> Currency: ...
    @property
    def activation_utc(self) -> pd.Timestamp: ...
    @property
    def expiration_utc(self) -> pd.Timestamp: ...
    @staticmethod
    def from_dict(values: dict) -> CryptoFuture: ...
    @staticmethod
    def to_dict(obj: CryptoFuture) -> dict[str, Any]: ...
    @staticmethod
    def from_pyo3(pyo3_instrument: NautilusPyO3CryptoFuture) -> CryptoFuture: ...
