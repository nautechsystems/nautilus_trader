#!/usr/bin/env python3
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
Run the flux bridge consumer for TokenMM strategy topics.
"""

from __future__ import annotations

import argparse
import logging
import tomllib
from pathlib import Path
from typing import Any

import redis

from nautilus_trader.flux.bridge.handlers import default_topic_handlers
from nautilus_trader.flux.bridge.stream_consumer import FluxBridgeStreamConsumer
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_ALERT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_BALANCES
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_EVENT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_FV
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_MARKET_BBO
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_STATE
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_TRADE


SAFE_MODES = frozenset({"paper", "testnet", "live"})


FULL_TO_SUFFIX_TOPICS: dict[str, str] = {
    TOPIC_STATE: "state",
    TOPIC_EVENT: "event",
    TOPIC_TRADE: "trade",
    TOPIC_ALERT: "alert",
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
    return data


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"[{name}] must be a TOML table")
    return value


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run Flux bridge consumer for TokenMM.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--strategy-id", default=None)
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


def _resolve_strategy_scope(config: dict[str, Any], args: argparse.Namespace) -> str | None:
    identity = _table(config, "identity")
    strategy_id_arg = _optional_text(args.strategy_id)
    all_strategies = bool(args.all_strategies)

    if all_strategies and strategy_id_arg is not None:
        raise ValueError("`--strategy-id` and `--all-strategies` cannot be used together")
    if all_strategies:
        return None

    strategy_id = strategy_id_arg or _optional_text(identity.get("strategy_id"))
    if not strategy_id:
        raise ValueError("A non-empty strategy_id is required unless `--all-strategies` is set")
    return strategy_id


def main() -> None:
    """
    Parse CLI arguments and run the TokenMM flux bridge consumer.
    """
    args = _parse_args()
    config = _load_config(args.config)
    mode = _resolve_mode(config, args)
    strategy_scope = _resolve_strategy_scope(config, args)

    flux = _table(config, "flux")
    redis_cfg = _table(config, "redis")
    bridge_cfg = _table(config, "bridge")

    log_level = str(args.log_level or bridge_cfg.get("log_level", "INFO")).upper()
    logging.basicConfig(
        level=getattr(logging, log_level, logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s - %(message)s",
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
        socket_connect_timeout=float(redis_cfg.get("connect_timeout_secs", 5.0)),
        socket_timeout=float(redis_cfg.get("read_timeout_secs", 5.0)),
        decode_responses=False,
    )

    consumer = FluxBridgeStreamConsumer(
        redis_client=redis_client,
        environment=mode,
        strategy_id=strategy_scope,
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
