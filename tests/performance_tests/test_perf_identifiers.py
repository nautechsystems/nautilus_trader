from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.performance import PerformanceBench


def test_symbol_equality():
    symbol = Symbol("AUD/USD")

    def symbol_equality() -> bool:
        return symbol == symbol

    PerformanceBench.profile_function(
        target=symbol_equality,
        runs=1_000_000,
        iterations=1,
    )


def test_venue_equality():
    venue = Venue("SIM")

    def venue_equality() -> bool:
        return venue == venue

    PerformanceBench.profile_function(
        target=venue_equality,
        runs=1_000_000,
        iterations=1,
    )
