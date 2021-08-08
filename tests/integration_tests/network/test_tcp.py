import asyncio
import json
import logging
import socketserver
import threading
import time

import orjson
import pytest

from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.network.socket import SocketClient
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.test_kit.stubs import TestStubs


logger = logging.getLogger(__name__)


class ThreadedTCPServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    daemon_threads = True
    allow_reuse_address = True


class BetfairTCPHandler(socketserver.StreamRequestHandler):
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
            line = orjson.dumps(data)
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


@pytest.fixture(autouse=True)
def betfair_server():
    print("Starting mock-betfair server")
    with ThreadedTCPServer(("127.0.0.1", 0), BetfairTCPHandler) as server:
        thread = threading.Thread(target=server.serve_forever)
        thread.daemon = True
        thread.start()
        yield server


@pytest.fixture()
def logger_adapter() -> LoggerAdapter:
    return LoggerAdapter("socket_test", TestStubs.logger())


@pytest.mark.asyncio
async def test_client_recv(event_loop, betfair_server, logger_adapter):
    lines = []

    def record(*args, **kwargs):
        lines.append((args, kwargs))

    client = SocketClient(
        logger_adapter=logger_adapter,
        message_handler=record,
        host=betfair_server.server_address[0],
        port=betfair_server.server_address[1],
        ssl=False,
    )
    await client.connect()
    # Simulate an auth message
    await client.send(orjson.dumps({"authentication": True}))
    await client.send(orjson.dumps({"num_lines": 10}))
    event_loop.create_task(client.start())
    await asyncio.sleep(1)
    client._stop = True
    await asyncio.sleep(1)
    assert len(lines) == 10
