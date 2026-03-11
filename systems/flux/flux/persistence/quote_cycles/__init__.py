"""
MakerV3 quote-cycle persistence support.
"""

from __future__ import annotations

from typing import Any


__all__ = (
    "QuoteCyclePersistenceActor",
    "QuoteCyclePersistenceActorConfig",
)


def __getattr__(name: str) -> Any:
    if name in {"QuoteCyclePersistenceActor", "QuoteCyclePersistenceActorConfig"}:
        from flux.persistence.quote_cycles.actor import QuoteCyclePersistenceActor
        from flux.persistence.quote_cycles.config import QuoteCyclePersistenceActorConfig

        return {
            "QuoteCyclePersistenceActor": QuoteCyclePersistenceActor,
            "QuoteCyclePersistenceActorConfig": QuoteCyclePersistenceActorConfig,
        }[name]
    raise AttributeError(name)
