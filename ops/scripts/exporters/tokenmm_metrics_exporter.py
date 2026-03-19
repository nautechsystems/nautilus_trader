#!/usr/bin/env python3
"""Minimal TokenMM Prometheus sidecar exporter for quoting liquidity panels."""

from __future__ import annotations

import argparse
import configparser
import importlib.util
import json
import logging
import os
import re
import signal
import sys
import time
import tomllib
from dataclasses import dataclass
from decimal import Decimal
from decimal import InvalidOperation
from pathlib import Path
from typing import Any
from urllib.parse import quote

import redis
from prometheus_client import CollectorRegistry
from prometheus_client import Gauge
from prometheus_client import start_http_server

REPO_ROOT = Path(__file__).resolve().parents[3]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from ops.scripts.exporters.common import poll_interval_seconds_arg

LOGGER = logging.getLogger("tokenmm_metrics_exporter")

LABEL_NAMES = ("env", "strategy_id", "token", "venue", "symbol", "strategy_family")
FLUX_DEFAULT_NAMESPACE = "flux"
FLUX_SCHEMA_VERSION = "v1"
_IDENTIFIER_SAFE_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
DEFAULT_STRATEGY_IDS = (
    "plumeusdt_bybit_perp_makerv3",
    "plumeusdt_bybit_spot_makerv3",
    "plumeusdt_okx_perp_makerv3",
    "plumeusdt_binance_perp_makerv3",
    "plumeusdt_binance_spot_makerv3",
    "plumeusdt_bitget_perp_makerv3",
    "plumeusdt_bitget_spot_makerv3",
)
DEFAULT_STRATEGY_FAMILY = "maker_v3"
ACTIVE_ORDER_STATUSES = {"open", "live", "placed"}
KNOWN_QUOTES = ("USDT", "USDC", "USD", "BTC", "ETH")
KNOWN_MARKET_VENUES = {
    "binance_spot",
    "binance_perp",
    "bybit_linear",
    "bybit_inverse",
    "bybit_perp",
    "bybit_spot",
    "bitget_perp",
    "bitget_spot",
    "okx_perp",
    "okx_spot",
}
STRATEGY_PARAM_KEYS = (
    "order_qty",
    "qty",
    "qty_unit",
    "bid_edge1",
    "ask_edge1",
    "place_edge1",
    "distance1",
    "n_orders1",
    "bid_edge2",
    "ask_edge2",
    "place_edge2",
    "distance2",
    "n_orders2",
    "bid_edge3",
    "ask_edge3",
    "place_edge3",
    "distance3",
    "n_orders3",
)


