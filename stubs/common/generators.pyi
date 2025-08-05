from stubs.common.component import Clock
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import OrderListId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId

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

    count: int
    use_uuids: bool
    use_hyphens: bool
    _id_tag_strategy: str

    def __init__(self, trader_id: TraderId, strategy_id: StrategyId, clock: Clock, initial_count: int = 0, use_uuids: bool = False, use_hyphens: bool = True) -> None: ...
    def set_count(self, count: int) -> None:
        """
        Set the internal counter to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        """
    def generate(self) -> ClientOrderId:
        """
        Return a unique client order ID.

        Returns
        -------
        ClientOrderId

        """
    def reset(self) -> None:
        """
        Reset the ID generator.

        All stateful fields are reset to their initial value.
        """


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

    count: int
    _id_tag_strategy: str

    def __init__(self, trader_id: TraderId, strategy_id: StrategyId, clock: Clock, initial_count: int = 0) -> None: ...
    def set_count(self, count: int) -> None:
        """
        Set the internal counter to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        """
    def generate(self) -> OrderListId:
        """
        Return a unique order list ID.

        Returns
        -------
        OrderListId

        """
    def reset(self) -> None:
        """
        Reset the ID generator.

        All stateful fields are reset to their initial value.
        """


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
    def set_count(self, strategy_id: StrategyId, count: int) -> None:
        """
        Set the internal position count for the given strategy ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the count.
        count : int
            The count to set.

        Raises
        ------
        ValueError
            If `count` is negative (< 0).

        """
    def get_count(self, strategy_id: StrategyId) -> int:
        """
        Return the internal position count for the given strategy ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the count.

        Returns
        -------
        int

        """
    def generate(self, strategy_id: StrategyId, flipped: bool = False) -> PositionId:
        """
        Return a unique position ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the position.
        flipped : bool
            If the position is being flipped. If True, then the generated id
            will be appended with 'F'.

        Returns
        -------
        PositionId

        """
    def reset(self) -> None:
        """
        Reset the ID generator.

        All stateful fields are reset to their initial value.
        """

