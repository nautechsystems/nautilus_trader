from __future__ import annotations

import argparse
import fcntl
import os
import sys
import tomllib
from collections.abc import Callable
from collections.abc import Iterable
from contextlib import contextmanager
from pathlib import Path
from typing import Any

import redis
from nautilus_trader.config import DatabaseConfig

from flux.runners.shared.redis_env import apply_redis_env_overrides
from flux.runners.shared.redis_env import optional_text as redis_optional_text
from flux.runners.shared.strategy_set import StrategySetDescriptor

if __name__ == "flux.runners.shared.bootstrap":
    sys.modules.setdefault("nautilus_trader.flux.runners.shared.bootstrap", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.shared.bootstrap":
    sys.modules.setdefault("flux.runners.shared.bootstrap", sys.modules[__name__])


def optional_text(value: Any) -> str | None:
    return redis_optional_text(value)


def load_toml_config(
    path: Path,
    *,
    redis_override_loader,
) -> dict[str, Any]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"Config root must be a table: {path}")
    return redis_override_loader(data)


def load_config(path: Path, *, env_prefix: str) -> dict[str, Any]:
    return load_toml_config(
        path,
        redis_override_loader=lambda data: apply_redis_env_overrides(data, env_prefix=env_prefix),
    )


def merge_shared_tables(
    *,
    config: dict[str, Any],
    shared_config: dict[str, Any],
    table_names: Iterable[str],
) -> dict[str, Any]:
    merged = dict(config)
    for table_name in table_names:
        if table_name in merged:
            continue
        value = shared_config.get(table_name)
        if isinstance(value, dict):
            merged[table_name] = dict(value)
    return merged


def load_runtime_config(
    path: Path,
    *,
    shared_config_path: Path | None,
    load_config,
    table_names: Iterable[str],
) -> dict[str, Any]:
    config = load_config(path)
    if shared_config_path is None:
        return config
    shared_config = load_config(shared_config_path)
    return merge_shared_tables(
        config=config,
        shared_config=shared_config,
        table_names=table_names,
    )


def table(data: dict[str, Any], name: str) -> dict[str, Any]:
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"[{name}] must be a TOML table")
    return value


def resolve_mode(
    config: dict[str, Any],
    args: argparse.Namespace,
    *,
    safe_modes: frozenset[str],
) -> str:
    flux = table(config, "flux")
    mode = str(args.mode or flux.get("mode", "paper")).strip().lower()
    if mode not in safe_modes:
        raise ValueError(f"Invalid mode {mode!r}; expected one of {sorted(safe_modes)}")
    if mode == "live" and not args.confirm_live:
        raise ValueError("Live mode requires explicit --confirm-live")
    return mode


def build_redis_client_kwargs(redis_cfg: dict[str, Any]) -> dict[str, Any]:
    return {
        "host": str(redis_cfg.get("host", "127.0.0.1")),
        "port": int(redis_cfg.get("port", 6380)),
        "db": int(redis_cfg.get("db", 0)),
        "username": optional_text(redis_cfg.get("username")),
        "password": optional_text(redis_cfg.get("password")),
        "ssl": bool(redis_cfg.get("ssl", False)),
        "socket_connect_timeout": float(redis_cfg.get("connect_timeout_secs", 5.0)),
        "socket_timeout": float(redis_cfg.get("read_timeout_secs", 5.0)),
        "decode_responses": False,
    }


def build_redis_database_kwargs(redis_cfg: dict[str, Any]) -> dict[str, Any]:
    return {
        "type": "redis",
        "host": str(redis_cfg.get("host", "127.0.0.1")),
        "port": int(redis_cfg.get("port", 6380)),
        "username": optional_text(redis_cfg.get("username")),
        "password": optional_text(redis_cfg.get("password")),
        "ssl": bool(redis_cfg.get("ssl", False)),
    }


def build_redis_client(redis_cfg: dict[str, Any]) -> redis.Redis:
    return redis.Redis(**build_redis_client_kwargs(redis_cfg))


def build_redis_database_config(redis_cfg: dict[str, Any]) -> DatabaseConfig:
    return DatabaseConfig(**build_redis_database_kwargs(redis_cfg))


def attach_runtime_params_manager(
    *,
    strategy: Any,
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
    params_manager_factory: Callable[..., Any],
) -> None:
    strategy.set_params_manager_factory(
        params_manager_factory(
            redis_client=build_redis_client(redis_cfg),
            namespace=namespace,
            schema_version=schema_version,
        ),
    )


def attach_portfolio_inventory_feed(
    *,
    strategy: Any,
    config: dict[str, Any],
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
    default_portfolio_id: str,
) -> None:
    portfolio_cfg = table(config, "portfolio")
    portfolio_id = optional_text(portfolio_cfg.get("portfolio_id")) or default_portfolio_id
    strategy.configure_portfolio_inventory_feed(
        redis_client=build_redis_client(redis_cfg),
        portfolio_id=portfolio_id,
        namespace=namespace,
        schema_version=schema_version,
        stale_after_ms=int(portfolio_cfg.get("inventory_stale_after_ms", 3_000)),
    )


def resolve_flux_strategy_id(config: dict[str, Any]) -> str:
    identity = table(config, "identity")
    return optional_text(identity.get("strategy_id")) or "makerv3"


@contextmanager
def strategy_startup_lock(
    config: dict[str, Any],
    *,
    descriptor: StrategySetDescriptor,
    repo_root: Path,
    lock_dir: Path | None = None,
):
    strategy_id = resolve_flux_strategy_id(config)
    root = lock_dir or (repo_root / ".run" / descriptor.lock_dir_name)
    root.mkdir(parents=True, exist_ok=True)
    lock_path = root / f"{strategy_id}.lock"
    lock_handle = lock_path.open("a+", encoding="utf-8")
    try:
        try:
            fcntl.flock(lock_handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            lock_handle.seek(0)
            owner = lock_handle.read().strip()
            detail = f" ({owner})" if owner else ""
            raise RuntimeError(
                f"{descriptor.profile.title()} strategy `{strategy_id}` is already running{detail}",
            ) from exc

        lock_handle.seek(0)
        lock_handle.truncate()
        lock_handle.write(f"pid={os.getpid()}\n")
        lock_handle.flush()
        yield
    finally:
        try:
            fcntl.flock(lock_handle.fileno(), fcntl.LOCK_UN)
        finally:
            lock_handle.close()
