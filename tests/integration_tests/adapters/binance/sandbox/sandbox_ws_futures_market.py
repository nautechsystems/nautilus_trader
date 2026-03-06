import asyncio

import pytest

from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio
async def test_binance_websocket_client():
    clock = LiveClock()

    loop = asyncio.get_running_loop()

    client = BinanceWebSocketClient(
        clock=clock,
        handler=print,
        base_url="wss://fstream.binance.com",
        loop=loop,
    )

    await client.connect()
    await client.subscribe_book_ticker("BTCUSDT-PERP")

    await asyncio.sleep(4)
    await client.disconnect()
