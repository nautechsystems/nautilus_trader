from __future__ import annotations

from typing import Any

from flux.runners.shared.redis_env import apply_redis_env_overrides as apply_shared_redis_env_overrides


def apply_redis_env_overrides(config: dict[str, Any]) -> dict[str, Any]:
    return apply_shared_redis_env_overrides(config, env_prefix="TOKENMM")
