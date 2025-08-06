from decimal import Decimal
from typing import Any

from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import InstrumentClass
from stubs.core.data import Data
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.identifiers import Venue
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.tick_scheme.base import TickScheme

EXPIRING_INSTRUMENT_TYPES: set[InstrumentClass]

class Instrument(Data):

    id: InstrumentId
    raw_symbol: Symbol
    asset_class: AssetClass
    instrument_class: InstrumentClass
    quote_currency: Currency
    is_inverse: bool
    price_precision: int
    price_increment: Price
    tick_scheme_name: str | None
    size_precision: int
    size_increment: Quantity
    multiplier: Quantity
    lot_size: Quantity | None
    max_quantity: Quantity | None
    min_quantity: Quantity | None
    max_notional: Money | None
    min_notional: Money | None
    max_price: Price | None
    min_price: Price | None
    margin_init: Decimal
    margin_maint: Decimal
    maker_fee: Decimal
    taker_fee: Decimal
    info: dict[str, Any] | None
    ts_event: int
    ts_init: int

    _min_price_increment_precision: int
    _min_size_increment_precision: int
    _increment_pow10: float
    _tick_scheme: TickScheme | None # This can be None if not initialized

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        instrument_class: InstrumentClass,
        quote_currency: Currency,
        is_inverse: bool,
        price_precision: int,
        size_precision: int,
        size_increment: Quantity,
        multiplier: Quantity,
        margin_init: Decimal,
        margin_maint: Decimal,
        maker_fee: Decimal,
        taker_fee: Decimal,
        ts_event: int,
        ts_init: int,
        price_increment: Price | None = None,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_notional: Money | None = None,
        min_notional: Money | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
        tick_scheme_name: str | None = None,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    def __eq__(self, other: Instrument) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def base_from_dict(values: dict[str, Any]) -> Instrument: ...
    @staticmethod
    def base_to_dict(obj: Instrument) -> dict[str, Any]: ...
    @property
    def symbol(self) -> Symbol: ...
    @property
    def venue(self) -> Venue: ...
    def get_base_currency(self) -> Currency | None: ...
    def get_settlement_currency(self) -> Currency: ...
    def get_cost_currency(self) -> Currency: ...
    def make_price(self, value) -> Price: ...
    def next_bid_price(self, value: float, num_ticks: int = 0) -> Price: ...
    def next_ask_price(self, value: float, num_ticks: int = 0) -> Price: ...
    def next_bid_prices(self, value: float, num_ticks: int = 100) -> list[Decimal]: ...
    def next_ask_prices(self, value: float, num_ticks: int = 100) -> list[Decimal]: ...
    def make_qty(self, value, round_down: bool = False) -> Quantity: ...
    def notional_value(
        self,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool = False,
    ) -> Money: ...
    def calculate_base_quantity(
        self,
        quantity: Quantity,
        last_px: Price,
    ) -> Quantity: ...

def instruments_from_pyo3(pyo3_instruments: list[Any]) -> list[Instrument]: ...
