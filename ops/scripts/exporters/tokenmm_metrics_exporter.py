#!/usr/bin/env python3
"""Minimal TokenMM Prometheus sidecar exporter for quoting liquidity panels."""

from __future__ import annotations

import argparse
import configparser
import json
import logging
import os
import signal
import sys
import time
from dataclasses import dataclass
from decimal import Decimal
from decimal import InvalidOperation
from pathlib import Path
from typing import Any

import redis
from prometheus_client import CollectorRegistry
from prometheus_client import Gauge
from prometheus_client import start_http_server

REPO_ROOT = Path(__file__).resolve().parents[3]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from ops.scripts.exporters.common import poll_interval_seconds_arg

LOGGER = logging.getLogger("tokenmm_metrics_exporter")

LABEL_NAMES = ("env", "token", "venue", "symbol", "strategy_family")
DEFAULT_STRATEGY_IDS = (
    "bybit_binance_plumeusdt_makerv3",
    "okx_binance_plumeusdt_makerv3",
)
DEFAULT_STRATEGY_FAMILY = "maker_v3"
ACTIVE_ORDER_STATUSES = {"open", "live", "placed"}
KNOWN_QUOTES = ("USDT", "USDC", "USD", "BTC", "ETH")
KNOWN_MARKET_VENUES = {
    "binance_spot",
    "binance_perp",
    "bybit_linear",
    "bybit_inverse",
    "bybit_spot",
    "okx_perp",
    "okx_spot",
}


def _env(name: str, default: str) -> str:
    value = os.getenv(name)
    return value if value else default


def _to_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        parsed = int(float(str(value)))
    except (TypeError, ValueError, OverflowError):
        return None
    return parsed


def _to_decimal(value: Any) -> Decimal | None:
    if value is None:
        return None
    try:
        parsed = Decimal(str(value))
    except (InvalidOperation, TypeError, ValueError):
        return None
    if not parsed.is_finite():
        return None
    return parsed

def _non_negative_int(value: str) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError) as exc:
        raise argparse.ArgumentTypeError("must be an integer") from exc
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be >= 0")
    return parsed


def normalize_venue(value: Any) -> str:
    text = str(value or "").strip().lower()
    if not text:
        return "unknown"
    text = text.split(":", 1)[0]
    if text in {"bybit", "bybit_spot"}:
        return "bybit_spot"
    if text in {"okx", "okx_spot"}:
        return "okx_spot"
    if text in {"binance", "binance_spot"}:
        return "binance_spot"
    if text in KNOWN_MARKET_VENUES:
        return text
    if text.startswith("bybit"):
        return "bybit_spot"
    if text.startswith("okx"):
        return "okx_spot"
    if text.startswith("binance"):
        return "binance_spot"
    if "_" in text:
        return text.split("_", 1)[0]
    return text


def normalize_symbol(value: Any) -> str:
    text = str(value or "").strip().upper()
    if not text:
        return "UNKNOWN/UNKNOWN"
    text = text.replace("-", "/").replace("_", "/")
    if "/" in text:
        parts = [part for part in text.split("/") if part]
        if len(parts) >= 2:
            return f"{parts[0]}/{parts[1]}"
    compact = text.replace("/", "")
    for quote in KNOWN_QUOTES:
        if compact.endswith(quote) and len(compact) > len(quote):
            base = compact[: -len(quote)]
            return f"{base}/{quote}"
    return text


def _symbol_base(symbol: str) -> str:
    return symbol.split("/", 1)[0] if "/" in symbol else symbol


def compute_quote_up(mode: Any, state_ts_ms: Any, now_ms: int, state_stale_ms: int) -> int:
    if str(mode or "").strip().upper() != "QUOTING":
        return 0
    ts_ms = _to_int(state_ts_ms)
    if ts_ms is None:
        return 0
    return 1 if now_ms - ts_ms <= int(state_stale_ms) else 0


