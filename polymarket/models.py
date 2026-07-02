"""Canonical Polymarket L2 replay data model for research backtests.

Adapters translate source-specific formats (PMXT parquet, PMXT curated events,
local raw WebSocket captures, or future event bundles) into these types.  The
backtest loop consumes only these canonical replay steps and must not branch on
adapter-specific source quirks.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal
from typing import Literal


EventTypeV1 = Literal["book", "price_change", "trade", "tick_size_change"]
BookSideV1 = Literal["BUY", "SELL"]


@dataclass(frozen=True, slots=True)
class LevelV1:
    """One price level in canonical L2 book form."""

    price: Decimal
    size: Decimal


@dataclass(frozen=True, slots=True)
class L2UpdateV1:
    """One canonical update inside an atomic replay step."""

    event_type: EventTypeV1
    market: str
    asset_id: str
    side: BookSideV1 | None = None
    price: Decimal | None = None
    size: Decimal | None = None
    bids: tuple[LevelV1, ...] = ()
    asks: tuple[LevelV1, ...] = ()
    best_bid: Decimal | None = None
    best_ask: Decimal | None = None
    old_tick_size: Decimal | None = None
    new_tick_size: Decimal | None = None


@dataclass(frozen=True, slots=True)
class L2ReplayStepV1:
    """Atomic unit consumed by the v1 replay loop."""

    sequence: int
    timestamp_received: datetime
    timestamp: datetime | None
    updates: tuple[L2UpdateV1, ...]


@dataclass(frozen=True, slots=True)
class DatasetMetadataV1:
    """Source metadata for reporting and audit only.

    Backtest behavior must not branch on these fields after adapter loading.
    """

    dataset_id: str
    adapter_name: str
    adapter_version: str
    source_type: str
    source_files: tuple[str, ...] = ()
    assumptions: tuple[str, ...] = ()
    warnings: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class PolymarketL2DatasetV1:
    """Canonical dataset produced by all Polymarket v1 adapters."""

    metadata: DatasetMetadataV1
    steps: tuple[L2ReplayStepV1, ...]

    def __post_init__(self) -> None:
        if not self.steps:
            raise ValueError("PolymarketL2DatasetV1 requires at least one replay step")
