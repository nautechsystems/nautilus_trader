from __future__ import annotations

import argparse
import inspect
import json
import logging
import os
import signal
import time
from dataclasses import dataclass
from typing import Any

import redis

from flux.bridge.handlers import default_topic_handlers
from flux.bridge.handlers.types import CorrelationContext
from flux.bridge.handlers.types import HandlerFn
from flux.bridge.handlers.types import ReplaceHashJSONOp
from flux.bridge.handlers.types import SetJSONOp
from flux.bridge.handlers.types import StreamJSONOp
from flux.bridge.handlers.types import WriteOp
from flux.bridge.handlers.utils import coerce_ts_ms
from flux.bridge.handlers.utils import decode_text
from flux.bridge.handlers.utils import first_text
from flux.bridge.handlers.utils import load_json_payload
from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.common.keys import FluxRedisKeys
from flux.common.config import validate_identifier_part
from flux.common.config import validate_schema_version
from flux.events import TOPIC_EXECUTION_ALERT


def _json_default(value: Any) -> Any:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    raise TypeError(f"Object of type {type(value).__name__} is not JSON serializable")


def _to_json(value: Any) -> str:
    return json.dumps(value, separators=(",", ":"), default=_json_default)


FULL_BOOTSTRAP_REPLAY_TOPIC_SUFFIXES = frozenset({"market_bbo"})


@dataclass(frozen=True)
class StreamCoordinates:
    environment: str
    strategy_id: str
    topic: str