def _order_active(order: dict[str, Any]) -> bool:
    rem_qty = _to_decimal(order.get("rem_qty"))
    if rem_qty is None or rem_qty <= 0:
        return False
    status = order.get("status")
    if status is None:
        return True
    return str(status).strip().lower() in ACTIVE_ORDER_STATUSES


def compute_depth_usd_within_bps(
    *,
    maker_orders: dict[str, Any],
    top_bid: Decimal,
    top_ask: Decimal,
    bps_limit: int,
) -> Decimal:
    if top_bid <= 0 or top_ask <= 0:
        return Decimal("0")
    mid = (top_bid + top_ask) / Decimal("2")
    if mid <= 0:
        return Decimal("0")
    limit = Decimal(str(bps_limit))
    total_depth = Decimal("0")
    for side in ("bid", "ask"):
        rows = maker_orders.get(side) or []
        if not isinstance(rows, list):
            continue
        for row in rows:
            if not isinstance(row, dict) or not _order_active(row):
                continue
            px = _to_decimal(row.get("px"))
            rem_qty = _to_decimal(row.get("rem_qty"))
            if px is None or rem_qty is None or px <= 0 or rem_qty <= 0:
                continue
            distance_bps = (abs(px - mid) / mid) * Decimal("10000")
            if distance_bps <= limit:
                total_depth += px * rem_qty
    return total_depth


@dataclass
class StrategyContext:
    strategy_id: str
    token: str
    venue: str
    symbol: str
    strategy_family: str = DEFAULT_STRATEGY_FAMILY

    def labels(self, env: str) -> dict[str, str]:
        return {
            "env": env,
            "token": self.token,
            "venue": self.venue,
            "symbol": self.symbol,
            "strategy_family": self.strategy_family,
        }


def _parse_strategy_context(strategy_id: str) -> StrategyContext:
    sid = str(strategy_id or "").strip()
    parts = sid.split("_")
    maker_venue = normalize_venue(parts[0] if parts else "unknown")
    raw_symbol = parts[2] if len(parts) >= 3 else ""
    symbol = normalize_symbol(raw_symbol)
    token = _symbol_base(symbol)
    return StrategyContext(
        strategy_id=sid,
        token=token,
        venue=maker_venue,
        symbol=symbol,
    )


def _parse_strategy_groups(value: Any) -> set[str]:
    raw = str(value or "").strip()
    if not raw:
        return set()
    return {part.strip().lower() for part in raw.split(",") if part.strip()}


def _context_from_strategy_section(
    strategy_id: str,
    section: configparser.SectionProxy,
) -> StrategyContext:
    fallback = _parse_strategy_context(strategy_id)
    venue = normalize_venue(
        section.get("exchange")
        or section.get("leg1_exchange")
        or section.get("maker_exchange")
        or fallback.venue
    )
    base_asset = str(section.get("base_asset") or "").strip().upper()
    quote_asset = str(section.get("quote_asset") or "").strip().upper()
    if base_asset and quote_asset:
        symbol = f"{base_asset}/{quote_asset}"
    else:
        symbol = normalize_symbol(
            section.get("symbol")
            or section.get("market_key")
            or section.get("leg1_symbol")
            or fallback.symbol
        )
    token = _symbol_base(symbol) if symbol and symbol != "UNKNOWN/UNKNOWN" else fallback.token
    return StrategyContext(
        strategy_id=strategy_id,
        token=token,
        venue=venue if venue != "unknown" else fallback.venue,
        symbol=symbol if symbol and symbol != "UNKNOWN/UNKNOWN" else fallback.symbol,
    )


