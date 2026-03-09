from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class LpHedgerMeta:
    id: str
    job_id: str
    state_key: str
    snapshot_key: str
    events_key: str
    mode_key: str
    geometry_overrides_key: str
    threshold_overrides_key: str
    config_env_var: str
    config_default_path: str
    default_enabled: bool
    public_visible: bool

    @property
    def hedger_id(self) -> str:
        return self.id

    @property
    def state_redis_key(self) -> str:
        return f"{self.state_key}:state"

    @property
    def enabled(self) -> bool:
        return self.default_enabled

    @property
    def staged(self) -> bool:
        return self.public_visible and not self.default_enabled


__all__ = ["LpHedgerMeta"]
