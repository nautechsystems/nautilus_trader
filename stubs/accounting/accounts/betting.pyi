from decimal import Decimal
from typing import ClassVar

from nautilus_trader.core.nautilus_pyo3 import AccountType
from nautilus_trader.core.nautilus_pyo3 import CashAccount
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity

class BettingAccount(CashAccount):
    """
    Provides a betting account.
    """

    ACCOUNT_TYPE: ClassVar[AccountType] = ...

    def calculate_balance_locked(
        self,
        instrument: Instrument,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool = False,
    ) -> Money: ...
    def balance_impact(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        order_side: OrderSide,
    ) -> Money: ...

def stake(quantity: Quantity, price: Price) -> Decimal: ...
def liability(quantity: Quantity, price: Price, side: OrderSide) -> Decimal: ...
def win_payoff(quantity: Quantity, price: Price, side: OrderSide) -> Decimal: ...
def lose_payoff(quantity: Quantity, side: OrderSide) -> Decimal: ...
def exposure(quantity: Quantity, price: Price, side: OrderSide) -> Decimal: ...
