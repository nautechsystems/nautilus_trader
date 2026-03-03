#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import logging
import signal
import sys
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Iterable

import redis

DEFAULT_STRATEGY_ID = "bybit_binance_plumeusdt_makerv3"

DEFAULT_TOPICS = {
    "state": "maker_poc.state",
    "event": "maker_poc.event",
    "trade": "maker_poc.trade",
    "alert": "maker_poc.alert",
    "market_bbo": "maker_poc.market_bbo",
    "fv": "maker_poc.fv",
    "balances": "maker_poc.balances",
}


def _now_ms() -> int:
    return int(time.time() * 1000)


def _json_default(value: Any) -> Any:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    raise TypeError(f"Object of type {type(value).__name__} is not JSON serializable")


def _to_json(obj: Any) -> str:
    return json.dumps(obj, separators=(",", ":"), default=_json_default)


def _decode_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def _safe_float(value: Any) -> float | None:
    try:
        if value is None:
            return None
        return float(value)
    except (TypeError, ValueError):
        return None


def _coerce_ts_ms(value: Any) -> int | None:
    if value is None:
        return None
    try:
        if isinstance(value, bytes):
            value = value.decode("utf-8", errors="replace")
        ts = float(value)
    except (TypeError, ValueError):
        try:
            ts = datetime.fromisoformat(str(value).replace("Z", "+00:00")).timestamp()
        except (TypeError, ValueError):
            return None
    if ts <= 0:
        return None
    if ts < 1_000_000_000_000:
        return int(ts * 1000)
    if ts >= 1_000_000_000_000_000_000:
        return int(ts / 1_000_000)
    if ts >= 1_000_000_000_000_000:
        return int(ts / 1000)
    return int(ts)


def _load_json_payload(raw_payload: Any) -> Any:
    if raw_payload is None:
        return None
    if isinstance(raw_payload, (dict, list)):
        return raw_payload
    if isinstance(raw_payload, (int, float, bool)):
        return raw_payload
    if isinstance(raw_payload, bytes):
        try:
            text = raw_payload.decode("utf-8")
        except UnicodeDecodeError:
            return raw_payload
    else:
        text = str(raw_payload)
    text = text.strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def _as_dict(payload: Any) -> dict[str, Any]:
    if isinstance(payload, dict):
        return dict(payload)
    if isinstance(payload, list):
        return {"rows": payload}
    if isinstance(payload, bytes):
        payload = _decode_text(payload)
    if isinstance(payload, str):
        parsed = _load_json_payload(payload)
        if isinstance(parsed, dict):
            return dict(parsed)
        return {"value": payload}
    return {"value": payload}


def _as_rows(payload: Any) -> list[dict[str, Any]]:
    if isinstance(payload, list):
        return [dict(row) for row in payload if isinstance(row, dict)]
    if isinstance(payload, dict):
        for key in ("rows", "trades", "alerts", "data", "items", "values"):
            value = payload.get(key)
            if isinstance(value, list):
                rows = [dict(row) for row in value if isinstance(row, dict)]
                if rows:
                    return rows
        return [dict(payload)]
    if isinstance(payload, str):
        parsed = _load_json_payload(payload)
        return _as_rows(parsed)
    return []


def _first_text(*values: Any) -> str:
    for value in values:
        text = _decode_text(value).strip()
        if text:
            return text
    return ""


def _normalize_symbol_parts(
    *,
    base: Any = None,
    quote: Any = None,
    symbol: Any = None,
) -> tuple[str, str]:
    base_text = _decode_text(base).strip().upper()
    quote_text = _decode_text(quote).strip().upper()
    if base_text and quote_text:
        return base_text, quote_text

    symbol_text = _decode_text(symbol).strip().upper()
    if not symbol_text:
        return "", ""

    cleaned = symbol_text.replace("-", "_").replace("/", "_")
    if "_" in cleaned:
        left, right = cleaned.split("_", 1)
        return left, right

    known_quotes = (
        "USDT",
        "USDC",
        "PUSD",
        "USDE",
        "USD",
        "BTC",
        "ETH",
        "EUR",
        "GBP",
        "JPY",
        "BNB",
    )
    for candidate in known_quotes:
        if cleaned.endswith(candidate) and len(cleaned) > len(candidate):
            return cleaned[: -len(candidate)], candidate

    return cleaned, ""


