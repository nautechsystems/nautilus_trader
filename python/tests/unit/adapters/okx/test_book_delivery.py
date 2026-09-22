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
Verify native OKX book delivery through Python callbacks against a local server.
"""

import base64
import hashlib
import json
import os
import struct
import subprocess
import sys
from decimal import Decimal
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from threading import Event
from threading import Thread

from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXDataClientFactory
from nautilus_trader.adapters.okx import OKXInstrumentType
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.common import LoggerConfig
from nautilus_trader.common import LogLevel
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import TraderId


FIXTURES = Path(__file__).resolve().parents[5] / "crates/adapters/okx/test_data"
INSTRUMENT_ID = InstrumentId.from_str("ETH-USD.OKX")
INITIAL_INTERVAL = Event()


def test_okx_depth_and_interval_python_delivery() -> None:
    """
    Check both Python callbacks using the native client and a local venue feed.
    """
    env = {key: value for key, value in os.environ.items() if not key.startswith("OKX_")}
    result = subprocess.run(
        [sys.executable, __file__, "--delivery"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=30,
        env=env,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert "OKX book delivery passed" in result.stdout


class BookServer(BaseHTTPRequestHandler):
    """
    Serve fixed OKX instruments and book messages.
    """

    def do_GET(self) -> None:  # noqa: C901 - Keep the handshake and frame loop in this fixture
        """
        Serve instrument metadata or exchange WebSocket book messages.
        """
        if self.headers.get("Upgrade", "").lower() != "websocket":
            body = (FIXTURES / "http_get_instruments_spot.json").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        key = self.headers["Sec-WebSocket-Key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
        accept = base64.b64encode(
            hashlib.sha1(key.encode(), usedforsecurity=False).digest(),
        ).decode()
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        while True:
            frame = self.rfile.read(2)
            if len(frame) != 2:
                return
            opcode = frame[0] & 15
            size = frame[1] & 127
            if size == 126:
                size = struct.unpack("!H", self.rfile.read(2))[0]
            elif size == 127:
                size = struct.unpack("!Q", self.rfile.read(8))[0]
            mask = self.rfile.read(4) if frame[1] & 128 else b""
            payload = self.rfile.read(size)
            if mask:
                payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
            if opcode == 8:
                self.wfile.write(b"\x88\x00")
                self.wfile.flush()
                return
            if payload == b"ping":
                self._send("pong")
                continue
            command = json.loads(payload)
            for arg in command.get("args", []):
                self._send(
                    json.dumps(
                        {"event": command["op"], "arg": arg, "code": "0", "connId": "book-test"},
                    ),
                )

                if command["op"] == "subscribe" and arg.get("channel") == "books5":
                    snapshot = _snapshot()
                    snapshot["arg"]["channel"] = "books5"
                    snapshot.pop("action")
                    for side in ("bids", "asks"):
                        snapshot["data"][0][side] = snapshot["data"][0][side][:5]
                    self._send(json.dumps(snapshot))
                    snapshot["data"][0]["bids"].pop(0)
                    snapshot["data"][0]["asks"][0][1] = "99"
                    snapshot["data"][0]["asks"][0][3] = "83"
                    snapshot["data"][0]["seqId"] = 123457
                    snapshot["data"][0]["ts"] = "1597026383086"
                    self._send(json.dumps(snapshot))
                if command["op"] == "subscribe" and arg.get("channel") == "books":
                    snapshot = _snapshot()
                    self._send(json.dumps(snapshot))
                    assert INITIAL_INTERVAL.wait(5), "initial interval callback missing"
                    heartbeat = json.loads(json.dumps(snapshot))
                    heartbeat["action"] = "update"
                    heartbeat["data"][0]["prevSeqId"] = 123456
                    heartbeat["data"][0]["bids"] = []
                    heartbeat["data"][0]["asks"] = []
                    self._send(json.dumps(heartbeat))
                    update = json.loads(json.dumps(snapshot))
                    update["action"] = "update"
                    update["data"][0]["prevSeqId"] = 123456
                    update["data"][0]["seqId"] = 123457
                    update["data"][0]["ts"] = "1597026383086"
                    update["data"][0]["bids"] = [[snapshot["data"][0]["bids"][0][0], "0", "0", "0"]]
                    update["data"][0]["asks"] = [
                        [snapshot["data"][0]["asks"][0][0], "99", "0", "83"],
                    ]
                    self._send(json.dumps(update))

    def _send(self, text: str) -> None:
        data = text.encode()
        header = (
            bytes([0x81, len(data)])
            if len(data) < 126
            else b"\x81\x7e" + struct.pack("!H", len(data))
        )
        self.wfile.write(header + data)
        self.wfile.flush()

    def log_message(self, format: str, *args: object) -> None:
        """
        Suppress HTTP access logs in the subprocess output.
        """


def _snapshot() -> dict:
    snapshot = json.loads((FIXTURES / "ws_books_snapshot.json").read_text(encoding="utf-8"))
    snapshot["arg"]["instId"] = "ETH-USD"
    for index in range(4):
        snapshot["data"][0]["bids"].append(
            [str(8445 - index), str(31 + index), "0", str(21 + index)],
        )
        snapshot["data"][0]["asks"].append(
            [str(8507 + index), str(51 + index), "0", str(41 + index)],
        )
    return snapshot


class BookObserver(DataActor):
    """
    Collect Python book callbacks and stop after the incremental update.
    """

    def __init__(self, handle) -> None:
        """
        Store the node control handle and received events.
        """
        super().__init__()
        self.handle = handle
        self.deltas = []
        self.depths = []
        self.books = []

    def on_start(self) -> None:
        """
        Subscribe to depth and interval delivery on the same instrument.
        """
        self.subscribe_book_deltas(INSTRUMENT_ID, BookType.L2_MBP, depth=25, managed=True)
        self.subscribe_book_depth(INSTRUMENT_ID, BookType.L2_MBP, depth=5, managed=False)
        self.subscribe_book_at_interval(INSTRUMENT_ID, BookType.L2_MBP, interval_ms=50, depth=25)

    def on_book_deltas(self, deltas) -> None:
        """
        Retain the delta events for interval book assertions.
        """
        self.deltas.append(deltas)

    def on_book_depth(self, depth) -> None:
        """
        Retain the complete depth event for assertions after shutdown.
        """
        self.depths.append(depth)
        if len(self.depths) == 2 and self.books and self.books[-1].sequence == 123457:
            self.handle.stop()

    def on_book(self, book) -> None:
        """
        Retain the interval snapshot and stop after the update arrives.
        """
        self.books.append(book)
        if book.sequence == 123456:
            INITIAL_INTERVAL.set()
        if book.sequence == 123457 and len(self.depths) == 2:
            self.handle.stop()


def _run_delivery() -> None:
    server = ThreadingHTTPServer(("127.0.0.1", 0), BookServer)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_port
    try:
        node = (
            LiveNode.builder("OKX-BOOK-PYTHON", TraderId.from_str("TESTER-001"), Environment.LIVE)
            .with_logging(LoggerConfig(stdout_level=LogLevel.ERROR))
            .with_delay_post_stop_secs(0)
            .with_delay_shutdown_secs(0)
            .with_timeout_connection(5)
            .with_timeout_disconnection_secs(2)
            .add_data_client(
                None,
                OKXDataClientFactory(),
                OKXDataClientConfig(
                    instrument_types=[OKXInstrumentType.SPOT],
                    base_url_http=f"http://127.0.0.1:{port}",
                    base_url_ws_public=f"ws://127.0.0.1:{port}/ws",
                    update_instruments_interval_mins=0,
                    book_stale_check_interval_secs=0,
                ),
            )
            .build()
        )
        observer = BookObserver(node.handle())
        node.add_actor(observer)
        node.run()
        assert len(observer.deltas) == 2
        assert len(observer.depths) == 2
        initial, updated = observer.depths
        snapshot = _snapshot()["data"][0]

        for depth, sequence, timestamp in [
            (initial, 123456, 1597026383085000000),
            (updated, 123457, 1597026383086000000),
        ]:
            assert depth.instrument_id == INSTRUMENT_ID
            assert depth.sequence == sequence
            assert depth.ts_event == timestamp
            assert depth.flags == 32
        for side, order_side in (("bids", OrderSide.BUY), ("asks", OrderSide.SELL)):
            expected = snapshot[side][:5]
            orders = getattr(initial, side)
            assert [
                (order.side, order.price.as_decimal(), order.size.as_decimal(), order.order_id)
                for order in orders
            ] == [(order_side, Decimal(level[0]), Decimal(level[1]), 0) for level in expected]
        assert initial.bid_counts == [int(level[3]) for level in snapshot["bids"][:5]]
        assert initial.ask_counts == [int(level[3]) for level in snapshot["asks"][:5]]
        assert [
            (order.side, order.price.as_decimal(), order.size.as_decimal(), order.order_id)
            for order in updated.bids
        ] == [
            (OrderSide.BUY, Decimal(level[0]), Decimal(level[1]), 0)
            for level in snapshot["bids"][1:5]
        ]
        assert updated.bid_counts == initial.bid_counts[1:]
        assert updated.asks[0].size.as_decimal() == Decimal(99)
        assert updated.ask_counts == [83, *initial.ask_counts[1:]]
        assert [
            (order.side, order.price.as_decimal(), order.size.as_decimal(), order.order_id)
            for order in updated.asks
        ] == [
            (OrderSide.SELL, Decimal(level[0]), Decimal(99) if index == 0 else Decimal(level[1]), 0)
            for index, level in enumerate(snapshot["asks"][:5])
        ]
        assert observer.books[-1].sequence == 123457
        assert observer.books[-1].bids_to_dict() == {
            Decimal(level[0]): Decimal(level[1]) for level in snapshot["bids"][1:]
        }
        assert observer.books[-1].asks_to_dict() == {
            Decimal(level[0]): Decimal(99) if index == 0 else Decimal(level[1])
            for index, level in enumerate(snapshot["asks"])
        }
        initial_book = next(book for book in observer.books if book.sequence == 123456)
        assert initial_book.bids_to_dict() == {
            Decimal(level[0]): Decimal(level[1]) for level in snapshot["bids"]
        }
        assert initial_book.asks_to_dict() == {
            Decimal(level[0]): Decimal(level[1]) for level in snapshot["asks"]
        }
        sys.stdout.write("OKX book delivery passed\n")
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    _run_delivery()
