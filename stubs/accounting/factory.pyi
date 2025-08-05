from stubs.accounting.accounts.base import Account
from stubs.model.events.account import AccountState

class AccountFactory:
    """
    Provides a factory for creating different account types.
    """

    @staticmethod
    def register_account_type(issuer: str, account_cls: type) -> None:
        """
        Register the given custom account type for the issuer.

        Parameters
        ----------
        issuer : str
            The issuer for the account.
        account_cls : type
            The custom account type.

        Raises
        ------
        KeyError
            If `issuer` has already registered a custom account type.

        """
        ...

    @staticmethod
    def register_calculated_account(issuer: str) -> None:
        """
        Register for account state of the given issuer to be calculated from
        order fills.

        Parameters
        ----------
        issuer : str
            The issuer for the account.

        Raises
        ------
        KeyError
            If an issuer has already been registered for the `issuer`.

        """
        ...

    @staticmethod
    def create(event: AccountState) -> Account:
        """
        Create an account based on the events account type.

        Parameters
        ----------
        event : AccountState
            The account state event for the creation.

        Returns
        -------
        Account

        """
        ...