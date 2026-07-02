"""Adapter protocol for Polymarket v1 datasets."""

from __future__ import annotations

from typing import Any, Mapping, Protocol

from polymarket.models import PolymarketL2DatasetV1


class DatasetAdapterV1(Protocol):
    """Translate one source format into the canonical L2 dataset."""

    adapter_name: str
    adapter_version: str

    def load(self, config: Mapping[str, Any]) -> PolymarketL2DatasetV1:
        """Load source-specific data into canonical replay steps."""
        ...