def discover_strategy_contexts(
    *,
    config_dir: str = "configs",
    strategy_group: str = "tokenmm",
) -> dict[str, StrategyContext]:
    group = str(strategy_group or "").strip().lower()
    if not group:
        return {}
    parser = configparser.ConfigParser()
    strategies_path = Path(config_dir) / "strategies.ini"
    try:
        read_files = parser.read(str(strategies_path))
    except (OSError, configparser.Error):
        LOGGER.exception("failed to read strategies config: %s", strategies_path)
        return {}
    if not read_files:
        LOGGER.warning("strategies config not found for discovery: %s", strategies_path)
        return {}

    contexts: dict[str, StrategyContext] = {}
    for section_name in parser.sections():
        if not section_name.startswith("strategy:"):
            continue
        strategy_id = section_name.split(":", 1)[1].strip()
        section = parser[section_name]
        groups = _parse_strategy_groups(section.get("strategy_groups"))
        if group not in groups:
            continue
        contexts[strategy_id] = _context_from_strategy_section(strategy_id, section)
    return contexts


class TokenMMMetricsExporter:
    def __init__(
        self,
        *,
        redis_client: Any,
        env: str,
        strategy_ids: list[str] | tuple[str, ...],
        state_stale_ms: int = 30_000,
        strategy_context_overrides: dict[str, StrategyContext] | None = None,
        registry: CollectorRegistry | None = None,
    ) -> None:
        self.redis = redis_client
        self.env = str(env or "prod")
        self.state_stale_ms = int(state_stale_ms)

        overrides = strategy_context_overrides or {}
        self._contexts: dict[str, StrategyContext] = {}
        for raw_strategy_id in strategy_ids:
            strategy_id = str(raw_strategy_id or "").strip()
            if not strategy_id:
                continue
            context = overrides.get(strategy_id) or _parse_strategy_context(strategy_id)
            if context.strategy_id != strategy_id:
                context = StrategyContext(
                    strategy_id=strategy_id,
                    token=context.token,
                    venue=context.venue,
                    symbol=context.symbol,
                    strategy_family=context.strategy_family,
                )
            self._contexts[strategy_id] = context

        self.registry = registry or CollectorRegistry(auto_describe=True)
        self.g_quote_up = Gauge(
            "tokenmm_quote_up",
            "Quoting status (1=quoting and fresh, 0=not quoting or stale)",
            LABEL_NAMES,
            registry=self.registry,
        )
        self.g_depth_100 = Gauge(
            "tokenmm_quote_depth_usd_100bps",
            "Maker quote depth in USD within 100bps of the mid-price",
            LABEL_NAMES,
            registry=self.registry,
        )
        self.g_depth_200 = Gauge(
            "tokenmm_quote_depth_usd_200bps",
            "Maker quote depth in USD within 200bps of the mid-price",
            LABEL_NAMES,
            registry=self.registry,
        )
        self._active_labels: dict[str, set[tuple[str, ...]]] = {
            "quote_up": set(),
            "depth_100": set(),
            "depth_200": set(),
        }

        for strategy_id in self._contexts:
            labels = self._labels(strategy_id)
            self.g_quote_up.labels(**labels).set(0.0)
            self.g_depth_100.labels(**labels).set(0.0)
            self.g_depth_200.labels(**labels).set(0.0)
            label_values = self._label_values(labels)
            self._active_labels["quote_up"].add(label_values)
            self._active_labels["depth_100"].add(label_values)
            self._active_labels["depth_200"].add(label_values)

    def _labels(self, strategy_id: str) -> dict[str, str]:
        return self._contexts[strategy_id].labels(self.env)

    def _label_values(self, labels: dict[str, str]) -> tuple[str, ...]:
        return tuple(labels[name] for name in LABEL_NAMES)

    def _preserve_current_metric_values(
        self,
        *,
        labels: dict[str, str],
        quote_up_values: dict[tuple[str, ...], float],
        depth_100_values: dict[tuple[str, ...], float],
        depth_200_values: dict[tuple[str, ...], float],
    ) -> None:
        label_values = self._label_values(labels)
        sample_values = {
            "tokenmm_quote_up": quote_up_values,
            "tokenmm_quote_depth_usd_100bps": depth_100_values,
            "tokenmm_quote_depth_usd_200bps": depth_200_values,
        }
        for metric_name, target in sample_values.items():
            current = self.registry.get_sample_value(metric_name, labels)
            if current is not None:
                target[label_values] = float(current)

    def _sync_metric(
        self,
        *,
        gauge: Gauge,
        metric_key: str,
        values: dict[tuple[str, ...], float],
    ) -> None:
        previous = self._active_labels[metric_key]
        current = set(values)
        for label_values in previous - current:
            gauge.remove(*label_values)
        for label_values, value in values.items():
            gauge.labels(*label_values).set(value)
        self._active_labels[metric_key] = current

    def _parse_state_payload(self, raw: Any) -> dict[str, Any] | None:
        if raw is None:
            return None
        if isinstance(raw, bytes):
            raw = raw.decode("utf-8", errors="ignore")
        if isinstance(raw, str):
            try:
                raw = json.loads(raw)
            except (TypeError, ValueError):
                return None
        if isinstance(raw, dict):
            return raw
        return None

    def _update_context_from_state(self, strategy_id: str, state: dict[str, Any]) -> None:
        context = self._contexts[strategy_id]
        maker_leg = state.get("maker_leg")
        if not isinstance(maker_leg, dict):
            return
        venue = normalize_venue(maker_leg.get("exchange"))
        symbol = normalize_symbol(maker_leg.get("symbol"))
        if venue != "unknown":
            context.venue = venue
        if symbol and symbol != "UNKNOWN/UNKNOWN":
            context.symbol = symbol
            context.token = _symbol_base(symbol)

    def poll_quote_states(self, *, now_ms: int) -> None:
        quote_up_values: dict[tuple[str, ...], float] = {}
        depth_100_values: dict[tuple[str, ...], float] = {}
        depth_200_values: dict[tuple[str, ...], float] = {}
        for strategy_id in self._contexts:
            previous_labels = self._labels(strategy_id)
            try:
                state_raw = self.redis.get(f"maker_arb:{strategy_id}:state")
                state = self._parse_state_payload(state_raw) or {}
                self._update_context_from_state(strategy_id, state)

                labels = self._labels(strategy_id)
                label_values = self._label_values(labels)
                quote_up = compute_quote_up(
                    state.get("mode"),
                    state.get("ts_ms"),
                    now_ms,
                    self.state_stale_ms,
                )
                quote_up_values[label_values] = float(quote_up)

                snapshot = state.get("quote_snapshot") if isinstance(state.get("quote_snapshot"), dict) else {}
                top_bid = _to_decimal(snapshot.get("maker_top_bid")) or _to_decimal(state.get("maker_top_bid"))
                top_ask = _to_decimal(snapshot.get("maker_top_ask")) or _to_decimal(state.get("maker_top_ask"))
                maker_orders = state.get("maker_orders") if isinstance(state.get("maker_orders"), dict) else {}
                if top_bid is None or top_ask is None:
                    depth_100 = Decimal("0")
                    depth_200 = Decimal("0")
                else:
                    depth_100 = compute_depth_usd_within_bps(
                        maker_orders=maker_orders,
                        top_bid=top_bid,
                        top_ask=top_ask,
                        bps_limit=100,
                    )
                    depth_200 = compute_depth_usd_within_bps(
                        maker_orders=maker_orders,
                        top_bid=top_bid,
                        top_ask=top_ask,
                        bps_limit=200,
                    )

                depth_100_values[label_values] = float(depth_100)
                depth_200_values[label_values] = float(depth_200)
            except Exception:
                LOGGER.exception("failed to poll strategy state for %s", strategy_id)
                self._preserve_current_metric_values(
                    labels=previous_labels,
                    quote_up_values=quote_up_values,
                    depth_100_values=depth_100_values,
                    depth_200_values=depth_200_values,
                )

        self._sync_metric(
            gauge=self.g_quote_up,
            metric_key="quote_up",
            values=quote_up_values,
        )
        self._sync_metric(
            gauge=self.g_depth_100,
            metric_key="depth_100",
            values=depth_100_values,
        )
        self._sync_metric(
            gauge=self.g_depth_200,
            metric_key="depth_200",
            values=depth_200_values,
        )


