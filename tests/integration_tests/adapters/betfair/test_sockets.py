from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient


def test_unique_id():
    clients = [
        BetfairMarketStreamClient(),
        BetfairOrderStreamClient(),
        BetfairMarketStreamClient(),
    ]
    result = [c.unique_id for c in clients]
    expected = [1, 2, 3]
    assert result == expected
