# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.network.socket import SocketClient


@pytest.mark.asyncio
@pytest.mark.skip  # Flaky
async def test_socket_base(socket_server, logger, event_loop):
    messages = []

    def handler(raw):
        messages.append(raw)
        if len(messages) > 5:
            client.stop()

    host, port = socket_server.server_address
    client = SocketClient(
        host=host,
        port=port,
        message_handler=handler,
        loop=event_loop,
        logger_adapter=LoggerAdapter("Socket", logger),
        ssl=False,
    )
    await client.start()
    assert messages == [b"hello"] * 6
