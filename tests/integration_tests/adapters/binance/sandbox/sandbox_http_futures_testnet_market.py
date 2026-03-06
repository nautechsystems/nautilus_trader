import os

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio
async def test_binance_futures_testnet_market_http_client():
    clock = LiveClock()

    account_type = BinanceAccountType.USDT_FUTURES

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=account_type,
        api_key=os.getenv("BINANCE_FUTURES_TESTNET_API_KEY"),
        api_secret=os.getenv("BINANCE_FUTURES_TESTNET_API_SECRET"),
        environment=BinanceEnvironment.TESTNET,
    )

    provider = BinanceFuturesInstrumentProvider(
        client=client,
        clock=clock,
        account_type=BinanceAccountType.USDT_FUTURES,
    )

    await provider.load_all_async()

    print(provider.count)
