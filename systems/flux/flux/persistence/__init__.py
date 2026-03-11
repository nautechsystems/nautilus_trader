"""
Flux-specific persistence surfaces.
"""

from __future__ import annotations

from typing import Any


__all__ = (
    "ExecutionMarkoutPersistenceActor",
    "ExecutionMarkoutPersistenceActorConfig",
    "FluxBalanceSnapshotPersistenceActor",
    "FluxBalanceSnapshotPersistenceActorConfig",
    "PortfolioInventorySnapshotWriter",
    "QuoteCyclePersistenceActor",
    "QuoteCyclePersistenceActorConfig",
)


def __getattr__(name: str) -> Any:
    if name in {
        "ExecutionMarkoutPersistenceActor",
        "ExecutionMarkoutPersistenceActorConfig",
    }:
        from flux.persistence.markouts import (
            ExecutionMarkoutPersistenceActor,
            ExecutionMarkoutPersistenceActorConfig,
        )

        return {
            "ExecutionMarkoutPersistenceActor": ExecutionMarkoutPersistenceActor,
            "ExecutionMarkoutPersistenceActorConfig": ExecutionMarkoutPersistenceActorConfig,
        }[name]
    if name in {
        "FluxBalanceSnapshotPersistenceActor",
        "FluxBalanceSnapshotPersistenceActorConfig",
    }:
        from flux.persistence.balance_snapshots import (
            FluxBalanceSnapshotPersistenceActor,
            FluxBalanceSnapshotPersistenceActorConfig,
        )

        return {
            "FluxBalanceSnapshotPersistenceActor": FluxBalanceSnapshotPersistenceActor,
            "FluxBalanceSnapshotPersistenceActorConfig": FluxBalanceSnapshotPersistenceActorConfig,
        }[name]
    if name == "PortfolioInventorySnapshotWriter":
        from flux.persistence.portfolio_inventory_snapshots import PortfolioInventorySnapshotWriter

        return PortfolioInventorySnapshotWriter
    if name in {
        "QuoteCyclePersistenceActor",
        "QuoteCyclePersistenceActorConfig",
    }:
        from flux.persistence.quote_cycles import QuoteCyclePersistenceActor, QuoteCyclePersistenceActorConfig

        return {
            "QuoteCyclePersistenceActor": QuoteCyclePersistenceActor,
            "QuoteCyclePersistenceActorConfig": QuoteCyclePersistenceActorConfig,
        }[name]
    raise AttributeError(name)
