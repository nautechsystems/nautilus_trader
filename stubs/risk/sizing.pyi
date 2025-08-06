from decimal import Decimal

from stubs.model.instruments.base import Instrument
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class PositionSizer:

    instrument: Instrument

    def __init__(self, instrument: Instrument) -> None: ...
    def update_instrument(self, instrument: Instrument) -> None: ...
    def calculate(
        self,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        risk: Decimal,
        commission_rate: Decimal = ...,
        exchange_rate: Decimal = ...,
        hard_limit: Decimal | None = None,
        unit_batch_size: Decimal = ...,
        units: int = 1,
    ) -> Quantity: ...


class FixedRiskSizer(PositionSizer):

    def __init__(self, instrument: Instrument) -> None: ...
    def calculate(
        self,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        risk: Decimal,
        commission_rate: Decimal = ...,
        exchange_rate: Decimal = ...,
        hard_limit: Decimal | None = None,
        unit_batch_size: Decimal = ...,
        units: int = 1,
    ) -> Quantity: ...
