from nautilus_trader.core.nautilus_pyo3 import AccountBalance
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import MarginBalance
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


class TestTypesProviderPyo3:
    @staticmethod
    def account_balance(
        total: Money = Money.from_str("1525000 USD"),
        locked: Money = Money.from_str("25000 USD"),
        free: Money = Money.from_str("1500000 USD"),
    ) -> AccountBalance:
        return AccountBalance(total, locked, free)

    @staticmethod
    def margin_balance(
        initial: Money = Money(1, Currency.from_str("USD")),
        maintenance: Money = Money(1, Currency.from_str("USD")),
        instrument_id: InstrumentId = TestIdProviderPyo3.audusd_id(),
    ) -> MarginBalance:
        return MarginBalance(initial, maintenance, instrument_id)
