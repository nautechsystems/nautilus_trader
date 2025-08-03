import datetime as dt
from collections.abc import Awaitable
from collections.abc import Callable
from collections.abc import Iterator
from decimal import Decimal
from enum import Enum
from os import PathLike
from typing import Any, Final, TypeAlias, Union, ClassVar



class FillModel:
    """
    Provides probabilistic modeling for order fill dynamics including probability
    of fills and slippage by order type.

    Parameters
    ----------
    prob_fill_on_limit : double
        The probability of limit order filling if the market rests on its price.
    prob_fill_on_stop : double
        The probability of stop orders filling if the market rests on its price.
    prob_slippage : double
        The probability of order fill prices slipping by one tick.
    random_seed : int, optional
        The random seed (if None then no random seed).
    config : FillModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If any probability argument is not within range [0, 1].
    TypeError
        If `random_seed` is not None and not of type `int`.
    """

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
    def is_limit_filled(self) -> bool:
        """
        Return a value indicating whether a ``LIMIT`` order filled.

        Returns
        -------
        bool

        """
        ...
    def is_stop_filled(self) -> bool:
        """
        Return a value indicating whether a ``STOP-MARKET`` order filled.

        Returns
        -------
        bool

        """
        ...
    def is_slipped(self) -> bool:
        """
        Return a value indicating whether an order fill slipped.

        Returns
        -------
        bool

        """
        ...


class LatencyModel:
    """
    Provides a latency model for simulated exchange message I/O.

    Parameters
    ----------
    base_latency_nanos : int, default 1_000_000_000
        The base latency (nanoseconds) for the model.
    insert_latency_nanos : int, default 0
        The order insert latency (nanoseconds) for the model.
    update_latency_nanos : int, default 0
        The order update latency (nanoseconds) for the model.
    cancel_latency_nanos : int, default 0
        The order cancel latency (nanoseconds) for the model.
    config : FillModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If `base_latency_nanos` is negative (< 0).
    ValueError
        If `insert_latency_nanos` is negative (< 0).
    ValueError
        If `update_latency_nanos` is negative (< 0).
    ValueError
        If `cancel_latency_nanos` is negative (< 0).
    """

    base_latency_nanos: int
    insert_latency_nanos: int
    update_latency_nanos: int
    cancel_latency_nanos: int

    def __init__(
        self,
        base_latency_nanos: int = 1_000_000_000,
        insert_latency_nanos: int = 0,
        update_latency_nanos: int = 0,
        cancel_latency_nanos: int = 0,
        config: Any = None,
    ) -> None: ...


class FeeModel:
    """
    Provides an abstract fee model for trades.
    """

    def get_commission(
        self,
        order: Order,
        fill_qty: Quantity,
        fill_px: Price,
        instrument: Instrument,
    ) -> Money:
        """
        Return the commission for a trade.

        Parameters
        ----------
        order : Order
            The order to calculate the commission for.
        fill_qty : Quantity
            The fill quantity of the order.
        fill_px : Price
            The fill price of the order.
        instrument : Instrument
            The instrument for the order.

        Returns
        -------
        Money

        """
        ...


class MakerTakerFeeModel(FeeModel):
    """
    Provide a fee model for trades based on a maker/taker fee schedule
    and notional value of the trade.

    Parameters
    ----------
    config : MakerTakerFeeModelConfig, optional
        The configuration for the fee model.
    """

    def __init__(self, config: Any = None) -> None: ...
    def get_commission(
        self,
        order: Order,
        fill_qty: Quantity,
        fill_px: Price,
        instrument: Instrument,
    ) -> Money: ...


class FixedFeeModel(FeeModel):
    """
    Provides a fixed fee model for trades.

    Parameters
    ----------
    commission : Money, optional
        The fixed commission amount for trades.
    charge_commission_once : bool, default True
        Whether to charge the commission once per order or per fill.
    config : FixedFeeModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If both ``commission`` **and** ``config`` are provided, **or** if both are ``None`` (exactly one must be supplied).
    ValueError
        If `commission` is not a positive amount.
    """

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
    """
    Provides a fee model which charges a commission per contract traded.

    Parameters
    ----------
    commission : Money, optional
        The commission amount per contract.
    config : PerContractFeeModelConfig, optional
        The configuration for the model.

    Raises
    ------
    ValueError
        If both ``commission`` **and** ``config`` are provided, **or** if both are ``None`` (exactly one must be supplied).
    ValueError
        If `commission` is negative (< 0).
    """

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