import asyncio

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.bus import MessageBus


class HistoricInteractiveBrokersClient(InteractiveBrokersClient):
    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 7497,
        client_id: int = 1,
    ):
        loop = asyncio.get_event_loop()
        clock = LiveClock()
        logger = Logger(clock)
        msgbus = MessageBus(
            TraderId("historic_interactive_brokers_client"),
            clock,
            logger,
        )
        cache = Cache(logger)
        super().__init__(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            host=host,
            port=port,
            client_id=client_id,
        )


if __name__ == "__main__":
    client = HistoricInteractiveBrokersClient()
