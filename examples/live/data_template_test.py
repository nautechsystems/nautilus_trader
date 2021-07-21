from abc import ABC
import asyncio

import ib_insync as ibi

from nautilus_trader.adapters._template.data import TemplateLiveMarketDataClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.infrastructure.cache import RedisCacheDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.serialization.msgpack.serializer import MsgPackCommandSerializer
from nautilus_trader.serialization.msgpack.serializer import MsgPackEventSerializer
from nautilus_trader.serialization.msgpack.serializer import MsgPackInstrumentSerializer
from nautilus_trader.trading.portfolio import Portfolio


class InteractiveBrokersClient(TemplateLiveMarketDataClient, ABC):
    def __init__(self, **kwargs):
        super().__init__(
            client_id=kwargs["client_id"],
            clock=kwargs["clock"],
            config={},
            engine=kwargs["engine"],
            logger=kwargs["logger"],
        )
        self.loop = kwargs["loop"]
        self.ib_cli = ibi.IB()

    def run(self):
        self.loop.create_task(self.start_client())

    async def connect(self):
        await self.ib_cli.connectAsync(host="127.0.0.1", port=4002, clientId=10)
        assert self.ib_cli.isConnected()

    def disconnect(self):
        self.ib_cli.disconnect()
        assert not self.ib_cli.isConnected()

    async def start_client(self):
        await self.connect()


async def main(loop):
    clock = LiveClock(loop=loop)

    config_trader = {"name": "my-config", "id_tag": "003"}
    trader_id = TraderId(
        f"{config_trader['name']}-{config_trader['id_tag']}",
    )

    uuid_factory = UUIDFactory()
    system_id = uuid_factory.generate()

    logger = LiveLogger(
        loop=loop,
        clock=clock,
        trader_id=trader_id,
        system_id=system_id,
        level_stdout=1,
    )
    msgbus = MessageBus(
        clock=clock,
        logger=logger,
    )
    cache_db = RedisCacheDatabase(
        trader_id=trader_id,
        logger=logger,
        instrument_serializer=MsgPackInstrumentSerializer(),
        command_serializer=MsgPackCommandSerializer(),
        event_serializer=MsgPackEventSerializer(),
        config={
            "host": "localhost",
            "port": "6379",
        },
    )
    cache = Cache(
        database=cache_db,
        logger=logger,
        config={},
    )
    portfolio = Portfolio(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=logger,
    )

    data_engine = LiveDataEngine(
        loop=loop,
        portfolio=portfolio,
        cache=cache,
        clock=clock,
        logger=logger,
        config={},
    )

    ibc = InteractiveBrokersClient(
        client_id=ClientId("MyClientID"),
        clock=clock,
        config={},
        engine=data_engine,
        logger=logger,
        loop=loop,
    )
    ibc.run()


if __name__ == "__main__":
    main_loop = asyncio.get_event_loop()
    main_loop.create_task(main(main_loop))
    main_loop.run_forever()
