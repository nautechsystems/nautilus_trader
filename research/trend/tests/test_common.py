"""
Seal and trial-registry checks for the shared trend tools.

Run from research/:
    ../python/.venv/Scripts/python -m pytest trend/tests -q

"""

from __future__ import annotations

import polars as pl

from trend import common


def test_minute_loader_never_opens_sealed_files(monkeypatch):
    opened = []
    real_read = pl.read_parquet

    def spy(source, *args, **kwargs):
        paths = source if isinstance(source, list) else [source]
        opened.extend(str(p) for p in paths)
        return real_read(source, *args, **kwargs)

    monkeypatch.setattr(pl, "read_parquet", spy)
    df = common.load_minutes("BTCUSDT")

    def month(path: str) -> tuple[int, int]:
        m = common._MONTH_RE.search(path)
        return int(m[1]), int(m[2])

    assert opened
    assert all(month(p) < common.SEAL_MONTH for p in opened)
    assert df["close_time"].max() < common.SEAL_START_MS


def test_registry_updates_a_rerun_in_place(tmp_path, monkeypatch):
    monkeypatch.setattr(common, "REGISTRY_PATH", tmp_path / "registry.csv")
    assert common.load_registry().height == 0
    common.register("a", 0.1)
    common.register("b", 0.2)
    common.register("a", 0.3)
    reg = common.load_registry().sort("name")
    assert reg["name"].to_list() == ["a", "b"]
    assert reg["sharpe_daily"].to_list() == [0.3, 0.2]
