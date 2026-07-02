"""PMXT curated-event adapter for Polymarket v1."""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
from typing import Any

from polymarket.adapters.pmxt_parquet_v1 import PMXTParquetV1Adapter
from polymarket.models import DatasetMetadataV1, PolymarketL2DatasetV1


class PMXTEventV1Adapter:
    adapter_name = "pmxt_event_v1"
    adapter_version = "v1"

    def __init__(self, *, repo_root: Path | None = None) -> None:
        self.repo_root = repo_root or Path.cwd()

    def load(self, config: Mapping[str, Any]) -> PolymarketL2DatasetV1:
        input_config = dict(config.get("input", config))
        event_dir = Path(str(input_config.get("event_dir") or input_config.get("path")))
        if not event_dir.is_absolute():
            event_dir = self.repo_root / event_dir
        parquet_path = event_dir / str(input_config.get("orderbook_filename", "orderbook.parquet"))
        parquet_config = dict(input_config)
        parquet_config["parquet_path"] = parquet_path
        dataset = PMXTParquetV1Adapter(repo_root=self.repo_root).load({"input": parquet_config})
        metadata = DatasetMetadataV1(
            dataset_id=str(input_config.get("dataset_id") or event_dir.name),
            adapter_name=self.adapter_name,
            adapter_version=self.adapter_version,
            source_type="pmxt_event",
            source_files=dataset.metadata.source_files,
            assumptions=dataset.metadata.assumptions
            + ("PMXT event folders are curated from PMXT-derived data.",),
            warnings=dataset.metadata.warnings
            + ("PMXT event adapter inherits PMXT parquet data-quality concerns.",),
        )
        return PolymarketL2DatasetV1(metadata=metadata, steps=dataset.steps)

