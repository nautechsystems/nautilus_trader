from typing import ClassVar, Literal

from nautilus_trader.core.nautilus_pyo3 import Account
from nautilus_trader.core.nautilus_pyo3 import AccountState
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import LiquiditySide
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Position
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity

class CashAccount(Account):
    """
    Provides a cash account.

    Parameters
    ----------
    event : AccountState
        The initial account state event.
    calculate_account_state : bool, optional
        If the account state should be calculated from order fills.

    Raises
    ------
    ValueError
        If `event.account_type` is not equal to ``CASH``.
    """

    ACCOUNT_TYPE: ClassVar[Literal["CASH"]] = ...

    _balances_locked: dict[InstrumentId, Money]

    def __init__(
        self,
        event: AccountState,
        calculate_account_state: bool = False,
    ) -> None: ...
    @staticmethod
    def to_dict(obj: CashAccount) -> dict: ...
    @staticmethod
    def from_dict(values: dict) -> CashAccount: ...
    def update_balance_locked(self, instrument_id: InstrumentId, locked: Money) -> None: ...
    def clear_balance_locked(self, instrument_id: InstrumentId) -> None: ...
    def is_unleveraged(self, instrument_id: InstrumentId) -> bool: ...
    def calculate_commission(
        self,
        instrument: Instrument,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: bool = False,
    ) -> Money: ...
    def calculate_balance_locked(
        self,
        instrument: Instrument,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool = False,
    ) -> Money: ...
    def calculate_pnls(
        self,
        instrument: Instrument,
        fill: OrderFilled,
        position: Position | None = None,
    ) -> list[Money]: ...
    def balance_impact(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        order_side: OrderSide,
    ) -> Money: ...
