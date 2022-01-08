from decimal import Decimal


class AccountBalanceNegative(Exception):
    """
    Represents a negative account balance error.
    """

    def __init__(self, balance: Decimal):
        super().__init__()

        self.balance = balance

    def __str__(self) -> str:
        return f"AccountBalanceNegative(balance={self.balance}"
