from decimal import Decimal
from typing import ClassVar

from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from stubs.accounting.accounts.cash import CashAccount
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

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
    ) -> Money:
        """
        Calculate the locked balance.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        use_quote_for_inverse : bool
            Not applicable for betting accounts.

        Returns
        -------
        Money

        """
        ...
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

