import asyncio

from nautilus_trader.adapters.polymarket.websocket.client import PolymarketWebSocketClient
from nautilus_trader.common.component import LiveClock


async def run_polymarket_websocket():
    clock = LiveClock()
    loop = asyncio.get_running_loop()

    client = PolymarketWebSocketClient(
        clock=clock,
        base_url=None,
        channel="market",
        handler=print,
        handler_reconnect=None,
        loop=loop,
    )

    # market = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
    token_yes = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
    token_no = "48331043336612883890938759509493159234755048973500640148014422747788308965732"

    await client.subscribe_book(asset=token_yes)
    await client.subscribe_book(asset=token_no)
    await client.connect()

    await asyncio.sleep(30)


if __name__ == "__main__":
    asyncio.run(run_polymarket_websocket())
