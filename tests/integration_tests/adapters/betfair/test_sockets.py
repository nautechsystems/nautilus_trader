from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient


def test_unique_id(betfair_client, live_logger):
    clients = [
        BetfairMarketStreamClient(
            client=betfair_client, logger=live_logger, message_handler=len
        ),
        BetfairOrderStreamClient(
            client=betfair_client, logger=live_logger, message_handler=len
        ),
        BetfairMarketStreamClient(
            client=betfair_client, logger=live_logger, message_handler=len
        ),
    ]
    result = [c.unique_id for c in clients]
    assert result == sorted(set(result))