def _parse_strategy_ids(values: list[str]) -> list[str]:
    parsed: list[str] = []
    seen: set[str] = set()
    for raw_value in values:
        for part in raw_value.split(","):
            strategy_id = part.strip()
            if not strategy_id or strategy_id in seen:
                continue
            seen.add(strategy_id)
            parsed.append(strategy_id)
    return parsed


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Export TokenMM quote uptime and quote depth metrics from Redis state.",
    )
    parser.add_argument(
        "--strategy-id",
        action="append",
        default=[],
        help="Strategy id to export. Repeat or pass a comma-separated list.",
    )
    parser.add_argument(
        "--config-dir",
        default="configs",
        help="Config directory containing strategies.ini for strategy discovery.",
    )
    parser.add_argument(
        "--strategy-group",
        default="tokenmm",
        help="Strategy group used when discovering ids from strategies.ini.",
    )
    parser.add_argument(
        "--env",
        default="prod",
        help="Environment label attached to exported metrics.",
    )
    parser.add_argument(
        "--redis-url",
        default=_env("REDIS_URL", "redis://localhost:6379/0"),
        help="Redis URL for reading existing maker state.",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=int(_env("EXPORTER_PORT", "9108")),
        help="HTTP port for the Prometheus exporter.",
    )
    parser.add_argument(
        "--poll-interval-s",
        type=poll_interval_seconds_arg,
        default=float(_env("POLL_INTERVAL_S", "5")),
        help="Polling interval in seconds.",
    )
    parser.add_argument(
        "--state-stale-ms",
        type=_non_negative_int,
        default=30_000,
        help="Freshness window for quote-up evaluation.",
    )
    parser.add_argument(
        "--log-level",
        default=_env("LOG_LEVEL", "INFO"),
        help="Python log level.",
    )
    return parser


