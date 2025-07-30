from typing import Any

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.core.nautilus_pyo3 import Account
from nautilus_trader.core.nautilus_pyo3 import AccountState
from nautilus_trader.core.nautilus_pyo3 import Clock
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import Logger
from nautilus_trader.core.nautilus_pyo3 import MarginAccount
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import Position

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
    def generate_account_state(self, account: Account, ts_event: int) -> AccountState: ...
    def update_balances(
        self,
        account: Account,
        instrument: Instrument,
        fill: OrderFilled,
    ) -> None: ...
    def update_orders(
        self,
        account: Account,
        instrument: Instrument,
        orders_open: list[Any],
        ts_event: int,
    ) -> bool: ...
    def update_positions(
        self,
        account: MarginAccount,
        instrument: Instrument,
        positions_open: list[Position],
        ts_event: int,
    ) -> bool: ...
