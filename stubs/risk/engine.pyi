import datetime as dt
from decimal import Decimal

from nautilus_trader.model.enums import TradingState
from nautilus_trader.risk.config import RiskEngineConfig
from stubs.cache.cache import Cache
from stubs.common.component import Clock
from stubs.common.component import Component
from stubs.common.component import MessageBus
from stubs.core.message import Command
from stubs.core.message import Event
from stubs.execution.messages import ModifyOrder
from stubs.execution.messages import SubmitOrder
from stubs.execution.messages import SubmitOrderList
from stubs.execution.messages import TradingCommand
from stubs.model.identifiers import InstrumentId
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order
from stubs.model.orders.list import OrderList
from stubs.portfolio.base import PortfolioFacade

class RiskEngine(Component):
    """
    Provides a high-performance risk engine.

    The `RiskEngine` is responsible for global strategy and portfolio risk
    within the platform. This includes both pre-trade risk checks and post-trade
    risk monitoring.

    Possible trading states:
     - ``ACTIVE`` (trading is enabled).
     - ``REDUCING`` (only new orders or updates which reduce an open position are allowed).
     - ``HALTED`` (all trading commands except cancels are denied).

    Parameters
    ----------
    portfolio : PortfolioFacade
        The portfolio for the engine.
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    config : RiskEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `RiskEngineConfig`.
    """

    trading_state: TradingState
    is_bypassed: bool
    debug: bool
    command_count: int
    event_count: int

    def __init__(
        self,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        config: RiskEngineConfig | None = None,
    ) -> None: ...
    def execute(self, command: Command) -> None:
        """
        Execute the given command.

        Parameters
        ----------
        command : Command
            The command to execute.

        """
        ...
    def process(self, event: Event) -> None:
        """
        Process the given event.

        Parameters
        ----------
        event : Event
            The event to process.

        """
        ...
    def set_trading_state(self, state: TradingState) -> None:
        """
        Set the trading state for the engine.

        Parameters
        ----------
        state : TradingState
            The state to set.

        """
        ...
    def set_max_notional_per_order(
        self,
        instrument_id: InstrumentId,
        new_value,
    ) -> None:
        """
        Set the maximum notional value per order for the given instrument ID.

        Passing a new_value of ``None`` will disable the pre-trade risk max
        notional check.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the max notional.
        new_value : integer, float, string or Decimal
            The max notional value to set.

        Raises
        ------
        decimal.InvalidOperation
            If `new_value` not a valid input for `decimal.Decimal`.
        ValueError
            If `new_value` is not ``None`` and not positive.

        """
        ...
    def max_order_submit_rate(self) -> tuple[int, dt.timedelta]:
        """
        Return the current maximum order submit rate limit setting.

        Returns
        -------
        (int, timedelta)
            The limit per timedelta interval.

        """
        ...
    def max_order_modify_rate(self) -> tuple[int, dt.timedelta]:
        """
        Return the current maximum order modify rate limit setting.

        Returns
        -------
        (int, timedelta)
            The limit per timedelta interval.

        """
        ...
    def max_notionals_per_order(self) -> dict[InstrumentId, Decimal]:
        """
        Return the current maximum notionals per order settings.

        Returns
        -------
        dict[InstrumentId, Decimal]

        """
        ...
    def max_notional_per_order(self, instrument_id: InstrumentId) -> Decimal | None:
        """
        Return the current maximum notional per order for the given instrument ID.

        Returns
        -------
        Decimal or ``None``

        """
        ...
    def _initialize_risk_checks(self, config: RiskEngineConfig) -> None: ...
    def _log_state(self) -> None: ...
    def _on_start(self) -> None: ...
    def _on_stop(self) -> None: ...
    def _start(self) -> None: ...
    def _stop(self) -> None: ...
    def _reset(self) -> None: ...
    def _dispose(self) -> None: ...
    def _execute_command(self, command: Command) -> None: ...
    def _handle_submit_order(self, command: SubmitOrder) -> None: ...
    def _handle_submit_order_list(self, command: SubmitOrderList) -> None: ...
    def _handle_modify_order(self, command: ModifyOrder) -> None: ...
    def _check_order(self, instrument: Instrument, order: Order) -> bool: ...
    def _check_order_price(self, instrument: Instrument, order: Order) -> bool: ...
    def _check_order_quantity(self, instrument: Instrument, order: Order) -> bool: ...
    def _check_orders_risk(self, instrument: Instrument, orders: list[Order]) -> bool: ...
    def _check_price(self, instrument: Instrument, price: Price | None) -> str | None: ...
    def _check_quantity(self, instrument: Instrument, quantity: Quantity | None) -> str | None: ...
    def _deny_command(self, command: TradingCommand, reason: str) -> None: ...
    def _deny_new_order(self, command: TradingCommand) -> None: ...
    def _deny_modify_order(self, command: ModifyOrder) -> None: ...
    def _deny_order(self, order: Order, reason: str) -> None: ...
    def _deny_order_list(self, order_list: OrderList, reason: str) -> None: ...
    def _execution_gateway(self, instrument: Instrument, command: TradingCommand) -> None: ...
    def _send_to_execution(self, command: TradingCommand) -> None: ...
    def _reject_modify_order(self, order: Order, reason: str) -> None: ...
    def _handle_event(self, event: Event) -> None: ...
