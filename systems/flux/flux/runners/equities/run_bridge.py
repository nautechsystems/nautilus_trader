#!/usr/bin/env python3
"""
Run the flux bridge consumer for Equities strategy topics.
"""

from __future__ import annotations

import argparse
import tomllib
from pathlib import Path
from typing import Any

import redis

from flux.bridge.handlers import default_topic_handlers
from flux.bridge.stream_consumer import FluxBridgeStreamConsumer
from flux.common.config import validate_identifier_part
from flux.events import TOPIC_EXECUTION_ALERT
from flux.runners.equities.node_groups import derive_equities_node_group_id
from flux.runners.shared.logging import configure_python_logging
from flux.strategies.makerv3.constants import TOPIC_ALERT
from flux.strategies.makerv3.constants import TOPIC_BALANCES
from flux.strategies.makerv3.constants import TOPIC_EVENT
from flux.strategies.makerv3.constants import TOPIC_FV
from flux.strategies.makerv3.constants import TOPIC_MARKET_BBO
from flux.strategies.makerv3.constants import TOPIC_STATE
from flux.strategies.makerv3.constants import TOPIC_TRADE
from flux.runners.equities.redis_runtime import apply_redis_env_overrides


SAFE_MODES = frozenset({"paper", "testnet", "live"})


FULL_TO_SUFFIX_TOPICS: dict[str, str] = {
    TOPIC_STATE: "state",
    TOPIC_EVENT: "event",
    TOPIC_TRADE: "trade",
    TOPIC_ALERT: "alert",
    TOPIC_EXECUTION_ALERT: "alert",
    TOPIC_MARKET_BBO: "market_bbo",
    TOPIC_FV: "fv",
    TOPIC_BALANCES: "balances",
}


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _load_config(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"Config root must be a table: {path}")
    return apply_redis_env_overrides(data)


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"[{name}] must be a TOML table")
    return value


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run Flux bridge consumer for Equities.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--strategy-id", action="append", default=None)
    parser.add_argument("--all-strategies", action="store_true")
    parser.add_argument("--topic", action="append", default=[])
    parser.add_argument("--log-level", default=None)
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    flux = _table(config, "flux")
    mode = str(args.mode or flux.get("mode", "paper")).strip().lower()
    if mode not in SAFE_MODES:
        raise ValueError(f"Invalid mode {mode!r}; expected one of {sorted(SAFE_MODES)}")
    if mode == "live" and not args.confirm_live:
        raise ValueError("Live mode requires explicit --confirm-live")
    return mode


def _build_handlers() -> dict[str, Any]:
    base_handlers = default_topic_handlers()
    handlers = dict(base_handlers)
    for full_topic, suffix in FULL_TO_SUFFIX_TOPICS.items():
        handlers[full_topic] = base_handlers[suffix]
    return handlers


def _coerce_strategy_ids(raw_value: Any) -> list[str]:
    if raw_value is None:
        return []
    values: list[Any]
    if isinstance(raw_value, str):
        values = [raw_value]
    elif isinstance(raw_value, list):
        values = list(raw_value)
    else:
        return []

    out: list[str] = []
    seen: set[str] = set()
    for index, value in enumerate(values):
        strategy_id = _optional_text(value)
        if not strategy_id:
            continue
        strategy_id = validate_identifier_part(strategy_id, f"strategy_id[{index}]")
        if strategy_id in seen:
            continue
        seen.add(strategy_id)
        out.append(strategy_id)
    return out


def _resolve_strategy_ids(config: dict[str, Any], args: argparse.Namespace) -> list[str] | None:
    api_cfg = _table(config, "api")
    strategy_id_args = _coerce_strategy_ids(args.strategy_id)
    all_strategies = bool(args.all_strategies)

    if all_strategies and strategy_id_args:
        raise ValueError("`--strategy-id` and `--all-strategies` cannot be used together")
    if all_strategies:
        return None

    if strategy_id_args:
        return strategy_id_args

    configured_ids = _coerce_strategy_ids(api_cfg.get("equities_strategy_ids"))
    if configured_ids:
        return configured_ids

    raise ValueError(
        "`api.equities_strategy_ids` must be configured explicitly for the equities bridge "
        "unless `--strategy-id` or `--all-strategies` is provided",
    )


def _resolve_stream_strategy_ids(strategy_ids: list[str] | None) -> list[str] | None:
    if strategy_ids is None:
        return None

    resolved: list[str] = []
    seen: set[str] = set()
    for strategy_id in strategy_ids:
        try:
            stream_strategy_id = derive_equities_node_group_id(strategy_id)
        except ValueError:
            stream_strategy_id = strategy_id
        if stream_strategy_id in seen:
            continue
        seen.add(stream_strategy_id)
        resolved.append(stream_strategy_id)
    return resolved


def main() -> None:
    """
    Parse CLI arguments and run the Equities flux bridge consumer.
    """
    args = _parse_args()
    config = _load_config(args.config)
    mode = _resolve_mode(config, args)
    strategy_scope = _resolve_strategy_ids(config, args)
    stream_strategy_scope = _resolve_stream_strategy_ids(strategy_scope)

    flux = _table(config, "flux")
    redis_cfg = _table(config, "redis")
    bridge_cfg = _table(config, "bridge")

    configure_python_logging(
        cli_level=args.log_level,
        config_level=bridge_cfg.get("log_level", "INFO"),
        service_env_var="FLUX_BRIDGE_LOG_LEVEL",
    )

    handlers = _build_handlers()
    topics = list(args.topic or FULL_TO_SUFFIX_TOPICS.keys())
    invalid_topics = sorted(topic for topic in topics if topic not in handlers)
    if invalid_topics:
        raise ValueError(f"Unsupported topic(s): {invalid_topics}. Supported: {sorted(handlers)}")

    redis_client = redis.Redis(
        host=str(redis_cfg.get("host", "127.0.0.1")),
        port=int(redis_cfg.get("port", 6380)),
        db=int(redis_cfg.get("db", 0)),
        username=_optional_text(redis_cfg.get("username")),
        password=_optional_text(redis_cfg.get("password")),
        ssl=bool(redis_cfg.get("ssl", False)),
        socket_connect_timeout=float(redis_cfg.get("connect_timeout_secs", 5.0)),
        socket_timeout=float(redis_cfg.get("read_timeout_secs", 5.0)),
        decode_responses=False,
    )

    consumer = FluxBridgeStreamConsumer(
        redis_client=redis_client,
        environment=mode,
        strategy_ids=strategy_scope,
        stream_strategy_ids=stream_strategy_scope,
        namespace=str(flux.get("namespace", "flux")),
        schema_version=str(flux.get("schema_version", "v1")),
        handlers=handlers,
        topics=topics,
        start_id=str(bridge_cfg.get("start_id", "$")),
        block_ms=int(bridge_cfg.get("block_ms", 1_000)),
        read_count=int(bridge_cfg.get("read_count", 200)),
        scan_interval_sec=float(bridge_cfg.get("scan_interval_sec", 3.0)),
    )
    consumer.run()


if __name__ == "__main__":
    main()
