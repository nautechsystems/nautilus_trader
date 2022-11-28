# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import socketserver
import threading
import time

import msgspec
import pytest

from nautilus_trader.network.socket import SocketClient
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider


class ThreadedTCPServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    daemon_threads = True
    allow_reuse_address = True


class TCPHandler(socketserver.StreamRequestHandler):
    def reader(self):
        raw = self.rfile.readline()
        print("SERVER [RECV]", raw)
        decoded = raw.strip().decode()
        if decoded == "GET / HTTP/1.1":
            return {}  # Health check
        msg = json.loads(decoded)
        return msg

    def on_connection_message(self):
        self.connection_info = {
            "client_address": self.client_address,
            "request": self.request,
        }

        msg = self.reader()

        if "authentication" in msg:
            print(f"Authenticated {self.client_address}")
            self.connection_info["auth"] = msg["authentication"]
            msg = self.reader()
            if "num_lines" in msg:
                print(f"Serving {msg['num_lines']} lines")
                self.connection_info["num_lines"] = msg["num_lines"]

    def handle(self):
        self.on_connection_message()

        if self.connection_info.get("auth") is None:
            return self.close()

        for n, data in enumerate(BetfairDataProvider.raw_market_updates()):
            line = msgspec.json.encode(data)
            try:
                print("SERVER [SEND]", line)
                self.wfile.write(line.strip() + b"\r\n")
                time.sleep(0.1)

                if (
                    self.connection_info.get("num_lines") is not None
                    and n > self.connection_info["num_lines"]
                ):
                    return self.close()

            except BrokenPipeError:
                return self.close()

    def close(self):
        if "auth" in self.connection_info:
            print(f"Closing connection for {self.client_address}")
        self.connection.close()
        self.finish()


@pytest.fixture()
def betfair_server():
    print("Starting mock server")
    with ThreadedTCPServer(("127.0.0.1", 0), TCPHandler) as server:
        thread = threading.Thread(target=server.serve_forever)
        thread.daemon = True
        thread.start()
        yield server


@pytest.mark.skip(reason="flaky test")
@pytest.mark.asyncio
async def test_client_recv(betfair_server, event_loop):
    lines = []

    def record(*args, **kwargs):
        lines.append((args, kwargs))

    client = SocketClient(
        host=betfair_server.server_address[0],
        port=betfair_server.server_address[1],
        loop=asyncio.get_event_loop(),
        handler=record,
        logger=TestComponentStubs.logger(),
        ssl=False,
    )
    await client.connect()
    # Simulate an auth message
    await client.send(msgspec.json.encode({"authentication": True}))
    await client.send(msgspec.json.encode({"num_lines": 10}))
    event_loop.create_task(client.start())
    await asyncio.sleep(1)
    client.stop()
    await asyncio.sleep(1)
    assert len(lines) == 10
