import os

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio
async def test_binance_spot_instrument_provider():
    clock = LiveClock()

    client = get_cached_binance_http_client(
        clock=clock,
        account_type=BinanceAccountType.SPOT,
        api_key=os.getenv("BINANCE_API_KEY"),
        api_secret=os.getenv("BINANCE_API_SECRET"),
        environment=BinanceEnvironment.TESTNET,
    )

    provider = BinanceSpotInstrumentProvider(
        client=client,
        clock=clock,
    )

    await provider.load_all_async()

    print(provider.count)
