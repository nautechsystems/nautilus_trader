from decimal import Decimal

from nautilus_trader.model.objects import Currency


class AccountError(Exception):
    """
    The base class for all account type errors.
    """


class AccountBalanceNegative(AccountError):
    """
    Raised when the account balance for a currency becomes negative.
    """

    def __init__(self, balance: Decimal, currency: Currency):
        super().__init__()

        self.balance = balance
        self.currency = currency

    def __str__(self) -> str:
        return f"{type(self).__name__}(balance={self.balance}, currency={self.currency})"


class AccountMarginExceeded(AccountError):
    """
    Raised when the account margin for a currency is exceeded.

    In this scenario some form of liquidation event will occur.

    """

    def __init__(self, balance: Decimal, margin: Decimal, currency: Currency):
        super().__init__()

        self.balance = balance
        self.margin = margin
        self.currency = currency

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"balance={self.balance}, "
            f"margin={self.margin}, "
            f"free={self.balance - self.margin}, "
            f"currency={self.currency})"
        )
