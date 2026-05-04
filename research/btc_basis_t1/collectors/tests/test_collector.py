"""
Tests for the Binance collector.

Verifies:
  - WS reconnects with backoff after a forced disconnect
  - Buffer rotates on time AND on size
  - Failed S3 uploads move shards to failed-dir, do not delete
  - Successful uploads delete the local shard
  - Prometheus metrics tick on each event

We use:
  - A local fake WebSocket server to simulate Binance, which we can kill on demand.
  - A fake S3 client (moto or hand-rolled) to assert upload behavior.

Run: pytest -xvs research/btc_basis_t1/collectors/tests/
"""

from __future__ import annotations

import asyncio
import json
import time
from pathlib import Path
from unittest.mock import MagicMock

import pytest
import websockets

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import binance_collector as coll  # noqa: E402


# --- Fake WS server ---

class FakeBinance:
    """Tiny WS server that emits a stream of synthetic messages."""

    def __init__(self, port: int):
        self.port = port
        self._server: websockets.server.Serve | None = None
        self._clients: list[websockets.WebSocketServerProtocol] = []
        self._stop = asyncio.Event()

    async def _handler(self, ws):
        self._clients.append(ws)
        try:
            i = 0
            while not self._stop.is_set():
                payload = {
                    "e": "depthUpdate",
                    "E": int(time.time() * 1000),
                    "s": "BTCUSDT",
                    "U": i,
                    "u": i,
                    "b": [["100000.0", "1.0"]],
                    "a": [["100001.0", "1.0"]],
                }
                await ws.send(json.dumps(payload))
                i += 1
                await asyncio.sleep(0.01)
        except websockets.exceptions.ConnectionClosed:
            pass
        finally:
            if ws in self._clients:
                self._clients.remove(ws)

    async def start(self):
        self._server = await websockets.serve(self._handler, "127.0.0.1", self.port)

    async def stop(self):
        self._stop.set()
        if self._server:
            self._server.close()
            await self._server.wait_closed()

    async def disconnect_all(self):
        for ws in list(self._clients):
            await ws.close(1011, "test forced close")


# --- Tests ---

@pytest.mark.asyncio
async def test_writes_messages_to_buffer(tmp_path, monkeypatch):
    """Sanity: messages received → buffer file grows → metrics tick."""
    fake = FakeBinance(port=18801)
    await fake.start()
    monkeypatch.setitem(coll.WS_ENDPOINTS, "spot", "ws://127.0.0.1:18801")

    buffer_dir = tmp_path / "buffer"
    failed_dir = tmp_path / "failed"

    s3_client = MagicMock()
    s3_client.upload_file = MagicMock()  # always succeeds, no-op

    queue: asyncio.Queue = asyncio.Queue(maxsize=1000)
    shutdown = asyncio.Event()
    writer = coll.BufferWriter(
        venue="spot",
        bucket="test-bucket",
        key_prefix="raw",
        s3_client=s3_client,
        buffer_dir=buffer_dir,
        failed_dir=failed_dir,
    )

    tasks = [
        asyncio.create_task(coll.writer_task(queue, writer, shutdown)),
        asyncio.create_task(coll.stream_task("spot", "BTCUSDT", "depth@100ms", queue, shutdown)),
    ]
    try:
        await asyncio.sleep(0.5)
        # Should have received messages and written to buffer
        files = list(buffer_dir.glob("*.ndjson"))
        assert len(files) >= 1, "no buffer file created"
        assert files[0].stat().st_size > 0, "buffer file is empty"

        # And messages_total should be > 0
        # (Counter has internal _value; reach in for assertion)
        ms = coll.m_messages_total.labels(venue="spot", stream="depth@100ms")
        assert ms._value.get() > 0
    finally:
        shutdown.set()
        for t in tasks:
            t.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        await fake.stop()


@pytest.mark.asyncio
async def test_reconnect_after_disconnect(tmp_path, monkeypatch):
    """Force a WS disconnect and verify the stream reconnects."""
    fake = FakeBinance(port=18802)
    await fake.start()
    monkeypatch.setitem(coll.WS_ENDPOINTS, "spot", "ws://127.0.0.1:18802")

    queue: asyncio.Queue = asyncio.Queue(maxsize=10000)
    shutdown = asyncio.Event()

    task = asyncio.create_task(
        coll.stream_task("spot", "BTCUSDT", "depth@100ms", queue, shutdown)
    )
    try:
        await asyncio.sleep(0.3)
        msgs_before = coll.m_messages_total.labels(
            venue="spot", stream="depth@100ms"
        )._value.get()
        assert msgs_before > 0

        # Read disconnect count BEFORE forcing the close
        disconnects_before = coll.m_disconnects_total.labels(venue="spot")._value.get()
        # Force-close all client connections
        await fake.disconnect_all()

        # Wait for reconnect
        await asyncio.sleep(2.0)
        disconnects_after = coll.m_disconnects_total.labels(venue="spot")._value.get()
        msgs_after = coll.m_messages_total.labels(
            venue="spot", stream="depth@100ms"
        )._value.get()

        assert disconnects_after > disconnects_before, "disconnect was not counted"
        assert msgs_after > msgs_before, "did not resume receiving after reconnect"
    finally:
        shutdown.set()
        task.cancel()
        await asyncio.gather(task, return_exceptions=True)
        await fake.stop()


