import asyncio
import json

import aiohttp


# Example taken from https://docs.tardis.dev/api/tardis-machine
# Run the following to start the tardis-machine server:
# docker run -p 8000:8000 -p 8001:8001 -e "TM_API_KEY=YOUR_API_KEY" -d tardisdev/tardis-machine


async def run():
    WS_REPLAY_URL = "ws://localhost:8001/ws-replay"
    URL = f"{WS_REPLAY_URL}?exchange=bitmex&from=2019-10-01&to=2019-10-02"

    async with aiohttp.ClientSession() as session:
        async with session.ws_connect(URL) as websocket:

            await websocket.send_str(
                json.dumps(
                    {
                        "op": "subscribe",
                        "args": [
                            "trade:XBTUSD",
                            "trade:ETHUSD",
                            "orderBookL2:XBTUSD",
                            "orderBookL2:ETHUSD",
                        ],
                    },
                ),
            )

            async for msg in websocket:
                print(msg.data)


if __name__ == "__main__":
    asyncio.run(run())