def _load_contracts_module(logger: logging.Logger) -> Any:
    contracts_path = Path(__file__).with_name("contracts.py")
    if not contracts_path.exists():
        raise FileNotFoundError(f"Required contracts.py not found at {contracts_path}")

    spec = importlib.util.spec_from_file_location("poc_contracts", contracts_path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Failed to load contracts.py spec at {contracts_path}")

    module = importlib.util.module_from_spec(spec)
    try:
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
    except Exception as exc:
        raise ImportError(f"Failed importing contracts.py ({exc})") from exc
    logger.info("Loaded contracts.py from %s", contracts_path)
    return module


class ContractsAdapter:
    def __init__(self, module: Any) -> None:
        self._module = module

    def _call_first(self, names: Iterable[str], *args: Any) -> Any | None:
        for name in names:
            fn = getattr(self._module, name, None)
            if callable(fn):
                try:
                    return fn(*args)
                except TypeError:
                    continue
                except Exception as exc:
                    raise RuntimeError(f"contracts.py helper `{name}` failed: {exc}") from exc
        return None

    def _value_first(self, names: Iterable[str]) -> Any | None:
        for name in names:
            if hasattr(self._module, name):
                return getattr(self._module, name)
        return None

    @staticmethod
    def _require_text(value: Any, *, required: str) -> str:
        if isinstance(value, str) and value:
            return value
        raise RuntimeError(f"contracts.py missing required mapping: {required}")

    def topic(self, name: str, default: str) -> str:
        value = self._call_first((f"{name}_topic", f"topic_{name}", f"get_{name}_topic"))
        if isinstance(value, str) and value:
            return value
        value = self._value_first(
            (
                f"{name.upper()}_TOPIC",
                f"TOPIC_{name.upper()}",
                f"MAKER_POC_{name.upper()}_TOPIC",
            ),
        )
        return self._require_text(
            value,
            required=f"topic `{name}` (expected default `{default}`)",
        )

    def state_key(self, strategy_id: str) -> str:
        value = self._call_first(
            ("state_key", "maker_state_key", "get_state_key", "maker_state_redis_key"),
            strategy_id,
        )
        if isinstance(value, str) and value:
            return value
        return f"maker_arb:{strategy_id}:state"

    def events_key(self, strategy_id: str) -> str:
        value = self._call_first(
            ("events_key", "maker_events_key", "get_events_key", "maker_events_redis_key"),
            strategy_id,
        )
        if isinstance(value, str) and value:
            return value
        return f"maker_arb:{strategy_id}:events"

    def market_key(self, exchange: str, base: str, quote: str) -> str:
        value = self._call_first(
            ("market_bbo_key", "market_last_key", "last_market_key", "compose_last_key"),
            exchange,
            base,
            quote,
        )
        if isinstance(value, str) and value:
            return value
        value = self._call_first(
            ("make_last_key_component",),
            exchange,
            f"{base.lower()}/{quote.lower()}",
        )
        if isinstance(value, str) and value:
            return value
        return f"last:{exchange}:{base}_{quote}"

    def balances_snapshot_key(self) -> str:
        value = self._call_first(("balances_snapshot_key", "get_balances_snapshot_key"))
        if isinstance(value, str) and value:
            return value
        value = self._value_first(("BALANCES_SNAPSHOT_KEY", "balances_snapshot_key"))
        if isinstance(value, str) and value:
            return value
        return "balances.snapshot"

    def balances_hash_key(self) -> str:
        value = self._call_first(("balances_hash_key", "get_balances_hash_key"))
        if isinstance(value, str) and value:
            return value
        value = self._value_first(("BALANCES_HASH_KEY", "balances_hash_key"))
        if isinstance(value, str) and value:
            return value
        return "balances"

    def fvs_snapshot_key(self) -> str:
        value = self._call_first(("fvs_snapshot_key", "get_fvs_snapshot_key"))
        if isinstance(value, str) and value:
            return value
        value = self._value_first(("FVS_SNAPSHOT_KEY", "fvs_snapshot_key"))
        if isinstance(value, str) and value:
            return value
        return "fvs.snapshot"

    def trades_blotter_key(self) -> str:
        value = self._call_first(("trades_blotter_key", "get_trades_blotter_key"))
        if isinstance(value, str) and value:
            return value
        value = self._value_first(("TRADES_BLOTTER_KEY", "trades_blotter_key"))
        if isinstance(value, str) and value:
            return value
        return "trades.blotter"

    def alerts_blotter_key(self) -> str:
        value = self._call_first(("alerts_blotter_key", "get_alerts_blotter_key"))
        if isinstance(value, str) and value:
            return value
        value = self._value_first(("ALERTS_BLOTTER_KEY", "alerts_blotter_key"))
        if isinstance(value, str) and value:
            return value
        return "alerts.blotter"

    def normalize_exchange(self, exchange: Any) -> str:
        value = self._call_first(
            ("normalize_exchange_name", "normalize_exchange", "to_chainsaw_exchange"),
            exchange,
        )
        if isinstance(value, str) and value:
            return value
        return _decode_text(exchange).strip().lower()


class ChainsawBridge:
    def __init__(self, args: argparse.Namespace) -> None:
        self._logger = logging.getLogger("chainsaw-bridge")
        self._redis = redis.Redis(
            host=args.redis_host,
            port=args.redis_port,
            db=args.redis_db,
            username=args.redis_username,
            password=args.redis_password,
            decode_responses=False,
        )
        self._contracts = ContractsAdapter(_load_contracts_module(self._logger))
        self._stream_prefix = args.stream_prefix
        self._stream_per_topic = args.stream_per_topic
        self._explicit_streams = list(args.streams or [])
        self._scan_interval_sec = max(1.0, args.scan_interval_sec)
        self._block_ms = max(10, args.block_ms)
        self._read_count = max(1, args.read_count)
        self._start_id = args.start_id
        self._strategy_id = _decode_text(args.strategy_id).strip() or DEFAULT_STRATEGY_ID
        self._events_max = max(1, args.events_max)
        self._trades_max = max(1, args.trades_max)
        self._alerts_max = max(1, args.alerts_max)
        self._rebuild_balances_hash = not args.no_balances_hash
        self._last_scan_ts = 0.0
        self._running = True
        self._stream_ids: dict[str, str] = {}
        self._state_keys: dict[str, str] = {}
        self._events_keys: dict[str, str] = {}

        self._topics = {
            name: self._contracts.topic(name, default)
            for name, default in DEFAULT_TOPICS.items()
        }
        self._state_keys[self._strategy_id] = self._contracts.state_key(self._strategy_id)
        self._events_keys[self._strategy_id] = self._contracts.events_key(self._strategy_id)
        self._trades_key = self._contracts.trades_blotter_key()
        self._alerts_key = self._contracts.alerts_blotter_key()
        self._fvs_key = self._contracts.fvs_snapshot_key()
        self._balances_snapshot_key = self._contracts.balances_snapshot_key()
        self._balances_hash_key = self._contracts.balances_hash_key()

        self._handlers: dict[str, Callable[[Any], None]] = {
            self._topics["state"]: self._handle_state,
            self._topics["event"]: self._handle_event,
            self._topics["trade"]: self._handle_trade,
            self._topics["alert"]: self._handle_alert,
            self._topics["market_bbo"]: self._handle_market_bbo,
            self._topics["fv"]: self._handle_fv,
            self._topics["balances"]: self._handle_balances,
        }

    def _install_signals(self) -> None:
        signal.signal(signal.SIGINT, self._on_signal)
        signal.signal(signal.SIGTERM, self._on_signal)

    def _on_signal(self, sig: int, _frame: Any) -> None:
        self._logger.info("Received signal %s, stopping", sig)
        self._running = False

    def _scan_patterns(self) -> list[str]:
        if self._stream_per_topic:
            patterns = []
            for topic in self._handlers:
                patterns.append(f"{self._stream_prefix}:{topic}")
                patterns.append(f"*:{self._stream_prefix}:{topic}")
            return patterns
        return [self._stream_prefix, f"*:{self._stream_prefix}"]

    def _track_stream_key(self, key: str, *, source: str) -> None:
        try:
            redis_type = _decode_text(self._redis.type(key)).strip().lower()
        except redis.RedisError as exc:
            self._logger.warning("Skipping %s key %s (failed TYPE: %s)", source, key, exc)
            self._stream_ids.pop(key, None)
            return
        if redis_type != "stream":
            if redis_type and redis_type != "none":
                self._logger.warning(
                    "Skipping %s key %s (redis type=%s, expected stream)",
                    source,
                    key,
                    redis_type,
                )
            self._stream_ids.pop(key, None)
            return
        if key not in self._stream_ids:
            self._logger.info("Discovered stream key %s", key)
            self._stream_ids[key] = self._start_id

    def _drop_non_stream_keys(self, *, reason: str) -> int:
        dropped = 0
        for key in list(self._stream_ids):
            try:
                redis_type = _decode_text(self._redis.type(key)).strip().lower()
            except redis.RedisError as exc:
                self._logger.warning(
                    "Failed TYPE for key %s while handling %s: %s",
                    key,
                    reason,
                    exc,
                )
                continue
            if redis_type == "stream":
                continue
            dropped += 1
            self._stream_ids.pop(key, None)
            self._logger.warning(
                "Dropped stream key %s after %s (redis type=%s)",
                key,
                reason,
                redis_type or "unknown",
            )
        return dropped

    def _refresh_streams(self, force: bool = False) -> None:
        if self._explicit_streams:
            for key in self._explicit_streams:
                self._track_stream_key(key, source="explicit")
            return

        now = time.time()
        if not force and (now - self._last_scan_ts) < self._scan_interval_sec:
            return

        discovered: set[str] = set()
        for pattern in self._scan_patterns():
            cursor = 0
            while True:
                cursor, keys = self._redis.scan(cursor=cursor, match=pattern, count=500)
                for raw_key in keys:
                    discovered.add(_decode_text(raw_key))
                if cursor == 0:
                    break

        for key in sorted(discovered):
            self._track_stream_key(key, source="discovered")

        self._last_scan_ts = now

    def _infer_topic(self, stream_key: str) -> str:
        marker = f":{self._stream_prefix}:"
        if marker in stream_key:
            return stream_key.split(marker, 1)[1]
        if stream_key.startswith(f"{self._stream_prefix}:"):
            return stream_key.split(":", 1)[1]
        return ""

    def _parse_bus_message(self, fields: dict[Any, Any], stream_key: str) -> tuple[str, Any]:
        topic = _first_text(fields.get("topic"), fields.get(b"topic"))
        if not topic:
            topic = self._infer_topic(stream_key)
        raw_payload = fields.get("payload")
        if raw_payload is None:
            raw_payload = fields.get(b"payload")
        payload = _load_json_payload(raw_payload)
        return topic, payload

    def _strategy_id_for_payload(self, payload: Any) -> str:
        parsed = payload
        if isinstance(payload, (str, bytes)):
            parsed = _load_json_payload(payload)
        if isinstance(parsed, dict):
            strategy_id = _decode_text(parsed.get("strategy_id")).strip()
            if strategy_id:
                return strategy_id
        return self._strategy_id

    def _state_key_for_strategy(self, strategy_id: str) -> str:
        key = self._state_keys.get(strategy_id)
        if key is None:
            key = self._contracts.state_key(strategy_id)
            self._state_keys[strategy_id] = key
        return key

    def _events_key_for_strategy(self, strategy_id: str) -> str:
        key = self._events_keys.get(strategy_id)
        if key is None:
            key = self._contracts.events_key(strategy_id)
            self._events_keys[strategy_id] = key
        return key

    def _json_state(self, payload: Any, strategy_id: str) -> str:
        if isinstance(payload, dict):
            payload = dict(payload)
            payload.setdefault("strategy_id", strategy_id)
            return _to_json(payload)
        if isinstance(payload, list):
            return _to_json(payload)
        if isinstance(payload, str):
            parsed = _load_json_payload(payload)
            if isinstance(parsed, dict):
                parsed = dict(parsed)
                parsed.setdefault("strategy_id", strategy_id)
                return _to_json(parsed)
            if isinstance(parsed, list):
                return _to_json(parsed)
            return _to_json({"value": payload, "strategy_id": strategy_id})
        return _to_json({"value": payload, "strategy_id": strategy_id})

    def _handle_state(self, payload: Any) -> None:
        strategy_id = self._strategy_id_for_payload(payload)
        self._redis.set(
            self._state_key_for_strategy(strategy_id),
            self._json_state(payload, strategy_id),
        )

    def _handle_event(self, payload: Any) -> None:
        row = _as_dict(payload)
        strategy_id = self._strategy_id_for_payload(row)
        row.setdefault("strategy_id", strategy_id)
        row.setdefault("ts_ms", _now_ms())
        events_key = self._events_key_for_strategy(strategy_id)
        pipe = self._redis.pipeline(transaction=True)
        # Events are append-only in chronological order: oldest -> newest.
        pipe.rpush(events_key, _to_json(row))
        pipe.ltrim(events_key, -self._events_max, -1)
        pipe.execute()

    def _synth_trade_row_id(self, row: dict[str, Any]) -> str:
        ts_ms = _coerce_ts_ms(
            row.get("ts_ms")
            or row.get("ts")
            or row.get("created_ts_ms")
            or row.get("timestamp")
            or _now_ms()
        ) or _now_ms()
        exch = _first_text(row.get("exchange"), row.get("venue"), "unknown").lower()
        symbol = _first_text(row.get("coin"), row.get("symbol"), "unknown").upper()
        trade_ref = _first_text(
            row.get("trade_id"),
            row.get("exchange_trade_id"),
            row.get("id"),
            row.get("tx_hash"),
            row.get("hash"),
            uuid.uuid4().hex[:10],
        )
        return f"{exch}:{symbol}:{ts_ms}:{trade_ref}"

    def _handle_trade_row(self, row: dict[str, Any]) -> None:
        clean = dict(row)
        incoming_strategy_id = _decode_text(clean.get("strategy_id")).strip()
        if incoming_strategy_id:
            if incoming_strategy_id != self._strategy_id:
                self._logger.debug(
                    "Preserving upstream trade strategy_id=%s (configured --strategy-id=%s)",
                    incoming_strategy_id,
                    self._strategy_id,
                )
        else:
            clean.setdefault("strategy_id", self._strategy_id)
            if not _decode_text(clean.get("strategy_id")).strip():
                clean["strategy_id"] = self._strategy_id
        clean["op"] = _first_text(clean.get("op"), "upsert")

        ts_ms = _coerce_ts_ms(
            clean.get("ts_ms")
            or clean.get("ts")
            or clean.get("created_ts_ms")
            or clean.get("timestamp")
            or clean.get("time")
            or clean.get("datetime")
        ) or _now_ms()
        clean["ts_ms"] = ts_ms
        clean.setdefault("ts", ts_ms)
        clean.setdefault(
            "time",
            datetime.fromtimestamp(ts_ms / 1000, tz=timezone.utc).strftime("%Y-%m-%d %H:%M:%S"),
        )

        row_id = _first_text(
            clean.get("row_id"),
            clean.get("exchange_trade_id"),
            clean.get("trade_id"),
            clean.get("id"),
            clean.get("tx_hash"),
            clean.get("hash"),
        )
        if not row_id:
            row_id = self._synth_trade_row_id(clean)
        clean["row_id"] = row_id

        seq = int(self._redis.incr("trades.seq"))
        version = int(self._redis.incr(f"trades.ver:{row_id}"))
        clean["seq"] = seq
        clean["version"] = version
        serialized = _to_json(clean)

        pipe = self._redis.pipeline(transaction=True)
        pipe.lpush(self._trades_key, serialized)
        pipe.ltrim(self._trades_key, 0, self._trades_max - 1)
        pipe.hset("trades.map", row_id, serialized)
        pipe.zadd("trades.index.seq", {row_id: seq})
        pipe.zadd("trades.index.ts", {row_id: ts_ms})
        pipe.execute()

    def _prune_trade_indices(self) -> None:
        for key in ("trades.index.seq", "trades.index.ts"):
            size = int(self._redis.zcard(key) or 0)
            overflow = size - self._trades_max
            if overflow > 0:
                self._redis.zremrangebyrank(key, 0, overflow - 1)

    def _handle_trade(self, payload: Any) -> None:
        rows = _as_rows(payload)
        if not rows:
            row = _as_dict(payload)
            if row:
                rows = [row]
        if not rows:
            return
        for row in rows:
            self._handle_trade_row(row)
        self._prune_trade_indices()

    def _handle_alert(self, payload: Any) -> None:
        rows = _as_rows(payload)
        if not rows:
            row = _as_dict(payload)
            if row:
                rows = [row]
        if not rows:
            return

        pipe = self._redis.pipeline(transaction=True)
        now_ts = time.time()
        for row in rows:
            alert = dict(row)
            alert.setdefault("strategy_id", self._strategy_id)
            alert.setdefault("id", str(uuid.uuid4()))
            alert.setdefault("timestamp", now_ts)
            pipe.rpush(self._alerts_key, _to_json(alert))
        pipe.ltrim(self._alerts_key, -self._alerts_max, -1)
        pipe.execute()

    def _handle_market_bbo(self, payload: Any) -> None:
        row = _as_dict(payload)
        exchange = self._contracts.normalize_exchange(
            _first_text(
                row.get("chainsaw_exchange"),
                row.get("exchange"),
                row.get("venue"),
                row.get("market_exchange"),
            ),
        )
        base, quote = _normalize_symbol_parts(
            base=row.get("base"),
            quote=row.get("quote"),
            symbol=_first_text(
                row.get("symbol"),
                row.get("market_key"),
                row.get("coin"),
                row.get("pair"),
            ),
        )
        if not exchange or not base or not quote:
            self._logger.warning("Skipping market_bbo without key fields: %s", row)
            return

        bid = _safe_float(row.get("bid") or row.get("best_bid") or row.get("bid_px"))
        ask = _safe_float(row.get("ask") or row.get("best_ask") or row.get("ask_px"))
        if bid is None and ask is None:
            self._logger.warning("Skipping market_bbo without bid/ask: %s", row)
            return

        ts_ms = _coerce_ts_ms(
            row.get("ts_ms")
            or row.get("timestamp")
            or row.get("observed_ts")
            or row.get("ts")
        ) or _now_ms()

        out = dict(row)
        out["chainsaw_exchange"] = exchange
        out["base"] = base
        out["quote"] = quote
        if bid is not None:
            out["bid"] = bid
        if ask is not None:
            out["ask"] = ask
        out["ts_ms"] = ts_ms

        key = self._contracts.market_key(exchange, base, quote)
        self._redis.set(key, _to_json(out))

    def _handle_fv(self, payload: Any) -> None:
        rows = _as_rows(payload)
        if not rows:
            self._logger.warning("Ignoring empty maker_poc.fv payload to keep fvs.snapshot non-empty")
            return
        self._redis.set(self._fvs_key, _to_json(rows))

    def _balance_hash_field(self, row: dict[str, Any], idx: int) -> str:
        exchange = self._contracts.normalize_exchange(
            _first_text(row.get("exchange"), row.get("venue"), row.get("source"), "unknown"),
        )
        coin = _first_text(row.get("coin"), row.get("asset"), row.get("symbol"), "UNKNOWN").upper()
        location = _first_text(row.get("balance_location"), row.get("scope"), str(idx))
        return f"{exchange}:{coin}:{location}"

    def _handle_balances(self, payload: Any) -> None:
        rows = _as_rows(payload)
        snapshot = _to_json(rows)
        if not self._rebuild_balances_hash:
            self._redis.set(self._balances_snapshot_key, snapshot)
            return

        pipe = self._redis.pipeline(transaction=True)
        pipe.set(self._balances_snapshot_key, snapshot)
        pipe.delete(self._balances_hash_key)
        for idx, row in enumerate(rows):
            pipe.hset(self._balances_hash_key, self._balance_hash_field(row, idx), _to_json(row))
        pipe.execute()

    def run(self) -> None:
        self._install_signals()
        self._refresh_streams(force=True)
        self._logger.info("Bridge default strategy_id=%s", self._strategy_id)
        self._logger.info("Listening for topics: %s", sorted(self._handlers))

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
            except redis.RedisError as exc:
                message = _decode_text(exc).upper()
                if "WRONGTYPE" in message or "XREAD" in message:
                    dropped = self._drop_non_stream_keys(reason=f"xread error ({exc})")
                    if dropped:
                        continue
                self._logger.error("xread failed: %s", exc)
                time.sleep(1.0)
                continue

            if not stream_bulk:
                continue

            for stream_raw, entries in stream_bulk:
                stream_key = _decode_text(stream_raw)
                for entry_id_raw, fields in entries:
                    entry_id = _decode_text(entry_id_raw)
                    self._stream_ids[stream_key] = entry_id

                    topic, payload = self._parse_bus_message(fields, stream_key)
                    if not topic:
                        self._logger.debug(
                            "Skipping stream entry without topic stream=%s id=%s",
                            stream_key,
                            entry_id,
                        )
                        continue

                    handler = self._handlers.get(topic)
                    if handler is None:
                        continue

                    try:
                        handler(payload)
                    except Exception as exc:
                        self._logger.exception(
                            "Handler failed topic=%s stream=%s id=%s err=%s",
                            topic,
                            stream_key,
                            entry_id,
                            exc,
                        )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Bridge Nautilus msgbus Redis streams into Fluxboard-compatible Redis keys.",
    )
    parser.add_argument("--redis-host", default="127.0.0.1")
    parser.add_argument("--redis-port", type=int, default=6379)
    parser.add_argument("--redis-db", type=int, default=0)
    parser.add_argument("--redis-username", default=None)
    parser.add_argument("--redis-password", default=None)
    parser.add_argument("--stream-prefix", default="maker_poc")
    parser.add_argument("--stream-per-topic", action=argparse.BooleanOptionalAction, default=False)
    parser.add_argument("--streams", nargs="*", default=[])
    parser.add_argument("--scan-interval-sec", type=float, default=3.0)
    parser.add_argument("--block-ms", type=int, default=1000)
    parser.add_argument("--read-count", type=int, default=200)
    parser.add_argument("--start-id", default="$")
    parser.add_argument("--strategy-id", default=DEFAULT_STRATEGY_ID)
    parser.add_argument("--events-max", type=int, default=300)
    parser.add_argument("--trades-max", type=int, default=1000)
    parser.add_argument("--alerts-max", type=int, default=1000)
    parser.add_argument("--no-balances-hash", action="store_true")
    parser.add_argument("--log-level", default="INFO")
    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()

    logging.basicConfig(
        level=getattr(logging, str(args.log_level).upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s - %(message)s",
    )

    bridge = ChainsawBridge(args)
    bridge.run()


if __name__ == "__main__":
    main()
