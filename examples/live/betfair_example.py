import asyncio
import os
import traceback

from examples.strategies.orderbook_imbalance import OrderBookImbalance
from examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.adapters.betfair.factory import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factory import BetfairLiveExecutionClientFactory
from nautilus_trader.adapters.betfair.factory import get_betfair_client
from nautilus_trader.adapters.betfair.factory import get_instrument_provider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.infrastructure.config import CacheDatabaseConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.live.node import TradingNodeConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


async def main():
    # Find instruments for a particular market_id
    market_filter = {"market_id": ("1.188629427",)}
    instruments = await get_instruments(market_filter=market_filter)
    print(f"Found instruments:\n{instruments}")

    # Configure trading node
    config = TradingNodeConfig(
        timeout_connection=30.0,
        log_level="INFO",
        cache_database=CacheDatabaseConfig(type="in-memory"),
        data_clients={
            "BETFAIR": {
                "username": "BETFAIR_USERNAME",  # value is the environment variable key
                "password": "BETFAIR_PASSWORD",  # value is the environment variable key
                "app_key": "BETFAIR_APP_KEY",  # value is the environment variable key
                "cert_dir": "BETFAIR_CERT_DIR",  # value is the environment variable key
                "market_filter": market_filter,
            },
        },
        exec_clients={
            "BETFAIR": {
                "base_currency": "AUD",
                "username": "BETFAIR_USERNAME",  # value is the environment variable key
                "password": "BETFAIR_PASSWORD",  # value is the environment variable key
                "app_key": "BETFAIR_APP_KEY",  # value is the environment variable key
                "cert_dir": "BETFAIR_CERT_DIR",  # value is the environment variable key
                "market_filter": market_filter,
                "sandbox_mode": False,  # If clients use the testnet
            },
        },
    )
    strategies = [
        OrderBookImbalance(
            config=OrderBookImbalanceConfig(
                instrument_id=instrument.id.value,
                max_trade_size=10,
                order_id_tag=instrument.selection_id,
            )
        )
        for instrument in instruments
    ]

    # Setup TradingNode
    node = TradingNode(config=config)
    node.trader.add_strategies(strategies)

    # Register your client factories with the node (can take user defined factories)
    node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
    node.add_exec_client_factory("BETFAIR", BetfairLiveExecutionClientFactory)
    node.build()

    try:
        await node.start()
    except Exception as e:
        print(e)
        print(traceback.format_exc())
    finally:
        node.dispose()


async def get_instruments(market_filter):
    # Load instruments
    loop = asyncio.get_event_loop()
    logger = LiveLogger(loop=loop, clock=LiveClock())
    client = get_betfair_client(
        username=os.getenv("BETFAIR_USERNAME"),
        password=os.getenv("BETFAIR_PASSWORD"),
        app_key=os.getenv("BETFAIR_APP_KEY"),
        cert_dir=os.getenv("BETFAIR_CERT_DIR"),
        logger=logger,
        loop=loop,
    )
    await client.connect()
    await client.get_account_funds()

    provider = get_instrument_provider(
        client=client,
        logger=logger,
        market_filter=tuple(market_filter.items()),
    )
    await provider.load_all_async()
    return provider.list_instruments()


if __name__ == "__main__":
    asyncio.run(main())
