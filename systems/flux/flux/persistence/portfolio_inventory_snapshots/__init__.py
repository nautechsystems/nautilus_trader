from __future__ import annotations

from typing import Any


__all__ = ("PortfolioInventorySnapshotWriter",)


def __getattr__(name: str) -> Any:
    if name == "PortfolioInventorySnapshotWriter":
        from flux.persistence.portfolio_inventory_snapshots.sqlite import PortfolioInventorySnapshotWriter

        return PortfolioInventorySnapshotWriter
    raise AttributeError(name)