def _load_pricing_helper(name: str) -> Any:
    pricing_path = REPO_ROOT / "flux/strategies/makerv3/pricing.py"
    spec = importlib.util.spec_from_file_location("tokenmm_metrics_exporter_pricing", pricing_path)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load pricing helpers from {pricing_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return getattr(module, name)


apply_inventory_skew_to_edges = _load_pricing_helper("apply_inventory_skew_to_edges")
build_ladder_place_cancel_levels_from_bps = _load_pricing_helper(
    "build_ladder_place_cancel_levels_from_bps",
)


def _validate_identifier_part(value: str, field_name: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    if _IDENTIFIER_SAFE_PATTERN.fullmatch(value) is None:
        raise ValueError(
            f"`{field_name}` was not identifier-safe: {value!r}. "
            "Allowed characters are letters, digits, '.', '_' and '-'.",
        )
    return value


def _validate_schema_version(value: str, field_name: str = "schema_version") -> str:
    _validate_identifier_part(value, field_name)
    if value != FLUX_SCHEMA_VERSION:
        raise ValueError(
            f"`{field_name}` was unsupported: {value!r}. "
            f"Supported schema version is {FLUX_SCHEMA_VERSION!r}.",
        )
    return value


def _flux_key_prefix(
    *,
    namespace: str = FLUX_DEFAULT_NAMESPACE,
    schema_version: str = FLUX_SCHEMA_VERSION,
) -> str:
    safe_namespace = _validate_identifier_part(namespace, "namespace")
    safe_schema_version = _validate_schema_version(schema_version, "schema_version")
    return f"{safe_namespace}:{safe_schema_version}"


def _flux_state_key(strategy_id: str) -> str:
    safe_strategy_id = _validate_identifier_part(strategy_id, "strategy_id")
    return f"{_flux_key_prefix()}:state:{safe_strategy_id}"


def _flux_params_hash_key(strategy_id: str) -> str:
    safe_strategy_id = _validate_identifier_part(strategy_id, "strategy_id")
    return f"{_flux_key_prefix()}:params:{safe_strategy_id}"


def _env(name: str, default: str) -> str:
    value = os.getenv(name)
    return value if value else default


def _env_optional(name: str) -> str | None:
    value = os.getenv(name)
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _env_bool(name: str) -> bool | None:
    value = _env_optional(name)
    if value is None:
        return None
    lowered = value.lower()
    if lowered in {"1", "true", "yes", "on"}:
        return True
    if lowered in {"0", "false", "no", "off"}:
        return False
    raise ValueError(f"`{name}` must be a boolean-like value")


def _default_redis_url() -> str:
    explicit = _env_optional("REDIS_URL")
    if explicit is not None:
        return explicit

    host = _env_optional("TOKENMM_REDIS_HOST")
    if host is None:
        return "redis://localhost:6379/0"

    scheme = "rediss" if _env_bool("TOKENMM_REDIS_SSL") else "redis"
    port = _env_optional("TOKENMM_REDIS_PORT") or "6379"
    db = _env_optional("TOKENMM_REDIS_DB") or "0"
    username = _env_optional("TOKENMM_REDIS_USERNAME")
    password = os.getenv("TOKENMM_REDIS_PASSWORD")

    auth = ""
    if username is not None or password is not None:
        encoded_user = quote(username or "", safe="")
        encoded_password = quote(password or "", safe="")
        auth = f"{encoded_user}:{encoded_password}@"

    return f"{scheme}://{auth}{host}:{port}/{db}"


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


def _decode_text(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="ignore")
    if value is None:
        return ""
    return str(value)


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
    if text in {"bybit_perp", "bybit_linear", "bybit_inverse"}:
        return text
    if text in {"bitget", "bitget_spot"}:
        return "bitget_spot"
    if text == "bitget_perp":
        return "bitget_perp"
    if text in {"okx", "okx_spot"}:
        return "okx_spot"
    if text in {"binance", "binance_spot"}:
        return "binance_spot"
    if text in KNOWN_MARKET_VENUES:
        return text
    if text.startswith("bybit"):
        return "bybit_spot"
    if text.startswith("bitget_perp"):
        return "bitget_perp"
    if text.startswith("bitget"):
        return "bitget_spot"
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
    for quote_name in KNOWN_QUOTES:
        if compact.endswith(quote_name) and len(compact) > len(quote_name):
            base = compact[: -len(quote_name)]
            return f"{base}/{quote_name}"
    return text


def _symbol_base(symbol: str) -> str:
    return symbol.split("/", 1)[0] if "/" in symbol else symbol


def compute_quote_up(mode: Any, state_ts_ms: Any, now_ms: int, state_stale_ms: int) -> int:
    if str(mode or "").strip().upper() not in {"QUOTING", "ON"}:
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


def _int_or_zero(value: Any) -> int:
    parsed = _to_int(value)
    return max(0, parsed or 0)


def _quote_snapshot_from_state(state: dict[str, Any]) -> dict[str, Any]:
    quote_snapshot: dict[str, Any] = {}
    raw_top_level = state.get("quote_snapshot")
    if isinstance(raw_top_level, dict):
        quote_snapshot.update(raw_top_level)
    maker_v3 = state.get("maker_v3")
    if isinstance(maker_v3, dict):
        nested = maker_v3.get("quote_snapshot")
        if isinstance(nested, dict):
            quote_snapshot.update(nested)
    return quote_snapshot


def _parse_params_payload(raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        return {}
    payload: dict[str, Any] = {}
    for key, value in raw.items():
        text_key = _decode_text(key).strip()
        if not text_key:
            continue
        payload[text_key] = _decode_text(value)
    return payload


def _apply_skewed_edges(
    *,
    params: dict[str, Any],
    total_skew_bps: Decimal,
) -> tuple[tuple[Decimal, Decimal, Decimal], tuple[Decimal, Decimal, Decimal]]:
    bid_edges: list[Decimal] = []
    ask_edges: list[Decimal] = []
    for idx in range(1, 4):
        bid_edge = _to_decimal(params.get(f"bid_edge{idx}")) or Decimal("0")
        ask_edge = _to_decimal(params.get(f"ask_edge{idx}")) or Decimal("0")
        bid_eff, ask_eff = apply_inventory_skew_to_edges(
            bid_edge_bps=bid_edge,
            ask_edge_bps=ask_edge,
            total_skew_bps=total_skew_bps,
        )
        bid_edges.append(bid_eff)
        ask_edges.append(ask_eff)
    return (
        (bid_edges[0], bid_edges[1], bid_edges[2]),
        (ask_edges[0], ask_edges[1], ask_edges[2]),
    )


def _project_depth_usd_from_quote_status(
    *,
    quote_status: dict[str, Any],
    params: dict[str, Any],
    quote_snapshot: dict[str, Any],
    top_bid: Decimal,
    top_ask: Decimal,
    bps_limit: int,
) -> Decimal:
    qty_unit = str(params.get("qty_unit") or "base").strip().lower()
    if qty_unit not in {"", "base"}:
        return Decimal("0")
    qty = _to_decimal(params.get("qty") or params.get("order_qty"))
    if qty is None or qty <= 0:
        return Decimal("0")

    total_skew_bps = _to_decimal(quote_snapshot.get("skew_bps_signed")) or Decimal("0")
    bid_edges, ask_edges = _apply_skewed_edges(params=params, total_skew_bps=total_skew_bps)
    place_edges = tuple(
        _to_decimal(params.get(f"place_edge{idx}")) or Decimal("0")
        for idx in range(1, 4)
    )
    distances = tuple(
        _to_decimal(params.get(f"distance{idx}")) or Decimal("0")
        for idx in range(1, 4)
    )
    n_orders = tuple(_int_or_zero(params.get(f"n_orders{idx}")) for idx in range(1, 4))

    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=top_bid,
        anchor_ask=top_ask,
        bid_edges_bps=bid_edges,
        ask_edges_bps=ask_edges,
        place_edges_bps=place_edges,
        distances_bps=distances,
        n_orders=n_orders,
        tick=Decimal("0"),
    )
    mid = (top_bid + top_ask) / Decimal("2")
    if mid <= 0:
        return Decimal("0")
    limit = Decimal(str(bps_limit))
    total = Decimal("0")

    bid_open = min(_int_or_zero(quote_status.get("bid_open")), len(bid_levels))
    ask_open = min(_int_or_zero(quote_status.get("ask_open")), len(ask_levels))
    for place_px, _cancel_px in bid_levels[:bid_open]:
        distance_bps = (abs(place_px - mid) / mid) * Decimal("10000")
        if distance_bps <= limit:
            total += place_px * qty
    for place_px, _cancel_px in ask_levels[:ask_open]:
        distance_bps = (abs(place_px - mid) / mid) * Decimal("10000")
        if distance_bps <= limit:
            total += place_px * qty
    return total


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
            "strategy_id": self.strategy_id,
            "token": self.token,
            "venue": self.venue,
            "symbol": self.symbol,
            "strategy_family": self.strategy_family,
        }


def _parse_strategy_context(strategy_id: str) -> StrategyContext:
    sid = str(strategy_id or "").strip()
    parts = sid.split("_")
    if len(parts) >= 4 and parts[0] and parts[-1] == "makerv3":
        symbol = normalize_symbol(parts[0])
        maker_venue = normalize_venue("_".join(parts[1:3]))
        token = _symbol_base(symbol)
        return StrategyContext(
            strategy_id=sid,
            token=token,
            venue=maker_venue,
            symbol=symbol,
        )

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


def discover_strategy_param_defaults(
    *,
    strategy_ids: list[str] | tuple[str, ...],
    strategy_config_dir: str = "deploy/tokenmm/strategies",
) -> dict[str, dict[str, Any]]:
    defaults: dict[str, dict[str, Any]] = {}
    base_dir = Path(strategy_config_dir)
    for raw_strategy_id in strategy_ids:
        strategy_id = str(raw_strategy_id or "").strip()
        if not strategy_id:
            continue
        config_path = base_dir / f"{strategy_id}.toml"
        try:
            config_data = tomllib.loads(config_path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            continue
        except (OSError, tomllib.TOMLDecodeError):
            LOGGER.exception("failed to read strategy config defaults: %s", config_path)
            continue
        strategy_section = config_data.get("strategy")
        if not isinstance(strategy_section, dict):
            continue
        params: dict[str, Any] = {}
        for key in STRATEGY_PARAM_KEYS:
            value = strategy_section.get(key)
            if value is not None:
                params[key] = value
        if params:
            defaults[strategy_id] = params
    return defaults


class TokenMMMetricsExporter:
    def __init__(
        self,
        *,
        redis_client: Any,
        env: str,
        strategy_ids: list[str] | tuple[str, ...],
        state_stale_ms: int = 30_000,
        strategy_context_overrides: dict[str, StrategyContext] | None = None,
        strategy_param_defaults: dict[str, dict[str, Any]] | None = None,
        registry: CollectorRegistry | None = None,
    ) -> None:
        self.redis = redis_client
        self.env = str(env or "prod")
        self.state_stale_ms = int(state_stale_ms)
        self._param_defaults = {
            str(strategy_id): dict(values)
            for strategy_id, values in (strategy_param_defaults or {}).items()
        }

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
        quote_snapshot = _quote_snapshot_from_state(state)
        venue = "unknown"
        symbol = "UNKNOWN/UNKNOWN"
        if isinstance(maker_leg, dict):
            venue = normalize_venue(maker_leg.get("exchange"))
            symbol = normalize_symbol(maker_leg.get("symbol"))
        if venue == "unknown":
            venue = normalize_venue(
                quote_snapshot.get("maker_exchange")
                or quote_snapshot.get("exchange"),
            )
        if symbol == "UNKNOWN/UNKNOWN":
            symbol = normalize_symbol(
                quote_snapshot.get("maker_symbol")
                or quote_snapshot.get("symbol"),
            )
        if venue != "unknown":
            context.venue = venue
        if symbol and symbol != "UNKNOWN/UNKNOWN":
            context.symbol = symbol
            context.token = _symbol_base(symbol)

    def _state_key(self, strategy_id: str) -> str:
        return _flux_state_key(strategy_id)

    def _params_key(self, strategy_id: str) -> str:
        return _flux_params_hash_key(strategy_id)

    def _load_state(self, strategy_id: str) -> dict[str, Any]:
        primary_raw = self.redis.get(self._state_key(strategy_id))
        primary = self._parse_state_payload(primary_raw)
        if primary:
            return primary
        legacy_raw = self.redis.get(f"maker_arb:{strategy_id}:state")
        return self._parse_state_payload(legacy_raw) or {}

    def _load_params(self, strategy_id: str) -> dict[str, Any]:
        params = dict(self._param_defaults.get(strategy_id, {}))
        hgetall = getattr(self.redis, "hgetall", None)
        if not callable(hgetall):
            return params
        try:
            params.update(_parse_params_payload(hgetall(self._params_key(strategy_id))))
        except Exception:
            LOGGER.exception("failed to read params for %s", strategy_id)
        return params

    def poll_quote_states(self, *, now_ms: int) -> None:
        quote_up_values: dict[tuple[str, ...], float] = {}
        depth_100_values: dict[tuple[str, ...], float] = {}
        depth_200_values: dict[tuple[str, ...], float] = {}
        for strategy_id in self._contexts:
            previous_labels = self._labels(strategy_id)
            try:
                state = self._load_state(strategy_id)
                self._update_context_from_state(strategy_id, state)
                params = self._load_params(strategy_id)

                labels = self._labels(strategy_id)
                label_values = self._label_values(labels)
                quote_snapshot = _quote_snapshot_from_state(state)
                effective_bot_on = state.get("effective_bot_on")
                quote_mode = quote_snapshot.get("mode") or state.get("mode")
                if quote_mode in (None, "") and effective_bot_on is not None:
                    quote_mode = "ON" if bool(effective_bot_on) else "OFF"
                quote_up = compute_quote_up(
                    quote_mode,
                    quote_snapshot.get("ts_ms") or state.get("ts_ms"),
                    now_ms,
                    self.state_stale_ms,
                )
                quote_up_values[label_values] = float(quote_up)

                top_bid = _to_decimal(quote_snapshot.get("maker_top_bid")) or _to_decimal(
                    state.get("maker_top_bid"),
                )
                top_ask = _to_decimal(quote_snapshot.get("maker_top_ask")) or _to_decimal(
                    state.get("maker_top_ask"),
                )
                maker_orders = state.get("maker_orders") if isinstance(state.get("maker_orders"), dict) else {}
                quote_status = (
                    state.get("maker_quote_status")
                    if isinstance(state.get("maker_quote_status"), dict)
                    else {}
                )
                if top_bid is None or top_ask is None:
                    depth_100 = Decimal("0")
                    depth_200 = Decimal("0")
                elif maker_orders:
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
                else:
                    depth_100 = _project_depth_usd_from_quote_status(
                        quote_status=quote_status,
                        params=params,
                        quote_snapshot=quote_snapshot,
                        top_bid=top_bid,
                        top_ask=top_ask,
                        bps_limit=100,
                    )
                    depth_200 = _project_depth_usd_from_quote_status(
                        quote_status=quote_status,
                        params=params,
                        quote_snapshot=quote_snapshot,
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
        "--strategy-config-dir",
        default="deploy/tokenmm/strategies",
        help="Directory containing per-strategy TOML configs for quote ladder defaults.",
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
        default=_default_redis_url(),
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
        strategy_param_defaults=discover_strategy_param_defaults(
            strategy_ids=strategy_ids,
            strategy_config_dir=str(args.strategy_config_dir),
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
