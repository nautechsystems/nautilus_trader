from nautilus_trader.common.config import OrderEmulatorConfig
from nautilus_trader.model.objects import Quantity
from nautilus_trader.core.message import Event
from nautilus_trader.model.orders.base import Order
from stubs.cache.cache import Cache
from stubs.common.actor import Actor
from stubs.common.component import Clock
from stubs.execution.manager import OrderManager
from stubs.portfolio.base import PortfolioFacade

class OrderEmulator(Actor):
    """
    Provides order emulation for specified trigger types.

    Parameters
    ----------
    portfolio : PortfolioFacade
        The read-only portfolio for the order emulator.
    msgbus : MessageBus
        The message bus for the order emulator.
    cache : Cache
        The cache for the order emulator.
    clock : Clock
        The clock for the order emulator.
    config : OrderEmulatorConfig, optional
        The configuration for the order emulator.

    """

    debug: bool
    command_count: int
    event_count: int

    _manager: OrderManager
    _matching_cores: dict[InstrumentId, MatchingCore]
    _subscribed_quotes: set[InstrumentId]
    _subscribed_trades: set[InstrumentId]
    _subscribed_strategies: set[StrategyId]
    _monitored_positions: set[PositionId]

    def __init__(
        self,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: Cache,
        clock: Clock,
        config: OrderEmulatorConfig | None = None,
    ) -> None: ...
    @property
    def subscribed_quotes(self) -> list[InstrumentId]:
        """
        Return the subscribed quote feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    @property
    def subscribed_trades(self) -> list[InstrumentId]:
        """
        Return the subscribed trade feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    def get_submit_order_commands(self) -> dict[ClientOrderId, SubmitOrder]:
        """
        Return the emulators cached submit order commands.

        Returns
        -------
        dict[ClientOrderId, SubmitOrder]

        """
        ...
    def get_matching_core(self, instrument_id: InstrumentId) -> MatchingCore | None:
        """
        Return the emulators matching core for the given instrument ID.

        Returns
        -------
        MatchingCore or ``None``

        """
        ...
    def on_start(self) -> None: ...
    def on_event(self, event: Event) -> None:
        """
        Handle the given `event`.

        Parameters
        ----------
        event : Event
            The received event to handle.

        """
        ...
    def on_stop(self) -> None: ...
    def on_reset(self) -> None: ...
    def on_dispose(self) -> None: ...
    def execute(self, command: TradingCommand) -> None:
        """
        Execute the given command.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        """
        ...
    def create_matching_core(
        self,
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> MatchingCore:
        """
        Create an internal matching core for the given `instrument`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the matching core.
        price_increment : Price
            The minimum price increment (tick size) for the matching core.

        Returns
        -------
        MatchingCore

        Raises
        ------
        KeyError
            If a matching core for the given `instrument_id` already exists.

        """
        ...
    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None: ...
    def on_quote_tick(self, tick: QuoteTick) -> None: ...
    def on_trade_tick(self, tick: TradeTick) -> None: ...
    def _check_monitoring(self, strategy_id: StrategyId, position_id: PositionId) -> None: ...
    def _cancel_order(self, order: Order) -> None: ...
    def _update_order(self, order: Order, new_quantity: Quantity) -> None: ...
    def _trigger_stop_order(self, order: Order) -> None: ...
    def _fill_market_order(self, order: Order) -> None: ...
    def _fill_limit_order(self, order: Order) -> None: ...

