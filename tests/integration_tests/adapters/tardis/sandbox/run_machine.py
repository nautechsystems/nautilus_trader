# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import asyncio
import json

import aiohttp


# Example taken from https://docs.tardis.dev/api/tardis-machine
# Run the following to start the tardis-machine server:
# docker run -p 8000:8000 -p 8001:8001 -e "TM_API_KEY=YOUR_API_KEY" -d tardisdev/tardis-machine


async def run():
    WS_REPLAY_URL = "ws://localhost:8001/ws-replay"
    URL = f"{WS_REPLAY_URL}?exchange=bitmex&from=2019-10-01&to=2019-10-02"

    async with aiohttp.ClientSession() as session, session.ws_connect(URL) as websocket:

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
