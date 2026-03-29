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


def test_latest_base_asset_qty_from_rows_prefers_higher_numeric_event_id_when_timestamps_tie() -> None:
    module = _load_module()

    qty, source = module._latest_base_asset_qty_from_rows(
        rows=[
            {
                "kind": "cash",
                "asset": "PLUME",
                "total": "-3447.93729095",
                "row_id": "plumeusdt_bitget_spot_makerv3:evt:9:0",
                "ts_ms": 1_700_000_000_002,
            },
            {
                "kind": "cash",
                "asset": "PLUME",
                "total": "-3447.95091031",
                "row_id": "plumeusdt_bitget_spot_makerv3:evt:136:0",
                "ts_ms": 1_700_000_000_002,
            },
        ],
        base_asset="PLUME",
    )

    assert qty == module.Decimal("-3447.95091031")
    assert source == "latest_base_asset_row"


def test_strategy_local_qty_from_rows_falls_back_to_component_snapshot_when_rows_missing() -> None:
    module = _load_module()

    qty, source = module._strategy_local_qty_from_rows(
        rows=[],
        base_asset="PLUME",
        expected_local_qty=None,
        component_local_position_qty=module.Decimal("-250030"),
        component_local_spot_qty=None,
    )

    assert qty == module.Decimal("-250030")
    assert source == "component_local_position_qty"


def test_component_is_missing_optional_only_for_nonrequired_missing_components() -> None:
    module = _load_module()

    assert module._component_is_missing_optional({"missing": True, "required": False}) is True
    assert module._component_is_missing_optional({"missing": True, "required": True}) is False
    assert module._component_is_missing_optional({"missing": False, "required": False}) is False


def test_main_fails_closed_when_profile_readiness_is_unhealthy(
    capsys,
    tmp_path: Path,
) -> None:
    module = _load_module()
    strategy_id = "plumeusdt_bybit_perp_makerv3"

    def _fake_fetch_enveloped_data(*, base_url: str, path: str, timeout: float):
        assert base_url == "http://127.0.0.1:5022"
        assert timeout == 5.0
        if path == module.PROFILE_READINESS_PATH:
            return {
                "ok": False,
                "summary": {
                    "failed_checks": ["state_stream_freshness"],
                    "stale_state_stream_strategy_ids": [strategy_id],
                },
            }
        if path == module.PROFILE_SIGNALS_PATH:
            return {
                "strategies": [
                    {
                        "id": strategy_id,
                        "meta": {"base_asset": "PLUME"},
                        "state": {
                            "state": "bot_off",
                            "local_qty_base": "0",
                            "global_qty_base": "0",
                            "global_qty_base_complete": True,
                            "aggregation_mode": "complete",
                        },
                    },
                ],
            }
        if path == module.PROFILE_BALANCES_PATH:
            return {
                "source": "portfolio_snapshot",
                "global_qty_base": "0",
                "global_qty_base_complete": True,
                "aggregation_mode": "complete",
                "components": [
                    {
                        "strategy_id": strategy_id,
                        "local_qty_base": "0",
                        "local_position_qty_base": "0",
                    },
                ],
            }
        if path == module._strategy_balances_path(strategy_id):
            return {"rows": []}
        raise AssertionError(f"unexpected path: {path}")

    module._fetch_enveloped_data = _fake_fetch_enveloped_data
    module._fetch_json = lambda **_: {
        "jobs": [
            {
                "id": f"tokenmm-node-{strategy_id}",
                "group_key": "tokenmm",
                "status": "inactive",
            },
        ],
    }

    exit_code = module.main(["--config", str(tmp_path / "missing.toml")])

    assert exit_code == 1
    captured = capsys.readouterr()
    assert "TOKENMM RISK AUDIT FAILED" in captured.err
    assert "readiness" in captured.err.lower()
    assert "state_stream_freshness" in captured.err


def test_main_success_banner_includes_readiness_freshness_summary(
    capsys,
    tmp_path: Path,
) -> None:
    module = _load_module()
    strategy_id = "plumeusdt_bybit_perp_makerv3"

    def _fake_fetch_enveloped_data(*, base_url: str, path: str, timeout: float):
        assert base_url == "http://127.0.0.1:5022"
        assert timeout == 5.0
        if path == module.PROFILE_READINESS_PATH:
            return {
                "ok": True,
                "summary": {
                    "ready_strategy_count": 1,
                    "required_strategy_count": 1,
                    "state_stream_max_age_ms": 30_000,
                    "failed_checks": [],
                },
            }
        if path == module.PROFILE_SIGNALS_PATH:
            return {
                "strategies": [
                    {
                        "id": strategy_id,
                        "meta": {"base_asset": "PLUME"},
                        "state": {
                            "state": "bot_off",
                            "local_qty_base": "5",
                            "global_qty_base": "5",
                            "global_qty_base_complete": True,
                            "aggregation_mode": "complete",
                        },
                    },
                ],
            }
        if path == module.PROFILE_BALANCES_PATH:
            return {
                "source": "portfolio_snapshot",
                "global_qty_base": "5",
                "global_qty_base_complete": True,
                "aggregation_mode": "complete",
                "components": [
                    {
                        "strategy_id": strategy_id,
                        "local_qty_base": "5",
                        "local_position_qty_base": "5",
                    },
                ],
            }
        if path == module._strategy_balances_path(strategy_id):
            return {
                "rows": [
                    {
                        "kind": "position",
                        "signed_qty_base": "5",
                        "row_id": f"{strategy_id}:pos:0",
                        "ts_ms": 1_700_000_000_000,
                    },
                ],
            }
        raise AssertionError(f"unexpected path: {path}")

    module._fetch_enveloped_data = _fake_fetch_enveloped_data
    module._fetch_json = lambda **_: {
        "jobs": [
            {
                "id": f"tokenmm-node-{strategy_id}",
                "group_key": "tokenmm",
                "status": "active",
            },
        ],
    }

    exit_code = module.main(
        [
            "--config",
            str(tmp_path / "missing.toml"),
            "--strategy-id",
            strategy_id,
        ],
    )

    assert exit_code == 0
    captured = capsys.readouterr()
    assert "TOKENMM RISK AUDIT PASSED" in captured.out
    assert "readiness=1/1" in captured.out
    assert "state_stream_max_age_ms=30000" in captured.out
