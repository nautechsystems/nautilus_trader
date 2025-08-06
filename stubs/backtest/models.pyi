from typing import Any

from stubs.model.instruments.base import Instrument
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order

class FillModel:

    prob_fill_on_limit: float
    prob_fill_on_stop: float
    prob_slippage: float

    def __init__(
        self,
        prob_fill_on_limit: float = 1.0,
        prob_fill_on_stop: float = 1.0,
        prob_slippage: float = 0.0,
        random_seed: int | None = None,
        config: Any = None,
    ) -> None: ...
    def is_limit_filled(self) -> bool: ...
    def is_stop_filled(self) -> bool: ...
    def is_slipped(self) -> bool: ...


class LatencyModel:

    base_latency_nanos: int
    insert_latency_nanos: int
    update_latency_nanos: int
    cancel_latency_nanos: int

    def __init__(
        self,
        base_latency_nanos: int = ...,
        insert_latency_nanos: int = 0,
        update_latency_nanos: int = 0,
        cancel_latency_nanos: int = 0,
        config: Any = None,
    ) -> None: ...


class FeeModel:

    def get_commission(
        self,
        order: Order,
        fill_qty: Quantity,
        fill_px: Price,
        instrument: Instrument,
    ) -> Money: ...


class MakerTakerFeeModel(FeeModel):

    def __init__(self, config: Any = None) -> None: ...
    def get_commission(
        self,
        order: Order,
        fill_qty: Quantity,
        fill_px: Price,
        instrument: Instrument,
    ) -> Money: ...


class FixedFeeModel(FeeModel):

    def __init__(
        self,
        commission: Money = None,
        charge_commission_once: bool = True,
        config: Any = None,
    ) -> None: ...
    def get_commission(
        self,
        order: Order,
        fill_qty: Quantity,
        fill_px: Price,
        instrument: Instrument,
    ) -> Money: ...


class PerContractFeeModel(FeeModel):

    def __init__(
        self,
        commission: Money = None,
        config: Any = None,
    ) -> None: ...
    def get_commission(
        self,
        order: Order,
        fill_qty: Quantity,
        fill_px: Price,
        instrument: Instrument,
    ) -> Money: ...
