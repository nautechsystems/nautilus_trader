import socketserver
import threading
import time

import pytest


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
def socket_server() -> ThreadedTCPServer:
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
