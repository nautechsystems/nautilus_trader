from __future__ import annotations

import importlib.util
import sys
from pathlib import Path
from types import ModuleType


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_module() -> ModuleType:
    module_path = _repo_root() / "ops/scripts/tokenmm_risk_audit.py"
    spec = importlib.util.spec_from_file_location("tokenmm_risk_audit", module_path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_extract_signal_inventory_reads_aggregation_mode_from_nested_skew() -> None:
    module = _load_module()

    row = module._extract_signal_inventory(
        {
            "id": "plumeusdt_bitget_perp_makerv3",
            "meta": {"base_asset": "PLUME"},
            "state": {
                "pricing_debug": {
                    "skew": {
                        "global_inventory_aggregation_mode": "partial",
                    },
                },
            },
        },
    )

    assert row.aggregation_mode == "partial"


def test_strategy_local_qty_from_rows_prefers_latest_base_row_for_spot_component() -> None:
    module = _load_module()

    qty, source = module._strategy_local_qty_from_rows(
        rows=[
            {
                "kind": "position",
                "signed_qty_base": "-500",
                "row_id": "plumeusdt_bitget_spot_makerv3:pos:0",
                "ts_ms": 1_700_000_000_000,
            },
            {
                "kind": "cash",
                "asset": "PLUME",
                "total": "1295.24669092",
                "row_id": "plumeusdt_bitget_spot_makerv3:evt:0:0",
                "ts_ms": 1_700_000_000_001,
            },
            {
                "kind": "cash",
                "asset": "PLUME",
                "total": "1045.24669092",
                "row_id": "plumeusdt_bitget_spot_makerv3:evt:1:0",
                "ts_ms": 1_700_000_000_002,
            },
        ],
        base_asset="PLUME",
        expected_local_qty=None,
        component_local_position_qty=None,
        component_local_spot_qty=module.Decimal("1045.24669092"),
    )

    assert qty == module.Decimal("1045.24669092")
    assert source == "latest_base_asset_row"
