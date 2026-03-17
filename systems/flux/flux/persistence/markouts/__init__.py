"""
MakerV3 execution markout persistence support.
"""

from __future__ import annotations

from typing import Any


__all__ = (
    "ExecutionMarkoutPersistenceActor",
    "ExecutionMarkoutPersistenceActorConfig",
)


def __getattr__(name: str) -> Any:
    if name in {"ExecutionMarkoutPersistenceActor", "ExecutionMarkoutPersistenceActorConfig"}:
        from flux.persistence.markouts.actor import ExecutionMarkoutPersistenceActor
        from flux.persistence.markouts.config import ExecutionMarkoutPersistenceActorConfig

        return {
            "ExecutionMarkoutPersistenceActor": ExecutionMarkoutPersistenceActor,
            "ExecutionMarkoutPersistenceActorConfig": ExecutionMarkoutPersistenceActorConfig,
        }[name]
    raise AttributeError(name)
