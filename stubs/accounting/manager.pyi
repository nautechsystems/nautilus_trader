from typing import Any

from nautilus_trader.cache.base import CacheFacade

class AccountsManager:
    """
    Provides account management functionality.

    Parameters
    ----------
    cache : CacheFacade
        The read-only cache for the manager.
    logger : Logger
        The logger for the manager.
    clock : Clock
        The clock for the manager.
    """

    def __init__(
        self,
        cache: CacheFacade,
        logger: Logger,
        clock: Clock,
    ) -> None: ...
    def generate_account_state(self, account: Account, ts_event: int) -> AccountState:
        """
        Generate a new account state event for the given `account`.

        Parameters
        ----------
        account : Account
            The account for the state event.
        ts_event : uint64_t
            The UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        AccountState

        """
        ...
    def update_balances(
        self,
        account: Account,
        instrument: Instrument,
        fill: OrderFilled,
    ) -> None:
        """
        Update the account balances based on the `fill` event.

        Parameters
        ----------
        account : Account
            The account to update.
        instrument : Instrument
            The instrument for the update.
        fill : OrderFilled
            The order filled event for the update

        Raises
        ------
        AccountBalanceNegative
            If account type is ``CASH`` and a balance becomes negative.

        """
        ...
    def update_orders(
        self,
        account: Account,
        instrument: Instrument,
        orders_open: list[Any],
        ts_event: int,
    ) -> bool:
        """
        Update the account states based on the given orders.

        Parameters
        ----------
        account : MarginAccount
            The account to update.
        instrument : Instrument
            The instrument for the update.
        orders_open : list[Order]
            The open orders for the update.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the account event occurred.

        Returns
        -------
        bool
            The result of the account operation.

        """
        ...
    def update_positions(
        self,
        account: MarginAccount,
        instrument: Instrument,
        positions_open: list[Position],
        ts_event: int,
    ) -> bool:
        """
        Update the maintenance (position) margin.

        Will return ``None`` if operation fails.

        Parameters
        ----------
        account : Account
            The account to update.
        instrument : Instrument
            The instrument for the update.
        positions_open : list[Position]
            The open positions for the update.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the account event occurred.

        Returns
        -------
        bool
            The result of the account operation.

        """
        ...

