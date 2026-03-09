from __future__ import annotations

from typing import Any


__all__ = (
    "FluxBalanceSnapshotPersistenceActor",
    "FluxBalanceSnapshotPersistenceActorConfig",
)


def __getattr__(name: str) -> Any:
    if name in {
        "FluxBalanceSnapshotPersistenceActor",
        "FluxBalanceSnapshotPersistenceActorConfig",
    }:
        from flux.persistence.balance_snapshots.actor import FluxBalanceSnapshotPersistenceActor
        from flux.persistence.balance_snapshots.config import FluxBalanceSnapshotPersistenceActorConfig

        return {
            "FluxBalanceSnapshotPersistenceActor": FluxBalanceSnapshotPersistenceActor,
            "FluxBalanceSnapshotPersistenceActorConfig": FluxBalanceSnapshotPersistenceActorConfig,
        }[name]
    raise AttributeError(name)
