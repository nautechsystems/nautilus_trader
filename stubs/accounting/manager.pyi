from typing import Any

from stubs.accounting.accounts.base import Account
from stubs.accounting.accounts.margin import MarginAccount
from stubs.cache.base import CacheFacade
from stubs.common.component import Clock
from stubs.common.component import Logger
from stubs.model.events.account import AccountState
from stubs.model.events.order import OrderFilled
from stubs.model.instruments.base import Instrument
from stubs.model.position import Position

class AccountsManager:

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
