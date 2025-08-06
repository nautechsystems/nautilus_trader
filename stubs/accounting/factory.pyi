from stubs.accounting.accounts.base import Account
from stubs.model.events.account import AccountState

class AccountFactory:

    @staticmethod
    def register_account_type(issuer: str, account_cls: type) -> None: ...
    @staticmethod
    def register_calculated_account(issuer: str) -> None: ...
    @staticmethod
    def create(event: AccountState) -> Account: ...