def _resolve_strategy_ids(args: argparse.Namespace) -> list[str]:
    strategy_ids = _parse_strategy_ids(args.strategy_id)
    if strategy_ids:
        return strategy_ids

    discovered = discover_strategy_contexts(
        config_dir=str(args.config_dir),
        strategy_group=str(args.strategy_group),
    )
    if discovered:
        return list(discovered.keys())

    return list(DEFAULT_STRATEGY_IDS)


def main(argv: list[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)

    logging.basicConfig(level=getattr(logging, str(args.log_level).upper(), logging.INFO))
    strategy_ids = _resolve_strategy_ids(args)
    redis_client = redis.Redis.from_url(str(args.redis_url))
    exporter = TokenMMMetricsExporter(
        redis_client=redis_client,
        env=str(args.env),
        strategy_ids=strategy_ids,
        state_stale_ms=int(args.state_stale_ms),
        strategy_context_overrides=discover_strategy_contexts(
            config_dir=str(args.config_dir),
            strategy_group=str(args.strategy_group),
        ),
    )

    start_http_server(int(args.port), registry=exporter.registry)
    LOGGER.info(
        "tokenmm liquidity exporter started on :%s for %s strategy ids",
        args.port,
        len(strategy_ids),
    )

    done = False

    def _sig(_signum, _frame) -> None:
        nonlocal done
        done = True

    signal.signal(signal.SIGINT, _sig)
    signal.signal(signal.SIGTERM, _sig)

    while not done:
        now_ms = int(time.time() * 1000)
        try:
            exporter.poll_quote_states(now_ms=now_ms)
        except Exception:
            LOGGER.exception("quote-state poll failed")
        time.sleep(max(0.5, float(args.poll_interval_s)))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
