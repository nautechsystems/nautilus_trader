from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient


def test_unique_id(betfair_client):
    clients = [
        BetfairMarketStreamClient(client=betfair_client, message_handler=len),
        BetfairOrderStreamClient(client=betfair_client, message_handler=len),
        BetfairMarketStreamClient(client=betfair_client, message_handler=len),
    ]
    result = [c.unique_id for c in clients]
    expected = [1, 2, 3]
    assert result == expected