class FluxBridgeStreamConsumer:
    BRIDGE_ALERTS_MAXLEN = 2_000

    def __init__(
        self,
        *,
        redis_client: redis.Redis,
        environment: str,
        strategy_id: str | None = None,
        strategy_ids: list[str] | tuple[str, ...] | None = None,
        stream_strategy_ids: list[str] | tuple[str, ...] | None = None,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
        handlers: dict[str, HandlerFn] | None = None,
        topics: list[str] | None = None,
        start_id: str = "$",
        block_ms: int = 1_000,
        read_count: int = 200,
        scan_interval_sec: float = 3.0,
        logger: logging.Logger | None = None,
    ) -> None:
        self._logger = logger or logging.getLogger("nautilus-flux-bridge")
        self._redis = redis_client
        self._namespace = validate_identifier_part(namespace, "namespace")
        self._schema_version = validate_schema_version(schema_version, "schema_version")
        self._environment = validate_identifier_part(environment, "environment")
        if strategy_id is not None and strategy_ids is not None:
            raise ValueError("Provide only one of `strategy_id` or `strategy_ids`")
        scoped_strategy_ids = list(strategy_ids or ())
        if strategy_id is not None:
            scoped_strategy_ids.append(strategy_id)
        self._strategy_ids = (
            frozenset(
                validate_identifier_part(current_strategy_id, "strategy_id")
                for current_strategy_id in scoped_strategy_ids
            )
            if scoped_strategy_ids
            else None
        )
        scoped_stream_strategy_ids = list(stream_strategy_ids or ())
        self._stream_strategy_ids = (
            frozenset(
                validate_identifier_part(current_strategy_id, "stream_strategy_id")
                for current_strategy_id in scoped_stream_strategy_ids
            )
            if scoped_stream_strategy_ids
            else self._strategy_ids
        )
        self._handlers = handlers or default_topic_handlers()
        self._topics = sorted(topics or self._handlers.keys())
        for topic in self._topics:
            validate_identifier_part(topic, "topic")
            if topic not in self._handlers:
                raise ValueError(f"Missing handler for topic {topic!r}")

        self._start_id = decode_text(start_id) or "$"
        self._block_ms = max(10, int(block_ms))
        self._read_count = max(1, int(read_count))
        self._scan_interval_sec = max(0.25, float(scan_interval_sec))

        self._running = True
        self._last_scan_ts = 0.0
        self._stream_ids: dict[str, str] = {}
        self._write_failure_entry: tuple[str, str] | None = None
        self._write_failure_streak = 0
        self._write_failure_first_failure_s = 0.0

    def _shutdown(self) -> None:
        close = getattr(self._redis, "close", None)
        if callable(close):
            try:
                close()
            except Exception as exc:
                self._logger.warning("Failed to close redis client cleanly: %s", exc)

        connection_pool = getattr(self._redis, "connection_pool", None)
        disconnect = getattr(connection_pool, "disconnect", None)
        if callable(disconnect):
            try:
                parameter_names = tuple(inspect.signature(disconnect).parameters)
                if "in_use_connections" in parameter_names:
                    disconnect(in_use_connections=False)
                elif "inuse_connections" in parameter_names:
                    disconnect(inuse_connections=False)
                else:
                    disconnect()
            except Exception as exc:
                self._logger.warning("Failed to disconnect redis connection pool cleanly: %s", exc)

    def _normalize_topic_name(self, topic: Any) -> str:
        topic_text = first_text(topic)
        if not topic_text:
            return ""
        if topic_text in self._handlers:
            return topic_text
        if "." in topic_text:
            suffix = topic_text.rsplit(".", maxsplit=1)[-1]
            if suffix in self._handlers:
                return suffix
        return topic_text

    def _unwrap_payload_envelope(self, payload: Any) -> tuple[str, Any]:
        if not isinstance(payload, dict):
            return "", payload

        payload_type = self._normalize_topic_name(payload.get("type"))
        if "FluxBusPayload" not in payload_type:
            return "", payload

        topic = self._normalize_topic_name(payload.get("topic"))
        inner = payload.get("payload")
        if isinstance(inner, dict):
            return topic, dict(inner)
        if isinstance(inner, list):
            return topic, {"rows": inner}
        if isinstance(inner, bytes | str):
            parsed = load_json_payload(inner)
            if isinstance(parsed, dict):
                return topic, dict(parsed)
            if isinstance(parsed, list):
                return topic, {"rows": parsed}
            if parsed is None:
                return topic, {}
            return topic, {"value": parsed}
        if inner is None:
            return topic, {}
        return topic, {"value": inner}

    @property
    def _prefix(self) -> str:
        return f"{self._namespace}:{self._schema_version}"

    def _install_signals(self) -> None:
        signal.signal(signal.SIGINT, self._on_signal)
        signal.signal(signal.SIGTERM, self._on_signal)

    def _on_signal(self, sig: int, _frame: Any) -> None:
        self._logger.info("Received signal %s, stopping bridge consumer", sig)
        self._running = False

    def _scan_patterns(self) -> list[str]:
        patterns: list[str] = []
        for topic in self._topics:
            if self._stream_strategy_ids is None:
                patterns.append(f"{self._prefix}:in:stream:{self._environment}:*:{topic}")
                continue
            for strategy_id in sorted(self._stream_strategy_ids):
                patterns.append(
                    f"{self._prefix}:in:stream:{self._environment}:{strategy_id}:{topic}",
                )
        return patterns

    def _parse_stream_key(self, key: str) -> StreamCoordinates | None:
        parts = key.split(":")
        if len(parts) != 7:
            return None
        namespace, schema_version, domain, bucket, environment, strategy_id, topic = parts
        if (
            namespace != self._namespace
            or schema_version != self._schema_version
            or domain != "in"
            or bucket != "stream"
        ):
            return None
        if environment != self._environment:
            return None
        if self._stream_strategy_ids is not None and strategy_id not in self._stream_strategy_ids:
            return None
        if topic not in self._handlers:
            return None
        try:
            safe_strategy_id = validate_identifier_part(strategy_id, "strategy_id")
            safe_topic = validate_identifier_part(topic, "topic")
        except ValueError:
            return None
        return StreamCoordinates(
            environment=environment,
            strategy_id=safe_strategy_id,
            topic=safe_topic,
        )

    def _track_stream_key(self, key: str) -> None:
        coordinates = self._parse_stream_key(key)
        if coordinates is None:
            return
        try:
            redis_type = decode_text(self._redis.type(key)).strip().lower()
        except redis.RedisError as e:
            self._logger.warning("Skipping stream key %s (TYPE failed: %s)", key, e)
            self._stream_ids.pop(key, None)
            return
        if redis_type != "stream":
            self._stream_ids.pop(key, None)
            return
        if key not in self._stream_ids:
            self._stream_ids[key] = self._initial_stream_id_for_discovered_stream(
                stream_key=key,
                coordinates=coordinates,
            )
            self._logger.info("Discovered inbound stream %s", key)

    def _latest_stream_entry_id(self, stream_key: str) -> str | None:
        xrevrange_fn = getattr(self._redis, "xrevrange", None)
        if not callable(xrevrange_fn):
            return None
        try:
            rows = xrevrange_fn(stream_key, "+", "-", count=1)
        except Exception:
            return None
        if not rows:
            return None
        first_row = rows[0]
        if not isinstance(first_row, tuple | list) or not first_row:
            return None
        entry_id = decode_text(first_row[0]).strip()
        return entry_id or None

    def _initial_stream_id_for_discovered_stream(
        self,
        *,
        stream_key: str,
        coordinates: StreamCoordinates,
    ) -> str:
        if self._start_id != "$":
            return self._start_id

        topic_suffix = coordinates.topic.rsplit(".", maxsplit=1)[-1]
        if topic_suffix in FULL_BOOTSTRAP_REPLAY_TOPIC_SUFFIXES:
            return "0-0"

        latest_entry_id = self._latest_stream_entry_id(stream_key)
        if topic_suffix != "trade":
            return latest_entry_id or self._start_id

        xlen_fn = getattr(self._redis, "xlen", None)
        if not callable(xlen_fn):
            return latest_entry_id or self._start_id

        target_stream_key = FluxRedisKeys(
            strategy_id=coordinates.strategy_id,
            namespace=self._namespace,
            schema_version=self._schema_version,
        ).trades_stream()
        try:
            if int(xlen_fn(target_stream_key) or 0) == 0:
                return "0-0"
        except Exception:
            return latest_entry_id or self._start_id
        return latest_entry_id or self._start_id

    def _refresh_streams(self, *, force: bool = False) -> None:
        now = time.time()
        if not force and (now - self._last_scan_ts) < self._scan_interval_sec:
            return

        discovered: set[str] = set()
        for pattern in self._scan_patterns():
            cursor = 0
            while True:
                cursor, keys = self._redis.scan(cursor=cursor, match=pattern, count=500)
                for raw_key in keys:
                    discovered.add(decode_text(raw_key))
                if cursor == 0:
                    break

        for key in sorted(discovered):
            self._track_stream_key(key)

        self._last_scan_ts = now

    def _payload_from_fields(self, fields: dict[Any, Any]) -> tuple[Any, str]:
        field_topic = self._normalize_topic_name(
            first_text(fields.get("topic"), fields.get(b"topic")),
        )
        raw_payload = fields.get("payload")
        if raw_payload is None:
            raw_payload = fields.get(b"payload")
        if raw_payload is not None:
            parsed = load_json_payload(raw_payload)
            envelope_topic, unwrapped = self._unwrap_payload_envelope(parsed)
            return unwrapped, envelope_topic or field_topic
        payload = {decode_text(key): load_json_payload(value) for key, value in fields.items()}
        envelope_topic, unwrapped = self._unwrap_payload_envelope(payload)
        return unwrapped, envelope_topic or field_topic

    def _normalized_ts_ms(self, payload: Any, fields: dict[Any, Any]) -> int:
        candidates = [
            fields.get("ts_ms"),
            fields.get(b"ts_ms"),
            fields.get("timestamp"),
            fields.get(b"timestamp"),
            fields.get("ts"),
            fields.get(b"ts"),
            fields.get("ts_event"),
            fields.get(b"ts_event"),
        ]
        if isinstance(payload, dict):
            candidates.extend(
                [
                    payload.get("ts_ms"),
                    payload.get("timestamp"),
                    payload.get("ts"),
                    payload.get("ts_event"),
                    payload.get("time"),
                    payload.get("datetime"),
                ],
            )
            rows = payload.get("rows")
            if isinstance(rows, list):
                for row in rows:
                    if not isinstance(row, dict):
                        continue
                    candidates.extend(
                        [
                            row.get("ts_ms"),
                            row.get("timestamp"),
                            row.get("ts"),
                            row.get("ts_event"),
                            row.get("time"),
                            row.get("datetime"),
                        ],
                    )
                    break
        ts_ms = None
        for candidate in candidates:
            parsed = coerce_ts_ms(candidate)
            if parsed is not None:
                ts_ms = parsed
                break
        if ts_ms is None:
            raise ValueError("Missing parseable timestamp for stream entry")
        return ts_ms

    def _decode_entry(
        self,
        *,
        stream_key: str,
        entry_id: str,
        fields: dict[Any, Any],
    ) -> tuple[Any, CorrelationContext] | None:
        coordinates = self._parse_stream_key(stream_key)
        if coordinates is None:
            return None

        payload, payload_topic = self._payload_from_fields(fields)
        topic = coordinates.topic
        normalized_payload_topic = self._normalize_topic_name(payload_topic)
        if normalized_payload_topic in self._handlers:
            topic = normalized_payload_topic

        payload_strategy = ""
        if isinstance(payload, dict):
            payload_strategy = first_text(payload.get("strategy_id"))
        field_strategy = first_text(fields.get("strategy_id"), fields.get(b"strategy_id"))
        context_strategy_id = coordinates.strategy_id
        stream_key_is_internal = (
            self._strategy_ids is not None and coordinates.strategy_id not in self._strategy_ids
        )
        if stream_key_is_internal:
            resolved_strategy_id = ""
            for raw_candidate in (payload_strategy, field_strategy):
                if not raw_candidate:
                    continue
                try:
                    candidate = validate_identifier_part(raw_candidate, "strategy_id")
                except ValueError:
                    continue
                if candidate in self._strategy_ids:
                    resolved_strategy_id = candidate
                    break
            if not resolved_strategy_id:
                raise ValueError(
                    "Grouped stream entry must include a recognized external strategy_id",
                )
            context_strategy_id = resolved_strategy_id
        else:
            if payload_strategy and payload_strategy != coordinates.strategy_id:
                self._logger.debug(
                    "Ignoring payload strategy_id=%s in stream %s, using key strategy_id=%s",
                    payload_strategy,
                    stream_key,
                    coordinates.strategy_id,
                )
            if field_strategy and field_strategy != coordinates.strategy_id:
                self._logger.debug(
                    "Ignoring field strategy_id=%s in stream %s, using key strategy_id=%s",
                    field_strategy,
                    stream_key,
                    coordinates.strategy_id,
                )

        ts_ms = self._normalized_ts_ms(payload, fields)
        context = CorrelationContext(
            strategy_id=context_strategy_id,
            topic=topic,
            entry_id=entry_id,
            ts_ms=ts_ms,
        )
        return payload, context

    def _emit_bridge_alert(
        self,
        *,
        stream_key: str,
        entry_id: str,
        topic: str,
        alert_key: str,
        message: str,
        ts_ms: int | None = None,
        **extra_fields: Any,
    ) -> None:
        coordinates = self._parse_stream_key(stream_key)
        if coordinates is None:
            return
        ts_ms_int = max(0, int(ts_ms if ts_ms is not None else time.time() * 1_000))
        payload: dict[str, Any] = {
            "strategy_id": coordinates.strategy_id,
            "level": "error",
            "message": message,
            "alert_key": alert_key,
            "reason_code": alert_key,
            "actionable": True,
            "source": "bridge",
            "event_type": type(self).__name__,
            "stream_key": stream_key,
            "source_topic": topic,
            "entry_id": entry_id,
            "ts_ms": ts_ms_int,
            "ts_event": ts_ms_int * 1_000_000,
            "row_id": f"{coordinates.strategy_id}:alert:entry:{entry_id}:{alert_key}",
        }
        payload.update(extra_fields)
        fields = {
            "strategy_id": coordinates.strategy_id,
            "topic": "alert",
            "entry_id": entry_id,
            "ts_ms": str(ts_ms_int),
            "payload": _to_json(payload),
        }
        try:
            self._redis.xadd(
                FluxRedisKeys(
                    strategy_id=coordinates.strategy_id,
                    namespace=self._namespace,
                    schema_version=self._schema_version,
                ).alerts(),
                fields,
                maxlen=self.BRIDGE_ALERTS_MAXLEN,
                approximate=True,
            )
        except Exception as exc:
            self._logger.warning(
                "Failed to emit bridge alert stream=%s id=%s err=%s",
                stream_key,
                entry_id,
                exc,
            )

    def _can_skip_decode_failure(self, *, stream_key: str, error: Exception) -> bool:
        coordinates = self._parse_stream_key(stream_key)
        if coordinates is None:
            return False
        if self._strategy_ids is None or coordinates.strategy_id in self._strategy_ids:
            return False
        if coordinates.topic != TOPIC_EXECUTION_ALERT:
            return False
        return "recognized external strategy_id" in str(error)

    def _apply_write_ops(self, ops: list[WriteOp]) -> None:  # noqa: C901
        if not ops:
            return

        pipe = self._redis.pipeline(transaction=True)
        for op in ops:
            if isinstance(op, SetJSONOp):
                encoded = _to_json(op.value)
                if op.ttl_seconds is None:
                    pipe.set(op.key, encoded)
                else:
                    pipe.set(op.key, encoded, ex=int(op.ttl_seconds))
                continue

            if isinstance(op, StreamJSONOp):
                row = dict(op.row)
                row.setdefault("strategy_id", "")
                row.setdefault("topic", "")
                row.setdefault("entry_id", "")
                row.setdefault("ts_ms", 0)
                ts_ms_raw = row.get("ts_ms")
                ts_ms_int = 0
                if isinstance(ts_ms_raw, (int, float)):
                    ts_ms_int = int(ts_ms_raw)
                elif isinstance(ts_ms_raw, str):
                    try:
                        ts_ms_int = int(ts_ms_raw)
                    except ValueError:
                        ts_ms_int = 0
                fields = {
                    "strategy_id": decode_text(row.get("strategy_id")),
                    "topic": decode_text(row.get("topic")),
                    "entry_id": decode_text(row.get("entry_id")),
                    "ts_ms": str(ts_ms_int),
                    "payload": _to_json(row),
                }
                pipe.xadd(op.key, fields, maxlen=int(op.maxlen), approximate=True)
                continue

            if isinstance(op, ReplaceHashJSONOp):
                pipe.delete(op.key)
                if op.mapping:
                    encoded_mapping: dict[str | bytes, bytes | float | int | str] = {}
                    for field, row in op.mapping.items():
                        field_key: str | bytes = field if isinstance(field, bytes) else str(field)
                        encoded_mapping[field_key] = _to_json(row)
                    pipe.hset(op.key, mapping=encoded_mapping)
                if op.ttl_seconds is not None:
                    pipe.expire(op.key, int(op.ttl_seconds))

        pipe.execute()

    def run(self) -> None:  # noqa: C901
        self._install_signals()
        self._refresh_streams(force=True)
        self._logger.info("Listening for bridge topics: %s", ", ".join(self._topics))
        try:
            while self._running:
                self._refresh_streams(force=False)
                if not self._stream_ids:
                    time.sleep(0.5)
                    continue

                try:
                    stream_bulk = self._redis.xread(
                        streams=self._stream_ids,
                        count=self._read_count,
                        block=self._block_ms,
                    )
                except redis.RedisError as e:
                    self._logger.error("xread failed: %s", e)
                    time.sleep(1.0)
                    continue

                if not stream_bulk:
                    continue

                batch_failed = False
                for stream_raw, entries in stream_bulk:
                    stream_key = decode_text(stream_raw)
                    for entry_id_raw, fields in entries:
                        entry_id = decode_text(entry_id_raw)

                        try:
                            decoded = self._decode_entry(
                                stream_key=stream_key,
                                entry_id=entry_id,
                                fields=fields,
                            )
                        except Exception as e:
                            self._emit_bridge_alert(
                                stream_key=stream_key,
                                entry_id=entry_id,
                                topic="decode",
                                alert_key="bridge_decode_failed",
                                message=(
                                    "bridge_decode_failed "
                                    f"stream={stream_key} entry_id={entry_id}"
                                ),
                                error_type=type(e).__name__,
                                error_message=str(e),
                            )
                            self._logger.error(
                                "Rejected stream entry stream=%s id=%s err=%s",
                                stream_key,
                                entry_id,
                                e,
                            )
                            if self._can_skip_decode_failure(stream_key=stream_key, error=e):
                                self._stream_ids[stream_key] = entry_id
                                continue
                            batch_failed = True
                            break
                        if decoded is None:
                            self._emit_bridge_alert(
                                stream_key=stream_key,
                                entry_id=entry_id,
                                topic="decode",
                                alert_key="bridge_decode_failed",
                                message=(
                                    "bridge_decode_failed "
                                    f"stream={stream_key} entry_id={entry_id}"
                                ),
                                error_message="decode returned no payload",
                            )
                            self._logger.error(
                                "Rejected stream entry stream=%s id=%s err=%s",
                                stream_key,
                                entry_id,
                                "decode returned no payload",
                            )
                            batch_failed = True
                            break
                        payload, context = decoded

                        handler = self._handlers.get(context.topic)
                        if handler is None:
                            self._emit_bridge_alert(
                                stream_key=stream_key,
                                entry_id=entry_id,
                                topic=context.topic,
                                alert_key="bridge_missing_handler",
                                message=(
                                    "bridge_missing_handler "
                                    f"topic={context.topic} stream={stream_key}"
                                ),
                                ts_ms=context.ts_ms,
                            )
                            self._logger.error(
                                "Rejected stream entry stream=%s id=%s err=%s",
                                stream_key,
                                entry_id,
                                f"missing handler for topic={context.topic}",
                            )
                            batch_failed = True
                            break

                        try:
                            ops = handler(payload, context)
                        except Exception as e:
                            self._emit_bridge_alert(
                                stream_key=stream_key,
                                entry_id=entry_id,
                                topic=context.topic,
                                alert_key="bridge_handler_failed",
                                message=(
                                    "bridge_handler_failed "
                                    f"topic={context.topic} stream={stream_key}"
                                ),
                                ts_ms=context.ts_ms,
                                error_type=type(e).__name__,
                                error_message=str(e),
                            )
                            self._logger.exception(
                                "Handler failed topic=%s stream=%s id=%s err=%s",
                                context.topic,
                                stream_key,
                                entry_id,
                                e,
                            )
                            batch_failed = True
                            break

                        try:
                            self._apply_write_ops(ops)
                        except Exception as e:
                            self._emit_bridge_alert(
                                stream_key=stream_key,
                                entry_id=entry_id,
                                topic=context.topic,
                                alert_key="bridge_write_failed",
                                message=(
                                    "bridge_write_failed "
                                    f"topic={context.topic} stream={stream_key}"
                                ),
                                ts_ms=context.ts_ms,
                                error_type=type(e).__name__,
                                error_message=str(e),
                            )
                            current = (stream_key, entry_id)
                            if self._write_failure_entry != current:
                                self._write_failure_entry = current
                                self._write_failure_streak = 0
                                self._write_failure_first_failure_s = time.monotonic()
                            self._write_failure_streak = min(self._write_failure_streak + 1, 50)
                            elapsed_s = max(0.0, time.monotonic() - self._write_failure_first_failure_s)
                            backoff_s = min(0.25 * (2 ** max(0, self._write_failure_streak - 1)), 5.0)
                            self._logger.error(
                                "Write-op application failed topic=%s stream=%s id=%s streak=%s elapsed_s=%.3f backoff_s=%.3f err=%s",
                                context.topic,
                                stream_key,
                                entry_id,
                                self._write_failure_streak,
                                elapsed_s,
                                backoff_s,
                                e,
                            )
                            # Do not advance offset on write failures; retry this entry with backoff.
                            if elapsed_s >= 60.0:
                                self._logger.critical(
                                    "Write-op application has failed for %.1fs (topic=%s stream=%s id=%s); stopping consumer to avoid silent stall",
                                    elapsed_s,
                                    context.topic,
                                    stream_key,
                                    entry_id,
                                )
                                raise
                            time.sleep(backoff_s)
                            batch_failed = True
                            break
                        else:
                            self._write_failure_entry = None
                            self._write_failure_streak = 0
                            self._write_failure_first_failure_s = 0.0

                        self._stream_ids[stream_key] = entry_id
                    if batch_failed:
                        break
        finally:
            self._shutdown()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Consume flux:v1 inbound streams and persist canonical Flux bridge outputs.",
    )
    parser.add_argument("--redis-host", default="127.0.0.1")
    parser.add_argument("--redis-port", type=int, default=6380)
    parser.add_argument("--redis-db", type=int, default=0)
    parser.add_argument("--redis-username", default=None)
    parser.add_argument("--redis-password", default=None)
    parser.add_argument(
        "--environment",
        default=os.getenv("FLUX_ENVIRONMENT", "paper"),
    )
    parser.add_argument("--strategy-id", default=None)
    parser.add_argument("--topics", nargs="*", default=[])
    parser.add_argument("--scan-interval-sec", type=float, default=3.0)
    parser.add_argument("--block-ms", type=int, default=1_000)
    parser.add_argument("--read-count", type=int, default=200)
    parser.add_argument("--start-id", default="$")
    parser.add_argument("--log-level", default="INFO")
    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()

    logging.basicConfig(
        level=getattr(logging, str(args.log_level).upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s - %(message)s",
    )

    redis_client = redis.Redis(
        host=args.redis_host,
        port=args.redis_port,
        db=args.redis_db,
        username=args.redis_username,
        password=args.redis_password,
        decode_responses=False,
    )
    consumer = FluxBridgeStreamConsumer(
        redis_client=redis_client,
        environment=args.environment,
        strategy_id=args.strategy_id,
        topics=list(args.topics or []),
        start_id=args.start_id,
        block_ms=args.block_ms,
        read_count=args.read_count,
        scan_interval_sec=args.scan_interval_sec,
    )
    consumer.run()


if __name__ == "__main__":
    main()
