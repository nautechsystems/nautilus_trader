from typing import ClassVar

from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import OrderListId
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TraderId
from stubs.common.component import Clock

class IdentifierGenerator:
    """
    Provides a generator for unique ID strings.

    Parameters
    ----------
    trader_id : TraderId
        The ID tag for the trader.
    clock : Clock
        The internal clock.
    """

    def __init__(self, trader_id: TraderId, clock: Clock) -> None: ...


class ClientOrderIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique `ClientOrderId`(s).

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the generator.
    strategy_id : StrategyId
        The strategy ID for the generator.
    clock : Clock
        The clock for the generator.
    initial_count : int
        The initial count for the generator.
    use_uuids : bool, default False
        If UUID4's should be used for client order ID values.
    use_hyphens : bool, default True
        If hyphens should be used in generated client order ID values.

    Raises
    ------
    ValueError
        If `initial_count` is negative (< 0).
    """

    count: ClassVar[int]
    use_uuids: ClassVar[bool]
    use_hyphens: ClassVar[bool]
    _id_tag_strategy: str

    def __init__(self, trader_id: TraderId, strategy_id: StrategyId, clock: Clock, initial_count: int = 0, use_uuids: bool = False, use_hyphens: bool = True) -> None: ...
    def set_count(self, count: int) -> None: ...
    def generate(self) -> ClientOrderId: ...
    def reset(self) -> None: ...


class OrderListIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique `OrderListId`(s).

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the generator.
    strategy_id : StrategyId
        The strategy ID for the generator.
    clock : Clock
        The clock for the generator.
    initial_count : int
        The initial count for the generator.

    Raises
    ------
    ValueError
        If `initial_count` is negative (< 0).
    """

    count: ClassVar[int]
    def __init__(self, trader_id: TraderId, strategy_id: StrategyId, clock: Clock, initial_count: int = 0) -> None: ...
    def set_count(self, count: int) -> None: ...
    def generate(self) -> OrderListId: ...
    def reset(self) -> None: ...


class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionId(s).

    Parameters
    ----------
    trader_id : TraderId
        The trader ID tag for the generator.
    """

    _counts: dict[StrategyId, int]

    def __init__(self, trader_id: TraderId, clock: Clock) -> None: ...
    def set_count(self, strategy_id: StrategyId, count: int) -> None: ...
    def get_count(self, strategy_id: StrategyId) -> int: ...
    def generate(self, strategy_id: StrategyId, flipped: bool = False) -> PositionId: ...
    def reset(self) -> None: ...