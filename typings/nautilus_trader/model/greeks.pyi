import numpy as np
from nautilus_trader.core.data import Data
from nautilus_trader.model.identifiers import InstrumentId
from numpy.typing import NDArray

class GreeksData(Data):
    instrument_id: InstrumentId
    is_call: bool
    strike: float
    expiry: int
    underlying_price: float
    expiry_in_years: float
    interest_rate: float
    vol: float
    price: float
    delta: float
    gamma: float
    vega: float
    theta: float
    quantity: float
    itm_prob: float

    def __init__(
        self,
        ts_init: int = ...,
        ts_event: int = ...,
        instrument_id: InstrumentId = ...,
        is_call: bool = ...,
        strike: float = ...,
        expiry: int = ...,
        underlying_price: float = ...,
        expiry_in_years: float = ...,
        interest_rate: float = ...,
        vol: float = ...,
        price: float = ...,
        delta: float = ...,
        gamma: float = ...,
        vega: float = ...,
        theta: float = ...,
        quantity: float = ...,
        itm_prob: float = ...,
    ) -> None: ...
    @classmethod
    def from_delta(cls, instrument_id: InstrumentId, delta: float) -> GreeksData: ...
    def __rmul__(self, quantity: float) -> GreeksData: ...

class PortfolioGreeks(Data):
    delta: float
    gamma: float
    vega: float
    theta: float

    def __init__(
        self,
        ts_init: int = ...,
        ts_event: int = ...,
        delta: float = ...,
        gamma: float = ...,
        vega: float = ...,
        theta: float = ...,
    ) -> None: ...
    def __add__(self, other: PortfolioGreeks) -> PortfolioGreeks: ...

class InterestRateData(Data):
    curve_name: str
    interest_rate: float

    def __init__(
        self,
        ts_init: int = ...,
        ts_event: int = ...,
        curve_name: str = ...,
        interest_rate: float = ...,
    ) -> None: ...
    def __call__(self, expiry_in_years: float) -> float: ...

class InterestRateCurveData(Data):
    curve_name: str
    tenors: NDArray[np.float64]
    interest_rates: NDArray[np.float64]

    def __init__(
        self,
        ts_init: int = ...,
        ts_event: int = ...,
        curve_name: str = ...,
        tenors: NDArray[np.float64] = ...,
        interest_rates: NDArray[np.float64] = ...,
    ) -> None: ...
    def __call__(self, expiry_in_years: float) -> float: ...
    def to_dict(self, to_arrow: bool = ...) -> dict: ...
    def from_dict(self, data: dict) -> InterestRateCurveData: ...
