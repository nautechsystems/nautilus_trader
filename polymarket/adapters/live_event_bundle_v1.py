"""Future data-team live event-bundle adapter boundary."""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
from typing import Any

from polymarket.adapters.live_ws_v1 import LiveWsV1Adapter
from polymarket.models import DatasetMetadataV1, PolymarketL2DatasetV1


class LiveEventBundleV1Adapter:
    adapter_name = "live_event_bundle_v1"
    adapter_version = "v1"

    def __init__(self, *, repo_root: Path | None = None) -> None:
        self.repo_root = repo_root or Path.cwd()

    def load(self, config: Mapping[str, Any]) -> PolymarketL2DatasetV1:
        input_config = dict(config.get("input", config))
        bundle_dir = Path(str(input_config.get("event_dir") or input_config.get("bundle_dir") or input_config.get("path")))
        if not bundle_dir.is_absolute():
            bundle_dir = self.repo_root / bundle_dir
        raw_path = input_config.get("raw_ws_path") or input_config.get("ndjson_path")
        if raw_path is None:
            candidates = sorted(bundle_dir.glob("*.ndjson")) + sorted((bundle_dir / "raw").glob("*.ndjson"))
            if not candidates:
                raise NotImplementedError(
                    "live_event_bundle_v1 is a schema boundary stub until data-team bundle schema stabilizes; "
                    "provide raw_ws_path/ndjson_path or include one NDJSON file in the bundle.",
                )
            if len(candidates) > 1:
                raise ValueError(
                    "live_event_bundle_v1 found multiple NDJSON candidates; provide raw_ws_path/ndjson_path "
                    "explicitly until the data-team bundle schema stabilizes.",
                )
            raw_path = candidates[0]
        live_config = dict(input_config)
        live_config["ndjson_path"] = raw_path
        dataset = LiveWsV1Adapter(repo_root=self.repo_root).load({"input": live_config})
        metadata = DatasetMetadataV1(
            dataset_id=dataset.metadata.dataset_id,
            adapter_name=self.adapter_name,
            adapter_version=self.adapter_version,
            source_type="live_event_bundle",
            source_files=dataset.metadata.source_files,
            assumptions=dataset.metadata.assumptions
            + ("live_event_bundle_v1 reused live_ws_v1 raw message parsing.",),
            warnings=dataset.metadata.warnings
            + ("live_event_bundle_v1 schema is provisional until data-team event bundle format stabilizes.",),
        )
        return PolymarketL2DatasetV1(metadata=metadata, steps=dataset.steps)
