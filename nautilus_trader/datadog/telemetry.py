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
Small, dependency-free DogStatsD telemetry client.

The critical trading path records metrics by placing pre-formatted samples onto a
bounded queue. A background thread owns UDP I/O to the local Datadog Agent. If
the telemetry queue fills, samples are dropped so trading can continue.
"""

from __future__ import annotations

import atexit
import os
import queue
import random
import socket
import threading
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Final


DEFAULT_DOGSTATSD_HOST: Final[str] = "127.0.0.1"
DEFAULT_DOGSTATSD_PORT: Final[int] = 8125
DEFAULT_NAMESPACE: Final[str] = "nautilus"
DEFAULT_QUEUE_SIZE: Final[int] = 100_000
DEFAULT_FLUSH_INTERVAL: Final[float] = 0.25
DEFAULT_PACKET_SIZE: Final[int] = 8_192


@dataclass(frozen=True, slots=True)
class DatadogTelemetryConfig:
    """
    Configuration for Datadog DogStatsD telemetry.

    Parameters
    ----------
    enabled : bool, default True
        If the telemetry worker should run.
    host : str, default "127.0.0.1"
        DogStatsD host.
    port : int, default 8125
        DogStatsD UDP port.
    namespace : str, default "nautilus"
        Metric namespace prepended to all submitted metric names.
    constant_tags : tuple[str, ...], default ()
        Tags added to every emitted metric.
    queue_size : int, default 100_000
        Bounded in-process telemetry queue size.
    default_sample_rate : float, default 1.0
        Default sample rate for recorded samples.
    flush_interval : float, default 0.25
        Worker queue polling interval in seconds.
    max_packet_size : int, default 8192
        Maximum DogStatsD packet size in bytes.

    """

    enabled: bool = True
    host: str = DEFAULT_DOGSTATSD_HOST
    port: int = DEFAULT_DOGSTATSD_PORT
    namespace: str = DEFAULT_NAMESPACE
    constant_tags: tuple[str, ...] = ()
    queue_size: int = DEFAULT_QUEUE_SIZE
    default_sample_rate: float = 1.0
    flush_interval: float = DEFAULT_FLUSH_INTERVAL
    max_packet_size: int = DEFAULT_PACKET_SIZE

    @classmethod
    def from_env(cls, *, enabled: bool = True) -> DatadogTelemetryConfig:
        """
        Build a config from Datadog and Nautilus environment variables.

        Recognized variables are ``DD_DOGSTATSD_HOST``, ``DD_DOGSTATSD_PORT``,
        ``DD_TAGS``, ``NAUTILUS_DATADOG_NAMESPACE``,
        ``NAUTILUS_DATADOG_QUEUE_SIZE`` and ``NAUTILUS_DATADOG_SAMPLE_RATE``.
        """
        return cls(
            enabled=enabled,
            host=os.getenv("DD_DOGSTATSD_HOST", DEFAULT_DOGSTATSD_HOST),
            port=_int_from_env("DD_DOGSTATSD_PORT", DEFAULT_DOGSTATSD_PORT),
            namespace=os.getenv("NAUTILUS_DATADOG_NAMESPACE", DEFAULT_NAMESPACE),
            constant_tags=_parse_tags(os.getenv("DD_TAGS", "")),
            queue_size=_int_from_env("NAUTILUS_DATADOG_QUEUE_SIZE", DEFAULT_QUEUE_SIZE),
            default_sample_rate=_float_from_env("NAUTILUS_DATADOG_SAMPLE_RATE", 1.0),
        )


@dataclass(frozen=True, slots=True)
class _MetricSample:
    name: str
    value: float
    metric_type: str
    tags: tuple[str, ...]
    sample_rate: float


@dataclass(frozen=True, slots=True)
class DatadogTelemetryStats:
    sent: int
    dropped: int
    send_errors: int
    queued: int


class DatadogTelemetry:
    """
    Asynchronous DogStatsD client with drop-on-full behavior.
    """

    def __init__(self, config: DatadogTelemetryConfig | None = None) -> None:
        self.config = config or DatadogTelemetryConfig.from_env()
        self._queue: queue.Queue[_MetricSample] = queue.Queue(maxsize=self.config.queue_size)
        self._running = threading.Event()
        self._thread: threading.Thread | None = None
        self._sock: socket.socket | None = None
        self._addr = (self.config.host, self.config.port)
        self._sent = 0
        self._dropped = 0
        self._send_errors = 0
        self._lock = threading.Lock()

    @property
    def is_enabled(self) -> bool:
        return self.config.enabled and self._running.is_set()

    def start(self) -> None:
        if not self.config.enabled or self._running.is_set():
            return

        self._sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self._running.set()
        self._thread = threading.Thread(
            target=self._run,
            name="nautilus-datadog-telemetry",
            daemon=True,
        )
        self._thread.start()

    def close(self, timeout: float = 2.0) -> None:
        self._running.clear()
        if self._thread is not None:
            self._thread.join(timeout=timeout)
            self._thread = None

        if self._sock is not None:
            self._sock.close()
            self._sock = None

    def stats(self) -> DatadogTelemetryStats:
        return DatadogTelemetryStats(
            sent=self._sent,
            dropped=self._dropped,
            send_errors=self._send_errors,
            queued=self._queue.qsize(),
        )

    def increment(
        self,
        name: str,
        value: int = 1,
        tags: Sequence[str] | None = None,
        sample_rate: float | None = None,
    ) -> None:
        self.record(name, value, "c", tags, sample_rate)

    def gauge(
        self,
        name: str,
        value: float,
        tags: Sequence[str] | None = None,
        sample_rate: float | None = None,
    ) -> None:
        self.record(name, value, "g", tags, sample_rate)

    def distribution(
        self,
        name: str,
        value: float,
        tags: Sequence[str] | None = None,
        sample_rate: float | None = None,
    ) -> None:
        self.record(name, value, "d", tags, sample_rate)

    def record(
        self,
        name: str,
        value: float,
        metric_type: str,
        tags: Sequence[str] | None = None,
        sample_rate: float | None = None,
    ) -> None:
        if not self.is_enabled:
            return

        rate = self.config.default_sample_rate if sample_rate is None else sample_rate
        if rate <= 0.0:
            return
        if rate < 1.0 and random.random() > rate:  # noqa: S311
            return

        sample = _MetricSample(
            name=name,
            value=value,
            metric_type=metric_type,
            tags=tuple(tags or ()),
            sample_rate=rate,
        )
        try:
            self._queue.put_nowait(sample)
        except queue.Full:
            self._increment_dropped()

    def _run(self) -> None:
        while self._running.is_set() or not self._queue.empty():
            try:
                sample = self._queue.get(timeout=self.config.flush_interval)
            except queue.Empty:
                continue

            self._send(sample)

    def _send(self, sample: _MetricSample) -> None:
        sock = self._sock
        if sock is None:
            self._increment_dropped()
            return

        line = _format_line(
            namespace=self.config.namespace,
            constant_tags=self.config.constant_tags,
            sample=sample,
        )
        packet = line.encode("utf-8")
        if len(packet) > self.config.max_packet_size:
            self._increment_dropped()
            return

        try:
            sock.sendto(packet, self._addr)
            with self._lock:
                self._sent += 1
        except OSError:
            with self._lock:
                self._send_errors += 1

    def _increment_dropped(self) -> None:
        with self._lock:
            self._dropped += 1


_GLOBAL_TELEMETRY: DatadogTelemetry | None = None
_GLOBAL_LOCK = threading.Lock()


def configure(config: DatadogTelemetryConfig | None = None) -> DatadogTelemetry:
    """
    Configure and start global Datadog telemetry.
    """
    global _GLOBAL_TELEMETRY

    telemetry = DatadogTelemetry(config)
    telemetry.start()
    with _GLOBAL_LOCK:
        previous = _GLOBAL_TELEMETRY
        _GLOBAL_TELEMETRY = telemetry

    if previous is not None:
        previous.close()

    return telemetry


def stop(timeout: float = 2.0) -> None:
    """
    Stop global Datadog telemetry.
    """
    global _GLOBAL_TELEMETRY

    with _GLOBAL_LOCK:
        telemetry = _GLOBAL_TELEMETRY
        _GLOBAL_TELEMETRY = None

    if telemetry is not None:
        telemetry.close(timeout=timeout)


def enabled() -> bool:
    telemetry = _GLOBAL_TELEMETRY
    return telemetry is not None and telemetry.is_enabled


def increment(
    name: str,
    value: int = 1,
    tags: Sequence[str] | None = None,
    sample_rate: float | None = None,
) -> None:
    telemetry = _GLOBAL_TELEMETRY
    if telemetry is not None:
        telemetry.increment(name, value, tags, sample_rate)


def gauge(
    name: str,
    value: float,
    tags: Sequence[str] | None = None,
    sample_rate: float | None = None,
) -> None:
    telemetry = _GLOBAL_TELEMETRY
    if telemetry is not None:
        telemetry.gauge(name, value, tags, sample_rate)


def distribution(
    name: str,
    value: float,
    tags: Sequence[str] | None = None,
    sample_rate: float | None = None,
) -> None:
    telemetry = _GLOBAL_TELEMETRY
    if telemetry is not None:
        telemetry.distribution(name, value, tags, sample_rate)


def _format_line(
    namespace: str,
    constant_tags: Sequence[str],
    sample: _MetricSample,
) -> str:
    metric_name = ".".join(part for part in (namespace, sample.name) if part)
    line = f"{_clean_metric(metric_name)}:{sample.value}|{sample.metric_type}"

    if sample.sample_rate < 1.0:
        line = f"{line}|@{sample.sample_rate:g}"

    tags = tuple(_clean_tag(tag) for tag in (*constant_tags, *sample.tags) if tag)
    if tags:
        line = f"{line}|#{','.join(tags)}"

    return line


def _parse_tags(raw_tags: str) -> tuple[str, ...]:
    normalized = raw_tags.replace(",", " ")
    return tuple(tag for tag in normalized.split() if tag)


def _clean_metric(value: str) -> str:
    return _clean_token(value).replace(":", "_")


def _clean_tag(value: str) -> str:
    return _clean_token(value)


def _clean_token(value: object) -> str:
    return str(value).replace("|", "_").replace(",", "_").replace("\n", "_").strip()


def _int_from_env(name: str, default: int) -> int:
    raw = os.getenv(name)
    if raw is None:
        return default
    try:
        return int(raw)
    except ValueError:
        return default


def _float_from_env(name: str, default: float) -> float:
    raw = os.getenv(name)
    if raw is None:
        return default
    try:
        return float(raw)
    except ValueError:
        return default


atexit.register(stop)
