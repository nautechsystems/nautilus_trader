from __future__ import annotations

import os
from typing import Any


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _env_bool(name: str) -> bool | None:
    value = _optional_text(os.getenv(name))
    if value is None:
        return None
    lowered = value.lower()
    if lowered in {"1", "true", "yes", "on"}:
        return True
    if lowered in {"0", "false", "no", "off"}:
        return False
    raise ValueError(f"`{name}` must be a boolean-like value")


def apply_redis_env_overrides(config: dict[str, Any]) -> dict[str, Any]:
    merged = dict(config)
    base_redis = merged.get("redis", {})
    if base_redis and not isinstance(base_redis, dict):
        raise ValueError("[redis] must be a TOML table")

    redis_cfg = dict(base_redis) if isinstance(base_redis, dict) else {}
    has_override = False

    text_overrides = {
        "host": _optional_text(os.getenv("TOKENMM_REDIS_HOST")),
        "username": _optional_text(os.getenv("TOKENMM_REDIS_USERNAME")),
        "password": _optional_text(os.getenv("TOKENMM_REDIS_PASSWORD")),
    }
    for field, value in text_overrides.items():
        if value is not None:
            redis_cfg[field] = value
            has_override = True

    port = _optional_text(os.getenv("TOKENMM_REDIS_PORT"))
    if port is not None:
        redis_cfg["port"] = int(port)
        has_override = True

    db = _optional_text(os.getenv("TOKENMM_REDIS_DB"))
    if db is not None:
        redis_cfg["db"] = int(db)
        has_override = True

    ssl = _env_bool("TOKENMM_REDIS_SSL")
    if ssl is not None:
        redis_cfg["ssl"] = ssl
        has_override = True

    if isinstance(base_redis, dict) and base_redis:
        merged["redis"] = redis_cfg
    elif has_override:
        merged["redis"] = redis_cfg
    return merged
