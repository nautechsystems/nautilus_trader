from decimal import Decimal
from typing import Any

from nautilus_trader.analysis.analyzer import PortfolioAnalyzer
from nautilus_trader.portfolio.config import PortfolioConfig
from stubs.accounting.manager import AccountsManager
from stubs.cache.base import CacheFacade
from stubs.common.component import Clock
from stubs.common.component import Logger
from stubs.portfolio.base import PortfolioFacade

class Portfolio(PortfolioFacade):
    """
    Provides a trading portfolio.

    Currently there is a limitation of one account per ``ExecutionClient``
    instance.

    Parameters
    ----------
    msgbus : MessageBus
        The message bus for the engine.
    cache : CacheFacade
        The read-only cache for the portfolio.
    clock : Clock
        The clock for the portfolio.
    config : PortfolioConfig
       The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `PortfolioConfig`.
    """

    analyzer: PortfolioAnalyzer
    initialized: bool

    _clock: Clock
    _log: Logger
    _msgbus: MessageBus
    _cache: CacheFacade
    _accounts: AccountsManager

    _config: PortfolioConfig
    _debug: bool
    _use_mark_prices: bool
    _use_mark_xrates: bool
    _convert_to_account_base_currency: bool
    _log_price: str
    _log_xrate: str
    _realized_pnls: dict[InstrumentId, Money]
    _unrealized_pnls: dict[InstrumentId, Money]
    _net_positions: dict[InstrumentId, Decimal]
    _bet_positions: dict[InstrumentId, Any]
    _index_bet_positions: dict[InstrumentId, set[PositionId]]
    _pending_calcs: set[InstrumentId]
    _bar_close_prices: dict[InstrumentId, Price]

    def __init__(
        self,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: Clock,
        config: PortfolioConfig | None = None,
    ) -> None: ...
    def set_use_mark_prices(self, value: bool) -> None:
        """
        Set the `use_mark_prices` setting with the given `value`.

        Parameters
        ----------
        value : bool
            The value to set.

        """
        ...
    def set_use_mark_xrates(self, value: bool) -> None:
        """
        Set the `use_mark_xrates` setting with the given `value`.

        Parameters
        ----------
        value : bool
            The value to set.

        """
        ...
    def set_specific_venue(self, venue: Venue) -> None:
        """
        Set a specific venue for the portfolio.

        Parameters
        ----------
        venue : Venue
            The specific venue to set.

        """
        ...
    def initialize_orders(self) -> None:
        """
        Initialize the portfolios orders.

        Performs all account calculations for the current orders state.
        """
        ...
    def initialize_positions(self) -> None:
        """
        Initialize the portfolios positions.

        Performs all account calculations for the current position state.
        """
        ...
    def update_quote_tick(self, tick: QuoteTick) -> None:
        """
        Update the portfolio with the given quote tick.

        Clears the cached unrealized PnL for the associated instrument, and
        performs any initialization calculations which may have been pending
        an update.

        Parameters
        ----------
        quote_tick : QuoteTick
            The quote tick to update with.

        """
        ...
    def update_mark_price(self, mark_price: object) -> None:
        """
        TBD
        """
        ...
    def update_bar(self, bar: Bar) -> None:
        """
        Update the portfolio with the given bar.

        Clears the cached unrealized PnL for the associated instrument, and
        performs any initialization calculations which may have been pending
        an update.

        Parameters
        ----------
        bar : Bar
            The bar to update with.

        """
        ...
    def update_account(self, event: AccountState) -> None:
        """
        Apply the given account state.

        Parameters
        ----------
        event : AccountState
            The account state to apply.

        """
        ...
    def update_order(self, event: OrderEvent) -> None:
        """
        Update the portfolio with the given order.

        Parameters
        ----------
        event : OrderEvent
            The event to update with.

        """
        ...
    def update_position(self, event: PositionEvent) -> None:
        """
        Update the portfolio with the given position event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        ...
    def on_order_event(self, event: OrderEvent) -> None:
        """
        Actions to be performed on receiving an order event.

        Parameters
        ----------
        event : OrderEvent
            The event received.

        """
        ...
    def on_position_event(self, event: PositionEvent) -> None:
        """
        Actions to be performed on receiving a position event.

        Parameters
        ----------
        event : PositionEvent
            The event received.

        """
        ...
    def _reset(self) -> None: ...
    def reset(self) -> None:
        """
        Reset the portfolio.

        All stateful fields are reset to their initial value.

        """
        ...
    def dispose(self) -> None:
        """
        Dispose of the portfolio.

        All stateful fields are reset to their initial value.

        """
        ...
    def account(self, venue: Venue) -> Account | None:
        """
        Return the account for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        Account or ``None``

        """
        ...
    def balances_locked(self, venue: Venue) -> dict[Currency, Money] | None:
        """
        Return the balances locked for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        ...
    def margins_init(self, venue: Venue) -> dict[Currency, Money] | None:
        """
        Return the initial (order) margins for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        ...
    def margins_maint(self, venue: Venue) -> dict[Currency, Money] | None:
        """
        Return the maintenance (position) margins for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        ...
    def realized_pnls(self, venue: Venue) -> dict[Currency, Money]:
        """
        Return the realized PnLs for the given venue (if found).

        If no positions exist for the venue or if any lookups fail internally,
        an empty dictionary is returned.

        Parameters
        ----------
        venue : Venue
            The venue for the realized PnLs.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def unrealized_pnls(self, venue: Venue) -> dict[Currency, Money]:
        """
        Return the unrealized PnLs for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the unrealized PnLs.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def total_pnls(self, venue: Venue) -> dict[Currency, Money]:
        """
        Return the total PnLs for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the total PnLs.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def net_exposures(self, venue: Venue) -> dict[Currency, Money] | None:
        """
        Return the net exposures for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the market value.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        ...
    def realized_pnl(self, instrument_id: InstrumentId) -> Money | None:
        """
        Return the realized PnL for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the realized PnL.

        Returns
        -------
        Money or ``None``

        """
        ...
    def unrealized_pnl(self, instrument_id: InstrumentId, price: Price | None = None) -> Money | None:
        """
        Return the unrealized PnL for the given instrument ID (if found).

        - If `price` is provided, a fresh calculation is performed without using or
          updating the cache.
        - If `price` is omitted, the method returns the cached PnL if available, or
          computes and caches it if not.

        Returns `None` if the calculation fails (e.g., the account or instrument cannot
        be found), or zero-valued `Money` if no positions are open. Otherwise, it returns
        a `Money` object (usually in the account’s base currency or the instrument’s
        settlement currency).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the unrealized PnL.
        price : Price, optional
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.

        Returns
        -------
        Money or ``None``
            The unrealized PnL or None if the calculation cannot be performed.

        """
        ...
    def total_pnl(self, instrument_id: InstrumentId, price: Price | None = None) -> Money | None:
        """
        Return the total PnL for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the total PnL.
        price : Price, optional
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.

        Returns
        -------
        Money or ``None``

        """
        ...
    def net_exposure(self, instrument_id: InstrumentId, price: Price | None = None) -> Money | None:
        """
        Return the net exposure for the given instrument (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the calculation.
        price : Price, optional
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.

        Returns
        -------
        Money or ``None``

        """
        ...
    def net_position(self, instrument_id: InstrumentId) -> object:
        """
        Return the total net position for the given instrument ID.
        If no positions for instrument_id then will return `Decimal('0')`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.

        Returns
        -------
        Decimal

        """
        ...
    def is_net_long(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the portfolio is net long the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.

        Returns
        -------
        bool
            True if net long, else False.

        """
        ...
    def is_net_short(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the portfolio is net short the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.

        Returns
        -------
        bool
            True if net short, else False.

        """
        ...
    def is_flat(self, instrument_id: InstrumentId) -> bool:
        """
        Return a value indicating whether the portfolio is flat for the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument query filter.

        Returns
        -------
        bool
            True if net flat, else False.

        """
        ...
    def is_completely_flat(self) -> bool:
        """
        Return a value indicating whether the portfolio is completely flat.

        Returns
        -------
        bool
            True if net flat across all instruments, else False.

        """
        ...