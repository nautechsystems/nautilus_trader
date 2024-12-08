from decimal import Decimal
from typing import Dict, List, Optional, Set

from nautilus_trader.core.data import Data
from nautilus_trader.core.model import AssetClass, InstrumentClass
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.model.objects import Currency, Money, Price, Quantity

EXPIRING_INSTRUMENT_TYPES: Set[InstrumentClass]

class Instrument(Data):
    id: InstrumentId
    raw_symbol: Symbol
    asset_class: AssetClass
    instrument_class: InstrumentClass
    quote_currency: Currency
    is_inverse: bool
    price_precision: int
    size_precision: int
    price_increment: Price
    size_increment: Quantity
    multiplier: Quantity
    lot_size: Optional[Quantity]
    max_quantity: Optional[Quantity]
    min_quantity: Optional[Quantity]
    max_notional: Optional[Money]
    min_notional: Optional[Money]
    max_price: Optional[Price]
    min_price: Optional[Price]
    margin_init: Decimal
    margin_maint: Decimal
    maker_fee: Decimal
    taker_fee: Decimal
    tick_scheme_name: Optional[str]
    info: Optional[Dict[str, object]]
    ts_event: int
    ts_init: int

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
        price_increment: Optional[Price] = None,
        lot_size: Optional[Quantity] = None,
        max_quantity: Optional[Quantity] = None,
        min_quantity: Optional[Quantity] = None,
        max_notional: Optional[Money] = None,
        min_notional: Optional[Money] = None,
        max_price: Optional[Price] = None,
        min_price: Optional[Price] = None,
        tick_scheme_name: Optional[str] = None,
        info: Optional[Dict[str, object]] = None,
    ) -> None: ...
    @property
    def symbol(self) -> Symbol: ...
    @property
    def venue(self) -> str: ...
    def get_base_currency(self) -> Optional[Currency]: ...
    def get_settlement_currency(self) -> Currency: ...
    def make_price(self, value: float | int | str | Decimal) -> Price: ...
    def next_bid_price(self, value: float, num_ticks: int = 0) -> Price: ...
    def next_ask_price(self, value: float, num_ticks: int = 0) -> Price: ...
    def make_qty(self, value: float | int | str | Decimal) -> Quantity: ...
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
    @staticmethod
    def base_from_dict(values: Dict[str, object]) -> "Instrument": ...
    @staticmethod
    def base_to_dict(obj: "Instrument") -> Dict[str, object]: ...

def instruments_from_pyo3(pyo3_instruments: List[object]) -> List[Instrument]: ...
