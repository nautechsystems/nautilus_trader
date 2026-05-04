"""
Binance market-data collector — minimal raw-bytes-to-S3 v1.

One process per (venue, symbol-set). Connects to Binance public WS streams,
appends every message as a line to a local NDJSON buffer, rotates the buffer
on time/size, uploads completed shards to S3.

Designed to run under systemd with Restart=always. State is recovered from
on-disk buffer files: a process restart does not lose the in-flight buffer.

Metrics exposed on --prom-port:
  collector_last_event_ts{venue, stream}      Unix ts of last received event
  collector_messages_total{venue, stream}      Counter of messages received
  collector_disconnects_total{venue}            Counter of WS disconnections
  collector_bytes_written_total{venue}          Counter of bytes written to buffer
  collector_upload_failures_total{venue}        Counter of S3 upload failures
  collector_queue_depth{venue}                  Current internal queue depth

The collector intentionally does NOT canonicalize messages here. That is a
downstream ETL job (see ../lib/basis/contracts.py and the canonicalization
worker in Phase 1). This stays as raw as possible to minimize the chance
of a bug in the collector corrupting the archive.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import os
import signal
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import boto3
import websockets
from prometheus_client import Counter, Gauge, start_http_server

log = logging.getLogger("binance_collector")

# --- Config ---

WS_ENDPOINTS = {
    "spot": "wss://stream.binance.com:9443",
    "umfutures": "wss://fstream.binance.com",
}

DEFAULT_BUFFER_DIR = Path("/var/lib/binance-collector/buffer")
DEFAULT_FAILED_DIR = Path("/var/lib/binance-collector/failed")
ROTATION_INTERVAL_S = 60         # rotate every 60s
ROTATION_SIZE_BYTES = 50 * 1024 * 1024  # or 50 MB, whichever first
QUEUE_MAXSIZE = 50_000           # backpressure threshold

# --- Metrics ---

m_last_event_ts = Gauge(
    "collector_last_event_ts",
    "Unix timestamp of last received event",
    ["venue", "stream"],
)
m_messages_total = Counter(
    "collector_messages_total",
    "Messages received",
    ["venue", "stream"],
)
m_disconnects_total = Counter(
    "collector_disconnects_total",
    "WebSocket disconnections",
    ["venue"],
)
m_bytes_written_total = Counter(
    "collector_bytes_written_total",
    "Bytes written to local buffer",
    ["venue"],
)
m_upload_failures_total = Counter(
    "collector_upload_failures_total",
    "S3 upload failures",
    ["venue"],
)
m_uploads_total = Counter(
    "collector_uploads_total",
    "S3 uploads completed",
    ["venue"],
)
m_queue_depth = Gauge(
    "collector_queue_depth",
    "Internal queue depth",
    ["venue"],
)


# --- Stream task: one per (symbol, stream_kind) ---

async def stream_task(
    venue: str,
    symbol: str,
    stream_kind: str,
    queue: asyncio.Queue,
    shutdown: asyncio.Event,
) -> None:
    """Connect to Binance WS, push each message into queue. Reconnect with backoff."""
    url = f"{WS_ENDPOINTS[venue]}/ws/{symbol.lower()}@{stream_kind}"
    backoff = 1.0
    while not shutdown.is_set():
        try:
            async with websockets.connect(
                url,
                ping_interval=20,
                ping_timeout=10,
                close_timeout=5,
                max_size=2**22,
            ) as ws:
                log.info("[%s/%s/%s] connected", venue, symbol, stream_kind)
                backoff = 1.0
                async for raw in ws:
                    if shutdown.is_set():
                        break
                    now = time.time()
                    try:
                        queue.put_nowait((venue, symbol, stream_kind, raw, now))
                    except asyncio.QueueFull:
                        log.error(
                            "[%s/%s/%s] queue full, dropping message",
                            venue, symbol, stream_kind,
                        )
                        continue
                    m_messages_total.labels(venue=venue, stream=stream_kind).inc()
                    m_last_event_ts.labels(venue=venue, stream=stream_kind).set(now)
        except asyncio.CancelledError:
            raise
        except Exception as e:
            m_disconnects_total.labels(venue=venue).inc()
            log.warning("[%s/%s/%s] disconnect: %s", venue, symbol, stream_kind, e)
            try:
                await asyncio.wait_for(shutdown.wait(), timeout=backoff)
                return  # shutdown signaled during backoff
            except asyncio.TimeoutError:
                pass
            backoff = min(backoff * 2, 60.0)


# --- Writer task: drains queue, rotates buffer, uploads ---

class BufferWriter:
    def __init__(
        self,
        venue: str,
        bucket: str,
        key_prefix: str,
        s3_client,
        buffer_dir: Path,
        failed_dir: Path,
    ):
        self.venue = venue
        self.bucket = bucket
        self.key_prefix = key_prefix
        self.s3 = s3_client
        self.buffer_dir = buffer_dir
        self.failed_dir = failed_dir
        self.buffer_dir.mkdir(parents=True, exist_ok=True)
        self.failed_dir.mkdir(parents=True, exist_ok=True)

        self._fh = None
        self._path: Path | None = None
        self._open_ts: float = 0.0
        self._bytes_written: int = 0

    def _open_new(self) -> None:
        ts_int = int(time.time())
        self._path = self.buffer_dir / f"{self.venue}-{ts_int}.ndjson"
        self._fh = open(self._path, "ab")
        self._open_ts = float(ts_int)
        self._bytes_written = 0

    def _rotate(self) -> tuple[Path, float]:
        assert self._fh and self._path
        self._fh.close()
        old_path = self._path
        old_ts = self._open_ts
        self._open_new()
        return old_path, old_ts

    def _should_rotate(self) -> bool:
        return (
            (time.time() - self._open_ts) >= ROTATION_INTERVAL_S
            or self._bytes_written >= ROTATION_SIZE_BYTES
        )

    def write(self, line: bytes) -> None:
        if self._fh is None:
            self._open_new()
        self._fh.write(line)
        self._bytes_written += len(line)

    def s3_key(self, path: Path, open_ts: float) -> str:
        dt = datetime.fromtimestamp(open_ts, tz=timezone.utc)
        return (
            f"{self.key_prefix}/venue={self.venue}/"
            f"year={dt.year:04d}/month={dt.month:02d}/"
            f"day={dt.day:02d}/hour={dt.hour:02d}/{path.name}"
        )

    def upload_sync(self, path: Path, open_ts: float) -> bool:
        """Synchronous upload. Returns True on success."""
        try:
            self.s3.upload_file(str(path), self.bucket, self.s3_key(path, open_ts))
            return True
        except Exception as e:
            log.error("[upload] %s: %s", path, e)
            return False


async def writer_task(
    queue: asyncio.Queue,
    writer: BufferWriter,
    shutdown: asyncio.Event,
) -> None:
    """Drain queue → buffer file → on rotation, upload to S3."""
    log.info("[writer] starting for venue=%s bucket=%s", writer.venue, writer.bucket)
    pending_uploads: list[asyncio.Task] = []
    while not shutdown.is_set():
        # Pull from queue with short timeout so rotation check runs even when idle
        try:
            item = await asyncio.wait_for(queue.get(), timeout=1.0)
        except asyncio.TimeoutError:
            item = None

        if item is not None:
            venue_, symbol, stream_kind, raw, recv_ts = item
            try:
                # raw is bytes from websockets; decode + re-encode as wrapped JSON
                msg = json.loads(raw)
                line = (
                    json.dumps(
                        {
                            "ts": recv_ts,
                            "symbol": symbol,
                            "stream": stream_kind,
                            "msg": msg,
                        },
                        separators=(",", ":"),
                    ).encode()
                    + b"\n"
                )
            except Exception as e:
                log.error("[writer] failed to encode msg: %s", e)
                continue
            writer.write(line)
            m_bytes_written_total.labels(venue=writer.venue).inc(len(line))

        m_queue_depth.labels(venue=writer.venue).set(queue.qsize())

        if writer._fh is not None and writer._should_rotate():
            old_path, old_ts = writer._rotate()
            pending_uploads.append(
                asyncio.create_task(_upload(writer, old_path, old_ts))
            )

        # Reap finished uploads
        pending_uploads = [t for t in pending_uploads if not t.done()]

    # On shutdown: rotate and upload one last time
    log.info("[writer] shutdown signaled, flushing buffer")
    if writer._fh is not None and writer._bytes_written > 0:
        old_path, old_ts = writer._rotate()
        pending_uploads.append(asyncio.create_task(_upload(writer, old_path, old_ts)))
    if pending_uploads:
        await asyncio.gather(*pending_uploads, return_exceptions=True)


async def _upload(writer: BufferWriter, path: Path, open_ts: float) -> None:
    """Run S3 upload in a thread and handle failure."""
    loop = asyncio.get_running_loop()
    ok = await loop.run_in_executor(None, writer.upload_sync, path, open_ts)
    if ok:
        m_uploads_total.labels(venue=writer.venue).inc()
        try:
            path.unlink()
        except OSError:
            pass
    else:
        m_upload_failures_total.labels(venue=writer.venue).inc()
        # Move to failed-dir for manual retry. Do NOT delete.
        target = writer.failed_dir / path.name
        try:
            path.rename(target)
            log.warning("[upload] moved failed shard to %s", target)
        except OSError as e:
            log.error("[upload] could not move %s to failed dir: %s", path, e)


# --- Main ---

def install_signal_handlers(shutdown: asyncio.Event, loop: asyncio.AbstractEventLoop) -> None:
    def _handle(sig):
        log.info("received signal %s, shutting down", sig)
        shutdown.set()
    for s in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(s, _handle, s)


def streams_for_venue(venue: str) -> list[str]:
    if venue == "spot":
        return ["depth@100ms", "trade"]
    if venue == "umfutures":
        return ["depth@100ms", "trade", "markPrice@1s"]
    raise ValueError(f"unknown venue: {venue}")


async def amain(args) -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )

    start_http_server(args.prom_port)
    log.info("prometheus exporter on :%d", args.prom_port)

    s3 = boto3.client("s3", region_name=args.region)
    queue: asyncio.Queue = asyncio.Queue(maxsize=QUEUE_MAXSIZE)
    shutdown = asyncio.Event()

    writer = BufferWriter(
        venue=args.venue,
        bucket=args.bucket,
        key_prefix=args.key_prefix,
        s3_client=s3,
        buffer_dir=Path(args.buffer_dir),
        failed_dir=Path(args.failed_dir),
    )

    install_signal_handlers(shutdown, asyncio.get_running_loop())

    streams = streams_for_venue(args.venue)
    symbols = [s.strip() for s in args.symbols.split(",") if s.strip()]

    tasks: list[asyncio.Task] = [
        asyncio.create_task(writer_task(queue, writer, shutdown), name="writer"),
    ]
    for sym in symbols:
        for sk in streams:
            tasks.append(
                asyncio.create_task(
                    stream_task(args.venue, sym, sk, queue, shutdown),
                    name=f"stream-{sym}-{sk}",
                )
            )

    log.info(
        "running venue=%s symbols=%s streams=%s",
        args.venue, symbols, streams,
    )

    await shutdown.wait()
    log.info("shutting down %d tasks", len(tasks))
    for t in tasks:
        t.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)
    return 0


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Binance market-data collector")
    p.add_argument("--venue", choices=["spot", "umfutures"], required=True)
    p.add_argument("--symbols", required=True, help="comma-separated, e.g. BTCUSDT or BTCUSDT_250926")
    p.add_argument("--bucket", required=True)
    p.add_argument("--key-prefix", default="raw")
    p.add_argument("--prom-port", type=int, default=9101)
    p.add_argument("--region", default="ap-northeast-1")
    p.add_argument("--buffer-dir", default=str(DEFAULT_BUFFER_DIR))
    p.add_argument("--failed-dir", default=str(DEFAULT_FAILED_DIR))
    return p.parse_args()


def main() -> int:
    args = parse_args()
    return asyncio.run(amain(args))


if __name__ == "__main__":
    sys.exit(main())
