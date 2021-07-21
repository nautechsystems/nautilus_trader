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
