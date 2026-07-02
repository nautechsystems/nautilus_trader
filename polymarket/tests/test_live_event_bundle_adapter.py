from __future__ import annotations

from pathlib import Path

import pytest

from polymarket.adapters.live_event_bundle_v1 import LiveEventBundleV1Adapter


def test_live_event_bundle_requires_explicit_raw_path_when_multiple_ndjson_files(tmp_path: Path) -> None:
    bundle = tmp_path / "bundle"
    raw_dir = bundle / "raw"
    raw_dir.mkdir(parents=True)
    (bundle / "a.ndjson").write_text("{}\n", encoding="utf-8")
    (raw_dir / "b.ndjson").write_text("{}\n", encoding="utf-8")

    adapter = LiveEventBundleV1Adapter(repo_root=Path.cwd())
    with pytest.raises(ValueError, match="multiple NDJSON candidates"):
        adapter.load({"input": {"bundle_dir": str(bundle)}})
