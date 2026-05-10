# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Sandbox test for the Binance WebSocket API user data stream.

Uses session.logon and userDataStream.subscribe instead of listenKey. Supports both
Ed25519 and HMAC API keys.

"""

import asyncio
import os

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.common.urls import get_ws_api_base_url
from nautilus_trader.adapters.binance.websocket.user import BinanceUserDataWebSocketClient
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio
async def test_binance_websocket_api_user_data():
    clock = LiveClock()
    loop = asyncio.get_running_loop()

    api_key = os.getenv("BINANCE_API_KEY")
    api_secret = os.getenv("BINANCE_API_SECRET")

    if not api_key or not api_secret:
        pytest.skip("BINANCE_API_KEY and BINANCE_API_SECRET not set")

    ws_api_url = get_ws_api_base_url(
        account_type=BinanceAccountType.SPOT,
        environment=BinanceEnvironment.LIVE,
        is_us=False,
    )

    client = BinanceUserDataWebSocketClient(
        clock=clock,
        base_url=ws_api_url,
        handler=lambda raw: print(f"Received: {raw}"),
        api_key=api_key,
        api_secret=api_secret,
        loop=loop,
    )

    await client.connect()
    await client.session_logon()
    subscription_id = await client.subscribe_user_data_stream()

    print(f"Subscribed to user data stream: {subscription_id}")

    # Wait for events
    await asyncio.sleep(10)

    await client.unsubscribe_user_data_stream()
    await client.disconnect()
