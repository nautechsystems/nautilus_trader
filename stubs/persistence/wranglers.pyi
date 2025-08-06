from typing import Any

import numpy as np
import pandas as pd

from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from stubs.model.data import Bar
from stubs.model.data import BarType
from stubs.model.data import OrderBookDelta
from stubs.model.data import QuoteTick
from stubs.model.data import TradeTick
from stubs.model.instruments.base import Instrument

BAR_PRICES: tuple[str, str, str, str]
BAR_COLUMNS: tuple[str, str, str, str, str]

def preprocess_bar_data(data: pd.DataFrame, is_raw: bool) -> pd.DataFrame:
    ...
def calculate_bar_price_offsets(num_records: int, timestamp_is_close: bool, offset_interval_ms: int, random_seed: int | None = None) -> dict[str, Any]:
    ...
def calculate_volume_quarter(volume: np.ndarray, precision: int, size_increment: float) -> np.ndarray:
    ...
def align_bid_ask_bar_data(bid_data: pd.DataFrame, ask_data: pd.DataFrame) -> pd.DataFrame:
    ...
def prepare_event_and_init_timestamps(index: pd.DatetimeIndex, ts_init_delta: int) -> tuple[np.ndarray, np.ndarray]: ...

class OrderBookDeltaDataWrangler:

    instrument: Instrument
    def __init__(self, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, ts_init_delta: int = 0, is_raw: bool = False) -> list[OrderBookDelta]: ...
    def _build_delta(
        self,
        action: BookAction,
        side: OrderSide,
        price: float,
        size: float,
        order_id: int,
        flags: int,
        sequence: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDelta: ...

class QuoteTickDataWrangler:

    instrument: Instrument
    def __init__(self, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, default_volume: float = ..., ts_init_delta: int = 0) -> list[QuoteTick]: ...
    def process_bar_data(self, bid_data: pd.DataFrame, ask_data: pd.DataFrame, default_volume: float = ..., ts_init_delta: int = 0, offset_interval_ms: int = 100, timestamp_is_close: bool = True, random_seed: int | None = None, is_raw: bool = False, sort_data: bool = True) -> list[QuoteTick]: ...
    def _create_quote_ticks_array(self, merged_data: Any, is_raw: bool, instrument: Instrument, offsets: dict[str, Any], ts_init_delta: int) -> np.ndarray: ...
    def _build_tick(
        self,
        bid: float,
        ask: float,
        bid_size: float,
        ask_size: float,
        ts_event: int,
        ts_init: int,
    ) -> QuoteTick: ...

class TradeTickDataWrangler:

    instrument: Instrument
    def __init__(self, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, ts_init_delta: int = 0, is_raw: bool = False) -> list[TradeTick]: ...
    def process_bar_data(self, data: pd.DataFrame, ts_init_delta: int = 0, offset_interval_ms: int = 100, timestamp_is_close: bool = True, random_seed: int | None = None, is_raw: bool = False, sort_data: bool = True) -> list[TradeTick]: ...
    def _create_trade_ticks_array(self, records: Any, offsets: dict[str, Any]) -> np.ndarray: ...
    def _create_side_if_not_exist(self, data: pd.DataFrame) -> Any: ...
    def _build_tick(
        self,
        price: float,
        size: float,
        aggressor_side: AggressorSide,
        trade_id: str,
        ts_event: int,
        ts_init: int,
    ) -> TradeTick: ...

class BarDataWrangler:

    bar_type: BarType
    instrument: Instrument
    def __init__(self, bar_type: BarType, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, default_volume: float = ..., ts_init_delta: int = 0) -> list[Bar]: ...
    def _build_bar(self, values: memoryview, ts_event: int, ts_init: int) -> Bar: ...

