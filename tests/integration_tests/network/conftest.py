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

import socketserver
import threading
import time
from typing import Generator

import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger


class ThreadedTCPRequestHandler(socketserver.StreamRequestHandler):
    def handle(self):
        while True:
            response = bytes("hello\r\n", "ascii")
            print("Sending response", response)
            self.request.sendall(response)
            time.sleep(0.1)


class ThreadedTCPServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    pass


@pytest.fixture()
def socket_server() -> Generator:
    server = ThreadedTCPServer(("localhost", 0), ThreadedTCPRequestHandler)
    with server:
        # Start a thread with the server -- that thread will then start one
        # more thread for each request
        server_thread = threading.Thread(target=server.serve_forever)
        # Exit the server thread when the main thread terminates
        server_thread.daemon = True
        server_thread.start()
        yield server
        server.shutdown()


@pytest.fixture()
def logger(event_loop):
    clock = LiveClock()
    return LiveLogger(loop=event_loop, clock=clock)
