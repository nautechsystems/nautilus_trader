#!/usr/bin/env python3
from __future__ import annotations

import argparse
import logging
import signal
import time
import tomllib
from pathlib import Path
from typing import Any

import redis

from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
from flux.common.portfolio_inventory import aggregate_components
from flux.common.portfolio_inventory import decode_component
from flux.common.portfolio_inventory import encode_portfolio_inventory
from flux.runners.tokenmm.redis_runtime import apply_redis_env_overrides


SAFE_MODES = frozenset({"paper", "testnet", "live"})
POLL_INTERVAL_SECS = 0.25


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
    parser = argparse.ArgumentParser(description="Run TokenMM portfolio inventory aggregator.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
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


def _tokenmm_strategy_ids(api_cfg: dict[str, Any]) -> list[str]:
    raw = api_cfg.get("tokenmm_strategy_ids") or []
    if not isinstance(raw, list):
        raise ValueError("`api.tokenmm_strategy_ids` must be a TOML array")
    out: list[str] = []
    seen: set[str] = set()
    for value in raw:
        text = _optional_text(value)
        if not text or text in seen:
            continue
        seen.add(text)
        out.append(text)
    if not out:
        raise ValueError("`api.tokenmm_strategy_ids` must be non-empty")
    return out


def _required_strategy_ids(api_cfg: dict[str, Any], *, fallback: list[str]) -> list[str]:
    raw = api_cfg.get("tokenmm_required_strategy_ids") or []
    if not raw:
        return list(fallback)
    if not isinstance(raw, list):
        raise ValueError("`api.tokenmm_required_strategy_ids` must be a TOML array")
    out: list[str] = []
    seen: set[str] = set()
    allowlist = set(fallback)
    for value in raw:
        text = _optional_text(value)
        if not text or text in seen:
            continue
        if text not in allowlist:
            raise ValueError(f"required TokenMM strategy not in allowlist: {text}")
        seen.add(text)
        out.append(text)
    return out or list(fallback)


def _portfolio_base_assets(config: dict[str, Any]) -> list[str]:
    contracts = config.get("contracts") or []
    out: list[str] = []
    seen: set[str] = set()
    if isinstance(contracts, list):
        for item in contracts:
            if not isinstance(item, dict):
                continue
            symbol = _optional_text(item.get("symbol")) or ""
            base = symbol.split("/", maxsplit=1)[0].strip().upper()
            if not base or base in seen:
                continue
            seen.add(base)
            out.append(base)
    return out or ["PLUME"]


class TokenMMPortfolioAggregator:
    def __init__(self, *, config: dict[str, Any], mode: str, logger: logging.Logger) -> None:
        flux = _table(config, "flux")
        redis_cfg = _table(config, "redis")
        api_cfg = _table(config, "api")
        portfolio_cfg = _table(config, "portfolio")

        self._namespace = str(flux.get("namespace", "flux"))
        self._schema_version = str(flux.get("schema_version", "v1"))
        self._mode = mode
        self._portfolio_id = _optional_text(portfolio_cfg.get("portfolio_id")) or "tokenmm"
        self._stale_after_ms = int(
            portfolio_cfg.get(
                "inventory_stale_after_ms",
                DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS,
            ),
        )
        self._strategy_ids = _tokenmm_strategy_ids(api_cfg)
        self._required_strategy_ids = set(
            _required_strategy_ids(api_cfg, fallback=self._strategy_ids),
        )
        self._base_assets = _portfolio_base_assets(config)
        self._redis = redis.Redis(
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
        self._log = logger
        self._running = True

    def stop(self, *_args: Any) -> None:
        self._running = False

    def _component_key(self, *, strategy_id: str, base_currency: str) -> str:
        return FluxRedisKeys.portfolio_inventory_component(
            strategy_id=strategy_id,
            portfolio_id=self._portfolio_id,
            base_currency=base_currency,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _aggregate_key(self, *, base_currency: str) -> str:
        return FluxRedisKeys.portfolio_inventory(
            portfolio_id=self._portfolio_id,
            base_currency=base_currency,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def _aggregate_channel(self, *, base_currency: str) -> str:
        return FluxRedisKeys.portfolio_inventory_channel(
            portfolio_id=self._portfolio_id,
            base_currency=base_currency,
            namespace=self._namespace,
            schema_version=self._schema_version,
        )

    def recompute_once(self) -> None:
        now_ms_value = int(time.time() * 1000)
        for base_currency in self._base_assets:
            pipeline = self._redis.pipeline(transaction=False)
            for strategy_id in self._strategy_ids:
                pipeline.get(self._component_key(strategy_id=strategy_id, base_currency=base_currency))
            raw_components = pipeline.execute()
            components = {
                strategy_id: decode_component(raw)
                for strategy_id, raw in zip(self._strategy_ids, raw_components, strict=True)
            }
            payload = aggregate_components(
                portfolio_id=self._portfolio_id,
                base_currency=base_currency,
                components=components,
                required_strategy_ids=self._required_strategy_ids,
                now_ms_value=now_ms_value,
                stale_after_ms=self._stale_after_ms,
            )
            encoded = encode_portfolio_inventory(payload)
            key = self._aggregate_key(base_currency=base_currency)
            previous = self._redis.get(key)
            self._redis.set(key, encoded)
            if previous != encoded.encode():
                self._redis.publish(self._aggregate_channel(base_currency=base_currency), encoded)

    def run(self) -> None:
        signal.signal(signal.SIGINT, self.stop)
        signal.signal(signal.SIGTERM, self.stop)
        self._log.info(
            "TokenMM portfolio aggregator started portfolio_id=%s mode=%s bases=%s strategies=%s",
            self._portfolio_id,
            self._mode,
            self._base_assets,
            self._strategy_ids,
        )
        while self._running:
            self.recompute_once()
            time.sleep(POLL_INTERVAL_SECS)


def main() -> None:
    args = _parse_args()
    config = _load_config(args.config)
    mode = _resolve_mode(config, args)
    portfolio_cfg = _table(config, "portfolio")
    log_level = str(args.log_level or portfolio_cfg.get("log_level", "INFO")).upper()
    logging.basicConfig(
        level=getattr(logging, log_level, logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s - %(message)s",
    )
    aggregator = TokenMMPortfolioAggregator(
        config=config,
        mode=mode,
        logger=logging.getLogger("nautilus-tokenmm-portfolio"),
    )
    aggregator.run()


if __name__ == "__main__":
    main()