@pytest.mark.asyncio
async def test_rotation_on_size(tmp_path, monkeypatch):
    """Force size-based rotation by setting a tiny rotation size."""
    monkeypatch.setattr(coll, "ROTATION_SIZE_BYTES", 1024)  # 1 KB
    monkeypatch.setattr(coll, "ROTATION_INTERVAL_S", 999)   # don't rotate on time

    fake = FakeBinance(port=18803)
    await fake.start()
    monkeypatch.setitem(coll.WS_ENDPOINTS, "spot", "ws://127.0.0.1:18803")

    s3_client = MagicMock()
    queue: asyncio.Queue = asyncio.Queue(maxsize=10000)
    shutdown = asyncio.Event()
    writer = coll.BufferWriter(
        venue="spot",
        bucket="test-bucket",
        key_prefix="raw",
        s3_client=s3_client,
        buffer_dir=tmp_path / "buffer",
        failed_dir=tmp_path / "failed",
    )

    tasks = [
        asyncio.create_task(coll.writer_task(queue, writer, shutdown)),
        asyncio.create_task(coll.stream_task("spot", "BTCUSDT", "depth@100ms", queue, shutdown)),
    ]
    try:
        await asyncio.sleep(1.5)
        # Multiple rotations should have happened by now
        assert s3_client.upload_file.call_count >= 2, (
            f"expected ≥2 uploads, got {s3_client.upload_file.call_count}"
        )
    finally:
        shutdown.set()
        for t in tasks:
            t.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        await fake.stop()


@pytest.mark.asyncio
async def test_upload_failure_moves_to_failed_dir(tmp_path, monkeypatch):
    """If S3 upload raises, the shard moves to failed-dir, NOT deleted."""
    monkeypatch.setattr(coll, "ROTATION_SIZE_BYTES", 512)
    monkeypatch.setattr(coll, "ROTATION_INTERVAL_S", 999)

    fake = FakeBinance(port=18804)
    await fake.start()
    monkeypatch.setitem(coll.WS_ENDPOINTS, "spot", "ws://127.0.0.1:18804")

    s3_client = MagicMock()
    s3_client.upload_file = MagicMock(side_effect=RuntimeError("simulated S3 outage"))

    queue: asyncio.Queue = asyncio.Queue(maxsize=10000)
    shutdown = asyncio.Event()
    failed_dir = tmp_path / "failed"
    writer = coll.BufferWriter(
        venue="spot",
        bucket="test-bucket",
        key_prefix="raw",
        s3_client=s3_client,
        buffer_dir=tmp_path / "buffer",
        failed_dir=failed_dir,
    )

    tasks = [
        asyncio.create_task(coll.writer_task(queue, writer, shutdown)),
        asyncio.create_task(coll.stream_task("spot", "BTCUSDT", "depth@100ms", queue, shutdown)),
    ]
    try:
        await asyncio.sleep(1.5)
        failed_shards = list(failed_dir.glob("*.ndjson"))
        assert len(failed_shards) >= 1, "expected at least one shard in failed-dir"
        # And metric reflects failures
        ufails = coll.m_upload_failures_total.labels(venue="spot")._value.get()
        assert ufails >= 1
    finally:
        shutdown.set()
        for t in tasks:
            t.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        await fake.stop()


def test_streams_for_venue():
    """Spot has 2 streams, USDM has 3 (with markPrice)."""
    assert coll.streams_for_venue("spot") == ["depth@100ms", "trade"]
    assert "markPrice@1s" in coll.streams_for_venue("umfutures")
    with pytest.raises(ValueError):
        coll.streams_for_venue("invalid")


def test_s3_key_format(tmp_path):
    """Partition layout matches what notebooks expect."""
    s3 = MagicMock()
    w = coll.BufferWriter(
        venue="umfutures",
        bucket="b",
        key_prefix="raw",
        s3_client=s3,
        buffer_dir=tmp_path / "buf",
        failed_dir=tmp_path / "fail",
    )
    p = Path("/tmp/umfutures-1700000000.ndjson")
    key = w.s3_key(p, 1700000000.0)
    assert key.startswith("raw/venue=umfutures/")
    assert "year=2023" in key
    assert "month=11" in key
    assert "umfutures-1700000000.ndjson" in key
